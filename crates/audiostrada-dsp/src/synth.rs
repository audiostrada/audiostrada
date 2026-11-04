//! Polyphonic subtractive synthesizer: two oscillators (band-limited saw and square by
//! polyBLEP, sine, triangle), each with its octave, tuning, level and unison (up to 16 voices
//! spread in pitch and across the stereo field), a sub oscillator and noise; a resonant filter
//! (state-variable low-pass 12 or 24 dB, high-pass, band-pass, notch, or a nonlinear 24 dB
//! ladder that sings into self-oscillation) with drive, key tracking and its envelope, and an
//! amplitude envelope. Two LFOs, a second envelope, velocity, the mod wheel, pressure and four
//! macros modulate pitch, levels, filter and volume through eight routings. Analog drift gives each note its own small
//! offsets, glide slides between notes, mono and legato play one note at a time.
//! Pitch bend, channel pressure and (in MPE mode) CC 74 follow each voice's channel, so MPE
//! controllers bend and shape notes one by one. Out of MPE, the host maps CC 74 to the cutoff.

use crate::delay::{NOTE_NAMES, synced_seconds};
use crate::filter::{SHAPES, lfo_value};
use crate::wavetable::{LOADED, TABLES, Table};
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, db_to_gain, param, switch};
use std::sync::Arc;

const VOICES: usize = 16;
/// Most unison voices per oscillator.
pub const MAX_UNISON: usize = 16;

pub const INFO: EffectInfo = EffectInfo {
    id: "synth",
    name: "Strada-Synth",
    params: &PARAMS,
    meters: &METERS,
    instrument: true,
    files: &[
        crate::FileSlot {
            id: "table1",
            name: "Wavetable 1",
            kind: crate::FileKind::Audio,
            extensions: &["wav"],
        },
        crate::FileSlot {
            id: "table2",
            name: "Wavetable 2",
            kind: crate::FileKind::Audio,
            extensions: &["wav"],
        },
    ],
    category: "Instrument",
    description: "Polyphonic synthesizer: two oscillators with unison, sub, noise, analog drift, \
                  glide, MPE.",
};

pub const WAVES: [&str; 5] = ["Saw", "Square", "Sine", "Triangle", "Wavetable"];
pub const WAVETABLE: u32 = 4;
pub const SUB_OCTAVES: [&str; 2] = ["-1 octave", "-2 octaves"];
pub const MODES: [&str; 3] = ["Poly", "Mono", "Legato"];
pub const FILTERS: [&str; 6] = [
    "Low-pass 12 dB",
    "Low-pass 24 dB",
    "Ladder 24 dB",
    "High-pass",
    "Band-pass",
    "Notch",
];
pub const LP12: usize = 0;
pub const LP24: usize = 1;
pub const LADDER: usize = 2;
pub const HP: usize = 3;
pub const BP: usize = 4;

pub(crate) mod p {
    pub const WAVE: usize = 0;
    pub const ATTACK: usize = 1;
    pub const DECAY: usize = 2;
    pub const SUSTAIN: usize = 3;
    pub const RELEASE: usize = 4;
    pub const CUTOFF: usize = 5;
    pub const RESONANCE: usize = 6;
    pub const ENV_AMOUNT: usize = 7;
    pub const DETUNE: usize = 8;
    pub const VELOCITY: usize = 9;
    pub const VOLUME: usize = 10;
    pub const BEND_RANGE: usize = 11;
    pub const MPE: usize = 12;
    pub const OSC1_OCTAVE: usize = 13;
    pub const OSC1_SEMI: usize = 14;
    pub const OSC1_FINE: usize = 15;
    pub const OSC1_LEVEL: usize = 16;
    pub const OSC1_VOICES: usize = 17;
    pub const OSC2_WAVE: usize = 18;
    pub const OSC2_OCTAVE: usize = 19;
    pub const OSC2_SEMI: usize = 20;
    pub const OSC2_FINE: usize = 21;
    pub const OSC2_LEVEL: usize = 22;
    pub const OSC2_VOICES: usize = 23;
    pub const OSC2_DETUNE: usize = 24;
    pub const SUB_LEVEL: usize = 25;
    pub const SUB_OCTAVE: usize = 26;
    pub const NOISE: usize = 27;
    pub const SPREAD: usize = 28;
    pub const DRIFT: usize = 29;
    pub const GLIDE: usize = 30;
    pub const MODE: usize = 31;
    pub const FILTER: usize = 32;
    pub const DRIVE: usize = 33;
    pub const KEY_TRACK: usize = 34;
    /// Shape, rate, sync, note and retrigger of LFO 1, then the same for LFO 2.
    pub const LFO: usize = 35;
    pub const LFO_FIELDS: usize = 5;
    pub const ENV2: usize = 45;
    pub const MACROS: usize = 49;
    /// Source, target and amount of each routing.
    pub const SLOTS: usize = 53;
    /// Table and position of each oscillator's wavetable.
    pub const TABLES: usize = 77;
    /// Oscillator 2 driving oscillator 1: phase modulation, hard sync, ring modulation.
    pub const FM: usize = 81;
    pub const SYNC: usize = 82;
    pub const RING: usize = 83;
    /// The effects after the voices: chorus mix, delay mix, note and feedback, reverb mix
    /// and decay.
    pub const CHORUS: usize = 84;
    pub const DELAY: usize = 85;
    pub const DELAY_NOTE: usize = 86;
    pub const DELAY_FEEDBACK: usize = 87;
    pub const REVERB: usize = 88;
    pub const REVERB_DECAY: usize = 89;
}
pub const SLOTS: usize = 8;

/// What the editor shows live, in the meters: the output as an oscilloscope (a few periods
/// from a rising zero crossing), then the newest note's LFO phases and values, its two
/// envelopes (level and stage), its cutoff, the voices playing, and each target's modulation.
pub mod meter {
    pub const SCOPE: usize = 256;
    pub const LFO_PHASE: usize = SCOPE;
    pub const LFO_VALUE: usize = SCOPE + 2;
    pub const ENV: usize = SCOPE + 4;
    pub const STAGE: usize = SCOPE + 5;
    pub const ENV2: usize = SCOPE + 6;
    pub const CUTOFF: usize = SCOPE + 7;
    pub const VOICES: usize = SCOPE + 8;
    pub const MODS: usize = SCOPE + 9;
    pub const COUNT: usize = MODS + super::TARGETS.len();
}
const METERS: [&str; meter::COUNT] = [""; meter::COUNT];
/// Samples kept for the oscilloscope: room for a trigger and the window, read every other one.
const SCOPE_LINE: usize = 2048;
const COUNT: usize = p::REVERB_DECAY + 1;

/// What modulates, in the order of the routings' source choice.
pub const SOURCES: [&str; 11] = [
    "None",
    "LFO 1",
    "LFO 2",
    "Envelope 2",
    "Velocity",
    "Mod wheel",
    "Pressure",
    "Macro 1",
    "Macro 2",
    "Macro 3",
    "Macro 4",
];
/// What is modulated, in the order of the routings' target choice.
pub const TARGETS: [&str; 15] = [
    "None",
    "Pitch",
    "Osc 1 pitch",
    "Osc 2 pitch",
    "Osc 1 level",
    "Osc 2 level",
    "Sub level",
    "Noise",
    "Cutoff",
    "Resonance",
    "Filter drive",
    "Volume",
    "Osc 1 position",
    "Osc 2 position",
    "FM",
];
pub mod target {
    pub const PITCH: usize = 1;
    pub const OSC1_PITCH: usize = 2;
    pub const OSC2_PITCH: usize = 3;
    pub const OSC1_LEVEL: usize = 4;
    pub const OSC2_LEVEL: usize = 5;
    pub const SUB: usize = 6;
    pub const NOISE: usize = 7;
    pub const CUTOFF: usize = 8;
    pub const RESONANCE: usize = 9;
    pub const DRIVE: usize = 10;
    pub const VOLUME: usize = 11;
    pub const OSC1_POSITION: usize = 12;
    pub const OSC2_POSITION: usize = 13;
    pub const FM: usize = 14;
}
/// Reach of a full modulation: semitones of pitch, octaves of cutoff.
const PITCH_REACH: f32 = 12.0;
const CUTOFF_REACH: f32 = 5.0;

