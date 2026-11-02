//! Look-ahead brick-wall limiter. The gain needed by each sample is held over the look-ahead
//! window and averaged over the same length, so it ramps down before every peak reaches the
//! (delayed) output and the ceiling is never exceeded; it recovers with the release time.

use crate::loudness::{Hold, LIMITER_METERS, LevelMeter, limiter_meters};
use crate::{Effect, EffectInfo, ParamInfo, db_to_gain, param, time_coefficient};

const LOOKAHEAD_MS: f32 = 2.0;

pub const INFO: EffectInfo = EffectInfo {
    id: "limiter",
    name: "Limiter",
    params: &PARAMS,
    meters: LIMITER_METERS,
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Brickwall peak limiter with input gain and ceiling.",
};

const PARAMS: [ParamInfo; 3] = [
    param("input", "Input gain", 0.0, 24.0, 0.0, "dB"),
    param("ceiling", "Ceiling", -12.0, 0.0, -0.3, "dB"),
    param("release", "Release", 1.0, 1000.0, 60.0, "ms"),
];

#[derive(Debug)]
pub struct Limiter {
    rate: f32,
    values: [f32; 3],
    length: usize,
    /// Delayed audio, both channels interleaved.
    delay: Vec<f32>,
    /// Needed gains over the window, for the running minimum.
    needed: Vec<f32>,
    /// Indices into `needed` with increasing gains (running minimum).
    deque: Vec<usize>,
    deque_head: usize,
    deque_len: usize,
    /// Held minima, for the running average.
    held: Vec<f32>,
    sum: f64,
    position: usize,
    gain: f32,
    release: f32,
    reduction: Hold,
    input_meter: LevelMeter,
    output_meter: LevelMeter,
}

impl Limiter {
    fn new_state(&mut self) {
        self.length = ((LOOKAHEAD_MS * 0.001 * self.rate) as usize).max(1);
        self.delay = vec![0.0; self.length * 2];
        self.needed = vec![1.0; self.length];
        self.deque = vec![0; self.length];
        self.held = vec![1.0; self.length];
        self.deque_head = 0;
        self.deque_len = 0;
        self.sum = self.length as f64;
        self.position = 0;
        self.gain = 1.0;
    }

    /// Running minimum of the needed gains over the last `length` samples.
    fn push_minimum(&mut self, index: usize, value: f32) -> f32 {
        let n = self.length;
        self.needed[index] = value;
        // Drop the entry leaving the window, then larger entries from the back.
        if self.deque_len > 0 && self.deque[self.deque_head] == index {
            self.deque_head = (self.deque_head + 1) % n;
            self.deque_len -= 1;
        }
        while self.deque_len > 0 {
            let back = (self.deque_head + self.deque_len - 1) % n;
            if self.needed[self.deque[back]] >= value {
                self.deque_len -= 1;
            } else {
                break;
            }
        }
        let slot = (self.deque_head + self.deque_len) % n;
        self.deque[slot] = index;
        self.deque_len += 1;
        self.needed[self.deque[self.deque_head]]
    }
}

impl Default for Limiter {
    fn default() -> Self {
        let mut limiter = Limiter {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            length: 0,
            delay: Vec::new(),
            needed: Vec::new(),
            deque: Vec::new(),
            deque_head: 0,
            deque_len: 0,
            held: Vec::new(),
            sum: 0.0,
            position: 0,
            gain: 1.0,
            release: 0.0,
            reduction: Hold::new(48_000),
            input_meter: LevelMeter::new(48_000),
            output_meter: LevelMeter::new(48_000),
        };
        limiter.new_state();
        limiter
    }
}

