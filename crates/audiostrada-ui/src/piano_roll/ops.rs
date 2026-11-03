//! Note transformations: pure functions on a clip's notes and the chosen ones (indices), so
//! that they can be previewed live, undone as one step and tested.

use audiostrada_core::Note;
use audiostrada_core::tempo::TempoMap;

use super::music::{Scale, Voicing, build_chord, diatonic_intervals};

/// Shortest note made by a transformation, in samples.
pub const MIN_LENGTH: u64 = 16;

/// Clip time and musical time: clip samples from `origin` (project samples) to quarter-note
/// beats and back.
#[derive(Clone, Copy)]
pub struct Timing<'a> {
    pub tempo: &'a TempoMap,
    pub rate: u32,
    pub origin: u64,
}

impl Timing<'_> {
    pub fn beat(&self, t: u64) -> f64 {
        self.tempo.sample_to_beat(self.origin + t, self.rate)
    }

    pub fn time(&self, beat: f64) -> u64 {
        self.tempo
            .beat_to_sample(beat.max(0.0), self.rate)
            .saturating_sub(self.origin)
    }
}

/// A small repeatable random generator (xorshift).
pub struct Random(u32);

impl Random {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// From -1 to 1.
    pub fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }

    /// From 0 to 1.
    pub fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f64::from(self.0) / f64::from(u32::MAX)
    }
}

/// Quantize settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantize {
    /// Grid step in beats.
    pub step: f64,
    /// How far notes go to the grid, 0 to 1.
    pub strength: f64,
    /// Delay of every second step, 0 to 1 (1: a triplet feel).
    pub swing: f64,
    /// The ends too (else lengths are kept).
    pub ends: bool,
}

fn grid_beat(beat: f64, q: &Quantize) -> f64 {
    let step = q.step.max(1e-6);
    let index = (beat / step).round();
    let swung = if index as i64 % 2 != 0 {
        q.swing * step / 3.0
    } else {
        0.0
    };
    index * step + swung
}

pub fn quantize(notes: &mut [Note], chosen: &[usize], timing: Timing, q: &Quantize) {
    let strength = q.strength.clamp(0.0, 1.0);
    let toward = |from: f64, to: f64| from + (to - from) * strength;
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else { continue };
        let start = timing.beat(n.start);
        let end = timing.beat(n.start + n.length);
        let new_start = toward(start, grid_beat(start, q));
        let new_end = if q.ends {
            let snapped = grid_beat(end, q).max(grid_beat(start, q) + q.step * 0.5);
            toward(end, snapped)
        } else {
            new_start + (end - start)
        };
        n.start = timing.time(new_start);
        n.length = timing.time(new_end).saturating_sub(n.start).max(MIN_LENGTH);
    }
}

/// Lengthens each chosen note up to the next start (of any note), less `gap` samples; the last
/// one keeps its length.
pub fn legato(notes: &mut [Note], chosen: &[usize], gap: u64) {
    let starts: Vec<u64> = notes.iter().map(|n| n.start).collect();
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else { continue };
        if let Some(next) = starts.iter().filter(|&&s| s > n.start).min() {
            n.length = (next - n.start).saturating_sub(gap).max(MIN_LENGTH);
        }
    }
}

/// Varies start times by up to `timing` samples, velocities by up to `velocity` and lengths by
/// up to `length` (a fraction), repeatably from `seed`.
pub fn humanize(
    notes: &mut [Note],
    chosen: &[usize],
    timing: u64,
    velocity: f64,
    length: f64,
    seed: u32,
) {
    let mut random = Random::new(seed);
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else { continue };
        let shift = (random.signed() * timing as f64) as i64;
        n.start = (n.start as i64 + shift).max(0) as u64;
        n.velocity = (f64::from(n.velocity) + random.signed() * velocity)
            .round()
            .clamp(1.0, 127.0) as u8;
        let scale = 1.0 + random.signed() * length;
        n.length = ((n.length as f64 * scale) as u64).max(MIN_LENGTH);
    }
}