macro_rules! lfo {
    ($n:literal) => {
        [
            choice(
                concat!("lfo", $n, "_shape"),
                concat!("LFO ", $n, " shape"),
                &SHAPES,
                0.0,
            ),
            param(
                concat!("lfo", $n, "_rate"),
                concat!("LFO ", $n, " rate"),
                0.05,
                20.0,
                4.0,
                "Hz",
            ),
            switch(
                concat!("lfo", $n, "_sync"),
                concat!("LFO ", $n, " sync"),
                0.0,
            ),
            choice(
                concat!("lfo", $n, "_note"),
                concat!("LFO ", $n, " note"),
                &NOTE_NAMES,
                5.0,
            ),
            switch(
                concat!("lfo", $n, "_retrig"),
                concat!("LFO ", $n, " retrigger"),
                1.0,
            ),
        ]
    };
}

macro_rules! slot {
    ($n:literal) => {
        [
            choice(
                concat!("mod", $n, "_source"),
                concat!("Modulation ", $n, " source"),
                &SOURCES,
                0.0,
            ),
            choice(
                concat!("mod", $n, "_target"),
                concat!("Modulation ", $n, " target"),
                &TARGETS,
                0.0,
            ),
            param(
                concat!("mod", $n, "_amount"),
                concat!("Modulation ", $n, " amount"),
                -100.0,
                100.0,
                0.0,
                "%",
            ),
        ]
    };
}

const fn join<const A: usize, const B: usize>(
    head: [ParamInfo; A],
    tail: &[ParamInfo],
) -> [ParamInfo; B] {
    let mut out = [head[0]; B];
    let mut i = 0;
    while i < A {
        out[i] = head[i];
        i += 1;
    }
    let mut j = 0;
    while j < tail.len() {
        out[A + j] = tail[j];
        j += 1;
    }
    out
}

const EXTRA: [ParamInfo; COUNT - 35] = {
    let lfo1 = lfo!("1");
    let lfo2 = lfo!("2");
    let slots = [
        slot!("1"),
        slot!("2"),
        slot!("3"),
        slot!("4"),
        slot!("5"),
        slot!("6"),
        slot!("7"),
        slot!("8"),
    ];
    let mut out = [lfo1[0]; COUNT - 35];
    let mut i = 0;
    while i < 5 {
        out[i] = lfo1[i];
        out[5 + i] = lfo2[i];
        i += 1;
    }
    out[10] = param("env2_attack", "Envelope 2 attack", 0.5, 4000.0, 5.0, "ms");
    out[11] = param("env2_decay", "Envelope 2 decay", 1.0, 8000.0, 400.0, "ms");
    out[12] = param("env2_sustain", "Envelope 2 sustain", 0.0, 100.0, 0.0, "%");
    out[13] = param(
        "env2_release",
        "Envelope 2 release",
        1.0,
        8000.0,
        400.0,
        "ms",
    );
    out[14] = param("macro1", "Macro 1", 0.0, 100.0, 0.0, "%");
    out[15] = param("macro2", "Macro 2", 0.0, 100.0, 0.0, "%");
    out[16] = param("macro3", "Macro 3", 0.0, 100.0, 0.0, "%");
    out[17] = param("macro4", "Macro 4", 0.0, 100.0, 0.0, "%");
    let tables = p::TABLES - 35;
    out[tables] = choice("osc1_table", "Osc 1 wavetable", &TABLES, 0.0);
    out[tables + 1] = param("osc1_position", "Osc 1 position", 0.0, 100.0, 0.0, "%");
    out[tables + 2] = choice("osc2_table", "Osc 2 wavetable", &TABLES, 0.0);
    out[tables + 3] = param("osc2_position", "Osc 2 position", 0.0, 100.0, 0.0, "%");
    let more = p::FM - 35;
    out[more] = param("fm", "FM", 0.0, 100.0, 0.0, "%");
    out[more + 1] = switch("sync", "Sync", 0.0);
    out[more + 2] = param("ring", "Ring", 0.0, 100.0, 0.0, "%");
    out[more + 3] = param("fx_chorus", "Chorus", 0.0, 100.0, 0.0, "%");
    out[more + 4] = param("fx_delay", "Delay", 0.0, 100.0, 0.0, "%");
    out[more + 5] = choice("fx_delay_note", "Delay time", &NOTE_NAMES, 6.0);
    out[more + 6] = param("fx_delay_feedback", "Delay feedback", 0.0, 90.0, 35.0, "%");
    out[more + 7] = param("fx_reverb", "Reverb", 0.0, 100.0, 0.0, "%");
    out[more + 8] = param("fx_reverb_decay", "Reverb decay", 0.3, 10.0, 2.5, "s");
    let mut k = 0;
    while k < SLOTS {
        let mut f = 0;
        while f < 3 {
            out[18 + k * 3 + f] = slots[k][f];
            f += 1;
        }
        k += 1;
    }
    out
};

const PARAMS: [ParamInfo; COUNT] = join(BASE, &EXTRA);

// The first thirteen keep their ids and places: projects store them.
const BASE: [ParamInfo; 35] = [
    choice("wave", "Osc 1 wave", &WAVES, 0.0),
    param("attack", "Attack", 0.5, 2000.0, 5.0, "ms"),
    param("decay", "Decay", 1.0, 4000.0, 300.0, "ms"),
    param("sustain", "Sustain", 0.0, 100.0, 70.0, "%"),
    param("release", "Release", 1.0, 8000.0, 400.0, "ms"),
    param("cutoff", "Cutoff", 40.0, 18000.0, 2500.0, "Hz"),
    param("resonance", "Resonance", 0.0, 95.0, 20.0, "%"),
    param("env_amount", "Filter envelope", 0.0, 100.0, 40.0, "%"),
    param("detune", "Osc 1 detune", 0.0, 50.0, 8.0, "cents"),
    param("velocity", "Velocity sensitivity", 0.0, 100.0, 70.0, "%"),
    param("volume", "Volume", -48.0, 6.0, -12.0, "dB"),
    param("bend_range", "Pitch bend range", 1.0, 24.0, 2.0, "st"),
    switch("mpe", "MPE (channel 1 global, 48 st per note)", 0.0),
    param("osc1_octave", "Osc 1 octave", -3.0, 3.0, 0.0, "oct"),
    param("osc1_semi", "Osc 1 semitones", -12.0, 12.0, 0.0, "semi"),
    param("osc1_fine", "Osc 1 fine", -50.0, 50.0, 0.0, "cents"),
    param("osc1_level", "Osc 1 level", 0.0, 100.0, 100.0, "%"),
    param(
        "osc1_voices",
        "Osc 1 unison",
        1.0,
        MAX_UNISON as f32,
        2.0,
        "voices",
    ),
    choice("osc2_wave", "Osc 2 wave", &WAVES, 0.0),
    param("osc2_octave", "Osc 2 octave", -3.0, 3.0, 0.0, "oct"),
    param("osc2_semi", "Osc 2 semitones", -12.0, 12.0, 0.0, "semi"),
    param("osc2_fine", "Osc 2 fine", -50.0, 50.0, 0.0, "cents"),
    param("osc2_level", "Osc 2 level", 0.0, 100.0, 0.0, "%"),
    param(
        "osc2_voices",
        "Osc 2 unison",
        1.0,
        MAX_UNISON as f32,
        1.0,
        "voices",
    ),
    param("osc2_detune", "Osc 2 detune", 0.0, 50.0, 8.0, "cents"),
    param("sub_level", "Sub level", 0.0, 100.0, 0.0, "%"),
    choice("sub_octave", "Sub octave", &SUB_OCTAVES, 0.0),
    param("noise", "Noise", 0.0, 100.0, 0.0, "%"),
    param("spread", "Stereo spread", 0.0, 100.0, 50.0, "%"),
    param("drift", "Analog drift", 0.0, 100.0, 10.0, "%"),
    param("glide", "Glide", 0.0, 2000.0, 0.0, "ms"),
    choice("mode", "Voice mode", &MODES, 0.0),
    choice("filter_type", "Filter type", &FILTERS, 0.0),
    param("drive", "Filter drive", 0.0, 100.0, 0.0, "%"),
    param("key_track", "Key tracking", 0.0, 100.0, 0.0, "%"),
];

/// Pitch bend range of MPE member channels, in semitones.
const MPE_BEND: f32 = 48.0;
/// Most pitch and cutoff offsets of a note at full drift, in cents and in octaves.
const DRIFT_CENTS: f32 = 12.0;
const DRIFT_OCTAVES: f32 = 0.25;

