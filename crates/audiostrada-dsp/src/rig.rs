//! Guitar and bass rig in one insert: four pedals, an amp made of Neural Amp Modeler captures,
//! a tone stack, a cabinet (simulated or an impulse response) and, after the amp, the delay
//! and reverb pedals as in an effects loop.
//!
//! The amp is one of the built-in models (see `amp`) or NAM captures. A capture freezes the
//! amp at one setting: the amp's Gain knob steps between the captures
//! of its pack (sorted by the gain they report) with a short crossfade; the tone controls and
//! master come after the amp. A capture pedal's Drive works the same way.

use nam_rs::NamModel;

use crate::amp::{AmpSim, Knobs};
use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::chorus::Chorus;
use crate::compressor::Clean;
use crate::convolver::{Convolver, PARTITION};
use crate::delay::Delay;
use crate::flanger::Flanger;
use crate::nam::{NamCore, impulse};
use crate::phaser::Phaser;
use crate::plate::Plate;
use crate::stomp::{self, Stomp, Wah};
use crate::{
    Effect, EffectInfo, FileData, FileKind, FileSlot, ParamInfo, choice, db_to_gain, gain_to_db,
    param, switch, time_coefficient,
};

pub const INFO: EffectInfo = EffectInfo {
    id: "amp_rig",
    name: "Strada-Amp",
    params: &PARAMS,
    meters: &["Input", "Output"],
    instrument: false,
    files: &FILES,
    category: "Amp & Cab",
    description: "Pick an amp, add pedals, choose a cabinet: a whole guitar or bass rig.",
};

/// Captures of the built-in amps, clean to driven, by model (empty: modeled in `amp`).
/// From the Neural Amp Modeler community collection (GPL-3.0), see assets/amps/CREDITS.md.
/// In each pack, from the cleanest capture to the most driven, as measured (distortion of a
/// tone at -36, -24 and -12 dBFS): their files tell no gain to sort them by, and in file
/// order the Plexi's gain knob jumped to its most driven capture at 2.5.
pub const FACTORY: [&[&[u8]]; 16] = [
    &[
        include_bytes!("../assets/amps/amp01_0.bin"),
        include_bytes!("../assets/amps/amp01_1.bin"),
    ],
    &[],
    &[],
    &[],
    &[],
    &[
        include_bytes!("../assets/amps/amp06_1.bin"),
        include_bytes!("../assets/amps/amp06_0.bin"),
        include_bytes!("../assets/amps/amp06_2.bin"),
        include_bytes!("../assets/amps/amp06_3.bin"),
    ],
    &[],
    &[
        include_bytes!("../assets/amps/amp08_0.bin"),
        include_bytes!("../assets/amps/amp08_2.bin"),
        include_bytes!("../assets/amps/amp08_3.bin"),
        include_bytes!("../assets/amps/amp08_1.bin"),
        include_bytes!("../assets/amps/amp08_4.bin"),
    ],
    &[
        include_bytes!("../assets/amps/amp09_0.bin"),
        include_bytes!("../assets/amps/amp09_1.bin"),
        include_bytes!("../assets/amps/amp09_2.bin"),
        include_bytes!("../assets/amps/amp09_3.bin"),
        include_bytes!("../assets/amps/amp09_4.bin"),
        include_bytes!("../assets/amps/amp09_5.bin"),
        include_bytes!("../assets/amps/amp09_6.bin"),
        include_bytes!("../assets/amps/amp09_7.bin"),
    ],
    &[
        include_bytes!("../assets/amps/amp10_0.bin"),
        include_bytes!("../assets/amps/amp10_1.bin"),
        include_bytes!("../assets/amps/amp10_2.bin"),
        include_bytes!("../assets/amps/amp10_3.bin"),
    ],
    &[
        include_bytes!("../assets/amps/amp11_2.bin"),
        include_bytes!("../assets/amps/amp11_3.bin"),
        include_bytes!("../assets/amps/amp11_0.bin"),
        include_bytes!("../assets/amps/amp11_1.bin"),
        include_bytes!("../assets/amps/amp11_4.bin"),
        include_bytes!("../assets/amps/amp11_6.bin"),
        include_bytes!("../assets/amps/amp11_5.bin"),
        include_bytes!("../assets/amps/amp11_7.bin"),
    ],
    &[include_bytes!("../assets/amps/amp12_0.bin")],
    &[
        include_bytes!("../assets/amps/amp13_0.bin"),
        include_bytes!("../assets/amps/amp13_2.bin"),
        include_bytes!("../assets/amps/amp13_1.bin"),
    ],
    &[
        include_bytes!("../assets/amps/amp14_0.bin"),
        include_bytes!("../assets/amps/amp14_1.bin"),
        include_bytes!("../assets/amps/amp14_2.bin"),
        include_bytes!("../assets/amps/amp14_3.bin"),
    ],
    &[],
    &[],
];

/// Into each built-in amp (by `MODELS` index), in dB: the Plexi's captures crunched from
/// the first notch of the gain knob; 9 dB less into them, they clean up as its sibling the
/// Brit 45 does at the same settings.
const INPUT_PAD_DB: [f32; 16] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -9.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

/// Out of each built-in amp (by `MODELS` index), in dB: every guitar amp about as loud as the
/// others (-12 LUFS) on the same DI with its knobs at their defaults, its peaks under
/// -1 dBFS. The Deluxes came out 5 dB over the Voxy; the clean US Twin stays a little
/// quieter, its peaks being the highest. The bass amps, measured with a guitar, keep theirs.
const LEVEL_TRIM_DB: [f32; 16] = [
    0.7, -0.5, -3.2, -4.5, 1.8, 1.0, -0.7, 2.5, -0.2, 1.6, -1.8, -0.7, 1.1, 0.1, 0.0, 0.0,
];

/// Impulse responses of the simulated cabinets, at 48 kHz (Guitarix, GPL-2.0-or-later).
const CAB_IRS: [&[u8]; 9] = [
    include_bytes!("../assets/cabs/cab0.f32"),
    include_bytes!("../assets/cabs/cab1.f32"),
    include_bytes!("../assets/cabs/cab2.f32"),
    include_bytes!("../assets/cabs/cab3.f32"),
    include_bytes!("../assets/cabs/cab4.f32"),
    include_bytes!("../assets/cabs/cab5.f32"),
    include_bytes!("../assets/cabs/cab6.f32"),
    include_bytes!("../assets/cabs/cab7.f32"),
    include_bytes!("../assets/cabs/cab8.f32"),
];

/// A packed capture: its JSON without the weights, then the weights.
fn unpack(bytes: &[u8]) -> Option<NamModel> {
    let length = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let text = std::str::from_utf8(bytes.get(4..4 + length)?).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
    value["weights"] = serde_json::Value::Array(Vec::new());
    let mut model = NamModel::from_json_str(&value.to_string()).ok()?;
    model.weights = bytes[4 + length..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    Some(model)
}

/// The captures of every built-in amp, read once.
fn factory_models() -> &'static [Vec<NamModel>] {
    static MODELS: std::sync::OnceLock<Vec<Vec<NamModel>>> = std::sync::OnceLock::new();
    MODELS.get_or_init(|| {
        FACTORY
            .iter()
            .map(|files| files.iter().filter_map(|b| unpack(b)).collect())
            .collect()
    })
}

