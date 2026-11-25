//! Strada-Tonewheel: the organ of turning tone wheels. Ninety-one wheels, one per pitch from
//! the lowest C, turn all the time; each key closes nine contacts, one per drawbar, that take
//! a wheel each at the drawbar's harmonic (the footages), folded back an octave past the last
//! wheel. The sines are not quite pure in the bass, neighbors leak in faintly, and the contacts
//! close one after the other when a key goes down: the key click. The percussion strikes the
//! second or third harmonic once for each legato phrase. Then the scanner's vibrato and
//! chorus, the preamp pushed by the expression pedal (CC 11), the rotary speaker (the mod
//! wheel switches it to fast), delay and reverb.

use std::f32::consts::TAU;

use super::common::{Amp, AmpSettings, Thump};
use super::rotary::{self, Rotary, SPEEDS};
use crate::delay::NOTE_NAMES;
use crate::line::Line;
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "tonewheel",
    name: "Strada-Tonewheel",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Tone wheel organ with its nine drawbars, percussion, key click, scanner \
                  vibrato and rotary speaker.",
};

/// The drawbars' parameter ids, from the lowest (16') to the highest (1').
pub const DRAWBARS: [&str; 9] = [
    "bar_16",
    "bar_5_1_3",
    "bar_8",
    "bar_4",
    "bar_2_2_3",
    "bar_2",
    "bar_1_3_5",
    "bar_1_1_3",
    "bar_1",
];
/// The drawbars' footages, as written on them.
pub const FOOTAGES: [&str; 9] = [
    "16'", "5 1/3'", "8'", "4'", "2 2/3'", "2'", "1 3/5'", "1 1/3'", "1'",
];
/// The semitones each drawbar sounds above the key.
const INTERVALS: [i32; 9] = [-12, 7, 0, 12, 19, 24, 28, 31, 36];

pub const PERCUSSIONS: [&str; 3] = ["Off", "Second", "Third"];
pub const DECAYS: [&str; 2] = ["Fast", "Slow"];
pub const LOUDNESSES: [&str; 2] = ["Normal", "Soft"];
pub const SCANNER: [&str; 7] = ["Off", "V1", "V2", "V3", "C1", "C2", "C3"];

pub mod p {
    pub const VOLUME: usize = 0;
    /// The nine drawbars follow.
    pub const BARS: usize = 1;
    pub const PERCUSSION: usize = 10;
    pub const PERC_DECAY: usize = 11;
    pub const PERC_SOFT: usize = 12;
    pub const VIBRATO: usize = 13;
    pub const CLICK: usize = 14;
    pub const LEAKAGE: usize = 15;
    pub const DRIVE: usize = 16;
    pub const ROTARY: usize = 17;
    pub const WIDTH: usize = 18;
    pub const BASS: usize = 19;
    pub const TREBLE: usize = 20;
    pub const DYNAMICS: usize = 21;
    pub const TUNE: usize = 22;
    pub const DELAY: usize = 23;
    pub const DELAY_NOTE: usize = 24;
    pub const DELAY_FEEDBACK: usize = 25;
    pub const REVERB: usize = 26;
    pub const REVERB_DECAY: usize = 27;
    pub const COUNT: usize = 28;
}

const fn bar(id: &'static str, name: &'static str, default: f32) -> ParamInfo {
    param(id, name, 0.0, 8.0, default, "")
}