/// Expression received on one MIDI channel.
#[derive(Clone, Copy, Debug)]
struct ChannelState {
    /// -1 to 1.
    bend: f32,
    /// 0 to 1.
    pressure: f32,
    /// CC 74, 0 to 1 (0.5 when never sent).
    slide: f32,
    /// The mod wheel (CC 1), 0 to 1.
    wheel: f32,
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            bend: 0.0,
            pressure: 0.0,
            slide: 0.5,
            wheel: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    key: u8,
    channel: u8,
    velocity: f32,
    /// Where the pitch is going, and where it is (glide), in semitones from A4.
    target: f32,
    note: f32,
    /// Phases of each oscillator's unison voices, then the sub.
    phases: [[f32; MAX_UNISON]; 2],
    sub: f32,
    stage: Stage,
    level: f32,
    /// Filter state of each side: two state-variable stages, or the ladder's four poles.
    filter: [[f32; 4]; 2],
    /// The note's own analog offsets: pitch in cents, cutoff in octaves, and a slow wander.
    drift_pitch: f32,
    drift_cutoff: f32,
    wander: f32,
    wander_rate: f32,
    /// LFO phases and the random value of their cycle; the second envelope.
    lfo: [f32; 2],
    held: [f32; 2],
    env2: f32,
    env2_stage: Stage,
    /// What the voice did last, for the editor: its modulations and its cutoff.
    mods: [f32; TARGETS.len()],
    cutoff: f32,
    age: u64,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            key: 0,
            channel: 0,
            velocity: 0.0,
            target: 0.0,
            note: 0.0,
            phases: [[0.0; MAX_UNISON]; 2],
            sub: 0.0,
            stage: Stage::Idle,
            level: 0.0,
            filter: [[0.0; 4]; 2],
            drift_pitch: 0.0,
            drift_cutoff: 0.0,
            wander: 0.0,
            wander_rate: 0.0,
            lfo: [0.0; 2],
            held: [0.0; 2],
            env2: 0.0,
            env2_stage: Stage::Idle,
            mods: [0.0; TARGETS.len()],
            cutoff: 0.0,
            age: 0,
        }
    }
}

/// PolyBLEP residual for a discontinuity at phase 0 (phase and increment in cycles).
#[inline]
fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn oscillator(wave: u32, phase: f32, dt: f32) -> f32 {
    match wave {
        0 => 2.0 * phase - 1.0 - blep(phase, dt),
        1 => {
            let square = if phase < 0.5 { 1.0 } else { -1.0 };
            square + blep(phase, dt) - blep((phase + 0.5) % 1.0, dt)
        }
        2 => (phase * std::f32::consts::TAU).sin(),
        _ => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

/// One step of an ADSR envelope: `rates` are attack, decay, sustain level and release.
#[inline]
fn advance(stage: &mut Stage, level: &mut f32, (a, d, sustain, r): (f32, f32, f32, f32)) {
    match stage {
        Stage::Attack => {
            *level += a;
            if *level >= 1.0 {
                *level = 1.0;
                *stage = Stage::Decay;
            }
        }
        Stage::Decay => {
            *level -= d * (1.0 - sustain).max(0.001);
            if *level <= sustain {
                *level = sustain;
                *stage = Stage::Sustain;
            }
        }
        Stage::Release => {
            *level -= r;
            if *level <= 0.0 {
                *level = 0.0;
                *stage = Stage::Idle;
            }
        }
        Stage::Sustain | Stage::Idle => {}
    }
}

/// A fast tanh, close enough for saturation (a rational approximation, exact at 0).
#[inline]
fn soft(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// One trapezoidal state-variable stage (Simper): low, band and high outputs.
#[inline]
fn svf(state: &mut [f32], x: f32, g: f32, k: f32) -> (f32, f32, f32) {
    let a1 = 1.0 / (1.0 + g * (g + k));
    let a2 = g * a1;
    let a3 = g * a2;
    let v3 = x - state[1];
    let v1 = a1 * state[0] + a2 * v3;
    let v2 = state[1] + a2 * state[0] + a3 * v3;
    state[0] = 2.0 * v1 - state[0];
    state[1] = 2.0 * v2 - state[1];
    (v2, v1, x - k * v1 - v2)
}

/// The filter of one side: `kind` from [`FILTERS`], `fc` in Hz, `resonance` 0 to 1, `drive`
/// as a gain into the saturation (1 for none).
#[inline]
fn filter(
    state: &mut [f32; 4],
    kind: usize,
    x: f32,
    fc: f32,
    rate: f32,
    resonance: f32,
    drive: f32,
) -> f32 {
    let x = if drive > 1.0 {
        soft(x * drive) / drive.sqrt()
    } else {
        x
    };
    if kind == LADDER {
        // Huovilainen's nonlinear ladder, run twice per sample: four saturating one-poles in a
        // loop that oscillates by itself near full resonance; the lows it loses are given back.
        let g = 1.0 - (-std::f32::consts::TAU * fc / (2.0 * rate)).exp();
        let k = resonance * 4.1;
        let mut y = 0.0;
        for _ in 0..2 {
            let input = soft(x - k * state[3]);
            state[0] += g * (input - soft(state[0]));
            state[1] += g * (soft(state[0]) - soft(state[1]));
            state[2] += g * (soft(state[1]) - soft(state[2]));
            state[3] += g * (soft(state[2]) - soft(state[3]));
            y = state[3];
        }
        return y * (1.0 + 0.8 * k);
    }
    let g = (std::f32::consts::PI * fc / rate).tan();
    let damping = 2.0 * (1.0 - resonance * 0.95).max(0.05);
    match kind {
        LP24 => {
            // A flat stage, then the resonant one.
            let (low, ..) = svf(&mut state[2..4], x, g, std::f32::consts::SQRT_2);
            svf(&mut state[0..2], low, g, damping).0
        }
        HP => svf(&mut state[0..2], x, g, damping).2,
        BP => svf(&mut state[0..2], x, g, damping).1 * damping,
        LP12 => svf(&mut state[0..2], x, g, damping).0,
        _ => {
            let (low, _, high) = svf(&mut state[0..2], x, g, damping);
            low + high
        }
    }
}

/// Pitch offset (cents) and pan (-1 to 1) of unison voice `k` of `n`.
fn unison(k: usize, n: usize) -> f32 {
    if n < 2 {
        0.0
    } else {
        2.0 * k as f32 / (n - 1) as f32 - 1.0
    }
}

/// Small pseudo-random numbers for drift and noise: xorshift, -1 to 1.
#[derive(Debug, Clone, Copy)]
struct Random(u32);

impl Random {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

#[derive(Debug)]
pub struct Synth {
    rate: f32,
    values: [f32; COUNT],
    voices: [Voice; VOICES],
    channels: [ChannelState; 16],
    clock: u64,
    random: Random,
    /// Keys held in mono and legato modes, oldest first, to fall back on at a release.
    held: [(u8, u8, u8); 16],
    held_count: usize,
    bpm: f64,
    /// Free-running LFO phases, which notes join when they do not restart them.
    lfo: [f32; 2],
    /// The latest output (mono), for the oscilloscope.
    scope: Vec<f32>,
    scope_at: usize,
    /// Each oscillator's wavetable, and the tables loaded from files.
    tables: [Arc<Table>; 2],
    loaded: [Option<Arc<Table>>; 2],
    /// Chorus, delay and reverb after the voices.
    effects: Effects,
}

/// The synth's own effects, in order.
struct Effects([Box<dyn Effect>; 3]);

impl std::fmt::Debug for Effects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Effects")
    }
}

impl Default for Effects {
    fn default() -> Self {
        let make = |id: &str, settings: &[(&str, f32)]| {
            let mut effect = crate::create(id).expect("built-in effect");
            let info = crate::info(id).expect("built-in effect");
            for &(name, value) in settings {
                if let Some(i) = info.params.iter().position(|p| p.id == name) {
                    effect.set_param(i, value);
                }
            }
            effect
        };
        Self([
            make("chorus", &[("mix", 0.0)]),
            make(
                "delay",
                &[
                    ("sync", 1.0),
                    ("pingpong", 1.0),
                    ("mix", 0.0),
                    ("high_cut", 6000.0),
                ],
            ),
            make("reverb", &[("mix", 0.0), ("predelay", 15.0)]),
        ])
    }
}

impl Effects {
    /// Which effect and parameter a synth parameter drives.
    fn route(index: usize) -> Option<(usize, &'static str)> {
        Some(match index {
            p::CHORUS => (0, "mix"),
            p::DELAY => (1, "mix"),
            p::DELAY_NOTE => (1, "note"),
            p::DELAY_FEEDBACK => (1, "feedback"),
            p::REVERB => (2, "mix"),
            p::REVERB_DECAY => (2, "decay"),
            _ => return None,
        })
    }

    fn set(&mut self, index: usize, value: f32) {
        let Some((k, name)) = Self::route(index) else {
            return;
        };
        let id = ["chorus", "delay", "reverb"][k];
        if let Some(i) =
            crate::info(id).and_then(|info| info.params.iter().position(|p| p.id == name))
        {
            self.0[k].set_param(i, value);
        }
    }
}

impl Default for Synth {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: [Voice::default(); VOICES],
            channels: [ChannelState::default(); 16],
            clock: 0,
            random: Random(0x9e37_79b9),
            held: [(0, 0, 0); 16],
            held_count: 0,
            bpm: 120.0,
            lfo: [0.0; 2],
            scope: vec![0.0; SCOPE_LINE],
            scope_at: 0,
            tables: [
                Arc::clone(&crate::wavetable::factory()[0]),
                Arc::clone(&crate::wavetable::factory()[0]),
            ],
            loaded: [None, None],
            effects: Effects::default(),
        }
    }
}

