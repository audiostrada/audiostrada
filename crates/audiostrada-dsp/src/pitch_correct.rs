//! Pitch Correct: follows the pitch of a voice, picks the nearest note of the chosen scale and
//! moves the voice there with TD-PSOLA, grains of two periods laid one output period apart,
//! which keeps the formants. The retune speed glides the correction, humanize corrects the
//! note's center only, and a note already within the tolerance is left as sung. The scale can
//! be one of its own (notes picked on a keyboard). The staircase mode puts every note flat on
//! its target at once, for the stepped voice of the first automatic corrections.
//!
//! The engine is in [`crate::correct`]. The one this effect started with ([`Original`]: YIN's
//! first dip every 5 ms, marks a period apart, the correction of the latest reading) is kept
//! for the projects made with it.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::correct::{Corrector, Settings};
use crate::{Effect, EffectInfo, ParamInfo, choice, param, time_coefficient};

pub const INFO: EffectInfo = EffectInfo {
    id: "pitch_correct",
    name: "Strada-Correct",
    params: &PARAMS,
    meters: &[
        "Input pitch",
        "Output pitch",
        "Target note",
        "Clarity",
        "Key heard",
        "Scale heard",
        "Key certainty",
    ],
    instrument: false,
    files: &[],
    category: "Utility",
    description: "Automatic pitch correction for voice, from natural to the robot effect.",
};

pub const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Scales: name and the semitones in it from the root.
pub const SCALES: [(&str, [bool; 12]); 7] = [
    (
        "Major",
        [
            true, false, true, false, true, true, false, true, false, true, false, true,
        ],
    ),
    (
        "Minor",
        [
            true, false, true, true, false, true, false, true, true, false, true, false,
        ],
    ),
    (
        "Harmonic minor",
        [
            true, false, true, true, false, true, false, true, true, false, false, true,
        ],
    ),
    (
        "Dorian",
        [
            true, false, true, true, false, true, false, true, false, true, true, false,
        ],
    ),
    (
        "Pentatonic major",
        [
            true, false, true, false, true, false, false, true, false, true, false, false,
        ],
    ),
    (
        "Pentatonic minor",
        [
            true, false, false, true, false, true, false, true, false, false, true, false,
        ],
    ),
    (
        "Blues",
        [
            true, false, false, true, false, true, true, true, false, false, true, false,
        ],
    ),
];

/// Chromatic, then [`SCALES`], then the notes picked by hand.
pub const SCALE_NAMES: [&str; 9] = [
    "Chromatic",
    "Major",
    "Minor",
    "Harmonic minor",
    "Dorian",
    "Pentatonic major",
    "Pentatonic minor",
    "Blues",
    "Custom",
];
pub const CUSTOM: usize = 8;

/// How the voice is corrected. Projects saved before the modes existed are given the
/// original one when they are read.
pub const MODES: [&str; 3] = ["Natural", "Staircase", "Original"];
pub const MODE_HINTS: [&str; 3] = [
    "Follows the settings below, from a light touch to a tight correction",
    "Every note flat on its target at once: the stepped, synthetic voice",
    "The first engine of this effect, for the projects made with it",
];
pub const NATURAL: usize = 0;
pub const STAIRCASE: usize = 1;
pub const ORIGINAL: usize = 2;
/// Index of the mode among the parameters.
const MODE: usize = 8;
/// Meters of the key heard: its root (-1 without one), its scale as an index of
/// [`SCALE_NAMES`], and how sure it is (0 to 1).
pub const KEY_METER: usize = 4;

const PARAMS: [ParamInfo; 9] = [
    choice("key", "Key", &NOTE_NAMES, 0.0),
    choice("scale", "Scale", &SCALE_NAMES, 0.0),
    param("speed", "Retune speed", 0.0, 400.0, 25.0, "ms"),
    param("humanize", "Humanize", 0.0, 100.0, 0.0, "%"),
    param("amount", "Amount", 0.0, 100.0, 100.0, "%"),
    param("transpose", "Transpose", -12.0, 12.0, 0.0, "st"),
    param("notes", "Custom notes", 0.0, 4095.0, 4095.0, ""),
    param("tolerance", "Tolerance", 0.0, 50.0, 0.0, "cents"),
    choice("mode", "Mode", &MODES, 0.0),
];

