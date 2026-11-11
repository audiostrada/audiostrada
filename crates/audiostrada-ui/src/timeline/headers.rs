//! Track headers: name, buttons, meters, lanes, and the edge that resizes them.

use super::*;

impl TimelineView {
    /// What a lane shows for its target: name, range, how values read, and the value set on
    /// the control itself, which the lane moves away from.
    pub(super) fn target_info(
        &mut self,
        doc: &mut Document,
        track: &Track,
        target: &AutomationTarget,
    ) -> LaneInfo {
        let simple = |name: String, min: f32, max: f32, reading: Reading, base: f32| LaneInfo {
            name,
            min,
            max,
            reading,
            log: false,
            base: Some(base),
            base_text: None,
        };
        match target {
            AutomationTarget::Gain => {
                simple(tr("Volume").into(), -60.0, 12.0, Reading::Db, track.gain_db)
            }
            AutomationTarget::Pan => simple(tr("Pan").into(), -1.0, 1.0, Reading::Pan, track.pan),
            AutomationTarget::Mute => simple(
                tr("Mute").into(),
                0.0,
                1.0,
                Reading::Toggle,
                if track.mute { 1.0 } else { 0.0 },
            ),
            AutomationTarget::Send(to) => {
                let name = doc.project().track(*to).map_or("?", |t| t.name.as_str());
                let base = track
                    .sends
                    .iter()
                    .find(|s| s.target == *to)
                    .map_or(0.0, |s| s.gain_db);
                simple(
                    trf!("Send → {name}", name = name),
                    -60.0,
                    12.0,
                    Reading::Db,
                    base,
                )
            }
            AutomationTarget::Param { .. } => self.param_info(
                doc,
                (track.id, Channel::Track(track.id)),
                &track.inserts,
                target,
            ),
        }
    }

    /// What a lane of an effect's parameter shows, the effect being one of `inserts` (a
    /// track's or a clip's) on `channel`; plugin parameters are cached under `owner`.
    pub(super) fn param_info(
        &mut self,
        doc: &mut Document,
        (owner, channel): (TrackId, Channel),
        inserts: &[audiostrada_core::Insert],
        target: &AutomationTarget,
    ) -> LaneInfo {
        let unknown = LaneInfo {
            name: "?".into(),
            min: 0.0,
            max: 1.0,
            reading: Reading::Share,
            log: false,
            base: None,
            base_text: None,
        };
        let AutomationTarget::Param { insert, param } = target else {
            return unknown;
        };
        let found = audiostrada_core::routing::flatten(inserts)
            .into_iter()
            .find(|i| i.id == *insert)
            .cloned();
        let Some(ins) = found else {
            return unknown;
        };
        if let Some(info) = audiostrada_dsp::info(&ins.effect) {
            return info
                .params
                .iter()
                .find(|p| p.id == param)
                .map_or(unknown, |p| LaneInfo {
                    name: format!("{}: {}", tr(info.name), tr(p.name)),
                    min: p.min,
                    max: p.max,
                    reading: Reading::Param(p),
                    log: p.min > 0.0
                        && p.max / p.min >= 20.0
                        && matches!(p.unit, "Hz" | "s" | "ms"),
                    base: Some(ins.params.get(param).copied().unwrap_or(p.default)),
                    base_text: None,
                });
        }
        let key = (owner, target.clone());
        let mut info = match self.target_info.get(&key) {
            Some(cached) => cached.clone(),
            None => {
                let name = doc
                    .player()
                    .plugins()
                    .find(&ins.effect)
                    .map_or_else(|| ins.effect.clone(), |p| p.name.clone());
                let info = param
                    .parse::<u32>()
                    .ok()
                    .and_then(|id| {
                        let plugin = doc.player_mut().plugin(channel, *insert)?;
                        plugin.params().into_iter().find(|p| p.id == id)
                    })
                    .map_or(
                        LaneInfo {
                            name: format!("{name}: {param}"),
                            ..unknown.clone()
                        },
                        |p| LaneInfo {
                            name: format!("{name}: {}", p.name),
                            min: p.min as f32,
                            max: p.max as f32,
                            ..unknown.clone()
                        },
                    );
                self.target_info.insert(key, info.clone());
                info
            }
        };
        // The plugin's current value, in its own words when it has them.
        if let Ok(id) = param.parse::<u32>()
            && let Some(plugin) = doc.player_mut().plugin(channel, *insert)
            && let Some(value) = plugin.param_value(id)
        {
            info.base = Some(value as f32);
            info.base_text = plugin.value_text(id, value);
        }
        info
    }