/// A cabinet's impulse response at `rate`, at the level of the others.
fn cab_ir(cab: usize, rate: f32) -> Vec<f32> {
    let Some(bytes) = CAB_IRS.get(cab) else {
        return Vec::new();
    };
    let source: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let step = 48_000.0 / rate;
    let length = (source.len() as f32 / step) as usize;
    let mut ir: Vec<f32> = (0..length)
        .map(|k| {
            let at = k as f32 * step;
            let i = at as usize;
            let frac = at - i as f32;
            let a = source[i];
            a + (source.get(i + 1).copied().unwrap_or(a) - a) * frac
        })
        .collect();
    // Its level as a guitar hears it: the power it passes from 100 Hz to 5 kHz.
    let power = (0..32)
        .map(|k| 100.0 * 50f32.powf(k as f32 / 31.0))
        .map(|f| {
            let w = std::f32::consts::TAU * f / rate;
            let (re, im) = ir.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, h)| {
                (re + h * (w * n as f32).cos(), im - h * (w * n as f32).sin())
            });
            re * re + im * im
        })
        .sum::<f32>()
        / 32.0;
    let level = power.sqrt();
    for h in &mut ir {
        *h /= level.max(1e-6);
    }
    ir
}

/// Pedal types of a slot, in parameter order (new types are added at the end).
pub const PEDALS: [&str; 22] = [
    "Empty",
    "Clean Boost",
    "Green Drive",
    "Orange Dist",
    "Vintage Fuzz",
    "Compressor",
    "Chorus",
    "Phaser",
    "Flanger",
    "Tremolo",
    "Tape Delay",
    "Reverb",
    "Capture",
    "Gold Drive",
    "Blue Drive",
    "Black Dist",
    "Big Fuzz",
    "Metal Dist",
    "Treble Boost",
    "Wah",
    "Analog Delay",
    "Yellow Drive",
];
const DELAY: usize = 10;
const REVERB: usize = 11;
const CAPTURE: usize = 12;
const WAH: usize = 19;
const ANALOG_DELAY: usize = 20;

/// Whether a pedal type plays after the amp, in its effects loop (delays and reverb).
pub fn in_loop(kind: usize) -> bool {
    matches!(kind, DELAY | REVERB | ANALOG_DELAY)
}

/// The pedal each type is modeled on.
pub fn pedal_based_on(kind: usize) -> &'static str {
    match kind {
        1 => "MXR Micro Amp",
        2 => "Ibanez TS808 / TS9 Tube Screamer",
        3 => "Boss DS-1",
        4 => "Dallas Arbiter Fuzz Face",
        5 => "MXR Dyna Comp",
        6 => "Boss CE-2",
        7 => "MXR Phase 90",
        8 => "MXR Flanger",
        9 => "Boss TR-2",
        10 => "Maestro Echoplex",
        11 => "EMT 140 plate",
        12 => "a NAM capture of your library",
        13 => "Klon Centaur",
        14 => "Boss BD-2 Blues Driver",
        15 => "ProCo RAT",
        16 => "Electro-Harmonix Big Muff Pi",
        17 => "Boss MT-2 Metal Zone",
        18 => "Dallas Rangemaster",
        19 => "Dunlop Cry Baby",
        20 => "MXR Carbon Copy",
        21 => "Boss SD-1",
        _ => "",
    }
}

/// Pedal types by family, in menu order.
pub const PEDAL_GROUPS: [(&str, &[usize]); 4] = [
    ("Drive", &[1, 18, 2, 21, 14, 13, 3, 15, 17, 4, 16]),
    ("Dynamics and filter", &[5, 19]),
    ("Modulation", &[6, 7, 8, 9]),
    ("Delay and reverb", &[20, 10, 11]),
];

pub const CABS: [&str; 11] = [
    "4x12 V30",
    "4x12 Greenback",
    "2x12 Blue (open)",
    "2x12 US (open)",
    "4x10 US (open)",
    "1x12 US (open)",
    "4x10 bass",
    "8x10 bass",
    "1x15 bass",
    "Direct",
    "Impulse response",
];
const DIRECT: usize = 9;
const IMPULSE: usize = 10;

/// Amp models: the loaded captures, then the built-in amps of `amp::MODELS`.
pub const AMPS: [&str; 17] = [
    "Capture",
    "US Twin",
    "US Super 4x10",
    "US Deluxe",
    "Tweed Deluxe 57",
    "Tweed Bass 59",
    "Voxy 15 Top Boost",
    "Brit 45 Bluesbreaker",
    "Brit Plexi 100",
    "Brit 900",
    "Brit Hot Mod",
    "Boutique 59",
    "Cali Mark IV",
    "Panama 52",
    "Brit Lead 100",
    "Bass Classic",
    "Bass Flip 15",
];

/// What the three knobs of a pedal type do.
pub fn knob_names(kind: usize) -> [&'static str; 3] {
    match kind {
        1 => ["Level", "Tone", ""],
        2 | 21 => ["Drive", "Tone", "Level"],
        3 | 17 => ["Dist", "Tone", "Level"],
        4 => ["Fuzz", "Tone", "Volume"],
        13 => ["Gain", "Treble", "Output"],
        14 => ["Gain", "Tone", "Level"],
        15 => ["Distortion", "Filter", "Volume"],
        16 => ["Sustain", "Tone", "Volume"],
        18 => ["Boost", "Tone", "Level"],
        19 => ["Pedal", "Auto", "Q"],
        20 => ["Time", "Repeats", "Mix"],
        5 => ["Sustain", "Attack", "Level"],
        6 => ["Rate", "Depth", "Mix"],
        7 | 8 => ["Rate", "Depth", "Feedback"],
        9 => ["Rate", "Depth", "Shape"],
        10 => ["Time", "Repeats", "Mix"],
        11 => ["Decay", "Tone", "Mix"],
        12 => ["Drive", "Tone", "Level"],
        _ => ["", "", ""],
    }
}

const fn knob(id: &'static str, name: &'static str) -> ParamInfo {
    param(id, name, 0.0, 10.0, 5.0, "")
}

const PARAMS: [ParamInfo; PARAM_COUNT] = [
    param("input", "Input", -24.0, 24.0, 0.0, "dB"),
    param("gate", "Gate (-100 = off)", -100.0, -20.0, -100.0, "dB"),
    knob("gain", "Gain"),
    knob("bass", "Bass"),
    knob("mid", "Middle"),
    knob("treble", "Treble"),
    knob("presence", "Presence"),
    param("master", "Master", -24.0, 12.0, 0.0, "dB"),
    choice("cab", "Cabinet", &CABS, 0.0),
    switch("normalize", "Normalize loudness", 1.0),
    choice("p1_type", "Pedal 1", &PEDALS, 0.0),
    switch("p1_on", "Pedal 1 on", 1.0),
    knob("p1_k1", "Pedal 1 knob 1"),
    knob("p1_k2", "Pedal 1 knob 2"),
    knob("p1_k3", "Pedal 1 knob 3"),
    choice("p2_type", "Pedal 2", &PEDALS, 0.0),
    switch("p2_on", "Pedal 2 on", 1.0),
    knob("p2_k1", "Pedal 2 knob 1"),
    knob("p2_k2", "Pedal 2 knob 2"),
    knob("p2_k3", "Pedal 2 knob 3"),
    choice("p3_type", "Pedal 3", &PEDALS, 0.0),
    switch("p3_on", "Pedal 3 on", 1.0),
    knob("p3_k1", "Pedal 3 knob 1"),
    knob("p3_k2", "Pedal 3 knob 2"),
    knob("p3_k3", "Pedal 3 knob 3"),
    choice("p4_type", "Pedal 4", &PEDALS, 0.0),
    switch("p4_on", "Pedal 4 on", 1.0),
    knob("p4_k1", "Pedal 4 knob 1"),
    knob("p4_k2", "Pedal 4 knob 2"),
    knob("p4_k3", "Pedal 4 knob 3"),
    choice("model", "Amp model", &AMPS, 2.0),
    choice("p5_type", "Pedal 5", &PEDALS, 0.0),
    switch("p5_on", "Pedal 5 on", 1.0),
    knob("p5_k1", "Pedal 5 knob 1"),
    knob("p5_k2", "Pedal 5 knob 2"),
    knob("p5_k3", "Pedal 5 knob 3"),
    choice("p6_type", "Pedal 6", &PEDALS, 0.0),
    switch("p6_on", "Pedal 6 on", 1.0),
    knob("p6_k1", "Pedal 6 knob 1"),
    knob("p6_k2", "Pedal 6 knob 2"),
    knob("p6_k3", "Pedal 6 knob 3"),
    choice("p7_type", "Pedal 7", &PEDALS, 0.0),
    switch("p7_on", "Pedal 7 on", 1.0),
    knob("p7_k1", "Pedal 7 knob 1"),
    knob("p7_k2", "Pedal 7 knob 2"),
    knob("p7_k3", "Pedal 7 knob 3"),
    choice("p8_type", "Pedal 8", &PEDALS, 0.0),
    switch("p8_on", "Pedal 8 on", 1.0),
    knob("p8_k1", "Pedal 8 knob 1"),
    knob("p8_k2", "Pedal 8 knob 2"),
    knob("p8_k3", "Pedal 8 knob 3"),
];
const MODEL: usize = 30;
pub const SLOTS: usize = 8;
const PARAM_COUNT: usize = 31 + (SLOTS - 4) * 5;

