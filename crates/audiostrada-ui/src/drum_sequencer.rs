//! The drum machine's step sequencer: the sixteen pads as rows of sixteenth-note steps over
//! the track's MIDI clip at the playhead, so that what is drawn here is what the piano roll
//! shows. A lane below sets the velocity, the chance, the roll or the flam of the selected
//! pad's steps; swing delays the off sixteenths.

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
    Roll,
    Flam,
}

impl Lane {
    const ALL: [(Lane, &str); 4] = [
        (Lane::Velocity, "Velocity"),
        (Lane::Chance, "Chance"),
        (Lane::Roll, "Roll"),
        (Lane::Flam, "Flam"),
    ];
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
    /// Painting flams on or off in the lane.
    flams: Option<bool>,
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
            flams: None,
            bars: 1,
        }
    }
}

/// A flam's grace note sits this far before its step, in steps.
const FLAM: f64 = 0.15;
/// A step owns its notes from this far before it, in steps, so that its flam is its own.
const LEAD: f64 = 0.16;
/// The most hits of a roll.
pub(crate) const ROLLS: usize = 4;

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
    fn at(&self, k: usize, swing: f32) -> f64 {
        let delay = if k % 2 == 1 {
            self.step * f64::from((swing / 100.0 * 2.0 - 1.0).clamp(0.0, 0.5))
        } else {
            0.0
        };
        self.origin + k as f64 * self.step + delay
    }

    /// The step a note belongs to: from just before a step to just before the next.
    pub fn step_of(&self, start: u64, swing: f32) -> Option<usize> {
        let lead = LEAD * self.step;
        let guess = ((start as f64 - self.origin) / self.step + LEAD).floor();
        if guess < 0.0 {
            return None;
        }
        let guess = guess as usize;
        let start = start as f64;
        [guess, guess.saturating_sub(1)].into_iter().find(|&k| {
            k < self.steps
                && start >= self.at(k, swing) - lead
                && start < self.at(k + 1, swing) - lead
        })
    }

    /// The time between the hits of a roll of `hits` on step `k`, kept clear of the next
    /// step's flam.
    fn gap(&self, k: usize, swing: f32, hits: usize) -> f64 {
        let span = self.at(k + 1, swing) - self.at(k, swing);
        let room = span - (LEAD + 0.01) * self.step;
        (span / hits as f64).min(room / (hits.max(2) - 1) as f64)
    }
}

/// A step as the sequencer sees it: one hit or a roll, with or without a flam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub key: u8,
    pub velocity: u8,
    pub probability: u8,
    pub hits: usize,
    pub flam: bool,
}

impl Step {
    fn new(pad: usize) -> Self {
        Self {
            key: VOICES[pad].2,
            velocity: 100,
            probability: 100,
            hits: 1,
            flam: false,
        }
    }
}

/// The notes of pad `pad` on step `k`.
fn on_step(notes: &[Note], grid: &Grid, pad: usize, k: usize, swing: f32) -> Vec<usize> {
    notes
        .iter()
        .enumerate()
        .filter(|(_, n)| voice_of(n.key) == Some(pad) && grid.step_of(n.start, swing) == Some(k))
        .map(|(i, _)| i)
        .collect()
}

/// Whether a note of step `k` comes before it: a flam's grace note.
fn is_grace(grid: &Grid, k: usize, swing: f32, start: u64) -> bool {
    (start as f64) < grid.at(k, swing) - 0.05 * grid.step
}

/// Pad `pad`'s step `k`, if it plays.
pub(crate) fn read_step(
    notes: &[Note],
    grid: &Grid,
    pad: usize,
    k: usize,
    swing: f32,
) -> Option<Step> {
    let present = on_step(notes, grid, pad, k, swing);
    let (grace, hits): (Vec<usize>, Vec<usize>) = present
        .iter()
        .partition(|&&i| is_grace(grid, k, swing, notes[i].start));
    let (grace, hits) = if hits.is_empty() {
        (Vec::new(), grace)
    } else {
        (grace, hits)
    };
    let head = &notes[*hits.iter().min_by_key(|&&i| notes[i].start)?];
    Some(Step {
        key: head.key,
        velocity: head.velocity,
        probability: head.probability,
        hits: hits.len(),
        flam: !grace.is_empty(),
    })
}

