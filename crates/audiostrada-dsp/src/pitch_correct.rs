//! Pitch Correct: follows the pitch of a voice (YIN on a decimated copy, refined at full rate,
//! every 5 ms), picks the nearest note of the chosen scale and moves the voice there with
//! TD-PSOLA, grains of two periods laid one output period apart, which keeps the formants.
//! The retune speed glides the correction, humanize corrects the note's center only.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, param, time_coefficient};

pub const INFO: EffectInfo = EffectInfo {
    id: "pitch_correct",
    name: "Strada-Pitch",
    params: &PARAMS,
    meters: &["Input pitch", "Output pitch", "Target note", "Clarity"],
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

/// Chromatic, then [`SCALES`].
const SCALE_NAMES: [&str; 8] = [
    "Chromatic",
    "Major",
    "Minor",
    "Harmonic minor",
    "Dorian",
    "Pentatonic major",
    "Pentatonic minor",
    "Blues",
];

const PARAMS: [ParamInfo; 6] = [
    choice("key", "Key", &NOTE_NAMES, 0.0),
    choice("scale", "Scale", &SCALE_NAMES, 0.0),
    param("speed", "Retune speed", 0.0, 400.0, 25.0, "ms"),
    param("humanize", "Humanize", 0.0, 100.0, 0.0, "%"),
    param("amount", "Amount", 0.0, 100.0, 100.0, "%"),
    param("transpose", "Transpose", -12.0, 12.0, 0.0, "st"),
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

/// Whether `note` (MIDI) belongs to `scale` (0 = chromatic) on `key`.
pub fn in_scale(note: i32, key: i32, scale: usize) -> bool {
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

#[derive(Debug)]
pub struct PitchCorrect {
    rate: f32,
    values: [f32; 6],
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

impl Default for PitchCorrect {
    fn default() -> Self {
        let mut p = Self {
            rate: 0.0,
            values: PARAMS.map(|p| p.default),
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

impl PitchCorrect {
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
        let [key, scale, speed, humanize, amount, transpose] = self.values;
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
            .filter(|&n| in_scale(n, key, scale))
            .min_by(|&a, &b| {
                (heard - a as f32)
                    .abs()
                    .total_cmp(&(heard - b as f32).abs())
            })
            .unwrap_or(base);
        let note = match self.note {
            Some(current)
                if in_scale(current, key, scale)
                    && (heard - current as f32).abs()
                        < (heard - nearest as f32).abs() + HYSTERESIS =>
            {
                current
            }
            _ => nearest,
        };
        self.note = Some(note);
        let goal = (note as f32 - heard) * amount / 100.0 + transpose.round();
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

impl Effect for PitchCorrect {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        if sample_rate as f32 != self.rate {
            self.allocate(sample_rate as f32);
        } else {
            self.reset();
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

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

    fn latency(&self) -> u32 {
        self.latency as u32
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
        assert!(in_scale(61, 2, 1) && !in_scale(61, 0, 1) && in_scale(61, 0, 0));
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
}
