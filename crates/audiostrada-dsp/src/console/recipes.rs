//! What each console is made of, channel and bus, and what it must measure: figures read off
//! the reference plugins (`measurements/references/windows.toml`), never their audio. The
//! stages' settings come from `fit`, which pushes them toward the targets.

use crate::analog::amplifier::ClassASettings;
use crate::analog::noise::NoiseSettings;
use crate::analog::transformer;

/// The circuit of a console's channel or bus, at its working level (amount 50 %).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    /// Its stages, in the signal's order; `None` for those it has not.
    pub input: Option<transformer::Settings>,
    pub class_a: Option<ClassASettings>,
    /// An op-amp: its rails, dBu, and slew rate, V/µs.
    pub op_amp: Option<(f32, f32)>,
    /// A VCA: its 2nd and 3rd harmonics at the working level, dB.
    pub vca: Option<(f32, f32)>,
    pub output: Option<transformer::Settings>,
    /// A low and a high shelf, Hz and dB: the faint tilt of its response.
    pub low: (f64, f64),
    pub high: (f64, f64),
    pub noise: NoiseSettings,
    /// What leaks from one side of a bus into the other at 1 kHz, dB; rising with frequency.
    pub crosstalk_db: f32,
}

/// What is read of a tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Thd,
    H2,
    H3,
}

/// A figure a console must measure, at amount 50 %: a tone's frequency and level (dBFS), what
/// is read of it, and its value in dB below the tone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub freq: f32,
    pub level: f32,
    pub reading: Reading,
    pub db: f32,
}

const fn t(freq: f32, level: f32, reading: Reading, db: f32) -> Target {
    Target {
        freq,
        level,
        reading,
        db,
    }
}

use Reading::{H2, H3, Thd};

/// The targets of `model` (an index into `MODELS`), channel or bus, from plugins modelling the
/// console each one is after, set neutral.
///
/// - British Warm, a Neve: transformers, the lows distort most, 2nd and 3rd harmonics alike.
/// - British Bright, an SSL 4000: the channel flat in frequency, odd harmonics (its VCA); the
///   bus colored, even ones, the lows heavy but tamer than its reference's (it saturates them
///   even at -36 dBFS).
/// - American Punch, an API: even harmonics. No bus measured: one a little hotter than the
///   channel.
/// - Nashville, a Harrison 32C: the channel nearly clean, the bus's drive making odd harmonics,
///   heaviest in the lows.
pub fn targets(model: usize, bus: bool) -> &'static [Target] {
    match (model, bus) {
        (1, false) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -75.0),
                t(1_000.0, -6.0, Thd, -60.0),
                t(1_000.0, 0.0, Thd, -50.0),
                t(1_000.0, -6.0, H2, -65.0),
                t(1_000.0, -6.0, H3, -63.0),
                t(100.0, -6.0, Thd, -60.0),
                t(30.0, -18.0, Thd, -58.0),
                t(30.0, -6.0, Thd, -48.0),
            ];
            T
        }
        (1, true) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -80.0),
                t(1_000.0, -6.0, Thd, -66.0),
                t(1_000.0, 0.0, Thd, -58.0),
                t(1_000.0, -6.0, H2, -70.0),
                t(100.0, -6.0, Thd, -63.0),
                t(30.0, -6.0, Thd, -52.0),
            ];
            T
        }
        (2, false) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -90.0),
                t(1_000.0, -6.0, Thd, -68.0),
                t(1_000.0, 0.0, Thd, -56.0),
                t(1_000.0, -6.0, H2, -82.0),
                t(100.0, -6.0, Thd, -67.0),
                t(30.0, -6.0, Thd, -66.0),
            ];
            T
        }
        (2, true) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -58.0),
                t(1_000.0, -6.0, Thd, -50.0),
                t(1_000.0, -6.0, H3, -70.0),
                t(100.0, -6.0, Thd, -46.0),
                t(30.0, -6.0, Thd, -40.0),
            ];
            T
        }
        (3, false) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -84.0),
                t(1_000.0, -6.0, Thd, -72.0),
                t(1_000.0, 0.0, Thd, -64.0),
                t(1_000.0, -6.0, H3, -88.0),
                t(100.0, -6.0, Thd, -70.0),
                t(30.0, -6.0, Thd, -62.0),
            ];
            T
        }
        (3, true) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -78.0),
                t(1_000.0, -6.0, Thd, -66.0),
                t(1_000.0, 0.0, Thd, -58.0),
                t(1_000.0, -6.0, H3, -80.0),
                t(100.0, -6.0, Thd, -62.0),
                t(30.0, -6.0, Thd, -54.0),
            ];
            T
        }
        (4, false) => {
            const T: &[Target] = &[
                t(1_000.0, -6.0, Thd, -92.0),
                t(1_000.0, 0.0, Thd, -82.0),
                t(1_000.0, -6.0, H2, -96.0),
                t(30.0, -6.0, Thd, -84.0),
            ];
            T
        }
        (4, true) => {
            const T: &[Target] = &[
                t(1_000.0, -18.0, Thd, -84.0),
                t(1_000.0, -6.0, Thd, -60.0),
                t(1_000.0, 0.0, Thd, -48.0),
                t(1_000.0, -6.0, H2, -105.0),
                t(100.0, -18.0, Thd, -69.0),
                t(100.0, -6.0, Thd, -45.0),
                t(30.0, -6.0, Thd, -46.0),
            ];
            T
        }
        _ => &[],
    }
}

