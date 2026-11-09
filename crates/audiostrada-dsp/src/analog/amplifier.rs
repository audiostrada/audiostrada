//! Amplifiers: a discrete class A stage (asymmetric, its bias drifting with the level), an
//! op-amp (clean until near its rails, its slew rate limited) and a VCA (a faint, steady
//! coloration).

use super::{Shaper, Stage, dbu, one_pole};
use crate::biquad::{Biquad, Coefficients};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassASettings {
    /// Level where the stage reaches 1 % of distortion, dBu.
    pub headroom_dbu: f32,
    /// Asymmetry of its curve: the share of even harmonics.
    pub asymmetry: f32,
    /// How far the DC its asymmetry makes moves its bias, and how fast (ms): the memory of
    /// coupling capacitors charged by a loud passage.
    pub drift: f32,
    pub drift_ms: f64,
    /// The output coupling's highpass, Hz.
    pub bottom_hz: f64,
}

impl Default for ClassASettings {
    fn default() -> Self {
        Self {
            headroom_dbu: 22.0,
            asymmetry: 0.25,
            drift: 0.6,
            drift_ms: 60.0,
            bottom_hz: 6.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClassA {
    settings: ClassASettings,
    shaper: Shaper,
    /// Input scale, so that `headroom_dbu` meets the curve's knee.
    scale: f32,
    dc: f64,
    dc_a: f64,
    bottom: Coefficients,
    bottom_state: Biquad,
}

impl ClassA {
    pub fn new(settings: ClassASettings, rate: f64) -> Self {
        // A tanh curve reaches 1 % of distortion near 0.35 (its 3rd harmonic is x² / 12).
        let scale = 0.35 / dbu(settings.headroom_dbu);
        Self {
            settings,
            shaper: Shaper::new(1.0, settings.asymmetry),
            scale,
            dc: 0.0,
            dc_a: one_pole(1_000.0 / (std::f64::consts::TAU * settings.drift_ms), rate),
            bottom: Coefficients::first_order(true, settings.bottom_hz, rate),
            bottom_state: Biquad::default(),
        }
    }
}

impl Stage for ClassA {
    fn process(&mut self, x: f32) -> f32 {
        let shift = -(self.settings.drift * self.dc as f32);
        let y = self.shaper.shifted(x * self.scale, shift);
        // The DC of the asymmetric curve charges the coupling, moving the bias back.
        self.dc += self.dc_a * (f64::from(y) - self.dc);
        let y = f64::from(y / self.scale);
        self.bottom_state.process(&self.bottom, y) as f32
    }

    fn reset(&mut self) {
        self.shaper.reset();
        self.dc = 0.0;
        self.bottom_state.reset();
    }

    fn latency(&self) -> f32 {
        0.5
    }
}

/// An op-amp: clean, then a firm limit at its rails; its output cannot move faster than its
/// slew rate.
#[derive(Clone, Debug)]
pub struct OpAmp {
    rail: f32,
    /// Largest change per sample.
    slew: f32,
    y1: f32,
}

impl OpAmp {
    /// `rails_dbu`: the level of its rails; `slew` in V/µs, against rails of ±15 V.
    pub fn new(rails_dbu: f32, slew: f32, rate: f64) -> Self {
        let rail = dbu(rails_dbu) * std::f32::consts::SQRT_2;
        let volts_per_unit = 15.0 / rail;
        Self {
            rail,
            slew: (f64::from(slew) * 1e6 / rate) as f32 / volts_per_unit,
            y1: 0.0,
        }
    }
}

impl Stage for OpAmp {
    fn process(&mut self, x: f32) -> f32 {
        // A soft knee of the 8th order: transparent below the rails.
        let r = x / self.rail;
        let limited = x / (1.0 + r.powi(8)).powf(0.125);
        let y = self.y1 + (limited - self.y1).clamp(-self.slew, self.slew);
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.y1 = 0.0;
    }
}

/// A VCA: its 2nd and 3rd harmonics stay near their level at the working level.
#[derive(Clone, Debug)]
pub struct Vca {
    a2: f32,
    a3: f32,
    dc: f64,
    dc_a: f64,
}

impl Vca {
    /// The 2nd and 3rd harmonics of a tone at the working level, dB below it.
    pub fn new(h2_db: f32, h3_db: f32, rate: f64) -> Self {
        let w = super::WORKING;
        // x + a2 x² gives a 2nd harmonic of a2 A / 2, x + a3 x³ a 3rd of a3 A² / 4.
        Self {
            a2: 2.0 * 10f32.powf(h2_db / 20.0) / w,
            a3: 4.0 * 10f32.powf(h3_db / 20.0) / (w * w),
            dc: 0.0,
            dc_a: one_pole(5.0, rate),
        }
    }
}

impl Stage for Vca {
    fn process(&mut self, x: f32) -> f32 {
        let y = x + self.a2 * x * x + self.a3 * x * x * x;
        self.dc += self.dc_a * (f64::from(y) - self.dc);
        (f64::from(y) - self.dc) as f32
    }

    fn reset(&mut self) {
        self.dc = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::tests::{h, print};

    #[test]
    fn class_a_makes_even_harmonics_growing_with_the_level() {
        let p = print(|| Box::new(ClassA::new(ClassASettings::default(), 48_000.0)));
        assert!(h(&p, 1_000.0, -12.0, 2) > h(&p, 1_000.0, -12.0, 3) + 6.0);
        let slope = (h(&p, 1_000.0, -12.0, 2) - h(&p, 1_000.0, -24.0, 2)) / 12.0;
        assert!((slope - 1.0).abs() < 0.3, "{slope}");
        // About 1 % at its headroom (+22 dBu, +4 dBFS): already a few tenths at 0 dBFS.
        let full = p.tone(1_000.0, 0.0).unwrap().thd_db;
        assert!((-60.0..-30.0).contains(&full), "{full}");
        assert!(p.tone(1_000.0, -18.0).unwrap().gain_db.abs() < 0.1);
    }

    #[test]
    fn class_a_remembers_a_loud_passage() {
        let p = print(|| Box::new(ClassA::new(ClassASettings::default(), 48_000.0)));
        let m = p.memory;
        assert!((m.h2_after_db - m.h2_settled_db).abs() > 3.0, "{m:?}");
        assert!(m.recovery_ms > 20.0, "{m:?}");
        let still = print(|| {
            Box::new(ClassA::new(
                ClassASettings {
                    drift: 0.0,
                    ..ClassASettings::default()
                },
                48_000.0,
            ))
        });
        let m0 = still.memory;
        assert!(
            (m0.h2_after_db - m0.h2_settled_db).abs() < (m.h2_after_db - m.h2_settled_db).abs(),
            "{m0:?} {m:?}"
        );
    }

    #[test]
    fn an_op_amp_is_clean_below_its_rails_and_slews() {
        let p = print(|| Box::new(OpAmp::new(24.0, 13.0, 48_000.0)));
        for t in p.tones.iter().filter(|t| t.level <= -6.0) {
            assert!(t.thd_db < -90.0, "{t:?}");
        }
        // Near its rails, odd harmonics.
        let low_rails = print(|| Box::new(OpAmp::new(16.0, 13.0, 48_000.0)));
        assert!(h(&low_rails, 1_000.0, 0.0, 3) > -60.0);
        assert!(h(&low_rails, 1_000.0, 0.0, 2) < -100.0);
        // A slow op-amp cannot follow a loud 10 kHz.
        let mut slow = OpAmp::new(24.0, 0.1, 48_000.0);
        let out: Vec<f32> = (0..480)
            .map(|i| 0.9 * (i as f32 * std::f32::consts::TAU * 10_000.0 / 48_000.0).sin())
            .map(|x| slow.process(x))
            .collect();
        let peak = out[240..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.8, "{peak}");
    }

    #[test]
    fn a_vca_keeps_its_harmonics_at_the_working_level() {
        let p = print(|| Box::new(Vca::new(-80.0, -95.0, 48_000.0)));
        assert!((h(&p, 1_000.0, -18.0, 2) + 80.0).abs() < 1.0);
        assert!((h(&p, 1_000.0, -18.0, 3) + 95.0).abs() < 1.5);
    }
}
