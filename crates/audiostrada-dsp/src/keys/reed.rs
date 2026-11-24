//! Strada-Reed: the electric piano of struck reeds. A felt hammer strikes a steel reed clamped
//! at one end, tuned by the solder at its tip; the reed rings in its first modes and swings
//! between the teeth of a charged comb. The comb and the reed form a capacitor whose charge
//! follows 1 / (gap - x): the nearer the reed comes, the more the voltage rises, so the note
//! is asymmetric and grows its nasal bite with the blow (Bark brings the comb closer). Struck
//! hard, the reed's swing itself saturates. Then the preamp, the small built-in speakers or
//! the line out, the amplitude tremolo, phaser, chorus, delay and reverb.

use super::common::{Amp, AmpSettings, TREMOLOS, Thump};
use super::modal::{Hammer, Mode};
use super::voice::{CURVES, Change, Keyboard, VOICES, frequency, velocity};
use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::delay::NOTE_NAMES;
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "reed",
    name: "Strada-Reed",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Electric piano of struck steel reeds, from sweet to biting, with its built-in \
                  speakers, tremolo, delay and reverb.",
};

pub const SPEAKERS: [&str; 2] = ["Line", "Cabinet"];

pub mod p {
    pub const VOLUME: usize = 0;
    pub const VOICING: usize = 1;
    pub const HAMMER: usize = 2;
    pub const DECAY: usize = 3;
    pub const DAMPER: usize = 4;
    pub const NOISE: usize = 5;
    pub const DYNAMICS: usize = 6;
    pub const CURVE: usize = 7;
    pub const BASS: usize = 8;
    pub const TREBLE: usize = 9;
    pub const DRIVE: usize = 10;
    pub const SPEAKER: usize = 11;
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
    param("voicing", "Reed bark", 0.0, 100.0, 60.0, "%"),
    param("hammer", "Hammer", 0.0, 100.0, 55.0, "%"),
    param("decay", "Decay", 25.0, 200.0, 100.0, "%"),
    param("damper", "Release", 0.0, 100.0, 40.0, "%"),
    param("noise", "Mechanics", 0.0, 100.0, 25.0, "%"),
    param("dynamics", "Velocity", 0.0, 100.0, 55.0, "%"),
    choice("curve", "Velocity curve", &CURVES, 1.0),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("treble", "Treble", -12.0, 12.0, 1.0, "dB"),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    choice("speaker", "Speaker", &SPEAKERS, 0.0),
    choice("tremolo", "Tremolo", &TREMOLOS, 1.0),
    param("tremolo_rate", "Tremolo rate", 0.5, 10.0, 5.5, "Hz"),
    param("tremolo_depth", "Tremolo depth", 0.0, 100.0, 30.0, "%"),
    param("phaser", "Phaser", 0.0, 100.0, 0.0, "%"),
    param("chorus", "Chorus", 0.0, 100.0, 0.0, "%"),
    param("tune", "Tuning", 430.0, 450.0, 440.0, "Hz"),
    param("stretch", "Octave stretch", 0.0, 100.0, 0.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 30.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 12.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 1.4, "s"),
];

/// The reed's fundamental, its second and third modes (a clamped bar weighted at its tip), by
/// frequency ratio and weight.
const MODES: usize = 3;
const RATIOS: [f32; MODES] = [1.0, 6.8, 18.3];
const WEIGHTS: [f32; MODES] = [1.0, 0.06, 0.015];

/// The lowest key of the instrument and its span, for what varies along the keyboard.
const LOW: f32 = 33.0;
const SPAN: f32 = 63.0;
/// Swing of the reed at full strength, in fractions of its gap to the comb.
const SWING: f32 = 0.9;
/// Most of the swing: the reed's travel saturates past it.
const LIMIT: f32 = 0.9;
const OUTPUT: f32 = 0.5;
/// The narrowest the gap gets, as a fraction of its rest.
const FLOOR: f32 = 0.38;

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
    damped: bool,
}

/// Where the key sits on the keyboard, from 0 (lowest) to 1.
fn place(key: u8) -> f32 {
    ((f32::from(key) - LOW) / SPAN).clamp(0.0, 1.0)
}