/// The notes with pad `pad`'s step `k` replaced by `step`, or emptied.
pub(crate) fn write_step(
    notes: &[Note],
    grid: &Grid,
    pad: usize,
    k: usize,
    swing: f32,
    step: Option<Step>,
) -> Vec<Note> {
    let present = on_step(notes, grid, pad, k, swing);
    let mut out: Vec<Note> = notes
        .iter()
        .enumerate()
        .filter(|(i, _)| !present.contains(i))
        .map(|(_, n)| *n)
        .collect();
    if let Some(s) = step {
        let hits = s.hits.max(1);
        let gap = grid.gap(k, swing, hits);
        let start = grid.at(k, swing);
        let note = |at: f64, velocity: u8| Note {
            start: at.round() as u64,
            length: (gap.min(grid.step) / 2.0).max(1.0) as u64,
            key: s.key,
            velocity,
            channel: 9,
            probability: s.probability,
            ..Default::default()
        };
        let grace = start - FLAM * grid.step;
        if s.flam && grace >= 0.0 {
            out.push(note(grace, ((f32::from(s.velocity) * 0.6) as u8).max(1)));
        }
        for j in 0..hits {
            out.push(note(start + j as f64 * gap, s.velocity));
        }
    }
    out
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
    match (on, read_step(notes, grid, pad, k, swing)) {
        (true, Some(_)) => notes.to_vec(),
        (true, None) => write_step(notes, grid, pad, k, swing, Some(Step::new(pad))),
        (false, _) => write_step(notes, grid, pad, k, swing, None),
    }
}

/// The notes moved from swing `from` to `to`, rolls stretched with their step.
pub(crate) fn reswing(notes: &[Note], grid: &Grid, from: f32, to: f32) -> Vec<Note> {
    notes
        .iter()
        .map(|n| {
            let mut n = *n;
            if let Some(k) = grid.step_of(n.start, from) {
                let (old, new) = (grid.at(k, from), grid.at(k, to));
                let start = n.start as f64;
                let moved = if is_grace(grid, k, from, n.start) {
                    start - old + new
                } else {
                    let lead = LEAD * grid.step;
                    let stretch =
                        (grid.at(k + 1, to) - new - lead) / (grid.at(k + 1, from) - old - lead);
                    new + (start - old) * stretch
                };
                n.start = moved.round().max(0.0) as u64;
            }
            n
        })
        .collect()
}

