//! Drum trigger: finds each kick or snare hit on the track and plays a sampled drum with it, at
//! the hit's strength, from the close mics to the room; the amount goes from the track alone to
//! the sample alone, both whole at half way. A short look-ahead puts each sample on its hit's
//! attack. A hit counts once it passes the threshold and stands out from what still rings
//! before it; the tolerance decides by how much, and how soon a new hit may come.

use std::sync::OnceLock;

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{Effect, EffectInfo, MidiEvent, ParamInfo, choice, db_to_gain, param};

pub const KINDS: [&str; 2] = ["Kick", "Snare"];
pub const KICKS: [&str; 6] = [
    "Unruly 20",
    "Unruly 20 dirty",
    "Rusty 24",
    "Rusty 24 open",
    "SM",
    "Muldjord",
];
pub const SNARES: [&str; 7] = [
    "Unruly 13",
    "Unruly 14",
    "Rusty rimshot",
    "SM",
    "Swirly",
    "Muldjord",
    "DRS",
];

pub mod p {
    pub const KIND: usize = 0;
    pub const KICK: usize = 1;
    pub const SNARE: usize = 2;
    pub const AMOUNT: usize = 3;
    pub const THRESHOLD: usize = 4;
    pub const TOLERANCE: usize = 5;
    pub const DYNAMICS: usize = 6;
    pub const ROOM: usize = 7;
    pub const TUNE: usize = 8;
    pub const DECAY: usize = 9;
    pub const LEVEL: usize = 10;
}
const COUNT: usize = 11;

const PARAMS: [ParamInfo; COUNT] = [
    choice("kind", "Drum", &KINDS, 0.0),
    choice("kick", "Kick sound", &KICKS, 0.0),
    choice("snare", "Snare sound", &SNARES, 1.0),
    param("amount", "Amount", 0.0, 100.0, 50.0, "%"),
    param("threshold", "Threshold", -60.0, 0.0, -30.0, "dB"),
    param("tolerance", "Tolerance", 0.0, 100.0, 60.0, "%"),
    param("dynamics", "Dynamics", 0.0, 100.0, 75.0, "%"),
    param("room", "Room", 0.0, 100.0, 20.0, "%"),
    param("tune", "Tune", -12.0, 12.0, 0.0, "st"),
    param("decay", "Decay", 10.0, 100.0, 100.0, "%"),
    param("level", "Level", -24.0, 12.0, 0.0, "dB"),
];

pub const INFO: EffectInfo = EffectInfo {
    id: "trigger",
    name: "Strada-Trigger",
    params: &PARAMS,
    meters: &["Input", "Detection", "Hits", "Velocity"],
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Reinforces or replaces a kick or a snare with sampled drums, hit for hit.",
};

/// The rate the library was recorded at.
const RATE: f32 = 48_000.0;
/// Velocity layers, each with two takes played in turn.
pub const LAYERS: usize = 5;
/// Look-ahead, and the time a hit's peak is measured over after it starts.
const LOOKAHEAD_MS: f32 = 20.0;
const WINDOW_MS: f32 = 5.0;
/// How far before the threshold the attack is looked for.
const BACK_MS: f32 = 14.0;
/// What rings before a hit is measured over this long, to find the hit's rise above it.
const BEFORE_MS: f32 = 3.0;
/// The share of a hit's peak that marks its attack, on the track as in the samples.
const ATTACK: f32 = 0.15;
const VOICES: usize = 4;
/// A hit stands out from what rang this long before it.
const RISE_MS: f32 = 6.0;

/// A sound: its close and room mics, hit after hit, and where each hit's attack is.
#[derive(Debug)]
pub struct Sound {
    pub close: Vec<f32>,
    pub room: Vec<f32>,
    pub hits: Vec<(usize, usize, usize)>,
}

