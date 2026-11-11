//! Stages of analog circuits, the bricks the consoles are built from: a soft shaper without
//! aliasing, an audio transformer, a class A amplifier, an op-amp and a VCA, a 2x oversampler
//! for the stages that need it; and what makes a channel of a console: a chain of stages pushed
//! by its amount, its parts' tolerances and its noise floor.
//!
//! Each stage takes one channel, a sample at a time, at 1.0 for 0 dBFS. The console's working
//! level, 0 dBu, is [`WORKING`] (-18 dBFS): levels of the circuits are given in dBu against it.
//! Nothing allocates once a stage is made.

pub mod amplifier;
pub mod chain;
pub mod noise;
pub mod oversample;
pub mod shaper;
pub mod transformer;
pub mod variation;

pub use amplifier::{ClassA, OpAmp, Vca};
pub use chain::Chain;
pub use noise::{Noise, NoiseSettings};
pub use oversample::Oversampled;
pub use shaper::Shaper;
pub use transformer::Transformer;
pub use variation::Variation;

/// Amplitude of the working level, 0 dBu: -18 dBFS.
pub const WORKING: f32 = 0.125_892_54;

/// Amplitude of a level in dBu.
pub fn dbu(level: f32) -> f32 {
    WORKING * 10f32.powf(level / 20.0)
}

/// One channel of a circuit.
pub trait Stage: Send {
    fn process(&mut self, x: f32) -> f32;
    fn reset(&mut self);
    /// Delay of the stage, in samples (a shaper's half sample included).
    fn latency(&self) -> f32 {
        0.0
    }
}

/// Stages one after the other.
impl Stage for Vec<Box<dyn Stage>> {
    fn process(&mut self, x: f32) -> f32 {
        self.iter_mut().fold(x, |y, stage| stage.process(y))
    }

    fn reset(&mut self) {
        for stage in self {
            stage.reset();
        }
    }

    fn latency(&self) -> f32 {
        self.iter().map(|s| s.latency()).sum()
    }
}

/// Coefficient of a one-pole lowpass at `hz`.
pub(crate) fn one_pole(hz: f64, rate: f64) -> f64 {
    1.0 - (-std::f64::consts::TAU * hz / rate).exp()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::measure::{self, Fingerprint};

    /// The fingerprint of a stage, the same on both sides.
    pub fn print(mut make: impl FnMut() -> Box<dyn Stage>) -> Fingerprint {
        let mut sides = [make(), make()];
        measure::measure(
            "stage",
            &mut |l: &mut [f32], r: &mut [f32]| {
                for (side, buffer) in [l, r].into_iter().enumerate() {
                    for x in buffer.iter_mut() {
                        *x = sides[side].process(*x);
                    }
                }
            },
            0,
        )
    }

    pub fn h(print: &Fingerprint, freq: f32, level: f32, order: usize) -> f32 {
        print.tone(freq, level).unwrap().harmonics[order - 2].unwrap()
    }

    /// Nanoseconds per sample of each stage, to keep the consoles within their budget:
    /// `scripts/test.sh -p audiostrada-dsp --release -- --ignored budget --nocapture`.
    #[test]
    #[ignore]
    fn budget() {
        let rate = 48_000.0;
        let stages: Vec<(&str, Box<dyn Stage>)> = vec![
            ("shaper", Box::new(Shaper::new(2.0, 0.1))),
            (
                "transformer",
                Box::new(Transformer::new(transformer::Settings::default(), rate)),
            ),
            (
                "class A",
                Box::new(ClassA::new(amplifier::ClassASettings::default(), rate)),
            ),
            ("op-amp", Box::new(OpAmp::new(24.0, 13.0, rate))),
            ("VCA", Box::new(Vca::new(-80.0, -95.0, rate))),
            (
                "transformer 2x",
                Box::new(Oversampled::new(
                    |r| Transformer::new(transformer::Settings::default(), r),
                    rate,
                )),
            ),
        ];
        let input: Vec<f32> = (0..480_000)
            .map(|i| 0.3 * (i as f32 * 0.05).sin())
            .collect();
        for (name, mut stage) in stages {
            let start = std::time::Instant::now();
            let mut sum = 0.0;
            for &x in &input {
                sum += stage.process(x);
            }
            let ns = start.elapsed().as_nanos() as f64 / input.len() as f64;
            println!("{name:<16} {ns:6.1} ns/sample ({sum:.1})");
        }
    }
}
