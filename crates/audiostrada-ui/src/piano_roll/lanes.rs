//! The lane under the notes: velocity or probability as bars (drag one, paint across, Shift
//! for a straight ramp), or a controller curve (the selection's channel, or the selected note
//! alone for per-note expression).

use audiostrada_core::model::PITCH_BEND;
use audiostrada_core::{ControlPoint, Note, edit};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use super::ops;
use super::{Drag, DragKind, Lane, PianoRoll, Scene};
use crate::i18n::tr;
use crate::theme;

/// The value a lane edits on a note, 0 to 1.
fn value_of(lane: Lane, n: &Note) -> f32 {
    match lane {
        Lane::Probability => f32::from(n.probability) / 100.0,
        _ => f32::from(n.velocity) / 127.0,
    }
}

fn set_value(lane: Lane, n: &mut Note, v: f32) {
    let v = v.clamp(0.0, 1.0);
    match lane {
        Lane::Probability => n.probability = (v * 100.0).round().clamp(1.0, 100.0) as u8,
        _ => n.velocity = (v * 127.0).round().clamp(1.0, 127.0) as u8,
    }
}

impl PianoRoll {
    pub(super) fn lane(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        head: Rect,
        rect: Rect,
        area: Rect,
    ) {
        let p = theme::current(ui);
        ui.painter().rect_filled(head, 0.0, p.panel);
        ui.painter()
            .rect_filled(rect, 0.0, theme::mix(p.base, p.panel, 0.4));
        self.lane_head(ui, scene, head);
        let inner = rect.shrink2(vec2(0.0, 8.0));
        for f in [0.25, 0.5, 0.75] {
            let y = inner.bottom() - inner.height() * f;
            ui.painter().line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(
                    1.0,
                    p.text.gamma_multiply(if f == 0.5 { 0.08 } else { 0.04 }),
                ),
            );
        }
        match self.lane {
            Lane::Velocity | Lane::Probability => self.value_lane(ui, scene, rect, inner, area),
            Lane::Controller(c) | Lane::Expression(c) => {
                self.controller_lane(ui, scene, rect, inner, area, c)
            }
        }
    }

    fn lane_head(&mut self, ui: &mut Ui, scene: &Scene, head: Rect) {
        let p = theme::current(ui);
        let button = Rect::from_min_size(head.min + vec2(4.0, 4.0), vec2(head.width() - 8.0, 20.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(button));
        child.spacing_mut().button_padding = vec2(4.0, 1.0);
        let response = child.menu_button(
            egui::RichText::new(format!("{} ⏷", self.lane.name())).size(10.5),
            |ui| {
                for (lane, name) in Lane::all() {
                    if ui.selectable_label(self.lane == lane, name).clicked() {
                        self.lane = lane;
                        ui.close();
                    }
                }
            },
        );
        response.response.on_hover_text(tr(
            "What the lane edits: velocity, chance of playing, a controller, or the selected \
             note's own pitch, pressure or timbre (MPE)",
        ));
        let (top, bottom) = match self.lane {
            Lane::Velocity => ("127", "1"),
            Lane::Probability => ("100%", "0%"),
            Lane::Controller(PITCH_BEND) | Lane::Expression(PITCH_BEND) => ("+", "−"),
            _ => ("127", "0"),
        };
        let font = FontId::proportional(9.0);
        ui.painter().text(
            pos2(head.right() - 5.0, head.top() + 30.0),
            Align2::RIGHT_TOP,
            top,
            font.clone(),
            p.weak,
        );
        ui.painter().text(
            pos2(head.right() - 5.0, head.bottom() - 6.0),
            Align2::RIGHT_BOTTOM,
            bottom,
            font,
            p.weak,
        );
        if let Lane::Expression(_) = self.lane {
            let single = self.selected.len() == 1;
            let note = self.selected.first().and_then(|&i| scene.notes.get(i));
            let shared = note.is_some_and(|n| {
                scene
                    .notes
                    .iter()
                    .filter(|o| o.channel == n.channel)
                    .count()
                    > 1
                    || n.channel == 0
            });
            if single && shared {
                let r = Rect::from_min_size(
                    pos2(head.left() + 4.0, head.top() + 46.0),
                    vec2(head.width() - 8.0, 20.0),
                );
                if ui
                    .put(
                        r,
                        egui::Button::new(egui::RichText::new(tr("Own channels")).size(10.0)),
                    )
                    .on_hover_text(tr(
                        "Give each note its own channel (2 to 16), so that its expression is its \
                         own (MPE)",
                    ))
                    .clicked()
                {
                    let mut notes = scene.notes.clone();
                    edit::spread_mpe_channels(&mut notes);
                    self.commit(tr("Spread notes over MPE channels"), notes);
                }
            }
        }
    }

    /// Velocity or probability, as a bar per note.
    fn value_lane(&mut self, ui: &mut Ui, scene: &mut Scene, rect: Rect, inner: Rect, area: Rect) {
        let p = theme::current(ui);
        let frame = self.frame(Rect::from_x_y_ranges(area.x_range(), rect.y_range()));
        let painter = ui.painter().with_clip_rect(rect);
        let lane = self.lane;
        let y_of = |v: f32| inner.bottom() - inner.height() * v.clamp(0.0, 1.0);
        let v_of = |y: f32| ((inner.bottom() - y) / inner.height()).clamp(0.0, 1.0);
        let (t0, t1) = (frame.t(rect.left()), frame.t(rect.right()));
        let mut order: Vec<usize> = (0..scene.notes.len()).collect();
        let mask = self.mask(scene.notes.len());
        order.sort_by_key(|&i| mask[i]);
        let hovered = self.hovered;
        for i in order {
            let n = &scene.notes[i];
            let (a, b) = (scene.origin + n.start, scene.origin + n.start + n.length);
            if b < t0 || a > t1 {
                continue;
            }
            let selected = mask[i];
            let x = frame.x(a);
            let y = y_of(value_of(lane, n));
            let color = super::canvas::note_fill(
                scene.color,
                if lane == Lane::Velocity {
                    n.velocity
                } else {
                    100
                },
                selected,
            );
            let color = if n.muted {
                p.weak.gamma_multiply(0.5)
            } else {
                color
            };
            // Its value held along the note, then the stem and its head.
            painter.line_segment(
                [pos2(x, y), pos2(frame.x(b), y)],
                Stroke::new(2.0, color.gamma_multiply(0.35)),
            );
            let stem = Rect::from_min_max(pos2(x - 1.5, y), pos2(x + 1.5, inner.bottom() + 6.0));
            painter.rect_filled(
                stem,
                1.5,
                color.gamma_multiply(if selected { 1.0 } else { 0.8 }),
            );
            painter.circle_filled(
                pos2(x, y),
                if selected || hovered == Some(i) {
                    4.0
                } else {
                    3.0
                },
                color,
            );
            if selected {
                painter.circle_stroke(pos2(x, y), 4.5, Stroke::new(1.0, egui::Color32::WHITE));
            }
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with("piano-values"),
            Sense::click_and_drag(),
        );
        let modifiers = ui.input(|i| i.modifiers);
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        // The stem under the pointer: the selected ones first.
        let selected = self.selected.clone();
        let stem_at = |notes: &[Note], pos: Pos2| -> Option<usize> {
            let candidates = notes.iter().enumerate().filter(|(_, n)| {
                (frame.x(scene.origin + n.start) - pos.x).abs() < 6.0
                    && (y_of(value_of(lane, n)) - pos.y).abs() < 12.0
            });
            let mut best: Option<(usize, bool)> = None;
            for (i, _) in candidates {
                let sel = selected.contains(&i);
                if best.is_none_or(|(_, s)| sel && !s) {
                    best = Some((i, sel));
                }
            }
            best.map(|b| b.0)
        };
        if let Some(pos) = response.hover_pos() {
            let v = v_of(pos.y);
            let text = match lane {
                Lane::Probability => format!("{}%", (v * 100.0).round()),
                _ => format!("{}", (v * 127.0).round()),
            };
            painter.text(
                pos2(pos.x + 8.0, pos.y - 8.0),
                Align2::LEFT_BOTTOM,
                text,
                FontId::proportional(10.0),
                p.text,
            );
            ui.ctx()
                .set_cursor_icon(if stem_at(&scene.notes, pos).is_some() {
                    egui::CursorIcon::ResizeVertical
                } else {
                    egui::CursorIcon::Crosshair
                });
        }
        if response.drag_started()
            && let Some(pos) = ui.input(|i| i.pointer.press_origin()).or(pointer)
        {
            let kind = match stem_at(&scene.notes, pos) {
                Some(index) if !modifiers.shift => DragKind::Value { index },
                _ => DragKind::Values {
                    line: modifiers.shift,
                },
            };
            self.drag = Some(Drag::new(kind, pos, scene.notes.clone()));
        }
        if response.dragged()
            && let Some(pos) = pointer
            && let Some(mut drag) = self.drag.take()
        {
            let mut edited = match drag.kind {
                DragKind::Values { line: false } => self
                    .preview
                    .clone()
                    .unwrap_or_else(|| drag.original.clone()),
                _ => drag.original.clone(),
            };
            let only_selected = !self.selected.is_empty();
            let picked = |i: usize, sel: &[usize]| !only_selected || sel.contains(&i);
            match drag.kind {
                DragKind::Value { index } => {
                    let before = drag.original.get(index).map_or(0.0, |n| value_of(lane, n));
                    let shift = v_of(pos.y) - before;
                    let group: Vec<usize> = if self.selected.contains(&index) {
                        self.selected.clone()
                    } else {
                        vec![index]
                    };
                    for i in group {
                        if let Some(n) = edited.get_mut(i) {
                            let v = value_of(lane, &drag.original[i]) + shift;
                            set_value(lane, n, v);
                        }
                    }
                }
                DragKind::Values { line: false } => {
                    let (a, b) = (drag.last.x.min(pos.x) - 1.0, drag.last.x.max(pos.x) + 1.0);
                    for (i, n) in edited.iter_mut().enumerate() {
                        let x = frame.x(scene.origin + n.start);
                        if x >= a && x <= b && picked(i, &self.selected) {
                            let t = if (pos.x - drag.last.x).abs() > 0.5 {
                                (x - drag.last.x) / (pos.x - drag.last.x)
                            } else {
                                1.0
                            };
                            let y = drag.last.y + (pos.y - drag.last.y) * t.clamp(0.0, 1.0);
                            set_value(lane, n, v_of(y));
                        }
                    }
                }
                DragKind::Values { line: true } => {
                    let (a, b) = (drag.origin.x.min(pos.x), drag.origin.x.max(pos.x));
                    for (i, n) in edited.iter_mut().enumerate() {
                        let x = frame.x(scene.origin + n.start);
                        if x >= a && x <= b && picked(i, &self.selected) {
                            let t = if (pos.x - drag.origin.x).abs() > 0.5 {
                                (x - drag.origin.x) / (pos.x - drag.origin.x)
                            } else {
                                1.0
                            };
                            let y = drag.origin.y + (pos.y - drag.origin.y) * t;
                            set_value(lane, n, v_of(y));
                        }
                    }
                    painter.line_segment([drag.origin, pos], Stroke::new(1.5, p.accent));
                }
                _ => {}
            }
            drag.last = pos;
            self.preview = Some(edited.clone());
            scene.notes = edited;
            self.drag = Some(drag);
        }
        if response.drag_stopped()
            && let Some(drag) = self.drag.take()
        {
            self.finish_lane(drag);
        }
        if response.clicked()
            && let Some(pos) = pointer
            && let Some(i) = stem_at(&scene.notes, pos).or_else(|| {
                scene
                    .notes
                    .iter()
                    .position(|n| (frame.x(scene.origin + n.start) - pos.x).abs() < 4.0)
            })
        {
            let mut edited = scene.notes.clone();
            set_value(lane, &mut edited[i], v_of(pos.y));
            self.commit(lane.name(), edited);
        }
        response.context_menu(|ui| self.values_menu(ui, scene));
    }

    fn finish_lane(&mut self, drag: Drag) {
        if let Some(edited) = self.preview.take()
            && edited != drag.original
        {
            self.commit(self.lane.name(), edited);
        }
    }

    /// Quick shapes for the velocities (or chances) of the selected notes, or all.
    fn values_menu(&mut self, ui: &mut Ui, scene: &Scene) {
        let lane = self.lane;
        let chosen = self.chosen(scene.notes.len());
        let apply = |this: &mut Self, ui: &mut Ui, label: &str, f: &dyn Fn(&mut Vec<Note>)| {
            if ui.button(label).clicked() {
                let mut notes = scene.notes.clone();
                f(&mut notes);
                this.commit(lane.name(), notes);
                ui.close();
            }
        };
        if lane == Lane::Probability {
            apply(self, ui, tr("Always play"), &|n| {
                chosen.iter().for_each(|&i| n[i].probability = 100)
            });
            apply(self, ui, tr("Half the time"), &|n| {
                chosen.iter().for_each(|&i| n[i].probability = 50)
            });
            apply(self, ui, tr("Random chances (50 to 100 %)"), &|n| {
                let seed = n.len() as u32 + 11;
                ops::randomize(n, &chosen, 0.0, 50, seed)
            });
            return;
        }
        apply(self, ui, tr("Crescendo"), &|n| {
            ops::velocity(n, &chosen, ops::Velocity::Ramp(40, 120, 1.0))
        });
        apply(self, ui, tr("Decrescendo"), &|n| {
            ops::velocity(n, &chosen, ops::Velocity::Ramp(120, 40, 1.0))
        });
        apply(self, ui, tr("Compress (closer together)"), &|n| {
            ops::velocity(n, &chosen, ops::Velocity::Contrast(0.6))
        });
        apply(self, ui, tr("Expand (further apart)"), &|n| {
            ops::velocity(n, &chosen, ops::Velocity::Contrast(1.5))
        });
        apply(self, ui, tr("Randomize slightly"), &|n| {
            let seed = n.len() as u32 + 3;
            ops::randomize(n, &chosen, 12.0, 100, seed)
        });
        apply(self, ui, tr("Accent the beats"), &|n| {
            let timing = scene.timing();
            for &i in &chosen {
                let beat = timing.beat(n[i].start);
                let on = (beat - beat.round()).abs() < 0.02;
                n[i].velocity = if on { 115 } else { 80 };
            }
        });
        apply(self, ui, tr("All at 100"), &|n| {
            ops::velocity(n, &chosen, ops::Velocity::Set(100))
        });
    }

    /// A controller curve: click adds a point, drag draws, Shift-drag a straight ramp,
    /// double-click removes a point.
    fn controller_lane(
        &mut self,
        ui: &mut Ui,
        scene: &Scene,
        rect: Rect,
        inner: Rect,
        area: Rect,
        controller: u16,
    ) {
        let p = theme::current(ui);
        let frame = self.frame(Rect::from_x_y_ranges(area.x_range(), rect.y_range()));
        let painter = ui.painter().with_clip_rect(rect);
        let expression = matches!(self.lane, Lane::Expression(_));
        let y_of = |v: f32| inner.bottom() - inner.height() * v.clamp(0.0, 1.0);
        let v_of = |y: f32| ((inner.bottom() - y) / inner.height()).clamp(0.0, 1.0);
        // Per-note expression: the selected note alone, on its channel and within its span.
        let note = (expression && self.selected.len() == 1)
            .then(|| {
                self.selected
                    .first()
                    .and_then(|&i| scene.notes.get(i))
                    .copied()
            })
            .flatten();
        if expression && note.is_none() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                tr("Select a note to shape its own expression"),
                FontId::proportional(11.0),
                p.weak,
            );
            return;
        }
        let channel = note.map_or_else(|| self.lane_channel(&scene.notes), |n| n.channel);
        let span = note.map(|n| (n.start, n.start + n.length));
        if let Some((a, b)) = span {
            let band = Rect::from_x_y_ranges(
                frame.x(scene.origin + a)..=frame.x(scene.origin + b),
                rect.y_range(),
            );
            painter.rect_filled(band, 0.0, scene.color.gamma_multiply(0.12));
        }
        if controller == PITCH_BEND {
            let y = y_of(0.5);
            painter.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(1.0, p.weak.gamma_multiply(0.4)),
            );
        }
        let points: Vec<ControlPoint> = match &self.drawing {
            Some((points, _)) => points.clone(),
            None => scene
                .clip
                .controls
                .as_ref()
                .map(|c| (**c).clone())
                .unwrap_or_default(),
        };
        let mine: Vec<&ControlPoint> = points
            .iter()
            .filter(|c| c.controller == controller && c.channel == channel)
            .collect();
        let color = scene.color;
        let mut shape: Vec<Pos2> = Vec::new();
        for (i, c) in mine.iter().enumerate() {
            let x = frame.x(scene.origin + c.time);
            let y = y_of(c.value);
            let next = mine
                .get(i + 1)
                .map_or(rect.right(), |n| frame.x(scene.origin + n.time));
            shape.push(pos2(x, y));
            shape.push(pos2(next, y));
        }
        if !shape.is_empty() {
            let mut fill = shape.clone();
            fill.push(pos2(rect.right(), inner.bottom()));
            fill.push(pos2(shape[0].x, inner.bottom()));
            painter.add(Shape::line(shape.clone(), Stroke::new(2.0, color)));
            for c in &mine {
                painter.circle_filled(
                    pos2(frame.x(scene.origin + c.time), y_of(c.value)),
                    3.0,
                    color,
                );
            }
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with("piano-controller"),
            Sense::click_and_drag(),
        );
        let modifiers = ui.input(|i| i.modifiers);
        let pointer = response.interact_pointer_pos();
        let clip_time = |x: f32| -> u64 {
            let t = frame.t(x).saturating_sub(scene.origin);
            match span {
                Some((a, b)) => t.clamp(a, b),
                None => t,
            }
        };
        if response.drag_started()
            && let Some(pos) = pointer
        {
            self.drawing = Some((points.clone(), clip_time(pos.x)));
            self.drag = Some(Drag::new(
                DragKind::Values {
                    line: modifiers.shift,
                },
                pos,
                Vec::new(),
            ));
        }
        if response.dragged()
            && let Some(pos) = pointer
        {
            let line = self
                .drag
                .as_ref()
                .is_some_and(|d| d.kind == DragKind::Values { line: true });
            let start = self.drag.as_ref().map_or(pos, |d| d.origin);
            let step = self
                .step(&scene.project, scene.origin + clip_time(start.x.min(pos.x)))
                .max(2)
                / 2;
            if let Some((drawn, last)) = &mut self.drawing {
                let t = clip_time(pos.x);
                if line {
                    // A straight ramp from the press, a point per grid step.
                    let mut ramp = points.clone();
                    let (ta, tb) = (clip_time(start.x), t);
                    let (a, b) = (ta.min(tb), ta.max(tb));
                    ramp.retain(|c| {
                        c.controller != controller
                            || c.channel != channel
                            || c.time < a
                            || c.time > b
                    });
                    let mut at = a;
                    while at <= b {
                        let f = if tb != ta {
                            (at as f64 - ta as f64) / (tb as f64 - ta as f64)
                        } else {
                            1.0
                        };
                        let v = v_of(start.y) + (v_of(pos.y) - v_of(start.y)) * f as f32;
                        ramp.push(ControlPoint {
                            time: at,
                            controller,
                            value: v,
                            channel,
                        });
                        at += step.max(1);
                    }
                    ramp.sort_by_key(|c| (c.time, c.controller));
                    *drawn = ramp;
                    painter.line_segment([start, pos], Stroke::new(1.5, p.accent));
                } else {
                    let (a, b) = ((*last).min(t), (*last).max(t));
                    drawn.retain(|c| {
                        c.controller != controller
                            || c.channel != channel
                            || c.time < a
                            || c.time > b
                    });
                    drawn.push(ControlPoint {
                        time: t,
                        controller,
                        value: v_of(pos.y),
                        channel,
                    });
                    drawn.sort_by_key(|c| (c.time, c.controller));
                    *last = t;
                }
            }
        }
        if response.drag_stopped() {
            self.drag = None;
            if let Some((drawn, _)) = self.drawing.take()
                && let Some(id) = self.clip
            {
                self.doc
                    .borrow_mut()
                    .history_mut()
                    .edit(tr("Draw controller"), |p| edit::set_controls(p, id, drawn));
            }
        }
        let nearest = |pos: Pos2| {
            points.iter().position(|c| {
                c.controller == controller
                    && c.channel == channel
                    && (frame.x(scene.origin + c.time) - pos.x).abs() < 6.0
                    && (y_of(c.value) - pos.y).abs() < 8.0
            })
        };
        if response.double_clicked()
            && let Some(i) = pointer.and_then(nearest)
            && let Some(id) = self.clip
        {
            let mut edited = points.clone();
            edited.remove(i);
            self.doc
                .borrow_mut()
                .history_mut()
                .edit(tr("Delete controller point"), |p| {
                    edit::set_controls(p, id, edited)
                });
        } else if response.clicked()
            && let Some(pos) = pointer
            && let Some(id) = self.clip
        {
            let t = self
                .snap(&scene.project, scene.origin + clip_time(pos.x), false)
                .saturating_sub(scene.origin);
            let t = span.map_or(t, |(a, b)| t.clamp(a, b));
            let mut edited = points.clone();
            edited.retain(|c| c.controller != controller || c.channel != channel || c.time != t);
            edited.push(ControlPoint {
                time: t,
                controller,
                value: v_of(pos.y),
                channel,
            });
            self.doc
                .borrow_mut()
                .history_mut()
                .edit(tr("Add controller point"), |p| {
                    edit::set_controls(p, id, edited)
                });
        }
        if let Some(pos) = response.hover_pos() {
            let v = v_of(pos.y);
            let text = if controller == PITCH_BEND {
                format!("{:+.0}%", (v - 0.5) * 200.0)
            } else {
                format!("{}", (v * 127.0).round())
            };
            painter.text(
                pos2(pos.x + 8.0, pos.y - 8.0),
                Align2::LEFT_BOTTOM,
                text,
                FontId::proportional(10.0),
                p.text,
            );
        }
    }
}
