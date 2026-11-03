//! Noise gate, expander and ducker: opens above the threshold, holds, then closes down to the
//! range once below the threshold minus the hysteresis. The detector follows the peaks of
//! the (filtered) key with a short release so that low notes do not make it chatter, and a
//! lookahead opens it before the transient.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{
    Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, key_peak, param, switch,
    time_coefficient,
};

pub const INFO: EffectInfo = EffectInfo {
    id: "gate",
    name: "Strada-Gate",
    params: &PARAMS,
    meters: &["Gain", "Input"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Gate, expander and ducker with hysteresis, lookahead, hold, range and a \
                  filtered sidechain.",
};

pub const MODES: [&str; 3] = ["Gate", "Expander", "Ducker"];
pub const MODE_HINTS: [&str; 3] = [
    "Shuts the sound below the threshold, down to the range",
    "Lowers the sound below the threshold by the ratio: gentler than a gate",
    "Lowers the sound by the range while the key is above the threshold",
];
pub const GATE: usize = 0;
pub const EXPANDER: usize = 1;
pub const DUCKER: usize = 2;
pub const MAX_LOOKAHEAD_MS: f32 = 10.0;
/// Release of the detector: long enough to ride over the cycles of a low note.
const DETECTOR_MS: f32 = 12.0;

pub(crate) mod p {
    pub const THRESHOLD: usize = 0;
    pub const RANGE: usize = 1;
    pub const ATTACK: usize = 2;
    pub const HOLD: usize = 3;
    pub const RELEASE: usize = 4;
    pub const HYSTERESIS: usize = 5;
    pub const LOOKAHEAD: usize = 6;
    pub const SC_HPF: usize = 7;
    pub const SC_LPF: usize = 8;
    pub const LISTEN: usize = 9;
    pub const MODE: usize = 10;
    pub const RATIO: usize = 11;
}
const COUNT: usize = 12;

const PARAMS: [ParamInfo; COUNT] = [
    param("threshold", "Threshold", -80.0, 0.0, -40.0, "dB"),
    param("range", "Range", -80.0, 0.0, -80.0, "dB"),
    param("attack", "Attack", 0.05, 50.0, 1.0, "ms"),
    param("hold", "Hold", 0.0, 500.0, 50.0, "ms"),
    param("release", "Release", 5.0, 2000.0, 150.0, "ms"),
    param("hysteresis", "Hysteresis", 0.0, 12.0, 4.0, "dB"),
    param("lookahead", "Lookahead", 0.0, MAX_LOOKAHEAD_MS, 0.0, "ms"),
    param("sc_hpf", "Sidechain HPF", 20.0, 2000.0, 20.0, "Hz"),
    param("sc_lpf", "Sidechain LPF", 1000.0, 20_000.0, 20_000.0, "Hz"),
    switch("listen", "Sidechain listen", 0.0),
    choice("mode", "Mode", &MODES, 0.0),
    param("ratio", "Ratio", 1.0, 10.0, 2.0, ""),
];

/// Delay the lookahead adds, in samples, for parameter `values` (in [`INFO`] order).
pub fn latency_of(values: &[f32], rate: f32) -> u32 {
    let ms = values
        .get(p::LOOKAHEAD)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, MAX_LOOKAHEAD_MS);
    (ms * 0.001 * rate).round() as u32
}

/// Gain in dB of an expander at `level` dB: down by the ratio below the threshold, to the
/// range at most.
pub fn expander_db(level: f32, threshold: f32, ratio: f32, range: f32) -> f32 {
    ((level - threshold).min(0.0) * (ratio - 1.0)).max(range)
}

#[derive(Debug)]
pub struct Gate {
    rate: f32,
    values: [f32; COUNT],
    open: bool,
    hold_left: u32,
    gain: f32,
    /// Peak follower of the key.
    envelope: f32,
    /// Peak of the detected signal over the last block, in dB.
    input: f32,
    attack: f32,
    release: f32,
    detector: f32,
    filters: [Coefficients; 2],
    filter_state: [[Biquad; 2]; 2],
    /// The audio, delayed by the lookahead (one line per channel).
    delay: [Vec<f32>; 2],
    at: usize,
}