/// Whether a routing moves the FM amount.
fn routes_fm(v: &[f32; COUNT]) -> bool {
    (0..SLOTS).any(|k| {
        let at = p::SLOTS + k * 3;
        v[at] >= 0.5 && v[at + 1].round() as usize == target::FM && v[at + 2] != 0.0
    })
}

impl Synth {
    /// Points oscillator `o` at its chosen table: a factory one, or the loaded file.
    fn choose_table(&mut self, o: usize) {
        let kind = self.values[p::TABLES + o * 2].round().max(0.0) as usize;
        let table = if kind >= LOADED {
            self.loaded[o].clone()
        } else {
            crate::wavetable::factory().get(kind).cloned()
        };
        // Swapping an Arc does not allocate.
        if let Some(table) = table {
            self.tables[o] = table;
        }
    }

    fn mode(&self) -> usize {
        (self.values[p::MODE].round().max(0.0) as usize).min(MODES.len() - 1)
    }

    fn note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        self.clock += 1;
        let mode = self.mode();
        if mode > 0 {
            let n = self.held.len();
            self.held.copy_within(0..n - 1, 1);
            self.held_count = (self.held_count + 1).min(self.held.len());
            self.held[0] = (channel, key, velocity);
        }
        let index = if mode > 0 {
            0
        } else {
            // Reuse the same key, else a free voice, else the oldest.
            self.voices
                .iter()
                .position(|v| v.stage != Stage::Idle && v.key == key && v.channel == channel)
                .or_else(|| self.voices.iter().position(|v| v.stage == Stage::Idle))
                .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].age).unwrap_or(0))
        };
        let sensitivity = self.values[p::VELOCITY] / 100.0;
        let drift = self.values[p::DRIFT] / 100.0;
        let glide = self.values[p::GLIDE] > 0.5;
        let (r1, r2, r3, r4) = (
            self.random.next(),
            self.random.next(),
            self.random.next(),
            self.random.next(),
        );
        let held = [self.random.next(), self.random.next()];
        let retrigs = [0, 1].map(|l| self.values[p::LFO + l * p::LFO_FIELDS + 4] >= 0.5);
        let free = self.lfo;
        let mut phases = [[0.0f32; MAX_UNISON]; 2];
        for row in &mut phases {
            for (k, ph) in row.iter_mut().enumerate() {
                // Unison voices start apart, as free-running oscillators would.
                *ph = if k == 0 {
                    0.0
                } else {
                    (self.random.next() * 0.5 + 0.5).fract()
                };
            }
        }
        let v = &mut self.voices[index];
        let sounding = v.stage != Stage::Idle;
        v.key = key;
        v.channel = channel;
        v.velocity = 1.0 - sensitivity + sensitivity * f32::from(velocity) / 127.0;
        v.target = f32::from(key) - 69.0;
        // Glide from the note that was playing; else start on the note.
        if !(glide && sounding) {
            v.note = v.target;
        }
        // Legato: a note played over another keeps its envelope going.
        let retrigger = !(mode == 2 && sounding);
        if !sounding {
            v.phases = phases;
            v.sub = 0.0;
            v.level = 0.0;
            v.filter = [[0.0; 4]; 2];
        }
        if retrigger {
            v.stage = Stage::Attack;
            v.env2_stage = Stage::Attack;
            if !sounding {
                v.env2 = 0.0;
            }
            for l in 0..2 {
                v.lfo[l] = if retrigs[l] { 0.0 } else { free[l] };
                v.held[l] = held[l];
            }
            v.drift_pitch = r1 * drift * DRIFT_CENTS;
            v.drift_cutoff = r2 * drift * DRIFT_OCTAVES;
            v.wander = r3 * std::f32::consts::PI;
            v.wander_rate = 0.1 + 0.2 * (r4 * 0.5 + 0.5);
        } else if v.stage == Stage::Release {
            v.stage = Stage::Decay;
        }
        v.age = self.clock;
    }

    /// Bend of a voice in semitones: its own channel, plus the global channel in MPE mode.
    fn bend(&self, channel: u8) -> f32 {
        let state = &self.channels[usize::from(channel & 15)];
        if self.values[p::MPE] >= 0.5 {
            let global = self.channels[0].bend * self.values[p::BEND_RANGE];
            if channel == 0 {
                global
            } else {
                global + state.bend * MPE_BEND
            }
        } else {
            state.bend * self.values[p::BEND_RANGE]
        }
    }

    fn note_off(&mut self, channel: u8, key: u8) {
        if self.mode() > 0 {
            // Back to the last key still held, else the note ends.
            let mut kept = 0;
            for i in 0..self.held_count {
                if self.held[i].0 != channel || self.held[i].1 != key {
                    self.held[kept] = self.held[i];
                    kept += 1;
                }
            }
            self.held_count = kept;
            let v = self.voices[0];
            if v.key == key && v.channel == channel {
                if let Some(&(c, k, vel)) = self.held[..self.held_count].first() {
                    self.held_count -= 1;
                    self.held.copy_within(1..=self.held_count, 0);
                    // Played again as if pressed, without a new attack in legato.
                    let mode = self.mode();
                    self.values[p::MODE] = 2.0;
                    self.note_on(c, k, vel);
                    self.values[p::MODE] = mode as f32;
                } else {
                    self.voices[0].stage = Stage::Release;
                }
            }
            return;
        }
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.key == key && v.channel == channel && v.stage != Stage::Idle)
        {
            v.stage = Stage::Release;
        }
    }
}

