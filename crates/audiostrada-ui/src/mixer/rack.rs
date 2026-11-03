//! The rack: the chain of the selected track (or of a pinned channel) as modules side by side,
//! each with its main controls, A/B and its editor a click away; parallel chains split the
//! signal into branches of their own and mix them back.

use std::collections::BTreeMap;

use super::*;

/// Width of an effect's module.
const MODULE: f32 = 224.0;
/// Height of the modules.
const HEIGHT: f32 = 168.0;
/// Width of the gaps between modules, where effects are added or dropped.
const GAP: f32 = 18.0;
/// Width of a parallel chain's module.
const PARALLEL: f32 = 380.0;

/// Parameter values of an insert, as an A or B slot keeps them.
type Settings = BTreeMap<String, f32>;

/// What the rack remembers between frames.
#[derive(Debug, Default)]
pub(super) struct RackState {
    /// Channel shown whatever the selection.
    pub pin: Option<Channel>,
    /// Where the effect chosen in the browser goes.
    pub place: Option<(Channel, routing::Place, usize)>,
    /// Insert being dragged.
    drag: Option<InsertId>,
    /// Where a dragged insert can land, drawn this frame.
    gaps: Vec<(Rect, routing::Place, usize)>,
    /// Settings kept in the A and B slots of each insert, and the one in use.
    ab: HashMap<InsertId, (usize, [Option<Settings>; 2])>,
}

/// The parameters shown on a built-in effect's face: the main ones of the common effects,
/// else its first values that are neither switches nor lists.
fn face(info: &audiostrada_dsp::EffectInfo) -> Vec<&'static str> {
    let chosen: &[&str] = match info.id {
        "reverb" | "plate" | "hall" => &["decay", "predelay", "size", "mix"],
        "delay" => &["time", "feedback", "high_cut", "mix"],
        "tape" => &["drive", "bump", "wow", "mix"],
        "compressor" => &["threshold", "ratio", "attack", "makeup"],
        _ => &[],
    };
    let known: Vec<&'static str> = info
        .params
        .iter()
        .filter(|p| chosen.contains(&p.id))
        .map(|p| p.id)
        .collect();
    if !known.is_empty() {
        return known;
    }
    info.params
        .iter()
        .filter(|p| !p.toggle && p.choices.is_empty())
        .take(4)
        .map(|p| p.id)
        .collect()
}

/// The response of an EQ module's bands, drawn in its face.
fn eq_curve(ui: &mut Ui, values: &[f32], tint: Color32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, p.panel);
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(1.0, p.weak.gamma_multiply(0.25)),
    );
    let offsets = [0.0; audiostrada_dsp::eq::BANDS];
    let points: Vec<egui::Pos2> = (0..=60)
        .map(|i| {
            let t = i as f32 / 60.0;
            let freq = 20.0 * 1000f64.powf(f64::from(t));
            let db = audiostrada_dsp::eq::response_db(values, &offsets, freq, 48_000.0)
                .clamp(-18.0, 18.0) as f32;
            pos2(
                rect.left() + rect.width() * t,
                rect.center().y - db / 18.0 * rect.height() * 0.45,
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, tint)));
}

