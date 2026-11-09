//! The piano roll's canvas: grid, notes, ghosts, ruler, chord track, keyboard, playhead, and
//! what the tools do on the notes.

use audiostrada_core::Note;
use audiostrada_midi::key_name;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    pos2, vec2,
};

use super::music::chord_track;
use super::ops::{self, MIN_LENGTH};
use super::view::Frame;
use super::{AUDITION, Drag, DragKind, Lane, PianoRoll, Scene, Tool};
use crate::i18n::tr;
use crate::theme;

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// Which part of a note is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    Body,
    Start,
    End,
}

/// The fill of a note: the track's color, darker for soft notes and lighter for loud ones.
pub(super) fn note_fill(color: Color32, velocity: u8, selected: bool) -> Color32 {
    let v = f32::from(velocity) / 127.0;
    let dark = color.gamma_multiply(0.42);
    let fill = theme::mix(dark, color, 0.35 + 0.65 * v);
    let fill = theme::mix(fill, Color32::WHITE, 0.12 * v);
    if selected {
        theme::mix(fill, Color32::WHITE, 0.28)
    } else {
        fill
    }
}

impl PianoRoll {
    pub(super) fn frame(&self, area: Rect) -> Frame {
        Frame {
            view: self.view.clone(),
            area,
        }
    }

    /// The note and part under `pos`, topmost (latest) first.
    fn hit(&self, scene: &Scene, area: Rect, pos: Pos2) -> Option<(usize, Part)> {
        let frame = self.frame(area);
        scene.notes.iter().enumerate().rev().find_map(|(i, n)| {
            let r = frame.note_rect(
                scene.origin + n.start,
                scene.origin + n.start + n.length,
                n.key,
            )?;
            if !r.expand2(vec2(2.0, 0.0)).contains(pos) {
                return None;
            }
            let edge = (r.width() / 4.0).clamp(2.0, 7.0);
            let part = if pos.x > r.right() - edge {
                Part::End
            } else if pos.x < r.left() + edge && r.width() > 12.0 {
                Part::Start
            } else {
                Part::Body
            };
            Some((i, part))
        })
    }

    /// The clip of the ghost note under `pos`, if any.
    fn ghost_at(&self, scene: &Scene, area: Rect, pos: Pos2) -> Option<audiostrada_core::ClipId> {
        let frame = self.frame(area);
        let (t, key) = (frame.t(pos.x), frame.key(pos.y));
        scene
            .project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find_map(|clip| {
                let notes = clip.notes.as_ref()?;
                if clip.id == scene.clip.id || t < clip.start || t >= clip.end() {
                    return None;
                }
                let origin = clip.start.saturating_sub(clip.offset);
                notes
                    .iter()
                    .any(|n| {
                        n.key == key && origin + n.start <= t && t < origin + n.start + n.length
                    })
                    .then_some(clip.id)
            })
    }

