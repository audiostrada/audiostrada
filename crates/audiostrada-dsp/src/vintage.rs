//! The vintage characters of the compressor: Bus (a VCA bus compressor), Opto (an optical
//! leveler, very soft knee) and FET (fast and colored, with an "all buttons" ratio); their
//! curves and the harmonic color of each model.

use crate::compressor::gain_computer;

/// Gain change in dB for a level over the threshold, per model.
pub(crate) fn curve(model: usize, over: f32, ratio: f32) -> f32 {
    match model {
        // Bus: firm 2 dB knee.
        0 => gain_computer(over, 0.0, ratio, 2.0),
        // Opto: very soft knee, ratio rising with the level up to the setting.
        1 => gain_computer(over, 0.0, ratio.min(10.0), 12.0),
        // FET: at the top of the range, "all buttons": the ratio overshoots on loud peaks.
        _ if ratio >= 19.5 => -over.max(0.0) * 1.02 - (over.max(0.0) * 0.05).powi(2),
        _ => gain_computer(over, 0.0, ratio, 4.0),
    }
}

/// The model's harmonic color for an amount 0 to 1 (unity gain for small signals).
pub(crate) fn color(model: usize, amount: f32, x: f32) -> f32 {
    if amount <= 0.0 {
        return x;
    }
    let drive = 1.0 + amount * 4.0;
    let shaped = match model {
        // Transformer: gentle symmetric soft clip.
        0 => (x * drive).tanh() / drive,
        // Tube: asymmetric, even harmonics.
        1 => {
            let bias = 0.15 * amount;
            ((x * drive + bias).tanh() - bias.tanh()) / drive
        }
        // FET: harder, odd harmonics.
        _ => {
            let y = x * drive;
            (y / (1.0 + y.abs().powf(2.5)).powf(0.4)) / drive
        }
    };
    x + (shaped - x) * amount.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_compresses_over_the_threshold_only() {
        for model in 0..3 {
            assert_eq!(curve(model, -10.0, 4.0), 0.0, "{model}");
            assert!(curve(model, 12.0, 4.0) < -6.0, "{model}");
        }
    }

    #[test]
    fn color_adds_harmonics_and_stays_bounded() {
        for model in 0..3 {
            for x in [-2.0f32, -0.3, 0.0, 0.3, 2.0] {
                let y = color(model, 1.0, x);
                assert!(
                    y.is_finite() && y.abs() <= x.abs() + 0.05,
                    "{model} {x} {y}"
                );
            }
            assert!(
                (color(model, 1.0, 0.001) - 0.001).abs() < 1e-4,
                "unity for small"
            );
            assert!(color(model, 1.0, 1.0) < 0.98, "{model} saturates");
        }
    }
}
