//! The drum machine's step sequencer: the sixteen pads as rows of sixteenth-note steps over
//! the track's MIDI clip at the playhead, so that what is drawn here is what the piano roll
//! shows. Bars play a variation (A, B, C… or a fill), edited once for all their bars; a step's
//! velocity is dragged on it, its roll, flam and chance are in its menu.

use std::sync::Arc;

use audiostrada_core::routing::Channel;
use audiostrada_core::{ClipId, Note, TrackId};
use audiostrada_dsp::drums::{LAYOUT, VOICES, voice_of};
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::drums_editor::{RHYTHMS, family, rhythm_notes};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;

/// What a press on the grid turned into.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// Not moved yet: letting go sets or clears the step.
    Press {
        pad: usize,
        c: usize,
        on: bool,
        y: f32,
    },
    /// Steps painted on or off.
    Paint(bool),
    /// A step's velocity, dragged up or down.
    Velocity {
        pad: usize,
        c: usize,
        y: f32,
        from: u8,
    },
}

/// What the sequencer remembers between frames.
#[derive(Debug)]
pub struct SequencerState {
    clip: Option<ClipId>,
    /// The variation edited.
    variation: u8,
    /// 50 plays straight, 66 swings the off sixteenths to triplets.
    swing: f32,
    gesture: Option<Gesture>,
    /// The pad and step whose menu is open.
    menu: Option<(usize, usize)>,
    /// The bars of a new pattern.
    new_bars: u32,
}

impl Default for SequencerState {
    fn default() -> Self {
        Self {
            clip: None,
            variation: A,
            swing: 50.0,
            gesture: None,
            menu: None,
            new_bars: 4,
        }
    }
}

/// The variations a bar can play.
pub(crate) const A: u8 = 0;
pub(crate) const B: u8 = 1;
pub(crate) const FILL: u8 = 2;
/// A bar the sequencer leaves alone.
pub(crate) const FREE: u8 = 255;
/// The last variation: after A, B and the fill come C to Z.
const LAST: u8 = 26;

/// A variation's name: A, B, C… or Fill.
fn variation_name(v: u8) -> String {
    match v {
        A => "A".into(),
        B => "B".into(),
        FILL => tr("Fill").into(),
        FREE => "-".into(),
        v => char::from(b'C' + v - 3).to_string(),
    }
}

/// Variations in the order they are shown: A, B, C… then the fill.
fn shown_order(mut list: Vec<u8>) -> Vec<u8> {
    list.retain(|&v| v != FREE);
    list.sort_by_key(|&v| (v == FILL, v));
    list.dedup();
    list
}