/// Spreads the notes that start together (within `window` samples) by `delay` samples each,
/// from the lowest (up), the highest (down), or alternately.
pub fn strum(notes: &mut [Note], chosen: &[usize], delay: u64, direction: Strum, window: u64) {
    let mut groups = groups_by_start(notes, chosen, window);
    for (g, group) in groups.iter_mut().enumerate() {
        group.sort_by_key(|&i| notes[i].key);
        let down = match direction {
            Strum::Up => false,
            Strum::Down => true,
            Strum::Alternate => g % 2 == 1,
        };
        if down {
            group.reverse();
        }
        let start = group.iter().map(|&i| notes[i].start).min().unwrap_or(0);
        for (k, &i) in group.iter().enumerate() {
            let n = &mut notes[i];
            let end = n.start + n.length;
            n.start = start + delay * k as u64;
            n.length = end.saturating_sub(n.start).max(MIN_LENGTH);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strum {
    Up,
    Down,
    Alternate,
}

/// The chosen notes grouped by start (chords), in time order.
fn groups_by_start(notes: &[Note], chosen: &[usize], window: u64) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = chosen
        .iter()
        .copied()
        .filter(|&i| i < notes.len())
        .collect();
    order.sort_by_key(|&i| notes[i].start);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in order {
        match groups.last_mut() {
            Some(g) if notes[i].start - notes[g[0]].start <= window => g.push(i),
            _ => groups.push(vec![i]),
        }
    }
    groups
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arpeggio {
    Up,
    Down,
    UpDown,
    Random,
    /// In the order the notes were played.
    AsPlayed,
}

/// Arpeggio settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arpeggiate {
    pub pattern: Arpeggio,
    /// Length of each step, in beats.
    pub step: f64,
    /// Sounding part of each step, 0 to 1.
    pub gate: f64,
    /// Octaves the pattern climbs (1 to 4).
    pub octaves: u8,
    pub seed: u32,
}

/// Replaces each chord (notes starting together) by its notes played one after another over
/// the chord's length. Returns all notes and the indices of the new ones.
pub fn arpeggiate(
    notes: &[Note],
    chosen: &[usize],
    timing: Timing,
    a: &Arpeggiate,
) -> (Vec<Note>, Vec<usize>) {
    let groups = groups_by_start(
        notes,
        chosen,
        timing.time(timing.beat(0) + 1.0 / 16.0).max(1),
    );
    let mut random = Random::new(a.seed);
    let mut out: Vec<Note> = notes
        .iter()
        .enumerate()
        .filter(|(i, _)| !chosen.contains(i))
        .map(|(_, n)| *n)
        .collect();
    let mut created = Vec::new();
    for group in groups {
        let mut keys: Vec<(u8, u8)> = group
            .iter()
            .map(|&i| (notes[i].key, notes[i].velocity))
            .collect();
        if a.pattern != Arpeggio::AsPlayed {
            keys.sort_unstable();
        }
        let base = keys.clone();
        for octave in 1..a.octaves.clamp(1, 4) {
            keys.extend(
                base.iter()
                    .map(|&(k, v)| (k.saturating_add(12 * octave).min(127), v)),
            );
        }
        let sequence: Vec<(u8, u8)> = match a.pattern {
            Arpeggio::Up | Arpeggio::AsPlayed | Arpeggio::Random => keys.clone(),
            Arpeggio::Down => keys.iter().rev().copied().collect(),
            Arpeggio::UpDown => {
                let mut s = keys.clone();
                s.extend(keys.iter().rev().skip(1).take(keys.len().saturating_sub(2)));
                s
            }
        };
        let start = group.iter().map(|&i| notes[i].start).min().unwrap_or(0);
        let end = group
            .iter()
            .map(|&i| notes[i].start + notes[i].length)
            .max()
            .unwrap_or(start);
        let first = timing.beat(start);
        let last = timing.beat(end);
        let steps = ((last - first) / a.step.max(1e-6)).round().max(1.0) as usize;
        for s in 0..steps {
            let (key, velocity) = match a.pattern {
                Arpeggio::Random => {
                    sequence[(random.unit() * sequence.len() as f64) as usize % sequence.len()]
                }
                _ => sequence[s % sequence.len()],
            };
            let at = timing.time(first + s as f64 * a.step);
            let off = timing.time(first + (s as f64 + a.gate.clamp(0.05, 1.0)) * a.step);
            created.push(out.len());
            out.push(Note {
                start: at,
                length: off.saturating_sub(at).max(MIN_LENGTH),
                key,
                velocity,
                ..notes[group[0]]
            });
        }
    }
    (out, created)
}

/// Adds to each chosen note another `steps` scale notes above (or below when negative); with
/// no scale, `steps` semitones. Returns the indices of the new notes.
pub fn harmonize(
    notes: &mut Vec<Note>,
    chosen: &[usize],
    scale: Option<Scale>,
    steps: i32,
) -> Vec<usize> {
    let mut added = Vec::new();
    for &i in chosen {
        let Some(n) = notes.get(i).copied() else {
            continue;
        };
        let key = match scale {
            Some(s) => s.step(n.key, steps),
            None => (i32::from(n.key) + steps).clamp(0, 127) as u8,
        };
        if key != n.key {
            added.push(notes.len());
            notes.push(Note { key, ..n });
        }
    }
    added
}

/// Turns each chosen note into the chord the scale builds on it (or a major/minor triad from
/// `intervals` without a scale), voiced. Returns the indices of the added notes.
pub fn chordify(
    notes: &mut Vec<Note>,
    chosen: &[usize],
    scale: Option<Scale>,
    size: usize,
    voicing: Voicing,
) -> Vec<usize> {
    let mut added = Vec::new();
    for &i in chosen {
        let Some(n) = notes.get(i).copied() else {
            continue;
        };
        let intervals = match scale {
            Some(s) => diatonic_intervals(s, n.key, size),
            None => super::music::ChordKind::Major.intervals().to_vec(),
        };
        let keys = build_chord(n.key, &intervals, 0, voicing);
        for key in keys.into_iter().filter(|&k| k != n.key) {
            added.push(notes.len());
            notes.push(Note { key, ..n });
        }
    }
    added
}

/// Variation settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vary {
    /// Share of the notes moved in time, 0 to 1.
    pub rhythm: f64,
    /// Share of the notes changed in pitch, 0 to 1.
    pub melody: f64,
    /// Grid step for the rhythm, in beats.
    pub step: f64,
    pub seed: u32,
}

/// A variation of the chosen notes: some move by a grid step, some go to a neighbouring note of
/// the scale (a semitone or a third without one), from `seed`.
pub fn vary(notes: &mut [Note], chosen: &[usize], timing: Timing, scale: Option<Scale>, v: &Vary) {
    let mut random = Random::new(v.seed);
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else { continue };
        if random.unit() < v.rhythm {
            let beat = timing.beat(n.start) + if random.unit() < 0.5 { -v.step } else { v.step };
            n.start = timing.time(beat.max(timing.beat(0)));
        }
        if random.unit() < v.melody {
            let steps =
                if random.unit() < 0.5 { -1 } else { 1 } * if random.unit() < 0.3 { 2 } else { 1 };
            n.key = match scale {
                Some(s) => s.step(n.key, steps),
                None => (i32::from(n.key) + steps * 2).clamp(0, 127) as u8,
            };
        }
    }
}