const LOWEST_HZ: f32 = 70.0;
const HIGHEST_HZ: f32 = 1200.0;
/// Rate the pitch is first looked for at.
const DETECTION_HZ: f32 = 12_000.0;
const HOP_SECONDS: f32 = 0.005;
/// Below this mean square (-60 dBFS) there is nothing to follow.
const SILENCE: f32 = 1e-6;
/// How much further the current note may be than the nearest one before it changes.
const HYSTERESIS: f32 = 0.15;
/// Pseudo-period of the grains when the sound has no pitch.
const UNVOICED_SECONDS: f32 = 0.004;
const MARKS: usize = 128;
/// Full-rate lags tried on each side of the decimated estimate.
const REACH: usize = 16;

/// Whether `note` (MIDI) belongs to `scale` (0 = chromatic) on `key`; the custom scale is
/// `notes`, a bit per pitch class from C.
pub fn in_scale(note: i32, key: i32, scale: usize, notes: u32) -> bool {
    if scale == CUSTOM {
        // No note picked would leave nothing to correct to: then every note.
        return notes & 0xfff == 0 || notes >> note.rem_euclid(12) & 1 == 1;
    }
    match scale.checked_sub(1).and_then(|s| SCALES.get(s)) {
        Some((_, steps)) => steps[(note - key).rem_euclid(12) as usize],
        None => true,
    }
}

