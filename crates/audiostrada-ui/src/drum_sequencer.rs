//! The drum machine's step sequencer: the sixteen pads as rows of sixteenth-note steps over
//! the track's MIDI clip at the playhead, so that what is drawn here is what the piano roll
//! shows. A lane below sets the velocity or the chance of the selected pad's steps; swing
//! delays the off sixteenths.

use std::sync::Arc;

use audiostrada_core::routing::Channel;
use audiostrada_core::{ClipId, Note, TrackId};
use audiostrada_dsp::drums::{LAYOUT, VOICES, voice_of};
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::drums_editor::{RHYTHMS, family, rhythm_notes};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;

/// What the lane below the grid sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lane {
    #[default]
    Velocity,
    Chance,
}

/// What the sequencer remembers between frames.
#[derive(Debug)]
pub struct SequencerState {
    clip: Option<ClipId>,
    /// The bar shown, from 0.
    page: usize,
    /// 50 plays straight, 66 swings the off sixteenths to triplets.
    swing: f32,
    lane: Lane,
    /// Painting steps on (true) or off while the mouse is down.
    painting: Option<bool>,
    /// Bars of a new pattern.
    bars: u32,
}

impl Default for SequencerState {
    fn default() -> Self {
        Self {
            clip: None,
            page: 0,
            swing: 50.0,
            lane: Lane::Velocity,
            painting: None,
            bars: 1,
        }
    }
}

/// Where the steps of a clip fall, in its content's time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Grid {
    /// Content time of the first step.
    pub origin: f64,
    /// A sixteenth note, in samples.
    pub step: f64,
    pub steps: usize,
}

impl Grid {
    /// When step `k` plays, swung.
    pub fn time(&self, k: usize, swing: f32) -> u64 {
        let delay = if k % 2 == 1 {
            self.step * f64::from((swing / 100.0 * 2.0 - 1.0).clamp(0.0, 0.5))
        } else {
            0.0
        };
        (self.origin + k as f64 * self.step + delay).round() as u64
    }

    /// The step a note sits on: the nearest, swung or not.
    pub fn step_of(&self, start: u64) -> Option<usize> {
        let at = (start as f64 - self.origin) / self.step;
        let k = at.round();
        (k >= 0.0 && (at - k).abs() < 0.45 && (k as usize) < self.steps).then_some(k as usize)
    }
}

/// The notes of pad `pad` on step `k`.
fn on_step(notes: &[Note], grid: &Grid, pad: usize, k: usize) -> impl Iterator<Item = usize> {
    notes
        .iter()
        .enumerate()
        .filter(move |(_, n)| voice_of(n.key) == Some(pad) && grid.step_of(n.start) == Some(k))
        .map(|(i, _)| i)
        .collect::<Vec<_>>()
        .into_iter()
}

/// The notes with pad `pad`'s step `k` set on or off.
pub(crate) fn set_step(
    notes: &[Note],
    grid: &Grid,
    pad: usize,
    k: usize,
    on: bool,
    swing: f32,
) -> Vec<Note> {
    let present: Vec<usize> = on_step(notes, grid, pad, k).collect();
    let mut out: Vec<Note> = notes
        .iter()
        .enumerate()
        .filter(|(i, _)| on || !present.contains(i))
        .map(|(_, n)| *n)
        .collect();
    if on && present.is_empty() {
        out.push(Note {
            start: grid.time(k, swing),
            length: (grid.step / 2.0) as u64,
            key: VOICES[pad].2,
            velocity: 100,
            channel: 9,
            ..Default::default()
        });
    }
    out
}

/// The notes with the odd steps moved to `swing`.
pub(crate) fn reswing(notes: &[Note], grid: &Grid, swing: f32) -> Vec<Note> {
    notes
        .iter()
        .map(|n| {
            let mut n = *n;
            if let Some(k) = grid.step_of(n.start) {
                n.start = grid.time(k, swing);
            }
            n
        })
        .collect()
}

/// The MIDI clip the sequencer edits on `track`: the one under the playhead, else the one
/// chosen before, else the first.
fn target(params: &Params, track: TrackId, remembered: Option<ClipId>) -> Option<ClipId> {
    let project = params.doc.project();
    let clips: Vec<&audiostrada_core::Clip> = project
        .tracks
        .iter()
        .find(|t| t.id == track)?
        .clips
        .iter()
        .filter(|c| c.is_midi())
        .collect();
    let at = params.doc.player().position();
    clips
        .iter()
        .find(|c| at >= c.start && at < c.start + c.length)
        .or_else(|| clips.iter().find(|c| Some(c.id) == remembered))
        .or_else(|| clips.first())
        .map(|c| c.id)
}