const PARAMS: [ParamInfo; p::COUNT] = [
    param("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
    bar(DRAWBARS[0], "Drawbar 16'", 8.0),
    bar(DRAWBARS[1], "Drawbar 5 1/3'", 8.0),
    bar(DRAWBARS[2], "Drawbar 8'", 8.0),
    bar(DRAWBARS[3], "Drawbar 4'", 0.0),
    bar(DRAWBARS[4], "Drawbar 2 2/3'", 0.0),
    bar(DRAWBARS[5], "Drawbar 2'", 0.0),
    bar(DRAWBARS[6], "Drawbar 1 3/5'", 0.0),
    bar(DRAWBARS[7], "Drawbar 1 1/3'", 0.0),
    bar(DRAWBARS[8], "Drawbar 1'", 0.0),
    choice("percussion", "Percussion", &PERCUSSIONS, 2.0),
    choice("perc_decay", "Percussion decay", &DECAYS, 0.0),
    choice("perc_soft", "Percussion volume", &LOUDNESSES, 0.0),
    choice("vibrato", "Vibrato and chorus", &SCANNER, 6.0),
    param("click", "Key click", 0.0, 100.0, 40.0, "%"),
    param("leakage", "Leakage", 0.0, 100.0, 25.0, "%"),
    param("drive", "Drive", 0.0, 100.0, 20.0, "%"),
    choice("rotary", "Rotary speaker", &SPEEDS, 2.0),
    param("rotary_width", "Rotary width", 0.0, 100.0, 70.0, "%"),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("treble", "Treble", -12.0, 12.0, 0.0, "dB"),
    param("dynamics", "Velocity", 0.0, 100.0, 0.0, "%"),
    param("tune", "Tuning", 430.0, 450.0, 440.0, "Hz"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 30.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 12.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 1.6, "s"),
];

const WHEELS: usize = 91;
/// The MIDI note of the first wheel (the lowest C).
const FIRST: i32 = 24;
/// Level of one contact at full drawbar.
const OUTPUT: f32 = 0.07;
/// The scanner's speed, in Hz, and its sweep for V1, V2 and V3, in ms.
const SCAN_HZ: f32 = 6.9;
const SCAN_MS: [f32; 4] = [0.0, 0.25, 0.5, 0.9];

/// The wheel a contact takes: the key's pitch at the drawbar's harmonic, folded back into the
/// wheels' range an octave at a time.
fn wheel(key: u8, bar: usize) -> usize {
    let mut n = i32::from(key) - FIRST + INTERVALS[bar];
    while n < 0 {
        n += 12;
    }
    while n >= WHEELS as i32 {
        n -= 12;
    }
    n as usize
}

/// A drawbar's gain: about 3 dB a step, nothing at 0.
fn bar_gain(value: f32) -> f32 {
    let v = value.round().clamp(0.0, 8.0);
    if v <= 0.0 {
        0.0
    } else {
        crate::db_to_gain(-3.0 * (8.0 - v))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Key {
    down: bool,
    sustained: bool,
    active: bool,
    gain: f32,
    /// Each contact's closing, and the samples before it closes.
    contacts: [f32; 9],
    wait: [u32; 9],
    percussion: f32,
}

#[derive(Debug)]
pub struct Tonewheel {
    rate: f32,
    values: [f32; p::COUNT],
    /// Each wheel's rotation (cosine, sine) and its step per sample.
    wheels: [(f32, f32); WHEELS],
    steps: [(f32, f32); WHEELS],
    tuned: f32,
    /// Each wheel's level, and how much of the wheels an octave and a twelfth above it
    /// cross into its output.
    color: Box<[(f32, f32, f32); WHEELS]>,
    keys: Box<[Key; 128]>,
    /// The wheel of each key's contacts.
    table: Box<[[u8; 9]; 128]>,
    pedal: bool,
    /// Keys down, for the percussion that strikes only after all were up.
    held: usize,
    /// Samples left in which a new key still takes the percussion.
    strike: u32,
    seed: u32,
    leak: f32,
    thump: Thump,
    /// The scanner: its line, phase, and the sweep, wet and dry shares as heard.
    scanner: Line,
    scan: f32,
    sweep: f32,
    wet: f32,
    dry: f32,
    /// The expression pedal and the mod wheel.
    expression: f32,
    heard_expression: f32,
    wheel_fast: bool,
    rotary: Rotary,
    amp: Amp,
}

impl Default for Tonewheel {
    fn default() -> Self {
        let mut organ = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            wheels: [(1.0, 0.0); WHEELS],
            steps: [(1.0, 0.0); WHEELS],
            tuned: 0.0,
            color: Box::new(std::array::from_fn(|w| {
                let w = w as f32;
                (
                    crate::db_to_gain(-0.35 * (w - 55.0).max(0.0)),
                    crate::db_to_gain((-17.0 - 0.55 * (w - 18.0)).min(-12.0)),
                    crate::db_to_gain((-38.0 - 0.37 * (w - 18.0)).min(-32.0)),
                )
            })),
            keys: Box::new([Key::default(); 128]),
            table: Box::new(std::array::from_fn(|k| {
                std::array::from_fn(|b| wheel(k as u8, b) as u8)
            })),
            pedal: false,
            held: 0,
            strike: 0,
            seed: 0x1234_5678,
            leak: 0.0,
            thump: Thump::default(),
            scanner: Line::default(),
            scan: 0.0,
            sweep: 0.0,
            wet: 0.0,
            dry: 1.0,
            expression: 1.0,
            heard_expression: 1.0,
            wheel_fast: false,
            rotary: Rotary::default(),
            amp: Amp::default(),
        };
        organ.prepare(48_000, 512);
        organ
    }
}

impl Tonewheel {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn choice(&self, i: usize) -> usize {
        self.values[i].round().max(0.0) as usize
    }

    fn tune(&mut self) {
        let a4 = self.value(p::TUNE);
        if a4 == self.tuned {
            return;
        }
        self.tuned = a4;
        for (w, step) in self.steps.iter_mut().enumerate() {
            let hz = a4 * 2f32.powf((FIRST + w as i32 - 69) as f32 / 12.0);
            let (s, c) = (TAU * hz / self.rate).sin_cos();
            *step = (c, s);
        }
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }

    fn press(&mut self, key: u8, velocity: u8) {
        let click = self.unit(p::CLICK);
        let dynamics = self.unit(p::DYNAMICS);
        let rate = self.rate;
        if self.held == 0 {
            self.strike = (0.03 * rate) as u32;
        }
        let mut waits = [0u32; 9];
        for w in &mut waits {
            // The contacts close one after the other, more apart for a louder click.
            *w = (self.random() * 0.003 * click * rate) as u32;
        }
        let percussion = self.strike > 0;
        let k = &mut self.keys[usize::from(key)];
        let fresh = !k.active;
        if !k.down {
            self.held += 1;
        }
        k.down = true;
        k.sustained = false;
        k.active = true;
        k.gain = 1.0 - dynamics * (1.0 - f32::from(velocity) / 127.0);
        if fresh {
            k.contacts = [0.0; 9];
            k.wait = waits;
        }
        k.percussion = if percussion { 1.0 } else { 0.0 };
        self.thump.hit(click * 0.012, 0.004, 4000.0, rate);
    }

    fn release(&mut self, key: u8) {
        let k = &mut self.keys[usize::from(key)];
        if !k.down {
            return;
        }
        k.down = false;
        self.held = self.held.saturating_sub(1);
        if self.pedal {
            k.sustained = true;
        }
        let click = self.unit(p::CLICK);
        self.thump.hit(click * 0.004, 0.003, 3000.0, self.rate);
    }

    fn amp_settings(&self) -> AmpSettings {
        AmpSettings {
            bass: self.value(p::BASS),
            treble: self.value(p::TREBLE),
            drive: self.unit(p::DRIVE),
            tremolo: 1,
            rate: 1.0,
            depth: 0.0,
            phaser: 0.0,
            chorus: 0.0,
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

impl Effect for Tonewheel {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.tuned = 0.0;
        self.tune();
        // The wheels start at spread phases, as they would after turning for a while.
        for (w, wheel) in self.wheels.iter_mut().enumerate() {
            let a = TAU * (w as f32 * 0.618_034).fract();
            *wheel = (a.cos(), a.sin());
        }
        for k in self.keys.iter_mut() {
            *k = Key::default();
        }
        self.held = 0;
        self.scanner = Line::new((0.004 * self.rate) as usize + 4);
        self.rotary.prepare(sample_rate);
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
        if let Some((key, velocity)) = event.as_note_on() {
            self.press(key, velocity);
        } else if let Some(key) = event.as_note_off() {
            self.release(key);
        } else if let Some((cc, value)) = event.as_control() {
            match cc {
                1 => self.wheel_fast = value >= 64,
                11 => self.expression = f32::from(value) / 127.0,
                64 => {
                    self.pedal = value >= 64;
                    if !self.pedal {
                        for k in self.keys.iter_mut() {
                            k.sustained = false;
                        }
                    }
                }
                120 | 123 => {
                    for k in self.keys.iter_mut() {
                        k.down = false;
                        k.sustained = false;
                        if cc == 120 {
                            *k = Key::default();
                        }
                    }
                    self.held = 0;
                    self.pedal = false;
                }
                121 => {
                    self.expression = 1.0;
                    self.wheel_fast = false;
                }
                _ => {}
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.tune();
        let rate = self.rate;
        let click = self.unit(p::CLICK);
        let percussion = self.choice(p::PERCUSSION).min(2);
        let mut gains: [f32; 9] = std::array::from_fn(|b| bar_gain(self.value(p::BARS + b)));
        let mut level = OUTPUT;
        let perc_bar = if percussion == 1 { 3 } else { 4 };
        let soft = self.choice(p::PERC_SOFT) == 1;
        let perc_level = if soft { 0.5 } else { 1.0 };
        if percussion > 0 {
            // As on the original, the percussion takes the 1' contacts and, at normal
            // volume, turns the organ down.
            gains[8] = 0.0;
            if !soft {
                level *= 0.7;
            }
        }
        let perc_t60 = if self.choice(p::PERC_DECAY) == 1 {
            2.5
        } else {
            0.9
        };
        let perc_fall = super::modal::decay(perc_t60, rate);
        let attack = 1.0 / ((0.004 * (1.0 - click) + 0.000_05) * rate);
        let fall = 1.0 / ((0.008 * (1.0 - click) + 0.000_3) * rate);
        let leak_level = self.unit(p::LEAKAGE) * 0.0012;
        let glide = 1.0 - (-1.0 / (0.02 * rate)).exp();
        let scanner = self.choice(p::VIBRATO).min(6);
        let (depth, wet) = match scanner {
            0 => (0.0, 0.0),
            1..=3 => (SCAN_MS[scanner], 1.0),
            _ => (SCAN_MS[scanner - 3], 0.5),
        };
        let depth = depth * rate / 1000.0;
        let scan_step = SCAN_HZ / rate;
        let mut list = [0u8; 128];
        let mut count = 0;
        for (i, k) in self.keys.iter().enumerate() {
            if k.active {
                list[count] = i as u8;
                count += 1;
            }
        }
        let any = self.held > 0 || count > 0;
        for out in left.iter_mut() {
            self.strike = self.strike.saturating_sub(1);
            let mut now = [0.0f32; WHEELS];
            for (w, ((c, s), (sc, ss))) in self.wheels.iter_mut().zip(&self.steps).enumerate() {
                let (nc, ns) = (*c * sc - *s * ss, *s * sc + *c * ss);
                (*c, *s) = (nc, ns);
                // The lowest wheels are not quite round: a touch of their third harmonic.
                let impure = 0.05 * (1.0 - w as f32 / 36.0).max(0.0);
                now[w] = ns + impure * (3.0 * ns - 4.0 * ns * ns * ns);
            }
            // The highest wheels come out softer, and each picks up a little of its upper
            // neighbors in the generator: the organ's faint second and third harmonics.
            for w in 0..WHEELS {
                let (level, octave, twelfth) = self.color[w];
                let mut x = now[w];
                if w + 12 < WHEELS {
                    x += octave * now[w + 12];
                }
                if w + 19 < WHEELS {
                    x += twelfth * now[w + 19];
                }
                now[w] = x * level;
            }
            let mut sum = 0.0;
            for &i in &list[..count] {
                let k = &mut self.keys[usize::from(i)];
                let wheels = &self.table[usize::from(i)];
                let on = k.down || k.sustained;
                let mut open = true;
                for b in 0..9 {
                    if k.wait[b] > 0 {
                        k.wait[b] -= 1;
                    } else if on {
                        k.contacts[b] = (k.contacts[b] + attack).min(1.0);
                    } else {
                        k.contacts[b] = (k.contacts[b] - fall).max(0.0);
                    }
                    if k.contacts[b] > 0.0 {
                        open = false;
                        sum += gains[b] * k.contacts[b] * k.gain * now[usize::from(wheels[b])];
                    }
                }
                if percussion > 0 && k.percussion > 1e-4 {
                    let w = usize::from(wheels[perc_bar]);
                    sum += perc_level * k.percussion * k.gain * k.contacts[2] * now[w];
                    k.percussion *= perc_fall;
                }
                if !on && open {
                    k.active = false;
                    k.percussion = 0.0;
                }
            }
            // The other wheels leak in faintly while the organ plays.
            self.leak += glide * (if any { leak_level } else { 0.0 } - self.leak);
            if self.leak > 1e-6 {
                let mut bleed = 0.0;
                for (w, v) in now.iter().enumerate().step_by(7) {
                    bleed += v * (1.0 - w as f32 / WHEELS as f32);
                }
                sum += bleed * self.leak;
            }
            sum = sum * level + self.thump.tick();
            // The scanner: a delay swept back and forth, alone (vibrato) or with the dry sound.
            self.sweep += glide * (depth - self.sweep);
            self.wet += glide * (wet - self.wet);
            self.dry += glide * (1.0 - wet - self.dry);
            self.scanner.push(sum);
            self.scan = (self.scan + scan_step).fract();
            let tri = 1.0 - 2.0 * (2.0 * self.scan - 1.0).abs();
            let delayed = self.scanner.read(1.0 + self.sweep * (0.5 + 0.5 * tri));
            *out = self.dry * sum + self.wet * delayed;
        }
        // The expression pedal: quieter and cleaner as it comes up.
        let start = self.heard_expression;
        let end = start
            + (self.expression - start) * (1.0 - (-(left.len() as f32) / (0.03 * rate)).exp());
        let n = left.len().max(1) as f32;
        for (j, x) in left.iter_mut().enumerate() {
            let e = start + (end - start) * (j + 1) as f32 / n;
            *x *= 0.1 + 0.9 * e * e;
        }
        self.heard_expression = end;
        right.copy_from_slice(left);
        let speed = match self.choice(p::ROTARY).min(3) {
            rotary::OFF => rotary::OFF,
            _ if self.wheel_fast => rotary::FAST,
            s => s,
        };
        let width = self.unit(p::WIDTH);
        let settings = self.amp_settings();
        let rotary = &mut self.rotary;
        self.amp.process_with(settings, left, right, |l, r| {
            rotary.process(l, r, speed, width)
        });
    }

    fn reset(&mut self) {
        for k in self.keys.iter_mut() {
            *k = Key::default();
        }
        self.held = 0;
        self.scanner.clear();
        self.rotary.reset();
        self.amp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn organ(settings: &[(usize, f32)]) -> Tonewheel {
        let mut o = Tonewheel::default();
        o.prepare(RATE, 256);
        // The organ alone: no rotary, scanner, percussion, leakage, click nor reverb.
        for (i, v) in [
            (p::ROTARY, 0.0),
            (p::VIBRATO, 0.0),
            (p::PERCUSSION, 0.0),
            (p::LEAKAGE, 0.0),
            (p::CLICK, 0.0),
            (p::REVERB, 0.0),
            (p::DRIVE, 0.0),
        ] {
            o.set_param(i, v);
        }
        for &(i, v) in settings {
            o.set_param(i, v);
        }
        o
    }

    fn render(o: &mut Tonewheel, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while out.len() < n {
            o.process(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
    }

    /// Level of `hz` in `x`, by correlation with a sine and a cosine under a Hann window, so
    /// that a strong neighbour does not leak into it.
    fn level(x: &[f32], hz: f32) -> f32 {
        let w = TAU * hz / RATE as f32;
        let len = x.len() as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (n, s) in x.iter().enumerate() {
            let hann = 0.5 - 0.5 * (TAU * n as f32 / len).cos();
            re += s * hann * (w * n as f32).cos();
            im += s * hann * (w * n as f32).sin();
        }
        4.0 * (re * re + im * im).sqrt() / len
    }

    fn only(bar: usize) -> Vec<(usize, f32)> {
        (0..9)
            .map(|b| (p::BARS + b, if b == bar { 8.0 } else { 0.0 }))
            .collect()
    }

    #[test]
    fn each_drawbar_sounds_its_harmonic() {
        for (b, interval) in INTERVALS.into_iter().enumerate() {
            let ratio = 2f32.powf(interval as f32 / 12.0);
            let mut o = organ(&only(b));
            o.midi(&MidiEvent::note_on(0, 0, 57, 100));
            let x = render(&mut o, 0.5);
            let seg = &x[4800..];
            let want = level(seg, 220.0 * ratio);
            let other = level(seg, 220.0 * ratio * 1.06);
            assert!(
                want > 0.02 && want > other * 10.0,
                "drawbar {b}: {want} {other}"
            );
        }
    }

    #[test]
    fn notes_past_the_last_wheel_fold_back_an_octave() {
        assert_eq!(wheel(96, 8), wheel(84, 8));
        assert_eq!(wheel(24, 0), 0);
        assert!(wheel(108, 8) < WHEELS);
    }

    #[test]
    fn the_percussion_strikes_once_per_phrase_and_dies_away() {
        let mut o = organ(&[(p::PERCUSSION, 2.0)]);
        o.midi(&MidiEvent::note_on(0, 0, 57, 100));
        let first = render(&mut o, 0.1);
        let third = |x: &[f32]| level(x, 660.0);
        let early = third(&first[1200..]);
        let late = render(&mut o, 1.5);
        assert!(early > third(&late[late.len() - 4800..]) * 5.0);
        // A key added while the first is held gets no percussion.
        o.midi(&MidiEvent::note_on(0, 0, 64, 100));
        let added = render(&mut o, 0.1);
        assert!(level(&added[1200..], 329.63 * 3.0) < early * 0.2);
    }

    #[test]
    fn released_keys_fall_silent_and_the_organ_stays_under_full_scale() {
        let mut o = organ(&[]);
        for key in 36..97 {
            o.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let all = render(&mut o, 0.5);
        assert!(all.iter().all(|s| s.is_finite() && s.abs() < 1.0));
        for key in 36..97 {
            o.midi(&MidiEvent::note_off(0, 0, key));
        }
        render(&mut o, 0.2);
        assert!(o.keys.iter().all(|k| !k.active));
    }
}