/// First parameter of a pedal slot: slots 5 to 8 came after the amp model.
const fn slot_base(index: usize) -> usize {
    if index < 4 {
        10 + index * 5
    } else {
        31 + (index - 4) * 5
    }
}

const FILES: [FileSlot; 2 + SLOTS] = [
    FileSlot {
        id: "amp",
        name: "Amp captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal1",
        name: "Pedal 1 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal2",
        name: "Pedal 2 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal3",
        name: "Pedal 3 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal4",
        name: "Pedal 4 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal5",
        name: "Pedal 5 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal6",
        name: "Pedal 6 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal7",
        name: "Pedal 7 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "pedal8",
        name: "Pedal 8 captures",
        kind: FileKind::Pack,
        extensions: &["nam"],
    },
    FileSlot {
        id: "ir",
        name: "Cabinet IR",
        kind: FileKind::Audio,
        extensions: &["wav", "flac", "aif", "aiff"],
    },
];

const TARGET_LOUDNESS: f32 = -18.0;
/// Most the normalization raises a capture. A clean amp captured quietly (the US Twin's at
/// -40 dB) has the peaks of a clean amp: raised by its whole 22 dB, a guitar DI peaking at
/// -6 dBFS came out at +10.
const MAX_BOOST_DB: f32 = 12.0;
const FADE_MS: f32 = 30.0;
/// How long the output stays muted when the amp changes, for the DC blocker and the
/// cabinet to settle on its offset.
const SETTLE_MS: f32 = 50.0;

/// Captures of one piece of gear at several settings, sorted by gain.
struct Pack {
    cores: Vec<NamCore>,
    current: usize,
    previous: Option<usize>,
    /// Samples the capture switched to still runs alongside the one heard, on the same
    /// input, before the crossfade: its state is then its own, not a thump from a blank one
    /// or an echo of what it last played.
    preroll: usize,
    /// Each capture's polarity, as heard: captures are not all made the same way round, and
    /// a crossfade between two of opposite polarity cancelled to nearly nothing halfway (the
    /// US Twin's, a hole at 5 on the gain knob). Set from how the one switched to answers
    /// the same input as the one heard, during the preroll.
    signs: Vec<f32>,
    agreement: f64,
    fade: usize,
    fade_len: usize,
    scratch: Vec<f32>,
}

impl Pack {
    fn new(mut models: Vec<NamModel>) -> Self {
        models.sort_by(|a, b| {
            let gain = |m: &NamModel| m.metadata_typed().gain.unwrap_or(f32::MAX);
            gain(a).total_cmp(&gain(b))
        });
        let signs = vec![1.0; models.len()];
        Self {
            cores: models.into_iter().map(NamCore::new).collect(),
            current: 0,
            previous: None,
            preroll: 0,
            signs,
            agreement: 0.0,
            fade: 0,
            fade_len: 1,
            scratch: Vec::new(),
        }
    }

    /// Warms the capture in use (see `NamCore::warm`), when preparing.
    fn warm(&mut self, max: usize) {
        if let Some(core) = self.cores.get_mut(self.current) {
            core.warm(max);
        }
    }

    /// Whether the capture in use has run long enough to be heard.
    fn is_warm(&self) -> bool {
        self.cores.get(self.current).is_none_or(NamCore::is_warm)
    }

    /// Samples the capture in use needs, from a blank state, before it can be heard.
    fn warm_len(&self) -> usize {
        self.cores.get(self.current).map_or(0, NamCore::warm_len)
    }

    fn prepare(&mut self, rate: u32, max: usize) {
        for core in &mut self.cores {
            core.prepare(rate, max);
        }
        self.scratch = vec![0.0; max];
        self.fade_len = (FADE_MS * 0.001 * rate as f32) as usize + 1;
        self.previous = None;
        self.preroll = 0;
    }

    fn index(&self, knob: f32) -> usize {
        let last = self.cores.len().saturating_sub(1);
        ((knob / 10.0).clamp(0.0, 1.0) * last as f32).round() as usize
    }

    /// Puts the capture for `knob` in use at once, without a crossfade (when preparing).
    fn jump(&mut self, knob: f32) {
        self.current = self.index(knob);
        self.previous = None;
        self.preroll = 0;
    }

    /// The capture for a knob from 0 to 10.
    fn select(&mut self, knob: f32) {
        let index = self.index(knob);
        if index != self.current && self.previous.is_none() {
            self.previous = Some(self.current);
            self.current = index;
            self.preroll = self.cores[index].warm_len();
            self.agreement = 0.0;
            self.fade = self.fade_len;
        }
    }

    fn level(&self, index: usize, normalize: bool) -> f32 {
        match (normalize, self.cores[index].loudness()) {
            (true, Some(loudness)) => db_to_gain((TARGET_LOUDNESS - loudness).min(MAX_BOOST_DB)),
            _ => 1.0,
        }
    }

    fn latency(&self) -> usize {
        self.cores.iter().map(NamCore::latency).max().unwrap_or(0)
    }

    fn reset(&mut self) {
        for core in &mut self.cores {
            core.reset();
        }
        self.previous = None;
    }

    fn process(&mut self, buffer: &mut [f32], normalize: bool) {
        let n = buffer.len().min(self.scratch.len());
        let buffer = &mut buffer[..n];
        let now = self.level(self.current, normalize) * self.signs[self.current];
        match self.previous {
            Some(old) => {
                let before = self.level(old, normalize) * self.signs[old];
                self.scratch[..n].copy_from_slice(buffer);
                self.cores[old].process(&mut self.scratch[..n]);
                self.cores[self.current].process(buffer);
                if self.preroll > 0 {
                    // The new capture runs, unheard, until its state is its own; meanwhile,
                    // whether it answers the way round the one heard does.
                    self.agreement += buffer
                        .iter()
                        .zip(&self.scratch[..n])
                        .map(|(s, o)| f64::from(s * now * o * before))
                        .sum::<f64>();
                    self.preroll = self.preroll.saturating_sub(n);
                    if self.preroll == 0 && self.agreement < 0.0 {
                        self.signs[self.current] = -self.signs[self.current];
                    }
                    for (s, o) in buffer.iter_mut().zip(&self.scratch[..n]) {
                        *s = o * before;
                    }
                    return;
                }
                for (s, o) in buffer.iter_mut().zip(&self.scratch[..n]) {
                    let t = self.fade as f32 / self.fade_len as f32;
                    *s = *s * now * (1.0 - t) + o * before * t;
                    self.fade = self.fade.saturating_sub(1);
                }
                if self.fade == 0 {
                    self.previous = None;
                }
            }
            None => {
                self.cores[self.current].process(buffer);
                buffer.iter_mut().for_each(|s| *s *= now);
            }
        }
    }
}