    pub(super) fn draw_headers(
        &mut self,
        ui: &mut Ui,
        header: Rect,
        doc: &mut Document,
        project: &Project,
        visuals: &egui::Visuals,
    ) {
        let painter = ui.painter().with_clip_rect(header);
        painter.rect_filled(header, 0.0, visuals.panel_fill);
        let rows = self.rows.clone();
        for (i, &(index, depth)) in rows.iter().enumerate() {
            let track = &project.tracks[index];
            let top = header.top() + self.tops[i] - self.scroll_y;
            let row = Rect::from_min_max(
                pos2(header.left(), top),
                pos2(header.right(), top + self.heights[i]),
            );
            if row.bottom() < header.top() || row.top() > header.bottom() {
                continue;
            }
            // The whole row selects, drags and opens the menu; its buttons come above it.
            let response = ui.interact(
                row,
                ui.scope_id().with(("header", track.id.0)),
                Sense::click_and_drag(),
            );
            self.height_edge(ui, doc, row, track.id);
            let color = crate::theme::track_color(track.color);
            // Lit from above; the track's color down the left, washing across when selected.
            let lift = if visuals.dark_mode {
                Color32::WHITE
            } else {
                Color32::BLACK
            };
            let fill = super::draw::mix(visuals.panel_fill, lift, 0.035);
            crate::theme::gradient_rect(&painter, row, visuals.panel_fill, fill);
            if doc.is_track_selected(track.id) {
                let strength = if visuals.dark_mode { 0.34 } else { 0.26 };
                let mut wash = egui::Mesh::default();
                let near = color.gamma_multiply(strength);
                let far = color.gamma_multiply(strength * 0.15);
                wash.colored_vertex(row.left_top(), near);
                wash.colored_vertex(row.right_top(), far);
                wash.colored_vertex(row.right_bottom(), far);
                wash.colored_vertex(row.left_bottom(), near);
                wash.add_triangle(0, 1, 2);
                wash.add_triangle(0, 2, 3);
                painter.add(wash);
            }
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(row.left() + 1.0, row.top() + 3.0),
                    pos2(row.left() + 5.0, row.bottom() - 3.0),
                ),
                2.0,
                color,
            );
            // An armed track shows it across its header.
            if track.armed {
                painter.rect_filled(
                    row.shrink(1.0),
                    2.0,
                    crate::theme::RECORD.gamma_multiply(0.12),
                );
                painter.rect_stroke(
                    row.shrink(1.0),
                    2.0,
                    Stroke::new(1.0, crate::theme::RECORD.gamma_multiply(0.6)),
                    StrokeKind::Inside,
                );
            }
            painter.hline(
                row.x_range(),
                row.bottom() - 0.5,
                Stroke::new(
                    1.0,
                    Color32::from_black_alpha(if visuals.dark_mode { 120 } else { 40 }),
                ),
            );
            // Low rows hold the name and the switches on one line.
            let compact = row.height() < 52.0;
            let id = track.id;
            let vca = track.kind == TrackKind::Vca;
            // The track's meter at the right, the rest of the header before it.
            let right = if vca {
                row.right()
            } else {
                let meter = Rect::from_min_max(
                    pos2(row.right() - HEADER_METER + 2.0, row.top() + 4.0),
                    pos2(row.right() - 3.0, row.bottom() - 4.0),
                );
                self.header_meter(ui, doc, id, meter);
                row.right() - HEADER_METER
            };
            let mut x = row.left() + 12.0 + depth as f32 * 12.0;
            let line_top = if compact {
                row.center().y - 9.0
            } else {
                row.top() + 7.0 + ((row.height() - 56.0) / 2.0).clamp(0.0, 8.0)
            };
            if routing::has_children(project, id) {
                let arrow = Rect::from_min_size(pos2(x - 2.0, line_top), vec2(16.0, 18.0));
                let text = if track.collapsed { "▸" } else { "▾" };
                if ui
                    .put(arrow, egui::Button::new(text).frame(false))
                    .on_hover_text(tr("Show or hide the tracks of this bus"))
                    .clicked()
                {
                    let collapsed = !track.collapsed;
                    doc.history_mut().modify_unrecorded(|p| {
                        if let Some(t) = p.track_mut(id) {
                            t.collapsed = collapsed;
                        }
                    });
                }
                x += 16.0;
            }
            // Two lines: the name with Mute, Solo and Record at its right, always in the same
            // column; under it what the track listens to, and its automation. A low row keeps
            // the first line only.
            let pad = ((row.height() - 56.0) / 2.0).clamp(0.0, 8.0);
            let line1 = if compact {
                row.center().y
            } else {
                row.top() + 16.0 + pad
            };
            let line2 = line1 + 24.0;
            let cell = vec2(22.0, 18.0);
            // The three switches from the right edge: M, S, R.
            let column = |k: f32, y: f32| {
                Rect::from_center_size(pos2(right - 17.0 - (2.0 - k) * 24.0, y), cell)
            };
            let switches_left = column(0.0, line1).left();
            // The instrument, in the track's color; a click picks another.
            let icon_rect = Rect::from_center_size(pos2(x + 8.0, line1), vec2(18.0, 18.0));
            let icon = crate::track_icons::of(project, track);
            let icon_response = ui.interact(
                icon_rect,
                ui.scope_id().with(("icon", id.0)),
                Sense::click(),
            );
            let tint = crate::theme::track_color(track.color);
            if icon_response.hovered() {
                painter.rect_filled(
                    icon_rect.expand(2.0),
                    4.0,
                    ui.visuals().widgets.hovered.bg_fill,
                );
            }
            crate::track_icons::paint(ui, icon_rect.shrink(1.0), icon, tint);
            let icon_response = icon_response.on_hover_text(crate::trf!(
                "{name}. Click to choose the icon",
                name = icon.label()
            ));
            egui::Popup::menu(&icon_response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    if crate::track_icons::picker(ui, doc, id) {
                        ui.close();
                    }
                });
            x += 24.0;
            let group = audiostrada_core::groups::group_of(project, id).map(|g| &project.groups[g]);
            // Up to the switches (and the group tag), cut with "…" beyond.
            let max_width =
                (switches_left - 8.0 - x - if group.is_some() { 20.0 } else { 0.0 }).max(20.0);
            let mut job = egui::text::LayoutJob::simple_singleline(
                track.name.clone(),
                crate::theme::strong(12.5),
                visuals.strong_text_color(),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let elided = galley.elided;
            let name = Rect::from_min_size(pos2(x, line1 - galley.size().y / 2.0), galley.size());
            // A double-click anywhere in the name's room renames, short names included.
            let name_room =
                Rect::from_min_size(pos2(x - 4.0, line1 - 13.0), vec2(max_width + 8.0, 26.0));
            let renaming = self.renaming.as_ref().is_some_and(|(t, _)| *t == id);
            if !renaming {
                painter.galley(name.min, galley, visuals.strong_text_color());
            }
            if let Some(g) = group {
                let c = crate::theme::track_color(g.color);
                let c = if g.active { c } else { c.gamma_multiply(0.4) };
                let tag =
                    Rect::from_center_size(pos2(name.right() + 13.0, line1), vec2(14.0, 13.0));
                painter.rect_filled(tag, 3.0, c);
                painter.text(
                    tag.center(),
                    Align2::CENTER_CENTER,
                    g.name.chars().next().unwrap_or('G').to_string(),
                    FontId::proportional(9.5),
                    Color32::BLACK,
                );
            }
            let plays = track.kind == TrackKind::Audio;
            if line1 - 9.0 > header.top() {
                // Mute and Solo.
                for (k, text, on, on_color, link, label) in [
                    (
                        0.0,
                        "M",
                        track.mute,
                        crate::theme::MUTE,
                        audiostrada_core::groups::Link::Mute,
                        tr("Mute"),
                    ),
                    (
                        1.0,
                        "S",
                        track.solo,
                        crate::theme::SOLO,
                        audiostrada_core::groups::Link::Solo,
                        tr("Solo"),
                    ),
                ] {
                    let rect = column(k, line1);
                    let response =
                        ui.interact(rect, ui.scope_id().with((text, id.0)), Sense::click());
                    crate::theme::flat_toggle(ui, rect, &response, text, on, on_color);
                    if response.on_hover_text(label).clicked() {
                        let alone = crate::mixer::clutch(ui);
                        doc.history_mut().edit(label, |p| {
                            audiostrada_core::groups::maybe_alone(p, alone, |p| {
                                audiostrada_core::groups::set_switch(p, id, link, !on)
                            })
                        });
                    }
                }
                // A bus records nothing: in that place, its freeze, which renders it and its
                // tracks to play that instead.
                if track.kind == TrackKind::Bus {
                    let rect = column(2.0, line1);
                    let response =
                        ui.interact(rect, ui.scope_id().with(("freeze", id.0)), Sense::click());
                    let frozen = track.frozen.is_some();
                    let ice = Color32::from_rgb(120, 196, 255);
                    crate::theme::flat_toggle(ui, rect, &response, "", frozen, ice);
                    let ink = if frozen {
                        crate::theme::text_on(ice)
                    } else if response.hovered() {
                        ice
                    } else {
                        visuals.weak_text_color()
                    };
                    // A snowflake: three strokes through the center.
                    for k in 0..3 {
                        let a = k as f32 * std::f32::consts::PI / 3.0 + std::f32::consts::FRAC_PI_2;
                        let d = vec2(a.cos(), a.sin()) * 5.5;
                        ui.painter().line_segment(
                            [rect.center() - d, rect.center() + d],
                            Stroke::new(1.4, ink),
                        );
                    }
                    let hint = if frozen {
                        tr("Frozen: click to unfreeze and hear its tracks and effects live again")
                    } else {
                        tr(
                            "Freeze this bus: render it with its tracks and effects and play that instead, to save processing",
                        )
                    };
                    if response.on_hover_text(hint).clicked() {
                        if frozen {
                            doc.unfreeze(id);
                        } else if let Err(e) = doc.freeze(id) {
                            doc.notify(trf!("Cannot freeze: {e}", e = e));
                        }
                    }
                }
                // Record arm, on the tracks that record.
                if plays {
                    let rect = column(2.0, line1);
                    let response =
                        ui.interact(rect, ui.scope_id().with(("arm", id.0)), Sense::click());
                    crate::theme::flat_arm(ui, rect, &response, track.armed);
                    if response.on_hover_text(tr("Record arm")).clicked() {
                        let armed = !track.armed;
                        let alone = crate::mixer::clutch(ui);
                        doc.history_mut().modify_unrecorded(|p| {
                            audiostrada_core::groups::maybe_alone(p, alone, |p| {
                                audiostrada_core::groups::set_switch(
                                    p,
                                    id,
                                    audiostrada_core::groups::Link::Arm,
                                    armed,
                                )
                            })
                        });
                    }
                }
            }
            if !compact && line2 - 9.0 > header.top() {
                // The second line, at the right: input monitoring and the automation lanes.
                let shown = track.show_automation;
                let a_rect = column(2.0, line2);
                let response =
                    ui.interact(a_rect, ui.scope_id().with(("lanes", id.0)), Sense::click());
                let editing = self.auto_editor.is_some() && doc.is_track_selected(id);
                crate::theme::flat_toggle(
                    ui,
                    a_rect,
                    &response,
                    "A",
                    shown || editing,
                    visuals.selection.bg_fill,
                );
                let response = response.on_hover_text(tr(
                    "Automation: choose what to automate, and draw it in the large editor",
                ));
                egui::Popup::menu(&response).show(|ui| self.automation_menu(ui, doc, track));
                if shown {
                    let mut child =
                        ui.new_child(egui::UiBuilder::new().max_rect(column(1.0, line2)));
                    child
                        .menu_button("+", |ui| self.add_lane_menu(ui, doc, project, track))
                        .response
                        .on_hover_text(tr("Add an automation lane"));
                }
                let instrument = plays && doc.is_instrument_track(id);
                if plays && !instrument {
                    crate::mixer::monitor_toggle(ui, column(0.0, line2), doc, track, true);
                }
                // At the left, under the name: what the track is, or listens to.
                let room = Rect::from_min_max(
                    pos2(x, line2 - 9.0),
                    pos2(switches_left - 8.0, line2 + 9.0),
                );
                let mut weak = visuals.weak_text_color();
                if doc.is_track_selected(id) {
                    // Brighter over the track's color wash.
                    weak = super::draw::mix(weak, visuals.text_color(), 0.5);
                }
                if plays && !instrument && track.frozen.is_none() {
                    // The input: a quiet link, its list a click away.
                    let label = if track.stereo {
                        trf!(
                            "In {first}-{second}",
                            first = track.input + 1,
                            second = track.input + 2
                        )
                    } else {
                        trf!("In {input}", input = track.input + 1)
                    };
                    let galley = painter.layout_no_wrap(
                        format!("{label} ▾"),
                        FontId::proportional(10.5),
                        weak,
                    );
                    let link = Rect::from_min_size(
                        pos2(room.left(), line2 - galley.size().y / 2.0),
                        galley.size(),
                    );
                    let response = ui
                        .interact(
                            link.expand2(vec2(4.0, 3.0)),
                            ui.scope_id().with(("input", id.0)),
                            Sense::click(),
                        )
                        .on_hover_text(tr("The input this track records and hears"));
                    if response.hovered() {
                        painter.rect_filled(
                            link.expand2(vec2(4.0, 3.0)),
                            4.0,
                            ui.visuals().widgets.hovered.bg_fill,
                        );
                    }
                    painter.galley(
                        link.min,
                        galley,
                        if response.hovered() {
                            visuals.text_color()
                        } else {
                            weak
                        },
                    );
                    egui::Popup::menu(&response)
                        .show(|ui| crate::mixer::input_choices(ui, doc, track));
                } else {
                    let (text, hint) = if vca {
                        ("VCA", "")
                    } else if track.kind == TrackKind::Bus {
                        (tr("BUS"), "")
                    } else if track.frozen.is_some() {
                        (tr("Frozen"), "")
                    } else {
                        (
                            tr("MIDI"),
                            tr("Plays the MIDI it receives while armed: keyboards and \
                                controllers, its instrument's own keyboard or pads"),
                        )
                    };
                    let galley =
                        painter.layout_no_wrap(text.to_owned(), FontId::proportional(10.5), weak);
                    let at = Rect::from_min_size(
                        pos2(room.left(), line2 - galley.size().y / 2.0),
                        galley.size(),
                    );
                    painter.galley(at.min, galley, weak);
                    if !hint.is_empty() {
                        ui.interact(at, ui.scope_id().with(("midi-in", id.0)), Sense::hover())
                            .on_hover_text(hint);
                    }
                }
            }
            if renaming {
                self.rename_field(ui, doc, id, name.with_max_x(name.left() + max_width));
            } else if response.double_clicked()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|p| name_room.contains(p))
            {
                self.renaming = Some((id, track.name.clone()));
            }
            if response.clicked() {
                let mode = SelectMode::from_modifiers(ui.input(|i| i.modifiers));
                doc.select_track(id, mode);
                if mode != SelectMode::Toggle {
                    self.follow_groups(doc);
                }
            }
            if response.secondary_clicked() && !doc.is_track_selected(id) {
                doc.select_track(id, SelectMode::Only);
            }
            if response.drag_started() {
                let dragged = if doc.is_track_selected(id) {
                    doc.selected_tracks().to_vec()
                } else {
                    vec![id]
                };
                response.dnd_set_drag_payload(crate::mixer::DraggedTracks(dragged));
            }
            // Tracks dropped on a header move above or below it.
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                let after = pointer.y > row.center().y;
                if response
                    .dnd_hover_payload::<crate::mixer::DraggedTracks>()
                    .is_some()
                {
                    let line = if after { row.bottom() } else { row.top() };
                    painter.line_segment(
                        [pos2(row.left(), line), pos2(row.right(), line)],
                        Stroke::new(3.0, visuals.selection.bg_fill),
                    );
                }
                if let Some(dragged) = response.dnd_release_payload::<crate::mixer::DraggedTracks>()
                    && !dragged.0.contains(&id)
                {
                    doc.history_mut().edit(tr("Move tracks"), |p| {
                        routing::move_tracks(p, &dragged.0, id, after)
                    });
                }
            }
            let hint = tr(
                "Click to select (Ctrl adds, Shift extends), double-click the name to rename, \
                 drag to reorder, right-click for options",
            );
            // The whole name when it is cut.
            let over_name = elided && ui.rect_contains_pointer(name);
            response
                .on_hover_text(if over_name {
                    format!("{}\n\n{hint}", track.name)
                } else {
                    hint.to_owned()
                })
                .context_menu(|ui| self.header_menu(ui, doc, id));
            if track.show_takes {
                let used = edit::comp_sources(track);
                for (k, playlist) in track.takes.iter().enumerate() {
                    let top = row.bottom() + k as f32 * TAKE_HEIGHT;
                    let take_row = Rect::from_min_max(
                        pos2(header.left(), top),
                        pos2(header.right(), top + TAKE_HEIGHT),
                    );
                    if take_row.bottom() < header.top() || take_row.top() > header.bottom() {
                        continue;
                    }
                    painter.rect_filled(
                        take_row.shrink2(vec2(0.0, 0.5)),
                        0.0,
                        visuals.extreme_bg_color,
                    );
                    // The take's color, and its name lit while the comp plays some of it.
                    painter.rect_filled(
                        Rect::from_min_size(take_row.min, vec2(4.0, TAKE_HEIGHT)),
                        0.0,
                        take_color(track, k),
                    );
                    let playing = used.get(k).is_some_and(|r| !r.is_empty());
                    let info = playlist.clips.iter().find_map(|c| c.take.as_deref());
                    let stars_left = take_row.right() - 112.0;
                    let name_right = if info.is_some() {
                        stars_left - 16.0
                    } else {
                        take_row.right() - 72.0
                    };
                    let name_rect = Rect::from_x_y_ranges(
                        take_row.left()..=name_right.max(take_row.left()),
                        take_row.y_range(),
                    );
                    painter.with_clip_rect(name_rect).text(
                        pos2(
                            take_row.left() + 12.0 + depth as f32 * 12.0,
                            take_row.center().y,
                        ),
                        Align2::LEFT_CENTER,
                        lane_label(playlist),
                        FontId::proportional(11.0),
                        if playing {
                            visuals.strong_text_color()
                        } else {
                            visuals.weak_text_color()
                        },
                    );
                    if let Some(take) = info {
                        self.take_marks(ui, doc, (id, k), take, take_row, stars_left);
                    }
                    let use_rect = Rect::from_min_size(
                        pos2(take_row.right() - 68.0, take_row.center().y - 9.0),
                        vec2(40.0, 18.0),
                    );
                    if ui
                        .put(
                            use_rect,
                            egui::Button::new(egui::RichText::new(tr("Use")).size(10.0)),
                        )
                        .on_hover_text(tr(
                            "Comp this whole take in: the track plays it from start to end",
                        ))
                        .clicked()
                    {
                        doc.history_mut().edit(tr("Comp take"), |p| {
                            edit::comp_whole_take(p, id, k);
                        });
                    }
                    let remove_rect = Rect::from_min_size(
                        pos2(take_row.right() - 24.0, take_row.center().y - 9.0),
                        vec2(20.0, 18.0),
                    );
                    if ui
                        .put(
                            remove_rect,
                            egui::Button::new(egui::RichText::new("✕").size(10.0)),
                        )
                        .on_hover_text(tr("Delete this take; what the track plays of it stays"))
                        .clicked()
                    {
                        doc.history_mut()
                            .edit(tr("Delete take"), |p| edit::remove_take(p, id, k));
                    }
                }
            }
            let mut top = row.bottom() + takes_height(track);
            for li in shown_lanes(track) {
                let height = lane_height(&track.automation[li]);
                let lane_row = Rect::from_min_max(
                    pos2(header.left(), top),
                    pos2(header.right(), top + height),
                );
                top += height;
                if lane_row.bottom() < header.top() || lane_row.top() > header.bottom() {
                    continue;
                }
                self.lane_edge(ui, doc, lane_row, (id, li));
                let lane = &track.automation[li];
                let target = lane.target.clone();
                painter.rect_filled(
                    lane_row.shrink2(vec2(0.0, 0.5)),
                    0.0,
                    super::draw::mix(visuals.panel_fill, visuals.extreme_bg_color, 0.55),
                );
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(lane_row.left() + 1.0, lane_row.top() + 3.0),
                        pos2(lane_row.left() + 5.0, lane_row.bottom() - 3.0),
                    ),
                    2.0,
                    color.gamma_multiply(0.45),
                );
                painter.hline(
                    lane_row.x_range(),
                    lane_row.bottom() - 0.5,
                    Stroke::new(
                        1.0,
                        Color32::from_black_alpha(if visuals.dark_mode { 110 } else { 36 }),
                    ),
                );
                // As a track's header: the name and what is set at the left, its switches in
                // the same columns at the right.
                let info = self.lane_info.get(&(id, li)).cloned();
                let name = info.as_ref().map_or("?".to_owned(), |i| i.name.clone());
                let middle = lane_row.center().y;
                let cell = vec2(22.0, 18.0);
                let column = |k: f32| {
                    Rect::from_center_size(
                        pos2(
                            lane_row.right() - HEADER_METER - 17.0 - (2.0 - k) * 24.0,
                            middle,
                        ),
                        cell,
                    )
                };
                let left = lane_row.left() + 24.0 + depth as f32 * 12.0;
                let room = column(-1.0).left() - 8.0 - left;
                let mut job = egui::text::LayoutJob::simple_singleline(
                    name,
                    crate::theme::strong(11.5),
                    if lane.enabled {
                        visuals.text_color()
                    } else {
                        visuals.weak_text_color()
                    },
                );
                job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(20.0));
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                let set = info.as_ref().and_then(LaneInfo::base_label);
                let name_top = if set.is_some() {
                    middle - galley.size().y - 1.0
                } else {
                    middle - galley.size().y / 2.0
                };
                painter.galley(pos2(left, name_top), galley, visuals.text_color());
                if let Some(set) = set {
                    painter.with_clip_rect(lane_row).text(
                        pos2(left, middle + 2.0),
                        Align2::LEFT_TOP,
                        trf!("Set: {value}", value = set),
                        FontId::proportional(10.0),
                        visuals.weak_text_color(),
                    );
                }
                // Hide, play, edit, delete.
                let hide = column(-1.0);
                let response = ui.interact(
                    hide,
                    ui.scope_id().with(("lane-hide", id.0, li)),
                    Sense::click(),
                );
                crate::theme::flat_toggle(
                    ui,
                    hide,
                    &response,
                    "–",
                    false,
                    visuals.selection.bg_fill,
                );
                if response
                    .on_hover_text(tr("Hide this lane; its points stay and still play"))
                    .clicked()
                {
                    doc.history_mut().modify_unrecorded(|p| {
                        if let Some(l) = p.track_mut(id).and_then(|t| t.automation.get_mut(li)) {
                            l.visible = false;
                        }
                    });
                }
                let on = column(0.0);
                let response = ui.interact(
                    on,
                    ui.scope_id().with(("lane-on", id.0, li)),
                    Sense::click(),
                );
                crate::theme::flat_toggle(ui, on, &response, "On", lane.enabled, theme_play());
                if response.on_hover_text(tr("Play this lane")).clicked() {
                    let enabled = !lane.enabled;
                    doc.history_mut().edit(tr("Automation lane"), |p| {
                        if let Some(l) = p.track_mut(id).and_then(|t| t.automation.get_mut(li)) {
                            l.enabled = enabled;
                        }
                    });
                }
                let open = column(1.0);
                let response = ui.interact(
                    open,
                    ui.scope_id().with(("lane-edit", id.0, li)),
                    Sense::click(),
                );
                crate::theme::flat_toggle(
                    ui,
                    open,
                    &response,
                    "✎",
                    false,
                    visuals.selection.bg_fill,
                );
                if response
                    .on_hover_text(tr("Draw this automation in the large editor"))
                    .clicked()
                {
                    self.open_auto_editor(doc, id, target.clone());
                }
                // Deleting asks first: the points go with the lane.
                let remove = column(2.0);
                let response = ui.interact(
                    remove,
                    ui.scope_id().with(("lane-delete", id.0, li)),
                    Sense::click(),
                );
                crate::theme::flat_toggle(
                    ui,
                    remove,
                    &response,
                    "✕",
                    false,
                    crate::settings::ERROR,
                );
                let points = lane.points.len();
                let response = response.on_hover_text(tr("Delete this lane"));
                egui::Popup::menu(&response).show(|ui| {
                    ui.label(trf!(
                        "Delete this automation and its {count} point(s)?",
                        count = points
                    ));
                    if ui.button(tr("Delete")).clicked() {
                        doc.history_mut().edit(tr("Delete automation lane"), |p| {
                            if let Some(t) = p.track_mut(id)
                                && li < t.automation.len()
                            {
                                t.automation.remove(li);
                            }
                        });
                        ui.close();
                    }
                    if ui.button(tr("Cancel")).clicked() {
                        ui.close();
                    }
                });
            }
        }
        // Below the tracks: a click clears the selection, a right-click adds tracks.
        let end = (header.top() + self.content_height - self.scroll_y).max(header.top());
        let empty = Rect::from_min_max(pos2(header.left(), end), header.max);
        if empty.height() > 4.0 {
            let response = ui.interact(empty, ui.scope_id().with("headers-empty"), Sense::click());
            if response.clicked() {
                doc.set_track_selection(Vec::new());
            }
            response.context_menu(|ui| {
                for action in [
                    Action::AddTrack,
                    Action::AddStereoTrack,
                    Action::AddTracks,
                    Action::AddInstrumentTrack,
                    Action::AddDrumTrack,
                    Action::AddBus,
                    Action::AddVca,
                ] {
                    if ui.button(action.label()).clicked() {
                        self.requests.push(action);
                        ui.close();
                    }
                }
                if !self.show_master {
                    ui.separator();
                    if ui.button(tr("Show the master track")).clicked() {
                        self.requests.push(Action::MasterTrack);
                        ui.close();
                    }
                }
            });
        }
    }

    /// A track's level (its input while armed), two thin bars beside its header; a click clears
    /// the clip light.
    pub(super) fn header_meter(&self, ui: &mut Ui, doc: &mut Document, id: TrackId, rect: Rect) {
        let mut level = doc.track_level(id);
        let response = ui.interact(rect, ui.scope_id().with(("meter", id.0)), Sense::click());
        crate::widgets::paint_slim_meter(ui, rect, &level);
        if response.clicked() {
            level.clipped = false;
            doc.set_track_level(id, level);
        }
        let peaks = trf!(
            "Peak {left} / {right} dBFS. Click to clear the clip light",
            left = format!("{:.1}", level.hold[0]),
            right = format!("{:.1}", level.hold[1])
        );
        let armed =
            doc.project().track(id).is_some_and(|t| t.armed) && !doc.is_instrument_track(id);
        response.on_hover_text(if armed {
            format!("{}\n{peaks}", tr("Input level (armed track)"))
        } else {
            peaks
        });
    }

    /// The right edge of the track headers, dragged to widen or narrow them.
    /// The bottom edge of a track's header: dragged, the track (or the selected ones) gets
    /// taller or shorter; a double-click gives it the others' height again.
    /// The bottom edge of a lane's header: dragged, it makes the lane taller or shorter;
    /// double-clicked, it brings it back to its usual height.
    fn lane_edge(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        row: Rect,
        (id, li): (TrackId, usize),
    ) {
        let edge = Rect::from_x_y_ranges(row.x_range(), row.bottom() - 3.0..=row.bottom() + 2.0);
        let response = ui
            .interact(
                edge,
                ui.scope_id().with(("lane-edge", id.0, li)),
                Sense::click_and_drag(),
            )
            .on_hover_text(tr(
                "Drag to resize the lane, double-click for its usual height",
            ));
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            ui.painter().hline(
                row.x_range(),
                row.bottom(),
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
        }
        let height = if response.double_clicked() {
            Some(1.0)
        } else if response.dragged() {
            let pixels =
                (row.height() + response.drag_delta().y).clamp(MIN_LANE_HEIGHT, MAX_LANE_HEIGHT);
            Some(pixels / LANE_HEIGHT)
        } else {
            None
        };
        if let Some(height) = height {
            doc.history_mut().modify_unrecorded(|p| {
                if let Some(lane) = p.track_mut(id).and_then(|t| t.automation.get_mut(li)) {
                    lane.height = height;
                }
            });
        }
    }

    fn height_edge(&mut self, ui: &mut Ui, doc: &mut Document, row: Rect, id: TrackId) {
        let edge = Rect::from_x_y_ranges(row.x_range(), row.bottom() - 3.0..=row.bottom() + 2.0);
        let response = ui.interact(
            edge,
            ui.scope_id().with(("height-edge", id.0)),
            Sense::click_and_drag(),
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            ui.painter().hline(
                row.x_range(),
                row.bottom(),
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
        }
        let tracks = if doc.is_track_selected(id) {
            doc.selected_tracks().to_vec()
        } else {
            vec![id]
        };
        let base = self.track_height;
        if response.dragged() {
            let height =
                (row.height() + response.drag_delta().y).clamp(MIN_TRACK_HEIGHT, MAX_TRACK_HEIGHT);
            doc.history_mut().modify_unrecorded(|p| {
                for t in &tracks {
                    if let Some(t) = p.track_mut(*t) {
                        t.height = height / base;
                    }
                }
            });
        }
        if response.double_clicked() {
            doc.history_mut().modify_unrecorded(|p| {
                for t in &tracks {
                    if let Some(t) = p.track_mut(*t) {
                        t.height = 1.0;
                    }
                }
            });
        }
        response.on_hover_text(tr(
            "Drag to make the track taller or shorter (all the selected ones); double-click for the others' height",
        ));
    }

    pub(super) fn header_edge(&mut self, ui: &mut Ui, full: Rect, header: Rect) {
        let edge =
            Rect::from_x_y_ranges(header.right() - 5.0..=header.right() + 1.0, full.y_range());
        let response = ui.interact(
            edge,
            ui.scope_id().with("header-edge"),
            Sense::click_and_drag(),
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            ui.painter().line_segment(
                [
                    pos2(header.right(), full.top()),
                    pos2(header.right(), full.bottom()),
                ],
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
        }
        if response.dragged() {
            self.header_width = (header.width() + response.drag_delta().x)
                .clamp(MIN_HEADER_WIDTH, MAX_HEADER_WIDTH);
        }
        if response.drag_stopped() {
            self.header_resized = true;
        }
        if response.double_clicked() {
            self.header_width = HEADER_WIDTH;
            self.header_resized = true;
        }
        response.on_hover_text(tr(
            "Drag to widen or narrow the track headers; double-click for the usual width",
        ));
    }

    /// The text field over a track's name while renaming it: Enter or a click elsewhere keeps
    /// the name, Escape leaves it as it was.
    pub(super) fn rename_field(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        id: TrackId,
        rect: Rect,
    ) {
        let Some((_, text)) = self.renaming.as_mut() else {
            return;
        };
        let edit_id = ui.scope_id().with(("rename", id.0));
        let response = ui.put(
            rect.expand2(vec2(2.0, 2.0)).with_min_y(rect.top() - 3.0),
            egui::TextEdit::singleline(text)
                .id(edit_id)
                .font(crate::theme::strong(13.0))
                .margin(vec2(2.0, 1.0)),
        );
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
            // The whole name selected, to type a new one.
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                )));
            state.store(ui.ctx(), edit_id);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.renaming = None;
        } else if response.lost_focus() {
            let name = text.trim().to_owned();
            self.renaming = None;
            if !name.is_empty() {
                doc.history_mut().edit(tr("Rename track"), |p| {
                    if let Some(t) = p.track_mut(id) {
                        t.name = name;
                    }
                });
            }
        }
    }

    pub(super) fn add_lane_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        track: &Track,
    ) {
        let targets = self.lane_targets(doc, track);
        let _ = project;
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                for target in targets {
                    let existing = track.automation.iter().position(|l| l.target == target);
                    if existing.is_some_and(|i| track.automation[i].visible) {
                        continue;
                    }
                    let name = self.target_info(doc, track, &target).name;
                    if ui.button(name).clicked() {
                        let id = track.id;
                        doc.history_mut().edit(tr("Add automation lane"), |p| {
                            if let Some(t) = p.track_mut(id) {
                                audiostrada_core::automation::lane_mut(t, &target).visible = true;
                                t.show_automation = true;
                            }
                        });
                        ui.close();
                    }
                }
            });
    }

    /// The menu of a track's automation button: what it can automate, each opening the large
    /// editor on it, and the lanes under the track.
    fn automation_menu(&mut self, ui: &mut Ui, doc: &mut Document, track: &Track) {
        ui.label(egui::RichText::new(tr("Draw the automation of")).weak());
        let targets = self.lane_targets(doc, track);
        egui::ScrollArea::vertical()
            .max_height(340.0)
            .show(ui, |ui| {
                for target in targets {
                    let name = self.target_info(doc, track, &target).name;
                    // A dot before what already has a curve.
                    let drawn = track
                        .automation
                        .iter()
                        .any(|l| l.target == target && !l.points.is_empty());
                    let label = if drawn { format!("● {name}") } else { name };
                    if ui.button(label).clicked() {
                        self.open_auto_editor(doc, track.id, target);
                        ui.close();
                    }
                }
            });
        ui.separator();
        let mut shown = track.show_automation;
        if ui
            .checkbox(&mut shown, tr("Lanes under the track"))
            .changed()
        {
            let id = track.id;
            doc.history_mut().modify_unrecorded(|p| {
                if let Some(t) = p.track_mut(id) {
                    t.show_automation = shown;
                }
            });
            ui.close();
        }
    }

    /// In the automation editor, the curve a track shows, named on a button that lists the
    /// others.
    pub(super) fn lane_chooser(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        rect: Rect,
    ) {
        let visuals = ui.visuals().clone();
        let current = self.lane_choice(track);
        let name = self.target_info(doc, track, &current).name;
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("lane-choice", track.id.0)),
                Sense::click(),
            )
            .on_hover_text(tr(
                "The curve shown over this track: click to choose another",
            ));
        let color = crate::theme::track_color(track.color);
        ui.painter().rect_filled(
            rect,
            5.0,
            if response.hovered() {
                visuals.widgets.hovered.bg_fill
            } else {
                visuals.widgets.inactive.bg_fill
            },
        );
        ui.painter().rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0, color.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
        let mut job = egui::text::LayoutJob::simple_singleline(
            name,
            crate::theme::strong(11.5),
            visuals.strong_text_color(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 24.0);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        ui.painter().galley(
            pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            visuals.text_color(),
        );
        ui.painter().text(
            pos2(rect.right() - 9.0, rect.center().y),
            Align2::CENTER_CENTER,
            "▾",
            FontId::proportional(10.0),
            visuals.weak_text_color(),
        );
        egui::Popup::menu(&response).show(|ui| {
            let targets = self.lane_targets(doc, track);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for target in targets {
                        let name = self.target_info(doc, track, &target).name;
                        if ui.selectable_label(target == current, name).clicked() {
                            self.lane_choice.insert(track.id, target);
                            ui.close();
                        }
                    }
                });
        });
    }

    /// What a track can automate: its volume, pan and mute, its sends, its effects' parameters.
    pub(super) fn lane_targets(
        &mut self,
        doc: &mut Document,
        track: &Track,
    ) -> Vec<AutomationTarget> {
        // A VCA's level moves its tracks' levels: that is what it automates.
        let mut targets = if track.kind == TrackKind::Vca {
            vec![AutomationTarget::Gain]
        } else {
            vec![
                AutomationTarget::Gain,
                AutomationTarget::Pan,
                AutomationTarget::Mute,
            ]
        };
        targets.extend(track.sends.iter().map(|s| AutomationTarget::Send(s.target)));
        for insert in &track.inserts {
            if let Some(info) = audiostrada_dsp::info(&insert.effect) {
                targets.extend(
                    info.params
                        .iter()
                        .filter(|p| {
                            crate::effect_editors::automatable(&insert.effect, &insert.params, p.id)
                        })
                        .map(|p| AutomationTarget::Param {
                            insert: insert.id,
                            param: p.id.to_owned(),
                        }),
                );
            } else if let Some(plugin) =
                doc.player_mut().plugin(Channel::Track(track.id), insert.id)
            {
                targets.extend(
                    plugin
                        .params()
                        .into_iter()
                        .filter(|p| !p.hidden && !p.read_only)
                        .take(200)
                        .map(|p| AutomationTarget::Param {
                            insert: insert.id,
                            param: p.id.to_string(),
                        }),
                );
            }
        }
        targets
    }
}

