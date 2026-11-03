//! De-esser: listens to the sibilance (through a high-pass or a band-pass), and when it is
//! louder than the threshold, turns down the highs (split mode) or the whole signal (wide
//! mode). The level can be read as is, or relative to the whole signal so that the threshold
//! holds whatever the singer's level. A lookahead catches the start of each S, and listening
//! plays the trigger or what is removed.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{
    Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, param, switch, time_coefficient,
};

pub const INFO: EffectInfo = EffectInfo {
    id: "deesser",
    name: "Strada-DeEss",
    params: &PARAMS,
    meters: &["Gain reduction", "Sibilance"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Tames harsh S and T sounds on vocals, on the highs only or the whole voice.",
};

pub const MODES: [&str; 2] = ["Split", "Wide"];
pub const MODE_HINTS: [&str; 2] = [
    "Turns down only the highs, above the crossover: the voice keeps its body",
    "Turns down the whole voice for a moment: more natural on soft sibilance",
];
pub const LISTEN: [&str; 3] = ["Off", "Trigger", "Delta"];
pub const DETECTORS: [&str; 2] = ["High pass", "Band pass"];
pub const MAX_LOOKAHEAD_MS: f32 = 15.0;
/// Slope above the threshold: about 4:1.
const SLOPE: f32 = 0.75;
/// Where the split mode's crossover sits, below the frequency.
pub const SPLIT: f64 = 0.7;

pub(crate) mod p {
    pub const FREQUENCY: usize = 0;
    pub const THRESHOLD: usize = 1;
    pub const RANGE: usize = 2;
    pub const MODE: usize = 3;
    pub const LISTEN: usize = 4;
    pub const DETECTOR: usize = 5;
    pub const RELATIVE: usize = 6;
    pub const ATTACK: usize = 7;
    pub const RELEASE: usize = 8;
    pub const LOOKAHEAD: usize = 9;
}
const COUNT: usize = 10;

const PARAMS: [ParamInfo; COUNT] = [
    param("frequency", "Frequency", 2000.0, 12000.0, 6500.0, "Hz"),
    param("threshold", "Threshold", -60.0, 0.0, -30.0, "dB"),
    param("range", "Range", 0.0, 24.0, 10.0, "dB"),
    choice("mode", "Mode", &MODES, 0.0),
    choice("listen", "Listen", &LISTEN, 0.0),
    choice("detector", "Detector", &DETECTORS, 0.0),
    switch("relative", "Relative", 0.0),
    param("attack", "Attack", 0.1, 10.0, 0.5, "ms"),
    param("release", "Release", 10.0, 500.0, 60.0, "ms"),
    param("lookahead", "Lookahead", 0.0, MAX_LOOKAHEAD_MS, 0.0, "ms"),
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

/// The detection filter for parameter `values`.
pub fn detector(values: &[f32], rate: f64) -> Coefficients {
    let f = f64::from(values.get(p::FREQUENCY).copied().unwrap_or(6500.0));
    if values.get(p::DETECTOR).is_some_and(|&d| d >= 0.5) {
        Coefficients::design(FilterKind::BandPass, f, 1.4, 0.0, rate)
    } else {
        // A slightly resonant high-pass is enough to catch sibilance.
        Coefficients::design(FilterKind::HighPass, f, 1.2, 0.0, rate)
    }
}

#[derive(Debug)]
pub struct DeEsser {
    rate: f32,
    values: [f32; COUNT],
    band: Coefficients,
    /// Linkwitz-Riley crossover under the sibilance: low-pass and high-pass.
    split: [Coefficients; 2],
    detect: Biquad,
    /// Per channel: two low-pass and two high-pass stages.
    crossover: [[Biquad; 4]; 2],
    /// Followers of the band and of the whole signal.
    envelope: f32,
    full: f32,
    gain_db: f32,
    meters: [f32; 2],
    delay: [Vec<f32>; 2],
    at: usize,
}

impl Default for DeEsser {
    fn default() -> Self {
        let mut d = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            band: Coefficients::IDENTITY,
            split: [Coefficients::IDENTITY; 2],
            detect: Biquad::default(),
            crossover: [[Biquad::default(); 4]; 2],
            envelope: 0.0,
            full: 0.0,
            gain_db: 0.0,
            meters: [0.0, -120.0],
            delay: [Vec::new(), Vec::new()],
            at: 0,
        };
        d.update();
        d
    }
}

impl DeEsser {
    fn update(&mut self) {
        let rate = f64::from(self.rate);
        let f = f64::from(self.values[p::FREQUENCY]);
        self.band = detector(&self.values, rate);
        let q = std::f64::consts::FRAC_1_SQRT_2;
        self.split = [
            Coefficients::design(FilterKind::LowPass, f * SPLIT, q, 0.0, rate),
            Coefficients::design(FilterKind::HighPass, f * SPLIT, q, 0.0, rate),
        ];
    }
}

impl Effect for DeEsser {
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
        let v = self.values;
        let (threshold, range) = (v[p::THRESHOLD], v[p::RANGE]);
        let split = v[p::MODE] < 0.5;
        let listen = v[p::LISTEN].round() as usize;
        let relative = v[p::RELATIVE] >= 0.5;
        let attack = time_coefficient(v[p::ATTACK], self.rate);
        let release = time_coefficient(v[p::RELEASE], self.rate);
        // A short glide on the gain itself, so that it never steps.
        let smooth = time_coefficient(0.3, self.rate);
        let len = self.delay[0].len().max(1);
        let lookahead = (self.latency() as usize).min(len - 1);
        let (mut deepest, mut loudest) = (0.0f32, -120.0f32);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mono = 0.5 * (*l + *r);
            let band = self.detect.process(&self.band, f64::from(mono)) as f32;
            let follow = |envelope: &mut f32, level: f32| {
                let c = if level > *envelope { attack } else { release };
                *envelope = level + c * (*envelope - level);
            };
            follow(&mut self.envelope, band.abs());
            follow(&mut self.full, mono.abs());
            let db = if relative {
                gain_to_db(self.envelope) - gain_to_db(self.full.max(1e-6))
            } else {
                gain_to_db(self.envelope)
            };
            loudest = loudest.max(db);
            let target = -((db - threshold).max(0.0) * SLOPE).min(range);
            self.gain_db = target + smooth * (self.gain_db - target);
            deepest = deepest.min(self.gain_db);
            let g = f64::from(db_to_gain(self.gain_db));
            // The audio, delayed so that the gain is already down when the S starts.
            let dry = if lookahead > 0 {
                let read = (self.at + len - lookahead) % len;
                let out = [self.delay[0][read], self.delay[1][read]];
                self.delay[0][self.at] = *l;
                self.delay[1][self.at] = *r;
                self.at = (self.at + 1) % len;
                out
            } else {
                [*l, *r]
            };
            if listen == 1 {
                (*l, *r) = (band, band);
                continue;
            }
            for (ch, s) in [&mut *l, &mut *r].into_iter().enumerate() {
                let x = f64::from(dry[ch]);
                let out = if split {
                    let f = &mut self.crossover[ch];
                    let low = f[0].process(&self.split[0], x);
                    let low = f[1].process(&self.split[0], low);
                    let high = f[2].process(&self.split[1], x);
                    let high = f[3].process(&self.split[1], high);
                    // The crossover's sum is an all-pass: the delta compares like with like.
                    if listen == 2 {
                        high * (1.0 - g)
                    } else {
                        low + high * g
                    }
                } else if listen == 2 {
                    x * (1.0 - g)
                } else {
                    x * g
                };
                *s = out as f32;
            }
        }
        self.meters = [deepest, loudest];
    }

    fn reset(&mut self) {
        self.detect = Biquad::default();
        self.crossover = [[Biquad::default(); 4]; 2];
        self.envelope = 0.0;
        self.full = 0.0;
        self.gain_db = 0.0;
        for line in &mut self.delay {
            line.fill(0.0);
        }
    }

    fn latency(&self) -> u32 {
        latency_of(&self.values, self.rate)
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

    fn tone(f: f32, a: f32) -> Vec<f32> {
        (0..24_000)
            .map(|i| a * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn peak_after(d: &mut DeEsser, f: f32, a: f32) -> f32 {
        let (mut l, mut r) = (tone(f, a), tone(f, a));
        d.process(&mut l, &mut r);
        l[12_000..].iter().fold(0.0f32, |m, s| m.max(s.abs())) / a
    }

    fn fresh(changes: &[(&str, f32)]) -> DeEsser {
        let mut d = DeEsser::default();
        for (id, v) in changes {
            d.set_param(index(id), *v);
        }
        d.prepare(48_000, 512);
        d
    }

    #[test]
    fn turns_down_sibilance_and_leaves_the_voice() {
        for mode in [0.0, 1.0] {
            for detector in [0.0, 1.0] {
                let set = [("mode", mode), ("detector", detector)];
                let ess = peak_after(&mut fresh(&set), 7000.0, 0.5);
                assert!(ess < db_to_gain(-4.0), "mode {mode} {detector}: {ess}");
                let voice = peak_after(&mut fresh(&set), 300.0, 0.5);
                assert!(
                    (voice - 1.0).abs() < 0.06,
                    "mode {mode} {detector}: {voice}"
                );
            }
        }
    }

    #[test]
    fn relative_detection_ignores_the_level() {
        // Sibilance alone, loud then quiet: relative to itself it is always 0 dB.
        let set = [("relative", 1.0), ("threshold", -12.0)];
        let loud = peak_after(&mut fresh(&set), 7000.0, 0.5);
        let quiet = peak_after(&mut fresh(&set), 7000.0, 0.02);
        assert!(
            (gain_to_db(loud) - gain_to_db(quiet)).abs() < 0.5,
            "{loud} {quiet}"
        );
        assert!(loud < db_to_gain(-4.0));
        // A voice alone has little energy up there: left alone.
        let voice = peak_after(&mut fresh(&set), 300.0, 0.5);
        assert!((voice - 1.0).abs() < 0.06, "{voice}");
    }

    #[test]
    fn delta_plays_what_is_removed_and_lookahead_is_reported() {
        let ess = peak_after(&mut fresh(&[("mode", 1.0)]), 7000.0, 0.5);
        let delta = peak_after(&mut fresh(&[("mode", 1.0), ("listen", 2.0)]), 7000.0, 0.5);
        assert!((ess + delta - 1.0).abs() < 0.02, "{ess} + {delta}");
        let d = fresh(&[("lookahead", 10.0)]);
        assert_eq!(d.latency(), 480);
        assert_eq!(latency_of(&d.values, 48_000.0), 480);
    }
}