impl Default for Gate {
    fn default() -> Self {
        let mut gate = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            open: false,
            hold_left: 0,
            gain: 0.0,
            envelope: 0.0,
            input: -120.0,
            attack: 0.0,
            release: 0.0,
            detector: 0.0,
            filters: [Coefficients::IDENTITY; 2],
            filter_state: Default::default(),
            delay: [Vec::new(), Vec::new()],
            at: 0,
        };
        gate.update();
        gate
    }
}

impl Gate {
    fn update(&mut self) {
        let v = self.values;
        self.attack = time_coefficient(v[p::ATTACK], self.rate);
        self.release = time_coefficient(v[p::RELEASE], self.rate);
        self.detector = time_coefficient(DETECTOR_MS, self.rate);
        let rate = f64::from(self.rate);
        let nyquist = rate * 0.45;
        self.filters = [
            if v[p::SC_HPF] > 21.0 {
                Coefficients::design(
                    FilterKind::HighPass,
                    f64::from(v[p::SC_HPF]),
                    0.707,
                    0.0,
                    rate,
                )
            } else {
                Coefficients::IDENTITY
            },
            if f64::from(v[p::SC_LPF]) < nyquist.min(19_900.0) {
                Coefficients::design(
                    FilterKind::LowPass,
                    f64::from(v[p::SC_LPF]),
                    0.707,
                    0.0,
                    rate,
                )
            } else {
                Coefficients::IDENTITY
            },
        ];
    }

    fn mode(&self) -> usize {
        (self.values[p::MODE].round().max(0.0) as usize).min(MODES.len() - 1)
    }

    /// Gain where the gate rests: the floor, or unity for a ducker.
    fn rest(&self) -> f32 {
        if self.mode() == DUCKER {
            1.0
        } else {
            db_to_gain(self.values[p::RANGE])
        }
    }
}

