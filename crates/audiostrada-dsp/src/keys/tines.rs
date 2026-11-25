//! Strada-Tines: the electric piano of struck tines. A neoprene hammer strikes a thin tine
//! clamped at one end, coupled to a tone bar; the tine rings in its first modes (the bell is
//! the second one, about six times higher, short-lived) and swings in front of an
//! electromagnetic pickup. The pickup's field falls off on each side of its pole, so the
//! voltage is a nonlinear function of the tine's position: set close to the pole (Voicing) and
//! struck hard, the tine crosses the field and the note barks. Then the suitcase amplifier:
//! bass and treble, drive, the stereo tremolo, then phaser, chorus, delay and reverb.

use std::f32::consts::TAU;

use super::common::{Amp, AmpSettings, TREMOLOS, Thump};
use super::modal::{Hammer, Mode};
use super::voice::{CURVES, Change, Keyboard, VOICES, frequency, velocity};
use crate::delay::NOTE_NAMES;
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "tines",
    name: "Strada-Tines",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Electric piano of struck tines and tone bars, from bell-like to barking, with \
                  its suitcase amplifier, stereo tremolo, delay and reverb.",
};

pub mod p {
    pub const VOLUME: usize = 0;
    pub const VOICING: usize = 1;
    pub const BELL: usize = 2;
    pub const DECAY: usize = 3;
    pub const DAMPER: usize = 4;
    pub const NOISE: usize = 5;
    pub const DYNAMICS: usize = 6;
    pub const CURVE: usize = 7;
    pub const BASS: usize = 8;
    pub const TREBLE: usize = 9;
    pub const DRIVE: usize = 10;
    pub const TREMOLO: usize = 11;
    pub const TREMOLO_RATE: usize = 12;
    pub const TREMOLO_DEPTH: usize = 13;
    pub const PHASER: usize = 14;
    pub const CHORUS: usize = 15;
    pub const TUNE: usize = 16;
    pub const STRETCH: usize = 17;
    pub const DELAY: usize = 18;
    pub const DELAY_NOTE: usize = 19;
    pub const DELAY_FEEDBACK: usize = 20;
    pub const REVERB: usize = 21;
    pub const REVERB_DECAY: usize = 22;
    pub const COUNT: usize = 23;
}

const PARAMS: [ParamInfo; p::COUNT] = [
    param("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
    param("voicing", "Tine voicing", 0.0, 100.0, 55.0, "%"),
    param("bell", "Bell", 0.0, 100.0, 60.0, "%"),
    param("decay", "Decay", 25.0, 200.0, 100.0, "%"),
    param("damper", "Release", 0.0, 100.0, 40.0, "%"),
    param("noise", "Mechanics", 0.0, 100.0, 25.0, "%"),
    param("dynamics", "Velocity", 0.0, 100.0, 50.0, "%"),
    choice("curve", "Velocity curve", &CURVES, 1.0),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("treble", "Treble", -12.0, 12.0, 2.0, "dB"),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    choice("tremolo", "Tremolo", &TREMOLOS, 0.0),
    param("tremolo_rate", "Tremolo rate", 0.5, 10.0, 4.5, "Hz"),
    param("tremolo_depth", "Tremolo depth", 0.0, 100.0, 35.0, "%"),
    param("phaser", "Phaser", 0.0, 100.0, 0.0, "%"),
    param("chorus", "Chorus", 0.0, 100.0, 0.0, "%"),
    param("tune", "Tuning", 430.0, 450.0, 440.0, "Hz"),
    param("stretch", "Octave stretch", 0.0, 100.0, 0.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 30.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 15.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 1.6, "s"),
];

/// The tine and its tone bar: the fundamental, the tone bar a hair above it, the bell (the
/// clamped bar's second mode) and its third mode, by frequency ratio and weight.
const MODES: usize = 4;
const RATIOS: [f32; MODES] = [1.0, 1.000_23, 6.267, 17.55];
const WEIGHTS: [f32; MODES] = [1.0, 0.22, 0.09, 0.025];

/// The lowest and highest keys of the instrument, for what varies along the keyboard.
const LOW: f32 = 28.0;
const SPAN: f32 = 72.0;
/// Swing of the tine at full strength, in widths of the pickup's field.
const SWING: f32 = 1.9;
/// How much wider the low tines swing than the high ones, in dB across the keyboard: the
/// bass barks and the treble rings almost pure.
const SPREAD: f32 = 42.0;
const OUTPUT: f32 = 0.2;
/// The pickup's slope at the usual voicing: a tine set closer to the pole gives less
/// fundamental and more growl, as on the real instrument.
const SLOPE: f32 = 0.6;

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    modes: [Mode; MODES],
    /// How much of the hammer's force goes into each mode.
    push: [f32; MODES],
    hammer: Hammer,
    thump: Thump,
    key: u8,
    channel: u8,
    /// Frequency the modes are tuned to, bend included.
    hz: f32,
    /// The pickup's flux at the last sample.
    flux: f32,
    damped: bool,
}

