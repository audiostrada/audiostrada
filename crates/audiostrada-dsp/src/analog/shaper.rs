//! A soft saturation, asymmetric for even harmonics, of slope 1 at rest, with first-order
//! antiderivative antialiasing: the harmonics it makes above half the sample rate barely fold
//! back, for the price of half a sample of delay.

use super::Stage;

/// `ln(cosh(u))`, without overflow.
fn log_cosh(u: f64) -> f64 {
    let a = u.abs();
    a + (-2.0 * a).exp().ln_1p() - std::f64::consts::LN_2
}

#[derive(Clone, Debug)]
pub struct Shaper {
    g: f64,
    bias: f64,
    /// `tanh(g bias)`, and the slope at rest to divide by.
    offset: f64,
    slope: f64,
    x1: f64,
    f1: f64,
}

impl Shaper {
    /// `gain` drives the curve (0: a wire), `bias` moves its center for even harmonics.
    pub fn new(gain: f32, bias: f32) -> Self {
        let g = f64::from(gain.max(0.0));
        let bias = f64::from(bias);
        let offset = (g * bias).tanh();
        let mut shaper = Self {
            g,
            bias,
            offset,
            slope: g * (1.0 - offset * offset),
            x1: 0.0,
            f1: 0.0,
        };
        shaper.f1 = shaper.antiderivative(0.0);
        shaper
    }

    /// The curve itself, without antialiasing.
    pub fn curve(&self, x: f64) -> f64 {
        if self.g < 1e-6 {
            return x;
        }
        ((self.g * (x + self.bias)).tanh() - self.offset) / self.slope
    }

    fn antiderivative(&self, x: f64) -> f64 {
        if self.g < 1e-6 {
            return 0.5 * x * x;
        }
        (log_cosh(self.g * (x + self.bias)) / self.g - self.offset * x) / self.slope
    }

    /// The curve at `x` moved by `shift` (a drifting bias), antialiased.
    pub fn shifted(&mut self, x: f32, shift: f32) -> f32 {
        let x = f64::from(x) + f64::from(shift);
        let f = self.antiderivative(x);
        let dx = x - self.x1;
        let y = if dx.abs() > 1e-5 {
            (f - self.f1) / dx
        } else {
            self.curve(0.5 * (x + self.x1))
        };
        self.x1 = x;
        self.f1 = f;
        y as f32
    }
}

impl Stage for Shaper {
    fn process(&mut self, x: f32) -> f32 {
        self.shifted(x, 0.0)
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.f1 = self.antiderivative(0.0);
    }

    fn latency(&self) -> f32 {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::amplitude;

    #[test]
    fn small_signals_pass_and_the_bias_makes_even_harmonics() {
        let mut shaper = Shaper::new(3.0, 0.2);
        let tone: Vec<f32> = (0..4_800)
            .map(|i| 0.01 * (i as f32 * std::f32::consts::TAU * 100.0 / 48_000.0).sin())
            .collect();
        let out: Vec<f32> = tone.iter().map(|&x| shaper.process(x)).collect();
        let gain = amplitude(&out, 10.0) / 0.01;
        assert!((gain - 1.0).abs() < 1e-3, "{gain}");
        let symmetric = Shaper::new(3.0, 0.0);
        assert!((symmetric.curve(0.3) + symmetric.curve(-0.3)).abs() < 1e-12);
        let biased = Shaper::new(3.0, 0.2);
        assert!((biased.curve(0.3) + biased.curve(-0.3)).abs() > 1e-3);
        assert_eq!(Shaper::new(0.0, 0.2).curve(0.7), 0.7);
    }

    #[test]
    fn antialiasing_keeps_the_folded_harmonics_down() {
        // 7 kHz driven hard: the odd harmonics above 24 kHz fold back to 3, 11, 17 kHz…
        let n = 48_000;
        let tone: Vec<f32> = (0..n)
            .map(|i| 0.8 * (i as f32 * std::f32::consts::TAU * 7_000.0 / 48_000.0).sin())
            .collect();
        let folded = |out: &[f32]| {
            // 5th harmonic (35 kHz) lands on 13 kHz, the 7th (49 kHz) on 1 kHz.
            let x = &out[n / 2..n / 2 + 4_800];
            (amplitude(x, 1_300.0).powi(2) + amplitude(x, 100.0).powi(2)).sqrt()
        };
        let shaper = Shaper::new(4.0, 0.0);
        let naive: Vec<f32> = tone
            .iter()
            .map(|&x| shaper.curve(f64::from(x)) as f32)
            .collect();
        let mut smooth = Shaper::new(4.0, 0.0);
        let antialiased: Vec<f32> = tone.iter().map(|&x| smooth.process(x)).collect();
        let gain = 20.0 * (folded(&naive) / folded(&antialiased)).log10();
        assert!(gain > 10.0, "{gain} dB less folding");
    }
}