    pub(super) fn draw_grid(&self, ui: &Ui, scene: &Scene, area: Rect) {
        let p = theme::current(ui);
        let frame = self.frame(area);
        let painter = ui.painter().with_clip_rect(area);
        let white = theme::mix(p.base, p.panel, 0.95);
        let black = theme::mix(p.base, p.panel, 0.35);
        let height = self.view.key_height;
        for (row, &key) in self.view.rows.iter().enumerate() {
            let y = frame.row_top(row);
            if y + height < area.top() || y > area.bottom() {
                continue;
            }
            let r = Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), height));
            let mut fill = if is_black(key) { black } else { white };
            if let Some(scale) = self.scale {
                if !scale.contains(key) {
                    fill = theme::mix(fill, p.base, 0.7);
                } else if key % 12 == scale.root % 12 {
                    fill = theme::mix(fill, p.accent, 0.12);
                }
            }
            painter.rect_filled(r, 0.0, fill);
            if key % 12 == 0 || self.fold {
                painter.line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    Stroke::new(
                        1.0,
                        p.border
                            .gamma_multiply(if key % 12 == 0 { 1.0 } else { 0.4 }),
                    ),
                );
            }
        }
        // Bars, beats and grid lines, the finer ones only when far enough apart.
        let (tempo, rate) = (&scene.project.tempo, scene.project.sample_rate);
        let first = tempo
            .sample_to_beat(frame.t(area.left()), rate)
            .floor()
            .max(0.0);
        let last = tempo.sample_to_beat(frame.t(area.right()), rate).ceil();
        let px_per_beat = (tempo.beat_to_sample(first + 1.0, rate)
            - tempo.beat_to_sample(first, rate)) as f32
            / self.view.zoom as f32;
        let mut step = self.grid.beats().unwrap_or(1.0).min(1.0);
        while px_per_beat * step as f32 * 1.0 < 7.0 && step < 4.0 {
            step *= 2.0;
        }
        let mut beat = first;
        let mut guard = 0;
        while beat <= last && guard < 6000 {
            guard += 1;
            let x = frame.x(tempo.beat_to_sample(beat, rate));
            let bbt = tempo.beat_to_bbt(beat);
            let alpha = if bbt.beat == 1 && bbt.tick == 0 && (beat.fract()).abs() < 1e-9 {
                0.30
            } else if (beat.fract()).abs() < 1e-9 {
                0.13
            } else {
                0.05
            };
            painter.line_segment(
                [pos2(x, area.top()), pos2(x, area.bottom())],
                Stroke::new(1.0, p.text.gamma_multiply(alpha)),
            );
            beat += step;
        }
        // Outside the clip.
        let shade = Color32::from_black_alpha(if p.dark { 80 } else { 30 });
        let (start, end) = (frame.x(scene.clip.start), frame.x(scene.clip.end()));
        painter.rect_filled(
            Rect::from_min_max(area.min, pos2(start, area.bottom())),
            0.0,
            shade,
        );
        painter.rect_filled(
            Rect::from_min_max(pos2(end, area.top()), area.max),
            0.0,
            shade,
        );
        if let Some((a, b)) = scene.project.looping() {
            let band = Rect::from_x_y_ranges(frame.x(a)..=frame.x(b), area.y_range());
            painter.rect_filled(band, 0.0, p.accent.gamma_multiply(0.035));
        }
    }

    /// The notes of the other MIDI clips in view, faint; a double-click edits their clip.
    pub(super) fn draw_ghosts(&self, ui: &Ui, scene: &Scene, area: Rect) {
        let frame = self.frame(area);
        let painter = ui.painter().with_clip_rect(area);
        let (t0, t1) = (frame.t(area.left()), frame.t(area.right()));
        for track in &scene.project.tracks {
            let color = crate::theme::track_color(track.color);
            for clip in &track.clips {
                let Some(notes) = &clip.notes else { continue };
                if clip.id == scene.clip.id || clip.end() < t0 || clip.start > t1 {
                    continue;
                }
                let origin = clip.start.saturating_sub(clip.offset);
                for n in notes.iter() {
                    let (a, b) = (
                        origin + n.start,
                        (origin + n.start + n.length).min(clip.end()),
                    );
                    if b < t0 || a > t1 || a < clip.start {
                        continue;
                    }
                    if let Some(r) = frame.note_rect(a, b, n.key) {
                        painter.rect_filled(r, 2.0, color.gamma_multiply(0.10));
                        painter.rect_stroke(
                            r,
                            2.0,
                            Stroke::new(1.0, color.gamma_multiply(0.35)),
                            StrokeKind::Inside,
                        );
                    }
                }
            }
        }
    }

    pub(super) fn draw_notes(&self, ui: &Ui, scene: &Scene, area: Rect) {
        let p = theme::current(ui);
        let frame = self.frame(area);
        let painter = ui.painter().with_clip_rect(area);
        let (t0, t1) = (frame.t(area.left()), frame.t(area.right()));
        let expression = match self.lane {
            Lane::Expression(c) => Some(c),
            _ => None,
        };
        let controls = scene
            .clip
            .controls
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let hovered = pointer
            .and_then(|pos| self.hit(scene, area, pos))
            .filter(|_| self.drag.is_none());
        // Unselected first, so that the selection is on top.
        let mut order: Vec<usize> = (0..scene.notes.len()).collect();
        let mask = self.mask(scene.notes.len());
        order.sort_by_key(|&i| mask[i]);
        for i in order {
            let n = &scene.notes[i];
            let (a, b) = (scene.origin + n.start, scene.origin + n.start + n.length);
            if b < t0 || a > t1 {
                continue;
            }
            let Some(r) = frame.note_rect(a, b, n.key) else {
                continue;
            };
            if r.bottom() < area.top() || r.top() > area.bottom() {
                continue;
            }
            let selected = mask[i];
            let mut fill = note_fill(scene.color, n.velocity, selected);
            if n.muted {
                fill = theme::mix(p.surface, scene.color, 0.15).gamma_multiply(0.7);
            }
            let radius = (r.height() * 0.3).min(4.0);
            painter.rect_filled(r, radius, fill);
            // A thin highlight on top: the note reads as a solid object.
            if r.height() >= 8.0 && !n.muted {
                painter.line_segment(
                    [
                        pos2(r.left() + radius, r.top() + 1.0),
                        pos2(r.right() - radius, r.top() + 1.0),
                    ],
                    Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.18)),
                );
            }
            let outline = if selected {
                Stroke::new(1.5, Color32::WHITE)
            } else if hovered.is_some_and(|(h, _)| h == i) {
                Stroke::new(1.0, p.text.gamma_multiply(0.9))
            } else {
                Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.45))
            };
            if n.probability < 100 || n.muted {
                let corners = [
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ];
                painter.extend(Shape::dashed_line(&corners, outline, 3.0, 2.0));
            } else {
                painter.rect_stroke(r, radius, outline, StrokeKind::Inside);
            }
            if let Some((h, part)) = hovered
                && h == i
                && self.tool == Tool::Pointer
                && part != Part::Body
            {
                let x = if part == Part::End {
                    r.right() - 1.5
                } else {
                    r.left() + 1.5
                };
                painter.line_segment(
                    [pos2(x, r.top() + 1.0), pos2(x, r.bottom() - 1.0)],
                    Stroke::new(2.5, Color32::WHITE),
                );
            }
            if let Some(controller) = expression {
                draw_expression(&painter, r, n, controller, controls, scene.origin, &frame);
            }
            // The note's name, and its chance when it may not play.
            if r.height() >= 9.0 && r.width() > 24.0 {
                let ink = theme::text_on(fill).gamma_multiply(if n.muted { 0.5 } else { 0.85 });
                let size = (r.height() * 0.62).clamp(8.0, 11.5);
                let mut label = key_name(n.key);
                if n.probability < 100 && r.width() > 60.0 {
                    label.push_str(&format!("  {}%", n.probability));
                }
                painter.with_clip_rect(r.shrink(1.0).intersect(area)).text(
                    pos2(r.left() + 4.0, r.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(size),
                    ink,
                );
            }
        }
    }

    pub(super) fn draw_ruler(&self, ui: &Ui, scene: &Scene, ruler: Rect) {
        let p = theme::current(ui);
        let frame = self.frame(Rect::from_x_y_ranges(ruler.x_range(), ruler.y_range()));
        let painter = ui.painter().with_clip_rect(ruler);
        painter.rect_filled(ruler, 0.0, p.panel);
        let (tempo, rate) = (&scene.project.tempo, scene.project.sample_rate);
        if let Some((a, b)) = scene.project.looping() {
            let band = Rect::from_min_max(
                pos2(frame.x(a), ruler.bottom() - 5.0),
                pos2(frame.x(b), ruler.bottom() - 1.0),
            );
            painter.rect_filled(band, 2.0, p.accent.gamma_multiply(0.7));
        }
        let clip = Rect::from_min_max(
            pos2(frame.x(scene.clip.start), ruler.top() + 1.0),
            pos2(frame.x(scene.clip.end()), ruler.top() + 3.0),
        );
        painter.rect_filled(clip, 1.0, scene.color);
        let first = tempo
            .sample_to_beat(frame.t(ruler.left()), rate)
            .floor()
            .max(0.0);
        let last = tempo.sample_to_beat(frame.t(ruler.right()), rate).ceil();
        let px_per_beat = (tempo.beat_to_sample(first + 1.0, rate)
            - tempo.beat_to_sample(first, rate)) as f32
            / self.view.zoom as f32;
        let mut beat = first;
        let mut guard = 0;
        while beat <= last && guard < 4000 {
            guard += 1;
            let x = frame.x(tempo.beat_to_sample(beat, rate));
            let bbt = tempo.beat_to_bbt(beat);
            let bar = bbt.beat == 1;
            let tick = if bar { 9.0 } else { 4.0 };
            painter.line_segment(
                [pos2(x, ruler.bottom() - tick), pos2(x, ruler.bottom())],
                Stroke::new(1.0, p.weak.gamma_multiply(if bar { 0.9 } else { 0.5 })),
            );
            let every = if px_per_beat * 4.0 > 60.0 {
                1
            } else {
                ((60.0 / (px_per_beat * 4.0)).ceil() as u32).next_power_of_two()
            };
            if bar && (bbt.bar.saturating_sub(1)) % every == 0 {
                painter.text(
                    pos2(x + 3.0, ruler.top() + 4.0),
                    Align2::LEFT_TOP,
                    bbt.bar.to_string(),
                    FontId::proportional(10.5),
                    p.text,
                );
            } else if px_per_beat > 70.0 {
                painter.text(
                    pos2(x + 2.0, ruler.top() + 6.0),
                    Align2::LEFT_TOP,
                    format!("{}.{}", bbt.bar, bbt.beat),
                    FontId::proportional(9.0),
                    p.weak,
                );
            }
            beat += 1.0;
        }
        // A click or drag in the ruler moves the playhead.
        let response = ui.interact(
            ruler,
            ui.scope_id().with("piano-ruler"),
            Sense::click_and_drag(),
        );
        if (response.clicked() || response.dragged())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let t = self.snap(
                &scene.project,
                frame.t(pos.x),
                ui.input(|i| i.modifiers.command),
            );
            self.doc.borrow_mut().player_mut().locate(t);
        }
        response.on_hover_text(tr("Click or drag to move the playhead"));
    }

    /// The chords the notes make, named over time.
    pub(super) fn draw_chords(&self, ui: &Ui, scene: &Scene, rect: Rect) {
        let p = theme::current(ui);
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, theme::mix(p.base, p.panel, 0.5));
        let frame = self.frame(rect);
        let eighth = scene.timing().time(scene.timing().beat(0) + 0.45);
        let (t0, t1) = (
            frame.t(rect.left()).saturating_sub(scene.origin),
            frame.t(rect.right()).saturating_sub(scene.origin),
        );
        let visible: Vec<Note> = scene
            .notes
            .iter()
            .filter(|n| n.start < t1 && n.start + n.length > t0)
            .copied()
            .collect();
        for (a, b, name) in chord_track(&visible, eighth) {
            let r = Rect::from_min_max(
                pos2(frame.x(scene.origin + a) + 1.0, rect.top() + 2.0),
                pos2(frame.x(scene.origin + b) - 1.0, rect.bottom() - 2.0),
            );
            if r.right() < rect.left() || r.left() > rect.right() || r.width() < 6.0 {
                continue;
            }
            painter.rect_filled(r, 4.0, scene.color.gamma_multiply(0.22));
            painter.rect_stroke(
                r,
                4.0,
                Stroke::new(1.0, scene.color.gamma_multiply(0.5)),
                StrokeKind::Inside,
            );
            painter.with_clip_rect(r.intersect(rect)).text(
                pos2(r.left().max(rect.left()) + 5.0, r.center().y),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(10.5),
                p.text,
            );
        }
    }

    pub(super) fn draw_corner(&self, ui: &Ui, corner: Rect) {
        let p = theme::current(ui);
        ui.painter().rect_filled(corner, 0.0, p.panel);
        ui.painter().text(
            pos2(
                corner.right() - 6.0,
                corner.top() + super::CHORDS_HEIGHT / 2.0,
            ),
            Align2::RIGHT_CENTER,
            tr("Chords"),
            FontId::proportional(9.5),
            p.weak,
        );
        ui.painter().text(
            pos2(
                corner.right() - 6.0,
                corner.bottom() - super::RULER_HEIGHT / 2.0,
            ),
            Align2::RIGHT_CENTER,
            self.grid.name(),
            FontId::proportional(9.5),
            p.weak,
        );
    }

    /// A piano along the rows: the keys played light up, the scale's are marked.
    pub(super) fn keyboard(&mut self, ui: &Ui, scene: &Scene, keys: Rect, area: Rect) {
        let p = theme::current(ui);
        let frame = self.frame(area);
        let painter = ui.painter().with_clip_rect(keys);
        let height = self.view.key_height;
        let ivory = if p.dark {
            Color32::from_gray(218)
        } else {
            Color32::from_gray(250)
        };
        let ebony = Color32::from_gray(if p.dark { 26 } else { 40 });
        let lit = p.accent;
        painter.rect_filled(keys, 0.0, ivory);
        // Keys sounding at the playhead while playing, or pressed.
        let position = self.doc.borrow().player().position();
        let playing = self.doc.borrow().player().engine().playing();
        let sounding: Vec<u8> = scene
            .notes
            .iter()
            .filter(|n| {
                playing
                    && !n.muted
                    && scene.origin + n.start <= position
                    && position < scene.origin + n.start + n.length
            })
            .map(|n| n.key)
            .chain(self.sounding.map(|(_, k)| k))
            .collect();
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|pos| keys.contains(*pos) || area.contains(*pos))
            .map(|pos| frame.key(pos.y));
        for (row, &key) in self.view.rows.iter().enumerate() {
            let y = frame.row_top(row);
            if y + height < keys.top() || y > keys.bottom() {
                continue;
            }
            let r = Rect::from_min_size(pos2(keys.left(), y), vec2(keys.width(), height));
            let black = is_black(key);
            let on = sounding.contains(&key);
            if self.fold {
                // Folded keys are not next to each other: each row is its own key.
                let fill = if on {
                    lit
                } else if black {
                    ebony
                } else {
                    ivory
                };
                painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 2.0, fill);
            } else if black {
                let k = Rect::from_min_max(r.min, pos2(r.left() + keys.width() * 0.6, r.bottom()));
                // The line between the two white keys on either side.
                painter.line_segment(
                    [pos2(k.right(), r.center().y), pos2(r.right(), r.center().y)],
                    Stroke::new(1.0, Color32::from_gray(150)),
                );
                let fill = if on { lit } else { ebony };
                painter.rect_filled(
                    k.shrink2(vec2(0.0, 0.5)),
                    egui::CornerRadius {
                        ne: 3,
                        se: 3,
                        ..Default::default()
                    },
                    fill,
                );
                painter.line_segment(
                    [
                        pos2(k.left(), k.top() + 1.5),
                        pos2(k.right() - 3.0, k.top() + 1.5),
                    ],
                    Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.12)),
                );
            } else {
                if on {
                    painter.rect_filled(r, 0.0, lit.gamma_multiply(0.8));
                }
                if matches!(key % 12, 0 | 5) {
                    painter.line_segment(
                        [r.left_bottom(), r.right_bottom()],
                        Stroke::new(1.0, Color32::from_gray(150)),
                    );
                }
            }
            if hover == Some(key) {
                painter.rect_filled(r, 0.0, p.accent.gamma_multiply(0.25));
            }
            if let Some(scale) = self.scale
                && scale.contains(key)
            {
                let dot = pos2(r.right() - 5.0, r.center().y);
                let root = key % 12 == scale.root % 12;
                painter.circle_filled(
                    dot,
                    if root { 2.6 } else { 1.6 },
                    p.accent.gamma_multiply(if root { 1.0 } else { 0.6 }),
                );
            }
            let named = key % 12 == 0 || self.fold || hover == Some(key) || height >= 18.0;
            if named && height >= 8.0 {
                let ink = if black {
                    Color32::from_gray(200)
                } else {
                    Color32::from_gray(60)
                };
                let x = if black && !self.fold {
                    r.left() + keys.width() * 0.6 - 4.0
                } else {
                    r.right() - 11.0
                };
                painter.text(
                    pos2(x, r.center().y),
                    Align2::RIGHT_CENTER,
                    key_name(key),
                    FontId::proportional((height * 0.6).clamp(8.0, 11.0)),
                    ink,
                );
            }
        }
        painter.line_segment(
            [keys.right_top(), keys.right_bottom()],
            Stroke::new(1.0, p.border),
        );
    }

    pub(super) fn draw_playhead(&self, ui: &Ui, scene: &Scene, rect: Rect) {
        let p = theme::current(ui);
        let frame = self.frame(rect);
        let position = self.doc.borrow().player().position();
        let x = frame.x(position);
        if x < rect.left() || x > rect.right() {
            return;
        }
        let color = if self.doc.borrow().player().engine().playing() {
            theme::PLAY
        } else {
            p.text.gamma_multiply(0.7)
        };
        let painter = ui.painter().with_clip_rect(rect);
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, color),
        );
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x - 5.0, rect.top()),
                pos2(x + 5.0, rect.top()),
                pos2(x, rect.top() + 6.0),
            ],
            color,
            Stroke::NONE,
        ));
        let _ = scene;
    }

    /// Notes for a click or a stamp at `start` (clip samples) on `key`: one, or a chord.
    fn stamp(&self, start: u64, length: u64, key: u8, velocity: u8) -> Vec<Note> {
        let keys = if self.stamp.on {
            self.stamp.keys(key, self.scale)
        } else {
            vec![self.in_scale(key)]
        };
        keys.into_iter()
            .map(|key| Note {
                start,
                length,
                key,
                velocity,
                ..Default::default()
            })
            .collect()
    }

    /// Velocity for new notes: that of the selected note, else of the last one, else 100.
    fn new_velocity(&self, notes: &[Note]) -> u8 {
        self.selected
            .first()
            .and_then(|&i| notes.get(i))
            .map_or(100, |n| n.velocity)
    }

    pub(super) fn interact(&mut self, ui: &mut Ui, scene: &Scene, area: Rect) {
        let response = ui.interact(
            area,
            ui.scope_id().with("piano-notes"),
            Sense::click_and_drag(),
        );
        if ui.input(|i| i.pointer.any_pressed()) {
            self.focused = ui.rect_contains_pointer(area.expand2(vec2(super::KEYS_WIDTH, 200.0)));
        }
        let project = &scene.project;
        let origin = scene.origin;
        let notes = &scene.notes;
        let (modifiers, pointer) = ui.input(|i| (i.modifiers, i.pointer.hover_pos()));
        let free = modifiers.command;
        let hovered = pointer
            .filter(|p| area.contains(*p))
            .and_then(|pos| self.hit(scene, area, pos));
        self.hovered = hovered.map(|h| h.0);

        // Pointer and feedback before a drag: the cursor, a ghost of the note to add, the cut.
        if self.drag.is_none()
            && let Some(pos) = response.hover_pos()
        {
            let frame = self.frame(area);
            let cursor = match (self.tool, hovered) {
                (Tool::Pointer | Tool::Draw, Some((_, Part::Start | Part::End))) => {
                    CursorIcon::ResizeHorizontal
                }
                (Tool::Pointer | Tool::Draw, Some(_)) => CursorIcon::Grab,
                (Tool::Pointer, None) => CursorIcon::Default,
                (Tool::Erase, _) => CursorIcon::Crosshair,
                (Tool::Slice, _) => CursorIcon::Crosshair,
                _ => CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(cursor);
            let painter = ui.painter().with_clip_rect(area);
            match (self.tool, hovered) {
                (Tool::Draw | Tool::Paint, None) => {
                    let at = self
                        .snap(
                            project,
                            frame
                                .t(pos.x)
                                .saturating_sub(self.step(project, frame.t(pos.x)) / 2),
                            free,
                        )
                        .max(origin);
                    let length = self.last_length.unwrap_or_else(|| self.step(project, at));
                    for n in self.stamp(at - origin, length, frame.key(pos.y), 100) {
                        if let Some(r) =
                            frame.note_rect(origin + n.start, origin + n.start + n.length, n.key)
                        {
                            painter.rect_filled(r, 3.0, scene.color.gamma_multiply(0.25));
                            painter.rect_stroke(
                                r,
                                3.0,
                                Stroke::new(1.0, scene.color.gamma_multiply(0.8)),
                                StrokeKind::Inside,
                            );
                        }
                    }
                }
                (Tool::Slice, Some((i, _))) => {
                    let t = self.snap(project, frame.t(pos.x), free);
                    let x = frame.x(t);
                    if let Some(r) = notes.get(i).and_then(|n| {
                        frame.note_rect(origin + n.start, origin + n.start + n.length, n.key)
                    }) {
                        painter.line_segment(
                            [pos2(x, r.top() - 3.0), pos2(x, r.bottom() + 3.0)],
                            Stroke::new(2.0, Color32::WHITE),
                        );
                    }
                }
                (Tool::Erase, Some((i, _))) => {
                    if let Some(r) = notes.get(i).and_then(|n| {
                        frame.note_rect(origin + n.start, origin + n.start + n.length, n.key)
                    }) {
                        painter.rect_stroke(
                            r,
                            3.0,
                            Stroke::new(2.0, theme::RECORD),
                            StrokeKind::Outside,
                        );
                    }
                }
                _ => {}
            }
            paint_tool_badge(ui, pos, self.tool);
        }

        // A drag does what was under the press.
        let press = ui.input(|i| i.pointer.press_origin());
        if response.drag_started()
            && let Some(pos) = press.or(response.interact_pointer_pos())
        {
            let under = self.hit(scene, area, pos);
            let frame = self.frame(area);
            let mut drag = Drag::new(DragKind::Select { anchor: pos }, pos, notes.clone());
            match (self.tool, under) {
                (Tool::Pointer | Tool::Draw, Some((i, part)))
                | (Tool::Paint, Some((i, part @ (Part::Start | Part::End)))) => {
                    if !self.selected.contains(&i) {
                        if !modifiers.shift {
                            self.selected.clear();
                        }
                        self.selected.push(i);
                    }
                    drag.grabbed = notes.get(i).copied();
                    drag.kind = match part {
                        Part::End => DragKind::Resize,
                        Part::Start => DragKind::ResizeStart,
                        Part::Body => {
                            if modifiers.alt {
                                // Alt-drag moves copies and leaves the notes where they are.
                                let first = drag.original.len();
                                let copies: Vec<Note> = self
                                    .selected
                                    .iter()
                                    .filter_map(|&i| notes.get(i).copied())
                                    .collect();
                                drag.original.extend(copies);
                                self.selected = (first..drag.original.len()).collect();
                                drag.copy = true;
                            }
                            DragKind::Move
                        }
                    };
                }
                (Tool::Draw, None) => {
                    let at = self
                        .snap(
                            project,
                            frame
                                .t(pos.x)
                                .saturating_sub(self.step(project, frame.t(pos.x)) / 2),
                            free,
                        )
                        .max(origin);
                    let length = self.step(project, at);
                    let velocity = self.new_velocity(notes);
                    drag.painted = self.stamp(at - origin, length, frame.key(pos.y), velocity);
                    drag.kind = DragKind::Draw;
                }
                (Tool::Paint, _) => drag.kind = DragKind::Paint,
                (Tool::Erase, _) => drag.kind = DragKind::Erase,
                (Tool::Slice, _) => drag.kind = DragKind::Select { anchor: pos },
                (Tool::Pointer, None) => {
                    if modifiers.shift {
                        drag.base = self.selected.clone();
                    }
                }
            }
            self.drag = Some(drag);
        }

        if let Some(pos) = ui.input(|i| i.pointer.interact_pos())
            && self.drag.as_ref().is_some_and(|d| {
                !matches!(d.kind, DragKind::Values { .. } | DragKind::Value { .. })
            })
        {
            self.drag_notes(ui, scene, area, pos, free);
        }

        if response.drag_stopped()
            && let Some(drag) = self.drag.take()
        {
            self.finish(drag);
        }

        // Clicks.
        let frame = self.frame(area);
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let under = self.hit(scene, area, pos);
            match (self.tool, under) {
                (Tool::Erase, Some((i, _))) => {
                    self.selected = vec![i];
                    self.delete_selected();
                }
                (Tool::Slice, Some((i, _))) => {
                    let at = self
                        .snap(project, frame.t(pos.x), free)
                        .saturating_sub(origin);
                    let targets = if self.selected.contains(&i) {
                        self.selected
                            .iter()
                            .copied()
                            .filter(|&k| {
                                notes
                                    .get(k)
                                    .is_some_and(|n| n.start < at && at < n.start + n.length)
                            })
                            .collect()
                    } else {
                        vec![i]
                    };
                    let mut edited = notes.clone();
                    let added = ops::slice(&mut edited, &targets, at);
                    self.selected = targets.into_iter().chain(added).collect();
                    self.commit(tr("Slice notes"), edited);
                }
                (_, Some((i, _))) => {
                    if modifiers.shift || modifiers.command {
                        if let Some(k) = self.selected.iter().position(|&s| s == i) {
                            self.selected.remove(k);
                        } else {
                            self.selected.push(i);
                        }
                    } else {
                        self.selected = vec![i];
                    }
                    if let Some(n) = notes.get(i) {
                        self.added = Some((n.key, ui.input(|i| i.time) + AUDITION));
                    }
                }
                (Tool::Draw | Tool::Paint, None) => self.add_at(ui, scene, area, pos, free),
                (Tool::Pointer, None) if !modifiers.shift => self.selected.clear(),
                _ => {}
            }
        }
        if response.double_clicked()
            && self.tool == Tool::Pointer
            && let Some(pos) = response.interact_pointer_pos()
        {
            match self.hit(scene, area, pos) {
                Some((i, _)) => {
                    self.selected = vec![i];
                    self.delete_selected();
                }
                None => match self
                    .ghosts
                    .then(|| self.ghost_at(scene, area, pos))
                    .flatten()
                {
                    Some(clip) => self.doc.borrow_mut().reveal_clip(clip),
                    None => self.add_at(ui, scene, area, pos, free),
                },
            }
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some((i, _)) = self.hit(scene, area, pos)
            && !self.selected.contains(&i)
        {
            self.selected = vec![i];
        }
        response.context_menu(|ui| self.context_menu(ui, scene));
    }

    /// Adds a note (or the stamped chord) at `pos`, selected, and lets it sound.
    fn add_at(&mut self, ui: &Ui, scene: &Scene, area: Rect, pos: Pos2, free: bool) {
        let frame = self.frame(area);
        let project = &scene.project;
        let at = self
            .snap(
                project,
                frame
                    .t(pos.x)
                    .saturating_sub(self.step(project, frame.t(pos.x)) / 2),
                free,
            )
            .max(scene.origin);
        let length = self.last_length.unwrap_or_else(|| self.step(project, at));
        let velocity = self.new_velocity(&scene.notes);
        let mut edited = scene.notes.clone();
        let first = edited.len();
        let stamped = self.stamp(at - scene.origin, length, frame.key(pos.y), velocity);
        let key = stamped.first().map_or(60, |n| n.key);
        edited.extend(stamped);
        self.selected = (first..edited.len()).collect();
        self.added = Some((key, ui.input(|i| i.time) + AUDITION));
        self.commit(
            if self.stamp.on {
                tr("Add chord")
            } else {
                tr("Add note")
            },
            edited,
        );
    }

    /// Follows the pointer during a drag on the notes.
    fn drag_notes(&mut self, ui: &Ui, scene: &Scene, area: Rect, pos: Pos2, free: bool) {
        let project = scene.project.clone();
        let origin = scene.origin;
        let zoom = self.view.zoom;
        let p = theme::current(ui);
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let frame = self.frame(area);
        let key_at = frame.key(pos.y);
        let t_at = frame.t(pos.x);
        let mut edited = drag.original.clone();
        let delta = pos - drag.origin;
        match drag.kind {
            DragKind::Move => {
                let samples = (f64::from(delta.x) * zoom) as i64;
                let grabbed = drag.grabbed.unwrap_or_default();
                let anchor = origin + grabbed.start;
                let target = self
                    .snap(&project, (anchor as i64 + samples).max(0) as u64, free)
                    .max(origin);
                let shift = target as i64 - anchor as i64;
                let keys = i32::from(key_at) - i32::from(grabbed.key);
                for &i in &self.selected {
                    if let Some(n) = edited.get_mut(i) {
                        n.start = (n.start as i64 + shift).max(0) as u64;
                        n.key = self.in_scale((i32::from(n.key) + keys).clamp(0, 127) as u8);
                    }
                }
            }
            DragKind::Resize => {
                let samples = (f64::from(delta.x) * zoom) as i64;
                for &i in &self.selected {
                    if let Some(n) = edited.get_mut(i) {
                        let end = self.snap(
                            &project,
                            (origin as i64 + (n.start + n.length) as i64 + samples).max(0) as u64,
                            free,
                        );
                        n.length = end.saturating_sub(origin + n.start).max(MIN_LENGTH);
                    }
                }
            }
            DragKind::ResizeStart => {
                let samples = (f64::from(delta.x) * zoom) as i64;
                for &i in &self.selected {
                    if let Some(n) = edited.get_mut(i) {
                        let end = n.start + n.length;
                        let start = self
                            .snap(
                                &project,
                                (origin as i64 + n.start as i64 + samples).max(origin as i64)
                                    as u64,
                                free,
                            )
                            .saturating_sub(origin)
                            .min(end.saturating_sub(MIN_LENGTH));
                        n.start = start;
                        n.length = end - start;
                    }
                }
            }
            DragKind::Draw => {
                let first = edited.len();
                let start = drag.painted.first().map_or(0, |n| n.start);
                let step = self.step(&project, origin + start);
                let end = self.snap(&project, t_at, free).saturating_sub(origin);
                let length = end
                    .saturating_sub(start)
                    .max(if free { MIN_LENGTH } else { step });
                for n in &mut drag.painted {
                    n.length = length;
                }
                edited.extend(drag.painted.iter().copied());
                self.selected = (first..edited.len()).collect();
            }
            DragKind::Paint => {
                let step = self.step(&project, t_at);
                let at = self
                    .snap(&project, t_at.saturating_sub(step / 2), free)
                    .max(origin)
                    - origin;
                let taken = drag.painted.iter().any(|n| n.start == at)
                    || drag
                        .original
                        .iter()
                        .any(|n| n.start == at && n.key == self.in_scale(key_at));
                if !taken {
                    let velocity = self.new_velocity(&drag.original);
                    let stamped = self.stamp(at, step, key_at, velocity);
                    drag.painted.extend(stamped);
                }
                let first = edited.len();
                edited.extend(drag.painted.iter().copied());
                self.selected = (first..edited.len()).collect();
            }
            DragKind::Erase => {
                // Erase along the pointer's way since the last frame.
                let steps = ((pos - drag.last).length() / 4.0).ceil().max(1.0) as usize;
                for s in 0..=steps {
                    let q = drag.last + (pos - drag.last) * (s as f32 / steps as f32);
                    let hit = drag.original.iter().enumerate().find(|(_, n)| {
                        frame
                            .note_rect(origin + n.start, origin + n.start + n.length, n.key)
                            .is_some_and(|r| r.contains(q))
                    });
                    if let Some((i, _)) = hit
                        && !drag.erased.contains(&i)
                    {
                        drag.erased.push(i);
                    }
                }
                edited = drag
                    .original
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !drag.erased.contains(i))
                    .map(|(_, n)| *n)
                    .collect();
                self.selected.clear();
            }
            DragKind::Select { anchor } => {
                let r = Rect::from_two_pos(anchor, pos);
                let painter = ui.painter().with_clip_rect(area);
                painter.rect_filled(r, 2.0, p.accent.gamma_multiply(0.08));
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, p.accent), StrokeKind::Inside);
                let mut chosen = drag.base.clone();
                for (i, n) in drag.original.iter().enumerate() {
                    if frame
                        .note_rect(origin + n.start, origin + n.start + n.length, n.key)
                        .is_some_and(|nr| nr.intersects(r))
                        && !chosen.contains(&i)
                    {
                        chosen.push(i);
                    }
                }
                self.selected = chosen;
            }
            DragKind::Values { .. } | DragKind::Value { .. } => {}
        }
        if !matches!(drag.kind, DragKind::Select { .. }) {
            self.preview = Some(edited);
        }
        // Past an edge the view follows, faster the further the pointer goes; what was grabbed
        // stays under it.
        let dt = ui.input(|i| i.stable_dt).min(0.05);
        let speed = |past: f32| {
            if past > -2.0 {
                (past + 2.0).min(60.0) * 12.0 * dt
            } else {
                0.0
            }
        };
        let (before_x, before_y) = (self.view.scroll, self.view.scroll_y);
        let dx = speed(pos.x - area.right()) - speed(area.left() - pos.x);
        let dy = speed(pos.y - area.bottom()) - speed(area.top() - pos.y);
        self.view.scroll = (self.view.scroll + f64::from(dx) * self.view.zoom).max(0.0);
        self.view.scroll_y = (self.view.scroll_y + dy)
            .clamp(0.0, (self.view.content_height() - area.height()).max(0.0));
        let moved = vec2(
            ((self.view.scroll - before_x) / self.view.zoom) as f32,
            self.view.scroll_y - before_y,
        );
        drag.origin -= moved;
        if let DragKind::Select { anchor } = &mut drag.kind {
            *anchor -= moved;
        }
        drag.last = pos;
        self.drag = Some(drag);
        ui.ctx().request_repaint();
    }

    /// Ends a drag on the notes: one undo step.
    fn finish(&mut self, drag: Drag) {
        let Some(edited) = self.preview.take() else {
            return;
        };
        if matches!(drag.kind, DragKind::Resize | DragKind::Draw)
            && let Some(n) = self.selected.first().and_then(|&i| edited.get(i))
        {
            self.last_length = Some(n.length);
        }
        let label = match drag.kind {
            DragKind::Move if drag.copy => tr("Copy notes"),
            DragKind::Move => tr("Move notes"),
            DragKind::Resize | DragKind::ResizeStart => tr("Resize notes"),
            DragKind::Paint => tr("Paint notes"),
            DragKind::Draw if self.stamp.on => tr("Add chord"),
            DragKind::Draw => tr("Add note"),
            DragKind::Erase => tr("Erase notes"),
            DragKind::Values { .. } | DragKind::Value { .. } => self.lane.name(),
            DragKind::Select { .. } => return,
        };
        if edited == drag.original && !drag.copy {
            return;
        }
        self.commit(label, edited);
    }
}

