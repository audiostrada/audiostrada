//! Strada-EP80: the electric grand of the seventies and eighties. Real piano strings, short and
//! stiff, struck by felt hammers and read by piezo pickups under the bridge, without a
//! soundboard: a bright, clangy, percussive piano that cuts through a band. Each string is a
//! waveguide: a delay line closed by its losses (the highs die first), a chain of allpasses
//! for the stiffness that sharpens the upper partials, and an allpass tuning it exactly. Two
//! strings a little apart sound each note above the bass, beating as a real unison does. Then
//! its preamp (bass, middle, treble and the brilliance switch), drive, tremolo, phaser, chorus,
//! delay and reverb.

use std::f32::consts::TAU;

use super::common::{Amp, AmpSettings, TREMOLOS, Thump};
use super::voice::{CURVES, Change, Keyboard, VOICES, frequency, velocity};
use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::delay::NOTE_NAMES;
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "ep80",
    name: "Strada-EP80",
    params: &PARAMS,
    meters: &[],
    instrument: true,
    files: &[],
    category: "Instrument",
    description: "Electric grand piano of short strings and piezo pickups, bright and clangy, \
                  with its preamp, brilliance switch, tremolo, chorus, delay and reverb.",
};

pub const BRILLIANCES: [&str; 3] = ["Mellow", "Normal", "Bright"];

pub mod p {
    pub const VOLUME: usize = 0;
    pub const HAMMER: usize = 1;
    pub const DECAY: usize = 2;
    pub const DAMPER: usize = 3;
    pub const UNISON: usize = 4;
    pub const NOISE: usize = 5;
    pub const DYNAMICS: usize = 6;
    pub const CURVE: usize = 7;
    pub const BASS: usize = 8;
    pub const MIDDLE: usize = 9;
    pub const TREBLE: usize = 10;
    pub const BRILLIANCE: usize = 11;
    pub const DRIVE: usize = 12;
    pub const TREMOLO: usize = 13;
    pub const TREMOLO_RATE: usize = 14;
    pub const TREMOLO_DEPTH: usize = 15;
    pub const PHASER: usize = 16;
    pub const CHORUS: usize = 17;
    pub const TUNE: usize = 18;
    pub const STRETCH: usize = 19;
    pub const DELAY: usize = 20;
    pub const DELAY_NOTE: usize = 21;
    pub const DELAY_FEEDBACK: usize = 22;
    pub const REVERB: usize = 23;
    pub const REVERB_DECAY: usize = 24;
    pub const COUNT: usize = 25;
}