/// The comb's voltage for a reed at `x` (in fractions of the gap), the comb `near` (0 to 1):
/// the field between them grows as the square of the closing gap, so each swing toward the
/// comb ends in a sharp peak, the reed's bite. About `x` for small swings.
#[inline]
fn pickup(x: f32, near: f32) -> f32 {
    let x = x / (1.0 + (x / LIMIT).powi(6)).powf(1.0 / 6.0);
    // The gap closes softly to a floor: the peaks stay round enough not to alias into a
    // harsh buzz on the hardest blows.
    let over = 1.0 - near * x - FLOOR;
    let gap = FLOOR + 0.5 * (over + (over * over + 0.02).sqrt());
    (1.0 / (gap * gap) - 1.0) / (2.0 * near)
}

#[derive(Debug)]
pub struct Reed {
    rate: f32,
    values: [f32; p::COUNT],
    keys: Keyboard,
    voices: Box<[Voice; VOICES]>,
    /// How near the comb is as heard, gliding to the Bark: a knob turned never clicks.
    heard: f32,
    /// The preamp's voicing: no rumble under the lowest notes, a forward middle.
    preamp: [Coefficients; 3],
    filters: [Biquad; 3],
    amp: Amp,
}

impl Default for Reed {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            keys: Keyboard::default(),
            voices: Box::new([Voice::default(); VOICES]),
            heard: f32::NAN,
            preamp: [Coefficients::IDENTITY; 3],
            filters: [Biquad::default(); 3],
            amp: Amp::default(),
        }
    }
}

impl Reed {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn hz(&self, key: u8) -> f32 {
        frequency(f32::from(key), self.value(p::TUNE), self.unit(p::STRETCH))
    }

    /// How near the comb sits to the reeds, from the Bark.
    fn near(&self) -> f32 {
        0.55 + 0.45 * self.unit(p::VOICING)
    }

    fn strike(&mut self, i: usize, key: u8, struck: u8, again: bool) {
        let rate = self.rate;
        let v = velocity(struck, self.value(p::CURVE));
        let dynamics = self.unit(p::DYNAMICS);
        let k = place(key);
        let force =
            SWING * crate::db_to_gain(-30.0 * dynamics * (1.0 - v)) * crate::db_to_gain(2.0 * k);
        let hammer = self.unit(p::HAMMER);
        let hz = self.hz(key) * self.keys.bend(self.keys.slots[i].channel);
        let decay = self.unit(p::DECAY);
        let below = f32::from(key) - LOW;
        let t60 = 16.0 * 2f32.powf(-below / 20.0) * decay;
        let t60s = [t60, 0.5 * 2f32.powf(-below / 24.0) * decay, 0.06];
        // A harder blow and harder felt touch the reed for less time: a brighter strike.
        let contact = ((0.003 - 0.0025 * v) * (1.5 - hammer))
            .max(0.000_12)
            .min(0.4 / hz);
        let noise = self.unit(p::NOISE);
        let voice = &mut self.voices[i];
        if !again {
            *voice = Voice::default();
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
            let felt = if m >= 1 { 0.3 + 1.4 * hammer } else { 1.0 };
            voice.push[m] = if f < rate * 0.45 {
                WEIGHTS[m] * felt / norm
            } else {
                0.0
            };
        }
        voice
            .thump
            .hit(noise * v * 0.025, 0.01, 1200.0 + 1800.0 * v, rate);
    }

    fn damp(&mut self, i: usize) {
        let rate = self.rate;
        let voice = &mut self.voices[i];
        let k = place(voice.key);
        let release = (0.14 - 0.09 * k) * (0.3 + 1.7 * self.values[p::DAMPER] / 100.0);
        for (m, mode) in voice.modes.iter_mut().enumerate() {
            mode.set_t60(if m >= 1 { release.min(0.05) } else { release }, rate);
        }
        voice.damped = true;
        let noise = self.values[p::NOISE] / 100.0;
        voice.thump.hit(noise * 0.005, 0.015, 700.0, rate);
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
            speaker: self.value(p::SPEAKER) >= 0.5,
        }
    }
}

