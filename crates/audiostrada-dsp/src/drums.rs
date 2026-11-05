//! Drum machine: sixteen pads on the General MIDI drum notes. Each drum is built from a recipe
//! of layers (a tuned body with its pitch drop, two more modes, filtered noise, a click, a bank
//! of metallic oscillators, the bursts of a clap) in three characters: 808, 909 and acoustic.
//! A pad can play a WAV file instead. Level, pan, tune, decay, tone, attack, pitch envelope,
//! filters and sends per pad; accent, drive, crush, compression, EQ, delay and room for the kit.
//! The closed hat chokes the open one.

use std::sync::Arc;

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{
    Effect, EffectInfo, FileData, FileKind, FileSlot, MidiEvent, ParamInfo, choice, db_to_gain,
    param,
};

/// Pads: identifier, name, and the MIDI note it answers to. The first twelve keep their place
/// from the first version, so that saved kits keep their settings.
pub const VOICES: [(&str, &str, u8); 16] = [
    ("kick", "Kick", 36),
    ("rim", "Rim shot", 37),
    ("snare", "Snare", 38),
    ("clap", "Clap", 39),
    ("closed", "Closed hat", 42),
    ("open", "Open hat", 46),
    ("low_tom", "Low tom", 45),
    ("mid_tom", "Mid tom", 47),
    ("high_tom", "High tom", 50),
    ("cymbal", "Crash", 49),
    ("cowbell", "Cowbell", 56),
    ("clave", "Clave", 75),
    ("kick2", "Kick 2", 35),
    ("snare2", "Snare 2", 40),
    ("ride", "Ride", 51),
    ("tambourine", "Tambourine", 54),
];

/// The pads in the order the editor lays them out, by family.
pub const LAYOUT: [usize; 16] = [0, 12, 2, 13, 3, 1, 4, 5, 6, 7, 8, 9, 14, 15, 10, 11];

/// What sort of drum each pad is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Kick,
    Snare,
    Clap,
    Closed,
    Open,
    Tom(u8),
    Crash,
    Ride,
    Cowbell,
    Rim,
    Clave,
    Tambourine,
}

pub const KINDS: [Kind; 16] = [
    Kind::Kick,
    Kind::Rim,
    Kind::Snare,
    Kind::Clap,
    Kind::Closed,
    Kind::Open,
    Kind::Tom(0),
    Kind::Tom(1),
    Kind::Tom(2),
    Kind::Crash,
    Kind::Cowbell,
    Kind::Clave,
    Kind::Kick,
    Kind::Snare,
    Kind::Ride,
    Kind::Tambourine,
];

/// The characters a pad can have.
pub const MODELS: [&str; 3] = ["808", "909", "Acoustic"];

#[cfg(test)]
const KICK: usize = 0;
const CLOSED: usize = 4;
const OPEN: usize = 5;

/// Where each setting sits: the first version's four per pad for twelve pads, its three kit
/// settings, then the same four for the four new pads, pans, characters, room and crush.
pub mod p {
    pub const ACCENT: usize = 48;
    pub const DRIVE: usize = 49;
    pub const VOLUME: usize = 50;
    pub const PANS: usize = 67;
    pub const MODELS: usize = 83;
    pub const ROOM: usize = 99;
    pub const CRUSH: usize = 100;
    pub const COMP: usize = 101;
    pub const LOW: usize = 102;
    pub const HIGH: usize = 103;
    pub const DELAY_NOTE: usize = 104;
    pub const DELAY_FEEDBACK: usize = 105;
    pub const DELAY: usize = 106;
    pub const ROOM_SIZE: usize = 107;
    pub const SHAPES: usize = 108;

    /// Attack, pitch envelope, its decay, low cut, high cut, delay and reverb sends of `pad`.
    pub const fn shape(pad: usize) -> usize {
        SHAPES + pad * super::SHAPE_FIELDS
    }

    /// Level, tune, decay and tone of pad `pad`, in this order from the index returned.
    pub const fn fields(pad: usize) -> usize {
        if pad < 12 {
            pad * 4
        } else {
            51 + (pad - 12) * 4
        }
    }
}
/// Settings per pad among the first four: level, tune, decay, tone.
pub const PER_VOICE: usize = 4;
/// Settings per pad among the last ones.
pub const SHAPE_FIELDS: usize = 7;
const COUNT: usize = p::SHAPES + VOICES.len() * SHAPE_FIELDS;

macro_rules! pad_fields {
    ($($id:literal, $name:literal;)*) => {
        [
            $(
                param(concat!($id, "_level"), concat!($name, " level"), 0.0, 100.0, 80.0, "%"),
                param(concat!($id, "_tune"), concat!($name, " tune"), -12.0, 12.0, 0.0, "st"),
                param(concat!($id, "_decay"), concat!($name, " decay"), 10.0, 400.0, 100.0, "%"),
                param(concat!($id, "_tone"), concat!($name, " tone"), 0.0, 100.0, 50.0, "%"),
            )*
        ]
    };
}

macro_rules! pad_more {
    ($($id:literal, $name:literal;)*) => {
        (
            [$(param(concat!($id, "_pan"), concat!($name, " pan"), -100.0, 100.0, 0.0, "%"),)*],
            [$(choice(concat!($id, "_model"), concat!($name, " character"), &MODELS, 0.0),)*],
        )
    };
}

macro_rules! pad_shapes {
    ($($id:literal, $name:literal;)*) => {
        [
            $(
                param(concat!($id, "_attack"), concat!($name, " attack"), 0.0, 100.0, 0.0, "ms"),
                param(concat!($id, "_pitch"), concat!($name, " pitch envelope"), -24.0, 24.0, 0.0, "st"),
                param(concat!($id, "_pitch_decay"), concat!($name, " pitch decay"), 5.0, 1000.0, 80.0, "ms"),
                param(concat!($id, "_low_cut"), concat!($name, " low cut"), 20.0, 2000.0, 20.0, "Hz"),
                param(concat!($id, "_high_cut"), concat!($name, " high cut"), 500.0, 20000.0, 20000.0, "Hz"),
                param(concat!($id, "_delay"), concat!($name, " delay send"), 0.0, 100.0, 100.0, "%"),
                param(concat!($id, "_reverb"), concat!($name, " reverb send"), 0.0, 100.0, 100.0, "%"),
            )*
        ]
    };
}

