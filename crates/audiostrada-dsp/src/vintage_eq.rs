//! Program equalizer in the manner of the passive tube units: a low boost and a low
//! attenuation shelf on the same frequency (together they make the bump with a dip above),
//! a high boost with a bandwidth, a high attenuation shelf, and a tube stage run at four
//! times the rate. The curves keep their analog shape up to Nyquist and settings glide.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::drive::{Oversampler, Taps};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param, time_coefficient};

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

pub const LOW_FREQS: [&str; 4] = ["20 Hz", "30 Hz", "60 Hz", "100 Hz"];
const LOW_HZ: [f32; 4] = [20.0, 30.0, 60.0, 100.0];
pub const HIGH_FREQS: [&str; 7] = [
    "3 kHz", "4 kHz", "5 kHz", "8 kHz", "10 kHz", "12 kHz", "16 kHz",
];
const HIGH_HZ: [f32; 7] = [3000.0, 4000.0, 5000.0, 8000.0, 10_000.0, 12_000.0, 16_000.0];
pub const CUT_FREQS: [&str; 3] = ["5 kHz", "10 kHz", "20 kHz"];
const CUT_HZ: [f32; 3] = [5000.0, 10_000.0, 20_000.0];
/// Most boost or attenuation of each knob at 10, in dB, as on the hardware.
const LOW_BOOST_DB: f64 = 13.5;
const LOW_ATTEN_DB: f64 = 17.5;
const HIGH_BOOST_DB: f64 = 18.0;
const HIGH_ATTEN_DB: f64 = 16.0;
const COUNT: usize = 10;
const SUB_BLOCK: usize = 32;
const GLIDE_MS: f32 = 25.0;

