//! Polyphonic subtractive synthesizer: two oscillators (band-limited saw and square by
//! polyBLEP, sine, triangle), each with its octave, tuning, level and unison (up to 16 voices
//! spread in pitch and across the stereo field), a sub oscillator and noise; a resonant filter
//! with its envelope and an amplitude envelope. Analog drift gives each note its own small
//! offsets, glide slides between notes, mono and legato play one note at a time.
//! Pitch bend, channel pressure and (in MPE mode) CC 74 follow each voice's channel, so MPE
//! controllers bend and shape notes one by one. Out of MPE, the host maps CC 74 to the cutoff.

use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, db_to_gain, param, switch};

const VOICES: usize = 16;
/// Most unison voices per oscillator.
pub const MAX_UNISON: usize = 16;

pub const INFO: EffectInfo = EffectInfo {
    id: "synth",
    name: "Strada-Synth",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Polyphonic synthesizer: two oscillators with unison, sub, noise, analog drift, \
                  glide, MPE.",
};

pub const WAVES: [&str; 4] = ["Saw", "Square", "Sine", "Triangle"];
pub const SUB_OCTAVES: [&str; 2] = ["-1 octave", "-2 octaves"];
pub const MODES: [&str; 3] = ["Poly", "Mono", "Legato"];

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
}
const COUNT: usize = 32;

// The first thirteen keep their ids and places: projects store them.
const PARAMS: [ParamInfo; COUNT] = [
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
}

impl Default for ChannelState {
    fn default() -> Self {
        Self {
            bend: 0.0,
            pressure: 0.0,
            slide: 0.5,
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
    /// State of the state-variable filter of each side (its two integrators).
    filter: [[f32; 2]; 2],
    /// The note's own analog offsets: pitch in cents, cutoff in octaves, and a slow wander.
    drift_pitch: f32,
    drift_cutoff: f32,
    wander: f32,
    wander_rate: f32,
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
            filter: [[0.0; 2]; 2],
            drift_pitch: 0.0,
            drift_cutoff: 0.0,
            wander: 0.0,
            wander_rate: 0.0,
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
        }
    }
}

impl Synth {
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
            v.filter = [[0.0; 2]; 2];
        }
        if retrigger {
            v.stage = Stage::Attack;
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
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES];
        self.held_count = 0;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
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
        let damping = 2.0 * (1.0 - v[p::RESONANCE] / 100.0).max(0.05);
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
                wave.round().clamp(0.0, 3.0) as u32,
                offset,
                // Unison voices add up in power: the level holds as their number grows.
                level / (n as f32).sqrt(),
                n,
                detune,
            )
        });
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
                voice.note = voice.target + glide * (voice.note - voice.target);
                voice.wander += voice.wander_rate * wander_step;
                let cents = voice.drift_pitch + drift * 3.0 * voice.wander.sin();
                let base = voice.note + bend + cents / 100.0;
                let note_dt = 440.0 * 2f32.powf(base / 12.0) / self.rate;
                let mut out = [0.0f32; 2];
                for (o, &(wave, _, level, n, _)) in oscs.iter().enumerate() {
                    if level <= 0.0 {
                        continue;
                    }
                    for k in 0..n {
                        let dt = note_dt * ratios[o][k];
                        let phase = &mut voice.phases[o][k];
                        let s = oscillator(wave, *phase, dt) * level;
                        *phase += dt;
                        if *phase >= 1.0 {
                            *phase -= 1.0;
                        }
                        let (pl, pr) = pans[o][k];
                        out[0] += s * pl;
                        out[1] += s * pr;
                    }
                }
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
                let fc = (cutoff * brightness * (1.0 + env_amount * 4.0 * voice.level))
                    .min(self.rate * 0.45);
                // Trapezoidal state-variable filter (Simper), stable at any cutoff; low-pass.
                let g = (std::f32::consts::PI * fc / self.rate).tan();
                let a1 = 1.0 / (1.0 + g * (g + damping));
                let a2 = g * a1;
                let amp = voice.level * voice.velocity * gain;
                for (side, x) in out.into_iter().enumerate() {
                    let [ic1, ic2] = &mut voice.filter[side];
                    let v3 = x - *ic2;
                    let v1 = a1 * *ic1 + a2 * v3;
                    let v2 = *ic2 + a2 * *ic1 + g * a2 * v3;
                    *ic1 = 2.0 * v1 - *ic1;
                    *ic2 = 2.0 * v2 - *ic2;
                    let y = v2 * amp;
                    let y = if y.is_finite() {
                        y
                    } else {
                        // Never let a broken value reach the mix.
                        *ic1 = 0.0;
                        *ic2 = 0.0;
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
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.stage = Stage::Idle;
        }
        self.channels = [ChannelState::default(); 16];
        self.held_count = 0;
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
}
