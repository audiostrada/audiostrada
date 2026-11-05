//! Multiband compressor: two to four bands split by Linkwitz-Riley crossovers (24 dB per
//! octave, the bands summing back flat, the lower ones phase-aligned with all-passes). Each band
//! compresses above its threshold and can lift what lies below a second one (upward
//! compression), with its own times, gain, bypass and solo. The mix scales each band's change,
//! so the dry signal stays phase-aligned with the bands.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, param, switch};

pub const BANDS: usize = 4;
/// Output spectrum, then each band's gain change (dB) and level (dBFS).
pub const METERS: usize = crate::analyzer::BANDS + 2 * BANDS;
pub const GAINS: usize = crate::analyzer::BANDS;
pub const LEVELS: usize = GAINS + BANDS;

pub const INFO: EffectInfo = EffectInfo {
    id: "mb_comp",
    name: "Strada-MB-Comp",
    params: &PARAMS,
    meters: &[""; METERS],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Multiband compressor: up to four bands, each compressed from above and \
                  lifted from below, for mastering, buses and the OTT sound.",
};

pub const COUNTS: [&str; 3] = ["2 bands", "3 bands", "4 bands"];

pub mod p {
    pub const COUNT: usize = 0;
    pub const X1: usize = 1;
    pub const X2: usize = 2;
    pub const X3: usize = 3;
    pub const MIX: usize = 4;
    pub const OUTPUT: usize = 5;
    pub const TIME: usize = 6;
    /// 1-based band heard alone, 0 for none; set by the editor without being saved.
    pub const SOLO: usize = 7;
    pub const FIRST_BAND: usize = 8;
    pub const PER_BAND: usize = 8;
    pub const THRESHOLD: usize = 0;
    pub const RATIO: usize = 1;
    pub const BELOW: usize = 2;
    pub const UPWARD: usize = 3;
    pub const ATTACK: usize = 4;
    pub const RELEASE: usize = 5;
    pub const GAIN: usize = 6;
    pub const BYPASS: usize = 7;
}
const COUNT: usize = p::FIRST_BAND + BANDS * p::PER_BAND;

macro_rules! band {
    ($n:literal, $attack:literal, $release:literal) => {
        [
            param(
                concat!("b", $n, "_thresh"),
                concat!("Band ", $n, " threshold"),
                -60.0,
                0.0,
                -18.0,
                "dB",
            ),
            param(
                concat!("b", $n, "_ratio"),
                concat!("Band ", $n, " ratio"),
                1.0,
                20.0,
                2.0,
                ":1",
            ),
            param(
                concat!("b", $n, "_below"),
                concat!("Band ", $n, " lift threshold"),
                -80.0,
                0.0,
                -40.0,
                "dB",
            ),
            param(
                concat!("b", $n, "_up"),
                concat!("Band ", $n, " lift ratio"),
                1.0,
                10.0,
                1.0,
                ":1",
            ),
            param(
                concat!("b", $n, "_attack"),
                concat!("Band ", $n, " attack"),
                0.1,
                300.0,
                $attack,
                "ms",
            ),
            param(
                concat!("b", $n, "_release"),
                concat!("Band ", $n, " release"),
                5.0,
                3000.0,
                $release,
                "ms",
            ),
            param(
                concat!("b", $n, "_gain"),
                concat!("Band ", $n, " gain"),
                -24.0,
                24.0,
                0.0,
                "dB",
            ),
            switch(
                concat!("b", $n, "_bypass"),
                concat!("Band ", $n, " bypass"),
                0.0,
            ),
        ]
    };
}

const fn params() -> [ParamInfo; COUNT] {
    let head = [
        choice("count", "Bands", &COUNTS, 1.0),
        param("x1", "Crossover 1", 20.0, 16_000.0, 150.0, "Hz"),
        param("x2", "Crossover 2", 20.0, 16_000.0, 2_500.0, "Hz"),
        param("x3", "Crossover 3", 20.0, 16_000.0, 8_000.0, "Hz"),
        param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
        param("output", "Output", -24.0, 24.0, 0.0, "dB"),
        param("time", "Time", 25.0, 400.0, 100.0, "%"),
        param("solo", "Solo", 0.0, 4.0, 0.0, ""),
    ];
    let bands = [
        band!("1", 20.0, 200.0),
        band!("2", 10.0, 150.0),
        band!("3", 5.0, 100.0),
        band!("4", 3.0, 80.0),
    ];
    let mut out = [head[0]; COUNT];
    let mut i = 0;
    while i < p::FIRST_BAND {
        out[i] = head[i];
        i += 1;
    }
    let mut b = 0;
    while b < BANDS {
        let mut k = 0;
        while k < p::PER_BAND {
            out[p::FIRST_BAND + b * p::PER_BAND + k] = bands[b][k];
            k += 1;
        }
        b += 1;
    }
    out
}

