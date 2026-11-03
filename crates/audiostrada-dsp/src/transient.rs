//! Transient shaper: compares a fast and a slow envelope to find attacks and sustains, then
//! boosts or softens each, whatever the level (no threshold). The sensitivity leaves the
//! small rises alone, a high-pass on the detection keeps the low end from triggering it, the
//! mix blends in the dry sound, and the guard catches what would go over full scale without
//! touching anything below it.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "transient",
    name: "Strada-Transient",
    params: &PARAMS,
    meters: &["Attack gain", "Sustain gain", "Gain", "Input", "Output"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "More or less punch and sustain on drums and plucks, without a threshold.",
};

pub const GUARDS: [&str; 3] = ["Off", "Clip", "Limit"];
/// The guard leaves everything under -1 dBFS alone and never lets a peak past -0.1 dBFS.
const KNEE: f32 = 0.891;
const CEILING: f32 = 0.989;
const LIMIT_RELEASE_MS: f32 = 50.0;

pub(crate) mod p {
    pub const ATTACK: usize = 0;
    pub const SUSTAIN: usize = 1;
    pub const SPEED: usize = 2;
    pub const GUARD: usize = 3;
    pub const OUTPUT: usize = 4;
    pub const MIX: usize = 5;
    pub const SENSITIVITY: usize = 6;
    pub const SC_HPF: usize = 7;
}
const COUNT: usize = 8;

const PARAMS: [ParamInfo; COUNT] = [
    param("attack", "Attack", -100.0, 100.0, 30.0, "%"),
    param("sustain", "Sustain", -100.0, 100.0, 0.0, "%"),
    param("speed", "Speed", 1.0, 50.0, 10.0, "ms"),
    choice("clip", "Guard", &GUARDS, 1.0),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("sensitivity", "Sensitivity", 0.0, 100.0, 0.0, "%"),
    param("sc_hpf", "Sidechain HPF", 20.0, 1000.0, 20.0, "Hz"),
];

/// A soft knee from -1 dBFS that reaches -0.1 dBFS at most; linear below the knee.
pub fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let room = CEILING - KNEE;
    (KNEE + room * ((a - KNEE) / room).tanh()).copysign(x)
}

#[derive(Debug)]
pub struct Transient {
    rate: f32,
    values: [f32; COUNT],
    fast: f32,
    slow: f32,
    hold: f32,
    gain: f32,
    limit: f32,
    filter: Coefficients,
    filter_state: [Biquad; 2],
    meters: [f32; 5],
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
            limit: 1.0,
            filter: Coefficients::IDENTITY,
            filter_state: [Biquad::default(); 2],
            meters: [0.0, 0.0, 0.0, -120.0, -120.0],
        }
    }
}

#[inline]
fn follow(env: &mut f32, x: f32, attack: f32, release: f32) {
    let c = if x > *env { attack } else { release };
    *env = x + c * (*env - x);
}

impl Transient {
    fn update(&mut self) {
        let hz = self.values[p::SC_HPF];
        self.filter = if hz > 21.0 {
            Coefficients::design(
                FilterKind::HighPass,
                f64::from(hz),
                0.707,
                0.0,
                f64::from(self.rate),
            )
        } else {
            Coefficients::IDENTITY
        };
    }
}

