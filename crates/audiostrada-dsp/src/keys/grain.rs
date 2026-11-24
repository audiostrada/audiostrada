//! Strada-Grain: a granular instrument. It plays a sound back as many short grains laid over
//! each other, each taken around a place in the sound (position and spray), transposed by the
//! key, a little detuned, panned, sometimes reversed, and faded in and out so that none
//! clicks. The sound is a file, one of the sources made here, or the live input heard through
//! the sidechain (or the track's own input), kept in a ten-second memory that freezing stops.
//! Keys play the grains, or in Continuous they stream on their own at the sound's pitch. Then the
//! filters, chorus, delay and a reverb that can be vast.

use std::f32::consts::TAU;
use std::sync::{Arc, LazyLock};

use super::common::{Amp, AmpSettings};
use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::delay::NOTE_NAMES;
use crate::{
    Effect, EffectInfo, FileData, FileKind, FileSlot, MidiEvent, ParamInfo, choice, param,
};

/// What the meters hold: an outline of the sound, where the grains play, and the region.
pub mod meter {
    pub const OUTLINE: usize = 0;
    pub const BINS: usize = 128;
    pub const GRAINS: usize = BINS;
    pub const SHOWN: usize = 32;
    /// The region's start and width, from 0 to 1 across the outline.
    pub const REGION: usize = GRAINS + SHOWN;
    pub const COUNT: usize = REGION + 2;
}

static METERS: [&str; meter::COUNT] = [""; meter::COUNT];

pub const INFO: EffectInfo = EffectInfo {
    id: "grain",
    name: "Strada-Grain",
    params: &PARAMS,
    meters: &METERS,
    instrument: true,
    files: &[FileSlot {
        id: "sample",
        name: "Sound",
        kind: FileKind::Audio,
        extensions: &["wav", "flac", "aif", "aiff", "ogg", "mp3"],
    }],
    category: "Instrument",
    description: "Granular instrument: a sound, a source of its own or the live sidechain turned \
                  into textures, pads and frozen moments.",
};

pub const SOURCES: [&str; 9] = [
    "File",
    "Live",
    "Choir",
    "Strings",
    "Organ",
    "Wind",
    "Bells",
    "Felt piano",
    "Glass",
];
pub const FILE: usize = 0;
pub const LIVE: usize = 1;
pub const TRIGGERS: [&str; 2] = ["Keyboard", "Continuous"];
pub const FREEZE: [&str; 2] = ["Off", "On"];

pub mod p {
    pub const VOLUME: usize = 0;
    pub const SOURCE: usize = 1;
    pub const TRIGGER: usize = 2;
    pub const POSITION: usize = 3;
    pub const SPRAY: usize = 4;
    pub const SIZE: usize = 5;
    pub const DENSITY: usize = 6;
    pub const SCAN: usize = 7;
    pub const FREEZE: usize = 8;
    pub const DETUNE: usize = 9;
    pub const WIDTH: usize = 10;
    pub const REVERSE: usize = 11;
    pub const SHAPE: usize = 12;
    pub const ATTACK: usize = 13;
    pub const RELEASE: usize = 14;
    pub const LOW_CUT: usize = 15;
    pub const HIGH_CUT: usize = 16;
    pub const DYNAMICS: usize = 17;
    pub const TUNE: usize = 18;
    pub const CHORUS: usize = 19;
    pub const DELAY: usize = 20;
    pub const DELAY_NOTE: usize = 21;
    pub const DELAY_FEEDBACK: usize = 22;
    pub const REVERB: usize = 23;
    pub const REVERB_DECAY: usize = 24;
    pub const COUNT: usize = 25;
}

