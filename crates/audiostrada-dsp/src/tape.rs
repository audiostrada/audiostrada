//! Tape machine: saturation with a touch of memory (soft hysteresis), the head bump in the
//! lows, the high-frequency loss of the chosen speed, wow and flutter, and hiss.
//!
//! Subtle by default: the tape has headroom above full scale, the drive changes the color and
//! not the level, and the highs are emphasized before the tape and restored after it, as on a
//! real machine, so they saturate first while the lows stay clear.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::drive::{Oversampler, TAPS, kernel};
use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "tape",
    name: "Strada-Tape",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Color",
    description: "Tape machine: warm saturation, head bump, wow and flutter, hiss.",
};

const SPEEDS: [&str; 3] = ["7.5 ips", "15 ips", "30 ips"];

const PARAMS: [ParamInfo; 8] = [
    param("drive", "Drive", -12.0, 24.0, 0.0, "dB"),
    choice("speed", "Speed", &SPEEDS, 1.0),
    param("bump", "Head bump", 0.0, 6.0, 1.5, "dB"),
    param("wow", "Wow & flutter", 0.0, 100.0, 0.0, "%"),
    param("hiss", "Hiss", 0.0, 100.0, 0.0, "%"),
    param("bias", "Bias", -100.0, 100.0, 0.0, "%"),
    param("output", "Output", -18.0, 12.0, 0.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
];

/// Head bump frequency and high-frequency roll-off for each speed.
const SPEED_SHAPE: [(f32, f32); 3] = [(55.0, 11_000.0), (85.0, 17_000.0), (110.0, 21_000.0)];
const MAX_WOW_MS: f32 = 6.0;
/// Level where the tape starts to bend, above full scale.
const HEADROOM: f32 = 1.6;
/// Highs lifted before the tape and lowered after it.
const EMPHASIS_HZ: f64 = 3_000.0;
const EMPHASIS_DB: f64 = 6.0;

#[derive(Debug)]
pub struct Tape {
    rate: f32,
    values: [f32; 8],
    kernel: [f32; TAPS],
    stages: [Oversampler; 2],
    /// Last output of the saturator per channel, its memory.
    memory: [f32; 2],
    bump: Coefficients,
    rolloff: Coefficients,
    emphasis: Coefficients,
    deemphasis: Coefficients,
    filters: [[Biquad; 4]; 2],
    wow: [Line; 2],
    phase: [f32; 2],
    noise: u32,
    hiss_lp: [f32; 2],
}

impl Default for Tape {
    fn default() -> Self {
        let mut tape = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            kernel: kernel(),
            stages: [Oversampler::new(), Oversampler::new()],
            memory: [0.0; 2],
            bump: Coefficients::IDENTITY,
            rolloff: Coefficients::IDENTITY,
            emphasis: Coefficients::IDENTITY,
            deemphasis: Coefficients::IDENTITY,
            filters: [[Biquad::default(); 4]; 2],
            wow: Default::default(),
            phase: [0.0, 1.7],
            noise: 0x1234_5678,
            hiss_lp: [0.0; 2],
        };
        tape.allocate();
        tape
    }
}

/// Saturation with memory: the previous output pulls the curve a little, a soft hysteresis.
/// Linear at low levels, it bends gently towards the headroom.
#[inline]
fn saturate(x: f32, memory: &mut f32, bias: f32) -> f32 {
    let v = (x + 0.03 * *memory) / HEADROOM + bias;
    let y = (v.tanh() - bias.tanh()) * HEADROOM;
    *memory = y;
    y
}

impl Tape {
    fn allocate(&mut self) {
        let frames = (MAX_WOW_MS * 2.0 * 0.001 * self.rate) as usize + 8;
        self.wow = [Line::new(frames), Line::new(frames)];
        self.update();
    }

    fn update(&mut self) {
        let speed = self.values[1].round().clamp(0.0, 2.0) as usize;
        let (bump_hz, rolloff_hz) = SPEED_SHAPE[speed];
        let rate = f64::from(self.rate);
        self.bump = Coefficients::design(
            FilterKind::Peak,
            f64::from(bump_hz),
            0.9,
            f64::from(self.values[2]),
            rate,
        );
        self.rolloff = Coefficients::design(
            FilterKind::LowPass,
            f64::from(rolloff_hz.min(self.rate * 0.45)),
            0.6,
            0.0,
            rate,
        );
        self.emphasis =
            Coefficients::design(FilterKind::HighShelf, EMPHASIS_HZ, 0.7, EMPHASIS_DB, rate);
        self.deemphasis =
            Coefficients::design(FilterKind::HighShelf, EMPHASIS_HZ, 0.7, -EMPHASIS_DB, rate);
    }