impl Effect for Synth {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES];
        self.held_count = 0;
        for effect in &mut self.effects.0 {
            effect.prepare(sample_rate, max_frames);
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
        self.effects.0[1].set_tempo(bpm);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
        if index == p::TABLES || index == p::TABLES + 2 {
            self.choose_table((index - p::TABLES) / 2);
        }
        self.effects.set(index, value);
    }

    fn load(&mut self, slot: &str, file: crate::FileData) -> Result<String, String> {
        let o = match slot {
            "table1" => 0,
            "table2" => 1,
            _ => return Err(format!("no file slot {slot}")),
        };
        let crate::FileData::Audio { channels, .. } = file else {
            return Err("a wavetable is an audio file".into());
        };
        let samples = channels.first().ok_or("the file is empty")?;
        let table = Table::from_samples(samples)?;
        let frames = table.frames();
        self.loaded[o] = Some(Arc::new(table));
        self.choose_table(o);
        Ok(format!("{frames} frames"))
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((key, velocity)) = event.as_note_on() {
            self.note_on(event.channel(), key, velocity);
        } else if let Some(key) = event.as_note_off() {
            self.note_off(event.channel(), key);
        } else if event
            .as_control()
            .is_some_and(|(cc, _)| cc == 120 || cc == 123)
        {
            for v in &mut self.voices {
                v.stage = if event.data[1] == 120 {
                    Stage::Idle
                } else {
                    Stage::Release
                };
            }
            self.held_count = 0;
        } else {
            let state = &mut self.channels[usize::from(event.channel())];
            match (event.data[0] & 0xF0, event.data[1]) {
                (0xE0, _) => {
                    let value = u16::from(event.data[2]) << 7 | u16::from(event.data[1]);
                    state.bend = ((f32::from(value) - 8192.0) / 8192.0).clamp(-1.0, 1.0);
                }
                (0xD0, pressure) => state.pressure = f32::from(pressure) / 127.0,
                (0xB0, 74) => state.slide = f32::from(event.data[2]) / 127.0,
                (0xB0, 1) => state.wheel = f32::from(event.data[2]) / 127.0,
                (0xB0, 121) => *state = ChannelState::default(),
                _ => {}
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let v = self.values;
        let per_ms = 1000.0 / self.rate;
        let (a, d, r) = (
            per_ms / v[p::ATTACK],
            per_ms / v[p::DECAY],
            per_ms / v[p::RELEASE],
        );
        let sustain = v[p::SUSTAIN] / 100.0;
        let resonance = v[p::RESONANCE] / 95.0;
        let kind = (v[p::FILTER].round().max(0.0) as usize).min(FILTERS.len() - 1);
        let drive = 1.0 + v[p::DRIVE] / 100.0 * 7.0;
        let key_track = v[p::KEY_TRACK] / 100.0;
        let gain = db_to_gain(v[p::VOLUME]) * 0.5;
        let (cutoff, env_amount) = (v[p::CUTOFF], v[p::ENV_AMOUNT] / 100.0);
        // Each oscillator: wave, pitch offset in semitones, level, unison count and detune.
        let oscs = [
            (
                v[p::WAVE],
                v[p::OSC1_OCTAVE].round() * 12.0
                    + v[p::OSC1_SEMI].round()
                    + v[p::OSC1_FINE] / 100.0,
                v[p::OSC1_LEVEL] / 100.0,
                v[p::OSC1_VOICES],
                v[p::DETUNE],
            ),
            (
                v[p::OSC2_WAVE],
                v[p::OSC2_OCTAVE].round() * 12.0
                    + v[p::OSC2_SEMI].round()
                    + v[p::OSC2_FINE] / 100.0,
                v[p::OSC2_LEVEL] / 100.0,
                v[p::OSC2_VOICES],
                v[p::OSC2_DETUNE],
            ),
        ]
        .map(|(wave, offset, level, voices, detune)| {
            let n = (voices.round().max(1.0) as usize).min(MAX_UNISON);
            (
                wave.round().clamp(0.0, WAVETABLE as f32) as u32,
                offset,
                // Unison voices add up in power: the level holds as their number grows.
                level / (n as f32).sqrt(),
                n,
                detune,
            )
        });
        let fm = v[p::FM] / 100.0;
        let sync = v[p::SYNC] >= 0.5;
        let ring = v[p::RING] / 100.0 / (oscs[0].3 as f32).sqrt();
        // Oscillator 2 runs even unheard while it shapes oscillator 1.
        let drives = fm > 0.0 || sync || ring > 0.0 || routes_fm(&v);
        let sub_level = v[p::SUB_LEVEL] / 100.0;
        let sub_ratio = if v[p::SUB_OCTAVE] >= 0.5 { 0.25 } else { 0.5 };
        let noise = v[p::NOISE] / 100.0;
        let spread = v[p::SPREAD] / 100.0;
        let drift = v[p::DRIFT] / 100.0;
        // The glide time is about how long the slide takes to arrive.
        let glide = if v[p::GLIDE] > 0.5 {
            crate::time_coefficient(v[p::GLIDE] / 4.0, self.rate)
        } else {
            0.0
        };
        left.fill(0.0);
        right.fill(0.0);
        let bends: [f32; 16] = std::array::from_fn(|c| self.bend(c as u8));
        let mpe = self.values[p::MPE] >= 0.5;
        // Equal-power pans of the unison voices.
        let pans: [[(f32, f32); MAX_UNISON]; 2] = std::array::from_fn(|o| {
            std::array::from_fn(|k| {
                let pan = unison(k, oscs[o].3) * spread;
                let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                (
                    angle.cos() * std::f32::consts::SQRT_2,
                    angle.sin() * std::f32::consts::SQRT_2,
                )
            })
        });
        // Frequency ratio of each unison voice to the note, once per block.
        let ratios: [[f32; MAX_UNISON]; 2] = std::array::from_fn(|o| {
            let (_, offset, _, n, detune) = oscs[o];
            std::array::from_fn(|k| 2f32.powf((offset + unison(k, n) * detune / 100.0) / 12.0))
        });
        let wander_step = std::f32::consts::TAU / self.rate;
        let tables = [Arc::clone(&self.tables[0]), Arc::clone(&self.tables[1])];
        // The LFOs: shape and phase step per sample.
        let lfos: [(usize, f32); 2] = std::array::from_fn(|l| {
            let base = p::LFO + l * p::LFO_FIELDS;
            let shape = (v[base].round().max(0.0) as usize).min(SHAPES.len() - 1);
            let hz = if v[base + 2] >= 0.5 {
                1.0 / synced_seconds(v[base + 3], self.bpm).max(0.01)
            } else {
                v[base + 1]
            };
            (shape, hz / self.rate)
        });
        for (phase, (_, step)) in self.lfo.iter_mut().zip(&lfos) {
            *phase = (*phase + step * left.len() as f32).fract();
        }
        let env2 = (
            per_ms / v[p::ENV2],
            per_ms / v[p::ENV2 + 1],
            v[p::ENV2 + 2] / 100.0,
            per_ms / v[p::ENV2 + 3],
        );
        let macros: [f32; 4] = std::array::from_fn(|m| v[p::MACROS + m] / 100.0);
        // The routings in use: source, target and amount.
        let mut routes = [(0usize, 0usize, 0.0f32); SLOTS];
        let mut route_count = 0;
        for k in 0..SLOTS {
            let at = p::SLOTS + k * 3;
            let (source, target, amount) = (
                v[at].round().max(0.0) as usize,
                v[at + 1].round().max(0.0) as usize,
                v[at + 2] / 100.0,
            );
            if source > 0
                && source < SOURCES.len()
                && target > 0
                && target < TARGETS.len()
                && amount != 0.0
            {
                routes[route_count] = (source, target, amount);
                route_count += 1;
            }
        }
        let routes = &routes[..route_count];
        let random = &mut self.random;
        for voice in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            let channel = &self.channels[usize::from(voice.channel & 15)];
            let bend = bends[usize::from(voice.channel & 15)];
            // Pressure and slide open the filter, up to two octaves each way for the slide.
            let slide = if mpe { channel.slide } else { 0.5 };
            let brightness = (1.0 + 3.0 * channel.pressure)
                * 2f32.powf((slide - 0.5) * 4.0 + voice.drift_cutoff);
            for (l, rr) in left.iter_mut().zip(right.iter_mut()) {
                match voice.stage {
                    Stage::Attack => {
                        voice.level += a;
                        if voice.level >= 1.0 {
                            voice.level = 1.0;
                            voice.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        voice.level -= d * (1.0 - sustain).max(0.001);
                        if voice.level <= sustain {
                            voice.level = sustain;
                            voice.stage = Stage::Sustain;
                        }
                    }
                    Stage::Release => {
                        voice.level -= r;
                        if voice.level <= 0.0 {
                            voice.level = 0.0;
                            voice.stage = Stage::Idle;
                        }
                    }
                    Stage::Sustain | Stage::Idle => {}
                }
                // The second envelope follows the notes as the first does.
                if voice.stage == Stage::Release && voice.env2_stage != Stage::Idle {
                    voice.env2_stage = Stage::Release;
                }
                advance(&mut voice.env2_stage, &mut voice.env2, env2);
                let mut lfo_out = [0.0f32; 2];
                for l in 0..2 {
                    let (shape, step) = lfos[l];
                    let next = voice.lfo[l] + step;
                    if next >= 1.0 {
                        voice.held[l] = random.next();
                    }
                    voice.lfo[l] = next.fract();
                    lfo_out[l] = lfo_value(shape, voice.lfo[l], voice.held[l]);
                }
                let mut m = [0.0f32; TARGETS.len()];
                for &(source, target, amount) in routes {
                    let value = match source {
                        1 => lfo_out[0],
                        2 => lfo_out[1],
                        3 => voice.env2,
                        4 => voice.velocity,
                        5 => channel.wheel,
                        6 => channel.pressure,
                        k => macros[k - 7],
                    };
                    m[target] += value * amount;
                }
                voice.note = voice.target + glide * (voice.note - voice.target);
                voice.wander += voice.wander_rate * wander_step;
                let cents = voice.drift_pitch + drift * 3.0 * voice.wander.sin();
                let base = voice.note + bend + cents / 100.0 + m[target::PITCH] * PITCH_REACH;
                let note_dt = 440.0 * 2f32.powf(base / 12.0) / self.rate;
                let mut out = [0.0f32; 2];
                let positions = [
                    (v[p::TABLES + 1] / 100.0 + m[target::OSC1_POSITION]).clamp(0.0, 1.0),
                    (v[p::TABLES + 3] / 100.0 + m[target::OSC2_POSITION]).clamp(0.0, 1.0),
                ];
                // Oscillator 2 first: it drives oscillator 1's phase, restarts and ring.
                let mut driver = [0.0f32; MAX_UNISON];
                let mut wrapped = [false; MAX_UNISON];
                let fm = (fm + m[target::FM]).clamp(0.0, 2.0);
                for o in [1, 0] {
                    let (wave, _, level, n, _) = oscs[o];
                    let (pitch, add) = if o == 0 {
                        (m[target::OSC1_PITCH], m[target::OSC1_LEVEL])
                    } else {
                        (m[target::OSC2_PITCH], m[target::OSC2_LEVEL])
                    };
                    let level = (level + add / (n as f32).sqrt()).clamp(0.0, 2.0);
                    if level <= 0.0 && !(o == 1 && drives) && !(o == 0 && ring > 0.0) {
                        continue;
                    }
                    let shift = if pitch != 0.0 {
                        2f32.powf(pitch * PITCH_REACH / 12.0)
                    } else {
                        1.0
                    };
                    let n2 = oscs[1].3;
                    for k in 0..n {
                        let dt = note_dt * ratios[o][k] * shift;
                        let phase = &mut voice.phases[o][k];
                        // Oscillator 1 voice k follows oscillator 2's voice of the same rank.
                        let (offset, restart, partner) = if o == 0 {
                            let j = k % n2;
                            (driver[j] * fm, sync && wrapped[j], driver[j])
                        } else {
                            (0.0, false, 0.0)
                        };
                        if restart {
                            *phase = 0.0;
                        }
                        let read = (*phase + offset).rem_euclid(1.0);
                        let raw = if wave == WAVETABLE {
                            tables[o].read(read, positions[o], dt)
                        } else {
                            oscillator(wave, read, dt)
                        };
                        *phase += dt;
                        let wrap = *phase >= 1.0;
                        if wrap {
                            *phase -= 1.0;
                        }
                        if o == 1 {
                            driver[k] = raw;
                            wrapped[k] = wrap;
                        }
                        let s = raw * level + if o == 0 { raw * partner * ring } else { 0.0 };
                        let (pl, pr) = pans[o][k];
                        out[0] += s * pl;
                        out[1] += s * pr;
                    }
                }
                let sub_level = (sub_level + m[target::SUB]).clamp(0.0, 2.0);
                let noise = (noise + m[target::NOISE]).clamp(0.0, 2.0);
                if sub_level > 0.0 {
                    let dt = note_dt * sub_ratio;
                    let s = (voice.sub * std::f32::consts::TAU).sin() * sub_level;
                    voice.sub = (voice.sub + dt).fract();
                    out[0] += s;
                    out[1] += s;
                }
                if noise > 0.0 {
                    out[0] += random.next() * noise * 0.5;
                    out[1] += random.next() * noise * 0.5;
                }
                // Key tracking from middle C: at 100 % the cutoff follows the notes.
                let tracked = 2f32.powf((voice.note + 9.0) / 12.0 * key_track);
                let fc = (cutoff
                    * brightness
                    * tracked
                    * (1.0 + env_amount * 4.0 * voice.level)
                    * 2f32.powf(m[target::CUTOFF] * CUTOFF_REACH))
                .clamp(20.0, self.rate * 0.45);
                let resonance = (resonance + m[target::RESONANCE]).clamp(0.0, 1.0);
                let drive = (drive + m[target::DRIVE] * 7.0).max(1.0);
                let amp = voice.level * voice.velocity * gain * (1.0 + m[target::VOLUME]).max(0.0);
                voice.mods = m;
                voice.cutoff = fc;
                for (side, x) in out.into_iter().enumerate() {
                    let state = &mut voice.filter[side];
                    let y = filter(state, kind, x, fc, self.rate, resonance, drive) * amp;
                    let y = if y.is_finite() {
                        y
                    } else {
                        // Never let a broken value reach the mix.
                        *state = [0.0; 4];
                        0.0
                    };
                    if side == 0 {
                        *l += y;
                    } else {
                        *rr += y;
                    }
                }
                if voice.stage == Stage::Idle {
                    break;
                }
            }
        }
        for (l, r) in left.iter().zip(right.iter()) {
            self.scope[self.scope_at] = 0.5 * (l + r);
            self.scope_at = (self.scope_at + 1) % SCOPE_LINE;
        }
        // The effects, only those turned up.
        for (effect, mix) in self
            .effects
            .0
            .iter_mut()
            .zip([p::CHORUS, p::DELAY, p::REVERB])
        {
            if v[mix] > 0.0 {
                effect.process(left, right);
            }
        }
    }

    fn meters(&self, out: &mut [f32]) {
        if out.len() < meter::COUNT {
            return;
        }
        // The window ends at the latest sample; it starts at a rising zero crossing, so that a
        // steady note stands still.
        let span = meter::SCOPE * 2;
        let at = |back: usize| self.scope[(self.scope_at + SCOPE_LINE - back) % SCOPE_LINE];
        let start = (span + 1..SCOPE_LINE - 1)
            .find(|&back| at(back + 1) < 0.0 && at(back) >= 0.0)
            .unwrap_or(span);
        for (i, o) in out[..meter::SCOPE].iter_mut().enumerate() {
            *o = at(start - i * 2);
        }
        let newest = self
            .voices
            .iter()
            .filter(|v| v.stage != Stage::Idle)
            .max_by_key(|v| v.age);
        let playing = self
            .voices
            .iter()
            .filter(|v| v.stage != Stage::Idle)
            .count();
        out[meter::VOICES] = playing as f32;
        match newest {
            Some(v) => {
                out[meter::LFO_PHASE] = v.lfo[0];
                out[meter::LFO_PHASE + 1] = v.lfo[1];
                for l in 0..2 {
                    let base = p::LFO + l * p::LFO_FIELDS;
                    let shape = (self.values[base].round().max(0.0) as usize).min(SHAPES.len() - 1);
                    out[meter::LFO_VALUE + l] = lfo_value(shape, v.lfo[l], v.held[l]);
                }
                out[meter::ENV] = v.level;
                out[meter::STAGE] = match v.stage {
                    Stage::Attack => 1.0,
                    Stage::Decay => 2.0,
                    Stage::Sustain => 3.0,
                    Stage::Release => 4.0,
                    Stage::Idle => 0.0,
                };
                out[meter::ENV2] = v.env2;
                out[meter::CUTOFF] = v.cutoff;
                out[meter::MODS..meter::COUNT].copy_from_slice(&v.mods);
            }
            None => {
                // Free LFOs keep turning without notes; nothing else moves.
                out[meter::LFO_PHASE] = self.lfo[0];
                out[meter::LFO_PHASE + 1] = self.lfo[1];
                out[meter::LFO_VALUE..meter::COUNT].fill(0.0);
                out[meter::VOICES] = 0.0;
            }
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.stage = Stage::Idle;
        }
        self.channels = [ChannelState::default(); 16];
        self.held_count = 0;
        for effect in &mut self.effects.0 {
            effect.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(synth: &mut Synth, blocks: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for _ in 0..blocks {
            let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
            synth.process(&mut l, &mut r);
            out.extend(l);
        }
        out
    }

    #[test]
    fn plays_notes_and_releases_them() {
        let mut synth = Synth::default();
        synth.prepare(48_000, 256);
        assert!(
            render(&mut synth, 2).iter().all(|&s| s == 0.0),
            "silent without notes"
        );
        synth.midi(&MidiEvent::note_on(0, 0, 69, 100));
        let held = render(&mut synth, 40);
        let energy: f32 = held[4800..].iter().map(|s| s * s).sum();
        assert!(energy > 1.0, "{energy}");
        assert!(held.iter().all(|s| s.is_finite() && s.abs() < 2.0));
        synth.midi(&MidiEvent::note_off(0, 0, 69));
        let released = render(&mut synth, 200);
        assert!(
            released[released.len() - 256..].iter().all(|&s| s == 0.0),
            "voice freed"
        );
        for key in 40..80 {
            synth.midi(&MidiEvent::note_on(0, 0, key, 90));
        }
        assert_eq!(
            synth
                .voices
                .iter()
                .filter(|v| v.stage != Stage::Idle)
                .count(),
            VOICES
        );
    }

    #[test]
    fn stays_finite_with_the_filter_wide_open() {
        for resonance in [0.0, 20.0, 62.0, 95.0] {
            wide_open(resonance);
        }
    }

    fn wide_open(resonance: f32) {
        let mut synth = Synth::default();
        synth.set_param(6, resonance);
        synth.set_param(7, 100.0);
        synth.prepare(48_000, 256);
        // CC 74 at the top and full pressure, as from a controller's knobs and pads.
        synth.midi(&MidiEvent::control(0, 0, 74, 127));
        synth.midi(&MidiEvent {
            time: 0,
            data: [0xD0, 127, 0],
        });
        for key in [60, 72, 84, 96] {
            synth.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let out = render(&mut synth, 400);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            out.iter().all(|s| s.is_finite()) && peak < 4.0,
            "resonance {resonance}: {peak}"
        );
    }

    /// Frequency of the output from its zero crossings.
    fn pitch(out: &[f32]) -> f32 {
        let part = &out[out.len() / 2..];
        let n = part
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        n as f32 * 48_000.0 / part.len() as f32
    }

    #[test]
    fn mpe_bends_each_note_on_its_channel() {
        let mut synth = Synth::default();
        synth.set_param(0, 2.0);
        synth.set_param(12, 1.0);
        synth.prepare(48_000, 256);
        // Member channel 2: +12 semitones of the 48 st range.
        synth.midi(&MidiEvent {
            time: 0,
            data: [0xE1, 0, 80],
        });
        synth.midi(&MidiEvent::note_on(0, 1, 57, 100));
        let bent = pitch(&render(&mut synth, 40));
        assert!((bent - 440.0).abs() < 8.0, "{bent}");
        synth.reset();
        // Without MPE the same bend uses the 2 st range.
        synth.set_param(12, 0.0);
        synth.midi(&MidiEvent {
            time: 0,
            data: [0xE1, 0, 80],
        });
        synth.midi(&MidiEvent::note_on(0, 1, 57, 100));
        let plain = pitch(&render(&mut synth, 40));
        assert!(
            (plain - 220.0 * 2f32.powf(2.0 / 12.0 * 0.25)).abs() < 6.0,
            "{plain}"
        );
        synth.midi(&MidiEvent::control(0, 1, 121, 0));
        let reset = pitch(&render(&mut synth, 40));
        assert!((reset - 220.0).abs() < 6.0, "{reset}");
    }

    fn stereo(synth: &mut Synth, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for _ in 0..blocks {
            let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
            synth.process(&mut l, &mut r);
            left.extend(l);
            right.extend(r);
        }
        (left, right)
    }

    fn synth(settings: &[(usize, f32)]) -> Synth {
        let mut synth = Synth::default();
        // Plain sine, no drift, filter open: pitches read cleanly.
        for (i, v) in [(p::WAVE, 2.0), (p::DRIFT, 0.0), (p::CUTOFF, 18_000.0)]
            .into_iter()
            .chain(settings.iter().copied())
        {
            synth.set_param(i, v);
        }
        synth.prepare(48_000, 256);
        synth
    }

    #[test]
    fn the_second_oscillator_sub_and_noise_add_up() {
        // Osc 1 off, osc 2 an octave up: A4 played sounds A5.
        let mut s = synth(&[
            (p::OSC1_LEVEL, 0.0),
            (p::OSC2_WAVE, 2.0),
            (p::OSC2_LEVEL, 100.0),
            (p::OSC2_OCTAVE, 1.0),
            (p::OSC1_VOICES, 1.0),
        ]);
        s.midi(&MidiEvent::note_on(0, 0, 69, 100));
        let found = pitch(&stereo(&mut s, 40).0);
        assert!((found - 880.0).abs() < 10.0, "{found}");
        // The sub alone: an octave down.
        let mut s = synth(&[(p::OSC1_LEVEL, 0.0), (p::SUB_LEVEL, 100.0)]);
        s.midi(&MidiEvent::note_on(0, 0, 69, 100));
        let found = pitch(&stereo(&mut s, 40).0);
        assert!((found - 220.0).abs() < 10.0, "{found}");
        let mut s = synth(&[(p::OSC1_LEVEL, 0.0), (p::NOISE, 100.0)]);
        s.midi(&MidiEvent::note_on(0, 0, 69, 100));
        let (l, r) = stereo(&mut s, 10);
        assert!(
            l.iter().zip(&r).any(|(a, b)| (a - b).abs() > 1e-3),
            "stereo noise"
        );
    }

    #[test]
    fn unison_spreads_across_the_stereo_field() {
        let mut s = synth(&[(p::WAVE, 0.0), (p::OSC1_VOICES, 7.0), (p::SPREAD, 100.0)]);
        s.midi(&MidiEvent::note_on(0, 0, 57, 100));
        let (l, r) = stereo(&mut s, 40);
        let side: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(side > 10.0, "wide: {side}");
        let mut s = synth(&[(p::WAVE, 0.0), (p::OSC1_VOICES, 7.0), (p::SPREAD, 0.0)]);
        s.midi(&MidiEvent::note_on(0, 0, 57, 100));
        let (l, r) = stereo(&mut s, 40);
        assert!(
            l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6),
            "mono at no spread"
        );
        // The most voices on every note stays finite and bounded.
        let mut s = synth(&[
            (p::WAVE, 0.0),
            (p::OSC1_VOICES, 16.0),
            (p::OSC2_LEVEL, 100.0),
            (p::OSC2_VOICES, 16.0),
            (p::SUB_LEVEL, 100.0),
            (p::NOISE, 100.0),
        ]);
        for key in 50..66 {
            s.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let (l, _) = stereo(&mut s, 40);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 8.0));
    }

    #[test]
    fn mono_glides_and_falls_back_to_the_held_key() {
        let mut s = synth(&[(p::MODE, 2.0), (p::GLIDE, 100.0), (p::OSC1_VOICES, 1.0)]);
        s.midi(&MidiEvent::note_on(0, 0, 57, 100));
        stereo(&mut s, 40);
        s.midi(&MidiEvent::note_on(0, 0, 69, 100));
        // Early in the glide the pitch is still on its way up (A4 is 0 semitones).
        stereo(&mut s, 4);
        let early = s.voices[0].note;
        assert!(early < -1.0 && early > -12.0, "{early}");
        stereo(&mut s, 80);
        assert!(s.voices[0].note.abs() < 0.05, "on A4");
        assert_eq!(
            s.voices.iter().filter(|v| v.stage != Stage::Idle).count(),
            1,
            "one voice"
        );
        // Releasing the top key goes back to the one still held.
        s.midi(&MidiEvent::note_off(0, 0, 69));
        stereo(&mut s, 80);
        assert!((s.voices[0].note + 12.0).abs() < 0.05, "back to A3");
    }

    #[test]
    fn every_filter_type_shapes_the_spectrum_and_stays_bounded() {
        // A saw at A2 through each type at 500 Hz: low-passes keep the fundamental, the
        // high-pass loses it.
        let level = |kind: usize, resonance: f32| {
            let mut s = synth(&[
                (p::WAVE, 0.0),
                (p::OSC1_VOICES, 1.0),
                (p::FILTER, kind as f32),
                (p::CUTOFF, 500.0),
                (p::ENV_AMOUNT, 0.0),
                (p::RESONANCE, resonance),
            ]);
            s.midi(&MidiEvent::note_on(0, 0, 45, 100));
            let (l, _) = stereo(&mut s, 60);
            assert!(l.iter().all(|x| x.is_finite() && x.abs() < 4.0), "{kind}");
            l[l.len() / 2..].iter().map(|x| x * x).sum::<f32>()
        };
        let low = level(LP12, 0.0);
        let steep = level(LP24, 0.0);
        assert!(steep < low, "24 dB cuts more: {steep} {low}");
        assert!(level(HP, 0.0) < low * 0.5, "the high-pass loses the lows");
        assert!(level(LADDER, 0.0) > 0.0);
        // Full resonance on the ladder rings by itself, bounded.
        for kind in 0..FILTERS.len() {
            level(kind, 95.0);
        }
    }

    #[test]
    fn key_tracking_opens_the_filter_with_the_notes() {
        let bright = |key: u8, track: f32| {
            let mut s = synth(&[
                (p::WAVE, 0.0),
                (p::OSC1_VOICES, 1.0),
                (p::CUTOFF, 400.0),
                (p::ENV_AMOUNT, 0.0),
                (p::KEY_TRACK, track),
            ]);
            s.midi(&MidiEvent::note_on(0, 0, key, 100));
            let (l, _) = stereo(&mut s, 40);
            l[l.len() / 2..].iter().map(|x| x * x).sum::<f32>()
        };
        // Two octaves up with full tracking keeps much more of the sound than without.
        assert!(bright(84, 100.0) > bright(84, 0.0) * 2.0);
    }

    fn route(slot: usize, source: usize, target: usize, amount: f32) -> [(usize, f32); 3] {
        let at = p::SLOTS + slot * 3;
        [
            (at, source as f32),
            (at + 1, target as f32),
            (at + 2, amount),
        ]
    }

    #[test]
    fn an_lfo_on_the_pitch_makes_a_vibrato() {
        let mut settings = vec![(p::OSC1_VOICES, 1.0), (p::LFO + 1, 5.0)];
        settings.extend(route(0, 1, target::PITCH, 10.0));
        let mut s = synth(&settings);
        s.midi(&MidiEvent::note_on(0, 0, 69, 100));
        // The pitch, block by block, swings above and below A4.
        let mut lowest = f32::MAX;
        let mut highest = f32::MIN;
        for _ in 0..60 {
            stereo(&mut s, 1);
            let v = s.voices.iter().find(|v| v.stage != Stage::Idle).unwrap();
            let lfo = lfo_value(0, v.lfo[0], 0.0);
            lowest = lowest.min(lfo);
            highest = highest.max(lfo);
        }
        assert!(lowest < -0.5 && highest > 0.5, "{lowest} {highest}");
        let wobble = |amount: f32| {
            let mut settings = vec![(p::OSC1_VOICES, 1.0), (p::LFO + 1, 3.0)];
            settings.extend(route(0, 1, target::PITCH, amount));
            let mut s = synth(&settings);
            s.midi(&MidiEvent::note_on(0, 0, 69, 100));
            let (l, _) = stereo(&mut s, 200);
            // Crossings per window vary when the pitch moves.
            let counts: Vec<usize> = l
                .chunks(4800)
                .map(|c| c.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count())
                .collect();
            counts.iter().max().unwrap() - counts.iter().min().unwrap()
        };
        assert!(wobble(20.0) > wobble(0.0) + 3);
    }

    #[test]
    fn macros_velocity_and_the_second_envelope_reach_their_targets() {
        let energy = |settings: &[(usize, f32)], velocity: u8| {
            let mut s = synth(settings);
            s.midi(&MidiEvent::note_on(0, 0, 57, velocity));
            let (l, _) = stereo(&mut s, 40);
            l[l.len() / 2..].iter().map(|x| x * x).sum::<f32>()
        };
        let base = [(p::WAVE, 0.0), (p::CUTOFF, 300.0), (p::ENV_AMOUNT, 0.0)];
        let closed = energy(&base, 100);
        let mut opened = base.to_vec();
        opened.push((p::MACROS, 100.0));
        opened.extend(route(2, 7, target::CUTOFF, 60.0));
        assert!(
            energy(&opened, 100) > closed * 2.0,
            "macro 1 opens the filter"
        );
        // Velocity on the volume: a soft note is quieter than a hard one, beyond the usual.
        let mut by_velocity = base.to_vec();
        by_velocity.extend(route(1, 4, target::VOLUME, 100.0));
        let ratio = energy(&by_velocity, 127) / energy(&by_velocity, 30).max(1e-9);
        let usual = energy(&base, 127) / energy(&base, 30).max(1e-9);
        assert!(ratio > usual * 1.5, "{ratio} {usual}");
        // The second envelope at full sustain holds the filter open.
        let mut env = base.to_vec();
        env.push((p::ENV2 + 2, 100.0));
        env.extend(route(3, 3, target::CUTOFF, 60.0));
        assert!(energy(&env, 100) > closed * 2.0);
    }

    #[test]
    fn the_meters_show_the_newest_note() {
        let mut s = synth(&[(p::OSC1_VOICES, 1.0)]);
        let mut out = [0.0; meter::COUNT];
        s.meters(&mut out);
        assert_eq!(out[meter::VOICES], 0.0);
        s.midi(&MidiEvent::note_on(0, 0, 57, 100));
        stereo(&mut s, 120);
        s.meters(&mut out);
        assert_eq!(out[meter::VOICES], 1.0);
        assert_eq!(out[meter::STAGE], 3.0, "sustaining");
        assert!(out[meter::CUTOFF] > 1000.0);
        // The oscilloscope starts on a rising crossing and swings both ways.
        let scope = &out[..meter::SCOPE];
        assert!(scope[0] >= 0.0 && scope.iter().any(|s| *s < -0.01));
    }

    #[test]
    fn the_wavetable_position_changes_the_wave_and_files_load() {
        // Basic shapes: a sine at the start, a square at the end (crest factor 1.41, then 1).
        let crest = |position: f32| {
            let mut s = synth(&[
                (p::WAVE, WAVETABLE as f32),
                (p::OSC1_VOICES, 1.0),
                (p::TABLES + 1, position),
                (p::DRIFT, 0.0),
            ]);
            s.midi(&MidiEvent::note_on(0, 0, 45, 100));
            let (l, _) = stereo(&mut s, 120);
            let tail = &l[l.len() / 2..];
            let peak = tail.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            let rms = (tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).sqrt();
            peak / rms
        };
        let (sine, square) = (crest(0.0), crest(100.0));
        assert!(sine > square + 0.2, "{sine} {square}");
        // A file of two frames loads and plays.
        let mut s = synth(&[(p::WAVE, WAVETABLE as f32), (p::TABLES, LOADED as f32)]);
        let samples: Vec<f32> = (0..crate::wavetable::SIZE * 2)
            .map(|i| if i % 2048 < 1024 { 0.8 } else { -0.8 })
            .collect();
        let told = s
            .load(
                "table1",
                crate::FileData::Audio {
                    channels: vec![samples],
                    rate: 48_000,
                },
            )
            .unwrap();
        assert_eq!(told, "2 frames");
        s.midi(&MidiEvent::note_on(0, 0, 100, 100));
        let (l, _) = stereo(&mut s, 20);
        assert!(l.iter().all(|x| x.is_finite()) && l.iter().any(|x| x.abs() > 0.01));
    }

    /// Normalized correlation of a signal with itself `lag` samples later.
    fn correlation(x: &[f32], lag: usize) -> f32 {
        let (a, b) = (&x[..x.len() - lag], &x[lag..]);
        let dot: f32 = a.iter().zip(b).map(|(p, q)| p * q).sum();
        let norm =
            (a.iter().map(|p| p * p).sum::<f32>() * b.iter().map(|q| q * q).sum::<f32>()).sqrt();
        dot / norm.max(1e-9)
    }

    /// Level of the `f` Hz component.
    fn bin(x: &[f32], f: f32) -> f32 {
        let w = std::f32::consts::TAU * f / 48_000.0;
        let (re, im) = x.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, s)| {
            (re + s * (w * i as f32).cos(), im + s * (w * i as f32).sin())
        });
        (re * re + im * im).sqrt() / x.len() as f32
    }

    #[test]
    fn oscillator_2_drives_oscillator_1() {
        let base = [
            (p::OSC1_VOICES, 1.0),
            (p::OSC2_VOICES, 1.0),
            (p::OSC2_WAVE, 2.0),
        ];
        let play = |extra: &[(usize, f32)]| {
            let mut s = synth(&[base.as_slice(), extra].concat());
            s.midi(&MidiEvent::note_on(0, 0, 57, 100));
            let (l, _) = stereo(&mut s, 80);
            l[l.len() / 2..].to_vec()
        };
        // FM: a pure sine gains harmonics.
        let (plain, fm) = (play(&[]), play(&[(p::FM, 60.0)]));
        assert!(bin(&fm, 440.0) > bin(&plain, 440.0) * 10.0 + 1e-4);
        // Sync: oscillator 1 a fifth up restarts with oscillator 2, at the note's period.
        let fifth = [(p::WAVE, 0.0), (p::OSC1_SEMI, 7.0)];
        let free = play(&fifth);
        let synced = play(&[fifth.as_slice(), &[(p::SYNC, 1.0)]].concat());
        let period = (48_000.0f32 / 220.0).round() as usize;
        assert!(correlation(&synced, period) > 0.9);
        assert!(correlation(&free, period) < correlation(&synced, period) - 0.2);
        // Ring: both oscillators silent, their product still sounds.
        let ring = play(&[(p::OSC1_LEVEL, 0.0), (p::RING, 100.0)]);
        assert!(ring.iter().any(|x| x.abs() > 0.01) && ring.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn the_effects_ring_on_after_the_note() {
        let tail = |reverb: f32| {
            let mut s = synth(&[(p::RELEASE, 10.0), (p::REVERB, reverb)]);
            s.midi(&MidiEvent::note_on(0, 0, 60, 100));
            stereo(&mut s, 40);
            s.midi(&MidiEvent::note_off(0, 0, 60));
            let (l, _) = stereo(&mut s, 60);
            l[4800..].iter().map(|x| x * x).sum::<f32>()
        };
        assert!(tail(50.0) > tail(0.0) * 100.0 + 1e-6);
    }
}