impl Effect for Limiter {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.release = time_coefficient(self.values[2], self.rate);
        self.reduction = Hold::new(sample_rate);
        self.input_meter = LevelMeter::new(sample_rate);
        self.output_meter = LevelMeter::new(sample_rate);
        self.new_state();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            self.release = time_coefficient(self.values[2], self.rate);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let input = db_to_gain(self.values[0]);
        let ceiling = db_to_gain(self.values[1]);
        let n = self.length;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.input_meter.push(*l, *r);
            let (x, y) = (*l * input, *r * input);
            let peak = x.abs().max(y.abs());
            let needed = if peak > ceiling { ceiling / peak } else { 1.0 };
            let i = self.position;
            let minimum = self.push_minimum(i, needed);
            self.sum += f64::from(minimum) - f64::from(self.held[i]);
            self.held[i] = minimum;
            let average = (self.sum / n as f64) as f32;
            self.gain = if average < self.gain {
                average
            } else {
                average + self.release * (self.gain - average)
            };
            self.reduction.push(1.0 - self.gain);
            // The output is the input from `length - 1` samples ago.
            let out = (i + 1) % n;
            *l = self.delay[out * 2] * self.gain;
            *r = self.delay[out * 2 + 1] * self.gain;
            self.delay[i * 2] = x;
            self.delay[i * 2 + 1] = y;
            self.position = out;
            self.output_meter.push(*l, *r);
        }
    }

    // Runs on the audio thread after jumps: clears in place, never reallocates.
    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.needed.fill(1.0);
        self.held.fill(1.0);
        self.deque_head = 0;
        self.deque_len = 0;
        self.sum = self.length as f64;
        self.position = 0;
        self.gain = 1.0;
        self.reduction.reset();
        self.input_meter.reset();
        self.output_meter.reset();
    }

    fn latency(&self) -> u32 {
        self.length.saturating_sub(1) as u32
    }

    fn meters(&self, out: &mut [f32]) {
        limiter_meters(out, &self.reduction, &self.input_meter, &self.output_meter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_the_ceiling() {
        let mut limiter = Limiter::default();
        limiter.prepare(48_000, 256);
        limiter.set_param(0, 12.0);
        let ceiling = db_to_gain(-0.3);
        let mut max: f32 = 0.0;
        let mut n = 0usize;
        for _ in 0..400 {
            let mut l: Vec<f32> = (0..256)
                .map(|i| {
                    let t = (n + i) as f32 / 48_000.0;
                    (t * 440.0 * std::f32::consts::TAU).sin()
                        * if (n + i) % 9000 < 40 { 1.0 } else { 0.3 }
                })
                .collect();
            let mut r = l.clone();
            n += 256;
            limiter.process(&mut l, &mut r);
            max = l.iter().chain(&r).fold(max, |m, s| m.max(s.abs()));
        }
        assert!(max <= ceiling + 1e-4, "{max} > {ceiling}");
        assert!(max > ceiling * 0.9, "the limiter does not just mute");
        assert_eq!(limiter.latency(), 95);
    }

    #[test]
    fn quiet_signals_pass_unchanged_after_the_delay() {
        let mut limiter = Limiter::default();
        limiter.prepare(48_000, 256);
        let mut l: Vec<f32> = (0..256).map(|i| i as f32 / 1000.0 * 0.1).collect();
        let mut r = l.clone();
        let original = l.clone();
        limiter.process(&mut l, &mut r);
        let d = limiter.latency() as usize;
        assert!((l[100] - original[100 - d]).abs() < 1e-6);
    }

    #[test]
    fn reset_clears_the_look_ahead_and_meters_read() {
        let mut limiter = Limiter::default();
        limiter.prepare(48_000, 256);
        limiter.set_param(0, 24.0);
        let (mut l, mut r) = (vec![0.9f32; 256], vec![0.9f32; 256]);
        limiter.process(&mut l, &mut r);
        let mut meters = [0.0; LIMITER_METERS.len()];
        limiter.meters(&mut meters);
        assert!(meters[0] < -20.0, "reduction shown: {meters:?}");
        limiter.reset();
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        limiter.process(&mut l, &mut r);
        assert!(
            l.iter().chain(&r).all(|s| *s == 0.0),
            "nothing left in the delay"
        );
        assert_eq!(limiter.gain, 1.0);
    }
}
