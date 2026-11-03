//! Drive: overdrive, distortion, fuzz, tube, tape, transformer or rectifier saturation, with a
//! low cut before it and tone, bass and middle after it, run four times oversampled so the
//! clipping does not alias.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param};

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

const PARAMS: [ParamInfo; 9] = [
    choice("mode", "Type", &TYPES, 0.0),
    param("drive", "Drive", 0.0, 48.0, 18.0, "dB"),
    param("tone", "Tone", 500.0, 12000.0, 3500.0, "Hz"),
    param("level", "Level", -36.0, 12.0, -12.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("low_cut", "Low cut", 20.0, 600.0, 20.0, "Hz"),
    param("bias", "Bias", 0.0, 100.0, 0.0, "%"),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("mid", "Middle", -12.0, 12.0, 0.0, "dB"),
];

/// Filters of each channel, in signal order.
const LOW_CUT: usize = 0;
const EMPHASIS: usize = 1;
const DE_EMPHASIS: usize = 2;
const TONE: usize = 3;
const BASS: usize = 4;
const MID: usize = 5;
const FILTERS: usize = 6;

pub(crate) const FACTOR: usize = 4;
pub(crate) const TAPS: usize = 32;

/// Low-pass FIR at a quarter of the band, for up- and downsampling by four.
pub(crate) fn kernel() -> [f32; TAPS] {
    let mut k = [0.0f32; TAPS];
    let centre = (TAPS - 1) as f32 / 2.0;
    let cutoff = 0.9 / FACTOR as f32;
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        let x = i as f32 - centre;
        let sinc = if x.abs() < 1e-6 {
            1.0
        } else {
            (std::f32::consts::PI * cutoff * x).sin() / (std::f32::consts::PI * cutoff * x)
        };
        let window = 0.42 - 0.5 * (std::f32::consts::TAU * i as f32 / (TAPS - 1) as f32).cos()
            + 0.08 * (2.0 * std::f32::consts::TAU * i as f32 / (TAPS - 1) as f32).cos();
        *v = sinc * window;
        sum += *v;
    }
    for v in &mut k {
        *v /= sum;
    }
    k
}

#[derive(Debug, Clone)]
pub(crate) struct Oversampler {
    up: [f32; TAPS],
    down: [f32; TAPS],
    at_up: usize,
    at_down: usize,
}

impl Oversampler {
    pub(crate) fn new() -> Self {
        Self {
            up: [0.0; TAPS],
            down: [0.0; TAPS],
            at_up: 0,
            at_down: 0,
        }
    }

    fn filter(line: &mut [f32; TAPS], at: &mut usize, k: &[f32; TAPS], x: f32) -> f32 {
        line[*at] = x;
        *at = (*at + 1) % TAPS;
        let mut y = 0.0;
        for (j, c) in k.iter().enumerate() {
            y += c * line[(*at + j) % TAPS];
        }
        y
    }

    /// Runs `shape` at four times the rate on one sample.
    pub(crate) fn run(
        &mut self,
        k: &[f32; TAPS],
        x: f32,
        mut shape: impl FnMut(f32) -> f32,
    ) -> f32 {
        let mut out = 0.0;
        for phase in 0..FACTOR {
            // Zero stuffing, gain restored by the factor.
            let v = if phase == 0 { x * FACTOR as f32 } else { 0.0 };
            let up = Self::filter(&mut self.up, &mut self.at_up, k, v);
            let shaped = shape(up);
            let down = Self::filter(&mut self.down, &mut self.at_down, k, shaped);
            if phase == FACTOR - 1 {
                out = down;
            }
        }
        out
    }
}

#[derive(Debug)]
pub struct Drive {
    rate: f32,
    values: [f32; 9],
    kernel: [f32; TAPS],
    stages: [Oversampler; 2],
    coefficients: [Coefficients; FILTERS],
    filters: [[Biquad; FILTERS]; 2],
    dc: [(f32, f32); 2],
}

impl Default for Drive {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            kernel: kernel(),
            stages: [Oversampler::new(), Oversampler::new()],
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
        let [mode, drive, _, level, mix, _, bias, ..] = self.values;
        let mode = mode.round() as u32;
        let (gain, out, wet) = (db_to_gain(drive), db_to_gain(level), mix / 100.0);
        let kernel = self.kernel;
        let c = &self.coefficients;
        for (ch, buffer) in [left, right].into_iter().enumerate() {
            let f = &mut self.filters[ch];
            for s in buffer.iter_mut() {
                let dry = *s;
                let mut x = f64::from(dry);
                for i in [LOW_CUT, EMPHASIS] {
                    x = f[i].process(&c[i], x);
                }
                let clipped =
                    self.stages[ch].run(&kernel, x as f32 * gain, |x| transfer(mode, bias, x));
                let clipped = f[DE_EMPHASIS].process(&c[DE_EMPHASIS], f64::from(clipped)) as f32;
                // DC blocker for the asymmetric shapes, then tone, bass and middle.
                let (x1, y1) = self.dc[ch];
                let y = clipped - x1 + 0.995 * y1;
                self.dc[ch] = (clipped, y);
                let mut toned = f64::from(y);
                for i in [TONE, BASS, MID] {
                    toned = f[i].process(&c[i], toned);
                }
                *s = dry * (1.0 - wet) + toned as f32 * out * wet;
            }
        }
    }

    fn reset(&mut self) {
        self.stages = [Oversampler::new(), Oversampler::new()];
        self.filters = [[Biquad::default(); FILTERS]; 2];
        self.dc = [(0.0, 0.0); 2];
    }
}

impl Drive {
    fn update(&mut self) {
        let [mode, _, tone, _, _, low_cut, _, bass, mid] = self.values;
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
}
