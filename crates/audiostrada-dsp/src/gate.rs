//! Noise gate: opens above the threshold, holds, then closes down to the range, with
//! hysteresis so that it does not chatter around the threshold.

use crate::{
    Effect, EffectInfo, ParamInfo, db_to_gain, gain_to_db, key_peak, param, time_coefficient,
};

const HYSTERESIS_DB: f32 = 4.0;

pub const INFO: EffectInfo = EffectInfo {
    id: "gate",
    name: "Gate",
    params: &PARAMS,
    meters: &["Gain", "Input"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Noise gate with hold, range and sidechain.",
};

const PARAMS: [ParamInfo; 5] = [
    param("threshold", "Threshold", -80.0, 0.0, -40.0, "dB"),
    param("range", "Range", -80.0, 0.0, -80.0, "dB"),
    param("attack", "Attack", 0.05, 50.0, 1.0, "ms"),
    param("hold", "Hold", 0.0, 500.0, 50.0, "ms"),
    param("release", "Release", 5.0, 2000.0, 150.0, "ms"),
];

#[derive(Debug)]
pub struct Gate {
    rate: f32,
    values: [f32; 5],
    open: bool,
    hold_left: u32,
    gain: f32,
    /// Peak of the detected signal over the last block, in dB.
    input: f32,
    attack: f32,
    release: f32,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            open: false,
            hold_left: 0,
            gain: 0.0,
            input: -120.0,
            attack: 0.0,
            release: 0.0,
        }
    }
}

impl Gate {
    fn update_times(&mut self) {
        self.attack = time_coefficient(self.values[2], self.rate);
        self.release = time_coefficient(self.values[4], self.rate);
    }
}

impl Effect for Gate {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.update_times();
        self.gain = db_to_gain(self.values[1]);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.update_times();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.run(left, right, None);
    }

    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        self.run(left, right, Some(key));
    }

    fn reset(&mut self) {
        self.open = false;
        self.hold_left = 0;
        self.gain = db_to_gain(self.values[1]);
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip([gain_to_db(self.gain), self.input]) {
            *o = v;
        }
    }
}

impl Gate {
    /// Gates `left`/`right`, listening to `key` when given, else to the signal itself.
    fn run(&mut self, left: &mut [f32], right: &mut [f32], key: Option<(&[f32], &[f32])>) {
        let [threshold, range, _, hold, _] = self.values;
        let floor = db_to_gain(range);
        let hold = (hold * 0.001 * self.rate) as u32;
        let mut loudest = 0.0f32;
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let peak = key_peak(key, i, *l, *r);
            loudest = loudest.max(peak);
            let level = gain_to_db(peak);
            if level > threshold {
                self.open = true;
                self.hold_left = hold;
            } else if self.open && level < threshold - HYSTERESIS_DB {
                if self.hold_left > 0 {
                    self.hold_left -= 1;
                } else {
                    self.open = false;
                }
            }
            let (target, coefficient) = if self.open {
                (1.0, self.attack)
            } else {
                (floor, self.release)
            };
            self.gain = target + coefficient * (self.gain - target);
            *l *= self.gain;
            *r *= self.gain;
        }
        self.input = gain_to_db(loudest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_signal_and_closes_on_silence() {
        let mut g = Gate::default();
        g.prepare(48_000, 480);
        let run = |g: &mut Gate, level: f32, blocks: usize| {
            let mut out = 0.0;
            for _ in 0..blocks {
                let (mut l, mut r) = (vec![level; 480], vec![level; 480]);
                g.process(&mut l, &mut r);
                out = l[479];
            }
            out
        };
        assert!((run(&mut g, 0.5, 20) - 0.5).abs() < 0.01, "open");
        let quiet = run(&mut g, 0.001, 200);
        assert!(quiet < 0.001 * 0.01, "closed to the range: {quiet}");
        let mut meters = [0.0; 2];
        g.meters(&mut meters);
        assert!(meters[0] < -60.0 && (meters[1] - gain_to_db(0.001)).abs() < 0.01);
    }

    #[test]
    fn the_key_opens_and_holds_the_gate() {
        let mut g = Gate::default();
        g.set_param(3, 10.0);
        g.prepare(48_000, 480);
        let quiet = vec![0.001f32; 480];
        let keyed = |g: &mut Gate, key: f32| {
            let (mut l, mut r) = (quiet.clone(), quiet.clone());
            let k = vec![key; 480];
            g.process_keyed(&mut l, &mut r, (&k, &k));
            l[479]
        };
        for _ in 0..10 {
            keyed(&mut g, 0.5);
        }
        assert!((keyed(&mut g, 0.5) - 0.001).abs() < 1e-5, "open on the key");
        // The 10 ms hold (480 samples) keeps it open for one block of silence.
        assert!((keyed(&mut g, 0.0) - 0.001).abs() < 1e-5, "held");
        for _ in 0..100 {
            keyed(&mut g, 0.0);
        }
        assert!(
            keyed(&mut g, 0.0) < 1e-5,
            "closed after the hold and release"
        );
        // A key shorter than the block counts as silence past its end.
        let (mut l, mut r) = (quiet.clone(), quiet.clone());
        g.process_keyed(&mut l, &mut r, (&[0.5], &[]));
        assert!(g.open);
        g.reset();
        assert!(!g.open && g.hold_left == 0);
        assert_eq!(g.gain, db_to_gain(g.values[1]));
    }
}