impl MixerView {
    /// The rack's panel.
    pub fn rack(&mut self, ui: &mut Ui) {
        let doc_rc = Rc::clone(&self.doc);
        let Ok(mut doc) = doc_rc.try_borrow_mut() else {
            return;
        };
        let project = doc.project().clone();
        let selected = doc
            .selected_tracks()
            .last()
            .copied()
            .filter(|&t| project.track(t).is_some_and(|t| t.kind == TrackKind::Audio));
        let channel = self
            .rack
            .pin
            .or(selected.map(Channel::Track))
            .filter(|c| match c {
                Channel::Track(t) => project.track(*t).is_some(),
                Channel::Master => true,
            });
        self.rack_header(ui, &doc, &project, channel);
        ui.separator();
        let Some(channel) = channel else {
            ui.weak(tr("Select a track to see its effects here."));
            return;
        };
        self.rack.gaps.clear();
        egui::ScrollArea::horizontal()
            .id_salt("rack")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.rack_strip(ui, &mut doc, channel);
                    let shown: Vec<audiostrada_core::Insert> = routing::inserts(&project, channel)
                        .iter()
                        .filter(|i| !i.monitor && i.effect != routing::STRIP)
                        .cloned()
                        .collect();
                    for (k, insert) in shown.iter().enumerate() {
                        let at = listed_index(&project, channel, false, k);
                        self.rack_gap(ui, &mut doc, channel, (None, at), true);
                        if insert.effect == routing::PARALLEL {
                            self.parallel_module(ui, &mut doc, channel, insert);
                        } else {
                            self.module(ui, &mut doc, channel, insert, MODULE);
                        }
                    }
                    let end = listed_index(&project, channel, false, shown.len());
                    self.rack_gap(ui, &mut doc, channel, (None, end), true);
                });
            });
        self.rack_drop(ui, &mut doc, channel);
    }

    /// The channel shown, the master's chain and the pin.
    fn rack_header(
        &mut self,
        ui: &mut Ui,
        doc: &Document,
        project: &Project,
        channel: Option<Channel>,
    ) {
        ui.horizontal(|ui| {
            match channel.and_then(|c| match c {
                Channel::Track(t) => project.track(t),
                Channel::Master => None,
            }) {
                Some(track) => {
                    let tint = color(track);
                    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    crate::track_icons::paint(
                        ui,
                        rect,
                        crate::track_icons::of(project, track),
                        tint,
                    );
                    ui.label(RichText::new(&track.name).strong().size(14.0).color(tint));
                }
                None if channel == Some(Channel::Master) => {
                    ui.label(RichText::new(tr("Master")).strong().size(14.0));
                }
                None => {}
            }
            if let Some(c) = channel {
                let count = routing::flatten(routing::inserts(project, c))
                    .iter()
                    .filter(|i| i.effect != routing::PARALLEL && i.effect != routing::STRIP)
                    .count();
                ui.weak(trf!("Effects: {count}", count = count));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let pinned = self.rack.pin.is_some();
                if ui
                    .selectable_label(pinned, tr("Pin"))
                    .on_hover_text(tr(
                        "Keep this chain in the rack while other tracks are selected",
                    ))
                    .clicked()
                {
                    self.rack.pin = if pinned { None } else { channel };
                }
                let master = self.rack.pin == Some(Channel::Master);
                if ui
                    .selectable_label(master, tr("Master"))
                    .on_hover_text(tr("The master's chain"))
                    .clicked()
                {
                    self.rack.pin = if master { None } else { Some(Channel::Master) };
                }
                let _ = doc;
            });
        });
    }

    /// The channel strip at the head of the rack.
    fn rack_strip(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel) {
        card(ui, 150.0, |ui| {
            ui.add_space(4.0);
            self.channel_section(ui, doc, channel, 138.0);
        });
    }

    /// A gap between modules: adds an effect there, and takes a dragged one.
    fn rack_gap(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        (place, index): (routing::Place, usize),
        parallel: bool,
    ) {
        let p = theme::current(ui);
        let (rect, response) = ui.allocate_exact_size(vec2(GAP, HEIGHT), Sense::click());
        self.rack.gaps.push((rect, place, index));
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| rect.contains(pos));
        if self.rack.drag.is_some() && hover {
            ui.painter().line_segment(
                [rect.center_top(), rect.center_bottom()],
                egui::Stroke::new(3.0, p.accent),
            );
        } else if response.hovered() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(16.0),
                p.text,
            );
        }
        let response = response.on_hover_text(tr("Add an effect here"));
        egui::Popup::menu(&response).show(|ui| {
            self.add_menu(ui, doc, channel, (place, index), parallel);
        });
    }

    /// What can be added at a place: an effect or a plugin, or a parallel chain.
    fn add_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        (place, index): (routing::Place, usize),
        parallel: bool,
    ) {
        if ui.button(tr("Effect or plugin…")).clicked() {
            self.rack.place = Some((channel, place, index));
            self.browser.borrow_mut().open((channel, false));
            ui.close();
        }
        if parallel
            && ui
                .button(tr("Parallel chain"))
                .on_hover_text(tr(
                    "Splits the signal into branches with effects of their own, then mixes them",
                ))
                .clicked()
        {
            doc.history_mut().edit(tr("Add a parallel chain"), |p| {
                routing::add_parallel(p, channel, index);
            });
            ui.close();
        }
    }

    /// Ends a drag: the insert goes to the gap under the pointer.
    fn rack_drop(&mut self, ui: &Ui, doc: &mut Document, channel: Channel) {
        let Some(id) = self.rack.drag else {
            return;
        };
        if ui.input(|i| i.pointer.any_down()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return;
        }
        self.rack.drag = None;
        let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if let Some(&(_, place, index)) = self.rack.gaps.iter().find(|(r, ..)| r.contains(pos)) {
            doc.history_mut().edit(tr("Move insert"), |p| {
                routing::move_within(p, channel, id, (place, index));
            });
        }
    }

    /// An effect's module: its switch and name, its main controls, A/B and its editor.
    fn module(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
        width: f32,
    ) {
        let id = insert.id;
        let name = effect_name(doc, &insert.effect);
        let error = doc.player().plugin_error(id).is_some();
        let latency = latency_label(doc, channel, id);
        let tint = theme::family_color(family(doc, &insert.effect));
        card(ui, width, |ui| {
            let look = Slot {
                name: &name,
                color: tint,
                lit: !insert.bypass,
                error,
                latency: latency.as_deref(),
            };
            let (head, power) = insert_slot(ui, vec2(width - 12.0, 24.0), id.0, &look);
            if power.on_hover_text(tr("Bypass or play")).clicked() {
                let bypass = !insert.bypass;
                doc.history_mut().edit(tr("Bypass insert"), |p| {
                    routing::set_bypass(p, channel, id, bypass)
                });
            }
            #[cfg(test)]
            super::tests::seen(format!("module {}", id.0), head.rect);
            let head = head.on_hover_text(tr("Click to edit, drag to move"));
            if head.drag_started() {
                self.rack.drag = Some(id);
            }
            if head.clicked() {
                self.open_insert(channel, insert);
            }
            ui.add_space(6.0);
            ui.allocate_ui(vec2(width - 12.0, HEIGHT - 70.0), |ui| {
                ui.set_min_height(HEIGHT - 70.0);
                match audiostrada_dsp::info(&insert.effect) {
                    Some(info) => {
                        let mut params = effect_editors::Params {
                            values: info
                                .params
                                .iter()
                                .map(|p| insert.params.get(p.id).copied().unwrap_or(p.default))
                                .collect(),
                            doc: &mut *doc,
                            channel,
                            insert: id,
                            info,
                        };
                        if info.id == "eq" {
                            eq_curve(ui, &params.values, tint);
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(2.0, 4.0);
                                for param in face(info) {
                                    effect_editors::param_knob(ui, &mut params, param, 32.0, tint);
                                }
                            });
                        }
                    }
                    None => self.macros(ui, doc, channel, insert, tint),
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                self.ab_buttons(ui, doc, channel, insert);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if remove_button(ui).clicked() {
                        self.remove_insert(doc, channel, id);
                    }
                    if ui
                        .small_button(tr("Edit"))
                        .on_hover_text(tr("Every setting, in the effect's window"))
                        .clicked()
                    {
                        self.open_insert(channel, insert);
                    }
                });
            });
        });
    }

    /// Opens an insert's window: AudioStrada's for a built-in effect, the plugin's own else.
    fn open_insert(&mut self, channel: Channel, insert: &audiostrada_core::Insert) {
        if audiostrada_dsp::info(&insert.effect).is_some() {
            self.show_editor(channel, insert.id);
        } else if !self.open_gui.contains(&(channel, insert.id)) {
            self.open_gui.push((channel, insert.id));
        }
    }

    /// A plugin's chosen parameters as knobs; a right-click on the face picks them.
    fn macros(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
        tint: Color32,
    ) {
        let id = insert.id;
        if !self.plugin_params.contains_key(&id)
            && let Some(plugin) = doc.player_mut().plugin(channel, id)
        {
            let params = plugin.params().into_iter().filter(|p| !p.hidden).collect();
            self.plugin_params.insert(id, params);
        }
        let params = self.plugin_params.get(&id).cloned().unwrap_or_default();
        let chosen: Vec<&audiostrada_plugins::ParamDesc> = insert
            .macros
            .iter()
            .filter_map(|m| params.iter().find(|p| p.id.to_string() == *m))
            .collect();
        let face = ui
            .horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                for p in &chosen {
                    let key = p.id.to_string();
                    let lane = AutomationTarget::Param {
                        insert: id,
                        param: key.clone(),
                    };
                    let value = doc
                        .automation_value(channel, &lane)
                        .or_else(|| insert.params.get(&key).copied())
                        .or_else(|| {
                            doc.player_mut()
                                .plugin(channel, id)
                                .and_then(|pl| pl.param_value(p.id))
                                .map(|v| v as f32)
                        })
                        .unwrap_or(p.default as f32);
                    ui.vertical(|ui| {
                        ui.set_width(40.0);
                        let (changed, response) = widgets::knob(
                            ui,
                            value,
                            (p.min as f32, p.max as f32),
                            p.default as f32,
                            30.0,
                            false,
                            tint,
                        );
                        ui.add(egui::Label::new(RichText::new(&p.name).size(9.0)).truncate());
                        let response = response.on_hover_text(&p.name);
                        let name = p.name.clone();
                        automated_control(doc, channel, &lane, changed, &response, |doc, v| {
                            doc.history_mut().edit_merged(
                                &name,
                                &format!("insert:{}:{}", id.0, key),
                                |proj| routing::set_insert_param(proj, channel, id, &key, v),
                            );
                        });
                    });
                }
                if chosen.is_empty() {
                    ui.weak(tr("Right-click to put parameters here"));
                }
            })
            .response
            .interact(Sense::click());
        face.context_menu(|ui| {
            ui.set_max_height(320.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for p in &params {
                    let key = p.id.to_string();
                    let mut on = insert.macros.contains(&key);
                    if ui.checkbox(&mut on, &p.name).changed() {
                        doc.history_mut().edit(tr("Rack controls"), |proj| {
                            if let Some(i) = routing::find_insert_mut(proj, channel, id) {
                                i.macros.retain(|m| *m != key);
                                if on && i.macros.len() < 8 {
                                    i.macros.push(key.clone());
                                }
                            }
                        });
                    }
                }
                if params.is_empty() {
                    ui.weak(tr("This plugin exposes no parameters."));
                }
            });
        });
    }

    /// A and B settings of an insert, to compare two of them.
    fn ab_buttons(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let (current, _) = self.rack.ab.get(&id).cloned().unwrap_or_default();
        for (slot, label) in [(0, "A"), (1, "B")] {
            if ui
                .selectable_label(current == slot, label)
                .on_hover_text(tr("Two settings to compare: the other one is kept"))
                .clicked()
                && current != slot
            {
                let entry = self.rack.ab.entry(id).or_default();
                entry.1[current] = Some(insert.params.clone());
                let next = entry.1[slot]
                    .clone()
                    .unwrap_or_else(|| insert.params.clone());
                entry.0 = slot;
                doc.history_mut()
                    .edit(&trf!("Settings {slot}", slot = label), |p| {
                        for (key, value) in &next {
                            routing::set_insert_param(p, channel, id, key, *value);
                        }
                    });
            }
        }
    }

    /// A parallel chain: a row per branch with its switches, its effects and its level.
    fn parallel_module(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let p = theme::current(ui);
        let id = insert.id;
        card(ui, PARALLEL, |ui| {
            let look = Slot {
                name: tr("Parallel chain"),
                color: theme::family_color("Utility"),
                lit: !insert.bypass,
                error: false,
                latency: None,
            };
            let (head, power) = insert_slot(ui, vec2(PARALLEL - 34.0, 24.0), id.0, &look);
            if power.on_hover_text(tr("Bypass or play")).clicked() {
                let bypass = !insert.bypass;
                doc.history_mut().edit(tr("Bypass insert"), |p| {
                    routing::set_bypass(p, channel, id, bypass)
                });
            }
            if head.on_hover_text(tr("Drag to move")).drag_started() {
                self.rack.drag = Some(id);
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt(("branches", id.0))
                .max_height(HEIGHT - 64.0)
                .show(ui, |ui| {
                    for (b, branch) in insert.branches.iter().enumerate() {
                        self.branch_row(ui, doc, channel, insert, b, branch);
                    }
                });
            ui.horizontal(|ui| {
                if ui.small_button(tr("+ Branch")).clicked() {
                    doc.history_mut().edit(tr("Add a branch"), |p| {
                        routing::add_branch(p, channel, id);
                    });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if remove_button(ui).clicked() {
                        self.remove_insert(doc, channel, id);
                    }
                    ui.label(
                        RichText::new(tr("Branches are mixed after their effects"))
                            .size(9.5)
                            .color(p.weak),
                    );
                });
            });
        });
    }

    /// A branch: mute, solo, its name, its effects as chips, an add button and its level.
    fn branch_row(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        parallel: &audiostrada_core::Insert,
        b: usize,
        branch: &audiostrada_core::Branch,
    ) {
        let p = theme::current(ui);
        let id = parallel.id;
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let edit =
                |doc: &mut Document, label: &str, f: &dyn Fn(&mut audiostrada_core::Branch)| {
                    doc.history_mut().edit(label, |p| {
                        if let Some(branch) = routing::branch_mut(p, channel, id, b) {
                            f(branch);
                        }
                    });
                };
            let size = vec2(18.0, 18.0);
            if theme::letter_toggle(ui, size, "M", branch.mute, theme::MUTE)
                .on_hover_text(tr("Mute this branch"))
                .clicked()
            {
                edit(doc, tr("Mute branch"), &|b| b.mute = !b.mute);
            }
            if theme::letter_toggle(ui, size, "S", branch.solo, theme::SOLO)
                .on_hover_text(tr("Hear this branch alone"))
                .clicked()
            {
                edit(doc, tr("Solo branch"), &|b| b.solo = !b.solo);
            }
            let name = ui
                .add_sized(
                    [18.0, 18.0],
                    egui::Label::new(RichText::new(&branch.name).strong()).sense(Sense::click()),
                )
                .on_hover_text(tr("Right-click to rename or remove"));
            name.context_menu(|ui| {
                let mut text = branch.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut text).desired_width(120.0))
                    .changed()
                {
                    doc.history_mut().edit_merged(
                        tr("Rename branch"),
                        &format!("branch:{}:{b}", id.0),
                        |p| {
                            if let Some(branch) = routing::branch_mut(p, channel, id, b) {
                                branch.name = text.clone();
                            }
                        },
                    );
                }
                if parallel.branches.len() > 1 && ui.button(tr("Remove the branch")).clicked() {
                    doc.history_mut().edit(tr("Remove branch"), |p| {
                        routing::remove_branch(p, channel, id, b)
                    });
                    ui.close();
                }
            });
            // Its effects, in their order.
            let chips = PARALLEL - 4.0 * 21.0 - 90.0;
            ui.allocate_ui_with_layout(
                vec2(chips, 22.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(chips);
                    egui::ScrollArea::horizontal()
                        .id_salt(("branch", id.0, b))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for inner in &branch.inserts {
                                    self.chip(ui, doc, channel, inner);
                                }
                                if branch.inserts.is_empty() {
                                    ui.label(RichText::new(tr("direct")).size(10.0).color(p.weak));
                                }
                            });
                        });
                },
            );
            let add = ui
                .small_button("+")
                .on_hover_text(tr("Add an effect to this branch"));
            egui::Popup::menu(&add).show(|ui| {
                self.add_menu(
                    ui,
                    doc,
                    channel,
                    (Some((id, b)), branch.inserts.len()),
                    false,
                );
            });
            let color = theme::family_color("Utility");
            let (changed, response) = send_level(ui, 70.0, branch.gain_db, color, branch.mute);
            let response = response.on_hover_text(tr("Level of this branch in the mix"));
            if let Some(db) = changed {
                doc.history_mut().edit_merged(
                    tr("Branch level"),
                    &format!("branch-level:{}:{b}", id.0),
                    |p| {
                        if let Some(branch) = routing::branch_mut(p, channel, id, b) {
                            branch.gain_db = db;
                        }
                    },
                );
            }
            let _ = response;
        });
        #[cfg(test)]
        super::tests::seen(format!("branch {} {b}", id.0), row.response.rect);
        // A dragged effect dropped on the row goes at the end of the branch.
        self.rack
            .gaps
            .push((row.response.rect, Some((id, b)), branch.inserts.len()));
    }

    /// An effect of a branch: its switch and name; click to edit, drag to move.
    fn chip(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let name = effect_name(doc, &insert.effect);
        let look = Slot {
            name: &name,
            color: theme::family_color(family(doc, &insert.effect)),
            lit: !insert.bypass,
            error: doc.player().plugin_error(id).is_some(),
            latency: None,
        };
        let width = (ui.fonts_mut(|f| {
            f.layout_no_wrap(
                name.clone(),
                egui::FontId::proportional(10.5),
                Color32::WHITE,
            )
            .size()
            .x
        }) + 34.0)
            .clamp(56.0, 120.0);
        let (head, power) = insert_slot(ui, vec2(width, 18.0), id.0, &look);
        if power.clicked() {
            let bypass = !insert.bypass;
            doc.history_mut().edit(tr("Bypass insert"), |p| {
                routing::set_bypass(p, channel, id, bypass)
            });
        }
        let head = head.on_hover_text(tr("Click to edit, drag to move, right-click to remove"));
        if head.drag_started() {
            self.rack.drag = Some(id);
        }
        if head.clicked() {
            self.open_insert(channel, insert);
        }
        head.context_menu(|ui| {
            if ui.button(tr("Remove")).clicked() {
                self.remove_insert(doc, channel, id);
                ui.close();
            }
        });
    }
}