/// How a lane's values read.
#[derive(Clone, Debug)]
pub(super) enum Reading {
    Db,
    Pan,
    Toggle,
    Param(&'static audiostrada_dsp::ParamInfo),
    /// A plugin's value, as a share of its range.
    Share,
}

/// What an automation lane shows: its name and range, how a value reads, and the value set on
/// the control itself (the fader, the effect's knob).
#[derive(Clone, Debug)]
pub(super) struct LaneInfo {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub reading: Reading,
    /// Drawn on a logarithmic scale: times and frequencies over a wide range.
    pub log: bool,
    pub base: Option<f32>,
    /// The plugin's own words for the base value.
    pub base_text: Option<String>,
}

impl LaneInfo {
    /// Where a value sits in the range, 0 to 1.
    pub fn share(&self, v: f32) -> f32 {
        // Volumes follow the fader's travel, finer around 0 dB.
        let f = if matches!(self.reading, Reading::Db) {
            crate::widgets::fader_position(v)
        } else if self.log {
            (v.max(self.min) / self.min).ln() / (self.max / self.min).ln()
        } else {
            (v - self.min) / (self.max - self.min).max(1e-9)
        };
        f.clamp(0.0, 1.0)
    }

    /// The value at a share of the range.
    pub fn at_share(&self, f: f32) -> f32 {
        if matches!(self.reading, Reading::Db) {
            crate::widgets::fader_db(f)
        } else if self.log {
            self.min * (self.max / self.min).powf(f)
        } else {
            self.min + f * (self.max - self.min)
        }
    }