impl Sound {
    /// Hit `layer` (0 softest), take `take`: start, length and attack in the buffers.
    pub fn hit(&self, layer: usize, take: usize) -> (usize, usize, usize) {
        self.hits[(layer.min(LAYERS - 1) * 2 + take % 2).min(self.hits.len() - 1)]
    }
}

macro_rules! library {
    ($($file:literal, $lengths:expr;)*) => {
        [$((include_bytes!(concat!("../assets/trigger/", $file, ".flac")).as_slice(), $lengths),)*]
    };
}

/// Kicks then snares, in the order of [`KICKS`] and [`SNARES`]; hits by layer then take.
const FILES: [(&[u8], [usize; 10]); 13] = library! {
    "kick_unruly", [62643, 65315, 49651, 48549, 48111, 52633, 48104, 50561, 54662, 50564];
    "kick_unruly_dirty", [54260, 53870, 53798, 54765, 49703, 49171, 40454, 48535, 52301, 55781];
    "kick_rusty", [67103, 66324, 66016, 64118, 60794, 59982, 57431, 59973, 50199, 48998];
    "kick_rusty_open", [67200, 67200, 67200, 67200, 67200, 67200, 67200, 67200, 67200, 67200];
    "kick_sm", [61697, 61015, 49788, 49788, 43130, 43812, 39364, 40110, 35560, 37183];
    "kick_muldjord", [15089, 29933, 29297, 37420, 27721, 38382, 23702, 29282, 31421, 33421];
    "snare_unruly13", [37774, 36446, 35625, 36948, 26157, 27229, 26161, 34531, 35524, 32556];
    "snare_unruly14", [29255, 29124, 29297, 29260, 29273, 29359, 29359, 29363, 29292, 28460];
    "snare_rusty_rim", [31820, 31132, 31931, 32427, 33159, 32836, 44741, 31750, 33396, 33134];
    "snare_sm", [10214, 10844, 10941, 10415, 10013, 10245, 9665, 9993, 11473, 10471];
    "snare_swirly", [52188, 52424, 50777, 50820, 52800, 51676, 52800, 52800, 52720, 52800];
    "snare_muldjord", [28579, 36187, 30114, 41888, 37065, 37999, 46493, 40908, 52800, 52692];
    "snare_drs", [18210, 17837, 19382, 19424, 20848, 26940, 29687, 26704, 36295, 34547];
};

/// The sounds of a kind (0 kicks, 1 snares), decoded once for every instance.
pub fn sounds(kind: usize) -> &'static [Sound] {
    static LIBRARY: OnceLock<Vec<Sound>> = OnceLock::new();
    let all = LIBRARY.get_or_init(|| {
        FILES
            .iter()
            .map(|(data, lengths)| decode(data, lengths))
            .collect()
    });
    if kind == 0 {
        &all[..KICKS.len()]
    } else {
        &all[KICKS.len()..]
    }
}

fn decode(data: &'static [u8], lengths: &[usize]) -> Sound {
    let [close, room] = decode_flac(data);
    let mut hits = Vec::with_capacity(lengths.len());
    let mut start = 0;
    for &length in lengths {
        let end = (start + length).min(close.len());
        let body = &close[start..end];
        let peak = body.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let attack = body
            .iter()
            .position(|s| s.abs() >= peak * ATTACK)
            .unwrap_or(0);
        hits.push((start, end - start, attack));
        start = end;
    }
    Sound { close, room, hits }
}

/// The two channels of an embedded FLAC file.
fn decode_flac(data: &'static [u8]) -> [Vec<f32>; 2] {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    let source = MediaSourceStream::new(Box::new(std::io::Cursor::new(data)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("flac");
    let mut out = [Vec::new(), Vec::new()];
    let Ok(mut format) = symphonia::default::get_probe().probe(
        &hint,
        source,
        FormatOptions::default(),
        MetadataOptions::default(),
    ) else {
        return out;
    };
    let Some(track) = format.default_track(TrackType::Audio) else {
        return out;
    };
    let id = track.id;
    let Some(params) = track.codec_params.as_ref().and_then(|p| p.audio()) else {
        return out;
    };
    let Ok(mut decoder) = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
    else {
        return out;
    };
    let mut planes: Vec<Vec<f32>> = Vec::new();
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        decoded.copy_to_vecs_planar(&mut planes);
        for (k, channel) in out.iter_mut().enumerate() {
            if let Some(plane) = planes.get(k.min(planes.len().saturating_sub(1))) {
                channel.extend_from_slice(plane);
            }
        }
    }
    out
}