/// Random velocities around their current value (by up to `velocity`) and probabilities
/// between `probability` and 100 %, from `seed`.
pub fn randomize(notes: &mut [Note], chosen: &[usize], velocity: f64, probability: u8, seed: u32) {
    let mut random = Random::new(seed);
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else { continue };
        n.velocity = (f64::from(n.velocity) + random.signed() * velocity)
            .round()
            .clamp(1.0, 127.0) as u8;
        if probability < 100 {
            let p = f64::from(probability) + random.unit() * f64::from(100 - probability);
            n.probability = p.round().clamp(1.0, 100.0) as u8;
        }
    }
}

/// Velocity shaping: sets, scales around the mean, compresses toward the mean, or ramps from
/// one value to another over time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Velocity {
    Set(u8),
    /// Scales the distance to the mean: below 1 compresses, above expands.
    Contrast(f64),
    /// From the first note to the last, with a curve (1 straight, below 1 fast, above slow).
    Ramp(u8, u8, f64),
}

pub fn velocity(notes: &mut [Note], chosen: &[usize], shape: Velocity) {
    let picked: Vec<usize> = chosen
        .iter()
        .copied()
        .filter(|&i| i < notes.len())
        .collect();
    if picked.is_empty() {
        return;
    }
    let mean = picked
        .iter()
        .map(|&i| f64::from(notes[i].velocity))
        .sum::<f64>()
        / picked.len() as f64;
    let first = picked.iter().map(|&i| notes[i].start).min().unwrap_or(0);
    let last = picked.iter().map(|&i| notes[i].start).max().unwrap_or(0);
    for &i in &picked {
        let n = &mut notes[i];
        let v = match shape {
            Velocity::Set(v) => f64::from(v),
            Velocity::Contrast(f) => mean + (f64::from(n.velocity) - mean) * f,
            Velocity::Ramp(a, b, curve) => {
                let t = if last > first {
                    (n.start - first) as f64 / (last - first) as f64
                } else {
                    0.0
                };
                f64::from(a) + (f64::from(b) - f64::from(a)) * t.powf(curve.max(0.05))
            }
        };
        n.velocity = v.round().clamp(1.0, 127.0) as u8;
    }
}

