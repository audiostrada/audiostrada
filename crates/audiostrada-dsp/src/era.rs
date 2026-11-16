//! The era of a reverb's tail: Clean leaves it as it is. 80s, the digital units of the 1980s,
//! narrows its band, holds it at a lower rate with 12 bits and chorusses it more. Vintage, those
//! of the 1970s, narrows it further with 11 bits, saturates its input softly, thins its density
//! and lets a hiss breathe with the tail, as their companders did.

use crate::biquad::{Biquad, Coefficients, FilterKind};

/// The eras, in parameter order.
pub const COLORS: [&str; 3] = ["Clean", "80s", "Vintage"];

/// What each era sounds like.
pub const COLOR_HINTS: [&str; 3] = [
    "Today's reverb: full band, dense and smooth",
    "The digital reverbs of the 1980s: a band up to 11 kHz, the grain of 12 bits, a lush chorus",
    "The first digital reverbs and the plates of the 1970s: a dark band up to 7 kHz, grainy, \
     with a soft hiss",
];

pub const CLEAN: usize = 0;
pub const EIGHTIES: usize = 1;
pub const VINTAGE: usize = 2;

/// Highest frequency of each era's tail, in Hz.
pub const BANDS: [f32; 3] = [20_000.0, 11_000.0, 7_000.0];
/// Rate at which each era holds its samples, and its bits.
const HOLD_HZ: [f32; 3] = [0.0, 32_000.0, 20_000.0];
const BITS: [i32; 3] = [0, 12, 11];
/// How each era scales the diffusion and the modulation set by the user.
const DIFFUSION: [f32; 3] = [1.0, 0.85, 0.55];
const MODULATION: [f32; 3] = [1.0, 1.6, 0.6];
/// Vintage's hiss under its tail, and the time its level follows the tail in.
const HISS: f32 = 0.025;
const FOLLOW_MS: f32 = 50.0;

/// Diffusion handed to the engines for the user's `value` in the era `kind`.
pub fn diffusion(kind: usize, value: f32) -> f32 {
    value * DIFFUSION[kind.min(VINTAGE)]
}

/// Modulation handed to the engines for the user's `value` in the era `kind`.
pub fn modulation(kind: usize, value: f32) -> f32 {
    (value * MODULATION[kind.min(VINTAGE)]).min(100.0)
}

#[derive(Debug, Default)]
pub struct Era {
    kind: usize,
    rate: f32,
    band: Coefficients,
    input: [Biquad; 2],
    output: [[Biquad; 2]; 2],
    phase: f32,
    step: f32,
    held: [f32; 2],
    quantum: f32,
    follow: f32,
    follow_c: f32,
    seed: u32,
}

impl Era {
    pub fn set(&mut self, kind: usize, rate: f32) {
        let kind = kind.min(VINTAGE);
        let changed = kind != self.kind;
        self.kind = kind;
        self.rate = rate;
        self.band = Coefficients::design(
            FilterKind::LowPass,
            f64::from(BANDS[kind]),
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            f64::from(rate),
        );
        self.step = (HOLD_HZ[kind] / rate).min(1.0);
        self.quantum = if BITS[kind] > 0 {
            2.0 / 2f32.powi(BITS[kind])
        } else {
            0.0
        };
        self.follow_c = 1.0 - crate::time_coefficient(FOLLOW_MS, rate);
        if changed {
            self.reset();
        }
    }

    pub fn reset(&mut self) {
        self.input = Default::default();
        self.output = Default::default();
        self.phase = 0.0;
        self.held = [0.0; 2];
        self.follow = 0.0;
        self.seed = 0x2545_f491;
    }

    /// What goes into the engine: band-limited, and saturated for Vintage.
    pub fn before(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.kind == CLEAN {
            return;
        }
        for (ch, buffer) in [left, right].into_iter().enumerate() {
            for x in buffer.iter_mut() {
                let mut y = self.input[ch].process(&self.band, f64::from(*x)) as f32;
                if self.kind == VINTAGE {
                    y = (y * 1.5).tanh() / 1.5;
                }
                *x = y;
            }
        }
    }

    /// What comes out of the engine: band-limited, held at the era's rate with its bits, and
    /// with Vintage's hiss, band-limited again.
    pub fn after(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.kind == CLEAN {
            return;
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut x = [*l, *r];
            for (ch, v) in x.iter_mut().enumerate() {
                *v = self.output[0][ch].process(&self.band, f64::from(*v)) as f32;
            }
            self.phase += self.step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.held = x.map(|v| (v / self.quantum).round() * self.quantum);
            }
            let mut y = self.held;
            if self.kind == VINTAGE {
                let level = 0.5 * (x[0].abs() + x[1].abs());
                self.follow += self.follow_c * (level - self.follow);
                for v in &mut y {
                    *v += self.noise() * self.follow * HISS;
                }
            }
            *l = self.output[1][0].process(&self.band, f64::from(y[0])) as f32;
            *r = self.output[1][1].process(&self.band, f64::from(y[1])) as f32;
        }
    }

    /// White noise from -1 to 1.
    fn noise(&mut self) -> f32 {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.seed >> 9) as f32 / 4_194_304.0 - 1.0
    }
}