const PARAMS: [ParamInfo; p::COUNT] = [
    param("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
    choice("source", "Source", &SOURCES, 2.0),
    choice("trigger", "Trigger", &TRIGGERS, 0.0),
    param("position", "Position", 0.0, 100.0, 30.0, "%"),
    param("spray", "Spray", 0.0, 100.0, 15.0, "%"),
    param("size", "Grain size", 10.0, 500.0, 120.0, "ms"),
    param("density", "Density", 1.0, 100.0, 30.0, "/s"),
    param("scan", "Scan speed", -100.0, 100.0, 0.0, "%"),
    choice("freeze", "Freeze", &FREEZE, 0.0),
    param("detune", "Detune", 0.0, 100.0, 10.0, "cents"),
    param("width", "Width", 0.0, 100.0, 70.0, "%"),
    param("reverse", "Reversed grains", 0.0, 100.0, 0.0, "%"),
    param("shape", "Grain shape", 0.0, 100.0, 30.0, "%"),
    param("attack", "Attack", 1.0, 10_000.0, 300.0, "ms"),
    param("release", "Release", 10.0, 10_000.0, 2000.0, "ms"),
    param("low_cut", "Low cut", 20.0, 2000.0, 40.0, "Hz"),
    param("high_cut", "High cut", 500.0, 20_000.0, 12_000.0, "Hz"),
    param("dynamics", "Velocity", 0.0, 100.0, 40.0, "%"),
    param("tune", "Tune", -24.0, 24.0, 0.0, "st"),
    param("chorus", "Chorus", 0.0, 100.0, 0.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 35.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 35.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 3.5, "s"),
];

/// The note the sounds play at their own pitch.
const ROOT: f32 = 48.0;
const GRAINS: usize = 256;
const VOICES: usize = 16;
/// The voice that streams grains in Continuous.
const AUTO: usize = VOICES;
const LIVE_SECONDS: f32 = 10.0;
const OUTPUT: f32 = 0.5;

// ---------------------------------------------------------------- The sources made here

/// The rate the sources are made at, and their length.
const SOURCE_RATE: u32 = 48_000;
const SOURCE_SECONDS: f32 = 4.0;

/// The sources made here, in [`SOURCES`] order after File and Live, at C3.
pub static FACTORY: LazyLock<Vec<Arc<[f32]>>> = LazyLock::new(|| {
    (0..SOURCES.len() - 2)
        .map(|k| Arc::from(make(k).into_boxed_slice()))
        .collect()
});

fn hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A source: sung vowels, a string chord, an organ chord, wind, bells, a felt piano, glass.
fn make(kind: usize) -> Vec<f32> {
    let rate = SOURCE_RATE as f32;
    let n = (SOURCE_SECONDS * rate) as usize;
    let mut out = vec![0.0f32; n];
    let mut seed = 0x9e37_79b9u32 ^ kind as u32;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    match kind {
        0 => {
            // Three voices on a fifth and a third, through the formants of "ah" turning to "oh".
            let notes = [48.0, 55.0, 64.0];
            let mut phases = [0.0f32; 3];
            let mut bands = [[Biquad::default(); 3]; 2];
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let mut saw = 0.0;
                for (k, ph) in phases.iter_mut().enumerate() {
                    let vib = 1.0 + 0.006 * (TAU * (5.0 + k as f32 * 0.3) * t).sin();
                    *ph = (*ph + hz(notes[k]) * vib / rate).fract();
                    saw += 2.0 * *ph - 1.0;
                }
                let morph = 0.5 - 0.5 * (TAU * t / SOURCE_SECONDS).cos();
                let formants = [
                    (700.0 + (450.0 - 700.0) * morph, 1.0),
                    (1220.0 + (800.0 - 1220.0) * morph, 0.5),
                    (2600.0, 0.25),
                ];
                let mut y = 0.0;
                for (f, (freq, level)) in formants.iter().enumerate() {
                    let c = Coefficients::design(
                        FilterKind::BandPass,
                        (*freq).into(),
                        8.0,
                        0.0,
                        rate.into(),
                    );
                    let mut v = f64::from(saw);
                    for band in &mut bands {
                        v = band[f].process(&c, v);
                    }
                    y += v as f32 * level;
                }
                *o = y * 0.6;
            }
        }
        1 => {
            // A chord of detuned saws, slowly swelling.
            let notes = [48.0, 52.0, 55.0, 59.0];
            let mut phases = [[0.0f32; 5]; 4];
            let mut low = 0.0f32;
            let k_low = crate::lowpass_coefficient(2500.0, rate);
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let mut s = 0.0;
                for (v, row) in phases.iter_mut().enumerate() {
                    for (d, ph) in row.iter_mut().enumerate() {
                        let cents = (d as f32 - 2.0) * 7.0;
                        *ph = (*ph + hz(notes[v]) * 2f32.powf(cents / 1200.0) / rate).fract();
                        s += 2.0 * *ph - 1.0;
                    }
                }
                low += k_low * (s - low);
                let swell = 0.6 + 0.4 * (TAU * 0.25 * t).sin();
                *o = low * 0.06 * swell;
            }
        }
        2 => {
            // An organ chord: the harmonics of the drawbars on a fifth.
            let parts = [
                (1.0, 1.0),
                (2.0, 0.7),
                (3.0, 0.5),
                (4.0, 0.4),
                (6.0, 0.2),
                (8.0, 0.15),
            ];
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let mut s = 0.0;
                for note in [48.0, 55.0] {
                    for (h, level) in parts {
                        s += (TAU * hz(note) * h * t).sin() * level;
                    }
                }
                *o = s * 0.1;
            }
        }
        3 => {
            // Wind: noise through two resonances wandering slowly.
            let mut bands = [Biquad::default(); 2];
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let x = f64::from(noise());
                let mut y = 0.0;
                for (b, band) in bands.iter_mut().enumerate() {
                    let f =
                        400.0 + 900.0 * (0.5 + 0.5 * (TAU * (0.11 + 0.07 * b as f32) * t).sin());
                    let c =
                        Coefficients::design(FilterKind::BandPass, f.into(), 4.0, 0.0, rate.into());
                    y += band.process(&c, x) as f32;
                }
                *o = y * 0.5 * (0.6 + 0.4 * (TAU * 0.3 * t).sin());
            }
        }
        4 => {
            // Bells struck one after the other.
            for (k, note) in [60.0, 67.0, 76.0, 72.0].into_iter().enumerate() {
                let start = (k as f32 * rate) as usize;
                for (h, level) in [(1.0, 1.0), (2.76, 0.6), (5.4, 0.4), (8.93, 0.2)] {
                    let f = hz(note) * h;
                    let fall = 1.0 / (1.6 / h.sqrt());
                    for (j, o) in out[start..].iter_mut().enumerate() {
                        let t = j as f32 / rate;
                        *o += (TAU * f * t).sin() * level * (-t * fall * 2.0).exp() * 0.2;
                    }
                }
            }
        }
        5 => {
            // A felt piano: soft notes rising, their harmonics fading first.
            for (k, note) in [48.0, 52.0, 55.0, 60.0].into_iter().enumerate() {
                let start = (k as f32 * rate) as usize;
                for h in 1..=6 {
                    let f = hz(note) * h as f32;
                    let level = 0.5 / (h * h) as f32;
                    for (j, o) in out[start..].iter_mut().enumerate() {
                        let t = j as f32 / rate;
                        *o += (TAU * f * t).sin() * level * (-t * (0.8 + h as f32 * 0.6)).exp();
                    }
                }
            }
        }
        _ => {
            // Glass: high sines in beating pairs.
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let mut s = 0.0;
                for (note, level) in [(72.0, 0.5), (79.0, 0.3), (84.0, 0.25), (91.0, 0.12)] {
                    let f = hz(note);
                    s += ((TAU * f * t).sin() + (TAU * f * 1.003 * t).sin()) * level;
                }
                *o = s * 0.25;
            }
        }
    }
    // Every source at the same peak, without a click at its ends.
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
    let fade = (0.01 * rate) as usize;
    for (i, x) in out.iter_mut().enumerate() {
        let edge = (i.min(n - 1 - i) as f32 / fade as f32).min(1.0);
        *x *= 0.8 / peak * edge;
    }
    out
}

