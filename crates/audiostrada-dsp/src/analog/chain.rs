//! A circuit made of stages, pushed as hard as its amount asks: the signal goes into it louder
//! and comes back out as much quieter, so that only its color changes, never its level. Its
//! channel's tolerance and noise floor come with it.

use super::noise::Noise;
use super::{Stage, one_pole};

/// Drive of the amount's ends against its middle, dB: 0 % 12 dB softer, 100 % 12 dB harder.
pub const DRIVE_RANGE_DB: f32 = 12.0;

/// `S` is the circuit: a list of boxed stages, or a console's own, which changes without
/// allocating.
pub struct Chain<S = Vec<Box<dyn Stage>>> {
    stages: S,
    /// Gain into the circuit, toward its target without zipper noise.
    drive: f32,
    target: f32,
    smooth: f32,
    /// The channel's own gain error.
    trim: f32,
    noise: Option<Noise>,
}

impl<S> std::fmt::Debug for Chain<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chain")
            .field("drive", &self.target)
            .finish_non_exhaustive()
    }
}

impl<S: Stage> Chain<S> {
    pub fn new(stages: S, trim_db: f32, noise: Option<Noise>, rate: f64) -> Self {
        Self {
            stages,
            drive: 1.0,
            target: 1.0,
            smooth: one_pole(1_000.0 / (std::f64::consts::TAU * 20.0), rate) as f32,
            trim: 10f32.powf(trim_db / 20.0),
            noise,
        }
    }

    /// How hard the circuit is pushed, 0 to 100 %: 50 % is the console at its working level.
    pub fn set_amount(&mut self, amount: f32) {
        let db = (amount.clamp(0.0, 100.0) - 50.0) / 50.0 * DRIVE_RANGE_DB;
        self.target = 10f32.powf(db / 20.0);
    }
}

impl<S: Stage> Stage for Chain<S> {
    fn process(&mut self, x: f32) -> f32 {
        self.drive += self.smooth * (self.target - self.drive);
        let y = self.stages.process(x * self.drive);
        let mut y = y / self.drive * self.trim;
        if let Some(noise) = &mut self.noise {
            y += noise.sample();
        }
        y
    }

    fn reset(&mut self) {
        self.drive = self.target;
        self.stages.reset();
    }

    fn latency(&self) -> f32 {
        self.stages.latency()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::noise::NoiseSettings;
    use crate::analog::tests::print;
    use crate::analog::{ClassA, Transformer, amplifier, transformer};

    fn chain(amount: f32, noise: bool) -> Box<dyn Stage> {
        let rate = 48_000.0;
        let stages: Vec<Box<dyn Stage>> = vec![
            Box::new(Transformer::new(transformer::Settings::default(), rate)),
            Box::new(ClassA::new(amplifier::ClassASettings::default(), rate)),
        ];
        let mut chain = Chain::new(
            stages,
            0.0,
            noise.then(|| Noise::new(NoiseSettings::default(), rate, 1)),
            rate,
        );
        chain.set_amount(amount);
        chain.reset();
        Box::new(chain)
    }

    #[test]
    fn the_amount_changes_the_color_not_the_level() {
        let half = print(|| chain(50.0, false));
        let full = print(|| chain(100.0, false));
        for p in [&half, &full] {
            let gain = p.tone(1_000.0, -18.0).unwrap().gain_db;
            assert!(gain.abs() < 0.3, "{gain}");
        }
        let thd = |p: &crate::measure::Fingerprint| p.tone(1_000.0, -6.0).unwrap().thd_db;
        // 12 dB harder: about 12 dB more distortion for a mostly 2nd-order color.
        assert!(
            thd(&full) > thd(&half) + 9.0,
            "{} {}",
            thd(&half),
            thd(&full)
        );
        let soft = print(|| chain(0.0, false));
        assert!(thd(&soft) < thd(&half) - 9.0);
    }

    #[test]
    fn a_new_amount_glides_and_the_floor_comes_with_the_noise() {
        let mut c = chain(0.0, false);
        let tone = |i: usize| 0.5 * (i as f32 * std::f32::consts::TAU * 100.0 / 48_000.0).sin();
        let mut last = 0.0;
        let mut jump = 0.0f32;
        for i in 0..9_600 {
            if i == 4_800 {
                // Set through a fresh chain: the steps of a glide stay small.
                let mut fresh: Chain = Chain::new(Vec::new(), 0.0, None, 48_000.0);
                fresh.set_amount(0.0);
                fresh.reset();
                fresh.set_amount(100.0);
                let steps: Vec<f32> = (0..2_000).map(|_| fresh.process(1.0)).collect();
                assert!(steps.windows(2).all(|w| (w[1] - w[0]).abs() < 0.01));
            }
            let y = c.process(tone(i));
            jump = jump.max((y - last).abs());
            last = y;
        }
        assert!(jump < 0.05, "{jump}");
        let quiet = print(|| chain(50.0, true));
        // A hiss of -88 dBu is -106 dBFS; the flicker adds a little.
        assert!(
            (-107.0..-103.0).contains(&quiet.noise.rms_db),
            "{}",
            quiet.noise.rms_db
        );
    }
}