const PARAMS: [ParamInfo; COUNT] = params();
const KNEE_DB: f32 = 6.0;
/// Most an upward band lifts, and the level under which it stops lifting (the noise floor).
const MAX_LIFT_DB: f32 = 24.0;
const FLOOR_DB: f32 = -80.0;

/// Gain change (dB, at most 0) of a downward compressor with a soft knee.
pub fn downward(level: f32, threshold: f32, ratio: f32) -> f32 {
    let over = level - threshold;
    let slope = 1.0 / ratio.max(1.0) - 1.0;
    if 2.0 * over <= -KNEE_DB {
        0.0
    } else if 2.0 * over.abs() < KNEE_DB {
        slope * (over + KNEE_DB / 2.0).powi(2) / (2.0 * KNEE_DB)
    } else {
        slope * over
    }
}

/// Gain change (dB, at least 0) lifting what lies under `below` towards it, fading out near
/// the noise floor.
pub fn upward(level: f32, below: f32, ratio: f32) -> f32 {
    let lift = -downward(-level, -below, ratio);
    let fade = ((level - FLOOR_DB) / 20.0).clamp(0.0, 1.0);
    lift.min(MAX_LIFT_DB) * fade
}

/// Crossovers in use for a band count, kept apart and below Nyquist.
pub fn crossovers(values: &[f32], rate: f32) -> [f32; 3] {
    let top = rate * 0.45;
    let x1 = values[p::X1].clamp(20.0, top / 2.0);
    let x2 = values[p::X2].max(x1 * 1.3).min(top / 1.3);
    let x3 = values[p::X3].max(x2 * 1.3).min(top);
    [x1, x2, x3]
}

/// A band's settings for one block, its times as smoothing coefficients.
#[derive(Clone, Copy, Debug, Default)]
struct Band {
    threshold: f32,
    ratio: f32,
    below: f32,
    up: f32,
    attack: f32,
    release: f32,
    gain: f32,
    bypass: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Split {
    low: [Biquad; 2],
    high: [Biquad; 2],
}

impl Split {
    /// The low and high halves, each through two Butterworth sections.
    #[inline]
    fn run(&mut self, x: f64, lp: &Coefficients, hp: &Coefficients) -> (f64, f64) {
        let l = self.low[0].process(lp, x);
        let l = self.low[1].process(lp, l);
        let h = self.high[0].process(hp, x);
        let h = self.high[1].process(hp, h);
        (l, h)
    }
}

#[derive(Debug)]
pub struct MultibandComp {
    rate: f32,
    values: [f32; COUNT],
    lp: [Coefficients; 3],
    hp: [Coefficients; 3],
    ap: [Coefficients; 3],
    /// Per channel: the three splits and the all-passes aligning the lower bands.
    splits: [[Split; 3]; 2],
    aligns: [[Biquad; 3]; 2],
    envelopes: [f32; BANDS],
    shown_gain: [f32; BANDS],
    shown_level: [f32; BANDS],
    analyzer: crate::analyzer::Analyzer,
}

impl Default for MultibandComp {
    fn default() -> Self {
        let mut comp = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            lp: [Coefficients::IDENTITY; 3],
            hp: [Coefficients::IDENTITY; 3],
            ap: [Coefficients::IDENTITY; 3],
            splits: [[Split::default(); 3]; 2],
            aligns: [[Biquad::default(); 3]; 2],
            envelopes: [0.0; BANDS],
            shown_gain: [0.0; BANDS],
            shown_level: [-120.0; BANDS],
            analyzer: crate::analyzer::Analyzer::default(),
        };
        comp.update();
        comp
    }
}

impl MultibandComp {
    fn bands(&self) -> usize {
        self.values[p::COUNT].round().clamp(0.0, 2.0) as usize + 2
    }

    fn band(&self, b: usize, k: usize) -> f32 {
        self.values[p::FIRST_BAND + b * p::PER_BAND + k]
    }