/// Swing of a key against the middle of the keyboard (key 56), in dB.
fn spread(key: u8) -> f32 {
    SPREAD * (56.0 - f32::from(key)) / SPAN
}

/// Where the key sits on the keyboard, from 0 (lowest) to 1.
fn place(key: u8) -> f32 {
    ((f32::from(key) - LOW) / SPAN).clamp(0.0, 1.0)
}

/// The pickup's flux for a tine at `x` (in field widths) beside a pole `offset` away.
#[inline]
fn flux(x: f32, offset: f32) -> f32 {
    let u = x + offset;
    1.0 / (1.0 + u * u)
}

#[derive(Debug)]
pub struct Tines {
    rate: f32,
    values: [f32; p::COUNT],
    keys: Keyboard,
    voices: Box<[Voice; VOICES]>,
    /// The pickups' offset as heard, gliding to the Voicing: a knob turned never clicks.
    heard: f32,
    amp: Amp,
}

impl Default for Tines {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            keys: Keyboard::default(),
            voices: Box::new([Voice::default(); VOICES]),
            heard: f32::NAN,
            amp: Amp::default(),
        }
    }
}

impl Tines {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn hz(&self, key: u8) -> f32 {
        frequency(f32::from(key), self.value(p::TUNE), self.unit(p::STRETCH))
    }

    /// The pole's distance from the tine's rest, from the Voicing.
    fn offset(&self) -> f32 {
        0.62 - 0.47 * self.unit(p::VOICING)
    }

    fn current_offset(&self) -> f32 {
        if self.heard.is_nan() {
            self.offset()
        } else {
            self.heard
        }
    }

    fn strike(&mut self, i: usize, key: u8, struck: u8, again: bool) {
        let rate = self.rate;
        let v = velocity(struck, self.value(p::CURVE));
        let dynamics = self.unit(p::DYNAMICS);
        // About 30 dB from a touch to a blow at the default Velocity, after the pickup.
        let force = SWING * crate::db_to_gain(-85.0 * dynamics * (1.0 - v).powf(1.3) + spread(key));
        let bell = self.unit(p::BELL);
        let hz = self.hz(key) * self.keys.bend(self.keys.slots[i].channel);
        let decay = self.unit(p::DECAY);
        let u = f32::from(key) - LOW;
        let t60 = 2f32.powf(5.0 - 0.028 * u - 6.7e-6 * u * u * u) * decay;
        let t60s = [
            t60,
            t60 * 1.5,
            0.6 * 2f32.powf(-(f32::from(key) - LOW) / 24.0) * decay,
            0.08,
        ];
        // A harder blow and a harder tip touch the tine for less time: a brighter strike.
        let contact = ((0.004 - 0.0034 * v) * (1.5 - bell))
            .max(0.000_15)
            .min(0.4 / hz);
        let noise = self.unit(p::NOISE);
        let rest = flux(0.0, self.current_offset());
        let voice = &mut self.voices[i];
        if !again {
            *voice = Voice::default();
            voice.flux = rest;
        }
        voice.key = key;
        voice.channel = self.keys.slots[i].channel;
        voice.damped = false;
        voice.hz = hz;
        for (m, mode) in voice.modes.iter_mut().enumerate() {
            mode.tune(hz * RATIOS[m], rate);
            mode.set_t60(t60s[m], rate);
        }
        voice.hammer.strike(force, contact, rate);
        let norm = voice.hammer.response(hz, rate);
        for m in 0..MODES {
            let f = hz * RATIOS[m];
            let tip = if m >= 2 { 0.2 + 1.6 * bell } else { 1.0 };
            voice.push[m] = if f < rate * 0.45 {
                WEIGHTS[m] * tip / norm
            } else {
                0.0
            };
        }
        voice
            .thump
            .hit(noise * v * 0.02, 0.012, 900.0 + 1500.0 * v, rate);
    }