    /// A value in the control's unit: "-3.0 dB", "L 20", "2.40 kHz", "62 %".
    pub fn text(&self, v: f32) -> String {
        match self.reading {
            Reading::Db if v <= -59.5 => "-∞ dB".to_owned(),
            Reading::Db => format!("{v:+.1} dB"),
            Reading::Pan => {
                let side = (v * 100.0).round() as i32;
                match side {
                    0 => tr("Center").to_owned(),
                    s if s < 0 => format!("L {}", -s),
                    s => format!("R {s}"),
                }
            }
            Reading::Toggle if v >= 0.5 => tr("Muted").to_owned(),
            Reading::Toggle => tr("Not muted").to_owned(),
            Reading::Param(p) => crate::effect_editors::format_value(p, v),
            Reading::Share => {
                format!(
                    "{:.0} %",
                    (v - self.min) / (self.max - self.min).max(1e-9) * 100.0
                )
            }
        }
    }

    /// Values a moved point is drawn to: the set value, 0 dB, the center, the default.
    pub fn landmarks(&self) -> Vec<f32> {
        let mut marks: Vec<f32> = self.base.into_iter().collect();
        match self.reading {
            Reading::Db | Reading::Pan => marks.push(0.0),
            Reading::Param(p) => marks.push(p.default),
            Reading::Toggle | Reading::Share => {}
        }
        marks
    }