fn to_note(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

fn to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct Mark {
    at: f64,
    period: f32,
    voiced: bool,
}

/// The engine the effect started with.
#[derive(Debug)]
struct Original {
    rate: f32,
    values: [f32; 8],
    /// Detection: one sample in `factor`, low-passed first.
    factor: usize,
    lowpass: [Coefficients; 2],
    filters: [Biquad; 2],
    shortest: usize,
    longest: usize,
    window: usize,
    hop: usize,
    decimated: Vec<f32>,
    filled: usize,
    difference: Vec<f32>,
    /// Full-rate rings: input per channel, its mono sum, overlap-added grains and windows.
    mask: usize,
    input: [Vec<f32>; 2],
    mono: Vec<f32>,
    output: [Vec<f32>; 2],
    weight: Vec<f32>,
    written: u64,
    latency: usize,
    marks: [Mark; MARKS],
    count: usize,
    next_mark: f64,
    next_grain: f64,
    /// Pitch followed: run of voiced readings, the last three, the slow center.
    run: u32,
    recent: [f32; 3],
    pitch: f32,
    period: f32,
    center: f32,
    note: Option<i32>,
    correction: f32,
    clarity: f32,
}

impl Default for Original {
    fn default() -> Self {
        let mut p = Self {
            rate: 0.0,
            values: std::array::from_fn(|i| PARAMS[i].default),
            factor: 1,
            lowpass: [Coefficients::IDENTITY; 2],
            filters: [Biquad::default(); 2],
            shortest: 0,
            longest: 0,
            window: 0,
            hop: 1,
            decimated: Vec::new(),
            filled: 0,
            difference: Vec::new(),
            mask: 0,
            input: [Vec::new(), Vec::new()],
            mono: Vec::new(),
            output: [Vec::new(), Vec::new()],
            weight: Vec::new(),
            written: 0,
            latency: 0,
            marks: [Mark::default(); MARKS],
            count: 0,
            next_mark: 0.0,
            next_grain: 0.0,
            run: 0,
            recent: [0.0; 3],
            pitch: 0.0,
            period: 0.0,
            center: 0.0,
            note: None,
            correction: 0.0,
            clarity: 0.0,
        };
        p.allocate(48_000.0);
        p
    }
}

impl Original {
    fn allocate(&mut self, rate: f32) {
        self.rate = rate;
        self.factor = ((rate / DETECTION_HZ).round() as usize).max(1);
        let detection = rate / self.factor as f32;
        let q = [0.541_196_1, 1.306_563];
        self.lowpass = q.map(|q| {
            Coefficients::design(
                FilterKind::LowPass,
                f64::from(detection * 0.22),
                q,
                0.0,
                f64::from(detection),
            )
        });
        self.shortest = (detection / HIGHEST_HZ) as usize;
        self.longest = (detection / LOWEST_HZ).ceil() as usize + 1;
        self.window = self.longest;
        self.hop = ((detection * HOP_SECONDS).round() as usize).max(1);
        self.decimated = vec![0.0; self.window + self.longest + 2];
        self.difference = vec![0.0; self.longest + 2];
        let longest_period = rate / LOWEST_HZ;
        // A grain reaches a period after its center, which may wait half a period for its
        // nearest mark and needs a period of input after that mark.
        self.latency = (2.5 * longest_period).ceil() as usize + 4;
        let size = (self.latency + 4 * longest_period as usize + 64).next_power_of_two();
        self.mask = size - 1;
        self.input = [vec![0.0; size], vec![0.0; size]];
        self.mono = vec![0.0; size];
        self.output = [vec![0.0; size], vec![0.0; size]];
        self.weight = vec![0.0; size];
        self.reset();
    }

    fn unvoiced_period(&self) -> f32 {
        (self.rate * UNVOICED_SECONDS).round()
    }

    fn decimate(&mut self, mono: f32) {
        let mut x = f64::from(mono);
        for (f, c) in self.filters.iter_mut().zip(&self.lowpass) {
            x = f.process(c, x);
        }
        if !(self.written as usize).is_multiple_of(self.factor) {
            return;
        }
        self.decimated[self.filled] = x as f32;
        self.filled += 1;
        if self.filled == self.decimated.len() {
            self.analyse();
            self.decimated.copy_within(self.hop.., 0);
            self.filled -= self.hop;
        }
    }

    /// Distance between the latest `window` samples and the ones `lag` before, at full rate.
    fn full_rate_difference(&self, lag: usize, window: usize) -> f32 {
        let end = self.written as usize - 1;
        (0..window)
            .map(|i| {
                let at = end.wrapping_sub(i);
                let d = self.mono[at & self.mask] - self.mono[at.wrapping_sub(lag) & self.mask];
                d * d
            })
            .sum()
    }

    /// Refines a period found on the decimated copy, near `guess` full-rate samples.
    fn refine(&self, guess: f32) -> f32 {
        let window = self.window * self.factor;
        let center = guess.round() as usize;
        let reach = self.factor.min(REACH);
        if self.factor == 1 || center <= reach + 2 {
            return guess;
        }
        let mut sums = [0.0f32; 2 * REACH + 1];
        let sums = &mut sums[..2 * reach + 1];
        for (k, sum) in sums.iter_mut().enumerate() {
            *sum = self.full_rate_difference(center - reach + k, window);
        }
        let best = (1..sums.len() - 1)
            .min_by(|&a, &b| sums[a].total_cmp(&sums[b]))
            .unwrap_or(reach);
        if sums[best] > sums[0] || sums[best] > sums[sums.len() - 1] {
            return guess;
        }
        let (a, b, c) = (sums[best - 1], sums[best], sums[best + 1]);
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1e-12 {
            (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        (center - reach + best) as f32 + shift
    }

    fn analyse(&mut self) {
        let energy =
            self.decimated.iter().map(|s| s * s).sum::<f32>() / self.decimated.len() as f32;
        let found = (energy > SILENCE)
            .then(|| {
                crate::tuner::yin(
                    &self.decimated,
                    self.window,
                    self.shortest,
                    self.longest,
                    &mut self.difference,
                )
            })
            .flatten();
        let Some((lag, clarity)) = found else {
            self.run = 0;
            self.note = None;
            self.clarity = 0.0;
            self.pitch = 0.0;
            return;
        };
        self.clarity = clarity;
        let period = self.refine(lag * self.factor as f32);
        let reading = to_note(self.rate / period);
        self.recent[self.run as usize % 3] = reading;
        self.run += 1;
        let pitch = if self.run >= 3 {
            // Median of the last three: a lone octave jump is ignored.
            let [a, b, c] = self.recent;
            a.max(b).min(a.min(b).max(c))
        } else {
            reading
        };
        self.pitch = pitch;
        self.period = self.rate / to_hz(pitch);
        let hop_rate = self.rate / (self.hop * self.factor) as f32;
        let [
            key,
            scale,
            speed,
            humanize,
            amount,
            transpose,
            notes,
            tolerance,
        ] = self.values;
        let notes = notes.round().max(0.0) as u32;
        if self.run == 1 {
            self.center = pitch;
            self.correction = 0.0;
        } else {
            let c = time_coefficient(150.0, hop_rate);
            self.center = pitch + c * (self.center - pitch);
        }
        let heard = pitch + humanize / 100.0 * (self.center - pitch);
        let (key, scale) = (key.round() as i32, scale.round().max(0.0) as usize);
        let base = heard.round() as i32;
        let nearest = (base - 6..=base + 6)
            .filter(|&n| in_scale(n, key, scale, notes))
            .min_by(|&a, &b| {
                (heard - a as f32)
                    .abs()
                    .total_cmp(&(heard - b as f32).abs())
            })
            .unwrap_or(base);
        let note = match self.note {
            Some(current)
                if in_scale(current, key, scale, notes)
                    && (heard - current as f32).abs()
                        < (heard - nearest as f32).abs() + HYSTERESIS =>
            {
                current
            }
            _ => nearest,
        };
        self.note = Some(note);
        // Close enough already: left as sung (flex tuning).
        let off = note as f32 - heard;
        let off = if off.abs() * 100.0 < tolerance {
            0.0
        } else {
            off
        };
        let goal = off * amount / 100.0 + transpose.round();
        let c = time_coefficient(speed, hop_rate);
        self.correction = goal + c * (self.correction - goal);
    }

    fn place_marks(&mut self) {
        let now = (self.written - 1) as f64;
        while self.next_mark <= now {
            let voiced = self.run > 0;
            let period = if voiced {
                self.period
            } else {
                self.unvoiced_period()
            };
            self.marks[self.count % MARKS] = Mark {
                at: self.next_mark,
                period,
                voiced,
            };
            self.count += 1;
            self.next_mark += f64::from(period);
        }
    }

    /// The mark nearest to `at`, once a later one shows it is the nearest.
    fn nearest_mark(&self, at: f64) -> Option<Mark> {
        let newest = self.marks[(self.count.checked_sub(1)?) % MARKS];
        if newest.at < at + 0.5 * f64::from(newest.period) {
            return None;
        }
        let mut best = newest;
        for back in 1..self.count.min(MARKS) {
            let mark = self.marks[(self.count - 1 - back) % MARKS];
            if (mark.at - at).abs() < (best.at - at).abs() {
                best = mark;
            } else if mark.at < at {
                break;
            }
        }
        Some(best)
    }

    fn place_grains(&mut self) {
        let now = (self.written - 1) as f64;
        while let Some(mark) = self.nearest_mark(self.next_grain) {
            let half = f64::from(mark.period);
            if mark.at + half + 3.0 > now {
                return;
            }
            let at = self.next_grain;
            let first = (at - half).ceil().max(0.0) as u64;
            let last = (at + half).floor() as u64;
            for j in first..=last {
                let x = j as f64 - at;
                let w = (0.5 + 0.5 * (std::f64::consts::PI * x / half).cos()) as f32;
                let source = mark.at + x;
                let slot = j as usize & self.mask;
                for ch in 0..2 {
                    self.output[ch][slot] += w * cubic(&self.input[ch], self.mask, source);
                }
                self.weight[slot] += w;
            }
            let ratio = if mark.voiced {
                2f32.powf(self.correction / 12.0).clamp(0.5, 2.0)
            } else {
                1.0
            };
            self.next_grain += f64::from(mark.period / ratio);
        }
    }
}

/// Cubic Hermite interpolation of the ring `x` at the fractional position `at`.
fn cubic(x: &[f32], mask: usize, at: f64) -> f32 {
    let i = at.floor();
    let t = (at - i) as f32;
    let i = i as i64;
    let s = |k: i64| x[(i + k) as usize & mask];
    let (y0, y1, y2, y3) = (s(-1), s(0), s(1), s(2));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

impl Original {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let slot = self.written as usize & self.mask;
            self.input[0][slot] = *l;
            self.input[1][slot] = *r;
            let mono = 0.5 * (*l + *r);
            self.mono[slot] = mono;
            self.written += 1;
            self.decimate(mono);
            self.place_marks();
            self.place_grains();
            if self.written > self.latency as u64 {
                let slot = (self.written - 1 - self.latency as u64) as usize & self.mask;
                let w = self.weight[slot].max(1.0);
                *l = self.output[0][slot] / w;
                *r = self.output[1][slot] / w;
                self.output[0][slot] = 0.0;
                self.output[1][slot] = 0.0;
                self.weight[slot] = 0.0;
            } else {
                *l = 0.0;
                *r = 0.0;
            }
        }
    }

    fn reset(&mut self) {
        self.filters = [Biquad::default(); 2];
        self.filled = 0;
        for ring in self
            .input
            .iter_mut()
            .chain(self.output.iter_mut())
            .chain([&mut self.mono, &mut self.weight])
        {
            ring.fill(0.0);
        }
        self.written = 0;
        self.count = 0;
        self.next_mark = 0.0;
        self.next_grain = 0.0;
        self.run = 0;
        self.pitch = 0.0;
        self.note = None;
        self.correction = 0.0;
        self.clarity = 0.0;
    }

    fn meters(&self, out: &mut [f32]) {
        let voiced = self.run > 0;
        let values = if voiced {
            [
                self.pitch,
                self.pitch + self.correction,
                self.note.unwrap_or(0) as f32,
                self.clarity,
            ]
        } else {
            [0.0, 0.0, 0.0, self.clarity]
        };
        for (o, v) in out.iter_mut().zip(values) {
            *o = v;
        }
    }
}

/// The corrector: the engine of the mode chosen, the other one at rest.
#[derive(Debug)]
pub struct PitchCorrect {
    values: [f32; 9],
    engine: Corrector,
    original: Original,
    /// The mode of the last block, to start the one switched to from nothing.
    mode: usize,
}

impl Default for PitchCorrect {
    fn default() -> Self {
        Self {
            values: PARAMS.map(|p| p.default),
            engine: Corrector::new(48_000.0),
            original: Original::default(),
            mode: NATURAL,
        }
    }
}

impl PitchCorrect {
    fn mode(&self) -> usize {
        (self.values[MODE].round().max(0.0) as usize).min(MODES.len() - 1)
    }

    fn settings(&self) -> Settings {
        let [
            key,
            scale,
            speed,
            humanize,
            amount,
            transpose,
            notes,
            tolerance,
            _,
        ] = self.values;
        Settings {
            key: key.round() as i32,
            scale: scale.round().max(0.0) as usize,
            notes: notes.round().max(0.0) as u32,
            speed,
            humanize,
            amount,
            transpose,
            tolerance,
            staircase: self.mode() == STAIRCASE,
        }
    }
}

impl Effect for PitchCorrect {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        let rate = sample_rate as f32;
        if rate != self.original.rate {
            self.original.allocate(rate);
        } else {
            self.original.reset();
        }
        if rate != self.engine.rate() {
            self.engine.allocate(rate);
        } else {
            self.engine.reset();
        }
        self.mode = self.mode();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
        if let Some(v) = self.original.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mode = self.mode();
        if (mode == ORIGINAL) != (self.mode == ORIGINAL) {
            self.original.reset();
            self.engine.reset();
        }
        self.mode = mode;
        if mode == ORIGINAL {
            self.original.process(left, right);
        } else {
            let settings = self.settings();
            self.engine.process(left, right, &settings);
        }
    }

    fn reset(&mut self) {
        self.original.reset();
        self.engine.reset();
    }

    /// The same in every mode.
    fn latency(&self) -> u32 {
        self.engine.latency() as u32
    }

    fn meters(&self, out: &mut [f32]) {
        if self.mode == ORIGINAL {
            out.fill(0.0);
            if let Some(key) = out.get_mut(KEY_METER) {
                *key = -1.0;
            }
            self.original.meters(out);
        } else {
            self.engine.meters(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A voice-like tone: a fundamental and decaying harmonics.
    fn voice(hz: impl Fn(f32) -> f32, seconds: f32) -> Vec<f32> {
        let mut phase = 0.0f32;
        (0..(seconds * RATE) as usize)
            .map(|i| {
                phase += hz(i as f32 / RATE) / RATE;
                let t = phase * std::f32::consts::TAU;
                0.3 * t.sin() + 0.15 * (2.0 * t).sin() + 0.08 * (3.0 * t).sin()
            })
            .collect()
    }

    fn run(effect: &mut PitchCorrect, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        for block in input.chunks(256) {
            let (mut l, mut r) = (block.to_vec(), block.to_vec());
            effect.process(&mut l, &mut r);
            assert_eq!(l, r);
            out.extend(l);
        }
        out
    }

    fn corrector(settings: &[(usize, f32)]) -> PitchCorrect {
        let mut p = PitchCorrect::default();
        p.prepare(RATE as u32, 256);
        for &(i, v) in settings {
            p.set_param(i, v);
        }
        p
    }

    fn pitch_of(x: &[f32]) -> f32 {
        let mut difference = vec![0.0; 2048];
        crate::tuner::detect(&x[x.len() - 4096..], RATE, &mut difference)
            .unwrap()
            .0
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn brings_a_sharp_voice_to_the_note() {
        for (hz, expected) in [(220.0 * 2f32.powf(0.3 / 12.0), 220.0), (98.0 * 0.985, 98.0)] {
            let mut p = corrector(&[(2, 0.0)]);
            let out = run(&mut p, &voice(|_| hz, 1.0));
            let found = pitch_of(&out);
            let cents = 1200.0 * (found / expected).log2();
            assert!(
                cents.abs() < 4.0,
                "{hz} Hz came out at {found} ({cents} cents)"
            );
        }
    }

    #[test]
    fn follows_the_scale_and_the_key() {
        // C# is not in C major: a little above it, D is nearer than C.
        let hz = to_hz(61.1);
        let mut p = corrector(&[(1, 1.0), (2, 0.0)]);
        let out = run(&mut p, &voice(|_| hz, 1.0));
        let cents = 1200.0 * (pitch_of(&out) / to_hz(62.0)).log2();
        assert!(cents.abs() < 4.0, "{cents}");
        let mut meters = [0.0; 4];
        p.meters(&mut meters);
        assert_eq!(meters[2], 62.0);
        // In D major, C# is a note of the scale.
        let mut p = corrector(&[(0, 2.0), (1, 1.0), (2, 0.0)]);
        run(&mut p, &voice(|_| hz, 0.5));
        p.meters(&mut meters);
        assert_eq!(meters[2], 61.0);
        assert!(in_scale(61, 2, 1, 0) && !in_scale(61, 0, 1, 0) && in_scale(61, 0, 0, 0));
        // C, E and G picked: C# is out, E in, whatever the key.
        let triad = 1 | 1 << 4 | 1 << 7;
        assert!(!in_scale(61, 0, CUSTOM, triad) && in_scale(64, 5, CUSTOM, triad));
        assert!(in_scale(61, 0, CUSTOM, 0), "nothing picked: every note");
    }

    #[test]
    fn transposes_and_keeps_the_level() {
        let mut p = corrector(&[(2, 0.0), (5, 3.0)]);
        let input = voice(|_| 220.0, 1.0);
        let out = run(&mut p, &input);
        let cents = 1200.0 * (pitch_of(&out) / to_hz(to_note(220.0) + 3.0)).log2();
        assert!(cents.abs() < 4.0, "{cents}");
        let level = 20.0 * (rms(&out[24_000..]) / rms(&input[24_000..])).log10();
        assert!(level.abs() < 2.0, "{level} dB");
    }

    #[test]
    fn without_correction_the_sound_is_only_delayed() {
        let mut noise = 1u32;
        let hiss: Vec<f32> = (0..24_000)
            .map(|_| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (noise >> 8) as f32 / (1u32 << 24) as f32 * 0.4 - 0.2
            })
            .collect();
        for (input, settings) in [(voice(|_| 180.0, 1.0), [(4, 0.0)]), (hiss, [(2, 0.0)])] {
            let mut p = corrector(&settings);
            let out = run(&mut p, &input);
            let delay = p.latency() as usize;
            let error = (12_000..input.len())
                .map(|i| (out[i] - input[i - delay]).abs())
                .fold(0.0f32, f32::max);
            assert!(error < 0.02, "{error}");
        }
    }

    #[test]
    fn retune_speed_glides_and_humanize_keeps_vibrato() {
        let mut meters = [0.0; 4];
        let sharp = to_hz(57.3);
        let mut p = corrector(&[(2, 200.0)]);
        run(&mut p, &voice(|_| sharp, 0.15));
        p.meters(&mut meters);
        let early = meters[1] - 57.0;
        run(&mut p, &voice(|_| sharp, 1.5));
        p.meters(&mut meters);
        assert!(early > 0.05 && early < 0.28, "glides: {early}");
        assert!((meters[1] - 57.0).abs() < 0.02, "arrives: {meters:?}");

        let vibrato = |t: f32| to_hz(57.0 + 0.4 * (t * 5.5 * std::f32::consts::TAU).sin());
        for (humanize, kept) in [(0.0, false), (100.0, true)] {
            let mut p = corrector(&[(2, 0.0), (3, humanize)]);
            let input = voice(vibrato, 1.0);
            let (mut low, mut high) = (f32::MAX, f32::MIN);
            for (i, block) in input.chunks(256).enumerate() {
                let (mut l, mut r) = (block.to_vec(), block.to_vec());
                p.process(&mut l, &mut r);
                p.meters(&mut meters);
                if i > 100 {
                    low = low.min(meters[1]);
                    high = high.max(meters[1]);
                }
            }
            assert_eq!(high - low > 0.4, kept, "humanize {humanize}: {low}..{high}");
        }
    }

    #[test]
    fn silence_stays_silent_after_the_latency() {
        let mut p = corrector(&[]);
        let delay = p.latency();
        assert!(delay > 1500 && delay < 2000, "{delay}");
        let out = run(&mut p, &vec![0.0; 9600]);
        assert!(out.iter().all(|s| *s == 0.0));
        p.prepare(96_000, 256);
        assert!(p.latency() > 2 * delay - 16);
        let out = run(&mut p, &voice(|_| 440.0, 0.3));
        assert!(out.iter().all(|s| s.is_finite()));
    }

    /// The settings by their place in the table.
    const SPEED: usize = 2;
    const TRANSPOSE: usize = 5;

    /// A voice as a microphone hears it: the breath loud in its upper harmonics, around
    /// 2 kHz, `breath` times a full-scale noise.
    fn breathy(hz: f32, seconds: f32, breath: f32) -> Vec<f32> {
        let mut noise = 777u32;
        let band = Coefficients::design(FilterKind::BandPass, 2000.0, 1.5, 0.0, f64::from(RATE));
        let mut filter = Biquad::default();
        voice(|_| hz, seconds)
            .into_iter()
            .map(|s| {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
                s + breath * filter.process(&band, f64::from(white)) as f32
            })
            .collect()
    }

    /// The meters after each block of `input`, from the block `skip` on.
    fn follow(p: &mut PitchCorrect, input: &[f32], skip: usize) -> Vec<[f32; 7]> {
        let mut seen = Vec::new();
        for (i, block) in input.chunks(256).enumerate() {
            let (mut l, mut r) = (block.to_vec(), block.to_vec());
            p.process(&mut l, &mut r);
            let mut meters = [0.0; 7];
            p.meters(&mut meters);
            if i >= skip {
                seen.push(meters);
            }
        }
        seen
    }

    /// The pitch of each stretch of 4096 samples of `x`, as notes.
    fn pitches(x: &[f32]) -> Vec<f32> {
        let mut difference = vec![0.0; 2048];
        x.chunks_exact(4096)
            .filter_map(|w| crate::tuner::detect(w, RATE, &mut difference))
            .map(|found| to_note(found.0))
            .collect()
    }

    #[test]
    fn reads_the_pitch_of_a_voice_within_a_cent() {
        for hz in [82.4f32, 146.8, 261.6, 440.0, 880.0] {
            // The phase kept in double precision: the tone is on its pitch to the cent.
            let input: Vec<f32> = (0..(0.6 * RATE) as usize)
                .map(|i| {
                    let t = (f64::from(hz) * i as f64 / f64::from(RATE)).fract() as f32;
                    let t = t * std::f32::consts::TAU;
                    0.3 * t.sin() + 0.15 * (2.0 * t).sin() + 0.08 * (3.0 * t).sin()
                })
                .collect();
            let mut p = corrector(&[]);
            for meters in follow(&mut p, &input, 40) {
                let cents = 100.0 * (meters[0] - to_note(hz));
                assert!(cents.abs() < 1.0, "{hz} Hz read {cents} cents off");
            }
        }
    }

    #[test]
    fn follows_a_voice_under_its_breath_where_the_original_engine_loses_it() {
        // A3 sung 30 cents sharp, the breath 6 dB under the voice.
        let input = breathy(to_hz(57.3), 2.0, 0.8);
        let on_the_note = |mode: usize| {
            let mut p = corrector(&[(SPEED, 0.0), (MODE, mode as f32)]);
            let seen = follow(&mut p, &input, 40);
            let held = seen
                .iter()
                .filter(|m| m[0] > 0.0 && (m[1] - 57.0).abs() < 0.05)
                .count();
            held as f32 / seen.len() as f32
        };
        let (new, original) = (on_the_note(NATURAL), on_the_note(ORIGINAL));
        assert!(new > 0.8, "the new engine holds the note {new} of the time");
        assert!(original < 0.2, "the original one did {original}");
    }

    #[test]
    fn the_staircase_flattens_a_vibrato_the_original_engine_let_through() {
        // 80 cents of vibrato from end to end around an A3 sung 20 cents sharp.
        let vibrato = |t: f32| to_hz(57.2 + 0.4 * (t * 5.5 * std::f32::consts::TAU).sin());
        let input = voice(vibrato, 2.0);
        let swing = |settings: &[(usize, f32)]| {
            let out = run(&mut corrector(settings), &input);
            let heard = pitches(&out[24_000..]);
            let (low, high) = heard
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), n| (lo.min(*n), hi.max(*n)));
            (low, high)
        };
        let (low, high) = swing(&[(MODE, STAIRCASE as f32)]);
        assert!(low > 56.95 && high < 57.05, "the staircase: {low}..{high}");
        let (low, high) = swing(&[(SPEED, 0.0), (MODE, ORIGINAL as f32)]);
        assert!(high - low > 0.4, "the original engine: {low}..{high}");
    }

    #[test]
    fn the_staircase_reaches_a_new_note_within_30_ms() {
        // Legato from an A3 to a C4, each a little off.
        let sung = |t: f32| to_hz(if t < 0.5 { 57.25 } else { 59.8 });
        let mut p = corrector(&[(MODE, STAIRCASE as f32)]);
        let seen = follow(&mut p, &voice(sung, 1.0), 0);
        let block = 256.0 / RATE;
        let before = seen
            .iter()
            .rposition(|m| m[2] == 57.0)
            .expect("the first note is followed");
        let after = seen
            .iter()
            .position(|m| m[2] == 60.0 && (m[1] - 60.0).abs() < 0.1)
            .expect("the second note is reached");
        let seconds = (after as f32 - before as f32) * block;
        assert!(seconds < 0.03, "{seconds} s from one note to the next");
        // And every reading after it is on the note.
        for m in &seen[after..] {
            assert!((m[1] - 60.0).abs() < 0.02, "{m:?}");
        }
    }

    #[test]
    fn a_change_of_note_leaves_no_click() {
        // A melody of half steps sung legato, moved to the staircase: the output bends no
        // more sharply from a sample to the next than the voice itself does.
        let melody = |t: f32| to_hz(57.3 + ((t / 0.25).floor() % 3.0) * 1.1);
        let input = voice(melody, 2.0);
        let sharpest = |x: &[f32]| {
            x.windows(3)
                .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
                .fold(0.0, f32::max)
        };
        for mode in [NATURAL, STAIRCASE] {
            let mut p = corrector(&[(SPEED, 0.0), (MODE, mode as f32)]);
            let out = run(&mut p, &input);
            let (dry, wet) = (sharpest(&input[4800..]), sharpest(&out[4800..]));
            assert!(wet < 1.25 * dry, "mode {mode}: {wet} against {dry}");
            let level = 20.0 * (rms(&out[24_000..]) / rms(&input[24_000..])).log10();
            assert!(level.abs() < 0.5, "mode {mode}: {level} dB");
        }
    }

    #[test]
    fn the_key_of_what_is_sung_is_heard() {
        // "Frère Jacques" and so on in D major, a quarter of a second a note.
        let tune = [
            0, 2, 4, 0, 0, 2, 4, 0, 4, 5, 7, 7, 4, 5, 7, 7, 9, 7, 5, 4, 0, 0,
        ];
        let sung = move |t: f32| to_hz(62.0 + tune[(t / 0.25) as usize % tune.len()] as f32);
        let mut p = corrector(&[]);
        let mut meters = [0.0; 7];
        p.meters(&mut meters);
        assert_eq!(meters[KEY_METER], -1.0, "nothing heard yet");
        let seen = follow(&mut p, &voice(sung, 6.0), 0);
        let last = seen.last().unwrap();
        assert_eq!(
            (last[KEY_METER], last[KEY_METER + 1]),
            (2.0, 1.0),
            "D major"
        );
        assert!(last[KEY_METER + 2] > 0.6, "{last:?}");
        // The key outlives a stop of the transport.
        p.reset();
        p.meters(&mut meters);
        assert_eq!(meters[KEY_METER], 2.0);
    }

    #[test]
    fn the_original_engine_sounds_as_it_did() {
        // What it gave before the modes existed, for the projects that keep it.
        let input = voice(|t| to_hz(57.3 + 2.0 * t), 0.5);
        let mut p = corrector(&[(SPEED, 10.0), (TRANSPOSE, 2.0), (MODE, ORIGINAL as f32)]);
        let out = run(&mut p, &input);
        let sum: f64 = out.iter().map(|s| f64::from(s.abs())).sum();
        let probes = [out[6000], out[12_000], out[18_000], out[23_999]];
        assert!(
            (sum - ORIGINAL_SUM).abs() < 1e-2 * ORIGINAL_SUM.max(1.0),
            "{sum}"
        );
        for (got, expected) in probes.iter().zip(ORIGINAL_PROBES) {
            assert!((got - expected).abs() < 1e-3, "{probes:?}");
        }
        assert_eq!(
            p.latency(),
            corrector(&[]).latency(),
            "the modes share a latency"
        );
    }
    const ORIGINAL_SUM: f64 = 4347.552_928;
    const ORIGINAL_PROBES: [f32; 4] = [0.251_958_9, -0.011_172_57, -0.260_529_94, -0.048_771_17];

    #[test]
    fn a_note_within_the_tolerance_is_left_as_sung() {
        // 20 cents sharp of A3: corrected without tolerance, left alone with 30 cents.
        let sung = 220.0 * 2f32.powf(20.0 / 1200.0);
        for (tolerance, corrected) in [(0.0, true), (30.0, false)] {
            let mut p = corrector(&[(2, 0.0), (7, tolerance)]);
            let out = pitch_of(&run(&mut p, &voice(|_| sung, 1.0)));
            let cents = 1200.0 * (out / 220.0).log2();
            assert_eq!(
                cents.abs() < 8.0,
                corrected,
                "tolerance {tolerance}: {cents} cents"
            );
        }
    }
}