    fn damp(&mut self, i: usize) {
        let rate = self.rate;
        let voice = &mut self.voices[i];
        let k = place(voice.key);
        let release = (0.25 - 0.17 * k) * (0.3 + 1.7 * self.values[p::DAMPER] / 100.0);
        for (m, mode) in voice.modes.iter_mut().enumerate() {
            mode.set_t60(if m >= 2 { release.min(0.08) } else { release }, rate);
        }
        voice.damped = true;
        let noise = self.values[p::NOISE] / 100.0;
        voice.thump.hit(noise * 0.004, 0.02, 500.0, rate);
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

impl Effect for Tines {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.keys.reset();
        for v in self.voices.iter_mut() {
            *v = Voice::default();
        }
        self.heard = f32::NAN;
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
        let start = self.current_offset();
        let n = left.len().max(1) as f32;
        let end = start + (self.offset() - start) * (1.0 - (-n / (0.02 * rate)).exp());
        let ramp = (end - start) / n;
        self.heard = end;
        for i in 0..VOICES {
            if !self.keys.slots[i].on {
                continue;
            }
            let hz = self.hz(self.voices[i].key) * self.keys.bend(self.voices[i].channel);
            let voice = &mut self.voices[i];
            if hz != voice.hz {
                voice.hz = hz;
                for (m, mode) in voice.modes.iter_mut().enumerate() {
                    mode.tune(hz * RATIOS[m], rate);
                }
            }
            // The pickup reads the tine's speed through the field: scaled back by the
            // frequency and the key's spread, every key comes out at about the same level.
            let gain =
                -OUTPUT * rate / (TAU * hz * SLOPE) * crate::db_to_gain(-0.95 * spread(voice.key));
            for (j, out) in left.iter_mut().enumerate() {
                let push = voice.hammer.tick();
                let mut x = 0.0;
                for (m, mode) in voice.modes.iter_mut().enumerate() {
                    x += mode.tick(push * voice.push[m]);
                }
                let now = flux(x, start + ramp * (j + 1) as f32);
                *out += (now - voice.flux) * gain + voice.thump.tick() * OUTPUT;
                voice.flux = now;
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

    fn tines(settings: &[(usize, f32)]) -> Tines {
        let mut t = Tines::default();
        t.prepare(RATE, 256);
        // The voices alone, without the tremolo's swing nor the reverb's tail.
        t.set_param(p::TREMOLO_DEPTH, 0.0);
        t.set_param(p::REVERB, 0.0);
        for &(i, v) in settings {
            t.set_param(i, v);
        }
        t
    }

    fn render(t: &mut Tines, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * RATE as f32) as usize;
        let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while left.len() < n {
            t.process(&mut l, &mut r);
            left.extend_from_slice(&l);
            right.extend_from_slice(&r);
        }
        (left, right)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Frequency from the zero crossings, between the first and last rising ones.
    fn pitch(x: &[f32]) -> f32 {
        let rising: Vec<f32> = x
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
            .map(|(i, w)| i as f32 + w[0] / (w[0] - w[1]))
            .collect();
        let (first, last) = (rising[0], rising[rising.len() - 1]);
        (rising.len() - 1) as f32 * RATE as f32 / (last - first)
    }

    /// Energy above 2 kHz against the whole, by a first difference.
    fn brightness(x: &[f32]) -> f32 {
        let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
        rms(&d) / rms(x).max(1e-9)
    }

    #[test]
    fn every_key_sounds_in_tune_stretch_included() {
        for stretch in [0.0, 100.0] {
            for key in (28..=100).step_by(7) {
                let mut t = tines(&[(p::STRETCH, stretch), (p::VOICING, 0.0), (p::BELL, 0.0)]);
                t.midi(&MidiEvent::note_on(0, 0, key, 40));
                let (l, _) = render(&mut t, 0.8);
                let want = frequency(f32::from(key), 440.0, stretch / 100.0);
                let got = pitch(&l[RATE as usize / 4..]);
                let cents = 1200.0 * (got / want).log2();
                assert!(
                    cents.abs() < 1.0,
                    "key {key}: {got} for {want} ({cents} cents)"
                );
            }
        }
    }

    #[test]
    fn harder_is_louder_and_brighter() {
        let play = |velocity| {
            let mut t = tines(&[]);
            t.midi(&MidiEvent::note_on(0, 0, 60, velocity));
            let (l, _) = render(&mut t, 0.3);
            (rms(&l[..4800]), brightness(&l[..4800]))
        };
        let (soft, hard) = (play(30), play(120));
        assert!(hard.0 > soft.0 * 2.0, "{soft:?} {hard:?}");
        assert!(hard.1 > soft.1 * 1.2, "{soft:?} {hard:?}");
    }

    #[test]
    fn the_pedal_holds_the_notes_and_they_end_after_their_release() {
        let mut t = tines(&[]);
        t.midi(&MidiEvent::control(0, 0, 64, 127));
        t.midi(&MidiEvent::note_on(0, 0, 60, 100));
        render(&mut t, 0.1);
        t.midi(&MidiEvent::note_off(0, 0, 60));
        let (held, _) = render(&mut t, 0.5);
        assert!(rms(&held[held.len() - 4800..]) > 1e-3);
        t.midi(&MidiEvent::control(0, 0, 64, 0));
        render(&mut t, 2.0);
        assert!(t.keys.slots.iter().all(|s| !s.on), "the voice ended");
        let (after, _) = render(&mut t, 0.1);
        assert!(after.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn a_chord_of_every_voice_stays_finite_and_under_full_scale() {
        let mut t = tines(&[(p::DRIVE, 100.0), (p::VOICING, 100.0), (p::CHORUS, 60.0)]);
        t.midi(&MidiEvent::control(0, 0, 64, 127));
        for key in 30..30 + VOICES as u8 + 8 {
            t.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let (l, r) = render(&mut t, 1.0);
        assert!(l.iter().chain(&r).all(|s| s.is_finite()));
        let mut t = tines(&[]);
        for key in [48, 55, 60, 64, 67, 71] {
            t.midi(&MidiEvent::note_on(0, 0, key, 112));
        }
        let (l, _) = render(&mut t, 1.0);
        let peak = l.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 1.0 && peak > 0.1, "{peak}");
        for s in t.voices.iter().flat_map(|v| v.modes.iter()) {
            assert!(s.energy() == 0.0 || s.energy() > 1e-30, "no denormals");
        }
    }

    #[test]
    fn the_voicing_brings_the_bark() {
        // The second harmonic against the fundamental, at A2 tuned straight.
        let level = |x: &[f32], hz: f32| {
            let w = TAU * hz / RATE as f32;
            let (re, im) = x
                .iter()
                .enumerate()
                .fold((0.0f32, 0.0f32), |(re, im), (n, s)| {
                    (re + s * (w * n as f32).cos(), im + s * (w * n as f32).sin())
                });
            (re * re + im * im).sqrt()
        };
        let play = |voicing| {
            let mut t = tines(&[(p::VOICING, voicing), (p::BELL, 0.0), (p::STRETCH, 0.0)]);
            t.midi(&MidiEvent::note_on(0, 0, 45, 127));
            let (l, _) = render(&mut t, 0.5);
            let x = &l[4800..];
            level(x, 220.0) / level(x, 110.0)
        };
        let (voiced, flat) = (play(100.0), play(0.0));
        assert!(voiced > flat * 1.5, "{voiced} {flat}");
    }
}