    /// A value typed in the lane's unit: "-3", "-3 dB", "L 20", "2.4 kHz", "50 %".
    pub fn parse(&self, text: &str) -> Option<f32> {
        let t = text.trim().to_lowercase();
        let digits: String = t
            .chars()
            .filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'))
            .collect();
        let number = digits.replace(',', ".").parse::<f32>().ok();
        let v = match self.reading {
            Reading::Db if t.contains("inf") || t.contains('∞') => self.min,
            Reading::Pan if t.starts_with('c') => 0.0,
            Reading::Pan => {
                let n = number? / 100.0;
                match t.chars().next() {
                    Some('l' | 'g') => -n.abs(),
                    Some('r' | 'd') => n.abs(),
                    _ => n,
                }
            }
            Reading::Toggle => match number {
                Some(n) => (n >= 0.5) as u8 as f32,
                None if t.starts_with(&tr("Not muted").to_lowercase()) => 0.0,
                None if t.starts_with('m') || t == "on" || t == "oui" || t == "yes" => 1.0,
                None => 0.0,
            },
            Reading::Share => self.min + number? / 100.0 * (self.max - self.min),
            Reading::Param(p) if !p.choices.is_empty() => p
                .choices
                .iter()
                .position(|c| tr(c).to_lowercase() == t || c.to_lowercase() == t)
                .map_or(number?, |i| p.min + i as f32),
            Reading::Param(p) if p.unit == "Hz" && t.contains('k') => number? * 1000.0,
            Reading::Db | Reading::Param(_) => number?,
        };
        v.is_finite().then(|| v.clamp(self.min, self.max))
    }

