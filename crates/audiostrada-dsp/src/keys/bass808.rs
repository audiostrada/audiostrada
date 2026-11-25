//! Strada-808: the sub bass of trap, drill and hip-hop. A sine whose pitch starts above the
//! note and drops onto it (the punch), a click at the strike, a long decay or one that follows
//! the key, rounded into harmonics by the body so it carries on small speakers, slides between
//! overlapping notes, then pushed into distortion while its lowest octave stays clean.

use std::f32::consts::TAU;

use super::common::{Amp, AmpSettings};
use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "bass808",
    name: "Strada-808",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "808 sub bass with punch, click, glide and distortion that keeps the low end \
                  clean.",
};

pub const LENGTHS: [&str; 2] = ["Follows key", "Full"];
pub const SHAPES: [&str; 4] = ["Soft", "Hard", "Fuzz", "Tube"];
pub const MODES: [&str; 2] = ["Mono", "Poly"];

pub mod p {
    pub const VOLUME: usize = 0;
    pub const TUNE: usize = 1;
    pub const PUNCH: usize = 2;
    pub const PUNCH_TIME: usize = 3;
    pub const LENGTH: usize = 4;
    pub const GATE: usize = 5;
    pub const RELEASE: usize = 6;
    pub const CLICK: usize = 7;
    pub const BODY: usize = 8;
    pub const DRIVE: usize = 9;
    pub const SHAPE: usize = 10;
    pub const CLEAN: usize = 11;
    pub const CUTOFF: usize = 12;
    pub const GLIDE: usize = 13;
    pub const MODE: usize = 14;
    pub const DYNAMICS: usize = 15;
    pub const COUNT: usize = 16;
}

const PARAMS: [ParamInfo; p::COUNT] = [
    param("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
    param("tune", "Tune", -12.0, 12.0, 0.0, "st"),
    param("punch", "Punch", 0.0, 24.0, 7.0, "st"),
    param("punch_time", "Punch time", 5.0, 200.0, 40.0, "ms"),
    param("length", "Length", 0.1, 5.0, 3.3, "s"),
    choice("gate", "Note length", &LENGTHS, 0.0),
    param("release", "Release", 5.0, 500.0, 60.0, "ms"),
    param("click", "Click", 0.0, 100.0, 30.0, "%"),
    param("body", "Body", 0.0, 100.0, 25.0, "%"),
    param("drive", "Drive", 0.0, 100.0, 30.0, "%"),
    choice("shape", "Distortion", &SHAPES, 0.0),
    param("clean", "Clean sub", 0.0, 100.0, 60.0, "%"),
    param("cutoff", "Filter", 100.0, 12_000.0, 6000.0, "Hz"),
    param("glide", "Glide", 0.0, 500.0, 60.0, "ms"),
    choice("mode", "Mode", &MODES, 0.0),
    param("dynamics", "Velocity", 0.0, 100.0, 40.0, "%"),
];

const VOICES: usize = 8;
/// Below this level (-20 dB) a held note no longer carries a slide.
const SLIDE_LEVEL: f32 = 0.1;
const OUTPUT: f32 = 0.55;
/// Where the clean sub takes over from the distorted sound.
const CLEAN_HZ: f32 = 90.0;

#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    key: u8,
    on: bool,
    down: bool,
    /// Where the pitch is, and where it slides to, in semitones from A4.
    note: f32,
    target: f32,
    phase: f32,
    /// The punch's offset in semitones, the level, its release, the click.
    punch: f32,
    level: f32,
    attack: f32,
    releasing: bool,
    gain: f32,
    click: f32,
    age: u64,
}

#[derive(Debug)]
pub struct Bass808 {
    rate: f32,
    values: [f32; p::COUNT],
    voices: [Voice; VOICES],
    /// Keys held in mono, oldest first, to slide back to.
    held: [u8; 16],
    count: usize,
    clock: u64,
    seed: u32,
    /// The clean and distorted sounds' lows, to swap them; the filter after.
    lows: [f32; 4],
    filter: Coefficients,
    filtered: [Biquad; 2],
    cutoff: f32,
    amp: Amp,
}

impl Default for Bass808 {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: [Voice::default(); VOICES],
            held: [0; 16],
            count: 0,
            clock: 0,
            seed: 0x51ed_270b,
            lows: [0.0; 4],
            filter: Coefficients::IDENTITY,
            filtered: [Biquad::default(); 2],
            cutoff: 0.0,
            amp: Amp::default(),
        }
    }
}

/// The distortion's curves, each about as loud as it came in at small levels.
#[inline]
fn shape(x: f32, kind: usize) -> f32 {
    match kind {
        1 => x.clamp(-1.0, 1.0),
        2 => x.signum() * (1.0 - (-x.abs() * 1.5).exp()) / 0.78,
        3 => ((x + 0.3).tanh() - 0.3f32.tanh()) * 1.1,
        _ => x.tanh(),
    }
}

impl Bass808 {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn mono(&self) -> bool {
        self.value(p::MODE) < 0.5
    }