/// A transformer: its core's corner, the level that saturates it, its hysteresis, and how much
/// of its core's error reaches the output.
const fn iron(
    corner_hz: f64,
    saturation_dbu: f32,
    hysteresis: f32,
    coupling: f32,
) -> transformer::Settings {
    transformer::Settings {
        corner_hz,
        saturation_dbu,
        hysteresis,
        coupling,
        top_hz: 40_000.0,
        top_q: 0.8,
        bottom_hz: 8.0,
    }
}

const fn class_a(headroom_dbu: f32, asymmetry: f32) -> ClassASettings {
    ClassASettings {
        headroom_dbu,
        asymmetry,
        drift: 0.6,
        drift_ms: 60.0,
        bottom_hz: 6.0,
    }
}

const fn noise(hiss_dbu: f32, hum_dbu: f32) -> NoiseSettings {
    NoiseSettings {
        hiss_dbu,
        flicker_hz: 200.0,
        hum: Some((50.0, hum_dbu)),
    }
}

const FLAT: (f64, f64) = (1_000.0, 0.0);

/// The circuit of `model`, channel or bus; `None` for Clean.
pub fn recipe(model: usize, bus: bool) -> Option<Recipe> {
    let none = Recipe {
        input: None,
        class_a: None,
        op_amp: None,
        vca: None,
        output: None,
        low: FLAT,
        high: FLAT,
        noise: noise(-90.0, -120.0),
        crosstalk_db: -80.0,
    };
    // Settings found by `fit_recipes` (console/tests.rs) on the targets above.
    Some(match (model, bus) {
        // A Neve: an input transformer, its hysteresis holding the lows, into a class A
        // amplifier.
        (1, false) => Recipe {
            input: Some(iron(30.0, 31.25, 0.95, 0.95)),
            class_a: Some(class_a(23.25, 0.0135)),
            high: (16_000.0, -0.3),
            noise: noise(-84.0, -120.0),
            ..none
        },
        // Its summing: a class A amplifier into the bus's output transformer.
        (1, true) => Recipe {
            class_a: Some(class_a(49.0, 0.188)),
            output: Some(iron(25.0, 28.75, 0.95, 0.5)),
            high: (16_000.0, -0.3),
            noise: noise(-82.0, -110.0),
            ..none
        },
        // An SSL channel: op-amps and the VCA of its dynamics, no transformer on its way.
        (2, false) => Recipe {
            op_amp: Some((26.0, 13.0)),
            vca: Some((-94.0, -91.5)),
            high: (12_000.0, 0.3),
            noise: noise(-88.0, -120.0),
            ..none
        },
        // Its mix bus: summing amplifier, its VCA, the output transformer driven hard.
        (2, true) => Recipe {
            class_a: Some(class_a(24.0, 0.1)),
            op_amp: Some((26.0, 13.0)),
            vca: Some((-89.5, -94.25)),
            output: Some(iron(30.0, 19.5, 0.95, 0.5)),
            high: (12_000.0, 0.3),
            noise: noise(-84.0, -110.0),
            ..none
        },
        // An API: a discrete amplifier, even harmonics, into its output transformer.
        (3, false) => Recipe {
            class_a: Some(class_a(36.0, 0.0252)),
            output: Some(iron(25.0, 26.5, 0.0067, 0.5)),
            low: (90.0, 0.2),
            noise: noise(-86.0, -120.0),
            ..none
        },
        (3, true) => Recipe {
            input: Some(iron(30.0, 28.75, 0.95, 0.2259)),
            class_a: Some(class_a(32.0, 0.0327)),
            output: Some(iron(25.0, 38.25, 0.95, 0.5)),
            low: (90.0, 0.2),
            noise: noise(-83.0, -110.0),
            ..none
        },
        // A Harrison 32C: op-amps, nearly clean; its input transformer's hysteresis, a trace
        // in the lows.
        (4, false) => Recipe {
            input: Some(iron(30.0, 55.0, 0.2824, 0.396)),
            op_amp: Some((27.0, 13.0)),
            vca: Some((-108.0, -118.5)),
            noise: noise(-92.0, -120.0),
            ..none
        },
        // Its bus: a transformer and a symmetric drive, odd harmonics, the lows heavy.
        (4, true) => Recipe {
            input: Some(iron(100.0, 10.5, 0.0, 0.07)),
            op_amp: Some((27.0, 13.0)),
            vca: Some((-117.0, -83.75)),
            noise: noise(-90.0, -110.0),
            ..none
        },
        _ => return None,
    })
}
