//! De-esser: listens to a band around the sibilance, and when it is louder than the threshold
//! turns down the highs (split mode) or the whole signal (wide mode). Listen plays the band.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, gain_to_db, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "deesser",
    name: "De-esser",
    params: &PARAMS,
    meters: &["Gain reduction", "Sibilance"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Tames harsh S and T sounds on vocals, on the highs only or the whole voice.",
};

const MODES: [&str; 2] = ["Split", "Wide"];

const PARAMS: [ParamInfo; 5] = [
    param("frequency", "Frequency", 2000.0, 12000.0, 6500.0, "Hz"),
    param("threshold", "Threshold", -60.0, 0.0, -30.0, "dB"),
    param("range", "Range", 0.0, 24.0, 10.0, "dB"),
    choice("mode", "Mode", &MODES, 0.0),
    switch("listen", "Listen", 0.0),
];

#[derive(Debug)]
pub struct DeEsser {
    rate: f32,
    values: [f32; 5],
    band: Coefficients,
    /// Linkwitz-Riley crossover under the sibilance: low-pass and high-pass.
    split: [Coefficients; 2],
    detect: Biquad,
    /// Per channel: two low-pass and two high-pass stages.
    crossover: [[Biquad; 4]; 2],
    envelope: f32,
    gain_db: f32,
    meters: [f32; 2],
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
            gain_db: 0.0,
            meters: [0.0, -120.0],
        };
        d.update();
        d
    }
}

impl DeEsser {
    fn update(&mut self) {
        let rate = f64::from(self.rate);
        let f = f64::from(self.values[0]);
        // A slightly resonant high-pass at the frequency is enough to catch sibilance.
        self.band = Coefficients::design(FilterKind::HighPass, f, 1.2, 0.0, rate);
        let q = std::f64::consts::FRAC_1_SQRT_2;
        self.split = [
            Coefficients::design(FilterKind::LowPass, f * 0.7, q, 0.0, rate),
            Coefficients::design(FilterKind::HighPass, f * 0.7, q, 0.0, rate),
        ];
    }
}

impl Effect for DeEsser {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
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
        let [_, threshold, range, mode, listen] = self.values;
        let split = mode < 0.5;
        let listen = listen >= 0.5;
        let attack = crate::time_coefficient(0.5, self.rate);
        let release = crate::time_coefficient(60.0, self.rate);
        let (mut deepest, mut loudest) = (0.0f32, -120.0f32);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mono = 0.5 * (*l + *r);
            let band = self.detect.process(&self.band, f64::from(mono)) as f32;
            let level = band.abs();
            let c = if level > self.envelope {
                attack
            } else {
                release
            };
            self.envelope = level + c * (self.envelope - level);
            let db = gain_to_db(self.envelope);
            loudest = loudest.max(db);
            // About 4:1 above the threshold, never more than the range.
            let target = -((db - threshold).max(0.0) * 0.75).min(range);
            self.gain_db = target + 0.2 * (self.gain_db - target);
            deepest = deepest.min(self.gain_db);
            let g = db_to_gain(self.gain_db);
            if listen {
                *l = band;
                *r = band;
                continue;
            }
            if split {
                for (ch, s) in [&mut *l, &mut *r].into_iter().enumerate() {
                    let x = f64::from(*s);
                    let f = &mut self.crossover[ch];
                    let low = f[0].process(&self.split[0], x);
                    let low = f[1].process(&self.split[0], low);
                    let high = f[2].process(&self.split[1], x);
                    let high = f[3].process(&self.split[1], high);
                    *s = (low + high * f64::from(g)) as f32;
                }
            } else {
                *l *= g;
                *r *= g;
            }
        }
        self.meters = [deepest, loudest];
    }

    fn reset(&mut self) {
        self.detect = Biquad::default();
        self.crossover = [[Biquad::default(); 4]; 2];
        self.envelope = 0.0;
        self.gain_db = 0.0;
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

    fn tone(f: f32) -> Vec<f32> {
        (0..24_000)
            .map(|i| 0.5 * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin())
            .collect()
    }

    fn peak_after(d: &mut DeEsser, f: f32) -> f32 {
        let (mut l, mut r) = (tone(f), tone(f));
        d.process(&mut l, &mut r);
        l[12_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn turns_down_sibilance_and_leaves_the_voice() {
        for mode in [0.0, 1.0] {
            let mut d = DeEsser::default();
            d.set_param(3, mode);
            d.prepare(48_000, 512);
            let ess = peak_after(&mut d, 7000.0);
            assert!(ess < 0.5 * db_to_gain(-4.0), "mode {mode}: {ess}");
            let mut d = DeEsser::default();
            d.set_param(3, mode);
            d.prepare(48_000, 512);
            let voice = peak_after(&mut d, 300.0);
            assert!((voice - 0.5).abs() < 0.03, "mode {mode}: {voice}");
        }
    }
}