const PARAMS: [ParamInfo; COUNT] = [
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

/// The four stages for `values` (in [`INFO`] order): low boost shelf, low attenuation shelf
/// a little higher and gentler, high peak, high attenuation shelf.
fn stages(values: &[f32], rate: f64) -> [Coefficients; 4] {
    let v = |i: usize| f64::from(values.get(i).copied().unwrap_or(PARAMS[i].default));
    let index = |i: usize, n: usize| (v(i).round().max(0.0) as usize).min(n - 1);
    let low = f64::from(LOW_HZ[index(0, 4)]);
    let high = f64::from(HIGH_HZ[index(3, 7)]);
    let cut = f64::from(CUT_HZ[index(6, 3)]);
    // Bandwidth 0 is sharp, 10 broad.
    let q = 0.3 + (10.0 - v(5)) * 0.25;
    [
        Coefficients::matched(
            FilterKind::LowShelf,
            low,
            1.0,
            v(1) * LOW_BOOST_DB / 10.0,
            rate,
        ),
        Coefficients::matched(
            FilterKind::LowShelf,
            low * 1.5,
            0.5,
            -v(2) * LOW_ATTEN_DB / 10.0,
            rate,
        ),
        Coefficients::matched(FilterKind::Peak, high, q, v(4) * HIGH_BOOST_DB / 10.0, rate),
        Coefficients::matched(
            FilterKind::HighShelf,
            cut,
            0.6,
            -v(7) * HIGH_ATTEN_DB / 10.0,
            rate,
        ),
    ]
}

/// Gain of the equalizer at `freq` for parameter `values` (in [`INFO`] order), in dB.
pub fn response_db(values: &[f32], rate: f32, freq: f32) -> f32 {
    stages(values, f64::from(rate))
        .iter()
        .map(|c| c.magnitude_db(f64::from(freq), f64::from(rate)) as f32)
        .sum()
}

#[derive(Debug)]
pub struct VintageEq {
    rate: f32,
    targets: [f32; COUNT],
    current: [f32; COUNT],
    settled: bool,
    coefficients: [Coefficients; 4],
    filters: [[Biquad; 4]; 2],
    tubes: [Oversampler; 2],
    /// DC blocker after the tube: last input and output of each channel.
    dc: [(f32, f32); 2],
}

impl Default for VintageEq {
    fn default() -> Self {
        let values = PARAMS.map(|p| p.default);
        Self {
            rate: 48_000.0,
            targets: values,
            current: values,
            settled: true,
            coefficients: stages(&values, 48_000.0),
            filters: [[Biquad::default(); 4]; 2],
            tubes: [Oversampler::new(Taps::Long), Oversampler::new(Taps::Long)],
            dc: [(0.0, 0.0); 2],
        }
    }
}

impl VintageEq {
    /// Moves the knobs towards their targets over `frames`; the switches jump.
    fn glide(&mut self, frames: usize) {
        let k = 1.0 - time_coefficient(GLIDE_MS, self.rate).powi(frames as i32);
        let mut settled = true;
        for ((c, &t), p) in self.current.iter_mut().zip(&self.targets).zip(&PARAMS) {
            let delta = t - *c;
            if !p.choices.is_empty() || delta.abs() <= (p.max - p.min) * 1e-4 {
                *c = t;
            } else {
                *c += delta * k;
                settled = false;
            }
        }
        self.settled = settled;
        self.coefficients = stages(&self.current, f64::from(self.rate));
    }
}

impl Effect for VintageEq {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.current = self.targets;
        self.settled = true;
        self.coefficients = stages(&self.current, f64::from(self.rate));
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.targets.get_mut(index) {
            *v = value;
            self.settled = false;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let dc = 1.0 - 25.0 / self.rate;
        let mut start = 0;
        while start < n {
            let end = (start + SUB_BLOCK).min(n);
            if !self.settled {
                self.glide(end - start);
            }
            let warmth = self.current[8] / 100.0;
            let output = db_to_gain(self.current[9]);
            let drive = 1.0 + warmth * 2.5;
            let bias = 0.1 * warmth;
            // A touch of even harmonics from the bias, level-matched.
            let tube = |u: f32| {
                let t = ((u * drive + bias).tanh() - bias.tanh()) / drive;
                u + (t - u) * warmth
            };
            let buffers = [&mut left[start..end], &mut right[start..end]];
            for (ch, buffer) in buffers.into_iter().enumerate() {
                let (filters, oversampler) = (&mut self.filters[ch], &mut self.tubes[ch]);
                for s in buffer.iter_mut() {
                    let x = filters
                        .iter_mut()
                        .zip(&self.coefficients)
                        .fold(f64::from(*s), |x, (f, c)| f.process(c, x));
                    let y = oversampler.run(x as f32, tube);
                    let (last_in, last_out) = self.dc[ch];
                    let out = y - last_in + dc * last_out;
                    self.dc[ch] = (y, out);
                    *s = out * output;
                }
            }
            start = end;
        }
    }

    fn reset(&mut self) {
        self.filters = [[Biquad::default(); 4]; 2];
        for tube in &mut self.tubes {
            tube.clear();
        }
        self.dc = [(0.0, 0.0); 2];
    }

    fn latency(&self) -> u32 {
        self.tubes[0].latency()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn values(changes: &[(&str, f32)]) -> [f32; COUNT] {
        let mut v = PARAMS.map(|p| p.default);
        for (id, x) in changes {
            v[index(id)] = *x;
        }
        v
    }

    #[test]
    fn boost_and_cut_make_a_bump_with_a_dip_above() {
        let v = values(&[("low_boost", 8.0), ("low_cut", 4.0), ("high_boost", 0.0)]);
        let response = |f: f32| response_db(&v, 48_000.0, f);
        assert!(response(30.0) > 3.0, "bump: {}", response(30.0));
        assert!(response(120.0) < -2.0, "dip above: {}", response(120.0));
        assert!(response(2000.0).abs() < 0.5, "mids: {}", response(2000.0));
    }

    #[test]
    fn the_knobs_reach_the_hardware_range() {
        let low = values(&[("low_boost", 0.0), ("low_cut", 10.0), ("high_boost", 0.0)]);
        let atten = response_db(&low, 48_000.0, 10.0);
        assert!((atten + 17.5).abs() < 1.0, "low atten {atten}");
        let high = values(&[
            ("low_boost", 0.0),
            ("low_cut", 0.0),
            ("high_freq", 6.0),
            ("high_boost", 10.0),
        ]);
        let boost = response_db(&high, 48_000.0, 16_000.0);
        assert!((boost - 18.0).abs() < 0.2, "16 kHz boost {boost}");
    }

    #[test]
    fn the_tube_is_delayed_by_its_latency_and_aliases_little() {
        let mut eq = VintageEq::default();
        for (id, v) in [
            ("low_boost", 0.0),
            ("low_cut", 0.0),
            ("high_boost", 0.0),
            ("warmth", 0.0),
        ] {
            eq.set_param(index(id), v);
        }
        eq.prepare(48_000, 256);
        let (mut l, mut r) = (vec![0.0f32; 64], vec![0.0f32; 64]);
        l[0] = 1.0;
        r[0] = 1.0;
        eq.process(&mut l, &mut r);
        let peak = (0..64).max_by(|&a, &b| l[a].abs().total_cmp(&l[b].abs()));
        assert!(matches!(peak, Some(23 | 24)), "{peak:?}");
        assert_eq!(eq.latency(), 24);

        // A loud 15 kHz tone fully warmed: its third harmonic would fold to 3 kHz.
        eq.set_param(index("warmth"), 100.0);
        eq.prepare(48_000, 4800);
        let tone = |i: usize| 0.8 * (std::f32::consts::TAU * 15_000.0 * i as f32 / 48_000.0).sin();
        let mut l: Vec<f32> = (0..9600).map(tone).collect();
        let mut r = l.clone();
        eq.process(&mut l, &mut r);
        let level = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in l[4800..].iter().enumerate() {
                let w = std::f32::consts::TAU * f * i as f32 / 48_000.0;
                re += s * w.cos();
                im += s * w.sin();
            }
            (re * re + im * im).sqrt() / 2400.0
        };
        let ratio = crate::gain_to_db(level(3000.0) / level(15_000.0));
        for f in [3000.0, 6000.0, 9000.0, 12_000.0, 18_000.0, 21_000.0] {
            println!("{f}: {}", crate::gain_to_db(level(f) / level(15_000.0)));
        }
        assert!(ratio < -50.0, "aliasing at {ratio} dB");
    }

    #[test]
    fn changes_glide() {
        let mut eq = VintageEq::default();
        eq.prepare(48_000, 256);
        eq.set_param(index("high_boost"), 10.0);
        let (mut l, mut r) = (vec![0.0f32; 32], vec![0.0f32; 32]);
        eq.process(&mut l, &mut r);
        let now = eq.current[index("high_boost")];
        assert!(now > 2.0 && now < 6.0, "{now}");
        for _ in 0..600 {
            eq.process(&mut l, &mut r);
        }
        assert!((eq.current[index("high_boost")] - 10.0).abs() < 0.01);
    }
}