/// The swing the clip's off sixteenths were written with, if it has any.
pub(crate) fn swing_of(notes: &[Note], grid: &Grid, current: f32) -> Option<f32> {
    let fits = |swing: f32| {
        let close = (0.004 * grid.step).max(2.0);
        notes
            .iter()
            .filter(|n| {
                grid.step_of(n.start, swing).is_some_and(|k| {
                    k % 2 == 1 && (n.start as f64 - grid.at(k, swing)).abs() <= close
                })
            })
            .count()
    };
    let (best, count) = (100..=150)
        .map(|h| h as f32 / 2.0)
        .map(|s| (s, fits(s)))
        .fold((current, 0), |b, c| if c.1 > b.1 { c } else { b });
    (count > 0 && fits(current) < count).then_some(best)
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
        if let Some(notes) = &clip.notes
            && let Some(swing) = swing_of(notes, &grid, state.swing)
        {
            state.swing = swing;
        }
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
                reswing(notes, &grid, before, state.swing),
                tr("Swing"),
                true,
            );
        }
        if response.drag_stopped() || response.lost_focus() {
            params.doc.history_mut().end_gesture();
        }
        ui.add_space(10.0);
        for (lane, name) in Lane::ALL {
            ui.selectable_value(&mut state.lane, lane, tr(name));
        }
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
            if let Some(n) = read_step(&notes, &grid, pad, k, state.swing) {
                let strength = 0.35 + 0.65 * f32::from(n.velocity) / 127.0;
                let lit = if playing == Some(k) { 1.0 } else { strength };
                painter.rect_filled(cell, 3.0, color.gamma_multiply(lit));
                // A roll splits the step in its hits.
                for j in 1..n.hits.min(8) {
                    let x = cell.left() + cell.width() * j as f32 / n.hits as f32;
                    painter.line_segment(
                        [pos2(x, cell.top()), pos2(x, cell.bottom())],
                        Stroke::new(1.5, base),
                    );
                }
                if n.flam {
                    painter.rect_filled(
                        Rect::from_min_size(cell.min, vec2(3.0, cell.height())),
                        1.0,
                        Color32::from_white_alpha(170),
                    );
                }
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
    // Right-click a step to roll it, one more hit at each click.
    if response.secondary_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((r, c)) = cell_at(pos)
        && c < count
    {
        let (pad, k) = (LAYOUT[r], first + c);
        *selected = pad;
        let step = read_step(&notes, &grid, pad, k, state.swing).unwrap_or(Step::new(pad));
        let hits = if step.hits >= ROLLS { 1 } else { step.hits + 1 };
        let changed = write_step(
            &notes,
            &grid,
            pad,
            k,
            state.swing,
            Some(Step { hits, ..step }),
        );
        store(params, clip.id, changed, tr("Roll"), false);
    } else if let Some(pos) = response.interact_pointer_pos()
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
                let on = read_step(&notes, &grid, pad, k, state.swing).is_some();
                let paint = *state.painting.get_or_insert(!on);
                if response.is_pointer_button_down_on()
                    && ui.input(|i| i.pointer.primary_down())
                    && on != paint
                {
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

/// Under the grid, the selected pad's steps: bars of velocity, chance or roll hits, or flams.
#[allow(clippy::too_many_arguments)]
fn lane(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut SequencerState,
    clip: &audiostrada_core::Clip,
    grid: &Grid,
    notes: &[Note],
    pad: usize,
    (first, count, label_width): (usize, usize, f32),
) {
    let swing = state.swing;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 46.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(16, 19, 25));
    let color = family(pad);
    let name = Lane::ALL
        .iter()
        .find(|l| l.0 == state.lane)
        .map_or("", |l| l.1);
    painter.text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        tr(name),
        FontId::proportional(11.0),
        Color32::from_rgb(170, 180, 195),
    );
    let cells = Rect::from_min_max(
        pos2(rect.left() + label_width, rect.top() + 4.0),
        rect.max - vec2(0.0, 4.0),
    );
    let cell_width = cells.width() / 16.0;
    let dim = Color32::from_rgb(50, 56, 68);
    for c in 0..count {
        let x = cells.left() + c as f32 * cell_width;
        let column = Rect::from_min_max(
            pos2(x + 3.0, cells.top()),
            pos2(x + cell_width - 3.0, cells.bottom()),
        );
        let Some(s) = read_step(notes, grid, pad, first + c, swing) else {
            painter.line_segment(
                [column.left_bottom(), column.right_bottom()],
                Stroke::new(1.0, dim),
            );
            continue;
        };
        let bar = |value: f32| {
            Rect::from_min_max(
                pos2(column.left(), column.bottom() - column.height() * value),
                column.max,
            )
        };
        match state.lane {
            Lane::Velocity => {
                painter.rect_filled(bar(f32::from(s.velocity) / 127.0), 2.0, color);
            }
            Lane::Chance => {
                painter.rect_filled(bar(f32::from(s.probability) / 100.0), 2.0, color);
            }
            Lane::Roll => {
                // One block per hit, up to the most a roll takes.
                let h = column.height() / ROLLS as f32;
                for j in 0..ROLLS {
                    let block = Rect::from_min_size(
                        pos2(column.left(), column.bottom() - (j + 1) as f32 * h + 1.0),
                        vec2(column.width(), h - 2.0),
                    );
                    let fill = if j < s.hits { color } else { dim };
                    painter.rect_filled(block, 2.0, fill);
                }
            }
            Lane::Flam => {
                let mark = Rect::from_center_size(column.center(), vec2(column.width(), 14.0));
                if s.flam {
                    painter.rect_filled(mark, 3.0, color);
                } else {
                    painter.rect_stroke(mark, 3.0, Stroke::new(1.0, dim), egui::StrokeKind::Inside);
                }
            }
        }
    }
    // Drag over the steps to set them; flams paint on or off from the first one touched.
    if let Some(pos) = response.interact_pointer_pos()
        && response.is_pointer_button_down_on()
        && cells.x_range().contains(pos.x)
    {
        let c = ((pos.x - cells.left()) / cell_width) as usize;
        let level = ((cells.bottom() - pos.y) / cells.height()).clamp(0.0, 1.0);
        let k = first + c;
        if c < count
            && let Some(s) = read_step(notes, grid, pad, k, swing)
        {
            let changed = match state.lane {
                Lane::Velocity => Step {
                    velocity: (level * 126.0).round() as u8 + 1,
                    ..s
                },
                Lane::Chance => Step {
                    probability: (level * 100.0).round() as u8,
                    ..s
                },
                Lane::Roll => Step {
                    hits: ((level * ROLLS as f32).ceil() as usize).clamp(1, ROLLS),
                    ..s
                },
                Lane::Flam => Step {
                    flam: *state.flams.get_or_insert(!s.flam),
                    ..s
                },
            };
            if changed != s {
                let label = tr(name);
                let written = write_step(notes, grid, pad, k, swing, Some(changed));
                store(params, clip.id, written, label, true);
            }
        }
    }
    if response.drag_stopped() || response.clicked() {
        state.flams = None;
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
        assert_eq!(g.step_of(swung[0].start, 66.0), Some(5));
        assert_eq!(swing_of(&swung, &g, 50.0), Some(66.0));
        assert_eq!(swing_of(&swung, &g, 66.0), None, "already right");
        let straight = reswing(&swung, &g, 66.0, 50.0);
        assert_eq!(straight[0].start, 5000);
        // A General MIDI neighbor counts as the pad's note.
        let neighbor = Note {
            start: 2000,
            key: 44,
            ..Default::default()
        };
        assert_eq!(on_step(&[neighbor], &g, 4, 2, 50.0).len(), 1);
    }

    #[test]
    fn rolls_and_flams_stay_on_their_step() {
        let g = grid();
        for swing in [50.0, 66.0, 75.0] {
            let mut notes = Vec::new();
            for k in 0..16 {
                let step = Step {
                    hits: 1 + k % ROLLS,
                    flam: k % 3 == 1,
                    ..Step::new(1)
                };
                notes = write_step(&notes, &g, 1, k, swing, Some(step));
            }
            for k in 0..16 {
                let s = read_step(&notes, &g, 1, k, swing).unwrap();
                assert_eq!((s.hits, s.flam), (1 + k % ROLLS, k % 3 == 1), "{swing} {k}");
            }
            // A roll of four, then back to one hit.
            let rolled = write_step(&notes, &g, 1, 2, swing, Some(Step::new(1)));
            assert_eq!(read_step(&rolled, &g, 1, 2, swing).unwrap().hits, 1);
            // Swung again, the rolls and flams keep their steps.
            let other = if swing == 50.0 { 70.0 } else { 50.0 };
            let moved = reswing(&notes, &g, swing, other);
            assert_eq!(swing_of(&moved, &g, swing), Some(other), "{swing}");
            for k in 0..16 {
                let s = read_step(&moved, &g, 1, k, other).unwrap();
                assert_eq!((s.hits, s.flam), (1 + k % ROLLS, k % 3 == 1), "{other} {k}");
            }
        }
        // The flam comes first and softer; the hits of a roll share the step.
        let notes = write_step(
            &[],
            &g,
            1,
            4,
            50.0,
            Some(Step {
                hits: 2,
                flam: true,
                ..Step::new(1)
            }),
        );
        let starts: Vec<(u64, u8)> = notes.iter().map(|n| (n.start, n.velocity)).collect();
        assert_eq!(starts, [(3850, 60), (4000, 100), (4500, 100)]);
    }
}
