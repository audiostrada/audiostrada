//! Strada-FM Keys: the frequency-modulation electric piano of the eighties. Two pairs of sine
//! operators per note, each a carrier whose phase a modulator pushes: the body (a modulator at
//! the carrier's frequency, its depth falling after the strike and growing with the velocity)
//! and the tine (a modulator far above, gone in a moment: the metallic ping that comes out
//! when the keys are hit hard). The pairs are slightly detuned from each other, which gives
//! the sound its shimmer; four characters change their ratios. Then the amplifier, tremolo,
//! phaser, the chorus it is known with, delay and reverb.

use std::f32::consts::TAU;

use super::common::{Amp, AmpSettings, TREMOLOS};
use super::modal::decay;
use super::voice::{CURVES, Change, Keyboard, VOICES, frequency, velocity};
use crate::delay::NOTE_NAMES;
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "fmkeys",
    name: "Strada-FM Keys",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Eighties FM electric piano, from glassy to bell-like, with its chorus, \
                  tremolo, delay and reverb.",
};

pub const CHARACTERS: [&str; 4] = ["Tine", "Bell", "Reed", "Glass"];

pub mod p {
    pub const VOLUME: usize = 0;
    pub const TONE: usize = 1;
    pub const TINE: usize = 2;
    pub const DECAY: usize = 3;
    pub const DAMPER: usize = 4;
    pub const DETUNE: usize = 5;
    pub const DYNAMICS: usize = 6;
    pub const CURVE: usize = 7;
    pub const CHARACTER: usize = 8;
    pub const BASS: usize = 9;
    pub const TREBLE: usize = 10;
    pub const DRIVE: usize = 11;
    pub const TREMOLO: usize = 12;
    pub const TREMOLO_RATE: usize = 13;
    pub const TREMOLO_DEPTH: usize = 14;
    pub const PHASER: usize = 15;
    pub const CHORUS: usize = 16;
    pub const TUNE: usize = 17;
    pub const STRETCH: usize = 18;
    pub const DELAY: usize = 19;
    pub const DELAY_NOTE: usize = 20;
    pub const DELAY_FEEDBACK: usize = 21;
    pub const REVERB: usize = 22;
    pub const REVERB_DECAY: usize = 23;
    pub const COUNT: usize = 24;
}

const PARAMS: [ParamInfo; p::COUNT] = [
    param("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
    param("tone", "Tone", 0.0, 100.0, 45.0, "%"),
    param("tine", "Tine", 0.0, 100.0, 60.0, "%"),
    param("decay", "Decay", 25.0, 200.0, 100.0, "%"),
    param("damper", "Release", 0.0, 100.0, 40.0, "%"),
    param("detune", "Detune", 0.0, 100.0, 35.0, "%"),
    param("dynamics", "Velocity", 0.0, 100.0, 60.0, "%"),
    choice("curve", "Velocity curve", &CURVES, 1.0),
    choice("character", "Character", &CHARACTERS, 0.0),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("treble", "Treble", -12.0, 12.0, 0.0, "dB"),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    choice("tremolo", "Tremolo", &TREMOLOS, 0.0),
    param("tremolo_rate", "Tremolo rate", 0.5, 10.0, 4.5, "Hz"),
    param("tremolo_depth", "Tremolo depth", 0.0, 100.0, 0.0, "%"),
    param("phaser", "Phaser", 0.0, 100.0, 0.0, "%"),
    param("chorus", "Chorus", 0.0, 100.0, 35.0, "%"),
    param("tune", "Tuning", 430.0, 450.0, 440.0, "Hz"),
    param("stretch", "Octave stretch", 0.0, 100.0, 0.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 30.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 15.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 1.6, "s"),
];

/// A character: the body's carrier and modulator ratios, the tine's, how deep the body's
/// modulation goes, and how long the body rings.
#[derive(Clone, Copy, Debug)]
struct Character {
    body: (f32, f32),
    tine: (f32, f32),
    depth: f32,
    ring: f32,
}

const SHAPES: [Character; 4] = [
    Character {
        body: (1.0, 1.0),
        tine: (1.0, 14.0),
        depth: 1.0,
        ring: 1.0,
    },
    Character {
        body: (1.0, 3.5),
        tine: (3.0, 7.0),
        depth: 0.8,
        ring: 1.6,
    },
    Character {
        body: (1.0, 2.0),
        tine: (1.0, 3.0),
        depth: 1.2,
        ring: 0.7,
    },
    Character {
        body: (2.0, 5.0),
        tine: (1.0, 17.0),
        depth: 0.7,
        ring: 1.2,
    },
];

const OUTPUT: f32 = 0.34;
/// The detune between the pairs at full Detune, in cents.
const DETUNE_CENTS: f32 = 7.0;

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    key: u8,
    channel: u8,
    /// Phases of the body's carrier and modulator, then the tine's, in cycles.
    phases: [f32; 4],
    steps: [f32; 4],
    hz: f32,
    /// The body's level and modulation depth, the tine's, each falling by its factor.
    body: f32,
    body_index: f32,
    body_floor: f32,
    tine: f32,
    tine_index: f32,
    falls: [f32; 4],
    /// The note's loudness and the tine's share; the levels as heard, gliding.
    loudness: f32,
    tine_level: f32,
    heard: [f32; 2],
    damped: bool,
}