    fn press(&mut self, key: u8, velocity: u8) {
        self.clock += 1;
        let dynamics = self.unit(p::DYNAMICS);
        let gain = 1.0 - dynamics * (1.0 - f32::from(velocity) / 127.0);
        let target = f32::from(key) - 69.0;
        let punch = self.value(p::PUNCH);
        if self.mono() {
            if self.count < self.held.len() {
                self.held[self.count] = key;
                self.count += 1;
            }
            let v = &mut self.voices[0];
            // A note over a held one still sounding slides to it without a new strike: the
            // 808 glide. Over one nearly faded it strikes again, else it would go unheard.
            let slide = v.on && v.down && v.level > SLIDE_LEVEL;
            v.key = key;
            v.target = target;
            v.down = true;
            v.releasing = false;
            v.age = self.clock;
            if !slide {
                v.on = true;
                v.note = target;
                v.punch = punch;
                v.level = 1.0;
                v.attack = 0.0;
                v.gain = gain;
                v.click = 1.0;
            }
            return;
        }
        let index = self
            .voices
            .iter()
            .position(|v| !v.on)
            .unwrap_or_else(|| (0..VOICES).min_by_key(|&i| self.voices[i].age).unwrap_or(0));
        self.voices[index] = Voice {
            key,
            on: true,
            down: true,
            note: target,
            target,
            phase: 0.0,
            punch,
            level: 1.0,
            attack: 0.0,
            releasing: false,
            gain,
            click: 1.0,
            age: self.clock,
        };
    }

    fn release(&mut self, key: u8) {
        let gate = self.value(p::GATE) < 0.5;
        if self.mono() {
            let mut kept = 0;
            for i in 0..self.count {
                if self.held[i] != key {
                    self.held[kept] = self.held[i];
                    kept += 1;
                }
            }
            self.count = kept;
            let v = &mut self.voices[0];
            if v.key != key {
                return;
            }
            // Back to the last key still held, sliding; else the note ends.
            if let Some(&last) = self.held[..self.count].last() {
                v.key = last;
                v.target = f32::from(last) - 69.0;
                return;
            }
            v.down = false;
            v.releasing = gate;
            return;
        }
        for v in &mut self.voices {
            if v.on && v.down && v.key == key {
                v.down = false;
                v.releasing = gate;
            }
        }
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
            chorus: 0.0,
            delay: 0.0,
            reverb: 0.0,
            delay_note: 6.0,
            feedback: 30.0,
            decay: 1.0,
            volume_db: self.value(p::VOLUME),
            speaker: false,
        }
    }
}

impl Effect for Bass808 {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.voices = [Voice::default(); VOICES];
        self.count = 0;
        self.lows = [0.0; 4];
        self.filtered = [Biquad::default(); 2];
        self.cutoff = 0.0;
        self.amp.prepare(sample_rate, max_frames);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((key, velocity)) = event.as_note_on() {
            self.press(key, velocity);
        } else if let Some(key) = event.as_note_off() {
            self.release(key);
        } else if event
            .as_control()
            .is_some_and(|(cc, _)| cc == 120 || cc == 123)
        {
            for v in &mut self.voices {
                if event.data[1] == 120 {
                    *v = Voice::default();
                } else {
                    v.down = false;
                    v.releasing = true;
                }
            }
            self.count = 0;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let tune = self.value(p::TUNE).round();
        let punch_fall = (-1.0 / (self.value(p::PUNCH_TIME) * 0.001 * rate)).exp();
        // Length is the decay at C3: lower notes ring longer, higher ones shorter.
        let length = self.value(p::LENGTH);
        let fulls = self
            .voices
            .map(|v| super::modal::decay(length * 2f32.powf(-(v.target + 21.0) / 10.0), rate));
        let release = super::modal::decay(self.value(p::RELEASE) * 0.001, rate);
        let attack = 1.0 / (0.0008 * rate);
        let glide_ms = self.value(p::GLIDE);
        let glide = if glide_ms <= 0.0 {
            1.0
        } else {
            1.0 - (-1.0 / (glide_ms * 0.001 * rate)).exp()
        };
        let body = self.unit(p::BODY);
        // The body rounds the low notes, with an even harmonic, and leaves the high ones
        // nearly pure.
        let bodies = self.voices.map(|v| {
            let low = v.target + 41.0;
            let warm = 0.05 + 4.0 * body * 2f32.powf(-low / 28.0);
            (warm, 2.8 * body * 2f32.powf(-low / 7.5), 1.0 / warm.tanh())
        });
        let click = self.unit(p::CLICK) * 0.5;
        let click_fall = (-1.0 / (0.002 * rate)).exp();
        let a4 = 440.0 / rate;
        for out in left.iter_mut() {
            let mut sum = 0.0;
            for ((v, &full), &(warm, even, norm)) in self.voices.iter_mut().zip(&fulls).zip(&bodies)
            {
                if !v.on {
                    continue;
                }
                v.note += glide * (v.target - v.note);
                let pitch = v.note + tune + v.punch;
                v.punch *= punch_fall;
                v.phase = (v.phase + a4 * 2f32.powf(pitch / 12.0)).fract();
                let sine = (v.phase * TAU).sin();
                let tone = ((sine * warm).tanh() + even * (sine * sine - 0.5)) * norm;
                v.attack = (v.attack + attack).min(1.0);
                v.level *= if v.releasing { release } else { full };
                let mut x = tone * v.level * v.attack * v.gain;
                if v.click > 1e-4 {
                    self.seed ^= self.seed << 13;
                    self.seed ^= self.seed >> 17;
                    self.seed ^= self.seed << 5;
                    let noise = self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
                    x += (noise * 0.4 + (v.phase * TAU * 8.0).sin()) * click * v.click * v.gain;
                    v.click *= click_fall;
                }
                sum += x;
                if v.level < 1e-4 {
                    v.on = false;
                }
            }
            *out = sum * OUTPUT;
        }
        // Distortion, the lowest octave kept clean, then the filter.
        let drive = self.unit(p::DRIVE);
        let kind = (self.value(p::SHAPE).round().max(0.0) as usize).min(SHAPES.len() - 1);
        let gain = crate::db_to_gain(drive * 24.0);
        let makeup = 1.0 / gain.powf(0.7);
        let clean = self.unit(p::CLEAN);
        let low = crate::lowpass_coefficient(CLEAN_HZ, rate);
        let cutoff = self.value(p::CUTOFF);
        if cutoff != self.cutoff {
            self.cutoff = cutoff;
            self.filter =
                Coefficients::design(FilterKind::LowPass, cutoff.into(), 0.707, 0.0, rate.into());
        }
        for x in left.iter_mut() {
            let dry = *x;
            let wet = if drive > 1e-3 {
                shape(dry * gain, kind) * makeup
            } else {
                dry
            };
            // Two one-pole stages on each: the lows of the dry sound and of the distorted.
            self.lows[0] += low * (dry - self.lows[0]);
            self.lows[1] += low * (self.lows[0] - self.lows[1]);
            self.lows[2] += low * (wet - self.lows[2]);
            self.lows[3] += low * (self.lows[2] - self.lows[3]);
            let y = wet + clean * (self.lows[1] - self.lows[3]);
            let mut z = f64::from(y);
            for f in &mut self.filtered {
                z = f.process(&self.filter, z);
            }
            *x = z as f32;
        }
        right.copy_from_slice(left);
        let settings = self.amp_settings();
        self.amp.process(settings, left, right);
    }