// ---------------------------------------------------------------- The instrument

/// The sound of `source`: the file, the live memory or a source made here, and its rate.
fn sound_of<'a>(
    source: usize,
    file: &'a Option<(Arc<[f32]>, f32)>,
    live: &'a [f32],
    rate: f32,
) -> Option<(&'a [f32], f32)> {
    match source {
        FILE => file.as_ref().map(|(s, r)| (&s[..], *r)),
        LIVE => Some((live, rate)),
        k => FACTORY.get(k - 2).map(|s| (&s[..], SOURCE_RATE as f32)),
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Grain {
    on: bool,
    voice: usize,
    /// Where it reads in the sound, in samples, and its step per sample (negative backward).
    at: f64,
    step: f32,
    age: u32,
    length: u32,
    gains: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    on: bool,
    down: bool,
    key: u8,
    gain: f32,
    level: f32,
    releasing: bool,
    /// Samples until the next grain.
    next: f32,
}

#[derive(Debug)]
pub struct GrainSynth {
    rate: f32,
    values: [f32; p::COUNT],
    voices: [Voice; VOICES + 1],
    grains: Box<[Grain; GRAINS]>,
    seed: u32,
    /// The loaded file (mono) and its rate.
    file: Option<(Arc<[f32]>, f32)>,
    /// The live memory, where it writes next, and the outline of what it holds.
    live: Vec<f32>,
    /// The live memory again, for the interface to save.
    recording: Option<Arc<crate::Recording>>,
    write: usize,
    live_outline: [f32; meter::BINS],
    /// The outline of the source last measured, and which it was.
    outline: [f32; meter::BINS],
    outlined: usize,
    /// How far the scan has moved the region, from 0 to 1.
    scan: f32,
    pedal: bool,
    filters: [[Biquad; 2]; 2],
    cuts: (f32, f32),
    coefficients: [Coefficients; 2],
    amp: Amp,
}

impl Default for GrainSynth {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: [Voice::default(); VOICES + 1],
            grains: Box::new([Grain::default(); GRAINS]),
            seed: 0x2468_ace1,
            file: None,
            live: Vec::new(),
            recording: None,
            write: 0,
            live_outline: [0.0; meter::BINS],
            outline: [0.0; meter::BINS],
            outlined: usize::MAX,
            scan: 0.0,
            pedal: false,
            filters: [[Biquad::default(); 2]; 2],
            cuts: (0.0, 0.0),
            coefficients: [Coefficients::IDENTITY; 2],
            amp: Amp::default(),
        }
    }
}