const FIRST: [ParamInfo; 48] = pad_fields! {
    "kick", "Kick";
    "rim", "Rim shot";
    "snare", "Snare";
    "clap", "Clap";
    "closed", "Closed hat";
    "open", "Open hat";
    "low_tom", "Low tom";
    "mid_tom", "Mid tom";
    "high_tom", "High tom";
    "cymbal", "Crash";
    "cowbell", "Cowbell";
    "clave", "Clave";
};
const KIT: [ParamInfo; 3] = [
    param("accent", "Accent", 0.0, 100.0, 60.0, "%"),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    param("volume", "Volume", -48.0, 6.0, -6.0, "dB"),
];
const NEW: [ParamInfo; 16] = pad_fields! {
    "kick2", "Kick 2";
    "snare2", "Snare 2";
    "ride", "Ride";
    "tambourine", "Tambourine";
};
const MORE: ([ParamInfo; 16], [ParamInfo; 16]) = pad_more! {
    "kick", "Kick";
    "rim", "Rim shot";
    "snare", "Snare";
    "clap", "Clap";
    "closed", "Closed hat";
    "open", "Open hat";
    "low_tom", "Low tom";
    "mid_tom", "Mid tom";
    "high_tom", "High tom";
    "cymbal", "Crash";
    "cowbell", "Cowbell";
    "clave", "Clave";
    "kick2", "Kick 2";
    "snare2", "Snare 2";
    "ride", "Ride";
    "tambourine", "Tambourine";
};
const LAST: [ParamInfo; 2] = [
    param("room", "Room", 0.0, 100.0, 0.0, "%"),
    param("crush", "Crush", 0.0, 100.0, 0.0, "%"),
];