    #[inline]
    fn white(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

impl Effect for Tape {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [drive, _, _, wow, hiss, bias, output, mix] = self.values;
        let gain = db_to_gain(drive);
        // Level-compensated: the drive changes the color, not the level.
        let makeup = db_to_gain(output) / gain;
        let bias = bias / 100.0 * 0.3;
        // Both grow slowly at first, so small settings stay subtle and the top end goes far.
        let hiss = (hiss / 100.0).powi(2) * db_to_gain(-36.0);
        let depth = (wow / 100.0).powi(2) * MAX_WOW_MS * 0.001 * self.rate;
        let base = depth + 2.0;
        let (wow_step, flutter_step) = (
            std::f32::consts::TAU * 0.6 / self.rate,
            std::f32::consts::TAU * 6.3 / self.rate,
        );
        let wet = mix / 100.0;
        let kernel = self.kernel;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let dry = [*l, *r];
            self.phase[0] = (self.phase[0] + wow_step) % std::f32::consts::TAU;
            self.phase[1] = (self.phase[1] + flutter_step) % std::f32::consts::TAU;
            let modulation = 0.8 * self.phase[0].sin() + 0.2 * self.phase[1].sin();
            let delay = base + depth * modulation;
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let memory = std::cell::Cell::new(self.memory[ch]);
                let x =
                    self.filters[ch][2].process(&self.emphasis, f64::from(dry[ch])) as f32 * gain;
                let y = self.stages[ch].run(&kernel, x, |v| {
                    let mut m = memory.get();
                    let y = saturate(v, &mut m, bias);
                    memory.set(m);
                    y
                });
                self.memory[ch] = memory.get();
                let y = self.filters[ch][3].process(&self.deemphasis, f64::from(y)) as f32;
                let y = self.filters[ch][0].process(&self.bump, f64::from(y)) as f32;
                let y = self.filters[ch][1].process(&self.rolloff, f64::from(y)) as f32;
                // Wow and flutter: the signal goes through a gently modulated delay.
                self.wow[ch].push(y);
                out[ch] = self.wow[ch].read(delay + 1.0);
            }
            if hiss > 0.0 {
                for (ch, o) in out.iter_mut().enumerate() {
                    let n = self.white();
                    self.hiss_lp[ch] += 0.35 * (n - self.hiss_lp[ch]);
                    *o += (n - self.hiss_lp[ch] * 0.5) * hiss;
                }
            }
            *l = dry[0] * (1.0 - wet) + out[0] * makeup * wet;
            *r = dry[1] * (1.0 - wet) + out[1] * makeup * wet;
        }
    }

    fn reset(&mut self) {
        self.stages = [Oversampler::new(), Oversampler::new()];
        self.memory = [0.0; 2];
        self.filters = [[Biquad::default(); 4]; 2];
        for line in &mut self.wow {
            line.clear();
        }
        self.hiss_lp = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(tape: &mut Tape, input: &[f32]) -> Vec<f32> {
        let (mut l, mut r) = (input.to_vec(), input.to_vec());
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            tape.process(cl, cr);
        }
        l
    }

    fn sine(f: f32, a: f32) -> Vec<f32> {
        (0..48_000)
            .map(|i| a * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn peak(x: &[f32]) -> f32 {
        x[24_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Energy left once the input sine is removed, relative to it (distortion and noise).
    fn residue(out: &[f32], f: f32) -> f32 {
        let part = &out[24_000..];
        let (mut c, mut s) = (0.0f32, 0.0f32);
        for (i, v) in part.iter().enumerate() {
            let t = (24_000 + i) as f32 * f * std::f32::consts::TAU / 48_000.0;
            c += v * t.cos();
            s += v * t.sin();
        }
        let n = part.len() as f32;
        let (c, s) = (2.0 * c / n, 2.0 * s / n);
        let fitted: f32 = part
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let t = (24_000 + i) as f32 * f * std::f32::consts::TAU / 48_000.0;
                (v - c * t.cos() - s * t.sin()).powi(2)
            })
            .sum();
        (fitted / (c * c + s * s) / n * 2.0).sqrt()
    }

    #[test]
    fn subtle_by_default_and_strong_when_pushed() {
        let input = sine(1000.0, 0.5);
        let gentle = run(&mut Tape::default(), &input);
        let level = peak(&gentle) / 0.5;
        assert!((0.9..1.1).contains(&level), "same level: {level}");
        assert!(residue(&gentle, 1000.0) < 0.01, "clean by default");
        let mut pushed = Tape::default();
        pushed.set_param(0, 18.0);
        pushed.prepare(48_000, 256);
        let hot = run(&mut pushed, &input);
        assert!(residue(&hot, 1000.0) > 0.05, "saturated when driven");
        assert!(peak(&hot) < 1.2, "the drive keeps the level");
    }

    #[test]
    fn bumps_the_lows_and_stays_finite() {
        let mut tape = Tape::default();
        tape.set_param(2, 6.0);
        tape.prepare(48_000, 256);
        let low = run(&mut tape, &sine(85.0, 0.05));
        let mid = run(&mut Tape::default(), &sine(1000.0, 0.05));
        assert!(
            peak(&low) > peak(&mid),
            "head bump: {} vs {}",
            peak(&low),
            peak(&mid)
        );
        let mut hot = Tape::default();
        hot.set_param(0, 24.0);
        hot.set_param(4, 100.0);
        hot.set_param(3, 100.0);
        hot.prepare(48_000, 256);
        let out = run(&mut hot, &sine(200.0, 0.9));
        assert!(out.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }
}