const PARAMS: [ParamInfo; p::COUNT] = [
    param("volume", "Volume", -24.0, 12.0, 0.0, "dB"),
    param("hammer", "Hammer", 0.0, 100.0, 55.0, "%"),
    param("decay", "Decay", 25.0, 200.0, 100.0, "%"),
    param("damper", "Release", 0.0, 100.0, 40.0, "%"),
    param("unison", "String detune", 0.0, 100.0, 40.0, "%"),
    param("noise", "Mechanics", 0.0, 100.0, 25.0, "%"),
    param("dynamics", "Velocity", 0.0, 100.0, 55.0, "%"),
    choice("curve", "Velocity curve", &CURVES, 1.0),
    param("bass", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("middle", "Middle", -12.0, 12.0, 0.0, "dB"),
    param("treble", "Treble", -12.0, 12.0, 0.0, "dB"),
    choice("brilliance", "Brilliance", &BRILLIANCES, 1.0),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    choice("tremolo", "Tremolo", &TREMOLOS, 0.0),
    param("tremolo_rate", "Tremolo rate", 0.5, 10.0, 5.0, "Hz"),
    param("tremolo_depth", "Tremolo depth", 0.0, 100.0, 0.0, "%"),
    param("phaser", "Phaser", 0.0, 100.0, 0.0, "%"),
    param("chorus", "Chorus", 0.0, 100.0, 20.0, "%"),
    param("tune", "Tuning", 430.0, 450.0, 440.0, "Hz"),
    param("stretch", "Octave stretch", 0.0, 100.0, 40.0, "%"),
    param("delay", "Delay", 0.0, 100.0, 0.0, "%"),
    choice("delay_note", "Delay time", &NOTE_NAMES, 6.0),
    param("delay_feedback", "Delay feedback", 0.0, 90.0, 30.0, "%"),
    param("reverb", "Reverb", 0.0, 100.0, 15.0, "%"),
    param("reverb_decay", "Reverb decay", 0.3, 6.0, 1.6, "s"),
];

/// The keyboard's lowest and highest keys, for what varies along it.
const LOW: f32 = 28.0;
const SPAN: f32 = 72.0;
/// Below this key a note has one string, a wound one; above, two.
const UNISON_FROM: u8 = 34;
/// From this key up the strings have no damper, as on an acoustic piano.
const UNDAMPED_FROM: u8 = 89;
/// Allpasses for the stiffness of each string.
const STIFF: usize = 4;
/// The lowest pitch a string must hold, with room for the bend.
const LOWEST_HZ: f32 = 24.0;
/// Where the hammer strikes, as a fraction of the string: the partials it lands on are
/// missing, as on a piano.
const STRIKE: f32 = 1.0 / 8.0;
const OUTPUT: f32 = 35.0;

/// The hammer's flight before it reaches the string, in seconds: up to 1.5 ms, spread over
/// the keys so that octaves and fifths land apart.
fn flight(key: u8) -> f32 {
    0.0015 * (f32::from(key) * 0.618_034).fract()
}

/// Where the key sits on the keyboard, from 0 (lowest) to 1.
fn place(key: u8) -> f32 {
    ((f32::from(key) - LOW) / SPAN).clamp(0.0, 1.0)
}

/// Phase delay in samples of the allpass `(a + z^-1) / (1 + a z^-1)` at `w` radians.
fn allpass_delay(a: f32, w: f32) -> f32 {
    let (s, c) = w.sin_cos();
    let phase = (-s).atan2(a + c) - (-a * s).atan2(1.0 + a * c);
    -phase / w
}

/// The allpass coefficient that delays `w` by `d` samples (0.3 to 2), by bisection: the
/// delay falls as the coefficient rises.
fn allpass_for(d: f32, w: f32) -> f32 {
    let (mut lo, mut hi) = (-0.95f32, 0.95f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if allpass_delay(mid, w) > d {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// How inharmonic `key`'s strings are (B, partial n at n f0 sqrt(1 + B n²)): the short bass
/// strings the most, then less up to the middle, more again in the stiff treble.
fn inharmonicity(key: u8) -> f32 {
    let k = f32::from(key) - LOW;
    0.0012 * (-k / 14.0).exp() + 0.000_04 * 2f32.powf(k / 12.0)
}

/// The frequency of partial `n` of a loop tuned to `hz` with stiffness `stiff`.
fn partial(hz: f32, stiff: f32, n: f32, rate: f32) -> f32 {
    let w0 = TAU * hz / rate;
    let period = rate / hz;
    let spent = 1.0 + STIFF as f32 * allpass_delay(stiff, w0);
    let len = (period - spent - 0.5).floor().max(1.0);
    let frac = allpass_for(period - spent - len, w0);
    let mut f = n * hz;
    for _ in 0..20 {
        let w = TAU * f / rate;
        let delay = len + 1.0 + STIFF as f32 * allpass_delay(stiff, w) + allpass_delay(frac, w);
        f = n * rate / delay;
    }
    f
}

/// The stiffness that sharpens `key`'s seventh partial (or the highest under a third of the
/// rate) as its strings' inharmonicity does, within what its loop can spare.
fn stiffness_for(key: u8, rate: f32) -> f32 {
    let hz = frequency(f32::from(key), 440.0, 0.0);
    let n = (0.33 * rate / hz).floor().clamp(1.0, 7.0);
    if n < 2.0 {
        return 0.0;
    }
    let want = (1.0 + inharmonicity(key) * n * n).sqrt();
    let w0 = TAU * hz / rate;
    let budget = 0.7 * rate / hz;
    let (mut lo, mut hi) = (-0.97f32, 0.0f32);
    while lo < -0.01 && STIFF as f32 * allpass_delay(lo, w0) > budget {
        lo *= 0.9;
    }
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if partial(hz, mid, n, rate) / (n * hz) > want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// A first-order allpass's memory.
#[derive(Clone, Copy, Debug, Default)]
struct Allpass {
    x1: f32,
    y1: f32,
}

impl Allpass {
    #[inline]
    fn tick(&mut self, a: f32, x: f32) -> f32 {
        let y = a * x + self.x1 - a * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// One string as a waveguide: the delay line and what closes its loop.
#[derive(Clone, Debug, Default)]
struct Line {
    buf: Vec<f32>,
    write: usize,
    /// Whole samples of delay, and the allpass taking the fraction.
    len: usize,
    frac: f32,
    tuner: Allpass,
    /// The stiffness: its coefficient and its allpasses.
    stiff: f32,
    stiffs: [Allpass; STIFF],
    /// The losses each round: their gain, and how much more the highs lose (a three-tap
    /// filter of exactly one sample's delay).
    gain: f32,
    shade: f32,
    taps: [f32; 2],
}

impl Line {
    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
        self.tuner = Allpass::default();
        self.stiffs = [Allpass::default(); STIFF];
        self.taps = [0.0; 2];
    }

    /// Tunes the loop to `hz` exactly, its stiffness `stiff` and its highs shaded by `shade`.
    fn tune(&mut self, hz: f32, stiff: f32, shade: f32, rate: f32) {
        let w = TAU * hz / rate;
        let period = rate / hz;
        let cap = self.buf.len().max(4);
        // Stiffness takes its delay out of the loop: less of it where the loop is short.
        let mut stiff = stiff;
        while stiff < -0.01 && period - 1.0 - STIFF as f32 * allpass_delay(stiff, w) < 3.0 {
            stiff *= 0.5;
        }
        let spent = 1.0 + STIFF as f32 * allpass_delay(stiff, w);
        let whole = (period - spent - 0.5).floor().max(1.0);
        self.len = (whole as usize).min(cap - 2);
        self.frac = allpass_for(period - spent - self.len as f32, w);
        self.stiff = stiff;
        self.shade = shade;
    }

    /// Sets the losses so that the fundamental falls 60 dB in `t60` seconds.
    fn set_t60(&mut self, t60: f32, hz: f32, rate: f32) {
        let w = TAU * hz / rate;
        let per_round = 10f32.powf(-3.0 / (t60.max(0.01) * hz));
        let filter = 1.0 - 2.0 * self.shade * (1.0 - w.cos());
        self.gain = (per_round / filter.max(0.05)).min(0.99995);
    }

    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let cap = self.buf.len();
        let read = self.buf[(self.write + cap - self.len) % cap];
        let h = self.shade;
        let lossy = self.gain * (h * read + (1.0 - 2.0 * h) * self.taps[0] + h * self.taps[1]);
        self.taps[1] = self.taps[0];
        self.taps[0] = read;
        let mut x = lossy;
        for s in &mut self.stiffs {
            x = s.tick(self.stiff, x);
        }
        x = self.tuner.tick(self.frac, x);
        let new = x + input;
        self.buf[self.write] = new;
        self.write = (self.write + 1) % cap;
        read
    }
}

/// One note: its strings, the hammer's blow still running, the mechanics' noise.
#[derive(Clone, Debug, Default)]
struct Voice {
    strings: [Line; 2],
    count: usize,
    key: u8,
    channel: u8,
    hz: f32,
    /// The hammer's raised-cosine blow: its length and height, and where it is (samples).
    blow: (f32, f32),
    at: f32,
    /// The blow again, delayed to the strike point's reflection.
    echo: f32,
    /// The pickup's darkening: a two-pole low-pass following the note and the blow.
    shut: f32,
    low: [f32; 2],
    thump: Thump,
    /// The last output, for the pickup's tilt, and a slow measure of the level.
    last: f32,
    level: f32,
    damped: bool,
}

impl Voice {
    /// The blow at sample `t`.
    #[inline]
    fn force(&self, t: f32) -> f32 {
        let (length, height) = self.blow;
        if t < 0.0 || t >= length {
            0.0
        } else {
            height * (0.5 - 0.5 * (TAU * t / length).cos())
        }
    }
}

#[derive(Debug)]
pub struct Ep80 {
    rate: f32,
    values: [f32; p::COUNT],
    keys: Keyboard,
    voices: Vec<Voice>,
    /// Each key's stiffness, worked out for the rate.
    stiff: [f32; 128],
    /// The preamp's middle and brilliance, their gains gliding to the settings.
    middle: (f32, Coefficients, Biquad),
    brilliance: (f32, Coefficients, Biquad),
    amp: Amp,
}

impl Default for Ep80 {
    fn default() -> Self {
        let mut e = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            keys: Keyboard::default(),
            voices: Vec::new(),
            stiff: [0.0; 128],
            middle: (f32::NAN, Coefficients::IDENTITY, Biquad::default()),
            brilliance: (f32::NAN, Coefficients::IDENTITY, Biquad::default()),
            amp: Amp::default(),
        };
        e.prepare(48_000, 1024);
        e
    }
}

impl Ep80 {
    fn value(&self, i: usize) -> f32 {
        self.values[i]
    }

    fn unit(&self, i: usize) -> f32 {
        self.values[i] / 100.0
    }

    fn hz(&self, key: u8) -> f32 {
        frequency(f32::from(key), self.value(p::TUNE), self.unit(p::STRETCH))
    }

    /// The second string's detune in cents, from the String detune.
    fn spread(&self) -> f32 {
        3.0 * self.unit(p::UNISON)
    }

    /// How much more the highs lose each round: less on the short treble strings, whose
    /// rounds are many.
    fn shade(&self, hz: f32) -> f32 {
        let hammer = self.unit(p::HAMMER);
        (0.09 * (hz / 261.6).powf(-0.6) * (1.3 - 0.6 * hammer)).clamp(0.003, 0.24)
    }

    fn t60(&self, key: u8) -> f32 {
        let u = f32::from(key) - LOW;
        let past = (u - 20.0).max(0.0);
        2f32.powf(5.6 - 0.0031 * past * past).max(3.0) * self.unit(p::DECAY)
    }

    fn tune_voice(&mut self, i: usize, hz: f32) {
        let rate = self.rate;
        let key = self.voices[i].key;
        let cents = self.spread();
        let shade = self.shade(hz);
        let t60 = self.t60(key);
        let damped = self.voices[i].damped;
        let release = self.release(key);
        let stiff = self.stiff[usize::from(key.min(127))];
        let voice = &mut self.voices[i];
        voice.hz = hz;
        for (s, line) in voice.strings.iter_mut().enumerate().take(voice.count) {
            let detune = if s == 0 { -0.5 } else { 0.5 } * cents;
            let f = hz * 2f32.powf(detune / 1200.0);
            line.tune(f, stiff, shade, rate);
            let t = if damped {
                release
            } else {
                t60 * (1.0 + 0.1 * s as f32)
            };
            line.set_t60(t, f, rate);
        }
    }

    /// How long a damped string takes to die away.
    fn release(&self, key: u8) -> f32 {
        let k = place(key);
        (0.35 - 0.2 * k) * (0.3 + 1.7 * self.unit(p::DAMPER))
    }

    fn strike(&mut self, i: usize, key: u8, struck: u8, again: bool) {
        let rate = self.rate;
        let v = velocity(struck, self.value(p::CURVE));
        let dynamics = self.unit(p::DYNAMICS);
        let hammer = self.unit(p::HAMMER);
        let hz = self.hz(key) * self.keys.bend(self.keys.slots[i].channel);
        // The felt's contact: longer for a soft blow and a soft hammer, never past 0.6 of a
        // period of the note.
        let contact = ((0.0045 - 0.0036 * v) * (1.6 - 1.2 * hammer)).max(0.000_25);
        let length = (contact * rate).min(0.6 * rate / hz).max(2.0);
        // Velocity spans up to 32 dB; the blow's height makes up for its length, so that
        // every key and every hammer starts the string at about the same swing.
        let height = crate::db_to_gain(-32.0 * dynamics * (1.0 - v)) * 2.0 / length
            * (rate / hz / 200.0).powf(0.35).clamp(0.3, 3.0);
        let noise = self.unit(p::NOISE);
        let channel = self.keys.slots[i].channel;
        let voice = &mut self.voices[i];
        if !again {
            for line in &mut voice.strings {
                line.clear();
            }
            voice.last = 0.0;
            voice.level = 0.0;
        }
        voice.key = key;
        voice.channel = channel;
        voice.count = if key < UNISON_FROM { 1 } else { 2 };
        voice.damped = false;
        voice.blow = (length, height);
        // Each key's hammer takes its own time to fly to the string: struck together, the
        // notes of a chord do not send their first pulses at the same instant.
        voice.at = -flight(key) * rate;
        voice.echo = STRIKE * rate / hz;
        let fc = (hz * (1.0 + 0.8 * v)).min(0.4 * rate);
        voice.shut = 1.0 - (-TAU * fc / rate).exp();
        if !again {
            voice.low = [0.0; 2];
        }
        voice
            .thump
            .hit(noise * v * 0.0006, 0.01, 350.0 + 700.0 * v, rate);
        self.tune_voice(i, hz);
    }

    fn damp(&mut self, i: usize) {
        let rate = self.rate;
        let key = self.voices[i].key;
        if key >= UNDAMPED_FROM {
            return;
        }
        let release = self.release(key);
        let noise = self.unit(p::NOISE);
        let voice = &mut self.voices[i];
        voice.damped = true;
        let hz = voice.hz;
        for line in voice.strings.iter_mut().take(voice.count) {
            line.set_t60(release, hz, rate);
        }
        voice.thump.hit(noise * 0.00015, 0.02, 400.0, rate);
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
                for v in &mut self.voices {
                    for line in &mut v.strings {
                        line.clear();
                    }
                    v.level = 0.0;
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

    /// The preamp's middle and brilliance on the voices' sum, their gains gliding.
    fn preamp(&mut self, x: &mut [f32]) {
        let rate = f64::from(self.rate);
        let glide = 1.0 - (-(x.len() as f32) / (0.03 * self.rate)).exp();
        let middle = self.value(p::MIDDLE);
        let brilliance = match self.value(p::BRILLIANCE).round() as i32 {
            0 => -6.0,
            2 => 5.0,
            _ => 0.0,
        };
        for (stage, target, kind, hz, q) in [
            (&mut self.middle, middle, FilterKind::Peak, 800.0, 0.7),
            (
                &mut self.brilliance,
                brilliance,
                FilterKind::HighShelf,
                2500.0,
                0.6,
            ),
        ] {
            let now = if stage.0.is_nan() {
                target
            } else {
                stage.0 + glide * (target - stage.0)
            };
            if now != stage.0 {
                stage.0 = now;
                stage.1 = Coefficients::design(kind, hz, q, f64::from(now), rate);
            }
            if now.abs() < 1e-3 && target == 0.0 {
                continue;
            }
            for s in x.iter_mut() {
                *s = stage.2.process(&stage.1, f64::from(*s)) as f32;
            }
        }
    }
}

impl Effect for Ep80 {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.keys.reset();
        let cap = (self.rate / LOWEST_HZ) as usize + 8;
        self.voices = (0..VOICES)
            .map(|_| {
                let mut v = Voice::default();
                for line in &mut v.strings {
                    line.buf = vec![0.0; cap];
                }
                v
            })
            .collect();
        for (key, stiff) in self.stiff.iter_mut().enumerate() {
            *stiff = stiffness_for(key as u8, self.rate);
        }
        self.middle = (f32::NAN, Coefficients::IDENTITY, Biquad::default());
        self.brilliance = (f32::NAN, Coefficients::IDENTITY, Biquad::default());
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
        let follow = 1.0 - (-1.0 / (0.05 * rate)).exp();
        for i in 0..self.voices.len().min(VOICES) {
            if !self.keys.slots[i].on {
                continue;
            }
            let voice = &self.voices[i];
            let hz = self.hz(voice.key) * self.keys.bend(voice.channel);
            if (hz - voice.hz).abs() > 1e-4 * hz {
                self.tune_voice(i, hz);
            }
            let voice = &mut self.voices[i];
            let count = voice.count.max(1);
            // The treble comes out softer than the middle, as on the instrument.
            let share = OUTPUT / count as f32
                * crate::db_to_gain(-0.24 * (f32::from(voice.key) - 56.0).max(0.0));
            for out in left.iter_mut() {
                // The blow, minus its reflection from the near end: the strike point.
                let input = if voice.at < voice.blow.0 + voice.echo + 1.0 {
                    let f = voice.force(voice.at) - voice.force(voice.at - voice.echo);
                    voice.at += 1.0;
                    f
                } else {
                    0.0
                };
                let mut sum = 0.0;
                for line in voice.strings.iter_mut().take(count) {
                    sum += line.tick(input);
                }
                // The piezo under the bridge reads its force: a little more of the highs.
                let y = sum - 0.35 * voice.last;
                voice.last = sum;
                voice.low[0] += voice.shut * (y - voice.low[0]);
                voice.low[1] += voice.shut * (voice.low[0] - voice.low[1]);
                let y = voice.low[1];
                voice.level += follow * (y.abs() - voice.level);
                *out += y * share + voice.thump.tick() * OUTPUT;
            }
            let floor = if voice.damped { 1e-5 } else { 2e-6 };
            if voice.level < floor && voice.at > voice.blow.0 + voice.echo && !voice.thump.active()
            {
                for line in &mut voice.strings {
                    line.clear();
                }
                self.keys.finish(i);
            }
        }
        self.preamp(left);
        right.copy_from_slice(left);
        let settings = self.amp_settings();
        self.amp.process(settings, left, right);
    }

    fn reset(&mut self) {
        self.keys.reset();
        for v in &mut self.voices {
            for line in &mut v.strings {
                line.clear();
            }
            v.level = 0.0;
        }
        self.middle.2 = Biquad::default();
        self.brilliance.2 = Biquad::default();
        self.amp.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn piano(settings: &[(usize, f32)]) -> Ep80 {
        let mut e = Ep80::default();
        e.prepare(RATE, 256);
        e.set_param(p::REVERB, 0.0);
        e.set_param(p::CHORUS, 0.0);
        for &(i, v) in settings {
            e.set_param(i, v);
        }
        e
    }

    fn render(e: &mut Ep80, seconds: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let mut out = Vec::with_capacity(n);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        while out.len() < n {
            e.process(&mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Level of `hz` in `x` under a Hann window.
    fn level(x: &[f32], hz: f32) -> f32 {
        let w = TAU * hz / RATE as f32;
        let len = x.len() as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (n, s) in x.iter().enumerate() {
            let hann = 0.5 - 0.5 * (TAU * n as f32 / len).cos();
            re += s * hann * (w * n as f32).cos();
            im += s * hann * (w * n as f32).sin();
        }
        (re * re + im * im).sqrt()
    }

    /// The fundamental's frequency, as the peak of the spectrum within 30 cents of `near`.
    fn fundamental(x: &[f32], near: f32) -> f32 {
        let mut best = (0.0, near);
        for k in -300..=300 {
            let f = near * 2f32.powf(k as f32 / 10.0 / 1200.0);
            let l = level(x, f);
            if l > best.0 {
                best = (l, f);
            }
        }
        best.1
    }

    #[test]
    fn every_key_sounds_in_tune_stretch_included() {
        for stretch in [0.0, 100.0] {
            for key in (28..=100).step_by(9) {
                let mut e = piano(&[(p::STRETCH, stretch), (p::UNISON, 0.0)]);
                e.midi(&MidiEvent::note_on(0, 0, key, 80));
                let x = render(&mut e, 1.2);
                let want = frequency(f32::from(key), 440.0, stretch / 100.0);
                let got = fundamental(&x[RATE as usize / 5..], want);
                let cents = 1200.0 * (got / want).log2();
                assert!(
                    cents.abs() < 1.0,
                    "key {key}: {got} for {want} ({cents} cents)"
                );
            }
        }
    }

    #[test]
    fn the_upper_partials_of_the_bass_are_sharp() {
        let mut e = piano(&[(p::UNISON, 0.0), (p::STRETCH, 0.0)]);
        e.midi(&MidiEvent::note_on(0, 0, 33, 100));
        let x = render(&mut e, 1.0);
        let f0 = frequency(33.0, 440.0, 0.0);
        let seventh = fundamental(&x[4800..], 7.0 * f0 * 1.01);
        assert!(seventh > 7.0 * f0 * 1.002, "{seventh} {}", 7.0 * f0);
    }

    #[test]
    fn harder_is_louder_and_brighter() {
        let play = |velocity| {
            let mut e = piano(&[]);
            e.midi(&MidiEvent::note_on(0, 0, 60, velocity));
            let x = render(&mut e, 0.2);
            let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
            (rms(&x), rms(&d) / rms(&x))
        };
        let (soft, hard) = (play(30), play(120));
        assert!(hard.0 > soft.0 * 2.0, "{soft:?} {hard:?}");
        assert!(hard.1 > soft.1 * 1.15, "{soft:?} {hard:?}");
    }

    #[test]
    fn a_released_note_dies_and_its_voice_ends() {
        let mut e = piano(&[]);
        e.midi(&MidiEvent::note_on(0, 0, 60, 100));
        render(&mut e, 0.2);
        e.midi(&MidiEvent::note_off(0, 0, 60));
        render(&mut e, 2.0);
        assert!(e.keys.slots.iter().all(|s| !s.on));
    }

    #[test]
    fn a_chord_of_every_key_stays_finite_and_under_full_scale() {
        let mut e = piano(&[(p::BRILLIANCE, 2.0), (p::DRIVE, 100.0)]);
        for key in (28..=100).step_by(3) {
            e.midi(&MidiEvent::note_on(0, 0, key, 127));
        }
        let x = render(&mut e, 1.0);
        assert!(x.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }
}