impl Effect for Reed {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.keys.reset();
        for v in self.voices.iter_mut() {
            *v = Voice::default();
        }
        self.heard = f32::NAN;
        let rate = f64::from(sample_rate);
        self.preamp = [
            Coefficients::design(FilterKind::HighPass, 45.0, 0.7, 0.0, rate),
            Coefficients::design(FilterKind::LowShelf, 220.0, 0.7, -2.0, rate),
            Coefficients::design(FilterKind::Peak, 800.0, 0.5, 3.5, rate),
        ];
        self.filters = [Biquad::default(); 3];
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
        let start = if self.heard.is_nan() {
            self.near()
        } else {
            self.heard
        };
        let n = left.len().max(1) as f32;
        let end = start + (self.near() - start) * (1.0 - (-n / (0.02 * rate)).exp());
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
            for (j, out) in left.iter_mut().enumerate() {
                let push = voice.hammer.tick();
                let mut x = 0.0;
                for (m, mode) in voice.modes.iter_mut().enumerate() {
                    x += mode.tick(push * voice.push[m]);
                }
                let near = start + ramp * (j + 1) as f32;
                // Louder for the same swing with the comb nearer: brought back to an even level.
                let gain = OUTPUT / (1.0 + 2.0 * near);
                *out += pickup(x, near) * gain + voice.thump.tick() * OUTPUT;
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
        // The preamp also blocks the steady offset the pickup's asymmetry leaves.
        for x in left.iter_mut() {
            let mut y = f64::from(*x);
            for (f, c) in self.filters.iter_mut().zip(&self.preamp) {
                y = f.process(c, y);
            }
            *x = y as f32;
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
        self.filters = [Biquad::default(); 3];
        self.amp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn reed(settings: &[(usize, f32)]) -> Reed {
        let mut r = Reed::default();
        r.prepare(RATE, 256);
        // The voices alone, without the tremolo's swing nor the reverb's tail.
        r.set_param(p::TREMOLO_DEPTH, 0.0);
        r.set_param(p::REVERB, 0.0);
        r.set_param(p::DRIVE, 0.0);
        for &(i, v) in settings {
            r.set_param(i, v);
        }
        r
    }

    fn render(r: &mut Reed, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut l, mut rr) = (vec![0.0; 256], vec![0.0; 256]);
        while out.len() < n {
            r.process(&mut l, &mut rr);
            out.extend_from_slice(&l);
        }
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

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

    fn brightness(x: &[f32]) -> f32 {
        let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
        rms(&d) / rms(x).max(1e-9)
    }

    #[test]
    fn every_key_sounds_in_tune() {
        for stretch in [0.0, 100.0] {
            for key in (33..=96).step_by(7) {
                let mut r = reed(&[(p::STRETCH, stretch), (p::VOICING, 0.0), (p::HAMMER, 0.0)]);
                r.midi(&MidiEvent::note_on(0, 0, key, 40));
                let l = render(&mut r, 0.6);
                let want = frequency(f32::from(key), 440.0, stretch / 100.0);
                let got = pitch(&l[RATE as usize / 5..]);
                let cents = 1200.0 * (got / want).log2();
                assert!(
                    cents.abs() < 1.0,
                    "key {key}: {got} for {want} ({cents} cents)"
                );
            }
        }
    }

    #[test]
    fn harder_is_louder_and_bites_more() {
        let play = |velocity| {
            let mut r = reed(&[]);
            r.midi(&MidiEvent::note_on(0, 0, 57, velocity));
            let l = render(&mut r, 0.2);
            (rms(&l[..4800]), brightness(&l[..4800]))
        };
        let (soft, hard) = (play(30), play(120));
        assert!(hard.0 > soft.0 * 2.0, "{soft:?} {hard:?}");
        assert!(hard.1 > soft.1 * 1.2, "{soft:?} {hard:?}");
    }

    #[test]
    fn released_notes_end_and_the_speakers_take_the_lows_away() {
        let mut r = reed(&[]);
        r.midi(&MidiEvent::note_on(0, 0, 45, 100));
        render(&mut r, 0.2);
        r.midi(&MidiEvent::note_off(0, 0, 45));
        render(&mut r, 1.5);
        assert!(r.keys.slots.iter().all(|s| !s.on));
        let level = |speaker| {
            let mut r = reed(&[(p::SPEAKER, speaker)]);
            r.midi(&MidiEvent::note_on(0, 0, 33, 100));
            rms(&render(&mut r, 0.3))
        };
        assert!(level(1.0) < level(0.0) * 0.7);
    }

    #[test]
    fn a_full_chord_stays_finite_and_under_full_scale() {
        let mut r = reed(&[(p::VOICING, 100.0), (p::DRIVE, 100.0)]);
        r.midi(&MidiEvent::control(0, 0, 64, 127));
        for key in 33..33 + VOICES as u8 {
            r.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let l = render(&mut r, 0.5);
        assert!(l.iter().all(|s| s.is_finite() && s.abs() < 1.0));
    }
}
