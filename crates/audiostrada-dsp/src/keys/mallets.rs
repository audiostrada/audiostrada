//! Strada-Mallets: struck bars and plucked tongues, in five instruments. Each rings in its
//! own modes: the marimba's wooden bar cut so its overtones fall at 4 and 10 times the note,
//! over a tube that feeds the fundamental; the vibraphone's aluminium bar, long-ringing, its
//! motor's tremolo and its damper pedal; the kalimba's tongues; the celesta's steel plates over
//! their resonators; the music box's comb. The mallet's hardness sets how bright the strike
//! is. Then the amplifier, the tremolo, the effects.

use super::common::{Amp, AmpSettings, TREMOLOS, Thump};
use super::modal::{Hammer, Mode};
use super::voice::{CURVES, Change, Keyboard, VOICES, frequency, velocity};
use crate::delay::NOTE_NAMES;
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "mallets",
    name: "Strada-Mallets",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Marimba, vibraphone, kalimba, celesta and music box, struck or plucked, \
                  with tremolo, delay and reverb.",
};

pub const INSTRUMENTS: [&str; 5] = ["Marimba", "Vibraphone", "Kalimba", "Celesta", "Music box"];

pub mod p {
    pub const VOLUME: usize = 0;
    pub const INSTRUMENT: usize = 1;
    pub const HARDNESS: usize = 2;
    pub const DECAY: usize = 3;
    pub const DAMPER: usize = 4;
    pub const RESONANCE: usize = 5;
    pub const NOISE: usize = 6;
    pub const DYNAMICS: usize = 7;
    pub const CURVE: usize = 8;
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
    choice("instrument", "Instrument", &INSTRUMENTS, 0.0),
    param("hardness", "Mallet hardness", 0.0, 100.0, 50.0, "%"),
    param("decay", "Decay", 25.0, 200.0, 100.0, "%"),
    param("damper", "Release", 0.0, 100.0, 40.0, "%"),
    param("resonance", "Resonance", 0.0, 100.0, 60.0, "%"),
    param("noise", "Mechanics", 0.0, 100.0, 25.0, "%"),
    param("dynamics", "Velocity", 0.0, 100.0, 60.0, "%"),
    choice("curve", "Velocity curve", &CURVES, 1.0),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("treble", "Treble", -12.0, 12.0, 0.0, "dB"),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    choice("tremolo", "Tremolo", &TREMOLOS, 1.0),
    param("tremolo_rate", "Tremolo rate", 0.5, 10.0, 5.0, "Hz"),
    param("tremolo_depth", "Tremolo depth", 0.0, 100.0, 0.0, "%"),
    param("phaser", "Phaser", 0.0, 100.0, 0.0, "%"),
    param("chorus", "Chorus", 0.0, 100.0, 0.0, "%"),
    param("tune", "Tuning", 430.0, 450.0, 440.0, "Hz"),
    param("stretch", "Octave stretch", 0.0, 100.0, 0.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 30.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 20.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 1.8, "s"),
];

const MODES: usize = 4;

/// An instrument: its modes' ratios and weights, how long each rings against the
/// fundamental, the fundamental's ring at middle C (s), how fast that shortens up the
/// keyboard (semitones per halving), and its strike's length scale (a pluck is short).
#[derive(Clone, Copy, Debug)]
struct Build {
    ratios: [f32; MODES],
    weights: [f32; MODES],
    rings: [f32; MODES],
    ring: f32,
    halving: f32,
    strike: f32,
}