/// A small sign of the tool beside the pointer.
fn paint_tool_badge(ui: &Ui, pos: Pos2, tool: Tool) {
    if tool == Tool::Pointer {
        return;
    }
    let p = theme::current(ui);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("piano-tool-badge"),
    ));
    let rect = Rect::from_min_size(pos + vec2(10.0, 10.0), vec2(16.0, 16.0));
    painter.rect_filled(rect, 4.0, p.surface.gamma_multiply(0.95));
    super::panel::paint_tool(&painter, rect.shrink(3.0), tool, p.text);
}

/// A note's own expression curve (MPE), drawn inside it.
fn draw_expression(
    painter: &egui::Painter,
    r: Rect,
    n: &Note,
    controller: u16,
    controls: &[audiostrada_core::ControlPoint],
    origin: u64,
    frame: &Frame,
) {
    let points: Vec<Pos2> = controls
        .iter()
        .filter(|c| {
            c.controller == controller
                && c.channel == n.channel
                && c.time >= n.start
                && c.time <= n.start + n.length
        })
        .map(|c| {
            pos2(
                frame.x(origin + c.time),
                r.bottom() - r.height() * c.value.clamp(0.0, 1.0),
            )
        })
        .collect();
    if points.len() >= 2 {
        painter.with_clip_rect(r).add(Shape::line(
            points,
            Stroke::new(1.5, Color32::WHITE.gamma_multiply(0.85)),
        ));
    }
}