const EFFECTS: [ParamInfo; 7] = [
    param("comp", "Compression", 0.0, 100.0, 0.0, "%"),
    param("low", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("high", "Treble", -12.0, 12.0, 0.0, "dB"),
    choice("delay_note", "Delay time", &crate::delay::NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 35.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    param("room_size", "Room size", 0.0, 100.0, 35.0, "%"),
];
const SHAPES: [ParamInfo; VOICES.len() * SHAPE_FIELDS] = pad_shapes! {
    "kick", "Kick";
    "rim", "Rim shot";
    "snare", "Snare";
    "clap", "Clap";
    "closed", "Closed hat";
    "open", "Open hat";
    "low_tom", "Low tom";
    "mid_tom", "Mid tom";
    "high_tom", "High tom";
    "cymbal", "Crash";
    "cowbell", "Cowbell";
    "clave", "Clave";
    "kick2", "Kick 2";
    "snare2", "Snare 2";
    "ride", "Ride";
    "tambourine", "Tambourine";
};

static PARAMS: [ParamInfo; COUNT] = {
    let mut out = [KIT[0]; COUNT];
    let mut i = 0;
    while i < 48 {
        out[i] = FIRST[i];
        i += 1;
    }
    out[48] = KIT[0];
    out[49] = KIT[1];
    out[50] = KIT[2];
    i = 0;
    while i < 16 {
        out[51 + i] = NEW[i];
        out[p::PANS + i] = MORE.0[i];
        out[p::MODELS + i] = MORE.1[i];
        i += 1;
    }
    out[p::ROOM] = LAST[0];
    out[p::CRUSH] = LAST[1];
    i = 0;
    while i < EFFECTS.len() {
        out[p::COMP + i] = EFFECTS[i];
        i += 1;
    }
    i = 0;
    while i < SHAPES.len() {
        out[p::SHAPES + i] = SHAPES[i];
        i += 1;
    }
    out
};

macro_rules! slots {
    ($($id:literal),*) => {
        [$(FileSlot { id: $id, name: "Sample", kind: FileKind::Audio, extensions: &["wav", "flac", "aif", "aiff", "ogg", "mp3"] },)*]
    };
}

/// One file per pad, played in place of its drum.
pub const SLOTS: [FileSlot; 16] = slots!(
    "kick",
    "rim",
    "snare",
    "clap",
    "closed",
    "open",
    "low_tom",
    "mid_tom",
    "high_tom",
    "cymbal",
    "cowbell",
    "clave",
    "kick2",
    "snare2",
    "ride",
    "tambourine"
);

pub const INFO: EffectInfo = EffectInfo {
    id: "drums",
    name: "Strada-Drums",
    params: &PARAMS,
    meters: &[""; VOICES.len()],
    instrument: true,
    files: &SLOTS,
    category: "Instrument",
    description: "Drum machine: sixteen pads in 808, 909 or acoustic character, or your own samples.",
};

/// The pad a MIDI note plays: its own, or the General MIDI neighbors of the same drum.
pub fn voice_of(key: u8) -> Option<usize> {
    Some(match key {
        36 => 0,
        37 => 1,
        38 => 2,
        39 => 3,
        42 | 44 => 4,
        46 => 5,
        41 | 43 | 45 => 6,
        47 | 48 => 7,
        50 => 8,
        49 | 52 | 55 | 57 => 9,
        56 => 10,
        75..=77 => 11,
        35 => 12,
        40 => 13,
        51 | 53 | 59 => 14,
        54 | 69 | 70 | 82 => 15,
        _ => return None,
    })
}

/// Level of each pad in each character, so that a kit is balanced at the same settings: kick
/// and snare in front, hats, cymbals and small percussion behind.
const BALANCE: [[f32; 3]; 16] = [
    [1.0, 0.912, 1.012],
    [0.892, 1.216, 1.95],
    [1.48, 2.213, 2.162],
    [1.819, 1.905, 3.091],
    [2.631, 2.63, 3.389],
    [3.311, 3.02, 3.673],
    [1.413, 1.38, 1.333],
    [1.397, 1.413, 1.413],
    [1.365, 1.38, 1.381],
    [3.236, 2.542, 5.559],
    [1.445, 1.445, 1.396],
    [1.096, 0.891, 0.989],
    [1.0, 0.912, 1.012],
    [1.48, 2.213, 2.162],
    [2.985, 2.985, 3.09],
    [1.122, 1.82, 1.737],
];

/// Level of the mix going into the drive and the crush.
const HEADROOM: f32 = 0.25;

/// Frequencies of the six square oscillators of the 808's hats and cymbal.
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Pass {
    #[default]
    Low,
    Band,
    High,
}

/// A drum as layers. Times in seconds, at decay 100 %; levels before the pad's level.
#[derive(Clone, Copy, Debug)]
struct Recipe {
    /// The tuned body: frequency (0 for none), decay and level.
    body: (f32, f32, f32),
    /// Its pitch starts this much higher (1 = an octave... times the note) and falls in this time.
    drop: (f32, f32),
    /// Two more modes: ratio to the body, decay and level.
    modes: [(f32, f32, f32); 2],
    /// Noise: filter, frequency, resonance, decay and level; then a low-pass over it (0 none).
    noise: (Pass, f32, f32, f32, f32),
    noise_low: f32,
    /// A short high-passed burst at the hit: decay and level.
    click: (f32, f32),
    /// Metallic bank: first and count of [`METAL`], their scale, sines rather than squares,
    /// filter, frequency, resonance, decay and level.
    metal: (usize, usize, f32, bool, Pass, f32, f32, f32, f32),
    /// Clap bursts before the tail: how many, and the gap between them.
    bursts: (u8, f32),
    drive: f32,
    /// Level for this recipe, so that pads balance at the same setting.
    gain: f32,
}

const NONE: Recipe = Recipe {
    body: (0.0, 0.1, 0.0),
    drop: (0.0, 0.01),
    modes: [(1.0, 0.1, 0.0); 2],
    noise: (Pass::Low, 1000.0, 0.7, 0.1, 0.0),
    noise_low: 0.0,
    click: (0.002, 0.0),
    metal: (0, 6, 1.0, false, Pass::High, 8000.0, 1.0, 0.1, 0.0),
    bursts: (0, 0.0),
    drive: 0.0,
    gain: 1.0,
};

/// The recipe of a drum in a character (0 808, 1 909, 2 acoustic).
fn recipe(kind: Kind, model: usize) -> Recipe {
    use Pass::*;
    let r = NONE;
    match (kind, model) {
        (Kind::Kick, 0) => Recipe {
            body: (50.0, 0.55, 1.0),
            drop: (2.0, 0.025),
            click: (0.004, 0.15),
            gain: 2.6,
            ..r
        },
        (Kind::Kick, 1) => Recipe {
            body: (54.0, 0.32, 1.0),
            drop: (3.5, 0.012),
            click: (0.006, 0.45),
            noise: (Band, 1500.0, 0.8, 0.02, 0.15),
            drive: 0.35,
            gain: 2.3,
            ..r
        },
        (Kind::Kick, _) => Recipe {
            body: (62.0, 0.28, 1.0),
            drop: (0.35, 0.02),
            modes: [(1.55, 0.09, 0.35), (2.3, 0.05, 0.15)],
            click: (0.003, 0.6),
            noise: (Low, 180.0, 0.8, 0.12, 0.35),
            drive: 0.15,
            gain: 2.2,
            ..r
        },
        (Kind::Snare, 0) => Recipe {
            body: (180.0, 0.12, 0.5),
            modes: [(1.83, 0.12, 0.5), (1.0, 0.1, 0.0)],
            noise: (High, 3500.0, 0.7, 0.2, 0.7),
            gain: 1.5,
            ..r
        },
        (Kind::Snare, 1) => Recipe {
            body: (190.0, 0.09, 0.6),
            drop: (0.4, 0.01),
            modes: [(1.75, 0.07, 0.35), (1.0, 0.1, 0.0)],
            noise: (High, 1800.0, 0.7, 0.18, 0.8),
            noise_low: 9000.0,
            click: (0.002, 0.2),
            drive: 0.2,
            gain: 1.4,
            ..r
        },
        (Kind::Snare, _) => Recipe {
            body: (200.0, 0.07, 0.55),
            drop: (0.15, 0.008),
            modes: [(1.5, 0.05, 0.3), (2.25, 0.035, 0.2)],
            noise: (Band, 4500.0, 0.5, 0.24, 0.9),
            click: (0.0015, 0.5),
            drive: 0.3,
            gain: 1.5,
            ..r
        },
        (Kind::Clap, 0) => Recipe {
            noise: (Band, 1100.0, 2.0, 0.18, 1.0),
            bursts: (3, 0.011),
            gain: 2.2,
            ..r
        },
        (Kind::Clap, 1) => Recipe {
            noise: (Band, 1200.0, 1.6, 0.25, 1.0),
            bursts: (4, 0.009),
            drive: 0.1,
            gain: 2.0,
            ..r
        },
        (Kind::Clap, _) => Recipe {
            noise: (Band, 1500.0, 1.0, 0.3, 1.0),
            bursts: (6, 0.007),
            gain: 1.8,
            ..r
        },
        (Kind::Closed | Kind::Open, m) => {
            let open = kind == Kind::Open;
            let decay = |closed: f32, opened: f32| if open { opened } else { closed };
            match m {
                0 => Recipe {
                    metal: (0, 6, 1.0, false, High, 8500.0, 1.2, decay(0.05, 0.4), 1.0),
                    noise: (High, 8500.0, 0.7, decay(0.05, 0.4), 0.15),
                    gain: 1.3,
                    ..r
                },
                1 => Recipe {
                    metal: (0, 6, 1.3, false, High, 9500.0, 1.0, decay(0.045, 0.35), 0.7),
                    noise: (High, 9000.0, 0.7, decay(0.05, 0.38), 0.5),
                    gain: 1.3,
                    ..r
                },
                _ => Recipe {
                    metal: (0, 6, 2.2, true, High, 7000.0, 0.8, decay(0.06, 0.45), 0.5),
                    noise: (Band, 9000.0, 0.6, decay(0.055, 0.45), 0.7),
                    click: (0.001, 0.3),
                    gain: 1.4,
                    ..r
                },
            }
        }
        (Kind::Tom(n), m) => {
            let f = [90.0, 130.0, 180.0][usize::from(n.min(2))];
            match m {
                0 => Recipe {
                    body: (f, 0.3, 1.0),
                    drop: (0.35, 0.06),
                    noise: (Low, 6000.0, 0.7, 0.05, 0.12),
                    gain: 1.6,
                    ..r
                },
                1 => Recipe {
                    body: (f * 1.05, 0.28, 1.0),
                    drop: (0.6, 0.04),
                    noise: (Low, 4000.0, 0.7, 0.06, 0.18),
                    click: (0.002, 0.2),
                    gain: 1.6,
                    ..r
                },
                _ => Recipe {
                    body: (f, 0.35, 1.0),
                    drop: (0.12, 0.08),
                    modes: [(1.5, 0.15, 0.35), (2.1, 0.08, 0.15)],
                    click: (0.002, 0.4),
                    noise: (Band, 1200.0, 0.6, 0.08, 0.15),
                    gain: 1.5,
                    ..r
                },
            }
        }
        (Kind::Crash, 0) => Recipe {
            metal: (0, 6, 1.0, false, High, 6500.0, 0.9, 1.4, 1.0),
            noise: (High, 6500.0, 0.7, 1.2, 0.2),
            gain: 1.15,
            ..r
        },
        (Kind::Crash, 1) => Recipe {
            metal: (0, 6, 1.25, false, High, 7000.0, 0.9, 1.6, 0.7),
            noise: (High, 6000.0, 0.6, 1.4, 0.6),
            gain: 1.1,
            ..r
        },
        (Kind::Crash, _) => Recipe {
            metal: (0, 6, 2.6, true, High, 5000.0, 0.7, 2.0, 0.5),
            noise: (Band, 7000.0, 0.4, 1.8, 0.7),
            click: (0.002, 0.3),
            gain: 1.1,
            ..r
        },
        (Kind::Ride, m) => Recipe {
            body: (3150.0, 0.6, 0.25),
            modes: [(1.47, 0.4, 0.15), (1.0, 0.1, 0.0)],
            metal: (
                0,
                6,
                [1.5, 1.8, 3.0][m.min(2)],
                m == 2,
                High,
                7000.0,
                0.9,
                1.0,
                0.7,
            ),
            noise: (Band, 9000.0, 0.4, 0.8, if m == 2 { 0.35 } else { 0.1 }),
            gain: 1.0,
            ..r
        },
        (Kind::Cowbell, 2) => Recipe {
            body: (560.0, 0.18, 0.7),
            modes: [(1.48, 0.12, 0.5), (2.6, 0.05, 0.2)],
            click: (0.002, 0.3),
            gain: 0.9,
            ..r
        },
        (Kind::Cowbell, _) => Recipe {
            metal: (4, 2, 1.0, false, Band, 2600.0, 3.0, 0.25, 1.0),
            noise_low: 3800.0,
            gain: 0.9,
            ..r
        },
        (Kind::Rim, 0) => Recipe {
            body: (1700.0, 0.012, 0.8),
            modes: [(0.29, 0.012, 0.8), (1.0, 0.1, 0.0)],
            noise: (High, 2000.0, 0.7, 0.006, 0.4),
            gain: 2.2,
            ..r
        },
        (Kind::Rim, 1) => Recipe {
            body: (1800.0, 0.015, 0.7),
            drop: (0.2, 0.002),
            modes: [(0.5, 0.01, 0.4), (1.0, 0.1, 0.0)],
            click: (0.002, 0.5),
            gain: 2.0,
            ..r
        },
        (Kind::Rim, _) => Recipe {
            body: (520.0, 0.04, 0.4),
            noise: (Band, 1800.0, 1.2, 0.03, 0.9),
            click: (0.001, 0.5),
            gain: 1.8,
            ..r
        },
        (Kind::Clave, 0) => Recipe {
            body: (2500.0, 0.03, 1.0),
            gain: 1.5,
            ..r
        },
        (Kind::Clave, 1) => Recipe {
            body: (2450.0, 0.035, 1.0),
            modes: [(1.62, 0.02, 0.3), (1.0, 0.1, 0.0)],
            gain: 1.5,
            ..r
        },
        (Kind::Clave, _) => Recipe {
            body: (1000.0, 0.05, 1.0),
            modes: [(2.8, 0.02, 0.3), (1.0, 0.1, 0.0)],
            noise: (Band, 2000.0, 2.0, 0.01, 0.3),
            gain: 1.5,
            ..r
        },
        (Kind::Tambourine, 0) => Recipe {
            noise: (High, 7000.0, 0.7, 0.09, 1.0),
            gain: 1.3,
            ..r
        },
        (Kind::Tambourine, 1) => Recipe {
            metal: (0, 6, 3.0, false, Band, 8000.0, 1.5, 0.15, 0.6),
            noise: (High, 8000.0, 0.7, 0.12, 0.6),
            gain: 1.3,
            ..r
        },
        (Kind::Tambourine, _) => Recipe {
            metal: (0, 6, 3.4, true, Band, 9000.0, 1.0, 0.22, 0.5),
            noise: (Band, 8500.0, 0.8, 0.2, 0.8),
            bursts: (2, 0.015),
            gain: 1.3,
            ..r
        },
    }
}

/// Trapezoidal state-variable filter (Simper), with coefficients set once per hit.
#[derive(Clone, Copy, Debug, Default)]
struct Filter {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    ic1: f32,
    ic2: f32,
}

impl Filter {
    fn set(&mut self, hz: f32, q: f32, rate: f32) {
        let g = (std::f32::consts::PI * hz.clamp(10.0, rate * 0.45) / rate).tan();
        self.k = 1.0 / q.max(0.1);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    fn run(&mut self, x: f32, pass: Pass) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match pass {
            Pass::Low => v2,
            Pass::Band => v1,
            Pass::High => x - self.k * v1 - v2,
        }
    }
}

/// One sounding pad: the recipe as played, its envelopes and its oscillators.
#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    active: bool,
    time: u32,
    gain: f32,
    pan: (f32, f32),
    /// Body frequency, its drop and drop time constant (samples), and its phases.
    frequency: f32,
    drop: f32,
    drop_time: f32,
    phase: [f32; 3],
    ratios: [f32; 2],
    /// Current levels and per-sample decay of body, modes, noise, click and metal.
    levels: [f32; 6],
    decays: [f32; 6],
    noise_pass: Pass,
    noise_filter: Filter,
    low: Option<Filter>,
    click_filter: Filter,
    metal: (usize, usize, bool, Pass),
    metal_steps: [f32; 6],
    metal_phase: [f32; 6],
    metal_filter: Filter,
    /// Clap bursts: count and gap in samples, with the tail's level.
    bursts: (u8, f32),
    tail: f32,
    drive: f32,
    /// A sample playing: where it is, its step, and when it fades.
    sample: Option<(f64, f64)>,
    fade: (f32, f32),
    tone: Option<(Filter, Pass)>,
    /// Attack in samples; pitch envelope in semitones, its per-sample falloff and its level.
    attack: f32,
    pitch: (f32, f32, f32),
    low_cut: Option<Filter>,
    high_cut: Option<Filter>,
    /// Delay and reverb sends.
    sends: (f32, f32),
}

/// A pad's own sound: its samples (mono) and their rate.
#[derive(Debug)]
struct Sample {
    data: Arc<Vec<f32>>,
    rate: f32,
}

pub struct Drums {
    rate: f32,
    values: [f32; COUNT],
    voices: [Voice; VOICES.len()],
    samples: [Option<Sample>; VOICES.len()],
    seed: u32,
    room: Box<dyn Effect>,
    delay: Box<dyn Effect>,
    comp: Box<dyn Effect>,
    crush: (f32, u32, f32),
    /// Low and high shelves per channel, their coefficients and the gains they were made for.
    eq: [Biquad; 4],
    shelves: (Coefficients, Coefficients),
    shelf_gains: (f32, f32),
    /// Delay and reverb send buses, left and right.
    bus: [Vec<f32>; 4],
}

/// Sets `effect`'s setting `name`, if the built-in effect `id` has it.
fn set_named(effect: &mut dyn Effect, id: &str, name: &str, value: f32) {
    if let Some(i) = crate::info(id).and_then(|info| info.params.iter().position(|q| q.id == name))
    {
        effect.set_param(i, value);
    }
}

impl std::fmt::Debug for Drums {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drums")
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

impl Default for Drums {
    fn default() -> Self {
        let mut room = crate::create("reverb").expect("built-in reverb");
        for (name, value) in [("type", 0.0), ("predelay", 4.0), ("mix", 100.0)] {
            set_named(&mut *room, "reverb", name, value);
        }
        let mut delay = crate::create("delay").expect("built-in delay");
        for (name, value) in [
            ("pingpong", 1.0),
            ("low_cut", 250.0),
            ("high_cut", 6000.0),
            ("mix", 100.0),
            ("sync", 1.0),
            ("wow", 0.0),
            ("drive", 0.0),
        ] {
            set_named(&mut *delay, "delay", name, value);
        }
        let mut comp = crate::create("compressor").expect("built-in compressor");
        for (name, value) in [
            ("type", 1.0),
            ("attack", 8.0),
            ("release", 120.0),
            ("knee", 6.0),
        ] {
            set_named(&mut *comp, "compressor", name, value);
        }
        let mut drums = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: [Voice::default(); VOICES.len()],
            samples: Default::default(),
            seed: 0x1234_5678,
            room,
            delay,
            comp,
            crush: (0.0, 0, 0.0),
            eq: [Biquad::default(); 4],
            shelves: (Coefficients::IDENTITY, Coefficients::IDENTITY),
            shelf_gains: (0.0, 0.0),
            bus: Default::default(),
        };
        for i in [p::COMP, p::DELAY_NOTE, p::DELAY_FEEDBACK, p::ROOM_SIZE] {
            drums.set_param(i, PARAMS[i].default);
        }
        drums
    }
}

/// Per-sample multiplier of an exponential decay lasting `seconds` (to about -60 dB).
fn falloff(seconds: f32, rate: f32) -> f32 {
    (-6.9 / (seconds.max(0.0005) * rate)).exp()
}

/// A band-limited square (polyBLEP): the 808's metal without the folded-back hiss of naive
/// squares at these high frequencies.
fn square(phase: f32, dt: f32) -> f32 {
    let blep = |t: f32| {
        if t < dt {
            let t = t / dt;
            2.0 * t - t * t - 1.0
        } else if t > 1.0 - dt {
            let t = (t - 1.0) / dt;
            t * t + 2.0 * t + 1.0
        } else {
            0.0
        }
    };
    let naive = if phase < 0.5 { 1.0 } else { -1.0 };
    naive + blep(phase) - blep((phase + 0.5).fract())
}

impl Drums {
    fn field(&self, pad: usize, k: usize) -> f32 {
        self.values[p::fields(pad) + k]
    }

    fn hit(&mut self, pad: usize, velocity: u8) {
        if pad == CLOSED {
            // The closed hat chokes the open one, as on the originals.
            self.voices[OPEN].active = false;
        }
        let rate = self.rate;
        let accent = self.values[p::ACCENT] / 100.0;
        let level = self.field(pad, 0) / 100.0;
        let ratio = 2f32.powf(self.field(pad, 1) / 12.0);
        let length = self.field(pad, 2) / 100.0;
        let tone = self.field(pad, 3) / 100.0;
        let model = (self.values[p::MODELS + pad].round().max(0.0) as usize).min(MODELS.len() - 1);
        let hard = f32::from(velocity) / 127.0;
        let velocity = 1.0 - accent + accent * hard;
        let pan = self.values[p::PANS + pad] / 100.0;
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let mut v = Voice {
            active: true,
            pan: (
                angle.cos() * std::f32::consts::SQRT_2,
                angle.sin() * std::f32::consts::SQRT_2,
            ),
            ..Voice::default()
        };
        if let Some(sample) = &self.samples[pad] {
            // Down for a darker tone, up for a thinner one.
            v.tone = if tone < 0.45 {
                let mut f = Filter::default();
                f.set(800.0 * 25f32.powf(tone / 0.45), 0.7, rate);
                Some((f, Pass::Low))
            } else if tone > 0.55 {
                let mut f = Filter::default();
                f.set(20.0 * 100f32.powf((tone - 0.55) / 0.45), 0.7, rate);
                Some((f, Pass::High))
            } else {
                None
            };
            let seconds = sample.data.len() as f32 / sample.rate / ratio;
            // Under 100 % the sample fades out early; over, it plays whole.
            v.fade = if length < 1.0 {
                (
                    0.6 * seconds * length * rate,
                    falloff(0.25 * seconds * length, rate),
                )
            } else {
                (f32::MAX, 1.0)
            };
            v.sample = Some((0.0, f64::from(sample.rate / rate * ratio)));
            v.gain = level * level * velocity * 1.4;
            self.shaping(pad, &mut v);
            self.voices[pad] = v;
            return;
        }
        let r = recipe(KINDS[pad], model);
        // Brighter with the tone, and a little with how hard the pad is hit.
        let bright = 2f32.powf((tone - 0.5) * 2.0) * (0.85 + 0.3 * hard);
        let loud = 0.5 + tone;
        v.gain = level * level * velocity * 1.4 * r.gain * BALANCE[pad][model];
        v.frequency = r.body.0 * ratio;
        v.drop = r.drop.0;
        v.drop_time = r.drop.1 * rate;
        v.ratios = [r.modes[0].0, r.modes[1].0];
        v.levels = [
            r.body.2 * (1.25 - 0.5 * tone),
            r.modes[0].2,
            r.modes[1].2,
            r.noise.4 * loud,
            r.click.1 * loud,
            r.metal.8,
        ];
        let tail = length.sqrt();
        v.decays = [
            falloff(r.body.1 * length, rate),
            falloff(r.modes[0].1 * length, rate),
            falloff(r.modes[1].1 * length, rate),
            falloff(r.noise.3 * if r.bursts.0 > 0 { tail } else { length }, rate),
            falloff(r.click.0, rate),
            falloff(r.metal.7 * length, rate),
        ];
        v.noise_pass = r.noise.0;
        v.noise_filter
            .set(r.noise.1 * bright * ratio.sqrt(), r.noise.2, rate);
        if r.noise_low > 0.0 {
            let mut f = Filter::default();
            f.set(r.noise_low * bright, 0.7, rate);
            v.low = Some(f);
        }
        v.click_filter.set(2500.0 * bright, 0.7, rate);
        let (first, count, scale, sines, pass, hz, q, _, _) = r.metal;
        v.metal = (first, count.min(6 - first), sines, pass);
        for (step, f) in v.metal_steps.iter_mut().zip(METAL) {
            *step = f * scale * ratio / rate;
        }
        v.metal_filter.set(hz * bright * ratio.sqrt(), q, rate);
        v.bursts = (r.bursts.0, r.bursts.1 * rate);
        v.tail = r.noise.4 * loud;
        v.drive = r.drive;
        self.shaping(pad, &mut v);
        self.voices[pad] = v;
    }

    /// The pad's attack, pitch envelope, filters and sends.
    fn shaping(&self, pad: usize, v: &mut Voice) {
        let at = p::shape(pad);
        let field = |k: usize| self.values[at + k];
        let rate = self.rate;
        v.attack = field(0) / 1000.0 * rate;
        v.pitch = (field(1), falloff(field(2) / 1000.0, rate), 1.0);
        let filter = |hz: f32| {
            let mut f = Filter::default();
            f.set(hz, std::f32::consts::FRAC_1_SQRT_2, rate);
            f
        };
        v.low_cut = (field(3) > 21.0).then(|| filter(field(3)));
        v.high_cut = (field(4) < 19_900.0).then(|| filter(field(4)));
        v.sends = (field(5) / 100.0, field(6) / 100.0);
    }

    /// The next sample of a pad, shaped, before its pan.
    fn render(&mut self, pad: usize) -> f32 {
        let mut x = self.source(pad);
        let v = &mut self.voices[pad];
        if v.attack > 0.0 && (v.time as f32) < v.attack {
            x *= v.time as f32 / v.attack;
        }
        if let Some(f) = &mut v.low_cut {
            x = f.run(x, Pass::High);
        }
        if let Some(f) = &mut v.high_cut {
            x = f.run(x, Pass::Low);
        }
        x
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// The next sample of a pad's drum or sample.
    fn source(&mut self, pad: usize) -> f32 {
        let rate = self.rate;
        let white = self.noise();
        let v = &mut self.voices[pad];
        let bend = if v.pitch.0 == 0.0 {
            1.0
        } else {
            let bend = 2f32.powf(v.pitch.0 * v.pitch.2 / 12.0);
            v.pitch.2 *= v.pitch.1;
            bend
        };
        if let Some((at, step)) = v.sample {
            let Some(sample) = &self.samples[pad] else {
                v.active = false;
                return 0.0;
            };
            let data = &sample.data;
            let i = at as usize;
            if i + 1 >= data.len() {
                v.active = false;
                return 0.0;
            }
            let frac = (at - i as f64) as f32;
            let mut x = data[i] + (data[i + 1] - data[i]) * frac;
            if let Some((filter, pass)) = &mut v.tone {
                x = filter.run(x, *pass);
            }
            v.sample = Some((at + step * f64::from(bend), step));
            let t = v.time as f32;
            v.time = v.time.saturating_add(1);
            if t > v.fade.0 {
                v.levels[0] = if v.levels[0] == 0.0 { 1.0 } else { v.levels[0] } * v.fade.1;
                if v.levels[0] < 1e-4 {
                    v.active = false;
                }
                x *= v.levels[0];
            }
            return x * v.gain;
        }
        let t = v.time as f32;
        v.time = v.time.saturating_add(1);
        let tau = std::f32::consts::TAU;
        let mut out = 0.0;
        if v.frequency > 0.0 {
            let f = v.frequency * (1.0 + v.drop * (-t / v.drop_time.max(1.0)).exp()) * bend;
            v.phase[0] = (v.phase[0] + f / rate).fract();
            out += (tau * v.phase[0]).sin() * v.levels[0];
            for k in 0..2 {
                if v.levels[k + 1] > 0.0 {
                    v.phase[k + 1] = (v.phase[k + 1] + f * v.ratios[k] / rate).fract();
                    out += (tau * v.phase[k + 1]).sin() * v.levels[k + 1];
                }
            }
        }
        if v.tail > 0.0 {
            let mut n = v.noise_filter.run(white, v.noise_pass);
            if let Some(low) = &mut v.low {
                n = low.run(n, Pass::Low);
            }
            let (count, gap) = v.bursts;
            let envelope = if count > 0 && t < f32::from(count) * gap {
                // The hands of a clap, a few ms apart, before the room's tail.
                let k = (t / gap).floor();
                let since = t - k * gap;
                v.tail * (-since / (0.004 * rate)).exp() * 1.6
            } else {
                v.levels[3]
            };
            out += n * envelope;
        }
        if v.levels[4] > 0.0 {
            out += v.click_filter.run(white, Pass::High) * v.levels[4];
        }
        if v.levels[5] > 0.0 {
            let (first, count, sines, pass) = v.metal;
            let mut metal = 0.0;
            for k in first..first + count {
                let step = v.metal_steps[k] * bend;
                v.metal_phase[k] = (v.metal_phase[k] + step).fract();
                metal += if sines {
                    (tau * v.metal_phase[k]).sin()
                } else {
                    square(v.metal_phase[k], step)
                };
            }
            out += v.metal_filter.run(metal / count.max(1) as f32, pass) * v.levels[5];
        }
        if v.drive > 0.0 {
            let push = 1.0 + 4.0 * v.drive;
            out = (out * push).tanh() / push.tanh();
        }
        let (count, gap) = v.bursts;
        let bursting = count > 0 && t < f32::from(count) * gap;
        if !bursting {
            v.levels[3] *= v.decays[3];
        }
        for k in [0, 1, 2, 4, 5] {
            v.levels[k] *= v.decays[k];
        }
        if (!bursting && v.levels.iter().fold(0.0f32, |m, l| m.max(*l)) < 1e-4) || !out.is_finite()
        {
            v.active = false;
            return 0.0;
        }
        out * v.gain
    }
}

/// The sound of pad `pad` with the settings `values` (in [`INFO`] order), dry and mono, at
/// 48 kHz, until it dies out or `seconds` have passed: for drawing it.
pub fn preview(values: &[f32], pad: usize, seconds: f32) -> Vec<f32> {
    let mut drums = Drums::default();
    for (i, v) in values.iter().enumerate().take(COUNT) {
        if i != p::ROOM && i != p::VOLUME && !(p::COMP..p::SHAPES).contains(&i) {
            drums.set_param(i, *v);
        }
    }
    drums.prepare(48_000, 512);
    drums.hit(pad.min(VOICES.len() - 1), 110);
    let mut out = Vec::new();
    while drums.voices.iter().any(|v| v.active) && out.len() < (seconds * 48_000.0) as usize {
        let (mut l, mut r) = ([0.0f32; 512], [0.0f32; 512]);
        drums.process(&mut l, &mut r);
        out.extend(l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)));
    }
    out
}

