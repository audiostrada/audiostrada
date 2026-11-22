//! The harmonic color of the compressor's Bus character (a VCA bus compressor's transformer),
//! and of the earlier models; their curves and times are their gain cells' (see `cell`).

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
