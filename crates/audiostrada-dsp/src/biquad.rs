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
}