    pub fn base_label(&self) -> Option<String> {
        self.base_text
            .clone()
            .or_else(|| self.base.map(|b| self.text(b)))
    }

    /// A value and, beside it, the one set on the control.
    pub fn compared(&self, v: f32) -> String {
        match self.base_label() {
            Some(set) => trf!("{value} · set {base}", value = self.text(v), base = set),
            None => self.text(v),
        }
    }
}

impl TimelineView {
    /// A take lane's stars, and a mark when the input clipped or the audio dropped out.
    fn take_marks(
        &self,
        ui: &mut Ui,
        doc: &mut Document,
        (track, lane): (TrackId, usize),
        take: &audiostrada_core::TakeInfo,
        row: Rect,
        left: f32,
    ) {
        use crate::settings::{ERROR, WARN};
        let weak = ui.visuals().weak_text_color();
        for star in 1..=3u8 {
            let rect = Rect::from_min_size(
                pos2(left + f32::from(star - 1) * 13.0, row.top()),
                vec2(13.0, row.height()),
            );
            let lit = take.rating >= star;
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                if lit { "★" } else { "☆" },
                FontId::proportional(11.0),
                if lit { WARN } else { weak },
            );
            let response = ui
                .interact(
                    rect,
                    ui.scope_id().with(("take-star", track, lane, star)),
                    Sense::click(),
                )
                .on_hover_text(tr("Rate this take"));
            if response.clicked() {
                let rating = if take.rating == star { 0 } else { star };
                let number = take.number;
                doc.history_mut().edit(tr("Rate take"), |p| {
                    edit::set_take_rating(p, track, number, rating)
                });
            }
        }
        let mut problems = Vec::new();
        if take.clipped > 0 {
            problems.push(trf!(
                "Clipped {count}×: the input was too loud",
                count = take.clipped
            ));
        }
        if !take.dropouts.is_empty() {
            problems.push(trf!(
                "{count} dropout(s): marked in red on the clip",
                count = take.dropouts.len()
            ));
        }
        if problems.is_empty() {
            return;
        }
        let rect = Rect::from_min_size(pos2(left - 14.0, row.top()), vec2(12.0, row.height()));
        let color = if take.dropouts.is_empty() {
            WARN
        } else {
            ERROR
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "!",
            FontId::proportional(13.0),
            color,
        );
        ui.interact(
            rect,
            ui.scope_id().with(("take-problems", track, lane)),
            Sense::hover(),
        )
        .on_hover_text(problems.join("\n"));
    }
}

