//! An audio transformer. Its core holds a flux that follows the integral of the voltage, so the
//! lows drive it hardest: its distortion grows 12 dB per octave down and 2 dB per dB of level,
//! as the core saturates. The core's hysteresis (Bouc-Wen) leaves some distortion even at low
//! levels and gives the lows their roundness. Its windings roll the top off, its coupling the
//! very bottom.

use super::{Stage, dbu, one_pole};
use crate::biquad::{Biquad, Coefficients, FilterKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Where the flux stops following the integral: the core's corner, Hz.
    pub corner_hz: f64,
    /// Level of a tone at `corner_hz` that saturates the core, dBu.
    pub saturation_dbu: f32,
    /// Share of the core's magnetization that lags (0: none): its hysteresis.
    pub hysteresis: f32,
    /// How much of the core's error reaches the output: the source impedance driving it.
    pub coupling: f32,
    /// The windings' rolloff: a lowpass, and its resonance.
    pub top_hz: f64,
    pub top_q: f64,
    /// The coupling's highpass, Hz.
    pub bottom_hz: f64,
}

impl Default for Settings {
    /// A line input transformer of a classic console.
    fn default() -> Self {
        Self {
            corner_hz: 30.0,
            saturation_dbu: 20.0,
            hysteresis: 0.15,
            coupling: 0.5,
            top_hz: 40_000.0,
            top_q: 0.8,
            bottom_hz: 8.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Transformer {
    settings: Settings,
    /// Flux follower, and the level the flux saturates at.
    a: f64,
    flux: f64,
    scale: f64,
    /// Bouc-Wen state: the lagging magnetization, and the last normalized flux.
    z: f64,
    u1: f64,
    top: Coefficients,
    top_state: Biquad,
    bottom: Coefficients,
    bottom_state: Biquad,
}

impl Transformer {
    pub fn new(settings: Settings, rate: f64) -> Self {
        // A tone at the corner of the saturation level has a flux of 1/sqrt(2) of it there.
        let scale = f64::from(dbu(settings.saturation_dbu)) * std::f64::consts::FRAC_1_SQRT_2;
        let nyquist = rate * 0.45;
        Self {
            settings,
            a: one_pole(settings.corner_hz, rate),
            flux: 0.0,
            scale: scale.max(1e-9),
            z: 0.0,
            u1: 0.0,
            top: Coefficients::design(
                FilterKind::LowPass,
                settings.top_hz.min(nyquist),
                settings.top_q,
                0.0,
                rate,
            ),
            top_state: Biquad::default(),
            bottom: Coefficients::first_order(true, settings.bottom_hz, rate),
            bottom_state: Biquad::default(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }
}

impl Stage for Transformer {
    fn process(&mut self, x: f32) -> f32 {
        let x = f64::from(x);
        self.flux += self.a * (x - self.flux);
        let u = self.flux / self.scale;
        // Bouc-Wen with A = 1, beta = gamma = 1/2: z follows u, lags it when it turns back, and
        // stays within ±1.
        let du = u - self.u1;
        self.u1 = u;
        let s = if du * self.z >= 0.0 { 1.0 } else { 0.0 };
        self.z += du * (1.0 - (0.5 * s + 0.5) * self.z.abs());
        self.z = self.z.clamp(-1.0, 1.0);
        let h = f64::from(self.settings.hysteresis);
        let magnetization = (1.0 - h) * u.tanh() + h * self.z;
        // The flux the core does not carry shows at the output, through the source impedance.
        let error = (u - magnetization) * self.scale;
        let y = x - f64::from(self.settings.coupling) * error;
        let y = self.top_state.process(&self.top, y);
        self.bottom_state.process(&self.bottom, y) as f32
    }

    fn reset(&mut self) {
        self.flux = 0.0;
        self.z = 0.0;
        self.u1 = 0.0;
        self.top_state.reset();
        self.bottom_state.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::tests::{h, print};

    fn transformer(settings: Settings) -> Box<dyn Stage> {
        Box::new(Transformer::new(settings, 48_000.0))
    }

    #[test]
    fn the_lows_distort_most_and_grow_with_the_level() {
        let p = print(|| {
            transformer(Settings {
                hysteresis: 0.0,
                ..Settings::default()
            })
        });
        // 12 dB per octave down: 30 Hz far above 1 kHz.
        assert!(h(&p, 30.0, -6.0, 3) > h(&p, 1_000.0, -6.0, 3) + 40.0);
        let (low, loud) = (h(&p, 50.0, -24.0, 3), h(&p, 50.0, -12.0, 3));
        // A saturating core: about 2 dB more per dB of level.
        assert!((loud - low - 24.0).abs() < 4.0, "{low} {loud}");
        // Symmetric: odd harmonics, no even ones.
        assert!(h(&p, 50.0, -6.0, 2) < h(&p, 50.0, -6.0, 3) - 40.0);
        // Flat at the working level, within the windings' rolloff.
        for t in p
            .tones
            .iter()
            .filter(|t| t.level == -18.0 && t.freq >= 100.0)
        {
            assert!(t.gain_db.abs() < 0.3, "{t:?}");
        }
    }

    #[test]
    fn hysteresis_leaves_distortion_at_low_levels() {
        let without = print(|| {
            transformer(Settings {
                hysteresis: 0.0,
                ..Settings::default()
            })
        });
        let with = print(|| transformer(Settings::default()));
        // A quiet low tone: the plain core is nearly clean, the hysteretic one is not.
        let quiet = |p| h(p, 50.0, -36.0, 3);
        assert!(
            quiet(&with) > quiet(&without) + 15.0,
            "{} {}",
            quiet(&with),
            quiet(&without)
        );
        // Its distortion grows slower than the saturation's: about 1 dB per dB.
        let slope = (h(&with, 50.0, -30.0, 3) - h(&with, 50.0, -36.0, 3)) / 6.0;
        assert!(slope < 1.6, "{slope}");
    }
}