/// The clip's steps, from the tempo at its start.
fn grid_of(params: &Params, clip: &audiostrada_core::Clip) -> Grid {
    let project = params.doc.project();
    let (rate, tempo) = (project.sample_rate, &project.tempo);
    let beat = tempo.sample_to_beat(clip.start, rate);
    let beat_length =
        tempo.beat_to_sample(beat + 1.0, rate) as f64 - tempo.beat_to_sample(beat, rate) as f64;
    let step = (beat_length / 4.0).max(1.0);
    Grid {
        origin: clip.offset as f64,
        step,
        steps: ((clip.length as f64 / step).round() as usize).max(1),
    }
}

fn find_clip(params: &Params, id: ClipId) -> Option<audiostrada_core::Clip> {
    params
        .doc
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.id == id)
        .cloned()
}

fn store(params: &mut Params, clip: ClipId, notes: Vec<Note>, label: &str, merge: bool) {
    let edit =
        |p: &mut audiostrada_core::Project| audiostrada_core::edit::set_notes(p, clip, notes);
    let history = params.doc.history_mut();
    if merge {
        history.edit_merged(label, "drum-steps", edit);
    } else {
        history.edit(label, edit);
    }
}

/// A new empty pattern of `bars` bars at the playhead's bar.
fn new_pattern(params: &mut Params, track: TrackId, bars: u32, notes: Vec<Note>) -> Option<ClipId> {
    let doc = &mut *params.doc;
    let project = doc.project();
    let (rate, tempo) = (project.sample_rate, project.tempo.clone());
    let beat = tempo.sample_to_beat(doc.player().position(), rate);
    let bar = tempo.beat_to_bbt(beat);
    let first = tempo.bbt_to_beat(audiostrada_core::tempo::BarBeatTick {
        bar: bar.bar,
        beat: 1,
        tick: 0,
    });
    let start = tempo.beat_to_sample(first, rate);
    let end = tempo.beat_to_sample(first + 4.0 * f64::from(bars), rate);
    let mut created = None;
    doc.history_mut().edit(tr("New drum pattern"), |p| {
        created = p.add_midi_clip(track, start, end - start, notes);
        if let Some(id) = created {
            audiostrada_core::edit::resolve_overlaps(p, track, &[id]);
        }
    });
    created
}