/// A module's frame, `width` wide and as tall as the rack's modules.
fn card(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(p.base)
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(width - 12.0);
            ui.set_min_height(HEIGHT - 12.0);
            ui.vertical(add);
        });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{rack_frame, rect};
    use super::*;
    use crate::document::AppDirs;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};

    #[test]
    fn a_module_drags_into_a_branch() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let (mut t, mut par, mut delay) = (None, None, None);
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            let id = p.add_track("A");
            let c = Channel::Track(id);
            par = routing::add_parallel(p, c, 0);
            delay = routing::add_insert(p, c, "delay", Default::default());
            t = Some(id);
        });
        let (t, par, delay) = (t.unwrap(), par.unwrap(), delay.unwrap());
        doc.borrow_mut()
            .select_track(t, crate::document::SelectMode::Only);
        let mut view = MixerView::new(Rc::clone(&doc));
        let ctx = egui::Context::default();
        rack_frame(&ctx, &mut view, vec![]);
        rack_frame(&ctx, &mut view, vec![]);
        let from = rect(&format!("module {}", delay.0)).center();
        let to = rect(&format!("branch {} 1", par.0)).center();
        let button = |pos: Pos2, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        rack_frame(&ctx, &mut view, vec![Event::PointerMoved(from)]);
        rack_frame(&ctx, &mut view, vec![button(from, true)]);
        for k in 1..=4 {
            let at = from.lerp(to, k as f32 / 4.0);
            rack_frame(&ctx, &mut view, vec![Event::PointerMoved(at)]);
        }
        rack_frame(&ctx, &mut view, vec![button(to, false)]);
        rack_frame(&ctx, &mut view, vec![]);
        let project = doc.borrow().project().clone();
        let list = routing::inserts(&project, Channel::Track(t));
        assert_eq!(list.len(), 1, "only the chain left at the top");
        assert_eq!(list[0].branches[1].inserts[0].id, delay);
    }
}
