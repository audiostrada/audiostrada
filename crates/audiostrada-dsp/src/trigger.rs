//! Drum trigger: finds each kick or snare hit on the track and plays a sampled drum with it, at
//! the hit's strength, from the close mics to the room; the amount goes from the track alone to
//! the sample alone, both whole at half way. A short look-ahead puts each sample on its hit's
//! attack. A hit counts once it passes the threshold and stands out from what still rings
//! before it; the tolerance decides by how much, and how soon a new hit may come. The sample
//! matches the energy of the track's attack. Each drum can also play a file of one's own.

use std::sync::OnceLock;

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{
    Effect, EffectInfo, FileData, FileKind, FileSlot, MidiEvent, ParamInfo, choice, db_to_gain,
    param, switch,
};

pub const KINDS: [&str; 2] = ["Kick", "Snare"];
/// The sounds of each drum; the last one plays the file loaded for it.
pub const KICKS: [&str; 7] = [
    "Studio",
    "Garage",
    "Deep",
    "Boom",
    "Vintage",
    "Rock",
    "Your sound",
];
pub const SNARES: [&str; 8] = [
    "Crisp",
    "Studio",
    "Rimshot",
    "Vintage",
    "Ringing",
    "Rock",
    "Session",
    "Your sound",
];
/// Built-in sounds of each drum.
pub const BUILT_IN: [usize; 2] = [KICKS.len() - 1, SNARES.len() - 1];

const EXTENSIONS: &[&str] = &["wav", "flac", "aif", "aiff", "ogg", "mp3"];
/// The file each drum plays as its own sound.
pub const SLOTS: [FileSlot; 2] = [
    FileSlot {
        id: "kick_file",
        name: "Kick sample",
        kind: FileKind::Audio,
        extensions: EXTENSIONS,
    },
    FileSlot {
        id: "snare_file",
        name: "Snare sample",
        kind: FileKind::Audio,
        extensions: EXTENSIONS,
    },
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
    pub const COMP: usize = 11;
    pub const LOW: usize = 12;
    pub const MID: usize = 13;
    pub const HIGH: usize = 14;
    pub const DRIVE: usize = 15;
    pub const MATCH: usize = 16;
}
const COUNT: usize = 17;

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
    param("comp", "Compression", 0.0, 100.0, 0.0, "%"),
    param("low", "Bass", -12.0, 12.0, 0.0, "dB"),
    param("mid", "Mid", -12.0, 12.0, 0.0, "dB"),
    param("high", "Treble", -12.0, 12.0, 0.0, "dB"),
    param("drive", "Drive", 0.0, 100.0, 0.0, "%"),
    // Read by the editor, which tunes the sound to the track's drum while it is on.
    switch("match", "Match the track", 0.0),
];

pub const INFO: EffectInfo = EffectInfo {
    id: "trigger",
    name: "Strada-Trigger",
    params: &PARAMS,
    meters: &["Input", "Detection", "Hits", "Velocity"],
    instrument: false,
    files: &SLOTS,
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
    pub rate: f32,
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
        &all[..BUILT_IN[0]]
    } else {
        &all[BUILT_IN[0]..]
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
    Sound {
        close,
        room,
        hits,
        rate: RATE,
    }
}

/// A file of one's own as a sound: one hit, mono, without the silence before it.
fn own(channels: &[Vec<f32>], rate: u32) -> Result<Sound, String> {
    let length = channels.iter().map(Vec::len).max().unwrap_or(0);
    let n = channels.len().max(1) as f32;
    let mono: Vec<f32> = (0..length)
        .map(|i| {
            channels
                .iter()
                .map(|c| c.get(i).copied().unwrap_or(0.0))
                .sum::<f32>()
                / n
        })
        .collect();
    let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak < 1e-4 {
        return Err("the file is silent".into());
    }
    let first = mono
        .iter()
        .position(|s| s.abs() >= peak * 0.02)
        .unwrap_or(0);
    let start = first.saturating_sub(12);
    let end = (start + (4.0 * rate as f32) as usize).min(length);
    let close: Vec<f32> = mono[start..end].iter().map(|s| s * 0.95 / peak).collect();
    let attack = close
        .iter()
        .position(|s| s.abs() >= 0.95 * ATTACK)
        .unwrap_or(0);
    Ok(Sound {
        room: close.clone(),
        hits: vec![(0, close.len(), attack)],
        close,
        rate: rate as f32,
    })
}