#[derive(Debug)]
pub struct FmKeys {
    rate: f32,
    values: [f32; p::COUNT],
    keys: Keyboard,
    voices: Box<[Voice; VOICES]>,
    amp: Amp,
}

impl Default for FmKeys {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            keys: Keyboard::default(),
            voices: Box::new([Voice::default(); VOICES]),
            amp: Amp::default(),
        }
    }
}

impl FmKeys {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn character(&self) -> Character {
        SHAPES[(self.value(p::CHARACTER).round().max(0.0) as usize).min(SHAPES.len() - 1)]
    }

    fn hz(&self, key: u8) -> f32 {
        frequency(f32::from(key), self.value(p::TUNE), self.unit(p::STRETCH))
    }

    /// Steps per sample of each operator at `hz`.
    fn steps(&self, hz: f32) -> [f32; 4] {
        let c = self.character();
        let cents = self.unit(p::DETUNE) * DETUNE_CENTS / 2.0;
        let up = 2f32.powf(cents / 1200.0);
        let (body, tine) = (hz * up / self.rate, hz / up / self.rate);
        [
            body * c.body.0,
            body * c.body.1,
            tine * c.tine.0,
            tine * c.tine.1,
        ]
    }

    fn strike(&mut self, i: usize, key: u8, struck: u8, again: bool) {
        let rate = self.rate;
        let v = velocity(struck, self.value(p::CURVE));
        let dynamics = self.unit(p::DYNAMICS);
        let c = self.character();
        let k = ((f32::from(key) - 36.0) / 60.0).clamp(0.0, 1.0);
        let hz = self.hz(key) * self.keys.bend(self.keys.slots[i].channel);
        let steps = self.steps(hz);
        let decay_scale = self.unit(p::DECAY);
        let body_t60 = 9.0 * 2f32.powf(-(f32::from(key) - 48.0) / 18.0) * decay_scale * c.ring;
        let tine_t60 = 0.6 * 2f32.powf(-(f32::from(key) - 60.0) / 24.0) * decay_scale;
        let tone = self.unit(p::TONE);
        let index = (0.3 + 2.4 * tone) * (0.35 + 0.65 * v) * c.depth;
        let tine = self.unit(p::TINE) * (0.1 + 0.9 * v) * (1.0 - 0.5 * k);
        let voice = &mut self.voices[i];
        if !again {
            *voice = Voice::default();
        }
        voice.key = key;
        voice.channel = self.keys.slots[i].channel;
        voice.hz = hz;
        voice.steps = steps;
        voice.damped = false;
        voice.loudness =
            crate::db_to_gain(-30.0 * dynamics * (1.0 - v)) * crate::db_to_gain(3.0 * k);
        voice.tine_level = tine;
        voice.body = 1.0;
        voice.body_index = index;
        voice.body_floor = index * 0.25;
        voice.tine = 1.0;
        voice.tine_index = 1.0 + 2.5 * v;
        voice.falls = [
            decay(body_t60, rate),
            decay(0.9, rate),
            decay(tine_t60, rate),
            decay(0.15, rate),
        ];
    }

    fn damp(&mut self, i: usize) {
        let rate = self.rate;
        let release = 0.08 + 0.9 * self.unit(p::DAMPER);
        let voice = &mut self.voices[i];
        voice.falls[0] = decay(release, rate);
        voice.falls[2] = decay(release.min(0.2), rate);
        voice.damped = true;
    }

    fn apply(&mut self, change: Change) {
        match change {
            Change::Strike {
                voice,
                key,
                velocity,
                again,
            } => self.strike(voice, key, velocity, again),
            Change::Damp(voice) => self.damp(voice),
            Change::Silence => {
                for v in self.voices.iter_mut() {
                    *v = Voice::default();
                }
            }
        }
    }