/// Lengthens or shortens the chosen notes by `factor`, from the earliest of them.
pub fn stretch(notes: &mut [Note], chosen: &[usize], factor: f64) {
    let Some(first) = chosen
        .iter()
        .filter_map(|&i| notes.get(i))
        .map(|n| n.start)
        .min()
    else {
        return;
    };
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else { continue };
        n.start = first + ((n.start - first) as f64 * factor) as u64;
        n.length = ((n.length as f64 * factor) as u64).max(MIN_LENGTH);
    }
}

/// Plays the chosen notes backwards within the time they span.
pub fn reverse(notes: &mut [Note], chosen: &[usize]) {
    let picked: Vec<usize> = chosen
        .iter()
        .copied()
        .filter(|&i| i < notes.len())
        .collect();
    let (Some(first), Some(last)) = (
        picked.iter().map(|&i| notes[i].start).min(),
        picked
            .iter()
            .map(|&i| notes[i].start + notes[i].length)
            .max(),
    ) else {
        return;
    };
    for &i in &picked {
        let n = &mut notes[i];
        n.start = first + (last - (n.start + n.length));
    }
}

/// Turns the chosen notes upside down around the middle of their range (in the scale when
/// there is one).
pub fn mirror(notes: &mut [Note], chosen: &[usize], scale: Option<Scale>) {
    let picked: Vec<usize> = chosen
        .iter()
        .copied()
        .filter(|&i| i < notes.len())
        .collect();
    let (Some(low), Some(high)) = (
        picked.iter().map(|&i| notes[i].key).min(),
        picked.iter().map(|&i| notes[i].key).max(),
    ) else {
        return;
    };
    for &i in &picked {
        let key = (i32::from(high) + i32::from(low) - i32::from(notes[i].key)).clamp(0, 127) as u8;
        notes[i].key = scale.map_or(key, |s| s.snap(key));
    }
}

/// Splits the notes under `at` (clip samples) in two; returns the indices of the second
/// halves.
pub fn slice(notes: &mut Vec<Note>, targets: &[usize], at: u64) -> Vec<usize> {
    let mut added = Vec::new();
    for &i in targets {
        let Some(n) = notes.get(i).copied() else {
            continue;
        };
        if at <= n.start + MIN_LENGTH || at + MIN_LENGTH >= n.start + n.length {
            continue;
        }
        notes[i].length = at - n.start;
        added.push(notes.len());
        notes.push(Note {
            start: at,
            length: n.start + n.length - at,
            ..n
        });
    }
    added
}

