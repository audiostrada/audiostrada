//! Chorus: short delays swept by sine LFOs in quadrature between the channels, for width and
//! shimmer; with feedback it turns into a flanger.

use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "chorus",
    name: "Chorus",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Short modulated delays for width and shimmer; turns into a flanger with feedback.",
};

const PARAMS: [ParamInfo; 5] = [
    param("rate", "Rate", 0.05, 8.0, 0.8, "Hz"),
    param("depth", "Depth", 0.0, 100.0, 40.0, "%"),
    param("delay", "Delay", 1.0, 30.0, 12.0, "ms"),
    param("feedback", "Feedback", 0.0, 90.0, 0.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 50.0, "%"),
];

const MAX_MS: f32 = 45.0;

#[derive(Debug)]
pub struct Chorus {
    rate: f32,
    values: [f32; 5],
    lines: [Line; 2],
    phase: f32,
}

impl Default for Chorus {
    fn default() -> Self {
        let mut chorus = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            lines: Default::default(),
            phase: 0.0,
        };
        chorus.prepare(48_000, 0);
        chorus
    }
}

impl Effect for Chorus {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        let frames = (MAX_MS * 0.001 * self.rate) as usize + 4;
        self.lines = [Line::new(frames), Line::new(frames)];
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [rate, depth, delay, feedback, mix] = self.values;
        let (feedback, wet) = (feedback / 100.0, mix / 100.0);
        let base = delay * 0.001 * self.rate;
        let sweep = (depth / 100.0) * base.min(MAX_MS * 0.001 * self.rate - base - 2.0) * 0.9;
        let step = rate / self.rate;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let angle = self.phase * std::f32::consts::TAU;
            let delays = [base + sweep * angle.sin(), base + sweep * angle.cos()];
            let dry = [*l, *r];
            let mut out = [0.0; 2];
            for ch in 0..2 {
                let echo = self.lines[ch].read(delays[ch]);
                self.lines[ch].push(dry[ch] + echo * feedback);
                out[ch] = dry[ch] * (1.0 - wet * 0.5) + echo * wet;
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
    fn widens_a_mono_signal() {
        let mut chorus = Chorus::default();
        chorus.set_param(4, 100.0);
        chorus.prepare(48_000, 4800);
        let input: Vec<f32> = (0..48_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (input.clone(), input);
        chorus.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 1.5));
        let difference: f32 = l[4800..]
            .iter()
            .zip(&r[4800..])
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(difference > 100.0, "left and right differ: {difference}");
    }
}