/// One hit of a sound in flight.
#[derive(Clone, Copy, Debug, Default)]
struct Voice {
    active: bool,
    kind: usize,
    sound: usize,
    start: usize,
    length: usize,
    /// Where it reads, in the hit's samples (negative: not yet).
    at: f64,
    step: f64,
    gain: f32,
    /// Level of its fade, and its per-sample factor once fading.
    level: f32,
    fade: f32,
    /// When its decay starts, in the hit's samples, and how fast it goes then.
    tail: f64,
    falloff: f32,
}

/// A hit found, its peak measured until `until`.
#[derive(Clone, Copy, Debug)]
struct Pending {
    onset: u64,
    until: u64,
    detected: f32,
    raw: f32,
}

pub struct Trigger {
    rate: f32,
    values: [f32; COUNT],
    lookahead: usize,
    /// The track, delayed by the look-ahead.
    delay: Vec<[f32; 2]>,
    /// The track's magnitude lately, to find each attack.
    history: Vec<f32>,
    now: u64,
    filters: [Biquad; 3],
    coefficients: [Coefficients; 3],
    filtered_for: Option<usize>,
    /// Fast envelope of the detection, the peak it decays from, and that peak lately.
    fast: f32,
    held: f32,
    helds: Vec<f32>,
    armed: u64,
    pending: Option<Pending>,
    /// The strongest hit lately, the reference for the dynamics.
    strongest: f32,
    voices: [Voice; VOICES],
    take: usize,
    room: Box<dyn Effect>,
    bus: [Vec<f32>; 2],
    meters: [f32; 4],
}

impl std::fmt::Debug for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Trigger")
            .field("rate", &self.rate)
            .finish_non_exhaustive()
    }
}

impl Default for Trigger {
    fn default() -> Self {
        let _ = sounds(0);
        let mut room = crate::create("reverb").expect("built-in reverb");
        let info = crate::info("reverb").expect("built-in reverb");
        for (name, value) in [
            ("type", 0.0),
            ("decay", 0.9),
            ("predelay", 6.0),
            ("size", 45.0),
            ("mix", 100.0),
            ("low_cut", 120.0),
        ] {
            if let Some(i) = info.params.iter().position(|q| q.id == name) {
                room.set_param(i, value);
            }
        }
        Self {
            rate: RATE,
            values: PARAMS.map(|p| p.default),
            lookahead: 0,
            delay: Vec::new(),
            history: Vec::new(),
            now: 0,
            filters: [Biquad::default(); 3],
            coefficients: [Coefficients::IDENTITY; 3],
            filtered_for: None,
            fast: 0.0,
            held: 0.0,
            helds: Vec::new(),
            armed: 0,
            pending: None,
            strongest: 0.0,
            voices: [Voice::default(); VOICES],
            take: 0,
            room,
            bus: Default::default(),
            meters: [0.0; 4],
        }
    }
}

/// Per-sample factor of an exponential fall of 60 dB over `seconds`.
fn falloff(seconds: f32, rate: f32) -> f32 {
    (-6.9 / (seconds.max(0.0005) * rate)).exp()
}

/// The layer and level of a hit `relative` (0 to 1) to the strongest lately: a layer per 6 dB.
pub fn layer_of(relative: f32) -> usize {
    let db = 20.0 * relative.max(1e-6).log10();
    (LAYERS as f32 - 1.0 + db / 6.0)
        .round()
        .clamp(0.0, LAYERS as f32 - 1.0) as usize
}