    fn reset(&mut self) {
        self.voices = [Voice::default(); VOICES];
        self.count = 0;
        self.lows = [0.0; 4];
        self.filtered = [Biquad::default(); 2];
        self.amp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn bass(settings: &[(usize, f32)]) -> Bass808 {
        let mut b = Bass808::default();
        b.prepare(RATE, 256);
        for &(i, v) in settings {
            b.set_param(i, v);
        }
        b
    }

    fn render(b: &mut Bass808, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while out.len() < n {
            b.process(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
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

    #[test]
    fn the_punch_drops_onto_the_note() {
        let mut b = bass(&[(p::DRIVE, 0.0), (p::CLICK, 0.0), (p::BODY, 0.0)]);
        b.midi(&MidiEvent::note_on(0, 0, 36, 100));
        let x = render(&mut b, 0.6);
        let early = pitch(&x[..1200]);
        let settled = pitch(&x[14_400..]);
        assert!((settled - 65.41).abs() < 0.5, "{settled}");
        assert!(early > settled * 1.2, "{early}");
    }

    #[test]
    fn an_overlapping_note_slides_and_the_gate_ends_the_note() {
        let mut b = bass(&[
            (p::DRIVE, 0.0),
            (p::PUNCH, 0.0),
            (p::CLICK, 0.0),
            (p::BODY, 0.0),
        ]);
        b.midi(&MidiEvent::note_on(0, 0, 36, 100));
        render(&mut b, 0.2);
        b.midi(&MidiEvent::note_on(0, 0, 43, 100));
        let slid = render(&mut b, 0.4);
        assert!((pitch(&slid[9600..]) - 98.0).abs() < 1.0);
        b.midi(&MidiEvent::note_off(0, 0, 43));
        b.midi(&MidiEvent::note_off(0, 0, 36));
        render(&mut b, 0.5);
        assert!(b.voices.iter().all(|v| !v.on));
    }

    #[test]
    fn a_note_over_a_held_one_that_faded_strikes_again() {
        let mut b = bass(&[(p::LENGTH, 0.3), (p::GATE, 1.0)]);
        b.midi(&MidiEvent::note_on(0, 0, 36, 100));
        render(&mut b, 0.5);
        b.midi(&MidiEvent::note_on(0, 0, 43, 100));
        let x = render(&mut b, 0.1);
        let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1, "{peak}");
    }

    #[test]
    fn distortion_keeps_the_sub_and_stays_under_full_scale() {
        let mut b = bass(&[(p::DRIVE, 100.0), (p::SHAPE, 2.0), (p::MODE, 1.0)]);
        for key in [24, 31, 36] {
            b.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let x = render(&mut b, 1.0);
        assert!(x.iter().all(|s| s.is_finite() && s.abs() < 1.0));
    }
}
