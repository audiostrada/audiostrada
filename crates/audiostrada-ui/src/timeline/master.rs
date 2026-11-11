//! The master at the top of the timeline, when shown: its name, its listening switches and its
//! automation lanes, drawn and edited as a track's (the master always reads them).

use audiostrada_core::automation::{AutomationLane, master_lane_mut};
use headers::{LaneInfo, Reading};

use super::*;

/// The master's volume lane, its main row, is taller than a lane.
const MAIN: f32 = 1.25 * LANE_HEIGHT;

/// The master's lanes shown: its volume first, always, then those added with +.
fn shown(project: &Project) -> Vec<AutomationLane> {
    let master = &project.master;
    let volume = master
        .automation
        .iter()
        .find(|l| l.target == AutomationTarget::Gain)
        .cloned()
        .unwrap_or_else(|| AutomationLane::new(AutomationTarget::Gain));
    std::iter::once(volume)
        .chain(
            master
                .automation
                .iter()
                .filter(|l| l.visible && l.target != AutomationTarget::Gain)
                .cloned(),
        )
        .collect()
}

/// A master lane's usual height, the volume's being the larger.
fn base(target: &AutomationTarget) -> f32 {
    if *target == AutomationTarget::Gain {
        MAIN
    } else {
        LANE_HEIGHT
    }
}

fn height(lane: &AutomationLane) -> f32 {
    (base(&lane.target) * lane.height).clamp(MIN_LANE_HEIGHT, MAX_LANE_HEIGHT)
}

/// Height of the master's band: its lanes, the volume first.
pub(super) fn band_height(project: &Project) -> f32 {
    shown(project).iter().map(height).sum()
}

impl TimelineView {
    /// Draws the master's band as a track: `head` beside the track headers, `area` above the
    /// lanes; its volume is its main row, under its name and switches.
    pub(super) fn master_band(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        (head, area): (Rect, Rect),
        scale: Scale,
        magnet: &super::magnet::Magnet,
    ) {
        let visuals = ui.visuals().clone();
        let painter = ui.painter().with_clip_rect(head.union(area));
        painter.rect_filled(head, 0.0, visuals.panel_fill);
        painter.rect_filled(area, 0.0, visuals.extreme_bg_color);
        let edge = visuals.widgets.noninteractive.bg_stroke.color;
        // Under its switches and lanes' buttons: a right-click hides the master.
        ui.interact(head, ui.scope_id().with("master-head"), Sense::click())
            .context_menu(|ui| {
                if ui.button(tr("Hide the master track")).clicked() {
                    self.requests.push(Action::MasterTrack);
                    ui.close();
                }
            });
        let mut top = head.top();
        for (k, lane) in shown(project).into_iter().enumerate() {
            let h = height(&lane);
            let name = Rect::from_min_max(pos2(head.left(), top), pos2(head.right(), top + h));
            let rect = Rect::from_min_max(pos2(area.left(), top), pos2(area.right(), top + h));
            top += h;
            if k > 0 {
                painter.hline(
                    head.left()..=area.right(),
                    name.top(),
                    Stroke::new(1.0, edge),
                );
            }
            let info = self.master_info(doc, &lane.target);
            if k == 0 {
                painter.text(
                    pos2(name.left() + 10.0, name.top() + 14.0),
                    Align2::LEFT_CENTER,
                    tr("Master"),
                    crate::theme::strong(13.0),
                    visuals.strong_text_color(),
                );
                painter.text(
                    pos2(name.left() + 10.0, name.top() + 32.0),
                    Align2::LEFT_CENTER,
                    &info.name,
                    FontId::proportional(10.5),
                    visuals.weak_text_color(),
                );
                self.master_switches(
                    ui,
                    doc,
                    Rect::from_min_size(name.min, vec2(name.width(), 28.0)),
                );
            } else {
                painter.text(
                    pos2(name.left() + 22.0, name.top() + 12.0),
                    Align2::LEFT_CENTER,
                    &info.name,
                    FontId::proportional(11.0),
                    visuals.text_color(),
                );
                let hide = Rect::from_min_size(
                    pos2(name.right() - 26.0, name.top() + 3.0),
                    vec2(20.0, 18.0),
                );
                if ui
                    .put(hide, egui::Button::new("✕").frame(false))
                    .on_hover_text(tr("Hide this lane; its points stay"))
                    .clicked()
                {
                    let target = lane.target.clone();
                    doc.history_mut().edit(tr("Hide automation lane"), |p| {
                        master_lane_mut(&mut p.master, &target).visible = false;
                    });
                }
            }
            let color = visuals.selection.bg_fill;
            draw_lane(
                &painter.with_clip_rect(rect),
                rect,
                &lane,
                &info,
                scale,
                color,
                &visuals,
            );
            let view = super::automation::LaneView {
                key: super::automation::LaneKey::Master(lane.target.clone()),
                rect,
                clip: area,
                info: &info,
                scale,
                magnet,
                origin: 0,
                overlay: false,
            };
            self.lane_editor(ui, doc, &view);
            master_lane_edge(ui, doc, name.union(rect), &lane.target);
        }
        painter.hline(
            head.left()..=area.right(),
            head.bottom() - 0.5,
            Stroke::new(2.0, edge),
        );
    }

