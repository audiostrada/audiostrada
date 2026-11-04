//! Drive: overdrive, distortion, fuzz, tube, tape, transformer or rectifier saturation, with a
//! low cut before it and tone, bass and middle after it, run four times oversampled so the
//! clipping does not alias. Auto gain keeps the level where it came in whatever the drive;
//! drive, level and mix glide.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "drive",
    name: "Strada-Drive",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Color",
    description: "Overdrive, distortion, fuzz, tube, tape, transformer and rectifier, oversampled.",
};

/// Drive types, in parameter order (new types are added at the end).
pub const TYPES: [&str; 7] = [
    "Overdrive",
    "Distortion",
    "Fuzz",
    "Tube",
    "Tape",
    "Transformer",
    "Rectifier",
];

/// What each type sounds like.
pub const TYPE_HINTS: [&str; 7] = [
    "Soft diode clipping, as in a Tube Screamer: a warm, mid-forward crunch",
    "Hard clipping: tight and aggressive",
    "Squared-off and sputtering, with long sustain",
    "A triode stage: asymmetric, rich in even harmonics, round",
    "Tape saturation: the highs saturate first, the peaks are rounded off",
    "An output transformer: the lows thicken and saturate first",
    "Rectified fuzz: an octave up, ring-like",
];

const TAPE: u32 = 4;
const TRANSFORMER: u32 = 5;

const PARAMS: [ParamInfo; 10] = [
    choice("mode", "Type", &TYPES, 0.0),
    param("drive", "Drive", 0.0, 48.0, 18.0, "dB"),
    param("tone", "Tone", 500.0, 12000.0, 3500.0, "Hz"),
    param("level", "Level", -36.0, 12.0, -12.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("low_cut", "Low cut", 20.0, 600.0, 20.0, "Hz"),
    param("bias", "Bias", 0.0, 100.0, 0.0, "%"),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("mid", "Middle", -12.0, 12.0, 0.0, "dB"),
    switch("auto_gain", "Auto gain", 0.0),
];
const GLIDE_MS: f32 = 15.0;
/// How fast auto gain follows the level, in ms: slow enough not to pump.
const AUTO_MS: f32 = 400.0;

/// Filters of each channel, in signal order.
const LOW_CUT: usize = 0;
const EMPHASIS: usize = 1;
const DE_EMPHASIS: usize = 2;
const TONE: usize = 3;
const BASS: usize = 4;
const MID: usize = 5;
const FILTERS: usize = 6;

pub(crate) const FACTOR: usize = 4;
const MAX_TAPS: usize = 96;

/// Length of the oversampling filters: long for the studio processors (flat to 18 kHz at
/// 48 kHz, 24 samples of delay), short for the guitar rig, where the delay of each stage adds
/// up (8 samples).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taps {
    Short = 32,
    Long = 96,
}

impl Taps {
    /// Delay of a run through an [`Oversampler`] at the base rate, rounded.
    pub(crate) const fn latency(self) -> u32 {
        (self as u32 - 1).div_ceil(FACTOR as u32)
    }
}

/// Delay of the studio processors' oversampling, in samples.
pub(crate) const LATENCY: u32 = Taps::Long.latency();

/// Low-pass FIR for up- and downsampling by four: a windowed sinc just below the edge of the
/// base band (Blackman for the short one, Kaiser for the long one).
fn kernel(taps: Taps) -> [f32; MAX_TAPS] {
    let n = taps as usize;
    let centre = (n - 1) as f64 / 2.0;
    let cutoff = if taps == Taps::Long { 0.95 } else { 0.9 } / FACTOR as f64;
    let bessel = |x: f64| {
        let (mut term, mut sum) = (1.0, 1.0);
        for k in 1..32 {
            term *= (x / (2.0 * f64::from(k))).powi(2);
            sum += term;
        }
        sum
    };
    let mut k = [0.0f32; MAX_TAPS];
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().take(n).enumerate() {
        let x = i as f64 - centre;
        let sinc = if x.abs() < 1e-9 {
            1.0
        } else {
            (std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * cutoff * x)
        };
        let t = i as f64 / (n - 1) as f64;
        let window = match taps {
            Taps::Short => {
                0.42 - 0.5 * (std::f64::consts::TAU * t).cos()
                    + 0.08 * (2.0 * std::f64::consts::TAU * t).cos()
            }
            Taps::Long => {
                let r = 2.0 * t - 1.0;
                bessel(7.0 * (1.0 - r * r).max(0.0).sqrt()) / bessel(7.0)
            }
        };
        *v = (sinc * window) as f32;
        sum += sinc * window;
    }
    for v in &mut k {
        *v /= sum as f32;
    }
    k
}

