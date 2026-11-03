//! Flanger: a very short delay swept by a triangle LFO, with positive or negative feedback for
//! the jet sound, and the right channel's LFO offset for stereo motion.

use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "flanger",
    name: "Strada-Flanger",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Classic jet flanger with positive or negative feedback and stereo sweep.",
};

const PARAMS: [ParamInfo; 7] = [
    param("rate", "Rate", 0.02, 5.0, 0.25, "Hz"),
    param("depth", "Depth", 0.0, 100.0, 70.0, "%"),
    param("delay", "Manual", 0.1, 8.0, 1.0, "ms"),
    param("feedback", "Feedback", -95.0, 95.0, 60.0, "%"),
    param("stereo", "Stereo", 0.0, 180.0, 90.0, "°"),
    param("mix", "Mix", 0.0, 100.0, 50.0, "%"),
    param("output", "Output", -12.0, 6.0, 0.0, "dB"),
];

const MAX_MS: f32 = 20.0;

#[derive(Debug)]
pub struct Flanger {
    rate: f32,
    values: [f32; 7],
    lines: [Line; 2],
    phase: f32,
}

impl Default for Flanger {
    fn default() -> Self {
        let mut f = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            lines: Default::default(),
            phase: 0.0,
        };
        f.allocate();
        f
    }
}

/// Triangle wave from 0 to 1 for a phase in cycles.
#[inline]
pub(crate) fn triangle(phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    if p < 0.5 { p * 2.0 } else { 2.0 - p * 2.0 }
}

impl Flanger {
    fn allocate(&mut self) {
        let frames = (MAX_MS * 0.001 * self.rate) as usize + 4;
        self.lines = [Line::new(frames), Line::new(frames)];
    }
}

impl Effect for Flanger {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [rate, depth, manual, feedback, stereo, mix, output] = self.values;
        let ms = 0.001 * self.rate;
        let base = manual * ms;
        let sweep = (depth / 100.0) * (MAX_MS * 0.6 - manual).max(0.0) * ms * 0.5;
        let step = rate / self.rate;
        let offset = stereo / 360.0;
        let feedback = feedback / 100.0;
        let (wet, dry) = (mix / 100.0, 1.0 - mix / 100.0);
        let gain = crate::db_to_gain(output);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = [*l, *r];
            let mut out = [0.0; 2];
            for ch in 0..2 {
                let phase = self.phase + if ch == 1 { offset } else { 0.0 };
                let echo = self.lines[ch].read(base + sweep * triangle(phase));
                self.lines[ch].push((input[ch] + echo * feedback).clamp(-4.0, 4.0));
                // Normalized so heavy feedback does not blow the level up.
                let comb = (input[ch] + echo) / (1.0 + feedback.abs() * 0.5);
                out[ch] = (input[ch] * dry + comb * wet) * gain;
            }
            *l = out[0];
            *r = out[1];
            self.phase = (self.phase + step).fract();
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_notches_and_stays_bounded() {
        for feedback in [-95.0, 0.0, 95.0] {
            let mut f = Flanger::default();
            f.set_param(3, feedback);
            f.set_param(0, 2.0);
            f.prepare(48_000, 512);
            let mut seed = 1u32;
            let mut noise = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            };
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
                .map(|_| {
                    let n = noise();
                    (n, n)
                })
                .unzip();
            f.process(&mut l, &mut r);
            assert!(
                l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 3.0),
                "{feedback}"
            );
            let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
            assert!(diff > 10.0, "stereo sweep");
        }
        assert_eq!(triangle(0.25), 0.5);
        assert_eq!(triangle(0.5), 1.0);
    }
}