impl Effect for Gate {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        let frames = (MAX_LOOKAHEAD_MS * 0.001 * self.rate) as usize + 2;
        self.delay = [vec![0.0; frames], vec![0.0; frames]];
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
        self.run(left, right, None);
    }

    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        self.run(left, right, Some(key));
    }

    fn reset(&mut self) {
        self.open = false;
        self.hold_left = 0;
        self.envelope = 0.0;
        self.gain = self.rest();
        self.filter_state = Default::default();
        for line in &mut self.delay {
            line.fill(0.0);
        }
    }

    fn latency(&self) -> u32 {
        latency_of(&self.values, self.rate)
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
        let v = self.values;
        let (threshold, range) = (v[p::THRESHOLD], v[p::RANGE]);
        let close = threshold - v[p::HYSTERESIS];
        let floor = db_to_gain(range);
        let hold = (v[p::HOLD] * 0.001 * self.rate) as u32;
        let mode = self.mode();
        let listen = v[p::LISTEN] >= 0.5;
        let lookahead = (self.latency() as usize).min(self.delay[0].len().saturating_sub(1));
        let len = self.delay[0].len().max(1);
        let mut loudest = 0.0f32;
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            // The key through its filters, per side, then its peak.
            let raw = match key {
                Some((kl, kr)) => (
                    kl.get(i).copied().unwrap_or(0.0),
                    kr.get(i).or(kl.get(i)).copied().unwrap_or(0.0),
                ),
                None => (*l, *r),
            };
            let mut sides = [f64::from(raw.0), f64::from(raw.1)];
            for (side, state) in sides.iter_mut().zip(&mut self.filter_state) {
                for (filter, c) in state.iter_mut().zip(&self.filters) {
                    *side = filter.process(c, *side);
                }
            }
            let (kl, kr) = (sides[0] as f32, sides[1] as f32);
            let peak = key_peak(None, i, kl, kr);
            loudest = loudest.max(peak);
            self.envelope = if peak > self.envelope {
                peak
            } else {
                peak + self.detector * (self.envelope - peak)
            };
            let level = gain_to_db(self.envelope);
            if level > threshold {
                self.open = true;
                self.hold_left = hold;
            } else if self.open && level < close {
                if self.hold_left > 0 {
                    self.hold_left -= 1;
                } else {
                    self.open = false;
                }
            }
            let target = match mode {
                EXPANDER if !self.open => db_to_gain(expander_db(level, close, v[p::RATIO], range)),
                DUCKER if self.open => floor,
                DUCKER => 1.0,
                _ if self.open => 1.0,
                _ => floor,
            };
            // Attack is how fast the gain moves away from rest, release how it returns.
            let leaving = if mode == DUCKER {
                target < self.gain
            } else {
                target > self.gain
            };
            let coefficient = if leaving { self.attack } else { self.release };
            self.gain = target + coefficient * (self.gain - target);
            // The audio, delayed by the lookahead so the gate opens before the transient.
            let (dl, dr) = if lookahead > 0 {
                let read = (self.at + len - lookahead) % len;
                let out = (self.delay[0][read], self.delay[1][read]);
                self.delay[0][self.at] = *l;
                self.delay[1][self.at] = *r;
                self.at = (self.at + 1) % len;
                out
            } else {
                (*l, *r)
            };
            if listen {
                (*l, *r) = (kl, kr);
            } else {
                *l = dl * self.gain;
                *r = dr * self.gain;
            }
        }
        self.input = gain_to_db(loudest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn sine(f: f32, a: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| a * (std::f32::consts::TAU * f * i as f32 / 48_000.0).sin())
            .collect()
    }

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
    fn a_low_note_keeps_it_open_without_hold() {
        let mut g = Gate::default();
        g.set_param(index("hold"), 0.0);
        g.set_param(index("threshold"), -20.0);
        g.prepare(48_000, 4800);
        let mut l = sine(45.0, 0.3, 48_000);
        let mut r = l.clone();
        let input = l.clone();
        g.process(&mut l, &mut r);
        // Past the first cycle, the bass comes out whole.
        let worst = l[4800..]
            .iter()
            .zip(&input[4800..])
            .map(|(o, i)| (o - i).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "chatter: {worst}");
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
        // The hold and the detector keep it open for one block of silence.
        assert!((keyed(&mut g, 0.0) - 0.001).abs() < 1e-5, "held");
        for _ in 0..100 {
            keyed(&mut g, 0.0);
        }
        assert!(
            keyed(&mut g, 0.0) < 1e-5,
            "closed after the hold and release"
        );
        g.reset();
        assert!(!g.open && g.hold_left == 0);
        assert_eq!(g.gain, db_to_gain(g.values[1]));
    }

    #[test]
    fn lookahead_opens_before_the_hit_and_is_reported() {
        let mut g = Gate::default();
        g.set_param(index("lookahead"), 5.0);
        g.set_param(index("attack"), 0.5);
        g.prepare(48_000, 2400);
        assert_eq!(g.latency(), 240);
        let mut l = vec![0.0f32; 2400];
        for s in &mut l[1000..] {
            *s = 0.5;
        }
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        // The hit comes out 240 samples later, already at full level.
        assert!(l[1000 + 240 - 1].abs() < 1e-6);
        assert!((l[1000 + 240] - 0.5).abs() < 0.01, "{}", l[1240]);
    }

    #[test]
    fn expander_and_ducker() {
        assert_eq!(expander_db(-50.0, -40.0, 2.0, -80.0), -10.0);
        assert_eq!(expander_db(-30.0, -40.0, 3.0, -80.0), 0.0);
        assert_eq!(expander_db(-100.0, -40.0, 4.0, -24.0), -24.0);
        let mut g = Gate::default();
        g.set_param(index("mode"), DUCKER as f32);
        g.set_param(index("range"), -12.0);
        g.prepare(48_000, 480);
        let program = vec![0.5f32; 480];
        let duck = |g: &mut Gate, key: f32| {
            let (mut l, mut r) = (program.clone(), program.clone());
            let k = vec![key; 480];
            g.process_keyed(&mut l, &mut r, (&k, &k));
            gain_to_db(l[479] / 0.5)
        };
        assert!(duck(&mut g, 0.0).abs() < 0.01, "quiet key: untouched");
        for _ in 0..20 {
            duck(&mut g, 0.5);
        }
        assert!((duck(&mut g, 0.5) + 12.0).abs() < 0.1, "loud key: ducked");
    }

    #[test]
    fn listen_plays_the_filtered_key() {
        let mut g = Gate::default();
        g.set_param(index("sc_hpf"), 1000.0);
        g.set_param(index("listen"), 1.0);
        g.prepare(48_000, 48_000);
        let mut l = sine(100.0, 0.5, 48_000);
        let mut r = l.clone();
        g.process(&mut l, &mut r);
        let peak = l[24_000..].iter().fold(0.0f32, |p, s| p.max(s.abs()));
        assert!(peak < 0.01, "the low note is filtered out: {peak}");
    }
}
