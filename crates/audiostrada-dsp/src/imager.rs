//! Stereo imager: three bands split by Linkwitz-Riley crossovers, each with its own width
//! (mid/side), mono lows below a frequency, and a correlation meter and goniometer points
//! for the editor.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, ParamInfo, db_to_gain, param};

/// Points of the goniometer the editor draws (mid, side pairs).
pub const SCOPE_POINTS: usize = 64;

const METERS: [&str; 2 + SCOPE_POINTS * 2] = {
    let mut m = [""; 2 + SCOPE_POINTS * 2];
    m[0] = "Correlation";
    m[1] = "Width";
    m
};

pub const INFO: EffectInfo = EffectInfo {
    id: "imager",
    name: "Stereo Imager",
    params: &PARAMS,
    meters: &METERS,
    instrument: false,
    files: &[],
    category: "Stereo",
    description: "Width per band, mono lows and a goniometer: wide mixes that stay solid.",
};

const PARAMS: [ParamInfo; 7] = [
    param("low_width", "Low width", 0.0, 200.0, 100.0, "%"),
    param("mid_width", "Mid width", 0.0, 200.0, 110.0, "%"),
    param("high_width", "High width", 0.0, 200.0, 130.0, "%"),
    param("low_split", "Low split", 60.0, 1000.0, 250.0, "Hz"),
    param("high_split", "High split", 1000.0, 12000.0, 4000.0, "Hz"),
    param("mono_bass", "Mono below", 20.0, 500.0, 120.0, "Hz"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

/// A fourth-order Linkwitz-Riley filter: two Butterworth biquads in series.
#[derive(Debug, Default, Clone, Copy)]
struct Lr4 {
    stages: [Biquad; 2],
}

impl Lr4 {
    #[inline]
    fn process(&mut self, c: &Coefficients, x: f32) -> f32 {
        let y = self.stages[0].process(c, f64::from(x));
        self.stages[1].process(c, y) as f32
    }
}

#[derive(Debug)]
pub struct Imager {
    rate: f32,
    values: [f32; 7],
    coefficients: [Coefficients; 5],
    /// Per channel: low LP, low HP, high LP, high HP; then the side high-pass.
    filters: [[Lr4; 4]; 2],
    side_hp: [Biquad; 2],
    sums: [f32; 3],
    scope: [(f32, f32); SCOPE_POINTS],
    scope_at: usize,
    decimate: u32,
}

impl Default for Imager {
    fn default() -> Self {
        let mut imager = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            coefficients: [Coefficients::IDENTITY; 5],
            filters: [[Lr4::default(); 4]; 2],
            side_hp: [Biquad::default(); 2],
            sums: [0.0; 3],
            scope: [(0.0, 0.0); SCOPE_POINTS],
            scope_at: 0,
            decimate: 0,
        };
        imager.update();
        imager
    }
}

impl Imager {
    fn update(&mut self) {
        let rate = f64::from(self.rate);
        let q = std::f64::consts::FRAC_1_SQRT_2;
        let low = f64::from(self.values[3].min(self.values[4] * 0.8));
        let high = f64::from(self.values[4]);
        self.coefficients = [
            Coefficients::design(FilterKind::LowPass, low, q, 0.0, rate),
            Coefficients::design(FilterKind::HighPass, low, q, 0.0, rate),
            Coefficients::design(FilterKind::LowPass, high, q, 0.0, rate),
            Coefficients::design(FilterKind::HighPass, high, q, 0.0, rate),
            Coefficients::design(
                FilterKind::HighPass,
                f64::from(self.values[5]),
                q,
                0.0,
                rate,
            ),
        ];
    }
}

impl Effect for Imager {
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
        let widths = [
            self.values[0] / 100.0,
            self.values[1] / 100.0,
            self.values[2] / 100.0,
        ];
        let mono_bass = self.values[5] > 21.0;
        let gain = db_to_gain(self.values[6]);
        let c = self.coefficients;
        let decay = 0.9995f32;
        let every = (self.rate / 3000.0).max(1.0) as u32;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut bands = [[0.0f32; 3]; 2];
            for (ch, x) in [*l, *r].into_iter().enumerate() {
                let f = &mut self.filters[ch];
                let low = f[0].process(&c[0], x);
                let rest = f[1].process(&c[1], x);
                let mid = f[2].process(&c[2], rest);
                let high = f[3].process(&c[3], rest);
                bands[ch] = [low, mid, high];
            }
            let (mut mid, mut side) = (0.0f32, 0.0f32);
            for b in 0..3 {
                let m = 0.5 * (bands[0][b] + bands[1][b]);
                let s = 0.5 * (bands[0][b] - bands[1][b]);
                mid += m;
                side += s * widths[b];
            }
            if mono_bass {
                side = self.side_hp[0].process(&c[4], f64::from(side)) as f32;
                side = self.side_hp[1].process(&c[4], f64::from(side)) as f32;
            }
            let (out_l, out_r) = ((mid + side) * gain, (mid - side) * gain);
            *l = out_l;
            *r = out_r;
            self.sums[0] = self.sums[0] * decay + out_l * out_r;
            self.sums[1] = self.sums[1] * decay + out_l * out_l;
            self.sums[2] = self.sums[2] * decay + out_r * out_r;
            self.decimate += 1;
            if self.decimate >= every {
                self.decimate = 0;
                self.scope[self.scope_at] = (0.5 * (out_l + out_r), 0.5 * (out_l - out_r));
                self.scope_at = (self.scope_at + 1) % SCOPE_POINTS;
            }
        }
    }

    fn reset(&mut self) {
        self.filters = [[Lr4::default(); 4]; 2];
        self.side_hp = [Biquad::default(); 2];
        self.sums = [0.0; 3];
    }

    fn meters(&self, out: &mut [f32]) {
        let energy = (self.sums[1] * self.sums[2]).sqrt();
        let correlation = if energy > 1e-9 {
            (self.sums[0] / energy).clamp(-1.0, 1.0)
        } else {
            1.0
        };
        let width = 1.0 - correlation;
        for (i, o) in out.iter_mut().enumerate() {
            *o = match i {
                0 => correlation,
                1 => width,
                _ => {
                    let k = i - 2;
                    let (m, s) = self.scope[(self.scope_at + k / 2) % SCOPE_POINTS];
                    if k % 2 == 0 { m } else { s }
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(imager: &mut Imager, f: f32, phase: f32) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
            .map(|i| {
                let t = i as f32 * f * std::f32::consts::TAU / 48_000.0;
                (0.5 * t.sin(), 0.5 * (t + phase).sin())
            })
            .unzip();
        imager.process(&mut l, &mut r);
        (l, r)
    }

    #[test]
    fn mono_stays_mono_and_bass_folds_to_mono() {
        let mut imager = Imager::default();
        imager.prepare(48_000, 512);
        let (l, r) = run(&mut imager, 1000.0, 0.0);
        assert!(
            l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-4),
            "mono in, mono out"
        );
        let mut imager = Imager::default();
        imager.prepare(48_000, 512);
        let (l, r) = run(&mut imager, 50.0, 1.5);
        let side: f32 = l[24_000..]
            .iter()
            .zip(&r[24_000..])
            .map(|(a, b)| (a - b).abs())
            .sum();
        let mid: f32 = l[24_000..]
            .iter()
            .zip(&r[24_000..])
            .map(|(a, b)| (a + b).abs())
            .sum();
        assert!(side < mid * 0.2, "lows folded to mono: {side} vs {mid}");
        let mut meters = [0.0; 2 + SCOPE_POINTS * 2];
        imager.meters(&mut meters);
        assert!(meters[0] > 0.5, "correlated: {}", meters[0]);
    }

    #[test]
    fn zero_width_collapses_and_wide_widens() {
        let mut narrow = Imager::default();
        for i in 0..3 {
            narrow.set_param(i, 0.0);
        }
        narrow.set_param(5, 20.0);
        narrow.prepare(48_000, 512);
        let (l, r) = run(&mut narrow, 2000.0, 1.0);
        let side: f32 = l[2400..]
            .iter()
            .zip(&r[2400..])
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(side < 1.0, "{side}");
        let mut wide = Imager::default();
        wide.set_param(1, 200.0);
        wide.prepare(48_000, 512);
        let (l, r) = run(&mut wide, 2000.0, 1.0);
        let wider: f32 = l[2400..]
            .iter()
            .zip(&r[2400..])
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(wider > 1000.0, "{wider}");
    }
}