pub fn show(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut SequencerState,
    selected: &mut usize,
    meters: &[f32],
) {
    let Channel::Track(track) = params.channel else {
        return;
    };
    let p = theme::current(ui);
    state.clip = target(params, track, state.clip);
    let clip = state.clip.and_then(|id| find_clip(params, id));
    // The header: the pattern, its bars, swing, the lane, and rhythms to start from.
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Pattern")).strong().color(p.weak));
        let Some(clip) = &clip else {
            ui.weak(tr("No MIDI clip on this track yet"));
            for n in [1, 2, 4] {
                ui.selectable_value(&mut state.bars, n, crate::trf!("{n} bars", n = n));
            }
            if ui
                .button(tr("New pattern"))
                .on_hover_text(tr(
                    "An empty MIDI clip at the playhead's bar, to fill step by step",
                ))
                .clicked()
            {
                state.clip = new_pattern(params, track, state.bars, Vec::new());
                state.page = 0;
            }
            return;
        };
        let grid = grid_of(params, clip);
        let pages = grid.steps.div_ceil(16).max(1);
        state.page = state.page.min(pages - 1);
        ui.label(RichText::new(&clip.name).size(11.5));
        if pages > 1 {
            ui.add_space(6.0);
            for page in 0..pages {
                ui.selectable_value(&mut state.page, page, crate::trf!("Bar {n}", n = page + 1));
            }
        }
        ui.add_space(10.0);
        ui.label(tr("Swing"));
        let before = state.swing;
        let response = ui.add(
            egui::DragValue::new(&mut state.swing)
                .range(50.0..=75.0)
                .suffix(" %")
                .speed(0.3),
        );
        if state.swing != before
            && let Some(notes) = &clip.notes
        {
            store(
                params,
                clip.id,
                reswing(notes, &grid, state.swing),
                tr("Swing"),
                true,
            );
        }
        if response.drag_stopped() || response.lost_focus() {
            params.doc.history_mut().end_gesture();
        }
        ui.add_space(10.0);
        ui.selectable_value(&mut state.lane, Lane::Velocity, tr("Velocity"));
        ui.selectable_value(&mut state.lane, Lane::Chance, tr("Chance"));
        ui.add_space(10.0);
        let mut chosen = None;
        egui::ComboBox::from_id_salt("drum-seq-rhythm")
            .width(150.0)
            .selected_text(tr("Start from a rhythm"))
            .show_ui(ui, |ui| {
                for (i, r) in RHYTHMS.iter().enumerate() {
                    let label = format!("{} · {} BPM", tr(r.name), r.bpm);
                    if ui.selectable_label(false, label).clicked() {
                        chosen = Some(i);
                    }
                }
            });
        if let Some(i) = chosen {
            let rhythm = &RHYTHMS[i];
            state.swing = rhythm.swing;
            let notes: Vec<Note> =
                rhythm_notes(rhythm, (grid.steps / 16).max(1) as u32, grid.step * 4.0)
                    .into_iter()
                    .map(|mut n| {
                        n.start += grid.origin as u64;
                        n
                    })
                    .collect();
            store(
                params,
                clip.id,
                notes,
                &crate::trf!("Rhythm {name}", name = tr(rhythm.name)),
                false,
            );
        }
        if ui.button(tr("Clear")).clicked() {
            store(params, clip.id, Vec::new(), tr("Clear the pattern"), false);
        }
    });
    let Some(clip) = clip else {
        return;
    };
    let grid = grid_of(params, &clip);
    let notes: Arc<Vec<Note>> = clip.notes.clone().unwrap_or_default();
    let first = state.page * 16;
    let count = (grid.steps - first.min(grid.steps)).min(16);
    // The step playing, when the playhead is in the clip.
    let at = params.doc.player().position();
    let playing = (at >= clip.start && at < clip.start + clip.length)
        .then(|| ((at - clip.start + clip.offset) as f64 - grid.origin) / grid.step)
        .filter(|s| *s >= 0.0)
        .map(|s| s.floor() as usize);
    ui.add_space(4.0);
    let label_width = 96.0;
    let row = 17.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, row * 16.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(16, 19, 25));
    let cells = Rect::from_min_max(pos2(rect.left() + label_width, rect.top()), rect.max);
    let cell_width = cells.width() / 16.0;
    let cell_at = |pos: egui::Pos2| -> Option<(usize, usize)> {
        if !rect.contains(pos) {
            return None;
        }
        let r = ((pos.y - rect.top()) / row) as usize;
        let c = ((pos.x - cells.left()) / cell_width).floor();
        (r < 16).then_some((r, if c < 0.0 { usize::MAX } else { c as usize }))
    };
    if let Some(k) = playing.filter(|k| (first..first + count).contains(k)) {
        let x = cells.left() + (k - first) as f32 * cell_width;
        painter.rect_filled(
            Rect::from_min_size(pos2(x, rect.top()), vec2(cell_width, rect.height())),
            0.0,
            Color32::from_white_alpha(18),
        );
    }
    for (r, &pad) in LAYOUT.iter().enumerate() {
        let y = rect.top() + r as f32 * row;
        let color = family(pad);
        let glow = meters.get(pad).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        let name_color = if pad == *selected {
            Color32::WHITE
        } else {
            Color32::from_rgb(170, 180, 195)
        };
        painter.circle_filled(
            pos2(rect.left() + 10.0, y + row / 2.0),
            3.5 + 2.0 * glow,
            color,
        );
        painter.text(
            pos2(rect.left() + 20.0, y + row / 2.0),
            Align2::LEFT_CENTER,
            tr(VOICES[pad].1),
            FontId::proportional(11.0),
            name_color,
        );
        for c in 0..count {
            let k = first + c;
            let x = cells.left() + c as f32 * cell_width;
            let cell =
                Rect::from_min_size(pos2(x + 1.5, y + 2.0), vec2(cell_width - 3.0, row - 4.0));
            // Beats stand out from their sixteenths.
            let base = if (c / 4) % 2 == 0 {
                Color32::from_rgb(34, 39, 49)
            } else {
                Color32::from_rgb(28, 32, 41)
            };
            painter.rect_filled(cell, 3.0, base);
            if let Some(i) = on_step(&notes, &grid, pad, k).next() {
                let n = &notes[i];
                let strength = 0.35 + 0.65 * f32::from(n.velocity) / 127.0;
                let lit = if playing == Some(k) { 1.0 } else { strength };
                painter.rect_filled(cell, 3.0, color.gamma_multiply(lit));
                if n.probability < 100 {
                    // A chance under 100 % leaves part of the step dark.
                    let dark = cell.width() * (1.0 - f32::from(n.probability) / 100.0);
                    painter.rect_filled(
                        Rect::from_min_size(
                            pos2(cell.right() - dark, cell.top()),
                            vec2(dark, cell.height()),
                        ),
                        3.0,
                        base.gamma_multiply(0.9),
                    );
                }
            }
        }
        if pad == *selected {
            painter.rect_stroke(
                Rect::from_min_size(
                    pos2(rect.left() + 2.0, y + 1.0),
                    vec2(rect.width() - 4.0, row - 2.0),
                ),
                4.0,
                Stroke::new(1.0, Color32::from_white_alpha(60)),
                egui::StrokeKind::Inside,
            );
        }
    }
    // Click a step to set it, drag to paint more; click a name to select its pad.
    if let Some(pos) = response.interact_pointer_pos()
        && let Some((r, c)) = cell_at(pos)
    {
        {
            let pad = LAYOUT[r];
            if c == usize::MAX {
                if response.clicked() {
                    *selected = pad;
                }
            } else if c < count {
                let k = first + c;
                let on = on_step(&notes, &grid, pad, k).next().is_some();
                let paint = *state.painting.get_or_insert(!on);
                if response.is_pointer_button_down_on() && on != paint {
                    *selected = pad;
                    let changed = set_step(&notes, &grid, pad, k, paint, state.swing);
                    store(params, clip.id, changed, tr("Steps"), true);
                }
            }
        }
    }
    if !response.is_pointer_button_down_on() && state.painting.take().is_some() {
        params.doc.history_mut().end_gesture();
    }
    lane(
        ui,
        params,
        state,
        &clip,
        &grid,
        &notes,
        *selected,
        (first, count, label_width),
    );
    if params.doc.player().engine().playing() {
        ui.ctx().request_repaint();
    }
}