impl Effect for Drums {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES.len()];
        self.room.prepare(sample_rate, max_frames);
        self.delay.prepare(sample_rate, max_frames);
        self.comp.prepare(sample_rate, max_frames);
        self.bus = std::array::from_fn(|_| vec![0.0; max_frames]);
        self.shelf_gains = (f32::NAN, f32::NAN);
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.delay.set_tempo(bpm);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
        match index {
            p::ROOM_SIZE => {
                set_named(&mut *self.room, "reverb", "size", value);
                set_named(&mut *self.room, "reverb", "decay", 0.2 + 0.0143 * value);
            }
            p::DELAY_NOTE => set_named(&mut *self.delay, "delay", "note", value),
            p::DELAY_FEEDBACK => set_named(&mut *self.delay, "delay", "feedback", value),
            p::COMP => {
                // From a light glue to a squash that brings the room and tails up.
                let amount = value / 100.0;
                let threshold = -4.0 - 26.0 * amount;
                let ratio = 1.5 + 4.5 * amount;
                for (name, v) in [
                    ("threshold", threshold),
                    ("ratio", ratio),
                    ("makeup", 0.6 * (-threshold - 4.0) * (1.0 - 1.0 / ratio)),
                ] {
                    set_named(&mut *self.comp, "compressor", name, v);
                }
            }
            _ => {}
        }
    }

    fn load(&mut self, slot: &str, file: FileData) -> Result<String, String> {
        let pad = VOICES
            .iter()
            .position(|v| v.0 == slot)
            .ok_or_else(|| format!("no pad {slot}"))?;
        let FileData::Audio { channels, rate } = file else {
            return Err("a pad plays an audio file".into());
        };
        let length = channels.iter().map(Vec::len).max().unwrap_or(0);
        if length == 0 {
            return Err("the file is empty".into());
        }
        let n = channels.len().max(1) as f32;
        let mono: Vec<f32> = (0..length)
            .map(|i| {
                channels
                    .iter()
                    .map(|c| c.get(i).copied().unwrap_or(0.0))
                    .sum::<f32>()
                    / n
            })
            .collect();
        self.voices[pad].active = false;
        self.samples[pad] = Some(Sample {
            data: Arc::new(mono),
            rate: rate as f32,
        });
        Ok(format!("{:.2} s", length as f32 / rate as f32))
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((key, velocity)) = event.as_note_on()
            && let Some(index) = voice_of(key)
        {
            self.hit(index, velocity);
        } else if event
            .as_control()
            .is_some_and(|(cc, _)| cc == 120 || cc == 123)
        {
            for v in &mut self.voices {
                v.active = false;
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let drive = self.values[p::DRIVE] / 100.0;
        let volume = db_to_gain(self.values[p::VOLUME]);
        let push = 1.0 + 6.0 * drive;
        // Keeps the level about the same as the drive rises.
        let makeup = 1.0 / push.tanh().max(0.2) / (1.0 + drive);
        let crush = self.values[p::CRUSH] / 100.0;
        // Fewer bits and a lower rate as the crush rises: 16 bits to 6, full rate to an eighth.
        let steps = 2f32.powf(15.0 - 9.0 * crush);
        let hold = 1 + (7.0 * crush * crush) as u32;
        let shelves = (self.values[p::LOW], self.values[p::HIGH]);
        if shelves != self.shelf_gains {
            self.shelf_gains = shelves;
            let rate = f64::from(self.rate);
            self.shelves = (
                Coefficients::design(FilterKind::LowShelf, 110.0, 0.7, f64::from(shelves.0), rate),
                Coefficients::design(
                    FilterKind::HighShelf,
                    5500.0,
                    0.7,
                    f64::from(shelves.1),
                    rate,
                ),
            );
        }
        let equalize = shelves != (0.0, 0.0);
        let duck = 0.6 * self.values[p::ROOM] / 100.0;
        if self.bus[0].len() < n {
            for b in &mut self.bus {
                b.resize(n, 0.0);
            }
        }
        for b in &mut self.bus {
            b[..n].fill(0.0);
        }
        for i in 0..n {
            let (mut a, mut b) = (0.0f32, 0.0f32);
            let mut sends = [0.0f32; 4];
            for pad in 0..VOICES.len() {
                if self.voices[pad].active {
                    let x = self.render(pad);
                    let v = &self.voices[pad];
                    let (l, r) = (x * v.pan.0, x * v.pan.1);
                    // The room takes its share of what each pad sends it, as a wet/dry mix.
                    let dry = 1.0 - duck * v.sends.1;
                    a += l * dry;
                    b += r * dry;
                    sends[0] += l * v.sends.0;
                    sends[1] += r * v.sends.0;
                    sends[2] += l * v.sends.1;
                    sends[3] += r * v.sends.1;
                }
            }
            for (bus, send) in self.bus.iter_mut().zip(sends) {
                bus[i] = send * HEADROOM;
            }
            // The mix reaches the drive with headroom: a kick peaks near half scale there.
            a *= HEADROOM;
            b *= HEADROOM;
            if drive > 0.0 {
                a = (a * push).tanh() * makeup;
                b = (b * push).tanh() * makeup;
            }
            if crush > 0.0 {
                let (held, count, other) = &mut self.crush;
                if *count == 0 {
                    *held = (a * steps).round() / steps;
                    *other = (b * steps).round() / steps;
                }
                *count = (*count + 1) % hold;
                a = *held;
                b = *other;
            }
            if equalize {
                let (low, high) = &self.shelves;
                let [l1, l2, r1, r2] = &mut self.eq;
                a = l2.process(high, l1.process(low, f64::from(a))) as f32;
                b = r2.process(high, r1.process(low, f64::from(b))) as f32;
            }
            left[i] = a;
            right[i] = b;
        }
        if self.values[p::COMP] > 0.0 {
            self.comp.process(&mut left[..n], &mut right[..n]);
        }
        let echo = self.values[p::DELAY] / 100.0;
        let room = self.values[p::ROOM] / 100.0;
        let [dl, dr, rl, rr] = &mut self.bus;
        if echo > 0.0 {
            self.delay.process(&mut dl[..n], &mut dr[..n]);
        }
        if room > 0.0 {
            self.room.process(&mut rl[..n], &mut rr[..n]);
        }
        let out = volume * 0.5 / HEADROOM;
        let (echo, room) = (0.7 * echo, 0.6 * room);
        for i in 0..n {
            let a = left[i] + dl[i] * echo + rl[i] * room;
            let b = right[i] + dr[i] * echo + rr[i] * room;
            // A soft guard from -1 dBFS: hard hits never clip.
            left[i] = crate::transient::soft_clip(a * out);
            right[i] = crate::transient::soft_clip(b * out);
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.active = false;
        }
        self.room.reset();
        self.delay.reset();
        self.comp.reset();
        for f in &mut self.eq {
            f.reset();
        }
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip(&self.voices) {
            *o = if v.active {
                let level = if v.sample.is_some() {
                    if v.levels[0] > 0.0 { v.levels[0] } else { 1.0 }
                } else {
                    v.levels.iter().fold(0.0f32, |m, l| m.max(*l))
                };
                (level * v.gain).min(1.0)
            } else {
                0.0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(drums: &mut Drums, key: u8, blocks: usize) -> Vec<f32> {
        drums.midi(&MidiEvent::note_on(0, 9, key, 120));
        let mut out = Vec::new();
        for _ in 0..blocks {
            let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
            drums.process(&mut l, &mut r);
            out.extend(l);
        }
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn fresh(model: usize) -> Drums {
        let mut drums = Drums::default();
        for pad in 0..16 {
            drums.set_param(p::MODELS + pad, model as f32);
        }
        drums.prepare(48_000, 480);
        drums
    }

    #[test]
    fn every_pad_sounds_then_dies_out_in_every_character() {
        for model in 0..MODELS.len() {
            for (_, name, key) in VOICES {
                let mut drums = fresh(model);
                let out = play(&mut drums, key, 500);
                assert!(out.iter().all(|s| s.is_finite()), "{name} {model}");
                assert!(peak(&out[..4800]) > 0.02, "{name} {model} is heard");
                assert!(peak(&out[out.len() - 480..]) < 1e-3, "{name} {model} ends");
                assert!(
                    drums.voices.iter().all(|v| !v.active),
                    "{name} {model} stops"
                );
            }
        }
    }

    /// Loudest 10 ms of each pad, in dB, for balancing the recipes.
    #[test]
    fn pads_are_balanced() {
        // Kick and snare lead; hats, cymbals and small percussion sit lower, as in a kit.
        let wanted = |kind: Kind| match kind {
            Kind::Kick | Kind::Snare => 0.0,
            Kind::Clap | Kind::Tom(_) => -2.0,
            Kind::Rim => -5.0,
            Kind::Crash | Kind::Open => -7.0,
            _ => -9.0,
        };
        let mut report = Vec::new();
        for model in 0..MODELS.len() {
            for (pad, (_, name, key)) in VOICES.iter().enumerate() {
                let mut drums = fresh(model);
                let out = play(&mut drums, *key, 100);
                // The loudest 10 ms: a short hit is not lifted to fill a longer window.
                let db = out
                    .chunks(480)
                    .map(|c| {
                        10.0 * (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32)
                            .max(1e-12)
                            .log10()
                    })
                    .fold(f32::MIN, f32::max);
                let top = 20.0 * peak(&out).max(1e-9).log10();
                report.push((name, model, db, top, wanted(KINDS[pad])));
            }
        }
        let (kick, kick_peak) = (report[0].2, report[0].3);
        for (name, model, db, top, want) in &report {
            eprintln!(
                "{name} {}: {:.1} dB (wanted {want}) peak {:.1}",
                MODELS[*model],
                db - kick,
                top - kick_peak
            );
        }
        for (name, model, db, top, want) in &report {
            // Peaks capped by family: up to 3 dB over the kick's for the loud drums, lower for
            // hats, cymbals and small percussion, whose peaks sound softer than they measure.
            assert!(
                top - kick_peak < (3.0f32).min(want + 5.0) + 0.5,
                "{name} {}: peak {:.1}",
                MODELS[*model],
                top - kick_peak
            );
            assert!(
                db - kick - want < 4.0 && db - kick - want > -12.0,
                "{name} {}: {:.1}",
                MODELS[*model],
                db - kick
            );
        }
    }

    #[test]
    fn the_closed_hat_chokes_the_open_one() {
        let mut drums = fresh(0);
        play(&mut drums, 46, 2);
        assert!(drums.voices[OPEN].active);
        play(&mut drums, 42, 1);
        assert!(!drums.voices[OPEN].active);
    }

    #[test]
    fn the_kick_follows_tune_and_decay() {
        let length = |tune: f32, decay: f32| {
            let mut drums = fresh(0);
            drums.set_param(1, tune);
            drums.set_param(2, decay);
            let out = play(&mut drums, 36, 300);
            out.iter().rposition(|s| s.abs() > 1e-3).unwrap_or(0)
        };
        assert!(length(0.0, 300.0) > 2 * length(0.0, 100.0));
        let crossings = |tune: f32| {
            let mut drums = fresh(0);
            drums.set_param(1, tune);
            let out = play(&mut drums, 36, 40);
            out[9600..19200]
                .windows(2)
                .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
                .count()
        };
        let (low, high) = (crossings(0.0), crossings(12.0));
        assert!((high as f32 / low as f32 - 2.0).abs() < 0.2, "{low} {high}");
    }

    #[test]
    fn general_midi_neighbors_play_the_same_drum() {
        assert_eq!(voice_of(44), voice_of(42));
        assert_eq!(voice_of(60), None);
        for (index, (_, _, key)) in VOICES.iter().enumerate() {
            assert_eq!(voice_of(*key), Some(index));
        }
        let mut seen = LAYOUT.to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (0..16).collect::<Vec<_>>());
    }

    #[test]
    fn accent_follows_velocity() {
        let level = |velocity: u8| {
            let mut drums = fresh(0);
            drums.midi(&MidiEvent::note_on(0, 9, 38, velocity));
            let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
            drums.process(&mut l, &mut r);
            peak(&l)
        };
        assert!(level(127) > 1.5 * level(30));
    }

    #[test]
    fn a_pad_plays_its_sample_panned() {
        let mut drums = fresh(0);
        drums.set_param(p::PANS + KICK, 100.0);
        let tone: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        let told = drums
            .load(
                "kick",
                FileData::Audio {
                    channels: vec![tone],
                    rate: 48_000,
                },
            )
            .unwrap();
        assert_eq!(told, "0.10 s");
        drums.midi(&MidiEvent::note_on(0, 9, 36, 127));
        let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
        drums.process(&mut l, &mut r);
        assert!(peak(&r[..4800]) > 0.1 && peak(&l[..4800]) < 1e-3);
        assert!(peak(&r[5000..]) < 1e-6, "it ends with the file");
    }

    #[test]
    fn room_and_crush_change_the_sound() {
        let tail = |room: f32| {
            let mut drums = fresh(0);
            drums.set_param(p::ROOM, room);
            let out = play(&mut drums, 38, 200);
            out[48_000..].iter().map(|s| s * s).sum::<f32>()
        };
        assert!(tail(60.0) > tail(0.0) * 10.0 + 1e-9);
        let mut drums = fresh(0);
        drums.set_param(p::CRUSH, 100.0);
        let out = play(&mut drums, 38, 20);
        let distinct: std::collections::BTreeSet<i32> =
            out.iter().map(|s| (s * 1e6) as i32).collect();
        assert!(distinct.len() < out.len() / 4);
    }

    fn energy(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s * s).sum()
    }

    fn crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn attack_pitch_and_filters_shape_a_pad() {
        let shaped = |field: usize, value: f32, key: u8| {
            let mut drums = fresh(0);
            let pad = voice_of(key).unwrap();
            drums.set_param(p::shape(pad) + field, value);
            play(&mut drums, key, 50)
        };
        let plain = shaped(0, 0.0, 36);
        let slow = shaped(0, 40.0, 36);
        assert!(peak(&slow[..240]) < peak(&plain[..240]) * 0.3, "attack");
        let mut drums = fresh(0);
        drums.set_param(p::shape(0) + 1, 24.0);
        drums.set_param(p::shape(0) + 2, 400.0);
        let bent = play(&mut drums, 36, 50);
        assert!(
            crossings(&bent[..4800]) > 2 * crossings(&plain[..4800]),
            "pitch"
        );
        let thin = shaped(3, 1500.0, 36);
        assert!(energy(&thin) < energy(&plain) * 0.2, "low cut");
        let hat = shaped(4, 20_000.0, 42);
        let dark = shaped(4, 800.0, 42);
        assert!(energy(&dark) < energy(&hat) * 0.3, "high cut");
    }

    #[test]
    fn sends_feed_the_delay_and_room_returns() {
        let tail = |delay: f32, send: f32| {
            let mut drums = fresh(0);
            drums.set_tempo(120.0);
            drums.set_param(p::DELAY, delay);
            drums.set_param(p::shape(2) + 5, send);
            let out = play(&mut drums, 38, 100);
            energy(&out[24_000..])
        };
        assert!(tail(80.0, 100.0) > tail(0.0, 100.0) * 10.0 + 1e-9);
        assert!(tail(80.0, 0.0) < tail(80.0, 100.0) * 0.01);
    }

    #[test]
    fn compression_and_eq_keep_the_kit_near_its_level() {
        let loudness = |settings: &[(usize, f32)]| {
            let mut drums = fresh(0);
            for (i, v) in settings {
                drums.set_param(*i, *v);
            }
            let mut out = Vec::new();
            for key in [36, 42, 38, 42, 36, 42, 38, 46] {
                out.extend(play(&mut drums, key, 12));
            }
            10.0 * (energy(&out) / out.len() as f32).log10()
        };
        let plain = loudness(&[]);
        for settings in [
            &[(p::COMP, 50.0)][..],
            &[(p::COMP, 100.0)],
            &[(p::LOW, 6.0), (p::HIGH, -6.0)],
        ] {
            let level = loudness(settings);
            assert!(
                (level - plain).abs() < 4.0,
                "{settings:?}: {plain:.1} {level:.1}"
            );
        }
        assert!((loudness(&[(p::HIGH, 9.0)]) - plain).abs() > 0.5);
    }
}