impl Trigger {
    fn kind(&self) -> usize {
        (self.values[p::KIND].round() as usize).min(1)
    }

    fn sound(&self) -> usize {
        let kind = self.kind();
        let (index, count) = if kind == 0 {
            (p::KICK, KICKS.len())
        } else {
            (p::SNARE, SNARES.len())
        };
        (self.values[index].round().max(0.0) as usize).min(count - 1)
    }

    /// The detection hears the kick's low end, or the snare above the kick's: steep slopes,
    /// to tell a soft hit from the other drum bleeding in.
    fn update_filters(&mut self) {
        let kind = self.kind();
        if self.filtered_for == Some(kind) {
            return;
        }
        self.filtered_for = Some(kind);
        let rate = f64::from(self.rate);
        let design =
            |k, hz| Coefficients::design(k, hz, std::f64::consts::FRAC_1_SQRT_2, 0.0, rate);
        self.coefficients = if kind == 0 {
            [
                design(FilterKind::HighPass, 30.0),
                design(FilterKind::LowPass, 100.0),
                design(FilterKind::LowPass, 100.0),
            ]
        } else {
            [
                design(FilterKind::HighPass, 250.0),
                design(FilterKind::HighPass, 250.0),
                design(FilterKind::LowPass, 7000.0),
            ]
        };
        self.filters = [Biquad::default(); 3];
    }

    /// Starts the sound for a hit `relative` to the strongest, `strength` loud (linear), whose
    /// attack is heard `wait` samples from now.
    fn play(&mut self, relative: f32, strength: f32, wait: f64) {
        let kind = self.kind();
        let sound = self.sound();
        let dynamics = self.values[p::DYNAMICS] / 100.0;
        let relative = relative.clamp(0.0, 1.0);
        // Fixed at the strongest hit's layer and level without dynamics, the hit's own with.
        let shaped = 10f32.powf(dynamics * 20.0 * relative.max(1e-4).log10() / 20.0);
        let layer = layer_of(shaped);
        self.take = self.take.wrapping_add(1);
        let hits = &sounds(kind)[sound];
        let (start, length, attack) = hits.hit(layer, self.take);
        // The sample's attack goes the way the track's does.
        let strength = strength * hits.close[start + attack].signum();
        let step = f64::from(RATE / self.rate * 2f32.powf(self.values[p::TUNE] / 12.0));
        let decay = self.values[p::DECAY] / 100.0;
        let seconds = length as f32 / RATE;
        // A new hit damps the ones still ringing, as on a real head.
        let damp = falloff(0.025, self.rate);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.fade = v.fade.min(damp);
        }
        let slot = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                (0..VOICES)
                    .min_by(|&a, &b| self.voices[a].level.total_cmp(&self.voices[b].level))
                    .unwrap_or(0)
            });
        self.voices[slot] = Voice {
            active: true,
            kind,
            sound,
            start,
            length,
            at: attack as f64 - wait * step,
            step,
            gain: strength * shaped / relative.max(1e-4),
            level: 1.0,
            fade: 1.0,
            tail: if decay < 1.0 {
                f64::from(0.5 * decay) * length as f64
            } else {
                f64::MAX
            },
            falloff: falloff(0.3 * decay * seconds, RATE / step as f32),
        };
        self.meters[2] = (self.meters[2] + 1.0) % 1_000_000.0;
        self.meters[3] = shaped;
    }

    /// The next sample of the sounds playing: close and room mics.
    fn voices(&mut self) -> (f32, f32) {
        let (mut close, mut room) = (0.0, 0.0);
        for v in &mut self.voices {
            if !v.active {
                continue;
            }
            let at = v.at;
            v.at += v.step;
            if at < 0.0 {
                continue;
            }
            let i = at as usize;
            if i + 1 >= v.length || v.level < 1e-4 {
                v.active = false;
                continue;
            }
            let sound = &sounds(v.kind)[v.sound];
            let frac = (at - i as f64) as f32;
            let read = |buffer: &[f32]| {
                let a = buffer[v.start + i];
                a + (buffer[v.start + i + 1] - a) * frac
            };
            if at > v.tail {
                v.fade = v.fade.min(v.falloff);
            }
            v.level *= v.fade;
            let g = v.gain * v.level;
            close += read(&sound.close) * g;
            room += read(&sound.room) * g;
        }
        (close, room)
    }
}