/// A sound of a drum: built in, or its own file once loaded.
fn lookup(custom: &[Option<Sound>; 2], kind: usize, sound: usize) -> Option<&Sound> {
    if sound >= BUILT_IN[kind] {
        custom[kind].as_ref()
    } else {
        sounds(kind).get(sound)
    }
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

/// A hit being heard whole on the track, to set the sample's level by its body: the track's
/// energy so far, until when, and what the sample gave over the same time at the gain used.
#[derive(Clone, Copy, Debug)]
struct Measure {
    until: u64,
    track: f32,
    count: u32,
    sample: f32,
    sound: (usize, usize),
}

/// How long a hit's body is measured for its level.
const BODY_MS: f32 = 80.0;

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
    custom: [Option<Sound>; 2],
    measure: Option<Measure>,
    /// Gain that brings the samples to the track's level over their body (dB), and the sound
    /// it was found for.
    calibration: Option<(f32, (usize, usize))>,
    /// Gain of a sample at the strongest hit's level: the processing works below it.
    reference: f32,
    eq: [Biquad; 3],
    eq_coefficients: [Coefficients; 3],
    eq_for: Option<[u32; 4]>,
    work: [Vec<f32>; 2],
    comp: Box<dyn Effect>,
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
        let mut comp = crate::create("compressor").expect("built-in compressor");
        let info = crate::info("compressor").expect("built-in compressor");
        for (name, value) in [
            ("type", 1.0),
            ("attack", 10.0),
            ("release", 100.0),
            ("knee", 6.0),
        ] {
            if let Some(i) = info.params.iter().position(|q| q.id == name) {
                comp.set_param(i, value);
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
            custom: [None, None],
            measure: None,
            calibration: None,
            reference: 0.0,
            eq: [Biquad::default(); 3],
            eq_coefficients: [Coefficients::IDENTITY; 3],
            eq_for: None,
            work: Default::default(),
            comp,
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
        let index = if kind == 0 { p::KICK } else { p::SNARE };
        (self.values[index].round().max(0.0) as usize).min(BUILT_IN[kind])
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

    /// Starts the sound for a hit `relative` to the strongest, whose attack is heard `wait`
    /// samples from now and should carry the energy `energy` (RMS over `span` samples, signed
    /// as the track's attack).
    fn play(&mut self, relative: f32, (energy, span): (f32, usize), wait: f64) {
        let kind = self.kind();
        let sound = self.sound();
        let dynamics = self.values[p::DYNAMICS] / 100.0;
        let relative = relative.clamp(0.0, 1.0);
        // Fixed at the strongest hit's layer and level without dynamics, the hit's own with.
        let shaped = 10f32.powf(dynamics * 20.0 * relative.max(1e-4).log10() / 20.0);
        let layer = layer_of(shaped);
        self.take = self.take.wrapping_add(1);
        let Some(hits) = lookup(&self.custom, kind, sound) else {
            return;
        };
        let (start, length, attack) = hits.hit(layer, self.take);
        let step = f64::from(hits.rate / self.rate * 2f32.powf(self.values[p::TUNE] / 12.0));
        // As much energy as the track's attack over the same time, the same way up.
        let read = ((span as f64 * step) as usize).max(1);
        let body = &hits.close[start + attack..start + (attack + read).min(length)];
        let own = (body.iter().map(|v| v * v).sum::<f32>() / body.len().max(1) as f32).sqrt();
        let matched = energy.abs() / own.max(1e-6);
        let calibration = match self.calibration {
            Some((db, for_sound)) if for_sound == (kind, sound) => db_to_gain(db),
            _ => 1.0,
        };
        let strength =
            matched * calibration * energy.signum() * hits.close[start + attack].signum();
        // What the sample gives over its body at this gain, to compare with the track's.
        let room = self.values[p::ROOM] / 100.0;
        let end = (attack + (f64::from(BODY_MS / 1000.0 * self.rate) * step) as usize).min(length);
        let heard = (attack..end)
            .map(|k| {
                let (c, r) = (hits.close[start + k], hits.room[start + k]);
                let x = c + (r - c) * room;
                x * x
            })
            .sum::<f32>()
            / (end - attack).max(1) as f32;
        if let Some(m) = &mut self.measure {
            m.sample = heard.sqrt() * strength.abs() * shaped / relative.max(1e-4);
            m.sound = (kind, sound);
        }
        self.reference = (strength / relative.max(1e-4)).abs();
        let decay = self.values[p::DECAY] / 100.0;
        let seconds = length as f32 / hits.rate;
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
            falloff: falloff(0.3 * decay * seconds, hits.rate / step as f32),
        };
        self.meters[2] = (self.meters[2] + 1.0) % 1_000_000.0;
        self.meters[3] = shaped;
    }

    /// The next sample of the sounds playing: close and room mics.
    fn voices(&mut self) -> (f32, f32) {
        let (mut close, mut room) = (0.0, 0.0);
        let custom = &self.custom;
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
            let Some(sound) = lookup(custom, v.kind, v.sound) else {
                v.active = false;
                continue;
            };
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

impl Trigger {
    /// Brings the samples to the track's level over a hit's body, by halves after the first.
    fn calibrate(&mut self, m: &Measure) {
        if m.sound.0 == usize::MAX || m.sample <= 1e-9 || m.count == 0 {
            return;
        }
        let track = (m.track / m.count as f32).sqrt();
        let step = 20.0 * (track / m.sample).max(1e-6).log10();
        let db = match self.calibration {
            Some((db, sound)) if sound == m.sound => db + 0.5 * step,
            _ => step,
        };
        self.calibration = Some((db.clamp(-15.0, 15.0), m.sound));
    }

    /// The sample's EQ, compression and drive, worked at the strongest hit's level.
    fn shape(&mut self, n: usize) {
        let (low, mid, high) = (
            self.values[p::LOW],
            self.values[p::MID],
            self.values[p::HIGH],
        );
        let comp = self.values[p::COMP] / 100.0;
        let drive = self.values[p::DRIVE] / 100.0;
        if (low, mid, high, comp, drive) == (0.0, 0.0, 0.0, 0.0, 0.0) {
            return;
        }
        let kind = self.kind();
        let key = [low.to_bits(), mid.to_bits(), high.to_bits(), kind as u32];
        if self.eq_for != Some(key) {
            self.eq_for = Some(key);
            let rate = f64::from(self.rate);
            let middle = if kind == 0 { 400.0 } else { 900.0 };
            self.eq_coefficients = [
                Coefficients::design(FilterKind::LowShelf, 90.0, 0.7, f64::from(low), rate),
                Coefficients::design(FilterKind::Peak, middle, 1.0, f64::from(mid), rate),
                Coefficients::design(FilterKind::HighShelf, 5000.0, 0.7, f64::from(high), rate),
            ];
        }
        let reference = self.reference.max(1e-6);
        let push = 1.0 + 6.0 * drive;
        let makeup = 1.0 / push.tanh().max(0.2) / (1.0 + drive);
        let [a, b] = &mut self.work;
        for i in 0..n {
            let mut x = f64::from(a[i] / reference);
            for (f, c) in self.eq.iter_mut().zip(&self.eq_coefficients) {
                x = f.process(c, x);
            }
            a[i] = x as f32;
            b[i] = a[i];
        }
        if comp > 0.0 {
            self.comp.process(&mut a[..n], &mut b[..n]);
        }
        for v in &mut a[..n] {
            if drive > 0.0 {
                *v = (*v * push).tanh() * makeup;
            }
            *v *= reference;
        }
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
        self.work = [vec![0.0; max_frames], vec![0.0; max_frames]];
        self.room.prepare(sample_rate, max_frames);
        self.comp.prepare(sample_rate, max_frames);
        self.eq_for = None;
        self.filtered_for = None;
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
        if index == p::COMP {
            // From a light hold to a squash that brings the body up, at about the same level.
            let amount = value / 100.0;
            let threshold = -6.0 - 24.0 * amount;
            let ratio = 1.5 + 4.5 * amount;
            if let Some(info) = crate::info("compressor") {
                for (name, v) in [
                    ("threshold", threshold),
                    ("ratio", ratio),
                    ("makeup", 0.6 * (-threshold - 6.0) * (1.0 - 1.0 / ratio)),
                ] {
                    if let Some(i) = info.params.iter().position(|q| q.id == name) {
                        self.comp.set_param(i, v);
                    }
                }
            }
        }
    }

    fn latency(&self) -> u32 {
        self.lookahead as u32
    }

    fn midi(&mut self, event: &MidiEvent) {
        if let Some((_, velocity)) = event.as_note_on() {
            let strength = f32::from(velocity) / 127.0;
            self.play(strength, (0.25 * strength, 480), 0.0);
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
        if self.bus[0].len() < n || self.work[0].len() < n {
            for b in self.bus.iter_mut().chain(&mut self.work) {
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
                    let span = (t + 1 - attack.min(t)) as usize;
                    let energy = ((attack..=t).map(|k| signed(k) * signed(k)).sum::<f32>()
                        / span as f32)
                        .sqrt();
                    self.strongest = self.strongest.max(hit.raw);
                    let relative = hit.raw / self.strongest.max(1e-9);
                    let body = (BODY_MS / 1000.0 * rate) as u64;
                    self.measure = Some(Measure {
                        until: attack + body,
                        track: energy * energy * span as f32,
                        count: span as u32,
                        sample: 0.0,
                        sound: (usize::MAX, 0),
                    });
                    // Heard where the track's attack comes out of the delay.
                    let wait = (attack + self.lookahead as u64) as f64 - t as f64;
                    self.play(relative, (energy * polarity, span), wait);
                }
            } else if t >= self.armed && x > threshold && self.fast > before * rise {
                self.pending = Some(Pending {
                    onset: t,
                    until: t + window,
                    detected: self.fast,
                    raw: mono.abs(),
                });
                // A new hit over the last one's body: that body is not the drum alone.
                self.measure = None;
                self.armed = t + gap;
            }
            if let Some(mut m) = self.measure {
                if t < m.until {
                    m.track += mono * mono;
                    m.count += 1;
                    self.measure = Some(m);
                } else {
                    self.measure = None;
                    self.calibrate(&m);
                }
            }
            let (close, ambience) = self.voices();
            self.work[0][i] = close + (ambience - close) * room;
            left[i] = dl * dry;
            right[i] = dr * dry;
            self.now += 1;
        }
        self.shape(n);
        for i in 0..n {
            let sample = self.work[0][i];
            left[i] += sample * wet;
            right[i] += sample * wet;
            self.bus[0][i] = sample * send;
            self.bus[1][i] = sample * send;
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

    fn load(&mut self, slot: &str, file: FileData) -> Result<String, String> {
        let kind = SLOTS
            .iter()
            .position(|s| s.id == slot)
            .ok_or_else(|| format!("no slot {slot}"))?;
        let FileData::Audio { channels, rate } = file else {
            return Err("a drum plays an audio file".into());
        };
        let sound = own(&channels, rate)?;
        let seconds = sound.close.len() as f32 / sound.rate;
        self.custom[kind] = Some(sound);
        Ok(format!("{seconds:.2} s"))
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
        self.eq = [Biquad::default(); 3];
        self.measure = None;
        self.room.reset();
        self.comp.reset();
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

/// The loud attacks of a recording, at least 100 ms apart, at most 32.
fn find_hits(samples: &[f32], rate: f32) -> Vec<usize> {
    let block = ((rate / 1000.0) as usize).max(1);
    let envelope: Vec<f32> = samples
        .chunks(block)
        .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
        .collect();
    let top = envelope.iter().fold(0.0f32, |m, v| m.max(*v));
    let mut hits = Vec::new();
    let mut last: Option<usize> = None;
    for k in 1..envelope.len() {
        let rising = envelope[k] > 2.0 * envelope[k.saturating_sub(4)].max(top * 0.01);
        if envelope[k] > top * 0.25 && rising && last.is_none_or(|l| k - l > 100) {
            hits.push(k.saturating_sub(1) * block);
            last = Some(k);
            if hits.len() == 32 {
                break;
            }
        }
    }
    hits
}

/// Whether a recording is a kick (0) or a snare (1), by the low end's share of its hits'
/// first 60 ms; `None` when it is not clear.
pub fn guess_kind(samples: &[f32], rate: u32) -> Option<usize> {
    let rate_f = rate as f32;
    let length = (0.06 * rate_f) as usize;
    let low = Coefficients::design(
        FilterKind::LowPass,
        120.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
        f64::from(rate),
    );
    let mut shares: Vec<f32> = find_hits(samples, rate_f)
        .into_iter()
        .filter(|&h| h + length <= samples.len())
        .map(|h| {
            let mut filters = [Biquad::default(); 2];
            let (mut lows, mut all) = (0.0f32, 0.0f32);
            for &x in &samples[h..h + length] {
                let mut y = f64::from(x);
                for f in &mut filters {
                    y = f.process(&low, y);
                }
                lows += (y * y) as f32;
                all += x * x;
            }
            lows / all.max(1e-12)
        })
        .collect();
    if shares.len() < 2 {
        return None;
    }
    shares.sort_by(f32::total_cmp);
    let median = shares[shares.len() / 2];
    if median > 0.2 {
        Some(0)
    } else if median < 0.1 {
        Some(1)
    } else {
        None
    }
}

/// The pitch a drum rings at once its attack has passed: the lowest clear resonance of its
/// hits between 30 and 150 Hz for a kick, 120 and 400 Hz for a snare.
pub fn fundamental(samples: &[f32], rate: u32, kind: usize) -> Option<f32> {
    use realfft::RealFftPlanner;
    const SIZE: usize = 8192;
    let rate_f = rate as f32;
    let skip = (0.015 * rate_f) as usize;
    let window: Vec<f32> = (0..SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let mut frame = vec![0.0f32; SIZE];
    let mut spectrum = fft.make_output_vec();
    let mut sum = vec![0.0f32; spectrum.len()];
    let mut count = 0;
    for hit in find_hits(samples, rate_f).into_iter().take(16) {
        let from = hit + skip;
        if from >= samples.len() {
            continue;
        }
        for (i, f) in frame.iter_mut().enumerate() {
            *f = samples.get(from + i).copied().unwrap_or(0.0) * window[i];
        }
        let _ = fft.process(&mut frame, &mut spectrum);
        let total: f32 = spectrum.iter().map(|c| c.norm()).sum::<f32>().max(1e-12);
        for (s, c) in sum.iter_mut().zip(&spectrum) {
            *s += c.norm() / total;
        }
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let (low, high) = if kind == 0 {
        (30.0, 150.0)
    } else {
        (120.0, 400.0)
    };
    let bin = |hz: f32| (hz / rate_f * SIZE as f32).ceil() as usize;
    let (from, to) = (bin(low).max(1), bin(high).min(sum.len() - 2));
    let strongest = (from..=to).map(|k| sum[k]).fold(0.0f32, f32::max);
    // The lowest clear resonance: the fundamental rather than a stronger overtone.
    let peak = (from..=to)
        .find(|&k| sum[k] >= 0.35 * strongest && sum[k] >= sum[k - 1] && sum[k] >= sum[k + 1])?;
    // Between the bins, from the curve through the peak and its neighbours.
    let (a, b, c) = (
        sum[peak - 1].max(1e-12).ln(),
        sum[peak].max(1e-12).ln(),
        sum[peak + 1].max(1e-12).ln(),
    );
    let offset = 0.5 * (a - c) / (a - 2.0 * b + c).min(-1e-9);
    Some((peak as f32 + offset.clamp(-0.5, 0.5)) * rate_f / SIZE as f32)
}

/// The pitch of a built-in sound's hardest hit.
pub fn sound_fundamental(kind: usize, sound: usize) -> Option<f32> {
    let s = sounds(kind.min(1)).get(sound)?;
    let (start, length, _) = s.hit(LAYERS - 1, 0);
    fundamental(&s.close[start..start + length], RATE as u32, kind)
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
        for (kind, count) in BUILT_IN.into_iter().enumerate() {
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
    fn the_sample_matches_the_track_level() {
        // The track is a recorded kick; the trigger plays the same one alone.
        let kick = &sounds(0)[0];
        let (start, length, _) = kick.hit(LAYERS - 1, 0);
        let mut input = vec![0.0f32; 48_000];
        for k in 0..length.min(40_000) {
            input[2_400 + k] = kick.close[start + k] * 0.5;
        }
        let mut t = trigger(&[(p::AMOUNT, 100.0), (p::ROOM, 0.0)]);
        let out = run(&mut t, &input);
        let latency = t.latency() as usize;
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let track = rms(&input[2_400..2_400 + 14_400]);
        let sample = rms(&out[2_400 + latency..2_400 + latency + 14_400]);
        let difference = 20.0 * (sample / track).log10();
        assert!(difference.abs() < 1.5, "{difference:.1} dB");
    }

    #[test]
    fn the_level_follows_a_dense_track_after_a_few_hits() {
        // A compressed, saturated kick: more body than the sample for the same attack.
        let kick = &sounds(0)[2];
        let (start, length, _) = kick.hit(LAYERS - 1, 0);
        let hit: Vec<f32> = (0..length.min(20_000))
            .map(|k| (kick.close[start + k] * 10.0).tanh() * 0.3)
            .collect();
        let mut input = vec![0.0f32; 6 * 24_000];
        for n in 0..6 {
            for (k, v) in hit.iter().enumerate() {
                input[2_400 + n * 24_000 + k] += v;
            }
        }
        let mut t = trigger(&[(p::AMOUNT, 100.0), (p::ROOM, 0.0), (p::KICK, 2.0)]);
        let out = run(&mut t, &input);
        let latency = t.latency() as usize;
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let last = 2_400 + 5 * 24_000;
        let body = (BODY_MS / 1000.0 * 48_000.0) as usize;
        let track = rms(&input[last..last + body]);
        let sample = rms(&out[last + latency..last + latency + body]);
        let difference = 20.0 * (sample / track).log10();
        assert!(difference.abs() < 1.5, "{difference:.1} dB");
    }

    #[test]
    fn eq_compression_and_drive_shape_the_sample_near_its_level() {
        let input = hits(&[(4_800, 0.8), (28_800, 0.8)], 48_000, 55.0);
        let render = |values: &[(usize, f32)]| {
            let mut all = vec![(p::AMOUNT, 100.0), (p::ROOM, 0.0)];
            all.extend_from_slice(values);
            let mut t = trigger(&all);
            run(&mut t, &input)
        };
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let plain = render(&[]);
        for values in [
            &[(p::COMP, 100.0)][..],
            &[(p::DRIVE, 100.0)],
            &[(p::LOW, 6.0), (p::HIGH, -6.0)],
        ] {
            let shaped = render(values);
            let difference = 20.0 * (rms(&shaped) / rms(&plain)).log10();
            assert!(difference.abs() < 4.0, "{values:?}: {difference:.1} dB");
            assert!(shaped.iter().zip(&plain).any(|(a, b)| (a - b).abs() > 1e-3));
        }
    }

    #[test]
    fn kicks_and_snares_are_told_apart_and_tuned() {
        let play = |sound: &Sound, times: usize| {
            let (start, length, _) = sound.hit(LAYERS - 1, 0);
            let mut out = vec![0.0f32; times * 24_000 + length];
            for n in 0..times {
                for k in 0..length {
                    out[n * 24_000 + 1_000 + k] += sound.close[start + k];
                }
            }
            out
        };
        for (kind, count) in BUILT_IN.into_iter().enumerate() {
            for k in 0..count {
                let recording = play(&sounds(kind)[k], 4);
                assert_eq!(guess_kind(&recording, 48_000), Some(kind), "{kind} {k}");
            }
        }
        // A sine kick at 60 Hz reads as 60 Hz.
        let sine: Vec<f32> = (0..4 * 24_000)
            .map(|i| {
                let k = i % 24_000;
                let t = k as f32 / 48_000.0;
                (std::f32::consts::TAU * 60.0 * t).sin() * (-t / 0.2).exp()
            })
            .collect();
        let f = fundamental(&sine, 48_000, 0).unwrap();
        assert!((f - 60.0).abs() < 1.5, "{f}");
        // Kicks ring low, the 24-inch ones lower than the 20-inch; snares above.
        let pitch = |kind, k| sound_fundamental(kind, k).unwrap();
        for k in 0..BUILT_IN[0] {
            assert!(
                (30.0..=70.0).contains(&pitch(0, k)),
                "kick {k}: {}",
                pitch(0, k)
            );
        }
        assert!(pitch(0, 2) < pitch(0, 0) && pitch(0, 3) < pitch(0, 1));
        for k in 0..BUILT_IN[1] {
            assert!(
                (120.0..=400.0).contains(&pitch(1, k)),
                "snare {k}: {}",
                pitch(1, k)
            );
        }
    }

    #[test]
    fn a_file_of_ones_own_plays_once_loaded() {
        let mut hit = vec![0.0f32; 4_800];
        hit.extend((0..9_600).map(|k| {
            let t = k as f32 / 44_100.0;
            0.4 * (std::f32::consts::TAU * 70.0 * t).sin() * (-t / 0.1).exp()
        }));
        let mut t = Trigger::default();
        assert!(
            t.load(
                "kick_file",
                FileData::Audio {
                    channels: vec![hit],
                    rate: 44_100
                }
            )
            .is_ok()
        );
        t.set_param(p::KICK, BUILT_IN[0] as f32);
        t.set_param(p::AMOUNT, 100.0);
        t.set_param(p::ROOM, 0.0);
        t.prepare(48_000, 512);
        let own = t.custom[0].as_ref().unwrap();
        assert!(own.hits[0].2 < 100, "the silence before it is gone");
        let input = hits(&[(4_800, 0.8)], 48_000, 55.0);
        let out = run(&mut t, &input);
        assert_eq!(t.meters[2] as usize, 1);
        assert!(out.iter().any(|v| v.abs() > 0.1), "it is heard");
        // Without its file, the last choice stays silent.
        let mut empty = trigger(&[(p::KICK, BUILT_IN[0] as f32), (p::AMOUNT, 100.0)]);
        let out = run(&mut empty, &input);
        assert!(out.iter().all(|v| v.abs() < 1e-6));
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