    fn update(&mut self) {
        let rate = f64::from(self.rate);
        let q = std::f64::consts::FRAC_1_SQRT_2;
        for (k, x) in crossovers(&self.values, self.rate).into_iter().enumerate() {
            let x = f64::from(x);
            self.lp[k] = Coefficients::design(FilterKind::LowPass, x, q, 0.0, rate);
            self.hp[k] = Coefficients::design(FilterKind::HighPass, x, q, 0.0, rate);
            self.ap[k] = Coefficients::design(FilterKind::AllPass, x, q, 0.0, rate);
        }
    }

    /// One channel's sample split into its bands, all in phase with each other.
    #[inline]
    fn split(&mut self, ch: usize, x: f64, count: usize) -> [f64; BANDS] {
        let (lp, hp, ap) = (self.lp, self.hp, self.ap);
        let splits = &mut self.splits[ch];
        let aligns = &mut self.aligns[ch];
        let (low, high) = splits[0].run(x, &lp[0], &hp[0]);
        match count {
            2 => [low, high, 0.0, 0.0],
            3 => {
                let low = aligns[0].process(&ap[1], low);
                let (mid, high) = splits[1].run(high, &lp[1], &hp[1]);
                [low, mid, high, 0.0]
            }
            _ => {
                let low = aligns[0].process(&ap[1], low);
                let low = aligns[1].process(&ap[2], low);
                let (mid, rest) = splits[1].run(high, &lp[1], &hp[1]);
                let mid = aligns[2].process(&ap[2], mid);
                let (upper, high) = splits[2].run(rest, &lp[2], &hp[2]);
                [low, mid, upper, high]
            }
        }
    }
}