/// One pedal slot: a processor for every type, so changing type never allocates.
struct Slot {
    stomp: Stomp,
    wah: Wah,
    compressor: Clean,
    chorus: Chorus,
    phaser: Phaser,
    flanger: Flanger,
    delay: Delay,
    reverb: Plate,
    capture: Option<Pack>,
    tone: Coefficients,
    filters: [Biquad; 2],
    gain: f32,
    tremolo: f32,
    /// Type and knobs the processors were last set for.
    applied: [f32; 4],
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            stomp: Stomp::default(),
            wah: Wah::default(),
            compressor: Clean::default(),
            chorus: Chorus::default(),
            phaser: Phaser::default(),
            flanger: Flanger::default(),
            delay: Delay::default(),
            reverb: Plate::default(),
            capture: None,
            tone: Coefficients::IDENTITY,
            filters: [Biquad::default(); 2],
            gain: 1.0,
            tremolo: 0.0,
            applied: [f32::NAN; 4],
        }
    }
}

impl Slot {
    fn prepare(&mut self, rate: u32, max: usize) {
        for effect in [
            &mut self.compressor as &mut dyn Effect,
            &mut self.chorus,
            &mut self.phaser,
            &mut self.flanger,
            &mut self.delay,
            &mut self.reverb,
        ] {
            effect.prepare(rate, max);
        }
        if let Some(pack) = &mut self.capture {
            pack.prepare(rate, max);
        }
        self.applied = [f32::NAN; 4];
    }

    fn reset(&mut self) {
        for effect in [
            &mut self.compressor as &mut dyn Effect,
            &mut self.chorus,
            &mut self.phaser,
            &mut self.flanger,
            &mut self.delay,
            &mut self.reverb,
        ] {
            effect.reset();
        }
        if let Some(pack) = &mut self.capture {
            pack.reset();
        }
        self.stomp.reset();
        self.wah.reset();
        self.filters = [Biquad::default(); 2];
    }

    /// Sets the processor of `kind` from the knobs (0 to 10) when they changed.
    fn apply(&mut self, kind: usize, knobs: [f32; 3], rate: f32) {
        let now = [kind as f32, knobs[0], knobs[1], knobs[2]];
        if now == self.applied {
            return;
        }
        self.applied = now;
        let [a, b, c] = knobs.map(|k| (k / 10.0).clamp(0.0, 1.0));
        let set = |effect: &mut dyn Effect, values: &[(usize, f32)]| {
            for &(i, v) in values {
                effect.set_param(i, v);
            }
        };
        let shelf = |gain: f32| {
            Coefficients::design(
                FilterKind::HighShelf,
                1800.0,
                0.7,
                f64::from(gain),
                f64::from(rate),
            )
        };
        match kind {
            1 => {
                self.gain = db_to_gain(a * 20.0);
                self.tone = shelf((b - 0.5) * 12.0);
            }
            2..=4 | 13..=18 | 21 => {
                let circuit = match kind {
                    2 => &stomp::GREEN,
                    3 => &stomp::ORANGE,
                    4 => &stomp::VINTAGE_FUZZ,
                    13 => &stomp::GOLD,
                    14 => &stomp::BLUE,
                    15 => &stomp::BLACK,
                    16 => &stomp::BIG_FUZZ,
                    17 => &stomp::METAL,
                    18 => &stomp::TREBLE,
                    _ => &stomp::YELLOW,
                };
                // The RAT's filter darkens clockwise.
                let tone = if kind == 15 { 1.0 - b } else { b };
                self.stomp.set(circuit, a, tone, c, rate);
            }
            WAH => self.wah.set(a, b, c, rate),
            5 => set(
                &mut self.compressor,
                &[
                    (0, -a * 40.0),
                    (1, 4.0),
                    (2, 1.0 + b * 49.0),
                    (3, 150.0),
                    (5, c * 18.0),
                    (6, 100.0),
                ],
            ),
            6 => set(
                &mut self.chorus,
                &[(0, 0.1 + a * a * 5.0), (1, b * 100.0), (4, c * 100.0)],
            ),
            7 => set(
                &mut self.phaser,
                &[
                    (0, 0.05 + a * a * 6.0),
                    (1, b * 100.0),
                    (4, c * 90.0),
                    (7, 50.0),
                ],
            ),
            8 => set(
                &mut self.flanger,
                &[
                    (0, 0.05 + a * a * 3.0),
                    (1, b * 100.0),
                    (3, (c * 2.0 - 1.0) * 90.0),
                ],
            ),
            DELAY | ANALOG_DELAY => set(
                &mut self.delay,
                &[
                    (0, 60.0 + a * 940.0),
                    (1, b * 90.0),
                    (5, c * 60.0),
                    (8, if kind == DELAY { 1.0 } else { 2.0 }),
                    (9, if kind == DELAY { 25.0 } else { 10.0 }),
                    (10, 30.0),
                ],
            ),
            REVERB => set(
                &mut self.reverb,
                &[
                    (0, 15.0),
                    (1, (0.2 + a * 0.78) / 0.98 * 100.0),
                    (2, 2000.0 + b * 14_000.0),
                    (8, c * 60.0),
                ],
            ),
            CAPTURE => {
                self.tone = shelf((b - 0.5) * 16.0);
                self.gain = db_to_gain(-18.0 + c * 30.0);
            }
            _ => {}
        }
    }

    /// Runs `kind` on mono audio, collapsing stereo effects back to mono.
    fn process_mono(
        &mut self,
        kind: usize,
        knobs: [f32; 3],
        buffer: &mut [f32],
        side: &mut [f32],
        rate: f32,
        normalize: bool,
    ) {
        match kind {
            1 => {
                for s in buffer.iter_mut() {
                    *s = self.filters[0].process(&self.tone, f64::from(*s * self.gain)) as f32;
                }
            }
            9 => {
                let step = (1.0 + knobs[0] / 10.0 * 11.0) / rate;
                let depth = knobs[1] / 10.0;
                let shape = 1.0 + knobs[2] / 10.0 * 8.0;
                for s in buffer.iter_mut() {
                    self.tremolo = (self.tremolo + step).fract();
                    let wave = (shape * (self.tremolo * std::f32::consts::TAU).sin()).tanh()
                        / shape.tanh();
                    *s *= 1.0 - depth * 0.5 * (1.0 - wave);
                }
            }
            CAPTURE => {
                if let Some(pack) = &mut self.capture {
                    pack.select(knobs[0]);
                    pack.process(buffer, normalize);
                }
                for s in buffer.iter_mut() {
                    *s = self.filters[0].process(&self.tone, f64::from(*s * self.gain)) as f32;
                }
            }
            2..=4 | 13..=18 | 21 => self.stomp.process(buffer),
            WAH => self.wah.process(buffer),
            _ => {
                let effect: &mut dyn Effect = match kind {
                    5 => &mut self.compressor,
                    6 => &mut self.chorus,
                    7 => &mut self.phaser,
                    8 => &mut self.flanger,
                    _ => return,
                };
                side.copy_from_slice(buffer);
                effect.process(buffer, side);
                for (s, o) in buffer.iter_mut().zip(side.iter()) {
                    *s = 0.5 * (*s + o);
                }
            }
        }
    }
}

pub struct Rig {
    values: [f32; PARAM_COUNT],
    rate: u32,
    amp: Option<Pack>,
    builtin: AmpSim,
    /// The captures of each built-in amp that has them.
    factory: Vec<Option<Pack>>,
    slots: [Slot; SLOTS],
    ir: Option<(Vec<f32>, u32)>,
    convolver: Option<Convolver>,
    /// Delay matching the convolver while another cabinet is used.
    dry: Vec<f32>,
    dry_at: usize,
    mono: Vec<f32>,
    side: Vec<f32>,
    tone: [Coefficients; 4],
    tone_filters: [Biquad; 4],
    /// The simulated cabinets' impulse responses at the current rate, and the signal lately.
    cab_irs: Vec<Vec<f32>>,
    cab_history: Vec<f32>,
    cab_at: usize,
    dc: (f32, f32),
    /// The DC blocker's state on silence, after the amp's offset: where a reset puts it.
    dc_rest: (f32, f32),
    /// The amp model last heard; samples the output stays muted while a model just chosen
    /// warms up (see `NamCore::warm_len`), then fades in over.
    heard_model: usize,
    muted: usize,
    unmute: usize,
    gate_env: f32,
    gate_gain: f32,
    dirty: bool,
    meters: [f32; 2],
}