/// Runs a nonlinearity at four times the rate (polyphase: only the samples that are not
/// stuffed zeros are filtered, and only the kept ones are decimated).
#[derive(Debug, Clone)]
pub(crate) struct Oversampler {
    taps: Taps,
    kernel: [f32; MAX_TAPS],
    /// Inputs then shaped samples, newest first, each written twice to read a window in one
    /// slice.
    input: [f32; 2 * MAX_TAPS / FACTOR],
    at_input: usize,
    shaped: [f32; 2 * MAX_TAPS],
    at_shaped: usize,
}

impl Oversampler {
    pub(crate) fn new(taps: Taps) -> Self {
        Self {
            taps,
            kernel: kernel(taps),
            input: [0.0; 2 * MAX_TAPS / FACTOR],
            at_input: 0,
            shaped: [0.0; 2 * MAX_TAPS],
            at_shaped: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.input.fill(0.0);
        self.shaped.fill(0.0);
    }

    pub(crate) fn latency(&self) -> u32 {
        self.taps.latency()
    }

    /// Runs `shape` at four times the rate on one sample.
    pub(crate) fn run(&mut self, x: f32, mut shape: impl FnMut(f32) -> f32) -> f32 {
        let n = self.taps as usize;
        let per_phase = n / FACTOR;
        self.at_input = (self.at_input + per_phase - 1) % per_phase;
        self.input[self.at_input] = x;
        self.input[self.at_input + per_phase] = x;
        let history = &self.input[self.at_input..self.at_input + per_phase];
        for phase in 0..FACTOR {
            // The zero-stuffed signal, gain restored by the factor.
            let up: f32 = history
                .iter()
                .enumerate()
                .map(|(i, v)| self.kernel[i * FACTOR + phase] * v)
                .sum();
            let y = shape(up * FACTOR as f32);
            self.at_shaped = (self.at_shaped + n - 1) % n;
            self.shaped[self.at_shaped] = y;
            self.shaped[self.at_shaped + n] = y;
        }
        self.shaped[self.at_shaped..self.at_shaped + n]
            .iter()
            .zip(&self.kernel[..n])
            .map(|(v, k)| v * k)
            .sum()
    }
}

/// A short whole-sample delay, to line the dry signal up with an oversampled one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Delay {
    line: [f32; 64],
    at: usize,
}

impl Default for Delay {
    fn default() -> Self {
        Self {
            line: [0.0; 64],
            at: 0,
        }
    }
}

impl Delay {
    /// `x` in, the sample of `delay` (below 64) frames ago out.
    pub(crate) fn tick(&mut self, x: f32, delay: usize) -> f32 {
        self.line[self.at] = x;
        let out = self.line[(self.at + 64 - delay.min(63)) % 64];
        self.at = (self.at + 1) % 64;
        out
    }
}

#[derive(Debug)]
pub struct Drive {
    rate: f32,
    values: [f32; 10],
    /// Drive, level and mix as they glide, and the auto gain.
    smooth: [f32; 3],
    auto: f32,
    /// Power of the input and of the driven sound, for the auto gain.
    power: [f32; 2],
    stages: [Oversampler; 2],
    /// The dry signal, delayed as much as the oversampled one.
    dry: [Delay; 2],
    coefficients: [Coefficients; FILTERS],
    filters: [[Biquad; FILTERS]; 2],
    dc: [(f32, f32); 2],
}

impl Default for Drive {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            smooth: [db_to_gain(18.0), db_to_gain(-12.0), 1.0],
            auto: 1.0,
            power: [0.0; 2],
            stages: [Oversampler::new(Taps::Long), Oversampler::new(Taps::Long)],
            dry: [Delay::default(); 2],
            coefficients: [Coefficients::IDENTITY; FILTERS],
            filters: [[Biquad::default(); FILTERS]; 2],
            dc: [(0.0, 0.0); 2],
        }
    }
}