/// Joins each chosen note with the next chosen one of the same key it touches.
pub fn glue(notes: &mut Vec<Note>, chosen: &[usize]) {
    let mut picked: Vec<usize> = chosen
        .iter()
        .copied()
        .filter(|&i| i < notes.len())
        .collect();
    picked.sort_by_key(|&i| (notes[i].key, notes[i].start));
    let mut gone = Vec::new();
    let mut k = 0;
    while k + 1 < picked.len() {
        let (a, b) = (picked[k], picked[k + 1]);
        if notes[a].key == notes[b].key && notes[b].start <= notes[a].start + notes[a].length {
            let end = (notes[a].start + notes[a].length).max(notes[b].start + notes[b].length);
            notes[a].length = end - notes[a].start;
            gone.push(b);
            picked.remove(k + 1);
        } else {
            k += 1;
        }
    }
    gone.sort_unstable();
    for i in gone.into_iter().rev() {
        notes.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: u64, length: u64, key: u8) -> Note {
        Note {
            start,
            length,
            key,
            ..Default::default()
        }
    }

    /// 120 BPM at 48 kHz: a beat is 24 000 samples.
    fn timing(tempo: &TempoMap) -> Timing<'_> {
        Timing {
            tempo,
            rate: 48_000,
            origin: 0,
        }
    }

    #[test]
    fn quantize_with_strength_swing_and_ends() {
        let tempo = TempoMap::default();
        let mut notes = vec![note(1_000, 10_000, 60), note(13_000, 5_000, 62)];
        let q = Quantize {
            step: 0.5,
            strength: 1.0,
            swing: 0.0,
            ends: false,
        };
        quantize(&mut notes, &[0, 1], timing(&tempo), &q);
        assert_eq!((notes[0].start, notes[0].length), (0, 10_000));
        assert_eq!(notes[1].start, 12_000);
        let mut swung = vec![note(12_500, 1_000, 60)];
        quantize(
            &mut swung,
            &[0],
            timing(&tempo),
            &Quantize { swing: 1.0, ..q },
        );
        assert_eq!(
            swung[0].start, 16_000,
            "an off-beat eighth goes a third later"
        );
        let mut half = vec![note(4_000, 1_000, 60)];
        quantize(
            &mut half,
            &[0],
            timing(&tempo),
            &Quantize {
                strength: 0.5,
                ends: true,
                ..q
            },
        );
        assert_eq!(half[0].start, 2_000);
    }

    #[test]
    fn strum_spreads_chords_and_arpeggios_walk_them() {
        let tempo = TempoMap::default();
        let chord = vec![
            note(0, 48_000, 64),
            note(0, 48_000, 60),
            note(0, 48_000, 67),
        ];
        let mut strummed = chord.clone();
        strum(&mut strummed, &[0, 1, 2], 500, Strum::Up, 10);
        let mut starts: Vec<(u8, u64)> = strummed.iter().map(|n| (n.key, n.start)).collect();
        starts.sort_unstable();
        assert_eq!(starts, [(60, 0), (64, 500), (67, 1_000)]);
        let a = Arpeggiate {
            pattern: Arpeggio::UpDown,
            step: 0.5,
            gate: 0.5,
            octaves: 1,
            seed: 1,
        };
        let (out, created) = arpeggiate(&chord, &[0, 1, 2], timing(&tempo), &a);
        let keys: Vec<u8> = created.iter().map(|&i| out[i].key).collect();
        assert_eq!(keys, [60, 64, 67, 64], "two beats of eighths, up and down");
        assert_eq!(out[created[1]].start, 12_000);
        assert_eq!(out[created[1]].length, 6_000);
    }

    #[test]
    fn harmony_follows_the_scale() {
        let c_major = Scale { root: 0, mode: 0 };
        let mut notes = vec![note(0, 100, 60), note(100, 100, 62)];
        let added = harmonize(&mut notes, &[0, 1], Some(c_major), 2);
        let keys: Vec<u8> = added.iter().map(|&i| notes[i].key).collect();
        assert_eq!(keys, [64, 65], "a third above, in the scale");
        let mut single = vec![note(0, 100, 62)];
        chordify(&mut single, &[0], Some(c_major), 3, Voicing::Close);
        let mut keys: Vec<u8> = single.iter().map(|n| n.key).collect();
        keys.sort_unstable();
        assert_eq!(keys, [62, 65, 69], "D minor");
    }

    #[test]
    fn shapes_reverse_mirror_slice_and_glue() {
        let mut notes = vec![note(0, 100, 60), note(200, 100, 67)];
        velocity(&mut notes, &[0, 1], Velocity::Ramp(20, 120, 1.0));
        assert_eq!((notes[0].velocity, notes[1].velocity), (20, 120));
        velocity(&mut notes, &[0, 1], Velocity::Contrast(0.5));
        assert_eq!((notes[0].velocity, notes[1].velocity), (45, 95));
        reverse(&mut notes, &[0, 1]);
        assert_eq!((notes[0].start, notes[1].start), (200, 0));
        mirror(&mut notes, &[0, 1], None);
        assert_eq!((notes[0].key, notes[1].key), (67, 60));
        let mut long = vec![note(0, 1_000, 60)];
        let added = slice(&mut long, &[0], 400);
        assert_eq!(
            (long[0].length, long[added[0]].start, long[added[0]].length),
            (400, 400, 600)
        );
        glue(&mut long, &[0, 1]);
        assert_eq!(long, [note(0, 1_000, 60)]);
        let mut randomized = vec![note(0, 10, 60); 20];
        let picked: Vec<usize> = (0..20).collect();
        randomize(&mut randomized, &picked, 20.0, 50, 7);
        assert!(randomized.iter().all(|n| (80..=120).contains(&n.velocity)));
        assert!(
            randomized
                .iter()
                .all(|n| (50..=100).contains(&n.probability))
        );
        assert!(randomized.iter().any(|n| n.probability < 100));
    }
}