impl Default for Rig {
    fn default() -> Self {
        Self {
            values: PARAMS.map(|p| p.default),
            rate: 48_000,
            amp: None,
            builtin: AmpSim::default(),
            factory: factory_models()
                .iter()
                .map(|models| (!models.is_empty()).then(|| Pack::new(models.clone())))
                .collect(),
            slots: Default::default(),
            ir: None,
            convolver: None,
            dry: Vec::new(),
            dry_at: 0,
            mono: Vec::new(),
            side: Vec::new(),
            tone: [Coefficients::IDENTITY; 4],
            tone_filters: [Biquad::default(); 4],
            cab_irs: Vec::new(),
            cab_history: Vec::new(),
            cab_at: 0,
            dc: (0.0, 0.0),
            dc_rest: (0.0, 0.0),
            heard_model: usize::MAX,
            muted: 0,
            unmute: 0,
            gate_env: 0.0,
            gate_gain: 1.0,
            dirty: true,
            meters: [-120.0; 2],
        }
    }
}

impl std::fmt::Debug for Rig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rig")
            .field("amp", &self.amp.as_ref().map(|p| p.cores.len()))
            .finish_non_exhaustive()
    }
}

/// Knob 0 to 10 as a boost or cut of up to 12 dB, flat at 5.
fn tone_db(knob: f32) -> f64 {
    f64::from((knob - 5.0) / 5.0 * 12.0)
}

impl Rig {
    fn update(&mut self) {
        let rate = f64::from(self.rate.max(1));
        let v = self.values;
        let design = |kind, f: f64, q, g| Coefficients::design(kind, f, q, g, rate);
        self.tone = [
            design(FilterKind::LowShelf, 120.0, 0.7, tone_db(v[3])),
            design(FilterKind::Peak, 650.0, 0.7, tone_db(v[4])),
            design(FilterKind::HighShelf, 2800.0, 0.7, tone_db(v[5])),
            design(FilterKind::HighShelf, 5500.0, 0.7, tone_db(v[6]) * 0.75),
        ];
        let model = v[MODEL].round().max(0.0) as usize;
        if let Some(index) = model.checked_sub(1) {
            let knobs = Knobs {
                gain: v[2],
                bass: v[3],
                mid: v[4],
                treble: v[5],
                presence: v[6],
            };
            self.builtin.update(index, knobs, rate);
        }
        self.dirty = false;
    }

    fn slot(&self, index: usize) -> (usize, bool, [f32; 3]) {
        let base = slot_base(index);
        let kind = self.values[base]
            .round()
            .clamp(0.0, (PEDALS.len() - 1) as f32) as usize;
        let on = self.values[base + 1] >= 0.5;
        (
            kind,
            on,
            [
                self.values[base + 2],
                self.values[base + 3],
                self.values[base + 4],
            ],
        )
    }

    /// Number of captures loaded for the amp, if any.
    pub fn captures(&self) -> usize {
        self.amp.as_ref().map_or(0, |p| p.cores.len())
    }
}

fn pack_from(data: FileData) -> Result<(Pack, String), String> {
    let files = match data {
        FileData::Pack(files) => files,
        FileData::Data(bytes) => vec![bytes],
        FileData::Audio { .. } => return Err("not NAM captures".into()),
    };
    let models: Vec<NamModel> = files
        .into_iter()
        .map(NamCore::parse)
        .collect::<Result<_, _>>()?;
    if models.is_empty() {
        return Err("no captures".into());
    }
    let meta = models[0].metadata_typed();
    let name = [meta.gear_make, meta.gear_model]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let count = models.len();
    let text = match (name.is_empty(), count) {
        (true, 1) => "1 capture".to_owned(),
        (true, n) => format!("{n} captures"),
        (false, 1) => name,
        (false, n) => format!("{name} · {n} captures"),
    };
    Ok((Pack::new(models), text))
}