impl GrainSynth {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn source(&self) -> usize {
        (self.value(p::SOURCE).round().max(0.0) as usize).min(SOURCES.len() - 1)
    }

    fn frozen(&self) -> bool {
        self.value(p::FREEZE) >= 0.5
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    /// The sound the grains read, and its rate; `None` when there is none (no file loaded).
    fn sound(&self) -> Option<(&[f32], f32)> {
        sound_of(self.source(), &self.file, &self.live, self.rate)
    }

    fn press(&mut self, key: u8, velocity: u8) {
        let dynamics = self.unit(p::DYNAMICS);
        let gain = 1.0 - dynamics * (1.0 - f32::from(velocity) / 127.0);
        let index = self.voices[..VOICES]
            .iter()
            .position(|v| v.on && v.key == key)
            .or_else(|| self.voices[..VOICES].iter().position(|v| !v.on))
            .unwrap_or_else(|| {
                // The quietest voice gives way.
                (0..VOICES)
                    .min_by(|&a, &b| self.voices[a].level.total_cmp(&self.voices[b].level))
                    .unwrap_or(0)
            });
        let v = &mut self.voices[index];
        if !v.on {
            v.level = 0.0;
            v.next = 0.0;
        }
        v.on = true;
        v.down = true;
        v.releasing = false;
        v.key = key;
        v.gain = gain;
    }

    fn release(&mut self, key: u8) {
        let pedal = self.pedal;
        for v in &mut self.voices[..VOICES] {
            if v.on && v.down && v.key == key {
                v.down = false;
                v.releasing = !pedal;
            }
        }
    }

    /// Starts a grain of voice `v`.
    fn spawn(&mut self, v: usize, len: usize, source_rate: f32) {
        let Some(slot) = self.grains.iter().position(|g| !g.on) else {
            return;
        };
        let size = (self.value(p::SIZE) * 0.001 * self.rate) as u32;
        let key = if v == AUTO {
            ROOT
        } else {
            f32::from(self.voices[v].key)
        };
        let detune = self.value(p::DETUNE) * (self.random() * 2.0 - 1.0);
        let pitch = key - ROOT + self.value(p::TUNE) + detune / 100.0;
        let mut step = source_rate / self.rate * 2f32.powf(pitch / 12.0);
        let reverse = self.random() < self.unit(p::REVERSE);
        let spray = self.unit(p::SPRAY) * (self.random() * 2.0 - 1.0);
        let span = f64::from(step) * f64::from(size);
        let at = if self.source() == LIVE {
            // Back from where it writes: never overtaking it, nor reading what it overwrites.
            let margin = span.max(f64::from(size)) + 64.0;
            let room = (len as f64 - 2.0 * margin).max(1.0);
            let back =
                margin + room * f64::from((self.unit(p::POSITION) + spray * 0.5).clamp(0.0, 1.0));
            (self.write as f64 - back).rem_euclid(len as f64)
        } else {
            let place = (self.unit(p::POSITION) + self.scan + spray * 0.5).rem_euclid(1.0);
            f64::from(place) * len as f64
        };
        if reverse {
            step = -step;
        }
        let pan = self.unit(p::WIDTH) * (self.random() * 2.0 - 1.0);
        let gains = [(0.5 * (1.0 - pan)).sqrt(), (0.5 * (1.0 + pan)).sqrt()];
        self.grains[slot] = Grain {
            on: true,
            voice: v,
            at: if reverse { at + span } else { at },
            step,
            age: 0,
            length: size.max(16),
            gains,
        };
    }

    /// The outline of the sound for the screen: its peaks in [`meter::BINS`] bins.
    fn measure(&mut self) {
        let which = self.source();
        if which == LIVE || which == self.outlined {
            return;
        }
        self.outlined = which;
        let mut outline = [0.0f32; meter::BINS];
        if let Some((sound, _)) = self.sound() {
            let per = sound.len().div_ceil(meter::BINS).max(1);
            for (b, chunk) in sound.chunks(per).enumerate().take(meter::BINS) {
                outline[b] = chunk.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            }
        }
        self.outline = outline;
    }

    fn amp_settings(&self) -> AmpSettings {
        AmpSettings {
            bass: 0.0,
            treble: 0.0,
            drive: 0.0,
            tremolo: 1,
            rate: 1.0,
            depth: 0.0,
            phaser: 0.0,
            chorus: self.value(p::CHORUS),
            delay: self.value(p::DELAY),
            reverb: self.value(p::REVERB),
            delay_note: self.value(p::DELAY_NOTE),
            feedback: self.value(p::DELAY_FEEDBACK),
            decay: self.value(p::REVERB_DECAY),
            volume_db: self.value(p::VOLUME),
            speaker: false,
        }
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32], input: Option<(&[f32], &[f32])>) {
        let n = left.len();
        let rate = self.rate;
        // The live memory takes the input, unless frozen.
        let live = self.source() == LIVE;
        if !self.frozen() && !self.live.is_empty() {
            let len = self.live.len();
            for i in 0..n {
                let x = match input {
                    Some((l, r)) => {
                        0.5 * (l.get(i).copied().unwrap_or(0.0) + r.get(i).copied().unwrap_or(0.0))
                    }
                    None => 0.5 * (left[i] + right[i]),
                };
                let bin = self.write * meter::BINS / len;
                if (self.write * meter::BINS) % len < meter::BINS {
                    self.live_outline[bin] = 0.0;
                }
                self.live_outline[bin] = self.live_outline[bin].max(x.abs());
                self.live[self.write] = x;
                if let Some(r) = &self.recording {
                    r.store(self.write, x);
                }
                self.write = (self.write + 1) % len;
            }
        }
        left.fill(0.0);
        right.fill(0.0);
        self.measure();
        let auto = self.value(p::TRIGGER) >= 0.5;
        self.voices[AUTO].on = auto;
        if auto {
            self.voices[AUTO].gain = 1.0;
            self.voices[AUTO].releasing = false;
        }
        let Some((len, source_rate)) = self.sound().map(|(s, r)| (s.len(), r)) else {
            return;
        };
        if len < 64 {
            return;
        }
        if !self.frozen() && !live {
            // The scan moves the region through the sound at its speed (1 = as recorded).
            self.scan = (self.scan
                + self.unit(p::SCAN) * n as f32 * source_rate / rate / len as f32)
                .rem_euclid(1.0);
        }
        let attack = 1.0 / (self.value(p::ATTACK) * 0.001 * rate).max(1.0);
        let release = super::modal::decay(self.value(p::RELEASE) * 0.001, rate);
        let density = self.value(p::DENSITY);
        let size = self.value(p::SIZE) * 0.001;
        // Many grains overlapping add up: brought back to an even level.
        let norm = OUTPUT / (density * size).max(1.0).sqrt();
        let shape = self.unit(p::SHAPE);
        let source = self.source();
        let fade = 0.5 * (1.0 - shape) + 0.06 * shape;
        for i in 0..n {
            for v in 0..=VOICES {
                if !self.voices[v].on {
                    continue;
                }
                {
                    let voice = &mut self.voices[v];
                    if voice.releasing {
                        voice.level *= release;
                        if voice.level < 1e-4 {
                            voice.on = false;
                            continue;
                        }
                    } else {
                        voice.level = (voice.level + attack).min(1.0);
                    }
                    voice.next -= 1.0;
                }
                if self.voices[v].next <= 0.0 {
                    let jitter = 0.75 + 0.5 * self.random();
                    self.voices[v].next += rate / density * jitter;
                    self.spawn(v, len, source_rate);
                }
            }
            let (mut l, mut r) = (0.0f32, 0.0f32);
            let Some((sound, _)) = sound_of(source, &self.file, &self.live, rate) else {
                return;
            };
            let sound_len = sound.len() as f64;
            for g in self.grains.iter_mut().filter(|g| g.on) {
                let x = g.age as f32 / g.length as f32;
                let edge = (x.min(1.0 - x) / fade).min(1.0);
                let env = edge * edge * (3.0 - 2.0 * edge);
                let at = g.at.rem_euclid(sound_len);
                let k = at.floor() as usize;
                let frac = (at - k as f64) as f32;
                let a = sound[k % sound.len()];
                let b = sound[(k + 1) % sound.len()];
                let s = (a + (b - a) * frac) * env;
                let voice = &self.voices[g.voice];
                let level = voice.level * voice.gain * norm;
                l += s * g.gains[0] * level;
                r += s * g.gains[1] * level;
                g.at += f64::from(g.step);
                g.age += 1;
                if g.age >= g.length {
                    g.on = false;
                }
            }
            left[i] = l;
            right[i] = r;
        }
        // The low and high cuts.
        let cuts = (self.value(p::LOW_CUT), self.value(p::HIGH_CUT));
        if cuts != self.cuts {
            self.cuts = cuts;
            let r = f64::from(rate);
            self.coefficients = [
                Coefficients::design(FilterKind::HighPass, cuts.0.into(), 0.707, 0.0, r),
                Coefficients::design(FilterKind::LowPass, cuts.1.into(), 0.707, 0.0, r),
            ];
        }
        for (side, buffer) in [&mut *left, &mut *right].into_iter().enumerate() {
            for x in buffer.iter_mut() {
                let mut y = f64::from(*x);
                for (f, c) in self.filters[side].iter_mut().zip(&self.coefficients) {
                    y = f.process(c, y);
                }
                *x = y as f32;
            }
        }
    }
}