impl Effect for MultibandComp {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.analyzer.prepare(sample_rate, max_frames);
        self.update();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            if matches!(index, p::COUNT | p::X1 | p::X2 | p::X3) {
                self.update();
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let count = self.bands();
        let time = self.values[p::TIME] / 100.0;
        let mix = self.values[p::MIX] / 100.0;
        let output = db_to_gain(self.values[p::OUTPUT]);
        let solo = self.values[p::SOLO].round() as usize;
        let mut settings = [Band::default(); BANDS];
        for (b, s) in settings.iter_mut().enumerate().take(count) {
            let coefficient =
                |ms: f32| 1.0 - (-1.0 / (ms * time * 0.001 * self.rate).max(1.0)).exp();
            *s = Band {
                threshold: self.band(b, p::THRESHOLD),
                ratio: self.band(b, p::RATIO),
                below: self.band(b, p::BELOW),
                up: self.band(b, p::UPWARD),
                attack: coefficient(self.band(b, p::ATTACK)),
                release: coefficient(self.band(b, p::RELEASE)),
                gain: self.band(b, p::GAIN),
                bypass: self.band(b, p::BYPASS) >= 0.5,
            };
        }
        let mut extreme = [0.0f32; BANDS];
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let bl = self.split(0, f64::from(*l), count);
            let br = self.split(1, f64::from(*r), count);
            let mut out = [0.0f64; 2];
            for (b, s) in settings.iter().enumerate().take(count) {
                let peak = bl[b].abs().max(br[b].abs()) as f32;
                let env = &mut self.envelopes[b];
                *env += if peak > *env { s.attack } else { s.release } * (peak - *env);
                let level = gain_to_db(*env);
                let change = if s.bypass {
                    0.0
                } else {
                    downward(level, s.threshold, s.ratio) + upward(level, s.below, s.up) + s.gain
                };
                if change.abs() > extreme[b].abs() {
                    extreme[b] = change;
                }
                if solo != 0 && solo != b + 1 {
                    continue;
                }
                let g = f64::from(1.0 + mix * (db_to_gain(change) - 1.0));
                out[0] += bl[b] * g;
                out[1] += br[b] * g;
            }
            *l = out[0] as f32 * output;
            *r = out[1] as f32 * output;
        }
        for (b, ((shown, level), env)) in self
            .shown_gain
            .iter_mut()
            .zip(self.shown_level.iter_mut())
            .zip(self.envelopes)
            .enumerate()
        {
            let used = b < count;
            *shown = if used { extreme[b] } else { 0.0 };
            *level = if used {
                gain_to_db(env).max(-120.0)
            } else {
                -120.0
            };
        }
        self.analyzer.process(left, right);
    }

    fn reset(&mut self) {
        self.splits = [[Split::default(); 3]; 2];
        self.aligns = [[Biquad::default(); 3]; 2];
        self.envelopes = [0.0; BANDS];
        self.shown_gain = [0.0; BANDS];
        self.shown_level = [-120.0; BANDS];
        self.analyzer.reset();
    }

    fn meters(&self, out: &mut [f32]) {
        self.analyzer.meters(out);
        for b in 0..BANDS {
            if let Some(slot) = out.get_mut(GAINS + b) {
                *slot = self.shown_gain[b];
            }
            if let Some(slot) = out.get_mut(LEVELS + b) {
                *slot = self.shown_level[b];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn comp(settings: &[(&str, f32)]) -> MultibandComp {
        let mut c = MultibandComp::default();
        c.prepare(48_000, 512);
        for (id, v) in settings {
            c.set_param(index(id), *v);
        }
        c
    }

    /// Steady level (dB, RMS) of a sine through the compressor, relative to its input.
    fn gain(c: &mut MultibandComp, freq: f32, amplitude: f32) -> f32 {
        let (mut sum, mut count) = (0.0f32, 0usize);
        let mut n = 0usize;
        for block in 0..300 {
            let mut l: Vec<f32> = (0..256)
                .map(|i| {
                    amplitude * (std::f32::consts::TAU * freq * (n + i) as f32 / 48_000.0).sin()
                })
                .collect();
            let mut r = l.clone();
            n += 256;
            c.process(&mut l, &mut r);
            if block > 200 {
                sum += l.iter().map(|s| s * s).sum::<f32>();
                count += l.len();
            }
        }
        gain_to_db((2.0 * sum / count as f32).sqrt() / amplitude)
    }

    #[test]
    fn the_bands_sum_back_flat() {
        for count in 0..3 {
            let mut c = comp(&[("count", count as f32)]);
            for b in 1..=4 {
                c.set_param(index(&format!("b{b}_ratio")), 1.0);
            }
            for freq in [40.0, 150.0, 700.0, 2_500.0, 6_000.0, 12_000.0] {
                let g = gain(&mut c, freq, 0.1);
                assert!(g.abs() < 0.2, "{count} bands, {freq} Hz: {g} dB");
            }
        }
    }

    #[test]
    fn a_loud_band_is_compressed_and_the_others_left_alone() {
        let mut c = comp(&[
            ("b1_thresh", -30.0),
            ("b1_ratio", 4.0),
            ("b3_ratio", 1.0),
            ("b2_ratio", 1.0),
        ]);
        let low = gain(&mut c, 60.0, 0.5);
        assert!(low < -10.0, "{low}");
        let high = gain(&mut c, 8_000.0, 0.5);
        assert!(high.abs() < 0.3, "{high}");
    }

    #[test]
    fn a_quiet_band_is_lifted() {
        let mut c = comp(&[("b2_below", -20.0), ("b2_up", 4.0), ("b2_ratio", 1.0)]);
        let lifted = gain(&mut c, 800.0, 0.01);
        assert!(lifted > 10.0, "{lifted}");
        // Below the floor, nothing is lifted.
        let silent = gain(&mut c, 800.0, 0.00005);
        assert!(silent < 1.0, "{silent}");
    }

    #[test]
    fn solo_plays_one_band_and_mix_undoes_the_compression() {
        let mut c = comp(&[("solo", 3.0)]);
        assert!(gain(&mut c, 60.0, 0.1) < -30.0);
        assert!(gain(&mut c, 8_000.0, 0.1).abs() < 0.5);
        let mut c = comp(&[("b1_thresh", -40.0), ("b1_ratio", 10.0), ("mix", 0.0)]);
        assert!(gain(&mut c, 60.0, 0.5).abs() < 0.2);
    }

    #[test]
    fn the_meters_show_each_band_working() {
        let mut c = comp(&[("b1_thresh", -30.0), ("b1_ratio", 4.0)]);
        gain(&mut c, 60.0, 0.5);
        let mut meters = [0.0; METERS];
        c.meters(&mut meters);
        assert!(meters[GAINS] < -10.0, "{}", meters[GAINS]);
        assert!(meters[LEVELS] > -12.0, "{}", meters[LEVELS]);
        assert_eq!(meters[GAINS + 3], 0.0);
    }

    #[test]
    fn the_gain_curves() {
        assert_eq!(downward(-40.0, -20.0, 4.0), 0.0);
        assert!((downward(0.0, -20.0, 4.0) + 15.0).abs() < 1e-4);
        assert_eq!(upward(-10.0, -30.0, 4.0), 0.0);
        assert!((upward(-50.0, -30.0, 2.0) - 10.0).abs() < 1e-4);
        assert!(upward(-90.0, -30.0, 4.0) == 0.0);
    }
}