impl Effect for Transient {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.update();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            if index == p::SC_HPF {
                self.update();
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let v = self.values;
        let speed = v[p::SPEED];
        let c = |ms: f32| crate::time_coefficient(ms, self.rate);
        let (fast_a, fast_r) = (c(0.5), c(speed * 2.0));
        let (slow_a, slow_r) = (c(speed * 3.0), c(speed * 6.0));
        let (hold_a, hold_r) = (c(0.5), c(speed * 25.0));
        let smooth = c(1.0);
        let limit_release = c(LIMIT_RELEASE_MS);
        let (attack, sustain) = (v[p::ATTACK] / 100.0 * 1.6, v[p::SUSTAIN] / 100.0 * 1.6);
        // Rises and falls under the sensitivity are left alone.
        let floor = v[p::SENSITIVITY] / 100.0 * 1.5;
        let out = db_to_gain(v[p::OUTPUT]);
        let wet = v[p::MIX] / 100.0;
        let guard = v[p::GUARD].round() as usize;
        let (mut peak_a, mut peak_s, mut peak_g) = (0.0f32, 0.0f32, 0.0f32);
        let (mut input, mut output) = (0.0f32, 0.0f32);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let key = [*l, *r].map(f64::from);
            let level = key
                .iter()
                .zip(&mut self.filter_state)
                .map(|(x, f)| f.process(&self.filter, *x).abs() as f32)
                .fold(0.0f32, f32::max)
                + 1e-6;
            input = input.max(l.abs().max(r.abs()));
            follow(&mut self.fast, level, fast_a, fast_r);
            follow(&mut self.slow, level, slow_a, slow_r);
            follow(&mut self.hold, level, hold_a, hold_r);
            // Attacks: the fast envelope runs ahead of the slow one.
            let rise = ((self.fast / self.slow.max(1e-6)).ln() - floor).max(0.0);
            // Sustain: the level has fallen under its long hold.
            let fall = ((self.hold / self.fast.max(1e-6)).ln() - floor).clamp(0.0, 3.0);
            // Fades out towards silence, so the noise floor is not shaped.
            let present = ((gain_to_db(self.hold) + 70.0) / 10.0).clamp(0.0, 1.0);
            let shape_a = (attack * rise).clamp(-2.5, 2.5) * present;
            let shape_s = (sustain * (1.0 - (-fall).exp()) * 1.5).clamp(-2.5, 2.5) * present;
            let target = (shape_a + shape_s).exp();
            self.gain = target + smooth * (self.gain - target);
            // The strongest change of the block, with its sign, for the meters.
            for (peak, shape) in [
                (&mut peak_a, shape_a),
                (&mut peak_s, shape_s),
                (&mut peak_g, self.gain.ln()),
            ] {
                if shape.abs() > peak.abs() {
                    *peak = shape;
                }
            }
            let g = self.gain * wet + (1.0 - wet);
            let (mut a, mut b) = (*l * g * out, *r * g * out);
            match guard {
                1 => {
                    a = soft_clip(a);
                    b = soft_clip(b);
                }
                2 => {
                    // Down at once to the ceiling, back up over 50 ms.
                    let peak = a.abs().max(b.abs());
                    let needed = if peak > CEILING { CEILING / peak } else { 1.0 };
                    self.limit = if needed < self.limit {
                        needed
                    } else {
                        needed + limit_release * (self.limit - needed)
                    };
                    a *= self.limit;
                    b *= self.limit;
                }
                _ => {}
            }
            output = output.max(a.abs().max(b.abs()));
            *l = a;
            *r = b;
        }
        self.meters = [
            gain_to_db(peak_a.exp()),
            gain_to_db(peak_s.exp()),
            gain_to_db(peak_g.exp()),
            gain_to_db(input),
            gain_to_db(output),
        ];
    }

    fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
        self.hold = 0.0;
        self.gain = 1.0;
        self.limit = 1.0;
        self.filter_state = [Biquad::default(); 2];
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

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    /// A decaying tone burst, like a drum hit, every half second.
    fn hits() -> Vec<f32> {
        (0..48_000)
            .map(|i| {
                let t = (i % 24_000) as f32 / 48_000.0;
                0.5 * (-t * 12.0).exp() * (i as f32 * 0.05).sin()
            })
            .collect()
    }

    fn shaped(changes: &[(&str, f32)]) -> Vec<f32> {
        let mut t = Transient::default();
        t.set_param(index("clip"), 0.0);
        for (id, v) in changes {
            t.set_param(index(id), *v);
        }
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

    fn attack(x: &[f32]) -> f32 {
        energy(&x[24_000..24_000 + 240])
    }

    #[test]
    fn attack_and_sustain_move_their_part() {
        let flat = shaped(&[("attack", 0.0)]);
        let punch = shaped(&[("attack", 100.0)]);
        let soft = shaped(&[("attack", -100.0)]);
        assert!(attack(&punch) > attack(&flat) * 1.2, "more punch");
        assert!(attack(&soft) < attack(&flat) * 0.9, "softer");
        let long = shaped(&[("attack", 0.0), ("sustain", 100.0)]);
        let tail = |x: &[f32]| energy(&x[24_000 + 4800..24_000 + 14_400]);
        assert!(tail(&long) > tail(&flat) * 1.3, "more sustain");
        assert!(punch.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn mix_and_sensitivity_soften_the_effect() {
        let flat = attack(&shaped(&[("attack", 0.0)]));
        let full = attack(&shaped(&[("attack", 100.0)]));
        let half = attack(&shaped(&[("attack", 100.0), ("mix", 50.0)]));
        assert!(half > flat && half < full, "{flat} {half} {full}");
        let picky = attack(&shaped(&[("attack", 100.0), ("sensitivity", 100.0)]));
        assert!(picky < full, "{picky} {full}");
    }

    #[test]
    fn the_guard_only_acts_near_full_scale() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert_eq!(soft_clip(-0.8), -0.8);
        assert!(soft_clip(4.0) <= CEILING && soft_clip(1.0) > KNEE && soft_clip(1.0) < 1.0);
        for guard in [1.0, 2.0] {
            let quiet = shaped(&[("attack", 0.0), ("clip", guard)]);
            let dry = hits();
            assert!(quiet.iter().zip(&dry).all(|(a, b)| (a - b).abs() < 1e-6));
            let loud = shaped(&[("attack", 100.0), ("output", 12.0), ("clip", guard)]);
            let peak = loud.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak <= CEILING + 1e-4, "guard {guard}: {peak}");
        }
    }

    #[test]
    fn silence_is_left_alone() {
        let mut t = Transient::default();
        t.set_param(index("attack"), 100.0);
        t.set_param(index("sustain"), 100.0);
        t.prepare(48_000, 256);
        let mut l: Vec<f32> = (0..4800).map(|i| 1e-5 * (i as f32 * 0.7).sin()).collect();
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        assert!((t.gain - 1.0).abs() < 1e-3, "{}", t.gain);
    }
}