/// The green of what plays.
fn theme_play() -> Color32 {
    crate::theme::PLAY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(reading: Reading, min: f32, max: f32) -> LaneInfo {
        LaneInfo {
            name: String::new(),
            min,
            max,
            reading,
            log: false,
            base: Some(-6.0),
            base_text: None,
        }
    }

    #[test]
    fn typed_values_read_in_the_lane_unit_and_volumes_follow_the_fader() {
        let db = info(Reading::Db, -60.0, 12.0);
        assert_eq!(db.parse("-3,5 dB"), Some(-3.5));
        assert_eq!(db.parse("-inf"), Some(-60.0));
        assert_eq!(db.parse("40"), Some(12.0));
        assert_eq!(db.parse("loud"), None);
        // 0 dB sits where the fader puts it, the top quarter keeping +12 dB.
        assert!((db.share(0.0) - crate::widgets::fader_position(0.0)).abs() < 1e-6);
        assert!((db.at_share(db.share(-7.5)) + 7.5).abs() < 1e-4);
        assert_eq!(db.landmarks(), vec![-6.0, 0.0]);
        let pan = info(Reading::Pan, -1.0, 1.0);
        assert_eq!(pan.parse("L 20"), Some(-0.2));
        assert_eq!(pan.parse("R30"), Some(0.3));
        assert_eq!(pan.parse("-50"), Some(-0.5));
        assert_eq!(pan.parse("C"), Some(0.0));
        let mute = info(Reading::Toggle, 0.0, 1.0);
        assert_eq!(mute.parse("1"), Some(1.0));
        assert_eq!(mute.parse("Muted"), Some(1.0));
        assert_eq!(mute.parse("Not muted"), Some(0.0));
        let share = info(Reading::Share, 0.0, 2.0);
        assert_eq!(share.parse("25 %"), Some(0.5));
    }
}