    fn amp_settings(&self) -> AmpSettings {
        AmpSettings {
            bass: self.value(p::BASS),
            treble: self.value(p::TREBLE),
            drive: self.unit(p::DRIVE),
            tremolo: self.value(p::TREMOLO).round() as usize,
            rate: self.value(p::TREMOLO_RATE),
            depth: self.unit(p::TREMOLO_DEPTH),
            phaser: self.value(p::PHASER),
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
}

impl Effect for FmKeys {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.keys.reset();
        for v in self.voices.iter_mut() {
            *v = Voice::default();
        }
        self.amp.prepare(sample_rate, max_frames);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.amp.set_tempo(bpm);
    }

    fn midi(&mut self, event: &MidiEvent) {
        let mut changes = [Change::Silence; VOICES + 1];
        let mut count = 0;
        self.keys.midi(event, |c| {
            if count < changes.len() {
                changes[count] = c;
                count += 1;
            }
        });
        for &c in &changes[..count] {
            self.apply(c);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        // The levels glide in 2 ms: a key struck again rises without a click.
        let glide = 1.0 - (-1.0 / (0.002 * self.rate)).exp();
        for i in 0..VOICES {
            if !self.keys.slots[i].on {
                continue;
            }
            let hz = self.hz(self.voices[i].key) * self.keys.bend(self.voices[i].channel);
            if hz != self.voices[i].hz {
                self.voices[i].hz = hz;
                self.voices[i].steps = self.steps(hz);
            }
            let voice = &mut self.voices[i];
            for out in left.iter_mut() {
                let [cb, mb, ct, mt] = voice.phases;
                let index = voice.body_floor + voice.body_index;
                let body = ((cb + index / TAU * (mb * TAU).sin()) * TAU).sin();
                let tine = ((ct + voice.tine_index / TAU * (mt * TAU).sin()) * TAU).sin();
                voice.heard[0] += glide * (voice.body * voice.loudness - voice.heard[0]);
                voice.heard[1] +=
                    glide * (voice.tine * voice.tine_level * voice.loudness - voice.heard[1]);
                *out += (body * voice.heard[0] + tine * voice.heard[1]) * OUTPUT;
                for (ph, step) in voice.phases.iter_mut().zip(voice.steps) {
                    *ph = (*ph + step).fract();
                }
                voice.body *= voice.falls[0];
                voice.body_index *= voice.falls[1];
                voice.tine *= voice.falls[2];
                voice.tine_index *= voice.falls[3];
            }
            let floor = if voice.damped { 1e-4 } else { 3e-5 };
            if voice.body < floor && voice.tine < floor {
                self.keys.finish(i);
            }
        }
        right.copy_from_slice(left);
        let settings = self.amp_settings();
        self.amp.process(settings, left, right);
    }

    fn reset(&mut self) {
        self.keys.reset();
        for v in self.voices.iter_mut() {
            *v = Voice::default();
        }
        self.amp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn keys(settings: &[(usize, f32)]) -> FmKeys {
        let mut k = FmKeys::default();
        k.prepare(RATE, 256);
        for (i, v) in [(p::CHORUS, 0.0), (p::REVERB, 0.0), (p::DETUNE, 0.0)] {
            k.set_param(i, v);
        }
        for &(i, v) in settings {
            k.set_param(i, v);
        }
        k
    }

    fn render(k: &mut FmKeys, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while out.len() < n {
            k.process(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn level(x: &[f32], hz: f32) -> f32 {
        let w = TAU * hz / RATE as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (n, s) in x.iter().enumerate() {
            re += s * (w * n as f32).cos();
            im += s * (w * n as f32).sin();
        }
        2.0 * (re * re + im * im).sqrt() / x.len() as f32
    }

    #[test]
    fn the_note_is_in_tune_and_harder_is_louder_and_brighter() {
        let play = |vel| {
            let mut k = keys(&[]);
            k.midi(&MidiEvent::note_on(0, 0, 57, vel));
            render(&mut k, 0.3)
        };
        let (soft, hard) = (play(30), play(120));
        assert!(level(&hard[2400..], 220.0) > 5.0 * level(&hard[2400..], 233.0));
        assert!(rms(&hard) > 2.0 * rms(&soft));
        let ping = |x: &[f32]| level(&x[..2400], 220.0 * 14.0) / level(&x[..2400], 220.0);
        assert!(ping(&hard) > 2.0 * ping(&soft));
    }

    #[test]
    fn released_notes_end_and_a_full_chord_stays_under_full_scale() {
        let mut k = keys(&[(p::CHARACTER, 1.0)]);
        for key in 30..30 + VOICES as u8 {
            k.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let x = render(&mut k, 0.5);
        assert!(x.iter().all(|s| s.is_finite() && s.abs() < 1.0));
        for key in 30..30 + VOICES as u8 {
            k.midi(&MidiEvent::note_off(0, 0, key));
        }
        render(&mut k, 3.0);
        assert!(k.keys.slots.iter().all(|s| !s.on));
    }
}
