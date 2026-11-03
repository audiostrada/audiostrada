//! Second-order filters (RBJ audio EQ cookbook), transposed direct form II in f64.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    Notch,
    /// Unity gain at the center.
    BandPass,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients {
    b: [f64; 3],
    a: [f64; 2],
}

impl Default for Coefficients {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Coefficients {
    pub const IDENTITY: Coefficients = Coefficients {
        b: [1.0, 0.0, 0.0],
        a: [0.0, 0.0],
    };

    pub fn design(kind: FilterKind, freq: f64, q: f64, gain_db: f64, rate: f64) -> Self {
        let freq = freq.clamp(1.0, rate * 0.49);
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.05));
        let sa = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = match kind {
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + sa),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - sa),
                (a + 1.0) + (a - 1.0) * cos + sa,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - sa,
            ),
            FilterKind::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + sa),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sa),
                (a + 1.0) - (a - 1.0) * cos + sa,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sa,
            ),
            FilterKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            FilterKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        Self {
            b: [b0 / a0, b1 / a0, b2 / a0],
            a: [a1 / a0, a2 / a0],
        }
    }

    /// Bell, shelf or low pass whose response follows the analog prototype up to Nyquist
    /// (Vicanek, "Matched Second Order Digital Filters"): poles placed by impulse invariance,
    /// zeros fitted to the analog magnitude. Other kinds fall back to [`Self::design`].
    pub fn matched(kind: FilterKind, freq: f64, q: f64, gain_db: f64, rate: f64) -> Self {
        let Some((num, den)) = analog(kind, q.max(0.05), gain_db) else {
            return Self::design(kind, freq, q, gain_db, rate);
        };
        let w0 = 2.0 * PI * freq.clamp(1.0, rate * 0.499) / rate;
        let wp = w0 * (den[2] / den[0]).sqrt();
        let zeta = den[1] / (2.0 * (den[0] * den[2]).sqrt());
        let r = (-zeta * wp).exp();
        let a1 = if zeta <= 1.0 {
            -2.0 * r * ((1.0 - zeta * zeta).sqrt() * wp).min(PI).cos()
        } else {
            -2.0 * r * ((zeta * zeta - 1.0).sqrt() * wp).cosh()
        };
        let a2 = r * r;
        let big_a = [(1.0 + a1 + a2).powi(2), (1.0 - a1 + a2).powi(2), -4.0 * a2];
        let phi = |w: f64| {
            let s = (w / 2.0).sin().powi(2);
            [1.0 - s, s, 4.0 * (1.0 - s) * s]
        };
        let dot = |x: &[f64; 3], p: &[f64; 3]| x[0] * p[0] + x[1] * p[1] + x[2] * p[2];
        let target = |w: f64| analog_power(&num, &den, w / w0);
        let b0 = target(0.0) * big_a[0];
        let b1 = target(PI) * big_a[1];
        let big_b = if kind == FilterKind::Peak {
            // Matched in level and slope at the center.
            let g2 = target(w0);
            let p = phi(w0);
            let r1 = dot(&big_a, &p) * g2;
            let r2 = (-big_a[0] + big_a[1] + 4.0 * (p[0] - p[1]) * big_a[2]) * g2;
            let b2 = (r1 - r2 * p[1] - b0) / (4.0 * p[1] * p[1]);
            [b0, r2 + b0 + 4.0 * (p[1] - p[0]) * b2, b2]
        } else {
            // Least squares on the relative error over the band's neighbourhood.
            let (mut top, mut bottom) = (0.0, 0.0);
            for i in 0..24 {
                let w = (w0 / 16.0 * 256f64.powf(f64::from(i) / 23.0)).min(PI * 0.98);
                let p = phi(w);
                let t = target(w) * dot(&big_a, &p);
                let weight = 1.0 / (t * t).max(1e-24);
                let rest = t - b0 * p[0] - b1 * p[1];
                top += weight * p[2] * rest;
                bottom += weight * p[2] * p[2];
            }
            [b0, b1, top / bottom.max(1e-30)]
        };
        Self::from_power(big_b, a1, a2)
    }

    /// Numerator from the squared-magnitude terms `B0`, `B1`, `B2` of the matched design.
    fn from_power(big_b: [f64; 3], a1: f64, a2: f64) -> Self {
        let (s0, s1) = (big_b[0].max(0.0).sqrt(), big_b[1].max(0.0).sqrt());
        let w = 0.5 * (s0 + s1);
        let b0 = 0.5 * (w + (w * w + big_b[2]).max(0.0).sqrt());
        let b1 = 0.5 * (s0 - s1);
        let b2 = if b0 > 1e-30 {
            -big_b[2] / (4.0 * b0)
        } else {
            0.0
        };
        Self {
            b: [b0, b1, b2],
            a: [a1, a2],
        }
    }

    /// First-order low-pass (or high-pass) at `freq`: 6 dB per octave.
    pub fn first_order(high: bool, freq: f64, rate: f64) -> Self {
        let k = (PI * freq.clamp(1.0, rate * 0.49) / rate).tan();
        let a1 = (k - 1.0) / (k + 1.0);
        let (b0, b1) = if high {
            (1.0 / (1.0 + k), -1.0 / (1.0 + k))
        } else {
            (k / (1.0 + k), k / (1.0 + k))
        };
        Self {
            b: [b0, b1, 0.0],
            a: [a1, 0.0],
        }
    }

    /// Gain of the filter at `freq`, in dB.
    pub fn magnitude_db(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * freq / rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num = (
            self.b[0] + self.b[1] * c1 + self.b[2] * c2,
            -(self.b[1] * s1 + self.b[2] * s2),
        );
        let den = (
            1.0 + self.a[0] * c1 + self.a[1] * c2,
            -(self.a[0] * s1 + self.a[1] * s2),
        );
        let mag =
            ((num.0 * num.0 + num.1 * num.1) / (den.0 * den.0 + den.1 * den.1).max(1e-30)).sqrt();
        20.0 * mag.max(1e-12).log10()
    }
}

