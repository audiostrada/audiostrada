//! Transient shaper: compares a fast and a slow envelope to find attacks and sustains, then
//! boosts or softens each, whatever the level (no threshold).

use crate::{Effect, EffectInfo, ParamInfo, db_to_gain, gain_to_db, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "transient",
    name: "Transient Shaper",
    params: &PARAMS,
    meters: &["Attack gain", "Sustain gain"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "More or less punch and sustain on drums and plucks, without a threshold.",
};

const PARAMS: [ParamInfo; 5] = [
    param("attack", "Attack", -100.0, 100.0, 30.0, "%"),
    param("sustain", "Sustain", -100.0, 100.0, 0.0, "%"),
    param("speed", "Speed", 1.0, 50.0, 10.0, "ms"),
    switch("clip", "Soft clip", 1.0),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

#[derive(Debug)]
pub struct Transient {
    rate: f32,
    values: [f32; 5],
    fast: f32,
    slow: f32,
    hold: f32,
    gain: f32,
    meters: [f32; 2],
}

impl Default for Transient {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            fast: 0.0,
            slow: 0.0,
            hold: 0.0,
            gain: 1.0,
            meters: [0.0; 2],
        }
    }
}

#[inline]
fn follow(env: &mut f32, x: f32, attack: f32, release: f32) {
    let c = if x > *env { attack } else { release };
    *env = x + c * (*env - x);
}

impl Effect for Transient {
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
        let [attack, sustain, speed, clip, output] = self.values;
        let c = |ms: f32| crate::time_coefficient(ms, self.rate);
        let (fast_a, fast_r) = (c(0.5), c(speed * 2.0));
        let (slow_a, slow_r) = (c(speed * 3.0), c(speed * 6.0));
        let (hold_a, hold_r) = (c(0.5), c(speed * 25.0));
        let smooth = c(1.0);
        let (attack, sustain) = (attack / 100.0 * 1.6, sustain / 100.0 * 1.6);
        let out = db_to_gain(output);
        let clip = clip >= 0.5;
        let (mut peak_a, mut peak_s) = (0.0f32, 0.0f32);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let level = l.abs().max(r.abs()) + 1e-6;
            follow(&mut self.fast, level, fast_a, fast_r);
            follow(&mut self.slow, level, slow_a, slow_r);
            follow(&mut self.hold, level, hold_a, hold_r);
            // Attacks: the fast envelope runs ahead of the slow one.
            let rise = (self.fast / self.slow.max(1e-6)).ln().max(0.0);
            // Sustain: the level has fallen under its long hold.
            let fall = (self.hold / self.fast.max(1e-6)).ln().clamp(0.0, 3.0);
            let shape_a = (attack * rise).clamp(-2.5, 2.5);
            let shape_s = (sustain * (1.0 - (-fall).exp()) * 1.5).clamp(-2.5, 2.5);
            let target = (shape_a + shape_s).exp();
            self.gain = target + smooth * (self.gain - target);
            // The strongest change of the block, with its sign, for the meters.
            if shape_a.abs() > peak_a.abs() {
                peak_a = shape_a;
            }
            if shape_s.abs() > peak_s.abs() {
                peak_s = shape_s;
            }
            let g = self.gain * out;
            let (mut a, mut b) = (*l * g, *r * g);
            if clip {
                a = a.tanh();
                b = b.tanh();
            }
            *l = a;
            *r = b;
        }
        self.meters = [gain_to_db(peak_a.exp()), gain_to_db(peak_s.exp())];
    }

    fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
        self.hold = 0.0;
        self.gain = 1.0;
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip(self.meters) {
            *o = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decaying tone burst, like a drum hit, every half second.
    fn hits() -> Vec<f32> {
        (0..48_000)
            .map(|i| {
                let t = (i % 24_000) as f32 / 48_000.0;
                0.5 * (-t * 12.0).exp() * (i as f32 * 0.05).sin()
            })
            .collect()
    }

    fn shaped(attack: f32, sustain: f32) -> Vec<f32> {
        let mut t = Transient::default();
        t.set_param(0, attack);
        t.set_param(1, sustain);
        t.set_param(3, 0.0);
        t.prepare(48_000, 256);
        let (mut l, mut r) = (hits(), hits());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            t.process(a, b);
        }
        l
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum()
    }

    #[test]
    fn attack_and_sustain_move_their_part() {
        let flat = shaped(0.0, 0.0);
        let punch = shaped(100.0, 0.0);
        let soft = shaped(-100.0, 0.0);
        let attack = |x: &[f32]| energy(&x[24_000..24_000 + 240]);
        assert!(attack(&punch) > attack(&flat) * 1.2, "more punch");
        assert!(attack(&soft) < attack(&flat) * 0.9, "softer");
        let long = shaped(0.0, 100.0);
        let tail = |x: &[f32]| energy(&x[24_000 + 4800..24_000 + 14_400]);
        assert!(tail(&long) > tail(&flat) * 1.3, "more sustain");
        assert!(punch.iter().all(|s| s.is_finite()));
    }
}
