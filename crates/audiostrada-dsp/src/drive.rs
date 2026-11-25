//! Drive: overdrive, distortion, fuzz, tube, tape, transformer or rectifier saturation, run
//! four times oversampled so the clipping does not alias. A low cut and a focus band before it
//! choose what saturates most; tone, bass and middle after it shape the result. The drive is
//! compensated so that peaks keep their level, from barely there to exaggerated; auto gain
//! matches the loudness instead. Drive, level and mix glide.

use crate::analyzer::{Analyzer, BANDS};
use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param, switch};

/// The spectra of the input and of the output, drawn while the window is open.
pub const INPUT_SPECTRUM: usize = 0;
pub const OUTPUT_SPECTRUM: usize = BANDS;
const METERS: usize = 2 * BANDS;

pub const INFO: EffectInfo = EffectInfo {
    id: "drive",
    name: "Strada-Drive",
    params: &PARAMS,
    meters: &[""; METERS],
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

const MODE: usize = 0;
const DRIVE: usize = 1;
const TONE_HZ: usize = 2;
const LEVEL: usize = 3;
const MIX: usize = 4;
const LOW_CUT_HZ: usize = 5;
const BIAS: usize = 6;
const BASS_DB: usize = 7;
const MID_DB: usize = 8;
const AUTO_GAIN: usize = 9;
const FOCUS_DB: usize = 10;
const FOCUS_HZ: usize = 11;
const MID_HZ: usize = 12;
const COUNT: usize = 13;

const PARAMS: [ParamInfo; COUNT] = [
    choice("mode", "Type", &TYPES, 0.0),
    param("drive", "Drive", 0.0, 100.0, 35.0, "%"),
    param("tone", "Tone", 500.0, 20000.0, 20000.0, "Hz"),
    param("level", "Level", -24.0, 24.0, 0.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("low_cut", "Low cut", 20.0, 600.0, 20.0, "Hz"),
    param("bias", "Bias", 0.0, 100.0, 0.0, "%"),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("mid", "Middle", -12.0, 12.0, 0.0, "dB"),
    switch("auto_gain", "Auto gain", 0.0),
    param("focus", "Focus", -12.0, 12.0, 0.0, "dB"),
    param("focus_freq", "Focus frequency", 40.0, 10000.0, 1000.0, "Hz"),
    param("mid_freq", "Middle frequency", 200.0, 5000.0, 800.0, "Hz"),
];
const GLIDE_MS: f32 = 15.0;
/// How fast auto gain follows the level, in ms: slow enough not to pump.
const AUTO_MS: f32 = 400.0;
/// The peak whose level the compensation keeps, whatever the drive (-6 dBFS).
const REFERENCE: f32 = 0.5;

/// Gain into the clipping for a drive (0 to 100 %): -24 dB, clean, to +48 dB, even steps of
/// dB, so that the first third stays subtle.
pub fn drive_db(percent: f32) -> f32 {
    -24.0 + 0.72 * percent.clamp(0.0, 100.0)
}

/// The gain into the clipping of a type at a drive, dB: the fuzz, whose curve already
/// amplifies by nearly four, saturates quiet signals already, as the classic pedals do.
pub fn clip_db(mode: f32, percent: f32) -> f32 {
    let trim = if mode.round() as u32 == 2 { -12.0 } else { 0.0 };
    drive_db(percent) - trim
}

/// The bias at a gain into the clipping: a quiet signal driven little gets less of it, or the
/// shift would leave it in a dead zone (a fuzz fully biased fell silent).
fn bias_at(bias: f32, gain: f32) -> f32 {
    bias * (2.0 * gain * REFERENCE).min(1.0)
}

/// Filters of each channel, in signal order.
const LOW_CUT: usize = 0;
const FOCUS_IN: usize = 1;
const EMPHASIS: usize = 2;
const DE_EMPHASIS: usize = 3;
const FOCUS_OUT: usize = 4;
const TONE: usize = 5;
const BASS: usize = 6;
const MID: usize = 7;
const FILTERS: usize = 8;

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
    values: [f32; COUNT],
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
    /// Spectra of the input and of the output, computed while the window is open.
    spectra: Box<[Analyzer; 2]>,
    watched: bool,
}

impl Default for Drive {
    fn default() -> Self {
        let values = PARAMS.map(|p| p.default);
        Self {
            rate: 48_000.0,
            values,
            smooth: [db_to_gain(clip_db(values[MODE], values[DRIVE])), 1.0, 1.0],
            auto: 1.0,
            power: [0.0; 2],
            stages: [Oversampler::new(Taps::Long), Oversampler::new(Taps::Long)],
            dry: [Delay::default(); 2],
            coefficients: [Coefficients::IDENTITY; FILTERS],
            filters: [[Biquad::default(); FILTERS]; 2],
            dc: [(0.0, 0.0); 2],
            spectra: Box::default(),
            watched: true,
        }
    }
}

fn shape(mode: u32, x: f32) -> f32 {
    match mode {
        // Soft and nearly symmetric, like the diodes of an overdrive: odd harmonics first.
        0 => {
            let y = (x + 0.02).tanh() - 0.02f32.tanh();
            y / (1.0 + 0.02f32.tanh())
        }
        1 => x.clamp(-1.0, 1.0) * 0.9 + (x * 2.0).tanh() * 0.1,
        2 => {
            let y = (2.5 * x).tanh();
            (y * 1.5).clamp(-0.8, 1.0)
        }
        // Grid current flattens the positive swing at a firm knee, long before cutoff touches
        // the negative one: mostly even harmonics.
        3 => {
            if x >= 0.0 {
                x / (1.0 + x.powi(4)).sqrt().sqrt()
            } else {
                x / (1.0 + 0.02 * x * x).sqrt()
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

/// The gain that brings a peak of [`REFERENCE`] driven by `gain` back to its level.
fn compensation(mode: u32, bias: f32, gain: f32) -> f32 {
    let x = gain * REFERENCE;
    let swing = 0.5 * (transfer(mode, bias, x).abs() + transfer(mode, bias, -x).abs());
    REFERENCE / swing.max(1e-6)
}

/// How much each frequency is driven and how the tone shapes it, for the editor.
#[derive(Debug)]
pub struct Response {
    rate: f64,
    drive_db: f32,
    coefficients: [Coefficients; FILTERS],
}

impl Response {
    /// From the parameter values, in parameter order.
    pub fn new(values: &[f32]) -> Self {
        let mut v = PARAMS.map(|p| p.default);
        for (a, b) in v.iter_mut().zip(values) {
            *a = *b;
        }
        Self {
            rate: 48_000.0,
            drive_db: clip_db(v[MODE], v[DRIVE]),
            coefficients: filters(&v, 48_000.0),
        }
    }

    fn sum(&self, which: &[usize], hz: f32) -> f32 {
        which
            .iter()
            .map(|&i| self.coefficients[i].magnitude_db(f64::from(hz), self.rate) as f32)
            .sum()
    }

    /// The gain into the clipping at `hz`, in dB.
    pub fn drive_at(&self, hz: f32) -> f32 {
        self.drive_db + self.sum(&[LOW_CUT, FOCUS_IN, EMPHASIS], hz)
    }

    /// The tone after the clipping at `hz`, in dB.
    pub fn tone_at(&self, hz: f32) -> f32 {
        self.sum(&[TONE, BASS, MID], hz)
    }
}

/// The filters for the values `v` at `rate`.
fn filters(v: &[f32; COUNT], rate: f32) -> [Coefficients; FILTERS] {
    let r = f64::from(rate);
    let design = |kind, f: f32, q, gain: f32| {
        Coefficients::design(kind, f64::from(f), q, f64::from(gain), r)
    };
    // Boosted before the clipping and cut back after it, so that band saturates first.
    let emphasis = match v[MODE].round() as u32 {
        TAPE => Some((FilterKind::HighShelf, 3000.0, 8.0)),
        TRANSFORMER => Some((FilterKind::LowShelf, 120.0, 9.0)),
        _ => None,
    };
    let focus = v[FOCUS_DB];
    let peak = |gain: f32| {
        if focus.abs() > 0.01 {
            design(FilterKind::Peak, v[FOCUS_HZ], 0.9, gain)
        } else {
            Coefficients::IDENTITY
        }
    };
    [
        if v[LOW_CUT_HZ] > 21.0 {
            design(FilterKind::HighPass, v[LOW_CUT_HZ], 0.707, 0.0)
        } else {
            Coefficients::IDENTITY
        },
        peak(focus),
        emphasis.map_or(Coefficients::IDENTITY, |(kind, f, db)| {
            design(kind, f, 0.707, db)
        }),
        emphasis.map_or(Coefficients::IDENTITY, |(kind, f, db)| {
            design(kind, f, 0.707, -db)
        }),
        peak(-focus),
        if v[TONE_HZ] < 19_500.0 {
            design(FilterKind::LowPass, v[TONE_HZ].min(rate * 0.45), 0.707, 0.0)
        } else {
            Coefficients::IDENTITY
        },
        if v[BASS_DB].abs() > 0.01 {
            design(FilterKind::LowShelf, 100.0, 0.707, v[BASS_DB])
        } else {
            Coefficients::IDENTITY
        },
        if v[MID_DB].abs() > 0.01 {
            design(FilterKind::Peak, v[MID_HZ], 0.8, v[MID_DB])
        } else {
            Coefficients::IDENTITY
        },
    ]
}

impl Effect for Drive {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        let v = self.values;
        self.smooth = [
            db_to_gain(clip_db(v[MODE], v[DRIVE])),
            db_to_gain(v[LEVEL]),
            v[MIX] / 100.0,
        ];
        self.coefficients = filters(&self.values, self.rate);
        for spectrum in self.spectra.iter_mut() {
            spectrum.prepare(sample_rate, max_frames);
        }
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.coefficients = filters(&self.values, self.rate);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let v = self.values;
        let mode = v[MODE].round() as u32;
        let bias = v[BIAS];
        let targets = [
            db_to_gain(clip_db(v[MODE], v[DRIVE])),
            db_to_gain(v[LEVEL]),
            v[MIX] / 100.0,
        ];
        let glide = crate::time_coefficient(GLIDE_MS, self.rate);
        let follow = 1.0 - crate::time_coefficient(AUTO_MS, self.rate);
        let auto_gain = v[AUTO_GAIN] >= 0.5;
        let c = &self.coefficients;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.watched {
                self.spectra[0].push(*l, *r);
            }
            for (s, t) in self.smooth.iter_mut().zip(targets) {
                *s = t + glide * (*s - t);
            }
            let [gain, out, wet] = self.smooth;
            let bias = bias_at(bias, gain);
            let keep = compensation(mode, bias, gain);
            let mut driven = [0.0f32; 2];
            let mut dry = [*l, *r];
            for ch in 0..2 {
                let f = &mut self.filters[ch];
                let mut x = f64::from(dry[ch]);
                for i in [LOW_CUT, FOCUS_IN, EMPHASIS] {
                    x = f[i].process(&c[i], x);
                }
                let clipped = self.stages[ch].run(x as f32 * gain, |x| transfer(mode, bias, x));
                let mut y = f64::from(clipped * keep);
                for i in [DE_EMPHASIS, FOCUS_OUT] {
                    y = f[i].process(&c[i], y);
                }
                // DC blocker for the asymmetric shapes, then tone, bass and middle.
                let clipped = y as f32;
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
            if self.watched {
                self.spectra[1].push(*l, *r);
            }
        }
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn set_watched(&mut self, watched: bool) {
        self.watched = watched;
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.clear();
        }
        self.dry = [Delay::default(); 2];
        self.power = [0.0; 2];
        self.filters = [[Biquad::default(); FILTERS]; 2];
        self.dc = [(0.0, 0.0); 2];
        for spectrum in self.spectra.iter_mut() {
            spectrum.reset();
        }
    }

    fn meters(&self, out: &mut [f32]) {
        for (k, spectrum) in self.spectra.iter().enumerate() {
            if let Some(slot) = out.get_mut(k * BANDS..) {
                spectrum.meters(slot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A second of a sine of `amplitude` through the drive, the first fifth left out.
    fn render_at(settings: &[(usize, f32)], freq: f32, amplitude: f32) -> Vec<f32> {
        let mut drive = Drive::default();
        for &(index, value) in settings {
            drive.set_param(index, value);
        }
        drive.prepare(48_000, 4800);
        let input: Vec<f32> = (0..48_000)
            .map(|i| amplitude * (i as f32 * freq * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        let (mut l, mut r) = (input.clone(), input);
        drive.process(&mut l, &mut r);
        l.split_off(9600)
    }

    fn render(settings: &[(usize, f32)], freq: f32) -> Vec<f32> {
        render_at(settings, freq, 0.3)
    }

    /// The harmonics up to the seventh over the fundamental.
    fn distortion(out: &[f32], freq: f32) -> f32 {
        let harmonics: f32 = (2..=7).map(|k| level(out, freq * k as f32).powi(2)).sum();
        harmonics.sqrt() / level(out, freq)
    }

    #[test]
    fn saturates_without_aliasing_much() {
        let out = render_at(&[(DRIVE, 85.0), (TONE_HZ, 20000.0)], 5000.0, 0.5);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 2.0));
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3, "clipped output stays loud: {peak}");
        // 9 x 5 kHz = 45 kHz folds back to 3 kHz without oversampling.
        let alias = level(&out, 3000.0) / level(&out, 5000.0);
        assert!(alias < 0.02, "aliasing at 3 kHz: {alias}");
    }

    #[test]
    fn every_type_stays_bounded_and_heard() {
        for (mode, name) in TYPES.iter().enumerate() {
            for bias in [0.0, 100.0] {
                for drive in [0.0, 100.0] {
                    let out = render(&[(MODE, mode as f32), (DRIVE, drive), (BIAS, bias)], 220.0);
                    assert!(
                        out.iter().all(|s| s.is_finite() && s.abs() < 4.0),
                        "{name} at bias {bias}, drive {drive}"
                    );
                    assert!(level(&out, 220.0) > 0.1, "{name} at bias {bias} is heard");
                }
            }
        }
    }

    #[test]
    fn the_drive_grows_by_even_steps_and_the_level_holds() {
        for (mode, name) in TYPES.iter().enumerate().take(6) {
            let mut last = 0.0;
            for drive in [0.0, 20.0, 40.0, 60.0, 80.0, 100.0] {
                let out = render(&[(MODE, mode as f32), (DRIVE, drive)], 220.0);
                let d = distortion(&out, 220.0);
                if drive == 0.0 {
                    assert!(d < 0.01, "{name} is clean at 0 %: {d}");
                }
                assert!(d >= last * 0.9, "{name} at {drive} %: {d} after {last}");
                last = d;
                if drive <= 40.0 {
                    let db = crate::gain_to_db(level(&out, 220.0) / 0.3);
                    assert!(db.abs() < 3.0, "{name} at {drive} %: {db} dB");
                }
            }
            assert!(last > 0.2, "{name} can be pushed hard: {last}");
        }
    }

    #[test]
    fn asymmetric_types_add_even_harmonics() {
        let second = |mode: f32, bias: f32| {
            let out = render(&[(MODE, mode), (DRIVE, 50.0), (BIAS, bias)], 500.0);
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
    fn the_focus_drives_its_band_harder() {
        let at = |focus: f32, freq: f32| {
            distortion(
                &render(&[(DRIVE, 40.0), (FOCUS_DB, focus), (FOCUS_HZ, 100.0)], freq),
                freq,
            )
        };
        assert!(
            at(12.0, 100.0) > at(0.0, 100.0) * 2.0,
            "the bass saturates more"
        );
        assert!(at(-12.0, 100.0) < at(0.0, 100.0) * 0.5, "or less");
        let far = at(12.0, 3000.0) / at(0.0, 3000.0);
        assert!((0.7..1.4).contains(&far), "the highs are left alone: {far}");
        let level_db = |focus: f32| {
            let out = render(&[(DRIVE, 0.0), (FOCUS_DB, focus), (FOCUS_HZ, 100.0)], 100.0);
            crate::gain_to_db(level(&out, 100.0) / 0.3)
        };
        assert!(
            level_db(12.0).abs() < 1.0,
            "clean, the focus changes no level"
        );
    }

    #[test]
    fn low_cut_and_eq_shape_the_sound() {
        let clean = [(DRIVE, 0.0)];
        let lows = |more: &[(usize, f32)]| {
            let settings: Vec<_> = clean.iter().chain(more).copied().collect();
            level(&render(&settings, 60.0), 60.0)
        };
        let plain = lows(&[]);
        assert!(lows(&[(LOW_CUT_HZ, 600.0)]) < plain * 0.1, "low cut");
        assert!(lows(&[(BASS_DB, 12.0)]) > plain * 2.0, "bass");
        let mids = |mid: f32, at: f32| {
            level(
                &render(&[(DRIVE, 0.0), (MID_DB, mid), (MID_HZ, at)], at),
                at,
            )
        };
        assert!(mids(12.0, 800.0) > mids(0.0, 800.0) * 3.0, "middle");
        assert!(mids(12.0, 3000.0) > mids(0.0, 3000.0) * 3.0, "middle moved");
        let response = Response::new(&[0.0, 0.0, 12000.0, 0.0, 100.0, 20.0, 0.0, 0.0, 6.0]);
        assert!((response.tone_at(800.0) - 6.0).abs() < 0.5);
        assert!((response.drive_at(1000.0) + 24.0).abs() < 0.5);
    }

    #[test]
    fn the_dry_signal_lines_up_with_the_processed_one() {
        let mut fx = Drive::default();
        fx.set_param(MIX, 0.0);
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
        fx.set_param(MIX, 100.0);
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
            fx.set_param(DRIVE, drive);
            fx.set_param(AUTO_GAIN, 1.0);
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
        for drive in [40.0, 70.0, 95.0] {
            let rms = level(drive);
            assert!(
                (crate::gain_to_db(rms / input)).abs() < 1.0,
                "{drive} % of drive: {rms}"
            );
        }
    }

    #[test]
    fn spectra_show_the_input_and_the_harmonics() {
        let mut fx = Drive::default();
        fx.set_param(DRIVE, 80.0);
        fx.prepare(48_000, 4800);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
            .map(|i| {
                let s = 0.3 * (i as f32 * 500.0 * std::f32::consts::TAU / 48_000.0).sin();
                (s, s)
            })
            .unzip();
        fx.process(&mut l, &mut r);
        let mut meters = vec![0.0; METERS];
        fx.meters(&mut meters);
        let band = |hz: f32| {
            (0..BANDS)
                .min_by(|&a, &b| {
                    let far = |k| (crate::analyzer::band_frequency(k) / hz).ln().abs();
                    far(a).total_cmp(&far(b))
                })
                .unwrap()
        };
        let third = band(1500.0);
        assert!(
            meters[OUTPUT_SPECTRUM + third] > meters[INPUT_SPECTRUM + third] + 20.0,
            "the third harmonic: {} in, {} out",
            meters[INPUT_SPECTRUM + third],
            meters[OUTPUT_SPECTRUM + third]
        );
    }
}
