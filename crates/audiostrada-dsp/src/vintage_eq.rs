//! Vintage program equalizer in the manner of the classic passive tube units: a low boost and
//! a low cut on the same frequency (together they make the famous bump with a dip above),
//! a high boost with a bandwidth, a high cut shelf, and the warmth of the tube stage.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "vintage_eq",
    name: "Strada-Tube EQ",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "EQ & Filter",
    description: "Passive tube program EQ: the low boost and cut trick, silky highs, warmth.",
};

const LOW_FREQS: [&str; 4] = ["20 Hz", "30 Hz", "60 Hz", "100 Hz"];
const LOW_HZ: [f32; 4] = [20.0, 30.0, 60.0, 100.0];
const HIGH_FREQS: [&str; 7] = [
    "3 kHz", "4 kHz", "5 kHz", "8 kHz", "10 kHz", "12 kHz", "16 kHz",
];
const HIGH_HZ: [f32; 7] = [3000.0, 4000.0, 5000.0, 8000.0, 10_000.0, 12_000.0, 16_000.0];
const CUT_FREQS: [&str; 3] = ["5 kHz", "10 kHz", "20 kHz"];
const CUT_HZ: [f32; 3] = [5000.0, 10_000.0, 20_000.0];

const PARAMS: [ParamInfo; 10] = [
    choice("low_freq", "Low frequency", &LOW_FREQS, 2.0),
    param("low_boost", "Low boost", 0.0, 10.0, 3.0, ""),
    param("low_cut", "Low atten", 0.0, 10.0, 2.0, ""),
    choice("high_freq", "High frequency", &HIGH_FREQS, 4.0),
    param("high_boost", "High boost", 0.0, 10.0, 2.0, ""),
    param("bandwidth", "Bandwidth", 0.0, 10.0, 5.0, ""),
    choice("cut_freq", "Atten select", &CUT_FREQS, 2.0),
    param("high_cut", "High atten", 0.0, 10.0, 0.0, ""),
    param("warmth", "Tube warmth", 0.0, 100.0, 25.0, "%"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

#[derive(Debug)]
pub struct VintageEq {
    rate: f32,
    values: [f32; 10],
    coefficients: [Coefficients; 4],
    filters: [[Biquad; 4]; 2],
}

impl Default for VintageEq {
    fn default() -> Self {
        let mut eq = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            coefficients: [Coefficients::IDENTITY; 4],
            filters: [[Biquad::default(); 4]; 2],
        };
        eq.update();
        eq
    }
}

impl VintageEq {
    /// The four stages: low boost shelf, low dip a little higher, high peak, high cut.
    fn update(&mut self) {
        let rate = f64::from(self.rate);
        let v = self.values;
        let index = |x: f32, n: usize| (x.round().max(0.0) as usize).min(n - 1);
        let low = f64::from(LOW_HZ[index(v[0], 4)]);
        let high = f64::from(HIGH_HZ[index(v[3], 7)]).min(rate * 0.45);
        let cut = f64::from(CUT_HZ[index(v[6], 3)]).min(rate * 0.45);
        // Knob 0 to 10 gives up to 13.5 dB of resonant low shelf; the attenuation is a broad
        // dip a little above it, so boosting and cutting together makes a bump and a dip.
        self.coefficients = [
            Coefficients::design(FilterKind::LowShelf, low, 0.9, f64::from(v[1]) * 1.35, rate),
            Coefficients::design(
                FilterKind::Peak,
                low * 2.5,
                0.5,
                -f64::from(v[2]) * 1.1,
                rate,
            ),
            Coefficients::design(
                FilterKind::Peak,
                high,
                0.3 + f64::from(10.0 - v[5]) * 0.25,
                f64::from(v[4]) * 1.6,
                rate,
            ),
            Coefficients::design(
                FilterKind::HighShelf,
                cut,
                0.6,
                -f64::from(v[7]) * 1.6,
                rate,
            ),
        ];
    }
}

/// Gain of the equalizer at `freq` for parameter `values` (in [`INFO`] order), in dB.
pub fn response_db(values: &[f32], rate: f32, freq: f32) -> f32 {
    let mut eq = VintageEq {
        rate,
        ..VintageEq::default()
    };
    for (slot, v) in eq.values.iter_mut().zip(values) {
        *slot = *v;
    }
    eq.update();
    eq.coefficients
        .iter()
        .map(|c| c.magnitude_db(f64::from(freq), f64::from(rate)) as f32)
        .sum()
}

impl Effect for VintageEq {
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
        let warmth = self.values[8] / 100.0;
        let output = db_to_gain(self.values[9]);
        let drive = 1.0 + warmth * 2.5;
        let c = self.coefficients;
        for (ch, buffer) in [left, right].into_iter().enumerate() {
            for s in buffer.iter_mut() {
                let mut x = f64::from(*s);
                for (filter, coefficients) in self.filters[ch].iter_mut().zip(&c) {
                    x = filter.process(coefficients, x);
                }
                let x = x as f32;
                // Tube stage: a touch of even harmonics, level-matched.
                let y = if warmth > 0.0 {
                    let bias = 0.1 * warmth;
                    ((x * drive + bias).tanh() - bias.tanh()) / drive
                } else {
                    x
                };
                *s = (x + (y - x) * warmth) * output;
            }
        }
    }

    fn reset(&mut self) {
        self.filters = [[Biquad::default(); 4]; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boost_and_cut_make_a_bump_with_a_dip_above() {
        let mut eq = VintageEq::default();
        eq.set_param(1, 8.0);
        eq.set_param(2, 6.0);
        eq.set_param(4, 0.0);
        eq.set_param(8, 0.0);
        eq.prepare(48_000, 256);
        let rate = 48_000.0;
        let response = |f: f64| -> f64 {
            eq.coefficients
                .iter()
                .map(|c| c.magnitude_db(f, rate))
                .sum()
        };
        assert!(response(40.0) > 3.0, "bump: {}", response(40.0));
        assert!(response(250.0) < response(40.0) - 3.0, "dip above");
        assert!(
            response(2000.0).abs() < 1.5,
            "mids untouched: {}",
            response(2000.0)
        );
        let (mut l, mut r) = (vec![0.5f32; 512], vec![-0.5f32; 512]);
        eq.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|s| s.is_finite()));
    }
}