impl Effect for Rig {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate.max(1);
        let max = max_frames.max(1);
        self.mono = vec![0.0; max];
        self.side = vec![0.0; max];
        if let Some(amp) = &mut self.amp {
            amp.prepare(self.rate, max);
        }
        for pack in self.factory.iter_mut().flatten() {
            pack.prepare(self.rate, max);
        }
        self.cab_irs = (0..DIRECT).map(|c| cab_ir(c, self.rate as f32)).collect();
        let longest = self.cab_irs.iter().map(Vec::len).max().unwrap_or(1).max(1);
        self.cab_history = vec![0.0; longest];
        self.cab_at = 0;
        for slot in &mut self.slots {
            slot.prepare(self.rate, max);
        }
        self.convolver = self.ir.as_ref().map(|(samples, rate)| {
            Convolver::new(&crate::resample::Resampler::convert(
                samples, *rate, self.rate,
            ))
        });
        self.dry = vec![0.0; PARTITION];
        self.dry_at = 0;
        self.tone_filters = [Biquad::default(); 4];
        self.update();
        // Only the captures in use are warmed here: all of them would take most of a second.
        // Another one warms when chosen, unheard (see `Pack::select`, `heard_model`).
        let gain = self.values[2];
        let model = self.values[MODEL].round().max(0.0) as usize;
        if let Some(pack) = model
            .checked_sub(1)
            .and_then(|i| self.factory.get_mut(i))
            .and_then(Option::as_mut)
        {
            pack.jump(gain);
            pack.warm(max);
        }
        if let Some(amp) = &mut self.amp {
            amp.jump(gain);
            amp.warm(max);
        }
        for index in 0..SLOTS {
            let (_, _, knobs) = self.slot(index);
            if let Some(pack) = &mut self.slots[index].capture {
                pack.jump(knobs[0]);
                pack.warm(max);
            }
        }
        self.heard_model = model;
        self.muted = 0;
        self.unmute = 0;
        // The DC blocker and the cabinet settle on the amp's offset before any sound.
        self.dc = (0.0, 0.0);
        let (mut left, mut right) = (vec![0.0f32; max], vec![0.0f32; max]);
        let mut done = 0;
        while done < self.rate as usize / 10 {
            left.fill(0.0);
            right.fill(0.0);
            self.process(&mut left, &mut right);
            done += max;
        }
        self.dc_rest = self.dc;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index)
            && *v != value
        {
            *v = value;
            self.dirty = true;
        }
    }

    fn latency(&self) -> u32 {
        let capture = self.slots.iter().filter_map(|s| s.capture.as_ref());
        let model = self.values[MODEL].round().max(0.0) as usize;
        let amp = match model.checked_sub(1) {
            Some(i) => self
                .factory
                .get(i)
                .and_then(Option::as_ref)
                .map_or(0, Pack::latency),
            None => self.amp.as_ref().map_or(0, Pack::latency),
        };
        (amp + capture.map(Pack::latency).max().unwrap_or(0)) as u32
            + self.convolver.as_ref().map_or(0, Convolver::latency)
    }

    fn reset(&mut self) {
        if let Some(amp) = &mut self.amp {
            amp.reset();
        }
        self.builtin.reset();
        for pack in self.factory.iter_mut().flatten() {
            pack.reset();
        }
        for slot in &mut self.slots {
            slot.reset();
        }
        if let Some(c) = &mut self.convolver {
            c.reset();
        }
        self.dry.fill(0.0);
        self.tone_filters = [Biquad::default(); 4];
        self.cab_history.fill(0.0);
        // The DC blocker goes back to the amp's offset on silence, as the captures keep
        // theirs (see `NamCore::reset`): cleared, it would step, and the cabinet ring with it.
        self.dc = self.dc_rest;
        self.gate_env = 0.0;
        self.gate_gain = 1.0;
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip(self.meters) {
            *o = v;
        }
    }

    fn load(&mut self, slot: &str, file: FileData) -> Result<String, String> {
        match slot {
            "amp" => {
                let (pack, text) = pack_from(file)?;
                self.amp = Some(pack);
                Ok(text)
            }
            _ if slot.starts_with("pedal") => {
                let index = slot[5..]
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=SLOTS).contains(n))
                    .ok_or_else(|| format!("unknown file slot {slot}"))?
                    - 1;
                let (pack, text) = pack_from(file)?;
                self.slots[index].capture = Some(pack);
                Ok(text)
            }
            "ir" => match file {
                FileData::Audio { channels, rate } => {
                    let mono = impulse(&channels, rate)?;
                    let frames = mono.len();
                    self.ir = Some((mono, rate));
                    Ok(format!("{} ms", frames * 1000 / rate.max(1) as usize))
                }
                _ => Err("not an impulse response".into()),
            },
            _ => Err(format!("unknown file slot {slot}")),
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.dirty {
            self.update();
        }
        let n = left.len().min(right.len()).min(self.mono.len());
        let rate = self.rate as f32;
        let [input, gate, gain, _, _, _, _, master, cab, normalize] =
            *self.values.first_chunk::<10>().unwrap_or(&[0.0; 10]);
        let normalize = normalize >= 0.5;
        let gain_in = db_to_gain(input);
        let mut peak_in = 0.0f32;
        for (m, (l, r)) in self.mono[..n].iter_mut().zip(left.iter().zip(right.iter())) {
            *m = 0.5 * (l + r) * gain_in;
            peak_in = peak_in.max(m.abs());
        }
        if gate > -99.5 {
            let threshold = db_to_gain(gate);
            let (attack, release) = (time_coefficient(1.0, rate), time_coefficient(80.0, rate));
            let smooth = time_coefficient(5.0, rate);
            for s in &mut self.mono[..n] {
                let level = s.abs();
                let c = if level > self.gate_env {
                    attack
                } else {
                    release
                };
                self.gate_env = level + c * (self.gate_env - level);
                let target = if self.gate_env > threshold { 1.0 } else { 0.0 };
                self.gate_gain = target + smooth * (self.gate_gain - target);
                *s *= self.gate_gain;
            }
        }
        // Pedals in front of the amp; delay and reverb wait for the effects loop.
        for index in 0..SLOTS {
            let (kind, on, knobs) = self.slot(index);
            self.slots[index].apply(kind, knobs, rate);
            if on && kind != 0 && !matches!(kind, DELAY | REVERB | ANALOG_DELAY) {
                let (mono, side) = (&mut self.mono[..n], &mut self.side[..n]);
                self.slots[index].process_mono(kind, knobs, mono, side, rate, normalize);
            }
        }
        let model = self.values[MODEL].round().max(0.0) as usize;
        if model != self.heard_model {
            // Another amp: unheard until its captures have run long enough (a blank one
            // thumps) and the DC blocker has followed its offset, then faded in.
            self.heard_model = model;
            let pack = match model.checked_sub(1) {
                None => self.amp.as_mut(),
                Some(i) => self.factory.get_mut(i).and_then(Option::as_mut),
            };
            let mut muted = (SETTLE_MS * 0.001 * rate) as usize;
            if let Some(pack) = pack {
                pack.select(gain);
                if !pack.is_warm() {
                    muted += pack.warm_len();
                }
            }
            self.muted = muted;
        }
        let factory = model
            .checked_sub(1)
            .and_then(|i| self.factory.get_mut(i))
            .and_then(Option::as_mut);
        let (pad, trim) = model.checked_sub(1).map_or((0.0, 0.0), |i| {
            (
                INPUT_PAD_DB.get(i).copied().unwrap_or(0.0),
                LEVEL_TRIM_DB.get(i).copied().unwrap_or(0.0),
            )
        });
        if pad != 0.0 {
            let pad = db_to_gain(pad);
            self.mono[..n].iter_mut().for_each(|s| *s *= pad);
        }
        if let Some(pack) = factory {
            pack.select(gain);
            pack.process(&mut self.mono[..n], normalize);
        } else if model >= 1 {
            self.builtin.process(&mut self.mono[..n]);
        } else if let Some(amp) = &mut self.amp {
            amp.select(gain);
            amp.process(&mut self.mono[..n], normalize);
        }
        let (mut x1, mut y1) = self.dc;
        let cab = cab.round().max(0.0) as usize;
        let simulated = cab < DIRECT && !self.cab_history.is_empty();
        let captured = model == 0
            || model
                .checked_sub(1)
                .is_some_and(|i| self.factory.get(i).is_some_and(Option::is_some));
        for s in &mut self.mono[..n] {
            // DC blocker, tone stack, then the cabinet simulation.
            let y = *s - x1 + 0.995 * y1;
            x1 = *s;
            y1 = y;
            let mut v = f64::from(y);
            // Built-in amps have their own tone stack; captures get this EQ after them.
            if captured {
                for (f, c) in self.tone_filters.iter_mut().zip(&self.tone) {
                    v = f.process(c, v);
                }
            }
            if simulated {
                // The cabinet's impulse response, sample by sample: short enough to need no
                // look-ahead.
                let len = self.cab_history.len();
                self.cab_history[self.cab_at] = v as f32;
                let ir = &self.cab_irs[cab];
                let mut acc = 0.0f32;
                for (k, h) in ir.iter().enumerate() {
                    acc += h * self.cab_history[(self.cab_at + len - k) % len];
                }
                self.cab_at = (self.cab_at + 1) % len;
                v = f64::from(acc);
            }
            *s = v as f32;
        }
        self.dc = (x1, y1);
        if let Some(convolver) = &mut self.convolver {
            if cab == IMPULSE {
                convolver.process(&mut self.mono[..n]);
            } else {
                for s in &mut self.mono[..n] {
                    std::mem::swap(s, &mut self.dry[self.dry_at]);
                    self.dry_at = (self.dry_at + 1) % PARTITION;
                }
            }
        }
        let out = db_to_gain(master + trim);
        let fade_len = (FADE_MS * 0.001 * rate) as usize + 1;
        for i in 0..n {
            let ramp = if self.muted > 0 {
                self.muted -= 1;
                self.unmute = fade_len;
                0.0
            } else if self.unmute > 0 {
                self.unmute -= 1;
                1.0 - self.unmute as f32 / fade_len as f32
            } else {
                1.0
            };
            let s = self.mono[i] * out * ramp;
            left[i] = s;
            right[i] = s;
        }
        // Effects loop, in stereo.
        for index in 0..SLOTS {
            let (kind, on, _) = self.slot(index);
            if on && matches!(kind, DELAY | REVERB | ANALOG_DELAY) {
                let slot = &mut self.slots[index];
                if kind != REVERB {
                    slot.delay.process(&mut left[..n], &mut right[..n]);
                } else {
                    slot.reverb.process(&mut left[..n], &mut right[..n]);
                }
            }
        }
        let peak_out = left[..n]
            .iter()
            .chain(&right[..n])
            .fold(0.0f32, |m, s| m.max(s.abs()));
        self.meters = [gain_to_db(peak_in), gain_to_db(peak_out)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-layer WaveNet computing a fixed nonlinear function (from the nam-rs docs).
    fn tiny(gain: f32) -> Vec<u8> {
        format!(
            r#"{{
            "version": "0.5.4", "architecture": "WaveNet",
            "metadata": {{"gain": {gain}, "gear_make": "Test", "gear_model": "Amp"}},
            "config": {{
                "layers": [{{
                    "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                    "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                    "gated": false, "head_bias": false
                }}],
                "head": null, "head_scale": 1.0
            }},
            "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, {gain}]
        }}"#
        )
        .into_bytes()
    }

    fn render(rig: &mut Rig, blocks: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for b in 0..blocks {
            let mut l: Vec<f32> = (0..256)
                .map(|i| (((b * 256 + i) as f32) * 0.03).sin() * 0.4)
                .collect();
            let mut r = l.clone();
            rig.process(&mut l, &mut r);
            out.extend(l);
        }
        out
    }

    #[test]
    fn every_pedal_and_cabinet_is_stable() {
        for (kind, pedal) in PEDALS.iter().enumerate() {
            for (cab, cabinet) in CABS.iter().enumerate() {
                let mut rig = Rig::default();
                rig.set_param(slot_base(0), kind as f32);
                rig.set_param(slot_base(0) + 2, 10.0);
                rig.set_param(slot_base(0) + 3, 10.0);
                rig.set_param(slot_base(0) + 4, 10.0);
                rig.set_param(8, cab as f32);
                rig.prepare(48_000, 256);
                let out = render(&mut rig, 100);
                assert!(
                    out.iter().all(|s| s.is_finite() && s.abs() < 8.0),
                    "{pedal} through {cabinet}"
                );
            }
        }
    }

    #[test]
    fn a_built_in_amp_glides_to_a_new_gain() {
        // A held note while the gain jumps from 0 to 10: the waveform bends no sharper than
        // the note does at the new gain, no click from the stages changing at once.
        let mut rig = Rig::default();
        rig.set_param(MODEL, 4.0);
        rig.set_param(2, 0.0);
        rig.set_param(8, DIRECT as f32);
        rig.prepare(48_000, 256);
        let mut t = 0usize;
        let mut block = |rig: &mut Rig| {
            let mut l: Vec<f32> = (0..256)
                .map(|i| 0.1 * ((t + i) as f32 * std::f32::consts::TAU * 110.0 / 48_000.0).sin())
                .collect();
            t += 256;
            let mut r = l.clone();
            rig.process(&mut l, &mut r);
            l
        };
        // A click is a break in the waveform's slope: its second difference.
        let steepest = |x: &[f32]| {
            x.windows(3)
                .fold(0.0f32, |m, w| m.max((w[2] - 2.0 * w[1] + w[0]).abs()))
        };
        for _ in 0..60 {
            block(&mut rig);
        }
        rig.set_param(2, 10.0);
        let change: Vec<f32> = (0..4).flat_map(|_| block(&mut rig)).collect();
        for _ in 0..60 {
            block(&mut rig);
        }
        let steady: Vec<f32> = (0..4).flat_map(|_| block(&mut rig)).collect();
        let (jump, held) = (steepest(&change), steepest(&steady));
        assert!(
            jump < held * 1.5,
            "a break of {jump} where the note bends by {held}"
        );
    }

    #[test]
    fn a_guitar_comes_out_near_its_level() {
        // A plucked note peaking at -6 dBFS, as from a DI: no amp may push it past +3 dBFS.
        // The US Twin, raised by its capture's whole 22 dB, put it at +10.
        // The pick's attack: a short burst of noise, then the string's tones.
        let mut seed = 1u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 23) as f32 - 1.0
        };
        let note: Vec<f32> = (0..48_000)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                let tones = (std::f32::consts::TAU * 196.0 * t).sin()
                    + 0.4 * (std::f32::consts::TAU * 392.0 * t).sin();
                0.5 * ((-t * 4.0).exp() * tones / 1.4 + (-t * 300.0).exp() * noise())
                    .clamp(-1.0, 1.0)
            })
            .collect();
        for (model, name) in AMPS.iter().enumerate().skip(1) {
            let mut rig = Rig::default();
            rig.set_param(MODEL, model as f32);
            rig.prepare(48_000, 256);
            let mut peak = 0.0f32;
            for block in note.chunks(256) {
                let (mut l, mut r) = (block.to_vec(), block.to_vec());
                rig.process(&mut l, &mut r);
                peak = l.iter().fold(peak, |m, s| m.max(s.abs()));
            }
            let db = 20.0 * peak.log10();
            assert!(db < 3.0, "{name} peaks at {db:.1} dBFS");
        }
    }

    #[test]
    fn playback_starts_in_silence() {
        // From a blank state, captures answered silence with a thump: -6 dBFS for the
        // Panama 52, and up to +9 dBFS on a guitar's first note.
        for (model, name) in AMPS.iter().enumerate().skip(1) {
            let mut rig = Rig::default();
            rig.set_param(MODEL, model as f32);
            rig.prepare(48_000, 256);
            let loudest = |rig: &mut Rig| {
                let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
                let mut most = 0.0f32;
                for _ in 0..40 {
                    l.fill(0.0);
                    r.fill(0.0);
                    rig.process(&mut l, &mut r);
                    most = l.iter().chain(&r).fold(most, |m, s| m.max(s.abs()));
                }
                most
            };
            assert!(loudest(&mut rig) < 1e-4, "{name} starts with a thump");
            // A locate or a stop resets the effects: still silent after.
            rig.reset();
            assert!(loudest(&mut rig) < 1e-4, "{name} thumps after a reset");
            // Another amp chosen, or another capture of the pack by the gain knob, while
            // playing: warmed unheard first.
            rig.set_param(MODEL, (model % (AMPS.len() - 1) + 1) as f32);
            assert!(loudest(&mut rig) < 1e-4, "{name}: the next amp thumps");
            rig.set_param(2, 10.0);
            assert!(loudest(&mut rig) < 1e-4, "{name}: its gain thumps");
        }
    }

    #[test]
    fn every_built_in_capture_and_cabinet_reads() {
        for (model, files) in FACTORY.iter().enumerate() {
            assert_eq!(
                factory_models()[model].len(),
                files.len(),
                "{}",
                AMPS[model + 1]
            );
            for capture in &factory_models()[model] {
                assert!(nam_rs::Model::from_nam(capture).is_ok());
                assert!(capture.loudness().is_some());
            }
        }
        for (cab, name) in CABS.iter().enumerate().take(DIRECT) {
            let ir = cab_ir(cab, 44_100.0);
            assert!(ir.len() > 50 && ir.iter().all(|h| h.is_finite()), "{name}");
        }
    }

    #[test]
    fn built_in_amps_stay_bounded_and_level() {
        let names: Vec<&str> = crate::amp::MODELS.iter().map(|m| m.name).collect();
        assert_eq!(&AMPS[1..], names.as_slice());
        for model in crate::amp::MODELS.iter() {
            assert!(model.cab < DIRECT, "{} has a simulated cabinet", model.name);
        }
        for (model, name) in AMPS.iter().enumerate().skip(1) {
            let mut levels = Vec::new();
            for gain in [0.0, 5.0, 10.0] {
                let mut rig = Rig::default();
                rig.set_param(MODEL, model as f32);
                rig.set_param(2, gain);
                rig.set_param(8, DIRECT as f32);
                rig.prepare(48_000, 256);
                let out = render(&mut rig, 40);
                assert!(
                    out.iter().all(|s| s.is_finite() && s.abs() < 4.0),
                    "{name} at {gain}"
                );
                let tail = &out[out.len() / 2..];
                levels.push((tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt());
            }
            let (low, high) = (levels[0], levels[2]);
            assert!(low > 0.02, "{name} is heard: {levels:?}");
            assert!(
                high / low < 4.0 && low / high < 4.0,
                "{name} keeps its level: {levels:?}"
            );
        }
    }

    #[test]
    #[ignore = "prints the tonal balance of each amp with its cabinet"]
    fn print_balance() {
        let band = |kind: FilterKind, f: f64| Coefficients::design(kind, f, 0.9, 0.0, 48_000.0);
        for (index, model) in crate::amp::MODELS.iter().enumerate() {
            let mut rig = Rig::default();
            rig.set_param(MODEL, (index + 1) as f32);
            rig.set_param(8, model.cab as f32);
            rig.prepare(48_000, 256);
            // Pink-ish noise: white noise through a gentle low-pass.
            let mut seed = 1u32;
            let mut last = 0.0f32;
            let mut out = Vec::new();
            for _ in 0..400 {
                let mut l: Vec<f32> = (0..256)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        let white = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
                        last = 0.97 * last + 0.03 * white;
                        last * 2.0
                    })
                    .collect();
                let mut r = l.clone();
                rig.process(&mut l, &mut r);
                out.extend(l);
            }
            let energy = |f: f64| {
                let (lo, hi) = (
                    band(FilterKind::HighPass, f / 1.6),
                    band(FilterKind::LowPass, f * 1.6),
                );
                let (mut a, mut b) = (Biquad::default(), Biquad::default());
                let tail = &out[out.len() / 4..];
                let sum: f64 = tail
                    .iter()
                    .map(|s| {
                        let v = b.process(&hi, a.process(&lo, f64::from(*s)));
                        v * v
                    })
                    .sum();
                10.0 * (sum / tail.len() as f64).log10()
            };
            let (low, mid, high) = (energy(140.0), energy(700.0), energy(3000.0));
            println!(
                "{:22} low-mid {:+5.1} high-mid {:+5.1}",
                model.name,
                low - mid,
                high - mid
            );
        }
    }

    #[test]
    fn gain_steps_through_the_captures_sorted_by_gain() {
        let mut rig = Rig::default();
        let text = rig
            .load("amp", FileData::Pack(vec![tiny(0.9), tiny(0.1), tiny(0.5)]))
            .unwrap();
        assert_eq!(text, "Test Amp · 3 captures");
        assert_eq!(rig.captures(), 3);
        rig.set_param(MODEL, 0.0);
        rig.set_param(8, DIRECT as f32);
        rig.prepare(48_000, 256);
        let gains: Vec<f32> = rig
            .amp
            .as_ref()
            .unwrap()
            .cores
            .iter()
            .map(|c| c.gain().unwrap())
            .collect();
        assert_eq!(gains, vec![0.1, 0.5, 0.9]);
        render(&mut rig, 4);
        assert_eq!(
            rig.amp.as_ref().unwrap().current,
            1,
            "knob at 5 of 0..10 on 3 captures"
        );
        rig.set_param(2, 0.0);
        // The capture switched to runs unheard for 120 ms at least, then fades in over 30.
        let out = render(&mut rig, 40);
        assert!(out.iter().all(|s| s.is_finite()));
        assert_eq!(rig.amp.as_ref().unwrap().current, 0);
        assert!(
            rig.amp.as_ref().unwrap().previous.is_none(),
            "crossfade finished"
        );
        assert!(
            rig.load("amp", FileData::Pack(vec![b"nope".to_vec()]))
                .is_err()
        );
    }

    #[test]
    fn simulated_cabinets_roll_off_the_highs() {
        let tone = |f: f32, cab: usize| {
            let mut rig = Rig::default();
            rig.set_param(MODEL, 0.0);
            rig.set_param(8, cab as f32);
            rig.prepare(48_000, 256);
            let mut peak = 0.0f32;
            for b in 0..200 {
                let mut l: Vec<f32> = (0..256)
                    .map(|i| {
                        0.1 * ((b * 256 + i) as f32 * f * std::f32::consts::TAU / 48_000.0).sin()
                    })
                    .collect();
                let mut r = l.clone();
                rig.process(&mut l, &mut r);
                if b > 100 {
                    peak = l.iter().fold(peak, |m, s| m.max(s.abs()));
                }
            }
            peak
        };
        for (cab, name) in CABS.iter().enumerate().take(DIRECT) {
            let ratio = tone(10_000.0, cab) / tone(1_000.0, cab);
            assert!(ratio < 0.1, "{name}: {ratio}");
        }
        assert!(
            (tone(10_000.0, DIRECT) - 0.1).abs() < 0.02,
            "direct is flat"
        );
    }

    #[test]
    fn every_pedal_type_names_its_model_knobs_and_menu_family() {
        for (kind, pedal) in PEDALS.iter().enumerate().skip(1) {
            assert!(!pedal_based_on(kind).is_empty(), "{pedal}");
            assert!(!knob_names(kind)[0].is_empty(), "{pedal}");
            let listed = PEDAL_GROUPS
                .iter()
                .flat_map(|(_, kinds)| kinds.iter())
                .filter(|&&k| k == kind)
                .count();
            assert_eq!(listed, usize::from(kind != CAPTURE), "{pedal}");
        }
        assert_eq!(pedal_based_on(0), "");
        assert_eq!(knob_names(0), ["", "", ""]);
    }

    #[test]
    fn files_load_into_their_slots() {
        let mut rig = Rig::default();
        assert_eq!(
            rig.load("pedal1", FileData::Data(tiny(0.5))),
            Ok("Test Amp".into())
        );
        assert!(rig.slots[0].capture.is_some());
        assert!(rig.load("pedal9", FileData::Data(tiny(0.5))).is_err());
        assert!(rig.load("pedalx", FileData::Data(tiny(0.5))).is_err());
        assert!(rig.load("cab", FileData::Data(tiny(0.5))).is_err());
        assert!(rig.load("amp", FileData::Pack(Vec::new())).is_err());
        let audio = || FileData::Audio {
            channels: vec![vec![0.0; 4]],
            rate: 48_000,
        };
        assert!(rig.load("amp", audio()).is_err());
        assert!(rig.load("ir", FileData::Data(Vec::new())).is_err());
        let ir = FileData::Audio {
            channels: vec![vec![1.0; 4800]],
            rate: 48_000,
        };
        assert_eq!(rig.load("ir", ir), Ok("100 ms".into()));
        assert_eq!(rig.load("ir", audio()), Ok("0 ms".into()));
        assert!(format!("{rig:?}").starts_with("Rig"));
    }

    #[test]
    fn an_impulse_response_replaces_the_cabinet_and_keeps_the_latency() {
        let mut rig = Rig::default();
        let mut ir = vec![0.0; 64];
        ir[0] = 1.0;
        rig.load(
            "ir",
            FileData::Audio {
                channels: vec![ir],
                rate: 48_000,
            },
        )
        .unwrap();
        for cab in [IMPULSE, DIRECT] {
            rig.set_param(8, cab as f32);
            rig.prepare(48_000, 256);
            assert_eq!(rig.latency(), PARTITION as u32, "{}", CABS[cab]);
            let out = render(&mut rig, 40);
            assert!(out[..PARTITION].iter().all(|s| *s == 0.0), "delayed");
            assert!(out[PARTITION..].iter().any(|s| s.abs() > 0.01));
            rig.reset();
            let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
            rig.process(&mut l, &mut r);
            assert!(l.iter().all(|s| *s == 0.0), "reset empties the line");
        }
        let mut meters = [0.0; 2];
        rig.meters(&mut meters);
        assert!(meters[0] < -100.0 && meters[1] < -100.0, "{meters:?}");
    }

    #[test]
    fn the_gate_mutes_what_stays_under_its_threshold() {
        let level = |gate: f32, amplitude: f32| {
            let mut rig = Rig::default();
            rig.set_param(1, gate);
            rig.set_param(8, DIRECT as f32);
            rig.prepare(48_000, 256);
            let mut out = Vec::new();
            for b in 0..100 {
                let mut l: Vec<f32> = (0..256)
                    .map(|i| amplitude * (((b * 256 + i) as f32) * 0.03).sin())
                    .collect();
                let mut r = l.clone();
                rig.process(&mut l, &mut r);
                out.extend(l);
            }
            out[out.len() / 2..]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
        };
        assert!(level(-40.0, 0.003) < level(-100.0, 0.003) * 0.01);
        assert!(level(-40.0, 0.3) > level(-100.0, 0.3) * 0.9);
    }
}