impl Effect for Trigger {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        self.lookahead = (LOOKAHEAD_MS / 1000.0 * self.rate) as usize;
        self.delay = vec![[0.0; 2]; self.lookahead.max(1)];
        self.history =
            vec![0.0; ((BACK_MS + WINDOW_MS + BEFORE_MS) / 1000.0 * self.rate) as usize + 2];
        self.helds = vec![0.0; (RISE_MS / 1000.0 * self.rate) as usize + 1];
        self.bus = [vec![0.0; max_frames], vec![0.0; max_frames]];
        self.room.prepare(sample_rate, max_frames);
        self.filtered_for = None;
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn latency(&self) -> u32 {
        self.lookahead as u32
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((_, velocity)) = event.as_note_on() {
            let strength = f32::from(velocity) / 127.0;
            self.play(strength, 0.7 * strength, 0.0);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        if self.delay.is_empty() || self.helds.is_empty() {
            return;
        }
        self.update_filters();
        let rate = self.rate;
        let threshold = db_to_gain(self.values[p::THRESHOLD]);
        let tolerance = self.values[p::TOLERANCE] / 100.0;
        let kick = self.kind() == 0;
        // Strict: a hit rises 9 dB over what rings; tolerant: 2 dB, and may come sooner.
        let rise = db_to_gain(9.0 - 7.0 * tolerance);
        let gap = ((if kick { 90.0 } else { 70.0 } - 45.0 * tolerance) / 1000.0 * rate) as u64;
        let fast_fall = falloff(if kick { 0.03 } else { 0.012 }, rate);
        let held_fall = falloff(if kick { 0.5 } else { 0.25 }, rate);
        let forget = falloff(20.0, rate);
        let window = (WINDOW_MS / 1000.0 * rate) as u64;
        let back = (BACK_MS / 1000.0 * rate) as u64;
        let ahead = (BEFORE_MS / 1000.0 * rate) as u64;
        let amount = self.values[p::AMOUNT] / 100.0;
        let dry = (2.0 * (1.0 - amount)).min(1.0);
        let wet = (2.0 * amount).min(1.0) * db_to_gain(self.values[p::LEVEL]);
        let room = self.values[p::ROOM] / 100.0;
        let send = 0.6 * room * room;
        if self.bus[0].len() < n {
            for b in &mut self.bus {
                b.resize(n, 0.0);
            }
        }
        let meter_fall = falloff(1.5, rate);
        let size = self.delay.len();
        let length = self.history.len();
        for i in 0..n {
            let (l, r) = (left[i], right[i]);
            let t = self.now;
            let slot = (t % size as u64) as usize;
            let [dl, dr] = self.delay[slot];
            self.delay[slot] = [l, r];
            let mono = 0.5 * (l + r);
            self.history[(t % length as u64) as usize] = mono;
            let mut x = f64::from(mono);
            for (f, c) in self.filters.iter_mut().zip(&self.coefficients) {
                x = f.process(c, x);
            }
            let x = (x as f32).abs();
            let ago = (t % self.helds.len() as u64) as usize;
            let before = self.helds[ago];
            self.fast = x.max(self.fast * fast_fall);
            self.held = self.fast.max(self.held * held_fall);
            self.helds[ago] = self.held;
            self.strongest *= forget;
            self.meters[0] = mono.abs().max(self.meters[0] * meter_fall);
            self.meters[1] = self.fast.max(self.meters[1] * meter_fall);
            if let Some(mut hit) = self.pending {
                hit.detected = hit.detected.max(self.fast);
                hit.raw = hit.raw.max(mono.abs());
                self.pending = Some(hit);
                if t >= hit.until {
                    self.pending = None;
                    // The attack: the first sample past a share of the peak, a little before
                    // the threshold was crossed.
                    let from = hit
                        .onset
                        .saturating_sub(back)
                        .max((t + 1).saturating_sub(length as u64));
                    let signed = |k: u64| self.history[(k % length as u64) as usize];
                    let magnitude = |k: u64| signed(k).abs();
                    let before = (from.saturating_sub(ahead)..from)
                        .map(magnitude)
                        .fold(0.0f32, f32::max);
                    let level = (hit.raw * ATTACK).max(before * 2.0);
                    let attack = (from..=t)
                        .find(|&k| magnitude(k) >= level)
                        .unwrap_or(hit.onset);
                    let polarity = signed(attack).signum();
                    self.strongest = self.strongest.max(hit.raw);
                    let relative = hit.raw / self.strongest.max(1e-9);
                    // Heard where the track's attack comes out of the delay.
                    let wait = (attack + self.lookahead as u64) as f64 - t as f64;
                    self.play(relative, hit.raw * 1.05 * polarity, wait);
                }
            } else if t >= self.armed && x > threshold && self.fast > before * rise {
                self.pending = Some(Pending {
                    onset: t,
                    until: t + window,
                    detected: self.fast,
                    raw: mono.abs(),
                });
                self.armed = t + gap;
            }
            let (close, ambience) = self.voices();
            let sample = close + (ambience - close) * room;
            self.bus[0][i] = sample * send;
            self.bus[1][i] = sample * send;
            left[i] = dl * dry + sample * wet;
            right[i] = dr * dry + sample * wet;
            self.now += 1;
        }
        if send > 0.0 {
            let [bl, br] = &mut self.bus;
            self.room.process(&mut bl[..n], &mut br[..n]);
            for i in 0..n {
                left[i] += bl[i] * wet;
                right[i] += br[i] * wet;
            }
        }
    }

    fn reset(&mut self) {
        self.delay.fill([0.0; 2]);
        self.history.fill(0.0);
        self.voices = [Voice::default(); VOICES];
        self.pending = None;
        self.fast = 0.0;
        self.held = 0.0;
        self.helds.fill(0.0);
        self.armed = 0;
        self.filters = [Biquad::default(); 3];
        self.room.reset();
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, m) in out.iter_mut().zip(self.meters) {
            *o = m;
        }
    }
}

