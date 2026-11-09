//! What sets a channel of a console apart from its neighbors: the tolerances of its parts. Each
//! channel gets its own, fixed, from a seed (a track's id): slightly different gains, drives and
//! corners on each side, which is what widens a mix through an analog console.

/// Factors a channel's settings are multiplied by, and its small gain error.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Variation {
    pub gain_db: f32,
    pub drive: f32,
    pub asymmetry: f32,
    pub corner: f32,
    pub top: f32,
}

impl Variation {
    /// A perfect channel.
    pub const NONE: Self = Self {
        gain_db: 0.0,
        drive: 1.0,
        asymmetry: 1.0,
        corner: 1.0,
        top: 1.0,
    };

    /// The tolerances of `side` (0 left, 1 right) of channel `seed`; `spread` 1 is a console's
    /// parts (1 % resistors, 5 to 10 % capacitors and transistors), 0 none.
    pub fn of(seed: u64, side: usize, spread: f32) -> Self {
        if spread <= 0.0 {
            return Self::NONE;
        }
        let mut state = seed ^ (side as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let mut normal = |deviation: f32| deviation * spread * gaussian(&mut state);
        Self {
            gain_db: normal(0.1),
            drive: 1.0 + normal(0.03),
            asymmetry: 1.0 + normal(0.1),
            corner: 1.0 + normal(0.05),
            top: 1.0 + normal(0.03),
        }
    }
}

/// SplitMix64: well spread numbers from consecutive seeds.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Close to a normal law of deviation 1, within ±3 (three uniforms summed).
fn gaussian(state: &mut u64) -> f32 {
    let sum: f32 = (0..3)
        .map(|_| (next(state) >> 40) as f32 / (1u64 << 24) as f32)
        .sum();
    (sum - 1.5) * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_keeps_its_own_tolerances() {
        assert_eq!(Variation::of(7, 0, 1.0), Variation::of(7, 0, 1.0));
        assert_ne!(Variation::of(7, 0, 1.0), Variation::of(8, 0, 1.0));
        assert_ne!(Variation::of(7, 0, 1.0), Variation::of(7, 1, 1.0));
        assert_eq!(Variation::of(7, 0, 0.0), Variation::NONE);
        let all: Vec<Variation> = (0..2_000).map(|s| Variation::of(s, 0, 1.0)).collect();
        let mean = all.iter().map(|v| v.gain_db).sum::<f32>() / all.len() as f32;
        assert!(mean.abs() < 0.01, "{mean}");
        let deviation =
            (all.iter().map(|v| v.gain_db * v.gain_db).sum::<f32>() / all.len() as f32).sqrt();
        assert!((deviation - 0.1).abs() < 0.015, "{deviation}");
        assert!(
            all.iter()
                .all(|v| v.gain_db.abs() <= 0.3 && (v.drive - 1.0).abs() <= 0.09)
        );
    }
}