const BUILDS: [Build; 5] = [
    // Marimba.
    Build {
        ratios: [1.0, 3.99, 9.9, 17.0],
        weights: [1.0, 0.22, 0.07, 0.02],
        rings: [1.0, 0.25, 0.1, 0.05],
        ring: 1.0,
        halving: 16.0,
        strike: 1.0,
    },
    // Vibraphone.
    Build {
        ratios: [1.0, 4.0, 10.0, 16.9],
        weights: [1.0, 0.3, 0.1, 0.03],
        rings: [1.0, 0.3, 0.12, 0.05],
        ring: 6.0,
        halving: 20.0,
        strike: 0.8,
    },
    // Kalimba.
    Build {
        ratios: [1.0, 5.9, 14.6, 3.1],
        weights: [1.0, 0.18, 0.05, 0.02],
        rings: [1.0, 0.2, 0.08, 0.3],
        ring: 2.4,
        halving: 18.0,
        strike: 0.35,
    },
    // Celesta.
    Build {
        ratios: [1.0, 3.0, 6.1, 9.8],
        weights: [1.0, 0.18, 0.07, 0.03],
        rings: [1.0, 0.35, 0.15, 0.08],
        ring: 1.8,
        halving: 20.0,
        strike: 0.6,
    },
    // Music box.
    Build {
        ratios: [1.0, 6.27, 17.55, 3.4],
        weights: [1.0, 0.25, 0.08, 0.03],
        rings: [1.0, 0.18, 0.06, 0.25],
        ring: 2.2,
        halving: 22.0,
        strike: 0.25,
    },
];

const OUTPUT: f32 = 0.3;

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    modes: [Mode; MODES],
    push: [f32; MODES],
    hammer: Hammer,
    thump: Thump,
    key: u8,
    channel: u8,
    hz: f32,
    /// The instrument the note was struck on, for its ratios when it is retuned.
    build: usize,
    damped: bool,
}

#[derive(Debug)]
pub struct Mallets {
    rate: f32,
    values: [f32; p::COUNT],
    keys: Keyboard,
    voices: Box<[Voice; VOICES]>,
    amp: Amp,
}

impl Default for Mallets {
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

impl Mallets {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn build(&self) -> usize {
        (self.value(p::INSTRUMENT).round().max(0.0) as usize).min(BUILDS.len() - 1)
    }

    fn hz(&self, key: u8) -> f32 {
        frequency(f32::from(key), self.value(p::TUNE), self.unit(p::STRETCH))
    }

    fn strike(&mut self, i: usize, key: u8, struck: u8, again: bool) {
        let rate = self.rate;
        let v = velocity(struck, self.value(p::CURVE));
        let dynamics = self.unit(p::DYNAMICS);
        let which = self.build();
        let b = BUILDS[which];
        let hz = self.hz(key) * self.keys.bend(self.keys.slots[i].channel);
        let hardness = self.unit(p::HARDNESS);
        let resonance = self.unit(p::RESONANCE);
        let ring = b.ring * 2f32.powf(-(f32::from(key) - 60.0) / b.halving) * self.unit(p::DECAY);
        // A harder mallet and a harder blow touch the bar for less time: brighter.
        let contact = ((0.006 - 0.005 * v) * (1.6 - 1.4 * hardness) * b.strike)
            .max(0.000_1)
            .min(0.45 / hz);
        let force = crate::db_to_gain(-30.0 * dynamics * (1.0 - v));
        let noise = self.unit(p::NOISE);
        let voice = &mut self.voices[i];
        if !again {
            *voice = Voice::default();
        }
        voice.key = key;
        voice.channel = self.keys.slots[i].channel;
        voice.hz = hz;
        voice.build = which;
        voice.damped = false;
        for (m, mode) in voice.modes.iter_mut().enumerate() {
            mode.tune(hz * b.ratios[m], rate);
            let fed = if m == 0 { 0.7 + 0.9 * resonance } else { 1.0 };
            mode.set_t60((ring * b.rings[m] * fed).max(0.02), rate);
        }
        voice.hammer.strike(force, contact, rate);
        let norm = voice.hammer.response(hz, rate);
        for m in 0..MODES {
            let f = hz * b.ratios[m];
            let fed = if m == 0 { 0.7 + 0.6 * resonance } else { 1.0 };
            voice.push[m] = if f < rate * 0.45 {
                b.weights[m] * fed / norm
            } else {
                0.0
            };
        }
        voice
            .thump
            .hit(noise * v * 0.02, 0.006, 1500.0 + 2500.0 * hardness, rate);
    }