fn shape(mode: u32, x: f32) -> f32 {
    match mode {
        // Soft and slightly asymmetric, like a diode overdrive.
        0 => {
            let y = (x + 0.2).tanh() - 0.2f32.tanh();
            y / (1.0 + 0.2f32.tanh())
        }
        1 => x.clamp(-1.0, 1.0) * 0.9 + (x * 2.0).tanh() * 0.1,
        2 => {
            let y = (2.5 * x).tanh();
            (y * 1.5).clamp(-0.8, 1.0)
        }
        // Grid current flattens the positive swing sooner than cutoff does the negative one.
        3 => {
            if x >= 0.0 {
                1.0 - (-x).exp()
            } else {
                ((1.6 * x).exp() - 1.0) / 1.6
            }
        }
        // Softer than tanh: the peaks round off slowly.
        TAPE => x / (1.0 + x * x).sqrt(),
        TRANSFORMER => {
            let t = (1.2 * x).tanh();
            0.85 * t + 0.15 * t * t
        }
        _ => {
            let t = (1.5 * x).tanh();
            0.5 * t + 0.8 * t.abs()
        }
    }
}

/// Transfer curve of a type with its bias (0 to 100 %), silent at rest.
pub fn transfer(mode: u32, bias: f32, x: f32) -> f32 {
    let offset = bias / 100.0 * 0.6;
    shape(mode, x + offset) - shape(mode, offset)
}

impl Effect for Drive {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        let v = self.values;
        self.smooth = [db_to_gain(v[1]), db_to_gain(v[3]), v[4] / 100.0];
        self.update();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [mode, drive, _, level, mix, _, bias, _, _, auto_gain] = self.values;
        let mode = mode.round() as u32;
        let targets = [db_to_gain(drive), db_to_gain(level), mix / 100.0];
        let glide = crate::time_coefficient(GLIDE_MS, self.rate);
        let follow = 1.0 - crate::time_coefficient(AUTO_MS, self.rate);
        let auto_gain = auto_gain >= 0.5;
        let c = &self.coefficients;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            for (s, t) in self.smooth.iter_mut().zip(targets) {
                *s = t + glide * (*s - t);
            }
            let [gain, out, wet] = self.smooth;
            let mut driven = [0.0f32; 2];
            let mut dry = [*l, *r];
            for ch in 0..2 {
                let f = &mut self.filters[ch];
                let mut x = f64::from(dry[ch]);
                for i in [LOW_CUT, EMPHASIS] {
                    x = f[i].process(&c[i], x);
                }
                let clipped = self.stages[ch].run(x as f32 * gain, |x| transfer(mode, bias, x));
                let clipped = f[DE_EMPHASIS].process(&c[DE_EMPHASIS], f64::from(clipped)) as f32;
                // DC blocker for the asymmetric shapes, then tone, bass and middle.
                let (x1, y1) = self.dc[ch];
                let y = clipped - x1 + 0.995 * y1;
                self.dc[ch] = (clipped, y);
                let mut toned = f64::from(y);
                for i in [TONE, BASS, MID] {
                    toned = f[i].process(&c[i], toned);
                }
                driven[ch] = toned as f32;
                dry[ch] = self.dry[ch].tick(dry[ch], LATENCY as usize);
            }
            // Auto gain: the driven sound brought back to the power of the input, both sides
            // together so that the image holds.
            if auto_gain {
                let p_in = 0.5 * (dry[0] * dry[0] + dry[1] * dry[1]);
                let p_out = 0.5 * (driven[0] * driven[0] + driven[1] * driven[1]);
                self.power[0] += follow * (p_in - self.power[0]);
                self.power[1] += follow * (p_out - self.power[1]);
                if self.power[0] > 1e-8 && self.power[1] > 1e-10 {
                    let wanted = (self.power[0] / self.power[1]).sqrt().clamp(0.06, 16.0);
                    self.auto += follow * (wanted - self.auto);
                }
            } else {
                self.auto = 1.0;
            }
            let makeup = if auto_gain { self.auto } else { out };
            *l = dry[0] * (1.0 - wet) + driven[0] * makeup * wet;
            *r = dry[1] * (1.0 - wet) + driven[1] * makeup * wet;
        }
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.clear();
        }
        self.dry = [Delay::default(); 2];
        self.power = [0.0; 2];
        self.filters = [[Biquad::default(); FILTERS]; 2];
        self.dc = [(0.0, 0.0); 2];
    }
}