/// The first variation after A, B and the fill that the chain does not use yet.
fn next_letter(chain: &[u8]) -> Option<u8> {
    (3..=LAST).find(|v| !chain.contains(v))
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

/// The notes of bar `bar`.
fn in_bar(notes: &[Note], grid: &Grid, bar: usize, swing: f32) -> Vec<usize> {
    notes
        .iter()
        .enumerate()
        .filter(|(_, n)| grid.step_of(n.start, swing).map(|k| k / 16) == Some(bar))
        .map(|(i, _)| i)
        .collect()
}

/// A bar's notes from its start, to tell bars apart.
fn signature(notes: &[Note], grid: &Grid, bar: usize, swing: f32) -> Vec<(u8, i64, u8, u8)> {
    let start = grid.origin + (bar * 16) as f64 * grid.step;
    let mut out: Vec<_> = in_bar(notes, grid, bar, swing)
        .into_iter()
        .map(|i| {
            let n = &notes[i];
            let at = ((n.start as f64 - start) / (grid.step / 96.0)).round() as i64;
            (n.key, at, n.velocity, n.probability)
        })
        .collect();
    out.sort_unstable();
    out
}

/// Each bar's variation: the stored ones; the others are the bar they sound like, else, with
/// nothing stored, A then B, else left alone.
pub(crate) fn chain(stored: &[u8], notes: &[Note], grid: &Grid, swing: f32) -> Vec<u8> {
    let bars = grid.steps.div_ceil(16).max(1);
    let signatures: Vec<_> = (0..bars)
        .map(|b| signature(notes, grid, b, swing))
        .collect();
    let mut out: Vec<u8> = Vec::with_capacity(bars);
    for b in 0..bars {
        let v = stored.get(b).copied().unwrap_or_else(|| {
            (0..b)
                .find(|&o| out[o] != FREE && signatures[o] == signatures[b])
                .map(|o| out[o])
                .or_else(|| {
                    [A, B]
                        .into_iter()
                        .find(|v| stored.is_empty() && !out.contains(v))
                })
                .unwrap_or(FREE)
        });
        out.push(v);
    }
    out
}

fn bars_of(chain: &[u8], variation: u8) -> Vec<usize> {
    (0..chain.len())
        .filter(|&b| chain[b] == variation)
        .collect()
}

/// Step `c` of every bar in `bars`, changed alike.
fn edit_bars(
    notes: &[Note],
    grid: &Grid,
    bars: &[usize],
    c: usize,
    change: impl Fn(&[Note], usize) -> Vec<Note>,
) -> Vec<Note> {
    bars.iter()
        .map(|b| b * 16 + c)
        .filter(|&k| k < grid.steps)
        .fold(notes.to_vec(), |n, k| change(&n, k))
}

/// Bar `to` with the notes of bar `from`.
fn copy_bar(notes: &[Note], grid: &Grid, from: usize, to: usize, swing: f32) -> Vec<Note> {
    if from == to {
        return notes.to_vec();
    }
    let gone = in_bar(notes, grid, to, swing);
    let shift = (to as f64 - from as f64) * 16.0 * grid.step;
    let copies: Vec<Note> = in_bar(notes, grid, from, swing)
        .into_iter()
        .map(|i| Note {
            start: (notes[i].start as f64 + shift).round().max(0.0) as u64,
            ..notes[i]
        })
        .filter(|n| grid.step_of(n.start, swing).is_some())
        .collect();
    notes
        .iter()
        .enumerate()
        .filter(|(i, _)| !gone.contains(i))
        .map(|(_, n)| *n)
        .chain(copies)
        .collect()
}

/// Bar `bar` set to play `variation`, with its notes when it has some already.
fn assign(
    notes: &[Note],
    grid: &Grid,
    chain: &mut [u8],
    bar: usize,
    variation: u8,
    swing: f32,
) -> Vec<Note> {
    let source = chain.iter().position(|&v| v == variation);
    chain[bar] = variation;
    match source {
        Some(from) => copy_bar(notes, grid, from, bar, swing),
        None => notes.to_vec(),
    }
}

fn pad_named(id: &str) -> Option<usize> {
    VOICES.iter().position(|v| v.0 == id)
}

/// A fill's build-up over the last beat of bar `bar`: the snare rolls up, the hats make room.
fn build_up(notes: &[Note], grid: &Grid, bar: usize, swing: f32) -> Vec<Note> {
    let (Some(snare), hats) = (pad_named("snare"), ["closed", "open"].map(pad_named)) else {
        return notes.to_vec();
    };
    let mut out = notes.to_vec();
    for (j, (velocity, hits)) in [(70, 1), (85, 1), (100, 2), (118, 4)]
        .into_iter()
        .enumerate()
    {
        let k = bar * 16 + 12 + j;
        if k >= grid.steps {
            continue;
        }
        for hat in hats.into_iter().flatten() {
            out = write_step(&out, grid, hat, k, swing, None);
        }
        let step = Step {
            velocity,
            hits,
            ..Step::new(snare)
        };
        out = write_step(&out, grid, snare, k, swing, Some(step));
    }
    out
}

/// The clip at `count` bars: new bars repeat the chain, bars past the end go.
pub(crate) fn resize(
    notes: &[Note],
    grid: &Grid,
    chain: &[u8],
    count: usize,
    swing: f32,
) -> (Vec<Note>, Vec<u8>) {
    let count = count.max(1);
    let old = chain.len().max(1);
    let grid = Grid {
        steps: grid.steps.max(count * 16),
        ..*grid
    };
    let mut chain: Vec<u8> = chain.to_vec();
    let mut notes = notes.to_vec();
    if count < chain.len() {
        chain.truncate(count);
        notes.retain(|n| grid.step_of(n.start, swing).is_some_and(|k| k < count * 16));
    }
    for b in chain.len()..count {
        let from = b % old;
        chain.push(chain.get(from).copied().unwrap_or(A));
        notes = copy_bar(&notes, &grid, from, b, swing);
    }
    (notes, chain)
}

/// A new variation `v` on a bar added at the end, copied from `from`'s first bar.
pub(crate) fn append_variation(
    notes: &[Note],
    grid: &Grid,
    chain: &[u8],
    v: u8,
    from: u8,
    swing: f32,
) -> (Vec<Note>, Vec<u8>) {
    let bar = chain.len();
    let grid = Grid {
        steps: grid.steps.max((bar + 1) * 16),
        ..*grid
    };
    let mut chain = chain.to_vec();
    chain.push(v);
    let notes = match chain.iter().position(|&x| x == from) {
        Some(source) => copy_bar(notes, &grid, source, bar, swing),
        None => notes.to_vec(),
    };
    (notes, chain)
}

/// Variation `variation` added to the clip, from A: B every other bar, the fill on every
/// fourth bar. A one-bar clip grows to two bars for B, to four for the fill. Gives the notes,
/// the chain and the bars the clip then has.
pub(crate) fn add_variation(
    notes: &[Note],
    grid: &Grid,
    chain: &[u8],
    variation: u8,
    swing: f32,
) -> (Vec<Note>, Vec<u8>, usize) {
    let mut chain = chain.to_vec();
    let mut notes = notes.to_vec();
    let bars = match (chain.len(), variation) {
        (1, B) => 2,
        (1, FILL) => 4,
        (n, _) => n,
    };
    let grid = Grid {
        steps: grid.steps.max(bars * 16),
        ..*grid
    };
    let a = chain.iter().position(|&v| v == A);
    for b in chain.len()..bars {
        chain.push(A);
        if let Some(a) = a {
            notes = copy_bar(&notes, &grid, a, b, swing);
        }
    }
    let targets: Vec<usize> = match variation {
        A => vec![0],
        B => (1..bars).step_by(2).filter(|&b| chain[b] == A).collect(),
        _ => (1..bars)
            .filter(|&b| b % 4 == 3 || (bars < 4 && b == bars - 1))
            .filter(|&b| chain[b] != FREE)
            .collect(),
    };
    for &b in &targets {
        if let Some(a) = a.filter(|_| variation != A) {
            notes = copy_bar(&notes, &grid, a, b, swing);
        }
        chain[b] = variation;
        if variation == FILL {
            notes = build_up(&notes, &grid, b, swing);
        }
    }
    (notes, chain, bars)
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

/// What the sequencer writes to the clip.
struct Write<'a> {
    notes: Vec<Note>,
    chain: &'a [u8],
    /// The clip's new length in bars, when it grows.
    bars: Option<usize>,
}

fn store(
    params: &mut Params,
    clip: &audiostrada_core::Clip,
    grid: &Grid,
    write: Write,
    label: &str,
    merge: bool,
) {
    let Channel::Track(track) = params.channel else {
        return;
    };
    let (id, chain) = (clip.id, write.chain.to_vec());
    let length = write
        .bars
        .map(|b| (b as f64 * 16.0 * grid.step).round() as u64);
    let edit = move |p: &mut audiostrada_core::Project| {
        audiostrada_core::edit::set_notes(p, id, write.notes);
        if let Some(c) = audiostrada_core::edit::clip_mut(p, id) {
            c.variations = chain;
            if let Some(length) = length {
                c.length = length;
            }
        }
        if length.is_some() {
            audiostrada_core::edit::resolve_overlaps(p, track, &[id]);
        }
    };
    let history = params.doc.history_mut();
    if merge {
        history.edit_merged(label, "drum-steps", edit);
    } else {
        history.edit(label, edit);
    }
}

/// A new empty pattern of `bars` bars at the playhead's bar.
fn new_pattern(params: &mut Params, track: TrackId, bars: u32) -> Option<ClipId> {
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
        created = p.add_midi_clip(track, start, end - start, Vec::new());
        if let Some(id) = created {
            audiostrada_core::edit::resolve_overlaps(p, track, &[id]);
        }
    });
    created
}

