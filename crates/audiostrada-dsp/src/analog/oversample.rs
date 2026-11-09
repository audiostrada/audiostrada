//! Running a stage at twice the sample rate, between two halfband filters: what it makes
//! between the two Nyquist frequencies is filtered out instead of folding back.

use super::Stage;

/// Taps of the halfband filters: flat to 19 kHz, 70 dB down from 29 kHz at 48 kHz.
const TAPS: usize = 39;
const CENTER: usize = (TAPS - 1) / 2;

fn halfband() -> [f64; TAPS] {
    // A windowed sinc cut at a quarter of the high rate (Kaiser, beta 7).
    let bessel = |x: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..30 {
            term *= (x / (2.0 * k as f64)).powi(2);
            sum += term;
        }
        sum
    };
    let beta = 7.0;
    let mut h = [0.0; TAPS];
    for (n, tap) in h.iter_mut().enumerate() {
        let t = n as f64 - CENTER as f64;
        let sinc = if t == 0.0 {
            0.5
        } else {
            (std::f64::consts::PI * t / 2.0).sin() / (std::f64::consts::PI * t)
        };
        let r = t / CENTER as f64;
        *tap = sinc * bessel(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel(beta);
    }
    let sum: f64 = h.iter().sum();
    h.map(|v| v / sum)
}

/// A stage run at twice the rate it is given samples at.
#[derive(Debug)]
pub struct Oversampled<S> {
    inner: S,
    h: [f64; TAPS],
    /// Input at the low rate, and the stage's output at the high rate, newest first.
    low: [f64; TAPS],
    high: [f64; TAPS],
}

impl<S: Stage> Oversampled<S> {
    /// `make` builds the stage for the rate it runs at: twice `rate`.
    pub fn new(make: impl FnOnce(f64) -> S, rate: f64) -> Self {
        Self {
            inner: make(rate * 2.0),
            h: halfband(),
            low: [0.0; TAPS],
            high: [0.0; TAPS],
        }
    }

    pub fn inner(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S: Stage> Stage for Oversampled<S> {
    fn process(&mut self, x: f32) -> f32 {
        self.low.copy_within(0..TAPS - 1, 1);
        self.low[0] = f64::from(x);
        for phase in 0..2 {
            // The zero-stuffed input: only the taps of this phase meet a sample.
            let mut up = 0.0;
            let mut j = phase;
            while j < TAPS {
                up += self.h[j] * self.low[(j - phase) / 2];
                j += 2;
            }
            let y = self.inner.process((2.0 * up) as f32);
            self.high.copy_within(0..TAPS - 1, 1);
            self.high[0] = f64::from(y);
        }
        let down: f64 = self.h.iter().zip(&self.high).map(|(h, v)| h * v).sum();
        down as f32
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.low = [0.0; TAPS];
        self.high = [0.0; TAPS];
    }

    fn latency(&self) -> f32 {
        // Each filter delays by CENTER samples of the high rate.
        CENTER as f32 + self.inner.latency() / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::amplitude;

    struct Wire;
    impl Stage for Wire {
        fn process(&mut self, x: f32) -> f32 {
            x
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn a_wire_oversampled_is_flat_and_late_by_its_filters() {
        let mut stage = Oversampled::new(|_| Wire, 48_000.0);
        assert_eq!(stage.latency(), CENTER as f32);
        for freq in [100.0f32, 1_000.0, 10_000.0, 18_000.0] {
            stage.reset();
            let out: Vec<f32> = (0..9_600)
                .map(|i| (i as f32 * std::f32::consts::TAU * freq / 48_000.0).sin())
                .map(|x| stage.process(x))
                .collect();
            let gain = amplitude(&out[4_800..], f64::from(freq) / 10.0);
            assert!((gain - 1.0).abs() < 0.012, "{freq} Hz: {gain}");
        }
    }

    #[test]
    fn what_the_stage_makes_above_the_band_is_removed() {
        // A full-wave rectifier doubles 15 kHz to 30 kHz: at the low rate it would fold to 18.
        struct Square;
        impl Stage for Square {
            fn process(&mut self, x: f32) -> f32 {
                x * x
            }
            fn reset(&mut self) {}
        }
        let mut stage = Oversampled::new(|_| Square, 48_000.0);
        let out: Vec<f32> = (0..9_600)
            .map(|i| (i as f32 * std::f32::consts::TAU * 15_000.0 / 48_000.0).sin())
            .map(|x| stage.process(x))
            .collect();
        let folded = amplitude(&out[4_800..], 1_800.0);
        assert!(folded < 0.5 * 10f64.powf(-60.0 / 20.0), "{folded}");
    }
}
