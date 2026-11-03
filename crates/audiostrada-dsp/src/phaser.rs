//! Phaser: a chain of first-order all-passes whose corner sweeps exponentially between two
//! frequencies, with feedback; the notches move in and out of phase between the channels.

use crate::{Effect, EffectInfo, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "phaser",
    name: "Strada-Phaser",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Swirling 4 to 12 stage phaser with feedback and stereo spread.",
};

const STAGES: [&str; 4] = ["4 stages", "6 stages", "8 stages", "12 stages"];
const STAGE_COUNT: [usize; 4] = [4, 6, 8, 12];

const PARAMS: [ParamInfo; 8] = [
    param("rate", "Rate", 0.02, 8.0, 0.4, "Hz"),
    param("depth", "Depth", 0.0, 100.0, 80.0, "%"),
    param("low", "Low", 50.0, 2000.0, 250.0, "Hz"),
    param("high", "High", 500.0, 12000.0, 3500.0, "Hz"),
    param("feedback", "Feedback", -90.0, 90.0, 40.0, "%"),
    choice("stages", "Stages", &STAGES, 1.0),
    param("stereo", "Stereo", 0.0, 180.0, 90.0, "°"),
    param("mix", "Mix", 0.0, 100.0, 50.0, "%"),
];

#[derive(Debug)]
pub struct Phaser {
    rate: f32,
    values: [f32; 8],
    /// All-pass states (input and output memory) per channel and stage.
    states: [[(f32, f32); 12]; 2],
    last: [f32; 2],
    phase: f32,
}

impl Default for Phaser {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            states: [[(0.0, 0.0); 12]; 2],
            last: [0.0; 2],
            phase: 0.0,
        }
    }
}

impl Effect for Phaser {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [rate, depth, low, high, feedback, stages, stereo, mix] = self.values;
        let count = STAGE_COUNT[(stages.round() as usize).min(3)];
        let (lo, hi) = (low.min(high), high.max(low).min(self.rate * 0.45));
        let span = (hi / lo).ln() * depth / 100.0;
        let step = rate / self.rate;
        let offset = stereo / 360.0;
        let feedback = feedback / 100.0;
        let (wet, dry) = (mix / 100.0, 1.0 - mix / 100.0);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = [*l, *r];
            let mut out = [0.0; 2];
            for ch in 0..2 {
                let lfo = 0.5
                    - 0.5
                        * (std::f32::consts::TAU
                            * (self.phase + if ch == 1 { offset } else { 0.0 }))
                        .cos();
                let corner = lo * (span * lfo).exp();
                let t = (std::f32::consts::PI * corner / self.rate).tan();
                let a = (t - 1.0) / (t + 1.0);
                let mut x = input[ch] + self.last[ch] * feedback;
                for (x1, y1) in self.states[ch].iter_mut().take(count) {
                    let y = a * x + *x1 - a * *y1;
                    *x1 = x;
                    *y1 = y;
                    x = y;
                }
                self.last[ch] = x.clamp(-4.0, 4.0);
                // Dry plus all-passed, half each, makes the notches; mix fades it in.
                out[ch] = input[ch] * dry + (input[ch] + x) * 0.5 * wet;
            }
            *l = out[0];
            *r = out[1];
            self.phase = (self.phase + step).fract();
        }
    }

    fn reset(&mut self) {
        self.states = [[(0.0, 0.0); 12]; 2];
        self.last = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_count_is_stable() {
        for stages in 0..4 {
            for feedback in [-90.0, 90.0] {
                let mut p = Phaser::default();
                p.set_param(5, stages as f32);
                p.set_param(4, feedback);
                p.set_param(0, 3.0);
                p.prepare(48_000, 512);
                let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
                    .map(|i| {
                        let s = 0.5 * (i as f32 * 0.07).sin() + 0.3 * (i as f32 * 0.31).sin();
                        (s, s)
                    })
                    .unzip();
                p.process(&mut l, &mut r);
                assert!(
                    l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 4.0),
                    "{stages} {feedback}"
                );
            }
        }
    }
}