fn variation_color(variation: u8) -> Color32 {
    const MORE: [Color32; 6] = [
        Color32::from_rgb(52, 158, 120),
        Color32::from_rgb(196, 86, 110),
        Color32::from_rgb(70, 160, 180),
        Color32::from_rgb(170, 150, 60),
        Color32::from_rgb(110, 110, 210),
        Color32::from_rgb(160, 110, 80),
    ];
    match variation {
        A => Color32::from_rgb(74, 128, 200),
        B => Color32::from_rgb(150, 98, 196),
        FILL => Color32::from_rgb(214, 132, 52),
        FREE => Color32::from_rgb(48, 54, 66),
        v => MORE[usize::from(v - 3) % MORE.len()],
    }
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
    let Some(clip) = state.clip.and_then(|id| find_clip(params, id)) else {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Pattern")).strong().color(p.weak));
            ui.weak(tr("No MIDI clip on this track yet"));
            if ui
                .button(tr("New pattern"))
                .on_hover_text(tr(
                    "An empty MIDI clip at the playhead's bar, to fill step by step",
                ))
                .clicked()
            {
                state.clip = new_pattern(params, track, state.new_bars);
                state.variation = A;
            }
            ui.add(
                egui::DragValue::new(&mut state.new_bars)
                    .range(1..=64)
                    .suffix(tr(" bars")),
            )
            .on_hover_text(tr("Bars of the new pattern"));
        });
        return;
    };
    let grid = grid_of(params, &clip);
    let notes: Arc<Vec<Note>> = clip.notes.clone().unwrap_or_default();
    if let Some(swing) = swing_of(&notes, &grid, state.swing) {
        state.swing = swing;
    }
    let swing = state.swing;
    let chain = chain(&clip.variations, &notes, &grid, swing);
    if !chain.contains(&state.variation) {
        state.variation = A;
    }
    let bars = bars_of(&chain, state.variation);
    // The step playing, when the playhead is in the clip.
    let at = params.doc.player().position();
    let playing = (at >= clip.start && at < clip.start + clip.length)
        .then(|| ((at - clip.start + clip.offset) as f64 - grid.origin) / grid.step)
        .filter(|s| *s >= 0.0)
        .map(|s| s.floor() as usize);

    // The variations, the swing, rhythms to start from.
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Pattern")).strong().color(p.weak));
        ui.label(RichText::new(&clip.name).size(11.5));
        ui.add_space(10.0);
        let used = shown_order(chain.clone());
        for &v in &used {
            if ui
                .selectable_label(
                    state.variation == v,
                    RichText::new(variation_name(v)).color(Color32::WHITE),
                )
                .on_hover_text(tr("Edit this variation"))
                .clicked()
            {
                state.variation = v;
            }
        }
        let add = |ui: &mut Ui, text: String, hint: &str| {
            ui.button(RichText::new(text).color(Color32::from_rgb(150, 160, 175)))
                .on_hover_text(hint)
                .clicked()
        };
        // B and the fill take their places in the chain; more letters come on a new bar.
        for (v, hint) in [
            (B, tr("A second pattern, every other bar")),
            (FILL, tr("A fill at the end of every four bars")),
        ] {
            if !chain.contains(&v) && add(ui, format!("+ {}", variation_name(v)), hint) {
                let (notes, chain, count) = add_variation(&notes, &grid, &chain, v, swing);
                let write = Write {
                    notes,
                    chain: &chain,
                    bars: (count * 16 > grid.steps).then_some(count),
                };
                let label = crate::trf!("Add {name}", name = variation_name(v));
                store(params, &clip, &grid, write, &label, false);
                state.variation = v;
            }
        }
        if chain.contains(&B)
            && let Some(v) = next_letter(&chain)
            && add(
                ui,
                format!("+ {}", variation_name(v)),
                tr("A new variation on a bar added at the end, from the one shown"),
            )
        {
            let (notes, chain) = append_variation(&notes, &grid, &chain, v, state.variation, swing);
            let write = Write {
                notes,
                bars: Some(chain.len()),
                chain: &chain,
            };
            let label = crate::trf!("Add {name}", name = variation_name(v));
            store(params, &clip, &grid, write, &label, false);
            state.variation = v;
        }
        ui.add_space(14.0);
        ui.label(tr("Swing"));
        let before = state.swing;
        let response = ui.add(
            egui::DragValue::new(&mut state.swing)
                .range(50.0..=75.0)
                .suffix(" %")
                .speed(0.3),
        );
        if state.swing != before {
            let write = Write {
                notes: reswing(&notes, &grid, before, state.swing),
                chain: &chain,
                bars: None,
            };
            store(params, &clip, &grid, write, tr("Swing"), true);
        }
        if response.drag_stopped() || response.lost_focus() {
            params.doc.history_mut().end_gesture();
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
            state.variation = A;
            let notes: Vec<Note> =
                rhythm_notes(rhythm, grid.steps.div_ceil(16) as u32, grid.step * 4.0)
                    .into_iter()
                    .map(|mut n| {
                        n.start += grid.origin as u64;
                        n
                    })
                    .filter(|n| grid.step_of(n.start, rhythm.swing).is_some())
                    .collect();
            let write = Write {
                notes,
                chain: &[],
                bars: None,
            };
            let label = crate::trf!("Rhythm {name}", name = tr(rhythm.name));
            store(params, &clip, &grid, write, &label, false);
        }
        if ui
            .button(tr("Clear"))
            .on_hover_text(tr("Clears the steps of the variation shown"))
            .clicked()
        {
            let gone: Vec<usize> = bars
                .iter()
                .flat_map(|&b| in_bar(&notes, &grid, b, swing))
                .collect();
            let write = Write {
                notes: (0..notes.len())
                    .filter(|i| !gone.contains(i))
                    .map(|i| notes[i])
                    .collect(),
                chain: &chain,
                bars: None,
            };
            store(params, &clip, &grid, write, tr("Clear the pattern"), false);
        }
    });

    // The clip's bars: what each plays. A click changes it.
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(RichText::new(tr("Chain")).size(11.0).color(p.weak));
        let mut count = chain.len();
        let response = ui
            .add(
                egui::DragValue::new(&mut count)
                    .range(1..=64)
                    .suffix(tr(" bars")),
            )
            .on_hover_text(tr(
                "The pattern's length: new bars repeat the chain, bars past the end go",
            ));
        if count != chain.len() {
            let (notes, changed) = resize(&notes, &grid, &chain, count, swing);
            let write = Write {
                notes,
                chain: &changed,
                bars: Some(count),
            };
            store(params, &clip, &grid, write, tr("Pattern length"), true);
        }
        if response.drag_stopped() || response.lost_focus() {
            params.doc.history_mut().end_gesture();
        }
        ui.add_space(4.0);
        let used = shown_order(chain.clone());
        for (b, &v) in chain.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(38.0, 20.0), Sense::click());
            let painter = ui.painter();
            let color = variation_color(v);
            let shown = v == state.variation;
            painter.rect_filled(
                rect,
                4.0,
                color.gamma_multiply(if shown { 1.0 } else { 0.45 }),
            );
            if playing.map(|k| k / 16) == Some(b) {
                painter.rect_stroke(
                    rect,
                    4.0,
                    Stroke::new(1.5, Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                variation_name(v),
                FontId::proportional(10.5),
                Color32::WHITE.gamma_multiply(if shown { 1.0 } else { 0.7 }),
            );
            let response = response.on_hover_text(crate::trf!(
                "Bar {n}: click to change what it plays, right-click for every choice",
                n = b + 1
            ));
            let mut picked = None;
            if response.clicked() {
                // The next variation in use, or B when there is only one.
                let next = used
                    .iter()
                    .position(|&x| x == v)
                    .and_then(|i| used.get(i + 1))
                    .copied()
                    .unwrap_or(if used.len() < 2 && v == A {
                        B
                    } else {
                        used.first().copied().unwrap_or(A)
                    });
                picked = Some(next);
            }
            response.context_menu(|ui| {
                for &x in &used {
                    if ui.selectable_label(x == v, variation_name(x)).clicked() {
                        picked = Some(x);
                        ui.close();
                    }
                }
                ui.separator();
                for x in [B, FILL]
                    .into_iter()
                    .filter(|x| !chain.contains(x))
                    .chain(next_letter(&chain))
                {
                    if ui
                        .button(crate::trf!("New: {name}", name = variation_name(x)))
                        .clicked()
                    {
                        picked = Some(x);
                        ui.close();
                    }
                }
            });
            if let Some(next) = picked.filter(|&n| n != v) {
                let mut changed = chain.clone();
                let mut notes = assign(&notes, &grid, &mut changed, b, next, swing);
                if next == FILL && !chain.contains(&FILL) {
                    notes = build_up(&notes, &grid, b, swing);
                }
                let write = Write {
                    notes,
                    chain: &changed,
                    bars: None,
                };
                store(params, &clip, &grid, write, tr("Chain"), false);
                state.variation = next;
            }
        }
    });

    ui.add_space(6.0);
    let source = bars.first().copied();
    let first = source.unwrap_or(0) * 16;
    let count = (grid.steps - first.min(grid.steps)).min(16);
    let playing_c = playing
        .filter(|k| chain.get(k / 16) == Some(&state.variation))
        .map(|k| k % 16);
    let read =
        |pad: usize, c: usize| source.and_then(|_| read_step(&notes, &grid, pad, first + c, swing));
    let label_width = 118.0;
    let row = 20.0;
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
    if let Some(c) = playing_c.filter(|c| *c < count) {
        let x = cells.left() + c as f32 * cell_width;
        painter.rect_filled(
            Rect::from_min_size(pos2(x, rect.top()), vec2(cell_width, rect.height())),
            0.0,
            Color32::from_white_alpha(14),
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
            FontId::proportional(11.5),
            name_color,
        );
        for c in 0..count {
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
            let Some(n) = read(pad, c) else {
                continue;
            };
            // The step's color rises with its velocity.
            painter.rect_filled(cell, 3.0, color.gamma_multiply(0.3));
            let h = cell.height() * (0.25 + 0.75 * f32::from(n.velocity) / 127.0);
            painter.rect_filled(
                Rect::from_min_max(pos2(cell.left(), cell.bottom() - h), cell.max),
                3.0,
                color,
            );
            if playing_c == Some(c) {
                painter.rect_filled(cell, 3.0, Color32::from_white_alpha(50));
            }
            // A roll splits the step in its hits, a flam marks its left edge.
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
                    Color32::WHITE,
                );
            }
            let dragged = matches!(state.gesture, Some(Gesture::Velocity { pad: g, c: gc, .. }) if g == pad && gc == c);
            let text = if dragged {
                Some(n.velocity.to_string())
            } else {
                (n.probability < 100).then(|| format!("{} %", n.probability))
            };
            if let Some(text) = text {
                painter.text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(10.0),
                    Color32::WHITE,
                );
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

    // Click sets or clears a step, dragging across paints, dragging a step up or down sets
    // its velocity; a click on a name selects the pad.
    let pointer = response.interact_pointer_pos();
    let down = response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    let set = |notes: &[Note], pad: usize, c: usize, on: bool| {
        edit_bars(notes, &grid, &bars, c, |n, k| {
            set_step(n, &grid, pad, k, on, swing)
        })
    };
    if down && let Some(pos) = pointer {
        let here = cell_at(pos);
        match state.gesture {
            None => match here {
                Some((r, c)) if c == usize::MAX => *selected = LAYOUT[r],
                Some((r, c)) if c < count && source.is_some() => {
                    let pad = LAYOUT[r];
                    *selected = pad;
                    let on = read(pad, c).is_some();
                    state.gesture = Some(Gesture::Press {
                        pad,
                        c,
                        on,
                        y: pos.y,
                    });
                }
                _ => {}
            },
            Some(Gesture::Press { pad, c, on, y }) => {
                let moved = here != Some((LAYOUT.iter().position(|&p| p == pad).unwrap_or(0), c));
                if on && (pos.y - y).abs() > 3.0 && here.is_some_and(|h| h.1 == c) {
                    let from = read(pad, c).map_or(100, |s| s.velocity);
                    state.gesture = Some(Gesture::Velocity { pad, c, y, from });
                } else if moved {
                    let mut changed = set(&notes, pad, c, !on);
                    if let Some((r, k)) = here.filter(|h| h.1 < count) {
                        changed = set(&changed, LAYOUT[r], k, !on);
                    }
                    let write = Write {
                        notes: changed,
                        chain: &chain,
                        bars: None,
                    };
                    store(params, &clip, &grid, write, tr("Steps"), true);
                    state.gesture = Some(Gesture::Paint(!on));
                }
            }
            Some(Gesture::Paint(on)) => {
                if let Some((r, c)) = here.filter(|h| h.1 < count)
                    && read(LAYOUT[r], c).is_some() != on
                {
                    let write = Write {
                        notes: set(&notes, LAYOUT[r], c, on),
                        chain: &chain,
                        bars: None,
                    };
                    store(params, &clip, &grid, write, tr("Steps"), true);
                }
            }
            Some(Gesture::Velocity { pad, c, y, from }) => {
                let velocity = (f32::from(from) + (y - pos.y) * 0.8).clamp(1.0, 127.0) as u8;
                if let Some(step) = read(pad, c).filter(|s| s.velocity != velocity) {
                    let step = Step { velocity, ..step };
                    let changed = edit_bars(&notes, &grid, &bars, c, |n, k| {
                        write_step(n, &grid, pad, k, swing, Some(step))
                    });
                    let write = Write {
                        notes: changed,
                        chain: &chain,
                        bars: None,
                    };
                    store(params, &clip, &grid, write, tr("Velocity"), true);
                }
            }
        }
    } else if let Some(gesture) = state.gesture.take() {
        if let Gesture::Press { pad, c, on, .. } = gesture {
            let write = Write {
                notes: set(&notes, pad, c, !on),
                chain: &chain,
                bars: None,
            };
            store(params, &clip, &grid, write, tr("Steps"), false);
        }
        params.doc.history_mut().end_gesture();
    }

    // Right-click a step for its roll, flam, chance and velocity.
    if response.secondary_clicked() {
        state.menu = pointer
            .and_then(cell_at)
            .filter(|&(_, c)| c < count && source.is_some())
            .map(|(r, c)| (LAYOUT[r], c));
        if let Some((pad, _)) = state.menu {
            *selected = pad;
        }
    }
    response.context_menu(|ui| {
        let Some((pad, c)) = state.menu else {
            ui.close();
            return;
        };
        let before = read(pad, c);
        let mut step = before.unwrap_or(Step::new(pad));
        ui.label(
            RichText::new(crate::trf!(
                "{pad}: step {n}",
                pad = tr(VOICES[pad].1),
                n = c + 1
            ))
            .strong(),
        );
        ui.add_space(4.0);
        egui::Grid::new("drum-step-menu")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(tr("Roll"));
                ui.horizontal(|ui| {
                    for hits in 1..=ROLLS {
                        ui.selectable_value(&mut step.hits, hits, format!("×{hits}"));
                    }
                });
                ui.end_row();
                ui.label(tr("Flam"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut step.flam, false, tr("Off"));
                    ui.selectable_value(&mut step.flam, true, tr("On"));
                });
                ui.end_row();
                ui.label(tr("Chance"));
                ui.horizontal(|ui| {
                    for chance in [100, 75, 50, 25] {
                        ui.selectable_value(&mut step.probability, chance, format!("{chance} %"));
                    }
                });
                ui.end_row();
                ui.label(tr("Velocity"));
                ui.horizontal(|ui| {
                    for (name, velocity) in [("Ghost", 50), ("Normal", 100), ("Accent", 127)] {
                        ui.selectable_value(&mut step.velocity, velocity, tr(name));
                    }
                });
                ui.end_row();
            });
        let mut changed = (before != Some(step)).then_some(Some(step));
        if before.is_some() {
            ui.separator();
            if ui.button(tr("Remove the step")).clicked() {
                changed = Some(None);
                ui.close();
            }
        }
        if let Some(step) = changed {
            let write = Write {
                notes: edit_bars(&notes, &grid, &bars, c, |n, k| {
                    write_step(n, &grid, pad, k, swing, step)
                }),
                chain: &chain,
                bars: None,
            };
            store(params, &clip, &grid, write, tr("Steps"), false);
        }
    });

    ui.add_space(4.0);
    ui.label(
        RichText::new(tr(
            "Click to set a step, drag across to paint, drag a step up or down for its velocity, right-click for its roll, flam and chance.",
        ))
        .size(11.0)
        .color(p.weak),
    );
    if params.doc.player().engine().playing() {
        ui.ctx().request_repaint();
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

    #[test]
    fn variations_edit_their_bars_together() {
        let g = Grid {
            steps: 64,
            ..grid()
        };
        let (kick, snare) = (pad_named("kick").unwrap(), pad_named("snare").unwrap());
        assert_eq!(chain(&[], &[], &g, 50.0), [A; 4]);
        let all = bars_of(&[A; 4], A);
        let notes = edit_bars(&[], &g, &all, 0, |n, k| {
            set_step(n, &g, kick, k, true, 50.0)
        });
        let kicks: Vec<u64> = notes.iter().map(|n| n.start).collect();
        assert_eq!(kicks, [0, 16_000, 32_000, 48_000]);
        let (notes, ab, bars) = add_variation(&notes, &g, &[A; 4], B, 50.0);
        assert_eq!((ab.as_slice(), bars), (&[A, B, A, B][..], 4));
        let notes = edit_bars(&notes, &g, &bars_of(&ab, B), 4, |n, k| {
            set_step(n, &g, snare, k, true, 50.0)
        });
        let snares: Vec<u64> = notes
            .iter()
            .filter(|n| voice_of(n.key) == Some(snare))
            .map(|n| n.start)
            .collect();
        assert_eq!(snares, [20_000, 52_000]);
        // Read back from the notes alone, B is found again.
        assert_eq!(chain(&[], &notes, &g, 50.0), [A, B, A, B]);
        let (filled, chain, _) = add_variation(&notes, &g, &ab, FILL, 50.0);
        assert_eq!(chain, [A, B, A, FILL]);
        let roll = read_step(&filled, &g, snare, 63, 50.0).unwrap();
        assert_eq!((roll.hits, roll.velocity), (4, 118));
        assert!(
            read_step(&filled, &g, kick, 48, 50.0).is_some(),
            "the fill starts from A"
        );
        assert!(read_step(&filled, &g, snare, 52, 50.0).is_none());
    }

    #[test]
    fn patterns_take_any_length_and_more_variations() {
        let g = Grid {
            origin: 0.0,
            step: 100.0,
            steps: 32,
        };
        let notes = set_step(&[], &g, 0, 0, true, 50.0);
        let notes = set_step(&notes, &g, 1, 20, true, 50.0);
        // Longer: the new bars repeat the chain.
        let (long, chain) = resize(&notes, &g, &[A, B], 5, 50.0);
        assert_eq!(chain, [A, B, A, B, A]);
        let g5 = Grid { steps: 80, ..g };
        assert!(read_step(&long, &g5, 0, 64, 50.0).is_some());
        assert!(read_step(&long, &g5, 1, 52, 50.0).is_some());
        // Shorter: the bars past the end go with their notes.
        let (short, chain) = resize(&long, &g5, &chain, 1, 50.0);
        assert_eq!((chain.as_slice(), short.len()), (&[A][..], 1));
        // C on a new bar, from A.
        let (more, chain) =
            append_variation(&notes, &g, &[A, B], next_letter(&[A, B]).unwrap(), A, 50.0);
        assert_eq!(chain, [A, B, 3]);
        assert_eq!(variation_name(3), "C");
        assert!(read_step(&more, &Grid { steps: 48, ..g }, 0, 32, 50.0).is_some());
        assert_eq!(shown_order(vec![FILL, 3, A, B, FREE]), [A, B, 3, FILL]);
        assert_eq!(next_letter(&(0..=LAST).collect::<Vec<_>>()), None);
    }

    #[test]
    fn a_one_bar_clip_grows_for_its_variations() {
        let g = grid();
        let notes = set_step(&[], &g, 0, 0, true, 50.0);
        let (notes, chain, bars) = add_variation(&notes, &g, &[A], B, 50.0);
        assert_eq!((chain.as_slice(), bars, notes.len()), (&[A, B][..], 2, 2));
        let (_, chain, bars) = add_variation(&notes[..1], &g, &[A], FILL, 50.0);
        assert_eq!((chain.as_slice(), bars), (&[A, A, A, FILL][..], 4));
        // A bar set to a variation takes its notes.
        let g = Grid { steps: 32, ..g };
        let mut chain = vec![A, B];
        let copied = assign(&notes[..1], &g, &mut chain, 1, A, 50.0);
        assert_eq!(
            copied.iter().map(|n| n.start).collect::<Vec<_>>(),
            [0, 16_000]
        );
        assert_eq!(chain, [A, A]);
    }
}