    fn damp(&mut self, i: usize) {
        let rate = self.rate;
        let release = 0.05 + 0.6 * self.unit(p::DAMPER);
        let voice = &mut self.voices[i];
        for mode in &mut voice.modes {
            mode.set_t60(release, rate);
        }
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

impl Effect for Mallets {
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
        let rate = self.rate;
        for i in 0..VOICES {
            if !self.keys.slots[i].on {
                continue;
            }
            let hz = self.hz(self.voices[i].key) * self.keys.bend(self.voices[i].channel);
            let voice = &mut self.voices[i];
            if hz != voice.hz {
                voice.hz = hz;
                let ratios = BUILDS[voice.build].ratios;
                for (m, mode) in voice.modes.iter_mut().enumerate() {
                    mode.tune(hz * ratios[m], rate);
                }
            }
            for out in left.iter_mut() {
                let push = voice.hammer.tick();
                let mut x = 0.0;
                for (m, mode) in voice.modes.iter_mut().enumerate() {
                    x += mode.tick(push * voice.push[m]);
                }
                *out += (x + voice.thump.tick()) * OUTPUT;
            }
            if !voice.hammer.active() {
                voice.hammer.clear();
            }
            let mut energy = 0.0;
            for mode in &mut voice.modes {
                let e = mode.energy();
                if e < 1e-20 {
                    mode.clear();
                }
                energy += e;
            }
            let floor = if voice.damped { 1e-9 } else { 1e-12 };
            if energy < floor && !voice.hammer.active() && !voice.thump.active() {
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

    fn render(m: &mut Mallets, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while out.len() < n {
            m.process(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_instrument_sounds_and_the_vibraphone_rings_longest() {
        let tail = |instrument: f32| {
            let mut m = Mallets::default();
            m.prepare(RATE, 256);
            m.set_param(p::REVERB, 0.0);
            m.set_param(p::INSTRUMENT, instrument);
            m.midi(&MidiEvent::control(0, 0, 64, 127));
            m.midi(&MidiEvent::note_on(0, 0, 60, 100));
            let x = render(&mut m, 2.0);
            assert!(x.iter().all(|s| s.is_finite() && s.abs() < 1.0));
            assert!(rms(&x[..4800]) > 0.01, "instrument {instrument}");
            rms(&x[x.len() - 4800..]) / rms(&x[..4800])
        };
        let marimba = tail(0.0);
        let vibraphone = tail(1.0);
        for k in 2..5 {
            tail(k as f32);
        }
        assert!(vibraphone > marimba * 3.0, "{vibraphone} {marimba}");
    }

    #[test]
    fn a_harder_mallet_is_brighter() {
        // The bar's second mode against its fundamental, in the strike.
        let second = |instrument: usize, hardness: f32| {
            let mut m = Mallets::default();
            m.prepare(RATE, 256);
            m.set_param(p::REVERB, 0.0);
            m.set_param(p::INSTRUMENT, instrument as f32);
            m.set_param(p::HARDNESS, hardness);
            m.midi(&MidiEvent::note_on(0, 0, 60, 100));
            let x = render(&mut m, 0.03);
            let level = |hz: f32| {
                let w = std::f32::consts::TAU * hz / RATE as f32;
                let (re, im) = x
                    .iter()
                    .enumerate()
                    .fold((0.0f32, 0.0f32), |(a, b), (n, s)| {
                        (a + s * (w * n as f32).cos(), b + s * (w * n as f32).sin())
                    });
                (re * re + im * im).sqrt()
            };
            let hz = 261.63;
            level(hz * BUILDS[instrument].ratios[1]) / level(hz)
        };
        for instrument in 0..BUILDS.len() {
            let (hard, soft) = (second(instrument, 100.0), second(instrument, 0.0));
            assert!(hard > soft * 3.0, "{instrument}: {hard} {soft}");
        }
    }
}