/// Analog prototype (RBJ) of `kind` as numerator and denominator coefficients of
/// `s^2, s, 1`, with `s` normalized to the center frequency.
fn analog(kind: FilterKind, q: f64, gain_db: f64) -> Option<([f64; 3], [f64; 3])> {
    let a = 10f64.powf(gain_db / 40.0);
    let sa = a.sqrt();
    Some(match kind {
        FilterKind::Peak => ([1.0, a / q, 1.0], [1.0, 1.0 / (a * q), 1.0]),
        FilterKind::LowShelf => ([a, a * sa / q, a * a], [a, sa / q, 1.0]),
        FilterKind::HighShelf => ([a * a, a * sa / q, a], [1.0, sa / q, a]),
        FilterKind::LowPass => ([0.0, 0.0, 1.0], [1.0, 1.0 / q, 1.0]),
        _ => return None,
    })
}

/// Squared magnitude of an analog prototype at the normalized frequency `w`.
fn analog_power(num: &[f64; 3], den: &[f64; 3], w: f64) -> f64 {
    let power = |c: &[f64; 3]| (c[2] - c[0] * w * w).powi(2) + (c[1] * w).powi(2);
    power(num) / power(den).max(1e-300)
}

/// Filter state for one channel.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    pub fn process(&mut self, c: &Coefficients, x: f64) -> f64 {
        let y = c.b[0] * x + self.z[0];
        self.z[0] = c.b[1] * x - c.a[0] * y + self.z[1];
        self.z[1] = c.b[2] * x - c.a[1] * y;
        y
    }

    pub fn reset(&mut self) {
        self.z = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_match_the_design() {
        let rate = 48_000.0;
        let peak = Coefficients::design(FilterKind::Peak, 1000.0, 1.0, 6.0, rate);
        assert!((peak.magnitude_db(1000.0, rate) - 6.0).abs() < 0.01);
        assert!(peak.magnitude_db(50.0, rate).abs() < 0.1);
        let low = Coefficients::design(FilterKind::LowShelf, 200.0, 0.707, -9.0, rate);
        assert!((low.magnitude_db(20.0, rate) + 9.0).abs() < 0.2);
        assert!(low.magnitude_db(10_000.0, rate).abs() < 0.1);
        let hp = Coefficients::design(FilterKind::HighPass, 100.0, 0.707, 0.0, rate);
        assert!((hp.magnitude_db(100.0, rate) + 3.01).abs() < 0.05);
        assert!(hp.magnitude_db(25.0, rate) < -20.0);
        // Running a sine through the peak gives the same gain as the formula.
        let mut state = Biquad::default();
        let mut peak_out: f64 = 0.0;
        for n in 0..48_000 {
            let x = (2.0 * PI * 1000.0 * n as f64 / rate).sin();
            let y = state.process(&peak, x);
            if n > 24_000 {
                peak_out = peak_out.max(y.abs());
            }
        }
        assert!((20.0 * peak_out.log10() - 6.0).abs() < 0.05);
        let notch = Coefficients::design(FilterKind::Notch, 1000.0, 2.0, 0.0, rate);
        assert!(notch.magnitude_db(1000.0, rate) < -60.0);
        let band = Coefficients::design(FilterKind::BandPass, 1000.0, 2.0, 0.0, rate);
        assert!(band.magnitude_db(1000.0, rate).abs() < 0.01);
        let first = Coefficients::first_order(false, 1000.0, rate);
        assert!((first.magnitude_db(1000.0, rate) + 3.01).abs() < 0.05);
        assert!(
            (first.magnitude_db(8000.0, rate) + 18.0).abs() < 1.5,
            "6 dB per octave"
        );
    }

    #[test]
    fn matched_filters_follow_the_analog_curve_up_to_nyquist() {
        let rate = 48_000.0;
        let cases = [
            (FilterKind::Peak, 1000.0, 1.0, 9.0),
            (FilterKind::Peak, 15_000.0, 1.0, 12.0),
            (FilterKind::Peak, 18_000.0, 4.0, -15.0),
            (FilterKind::Peak, 60.0, 0.5, -6.0),
            (FilterKind::LowShelf, 100.0, 0.707, 9.0),
            (FilterKind::LowShelf, 8000.0, 0.707, -12.0),
            (FilterKind::HighShelf, 10_000.0, 0.707, 12.0),
            (FilterKind::HighShelf, 16_000.0, 1.5, -18.0),
            (FilterKind::HighShelf, 2000.0, 0.5, 6.0),
            (FilterKind::LowPass, 15_000.0, 2.0, 0.0),
            (FilterKind::LowPass, 500.0, 0.707, 0.0),
        ];
        for (kind, f, q, g) in cases {
            let (num, den) = analog(kind, q, g).unwrap();
            let matched = Coefficients::matched(kind, f, q, g, rate);
            let classic = Coefficients::design(kind, f, q, g, rate);
            let (mut worst, mut worst_classic) = (0.0f64, 0.0f64);
            for i in 0..200 {
                let hz = (20.0 * 1100f64.powf(f64::from(i) / 199.0)).min(rate * 0.49);
                let want = 10.0 * analog_power(&num, &den, hz / f).log10();
                if want < -40.0 {
                    continue;
                }
                worst = worst.max((matched.magnitude_db(hz, rate) - want).abs());
                worst_classic = worst_classic.max((classic.magnitude_db(hz, rate) - want).abs());
            }
            assert!(worst < 1.6, "{kind:?} {f} Hz: {worst} dB off");
            assert!(worst <= worst_classic + 0.01, "{kind:?} {f} Hz");
        }
    }
}