impl Effect for GrainSynth {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES + 1];
        for g in self.grains.iter_mut() {
            g.on = false;
        }
        self.live = vec![0.0; (LIVE_SECONDS * self.rate) as usize];
        self.recording = Some(Arc::new(crate::Recording::new(
            self.live.len(),
            sample_rate,
        )));
        self.write = 0;
        self.live_outline = [0.0; meter::BINS];
        self.outlined = usize::MAX;
        self.filters = [[Biquad::default(); 2]; 2];
        self.cuts = (0.0, 0.0);
        LazyLock::force(&FACTORY);
        self.amp.prepare(sample_rate, max_frames);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn recording(&self) -> Option<Arc<crate::Recording>> {
        self.recording.clone()
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.amp.set_tempo(bpm);
    }

    fn load(&mut self, slot: &str, file: FileData) -> Result<String, String> {
        if slot != "sample" {
            return Err(format!("no file slot {slot}"));
        }
        let FileData::Audio { channels, rate } = file else {
            return Err("the sound is an audio file".into());
        };
        let length = channels.iter().map(Vec::len).max().unwrap_or(0);
        if length == 0 {
            return Err("the file is empty".into());
        }
        let count = channels.len().max(1) as f32;
        let mono: Vec<f32> = (0..length)
            .map(|i| {
                channels
                    .iter()
                    .map(|c| c.get(i).copied().unwrap_or(0.0))
                    .sum::<f32>()
                    / count
            })
            .collect();
        self.file = Some((Arc::from(mono.into_boxed_slice()), rate as f32));
        self.outlined = usize::MAX;
        Ok(format!("{:.2} s", length as f32 / rate as f32))
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((key, velocity)) = event.as_note_on() {
            self.press(key, velocity);
        } else if let Some(key) = event.as_note_off() {
            self.release(key);
        } else if let Some((cc, value)) = event.as_control() {
            match cc {
                64 => {
                    self.pedal = value >= 64;
                    if !self.pedal {
                        for v in &mut self.voices[..VOICES] {
                            if v.on && !v.down {
                                v.releasing = true;
                            }
                        }
                    }
                }
                120 | 123 => {
                    for v in &mut self.voices[..VOICES] {
                        v.down = false;
                        v.releasing = true;
                        if cc == 120 {
                            v.on = false;
                        }
                    }
                    if cc == 120 {
                        for g in self.grains.iter_mut() {
                            g.on = false;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.render(left, right, None);
        let settings = self.amp_settings();
        self.amp.process(settings, left, right);
    }

    fn process_keyed(&mut self, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        self.render(left, right, Some(key));
        let settings = self.amp_settings();
        self.amp.process(settings, left, right);
    }

    fn meters(&self, out: &mut [f32]) {
        if out.len() < meter::COUNT {
            return;
        }
        let live = self.source() == LIVE;
        let len = if live {
            self.live.len()
        } else {
            self.sound().map_or(0, |(s, _)| s.len())
        };
        // Live, the outline runs from the oldest on the left to the newest on the right.
        let head = if live && len > 0 {
            self.write * meter::BINS / len
        } else {
            0
        };
        for (b, o) in out[..meter::BINS].iter_mut().enumerate() {
            *o = if live {
                self.live_outline[(head + 1 + b) % meter::BINS]
            } else {
                self.outline[b]
            };
        }
        let place = |at: f64| -> f32 {
            if len == 0 {
                return -1.0;
            }
            let x = (at.rem_euclid(len as f64) / len as f64) as f32;
            if live {
                (x - self.write as f32 / len as f32).rem_euclid(1.0)
            } else {
                x
            }
        };
        let mut shown = self.grains.iter().filter(|g| g.on);
        for o in &mut out[meter::GRAINS..meter::GRAINS + meter::SHOWN] {
            *o = shown.next().map_or(-1.0, |g| place(g.at));
        }
        let spray = self.unit(p::SPRAY);
        let start = if live {
            1.0 - self.unit(p::POSITION) - spray * 0.5
        } else {
            (self.unit(p::POSITION) + self.scan - spray * 0.5).rem_euclid(1.0)
        };
        out[meter::REGION] = start;
        out[meter::REGION + 1] = spray;
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            *v = Voice::default();
        }
        for g in self.grains.iter_mut() {
            g.on = false;
        }
        self.filters = [[Biquad::default(); 2]; 2];
        self.amp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn render(g: &mut GrainSynth, seconds: f32, key: Option<&[f32]>) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * RATE as f32) as usize;
        let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while left.len() < n {
            match key {
                Some(k) => {
                    let at = left.len() % k.len().max(1);
                    let end = (at + 256).min(k.len());
                    let chunk = &k[at..end];
                    g.process_keyed(&mut l, &mut r, (chunk, chunk));
                }
                None => g.process(&mut l, &mut r),
            }
            left.extend_from_slice(&l);
            right.extend_from_slice(&r);
        }
        (left, right)
    }

    #[test]
    fn the_live_memory_can_be_read_from_its_first_sound_and_freezing_keeps_it() {
        let mut g = synth(&[(p::SOURCE, LIVE as f32)]);
        let recording = g.recording().unwrap();
        render(&mut g, 1.0, Some(&[0.5; 256]));
        let (heard, rate) = recording.read();
        assert_eq!(rate, RATE);
        assert!((heard.len() as f32 / RATE as f32 - 1.0).abs() < 0.01);
        assert!(heard.iter().all(|x| (x - 0.5).abs() < 1e-6));
        g.set_param(p::FREEZE, 1.0);
        render(&mut g, 0.5, Some(&[0.25; 256]));
        assert_eq!(recording.read().0, heard);
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn synth(settings: &[(usize, f32)]) -> GrainSynth {
        let mut g = GrainSynth::default();
        g.prepare(RATE, 256);
        g.set_param(p::REVERB, 0.0);
        for &(i, v) in settings {
            g.set_param(i, v);
        }
        g
    }

    #[test]
    fn every_source_made_here_sounds_and_stays_under_full_scale() {
        for (source, name) in SOURCES.iter().enumerate().skip(2) {
            let mut g = synth(&[(p::SOURCE, source as f32), (p::ATTACK, 5.0)]);
            g.midi(&MidiEvent::note_on(0, 0, 60, 110));
            let (l, r) = render(&mut g, 1.0, None);
            assert!(rms(&l[24_000..]) > 0.005, "{name}");
            assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 1.0));
        }
    }

    #[test]
    fn notes_end_after_their_release_and_auto_streams_without_notes() {
        let mut g = synth(&[(p::ATTACK, 5.0), (p::RELEASE, 50.0)]);
        g.midi(&MidiEvent::note_on(0, 0, 60, 100));
        render(&mut g, 0.3, None);
        g.midi(&MidiEvent::note_off(0, 0, 60));
        render(&mut g, 1.0, None);
        let (quiet, _) = render(&mut g, 0.2, None);
        assert!(rms(&quiet) < 1e-4);
        g.set_param(p::TRIGGER, 1.0);
        let (auto, _) = render(&mut g, 1.0, None);
        assert!(rms(&auto[24_000..]) > 0.005);
    }

    #[test]
    fn the_live_input_is_heard_and_freezing_keeps_it() {
        let tone: Vec<f32> = (0..RATE as usize)
            .map(|i| (TAU * 330.0 * i as f32 / RATE as f32).sin() * 0.5)
            .collect();
        let silence = vec![0.0f32; RATE as usize];
        let mut g = synth(&[
            (p::SOURCE, LIVE as f32),
            (p::TRIGGER, 1.0),
            (p::POSITION, 0.0),
        ]);
        let (heard, _) = render(&mut g, 1.0, Some(&tone));
        assert!(rms(&heard[24_000..]) > 0.01);
        // Frozen, the tone stays though the input falls silent.
        g.set_param(p::FREEZE, 1.0);
        let (kept, _) = render(&mut g, 1.0, Some(&silence));
        assert!(rms(&kept[24_000..]) > 0.01);
        g.set_param(p::FREEZE, 0.0);
        let (gone, _) = render(&mut g, 11.0, Some(&silence));
        assert!(rms(&gone[gone.len() - 24_000..]) < 1e-3);
    }

    #[test]
    fn a_loaded_file_plays_and_the_meters_outline_it() {
        let mut g = synth(&[(p::SOURCE, FILE as f32), (p::ATTACK, 5.0)]);
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let told = g
            .load(
                "sample",
                FileData::Audio {
                    channels: vec![tone],
                    rate: 48_000,
                },
            )
            .unwrap();
        assert_eq!(told, "1.00 s");
        g.midi(&MidiEvent::note_on(0, 0, 48, 100));
        let (l, _) = render(&mut g, 0.5, None);
        assert!(rms(&l[12_000..]) > 0.01);
        let mut meters = [0.0; meter::COUNT];
        g.meters(&mut meters);
        assert!(meters[..meter::BINS].iter().all(|v| *v > 0.4));
        assert!(meters[meter::GRAINS] >= 0.0, "a grain is shown");
    }
}