    /// Mute and Mono of the control room, and the button adding a lane, at the right of the
    /// master's title.
    fn master_switches(&mut self, ui: &mut Ui, doc: &mut Document, title: Rect) {
        let (mut mute, dim, mut mono) = doc.player().control_room();
        let before = (mute, mono);
        let size = vec2(24.0, 20.0);
        let y = title.center().y - size.y / 2.0;
        let add = Rect::from_min_size(pos2(title.right() - 30.0, y), size);
        let mono_rect = Rect::from_min_size(pos2(add.left() - 46.0, y), vec2(42.0, size.y));
        let mute_rect = Rect::from_min_size(pos2(mono_rect.left() - 28.0, y), size);
        let response = ui.interact(mute_rect, ui.scope_id().with("master-mute"), Sense::click());
        crate::theme::paint_letter_toggle(ui, mute_rect, &response, "M", mute, crate::theme::MUTE);
        if response
            .on_hover_text(tr(
                "Mute the speakers: what is heard only, never the export",
            ))
            .clicked()
        {
            mute = !mute;
        }
        let response = ui.interact(mono_rect, ui.scope_id().with("master-mono"), Sense::click());
        crate::theme::paint_letter_toggle(
            ui,
            mono_rect,
            &response,
            tr("Mono"),
            mono,
            crate::settings::WARN,
        );
        if response
            .on_hover_text(tr(
                "Hear left and right summed, to check the mix in mono; never the export",
            ))
            .clicked()
        {
            mono = !mono;
        }
        if (mute, mono) != before {
            doc.player_mut().set_control_room((mute, dim, mono));
        }
        let add = ui
            .put(add, egui::Button::new("+"))
            .on_hover_text(tr("Add an automation lane"));
        egui::Popup::menu(&add)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.master_lane_menu(ui, doc));
    }

    /// What a master lane shows: its volume, or a parameter of one of its effects.
    fn master_info(&mut self, doc: &mut Document, target: &AutomationTarget) -> LaneInfo {
        let unknown = LaneInfo {
            name: "?".into(),
            min: 0.0,
            max: 1.0,
            reading: Reading::Share,
            log: false,
            base: None,
            base_text: None,
        };
        let master = &doc.project().master;
        match target {
            AutomationTarget::Gain => LaneInfo {
                name: tr("Volume").into(),
                min: -60.0,
                max: 12.0,
                reading: Reading::Db,
                base: Some(master.gain_db),
                ..unknown
            },
            AutomationTarget::Param { insert, param } => {
                let Some(ins) = audiostrada_core::routing::flatten(&master.inserts)
                    .into_iter()
                    .find(|i| i.id == *insert)
                    .cloned()
                else {
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
                let name = doc
                    .player()
                    .plugins()
                    .find(&ins.effect)
                    .map_or_else(|| ins.effect.clone(), |p| p.name.clone());
                let found = param.parse::<u32>().ok().and_then(|id| {
                    let plugin = doc.player_mut().plugin(Channel::Master, *insert)?;
                    plugin.params().into_iter().find(|p| p.id == id)
                });
                match found {
                    Some(p) => LaneInfo {
                        name: format!("{name}: {}", p.name),
                        min: p.min as f32,
                        max: p.max as f32,
                        ..unknown
                    },
                    None => LaneInfo {
                        name: format!("{name}: {param}"),
                        ..unknown
                    },
                }
            }
            _ => unknown,
        }
    }

    /// The master's volume and its effects' parameters, to add as a lane.
    fn master_lane_menu(&mut self, ui: &mut Ui, doc: &mut Document) {
        let master = doc.project().master.clone();
        // The volume is always shown: its effects' parameters only.
        let mut targets = Vec::new();
        for insert in master.inserts.iter().filter(|i| !i.monitor) {
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
            } else if let Some(plugin) = doc.player_mut().plugin(Channel::Master, insert.id) {
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
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                for target in targets {
                    if master
                        .automation
                        .iter()
                        .any(|l| l.target == target && l.visible)
                    {
                        continue;
                    }
                    let name = self.master_info(doc, &target).name;
                    if ui.button(name).clicked() {
                        doc.history_mut().edit(tr("Add automation lane"), |p| {
                            master_lane_mut(&mut p.master, &target).visible = true;
                        });
                        ui.close();
                    }
                }
            });
    }
}

/// The lower edge of a master lane: dragged, it resizes the lane, as a track's; a double-click
/// gives it back its usual height.
fn master_lane_edge(ui: &mut Ui, doc: &mut Document, row: Rect, target: &AutomationTarget) {
    let edge = Rect::from_x_y_ranges(row.x_range(), row.bottom() - 4.0..=row.bottom() + 2.0);
    let response = ui
        .interact(
            edge,
            ui.scope_id()
                .with(("master-lane-edge", format!("{target:?}"))),
            Sense::click_and_drag(),
        )
        .on_hover_text(tr(
            "Drag to resize the lane, double-click for its usual height",
        ));
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        ui.painter().hline(
            row.x_range(),
            row.bottom(),
            Stroke::new(2.0, ui.visuals().selection.bg_fill),
        );
    }
    let factor = if response.double_clicked() {
        Some(1.0)
    } else if response.dragged() {
        let pixels =
            (row.height() + response.drag_delta().y).clamp(MIN_LANE_HEIGHT, MAX_LANE_HEIGHT);
        Some(pixels / base(target))
    } else {
        None
    };
    if let Some(factor) = factor {
        doc.history_mut()
            .modify_unrecorded(|p| master_lane_mut(&mut p.master, target).height = factor);
    }
}