impl Drive {
    fn update(&mut self) {
        let [mode, _, tone, _, _, low_cut, _, bass, mid, _] = self.values;
        let rate = f64::from(self.rate);
        let design = |kind, f: f32, q, gain: f32| {
            Coefficients::design(kind, f64::from(f), q, f64::from(gain), rate)
        };
        // Boosted before the clipping and cut back after it, so that band saturates first.
        let emphasis = match mode.round() as u32 {
            TAPE => Some((FilterKind::HighShelf, 3000.0, 8.0)),
            TRANSFORMER => Some((FilterKind::LowShelf, 120.0, 9.0)),
            _ => None,
        };
        self.coefficients = [
            if low_cut > 21.0 {
                design(FilterKind::HighPass, low_cut, 0.707, 0.0)
            } else {
                Coefficients::IDENTITY
            },
            emphasis.map_or(Coefficients::IDENTITY, |(kind, f, db)| {
                design(kind, f, 0.707, db)
            }),
            emphasis.map_or(Coefficients::IDENTITY, |(kind, f, db)| {
                design(kind, f, 0.707, -db)
            }),
            design(FilterKind::LowPass, tone.min(self.rate * 0.45), 0.707, 0.0),
            if bass.abs() > 0.01 {
                design(FilterKind::LowShelf, 100.0, 0.707, bass)
            } else {
                Coefficients::IDENTITY
            },
            if mid.abs() > 0.01 {
                design(FilterKind::Peak, 800.0, 0.8, mid)
            } else {
                Coefficients::IDENTITY
            },
        ];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturates_without_aliasing_much() {
        let mut drive = Drive::default();
        drive.set_param(1, 36.0);
        drive.set_param(2, 12000.0);
        drive.set_param(3, 0.0);
        drive.prepare(48_000, 4800);
        // A 5 kHz sine driven hard: harmonics at 15, 25 kHz... fold back without oversampling.
        let input: Vec<f32> = (0..48_000)
            .map(|i| 0.5 * (i as f32 * 5000.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let (mut l, mut r) = (input.clone(), input);
        drive.process(&mut l, &mut r);
        assert!(l.iter().all(|s| s.is_finite() && s.abs() < 2.0));
        let peak = l[4800..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3, "clipped output stays loud: {peak}");
        // Energy at 3 kHz (alias of 3 x 5 = 15 kHz -> no; of 5 x 5 = 25 kHz -> 23 kHz; 9 x 5 =
        // 45 kHz -> 3 kHz) compared to the fundamental.
        let tone = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in l[4800..].iter().enumerate() {
                let a = i as f32 * f * std::f32::consts::TAU / 48_000.0;
                re += s * a.cos();
                im += s * a.sin();
            }
            (re * re + im * im).sqrt()
        };
        let alias = tone(3000.0) / tone(5000.0);
        assert!(alias < 0.02, "aliasing at 3 kHz: {alias}");
        for mode in 0..TYPES.len() {
            drive.set_param(0, mode as f32);
            let (mut l, mut r) = (vec![0.3f32; 256], vec![0.3f32; 256]);
            drive.process(&mut l, &mut r);
            assert!(l.iter().all(|s| s.is_finite()));
        }
    }

    /// Level of `f` in `signal`, at 48 kHz.
    fn level(signal: &[f32], f: f32) -> f32 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in signal.iter().enumerate() {
            let a = i as f32 * f * std::f32::consts::TAU / 48_000.0;
            re += s * a.cos();
            im += s * a.sin();
        }
        (re * re + im * im).sqrt() * 2.0 / signal.len() as f32
    }

    fn render(settings: &[(usize, f32)], freq: f32) -> Vec<f32> {
        let mut drive = Drive::default();
        for &(index, value) in settings {
            drive.set_param(index, value);
        }
        drive.prepare(48_000, 4800);
        let input: Vec<f32> = (0..48_000)
            .map(|i| 0.3 * (i as f32 * freq * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let (mut l, mut r) = (input.clone(), input);
        drive.process(&mut l, &mut r);
        l.split_off(9600)
    }

    #[test]
    fn every_type_stays_bounded_and_heard() {
        for (mode, name) in TYPES.iter().enumerate() {
            for bias in [0.0, 100.0] {
                let out = render(&[(0, mode as f32), (1, 48.0), (3, 0.0), (6, bias)], 220.0);
                assert!(
                    out.iter().all(|s| s.is_finite() && s.abs() < 4.0),
                    "{name} at bias {bias}"
                );
                assert!(level(&out, 220.0) > 0.1, "{name} at bias {bias} is heard");
            }
        }
    }

    #[test]
    fn asymmetric_types_add_even_harmonics() {
        let second = |mode: f32, bias: f32| {
            let out = render(
                &[(0, mode), (1, 12.0), (2, 12000.0), (3, 0.0), (6, bias)],
                500.0,
            );
            level(&out, 1000.0) / level(&out, 500.0)
        };
        assert!(second(1.0, 0.0) < 0.01, "distortion is symmetric");
        assert!(second(4.0, 0.0) < 0.01, "tape is symmetric");
        for (mode, name) in [(3.0, "tube"), (5.0, "transformer"), (6.0, "rectifier")] {
            assert!(second(mode, 0.0) > 0.05, "{name}: {}", second(mode, 0.0));
        }
        assert!(second(1.0, 60.0) > 0.05, "bias makes distortion asymmetric");
    }

    #[test]
    fn low_cut_and_eq_shape_the_sound() {
        let lows = |settings: &[(usize, f32)]| level(&render(settings, 60.0), 60.0);
        let plain = lows(&[(1, 0.0), (3, 0.0)]);
        assert!(
            lows(&[(1, 0.0), (3, 0.0), (5, 600.0)]) < plain * 0.1,
            "low cut"
        );
        assert!(lows(&[(1, 0.0), (3, 0.0), (7, 12.0)]) > plain * 2.0, "bass");
        let mids = |mid: f32| level(&render(&[(1, 0.0), (3, 0.0), (8, mid)], 800.0), 800.0);
        assert!(mids(12.0) > mids(0.0) * 3.0, "middle");
    }

    #[test]
    fn the_dry_signal_lines_up_with_the_processed_one() {
        let mut fx = Drive::default();
        fx.set_param(4, 0.0);
        fx.prepare(48_000, 256);
        let impulse = |fx: &mut Drive| {
            let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
            l[0] = 0.1;
            r[0] = 0.1;
            fx.process(&mut l, &mut r);
            (0..64).max_by(|&a, &b| l[a].abs().total_cmp(&l[b].abs()))
        };
        let latency = fx.latency() as usize;
        assert_eq!(impulse(&mut fx), Some(latency), "dry");
        fx.set_param(4, 100.0);
        fx.reset();
        // Its tone filters move the peak a little.
        let wet = impulse(&mut fx).unwrap();
        assert!(
            wet.abs_diff(latency) <= 3,
            "wet at {wet}, latency {latency}"
        );
    }

    #[test]
    fn auto_gain_keeps_the_level_whatever_the_drive() {
        let level = |drive: f32| {
            let mut fx = Drive::default();
            fx.set_param(1, drive);
            fx.set_param(9, 1.0);
            fx.prepare(48_000, 4800);
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..96_000)
                .map(|i| {
                    let s = 0.2 * (i as f32 * 220.0 * std::f32::consts::TAU / 48_000.0).sin();
                    (s, s)
                })
                .unzip();
            fx.process(&mut l, &mut r);
            let tail = &l[72_000..];
            (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let input = 0.2 / std::f32::consts::SQRT_2;
        for drive in [6.0, 24.0, 42.0] {
            let rms = level(drive);
            assert!(
                (crate::gain_to_db(rms / input)).abs() < 1.0,
                "{drive} dB of drive: {rms}"
            );
        }
    }
}