/// A hit of `sound` of `kind` at `velocity` (0 to 1), close mics to room by `room`, mono at
/// `rate`: for auditioning it.
pub fn audition(kind: usize, sound: usize, velocity: f32, room: f32, rate: u32) -> Vec<f32> {
    let Some(s) = sounds(kind.min(1)).get(sound) else {
        return Vec::new();
    };
    let (start, length, _) = s.hit(layer_of(velocity), 0);
    let step = f64::from(RATE / rate as f32);
    let mut out = Vec::with_capacity((length as f64 / step) as usize + 1);
    let mut at = 0.0;
    while (at as usize) + 1 < length {
        let i = at as usize;
        let frac = (at - i as f64) as f32;
        let read = |b: &[f32]| b[start + i] + (b[start + i + 1] - b[start + i]) * frac;
        let (c, r) = (read(&s.close), read(&s.room));
        out.push((c + (r - c) * room) * velocity);
        at += step;
    }
    out
}

/// The strongest take of each sound's softest to hardest layers, for drawing: peaks over
/// `bins` columns of the hardest one.
pub fn shape(kind: usize, sound: usize, bins: usize) -> Vec<f32> {
    let Some(s) = sounds(kind.min(1)).get(sound) else {
        return Vec::new();
    };
    let (start, length, _) = s.hit(LAYERS - 1, 0);
    let body = &s.close[start..start + length];
    let per = (length / bins.max(1)).max(1);
    body.chunks(per)
        .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trigger(values: &[(usize, f32)]) -> Trigger {
        let mut t = Trigger::default();
        for (i, v) in values {
            t.set_param(*i, *v);
        }
        t.prepare(48_000, 512);
        t
    }

    /// A kick-like hit: a falling sine with a click, `level` loud, at `at` samples.
    fn hits(at: &[(usize, f32)], total: usize, hz: f32) -> Vec<f32> {
        let mut out = vec![0.0f32; total];
        for &(start, level) in at {
            for k in 0..(total - start).min(24_000) {
                let t = k as f32 / 48_000.0;
                let f = hz * (1.0 + 1.5 * (-t / 0.02).exp());
                out[start + k] += level * (std::f32::consts::TAU * f * t).sin() * (-t / 0.15).exp();
            }
        }
        out
    }

    fn run(t: &mut Trigger, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in input.chunks(512) {
            let (mut l, mut r) = (chunk.to_vec(), chunk.to_vec());
            t.process(&mut l, &mut r);
            out.extend(l);
        }
        out
    }

    #[test]
    fn every_sound_decodes_with_its_hits() {
        for kind in 0..2 {
            let count = if kind == 0 { KICKS.len() } else { SNARES.len() };
            assert_eq!(sounds(kind).len(), count);
            for (k, s) in sounds(kind).iter().enumerate() {
                assert_eq!(s.close.len(), s.room.len(), "{kind} {k}");
                let (start, length, attack) = s.hit(LAYERS - 1, 1);
                assert!(start + length <= s.close.len(), "{kind} {k}");
                assert!(attack < 400, "{kind} {k}: attack at {attack}");
                let peak = s.close[start..start + length]
                    .iter()
                    .fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak > 0.5, "{kind} {k}: {peak}");
            }
        }
    }

    #[test]
    fn each_hit_triggers_once_on_its_attack() {
        let starts = [4_800, 28_800, 52_800, 60_000];
        let input = hits(&starts.map(|s| (s, 0.8)), 96_000, 55.0);
        let mut t = trigger(&[(p::AMOUNT, 100.0), (p::ROOM, 0.0)]);
        let out = run(&mut t, &input);
        assert_eq!(t.meters[2] as usize, starts.len(), "one sample per hit");
        let latency = t.latency() as usize;
        for start in starts {
            // The sample's attack lands within a millisecond of the track's, once delayed.
            let from = start + latency - 240;
            let onset = out[from..from + 960]
                .iter()
                .position(|v| v.abs() > 0.05)
                .map(|k| k + from);
            let onset = onset.expect("a sample is heard");
            assert!(
                onset.abs_diff(start + latency) < 48,
                "attack at {onset}, hit at {}",
                start + latency
            );
        }
    }

    #[test]
    fn the_threshold_and_tolerance_keep_bleed_and_ringing_out() {
        // A loud kick, then a soft one under the threshold.
        let input = hits(&[(4_800, 0.8), (40_000, 0.01)], 72_000, 55.0);
        let mut t = trigger(&[(p::THRESHOLD, -24.0)]);
        run(&mut t, &input);
        assert_eq!(t.meters[2] as usize, 1);
        // Strict, a hit 3 dB over the ringing one is taken for ringing; tolerant, it counts.
        let input = hits(&[(4_800, 0.6), (9_600, 0.6 * 1.6)], 48_000, 55.0);
        let mut strict = trigger(&[(p::TOLERANCE, 0.0)]);
        run(&mut strict, &input);
        let mut tolerant = trigger(&[(p::TOLERANCE, 100.0)]);
        run(&mut tolerant, &input);
        assert_eq!(strict.meters[2] as usize, 1);
        assert_eq!(tolerant.meters[2] as usize, 2);
    }

    #[test]
    fn amount_goes_from_the_track_to_the_sample() {
        let input = hits(&[(4_800, 0.8)], 48_000, 55.0);
        let render = |amount: f32| {
            let mut t = trigger(&[(p::AMOUNT, amount), (p::ROOM, 0.0)]);
            let out = run(&mut t, &input);
            (out, t.latency() as usize)
        };
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        let (track, latency) = render(0.0);
        let difference: f32 = input
            .iter()
            .zip(&track[latency..])
            .map(|(a, b)| (b - a) * (b - a))
            .sum();
        assert!(difference < 1e-9, "the track alone, delayed");
        let (half, _) = render(50.0);
        let (full, _) = render(100.0);
        assert!(
            energy(&half) > energy(&input) && energy(&half) > energy(&full),
            "both whole at half way, adding up"
        );
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let ratio = 20.0 * (peak(&full) / peak(&input)).log10();
        assert!(
            ratio.abs() < 3.0,
            "the sample peaks as the track: {ratio:.1} dB"
        );
    }

    #[test]
    fn recorded_kicks_are_found_among_snare_bleed() {
        // Recorded kicks, loud and soft, some as fast as sixteenths at 120 bpm, with the
        // snare bleeding in 18 dB lower between them.
        let kick = &sounds(0)[2];
        let snare = &sounds(1)[1];
        let mut input = vec![0.0f32; 4 * 48_000];
        let mut add = |sound: &Sound, layer: usize, at: usize, level: f32| {
            let (start, length, _) = sound.hit(layer, at);
            for k in 0..length.min(input.len() - at) {
                input[at + k] += sound.close[start + k] * level;
            }
        };
        let kicks = [
            (2_400, 4, 0.9),
            (26_400, 2, 0.5),
            (32_400, 4, 0.8),
            (50_400, 1, 0.3),
            (98_400, 4, 0.9),
            (104_400, 3, 0.7),
            (146_400, 4, 0.9),
        ];
        for (at, layer, level) in kicks {
            add(kick, layer, at, level);
        }
        for at in [14_400, 62_400, 110_400, 158_400] {
            add(snare, 4, at, 0.12);
        }
        let mut t = trigger(&[(p::THRESHOLD, -36.0)]);
        run(&mut t, &input);
        assert_eq!(t.meters[2] as usize, kicks.len());
    }

    #[test]
    fn softer_hits_play_softer_layers_with_dynamics() {
        assert_eq!(layer_of(1.0), LAYERS - 1);
        assert_eq!(layer_of(0.5), LAYERS - 2);
        assert_eq!(layer_of(0.01), 0);
        let input = hits(&[(4_800, 0.8), (40_000, 0.2)], 72_000, 55.0);
        for (dynamics, expected) in [(100.0, 0.25), (0.0, 1.0)] {
            let mut t = trigger(&[(p::DYNAMICS, dynamics), (p::THRESHOLD, -40.0)]);
            run(&mut t, &input);
            assert_eq!(t.meters[2] as usize, 2);
            assert!(
                (t.meters[3] - expected).abs() < 0.08,
                "dynamics {dynamics}: {}",
                t.meters[3]
            );
        }
    }

    #[test]
    fn the_room_and_the_snare_sound_differ() {
        let input = hits(&[(4_800, 0.8)], 48_000, 220.0);
        let tail = |values: &[(usize, f32)]| {
            let mut t = trigger(values);
            let out = run(&mut t, &input);
            out[24_000..].iter().map(|v| v * v).sum::<f32>()
        };
        let base = [(p::AMOUNT, 100.0), (p::KIND, 1.0)];
        let close = tail(&[base[0], base[1], (p::ROOM, 0.0)]);
        let wide = tail(&[base[0], base[1], (p::ROOM, 100.0)]);
        assert!(wide > close * 2.0, "{close} {wide}");
    }
}