/// Bars under the grid: the velocity or the chance of the selected pad's steps.
#[allow(clippy::too_many_arguments)]
fn lane(
    ui: &mut Ui,
    params: &mut Params,
    state: &SequencerState,
    clip: &audiostrada_core::Clip,
    grid: &Grid,
    notes: &[Note],
    pad: usize,
    (first, count, label_width): (usize, usize, f32),
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 46.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(16, 19, 25));
    let color = family(pad);
    painter.text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        tr(match state.lane {
            Lane::Velocity => "Velocity",
            Lane::Chance => "Chance",
        }),
        FontId::proportional(11.0),
        Color32::from_rgb(170, 180, 195),
    );
    let cells = Rect::from_min_max(
        pos2(rect.left() + label_width, rect.top() + 4.0),
        rect.max - vec2(0.0, 4.0),
    );
    let cell_width = cells.width() / 16.0;
    let value = |n: &Note| match state.lane {
        Lane::Velocity => f32::from(n.velocity) / 127.0,
        Lane::Chance => f32::from(n.probability) / 100.0,
    };
    for c in 0..count {
        let x = cells.left() + c as f32 * cell_width;
        if let Some(i) = on_step(notes, grid, pad, first + c).next() {
            let h = cells.height() * value(&notes[i]);
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(x + 3.0, cells.bottom() - h),
                    pos2(x + cell_width - 3.0, cells.bottom()),
                ),
                2.0,
                color,
            );
        } else {
            painter.line_segment(
                [
                    pos2(x + 3.0, cells.bottom()),
                    pos2(x + cell_width - 3.0, cells.bottom()),
                ],
                Stroke::new(1.0, Color32::from_rgb(50, 56, 68)),
            );
        }
    }
    // Drag over the bars to set them.
    if let Some(pos) = response.interact_pointer_pos()
        && response.is_pointer_button_down_on()
        && cells.x_range().contains(pos.x)
    {
        let c = ((pos.x - cells.left()) / cell_width) as usize;
        let level = ((cells.bottom() - pos.y) / cells.height()).clamp(0.0, 1.0);
        if c < count
            && let Some(i) = on_step(notes, grid, pad, first + c).next()
        {
            let mut changed = notes.to_vec();
            match state.lane {
                Lane::Velocity => changed[i].velocity = (level * 126.0).round() as u8 + 1,
                Lane::Chance => changed[i].probability = (level * 100.0).round() as u8,
            }
            if changed[i] != notes[i] {
                store(params, clip.id, changed, tr("Steps"), true);
            }
        }
    }
    if response.drag_stopped() || response.clicked() {
        params.doc.history_mut().end_gesture();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid {
            origin: 0.0,
            step: 1000.0,
            steps: 16,
        }
    }

    #[test]
    fn steps_go_on_and_off_and_swing() {
        let g = grid();
        let notes = set_step(&[], &g, 0, 4, true, 50.0);
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].start, notes[0].key), (4000, 36));
        // Set twice: still one note.
        assert_eq!(set_step(&notes, &g, 0, 4, true, 50.0).len(), 1);
        assert!(set_step(&notes, &g, 0, 4, false, 50.0).is_empty());
        // An off sixteenth swings, and is still found on its step.
        let swung = set_step(&[], &g, 4, 5, true, 66.0);
        assert_eq!(swung[0].start, 5000 + 320);
        assert_eq!(g.step_of(swung[0].start), Some(5));
        let straight = reswing(&swung, &g, 50.0);
        assert_eq!(straight[0].start, 5000);
        // A General MIDI neighbor counts as the pad's note.
        let neighbor = Note {
            start: 2000,
            key: 44,
            ..Default::default()
        };
        assert_eq!(on_step(&[neighbor], &g, 4, 2).count(), 1);
    }
}
