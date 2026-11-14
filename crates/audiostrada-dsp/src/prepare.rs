//! Preparing a track without a learned model: which instrument it is, then rules that clean it
//! (filters, resonances, plosives, sibilance, gate) or bring it out (the moves an engineer
//! makes on that instrument, compression, level riding), given as settings of the channel
//! strip, of a Strada-EQ and of a Strada-DeEss.
//!
//! The recipes come from mixing practice (no measured profiles were found): each move looks
//! for its place in the track's own spectrum and takes its amount from what it finds there.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::report::{self, Report};
use crate::time_coefficient;

pub mod mix;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Instrument {
    VocalMale,
    VocalFemale,
    Kick,
    Snare,
    Toms,
    HiHat,
    Overheads,
    Drums,
    Bass,
    ElectricGuitar,
    AcousticGuitar,
    Keys,
    Synth,
    Strings,
    Brass,
    Other,
    BackingVocals,
    HandDrums,
    Cajon,
    Shaker,
    Tambourine,
    Cowbell,
    Claps,
    Whistling,
}

use Instrument::*;

/// Above this fundamental (Hz, the median through the track), a voice is taken for a woman's.
const FEMALE_FROM: f32 = 165.0;
/// Above this one, nobody sings: someone whistles.
const WHISTLE_FROM: f32 = 700.0;

impl Instrument {
    /// In the order of their categories.
    pub const ALL: [Instrument; 24] = [
        VocalMale,
        VocalFemale,
        BackingVocals,
        Whistling,
        Kick,
        Snare,
        Toms,
        HiHat,
        Overheads,
        Drums,
        HandDrums,
        Cajon,
        Shaker,
        Tambourine,
        Cowbell,
        Claps,
        Bass,
        ElectricGuitar,
        AcousticGuitar,
        Keys,
        Synth,
        Strings,
        Brass,
        Other,
    ];

    /// The family it is listed under, in English.
    pub fn category(self) -> &'static str {
        match self {
            VocalMale | VocalFemale | BackingVocals | Whistling => "Voices",
            Kick | Snare | Toms | HiHat | Overheads | Drums => "Drums",
            HandDrums | Cajon | Shaker | Tambourine | Cowbell | Claps => "Percussion",
            Bass => "Bass",
            ElectricGuitar | AcousticGuitar => "Guitars",
            Keys | Synth => "Keys and synths",
            Strings | Brass => "Strings and winds",
            Other => "Other",
        }
    }

    /// Stable id, stored in projects.
    pub fn id(self) -> &'static str {
        match self {
            VocalMale => "vocal_male",
            VocalFemale => "vocal_female",
            Kick => "kick",
            Snare => "snare",
            Toms => "toms",
            HiHat => "hihat",
            Overheads => "overheads",
            Drums => "drums",
            Bass => "bass",
            ElectricGuitar => "electric_guitar",
            AcousticGuitar => "acoustic_guitar",
            Keys => "keys",
            Synth => "synth",
            Strings => "strings",
            Brass => "brass",
            Other => "other",
            BackingVocals => "backing_vocals",
            HandDrums => "hand_drums",
            Cajon => "cajon",
            Shaker => "shaker",
            Tambourine => "tambourine",
            Cowbell => "cowbell",
            Claps => "claps",
            Whistling => "whistling",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|i| i.id() == id)
    }

    /// English name as it ends "The instrument is", translated by the interface.
    pub fn name(self) -> &'static str {
        match self {
            VocalMale => "A male voice",
            VocalFemale => "A female voice",
            Kick => "A kick drum",
            Snare => "A snare drum",
            Toms => "A set of toms",
            HiHat => "A hi-hat",
            Overheads => "The overheads",
            Drums => "A drum kit",
            Bass => "A bass",
            ElectricGuitar => "An electric guitar",
            AcousticGuitar => "An acoustic guitar",
            Keys => "A piano or keys",
            Synth => "A synth",
            Strings => "A string section",
            Brass => "Brass or winds",
            Other => "Something else",
            BackingVocals => "Backing vocals",
            HandDrums => "Congas, bongos or djembe",
            Cajon => "A cajón",
            Shaker => "A shaker or maracas",
            Tambourine => "A tambourine",
            Cowbell => "A cowbell or woodblock",
            Claps => "Claps or snaps",
            Whistling => "Whistling",
        }
    }

    /// A track's name says it carries the song: "Lead vox", "Main voice", "Chant principal".
    pub fn leads(name: &str) -> bool {
        let lower = name.to_lowercase();
        lower.split(|c: char| !c.is_alphanumeric()).any(|w| {
            [
                "lead",
                "main",
                "lv",
                "principal",
                "principale",
                "solo",
                "soliste",
            ]
            .contains(&w)
        })
    }

    pub fn is_vocal(self) -> bool {
        matches!(self, VocalMale | VocalFemale | BackingVocals)
    }

    /// The instrument a track's name says, in English or French: "Lead vox", "02-KICK in".
    /// `pitch` (the track's median fundamental) tells a man's voice from a woman's when the
    /// name does not.
    pub fn from_name(name: &str, pitch: Option<f32>) -> Option<Self> {
        let lower = name.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
        let phrase = |list: &[&str]| list.iter().any(|p| lower.contains(p));
        if phrase(&["bass drum", "grosse caisse"]) || has(&["kick", "kik", "bd", "kck"]) {
            return Some(Kick);
        }
        if phrase(&["caisse claire"]) || has(&["snare", "snr", "sn", "sd"]) {
            return Some(Snare);
        }
        if has(&["tom", "toms", "ft", "floor", "rack"]) {
            return Some(Toms);
        }
        if phrase(&["hi-hat", "hi hat", "charley", "charleston"])
            || has(&["hh", "hihat", "hat", "hats"])
        {
            return Some(HiHat);
        }
        if has(&[
            "conga", "congas", "bongo", "bongos", "djembe", "djembé", "tumba", "quinto",
        ]) {
            return Some(HandDrums);
        }
        if has(&["cajon", "cajón"]) {
            return Some(Cajon);
        }
        if has(&["shaker", "shakers", "shk", "maracas", "cabasa", "egg"]) {
            return Some(Shaker);
        }
        if has(&["tamb", "tambourine", "tambourin", "tambo"]) {
            return Some(Tambourine);
        }
        if has(&[
            "cowbell",
            "woodblock",
            "block",
            "clave",
            "claves",
            "agogo",
            "cloche",
        ]) {
            return Some(Cowbell);
        }
        if has(&[
            "clap",
            "claps",
            "clapping",
            "handclap",
            "handclaps",
            "snap",
            "snaps",
        ]) {
            return Some(Claps);
        }
        if has(&[
            "oh",
            "overhead",
            "overheads",
            "cymbal",
            "cymbals",
            "cymbales",
            "room",
            "ride",
            "crash",
        ]) {
            return Some(Overheads);
        }
        if has(&[
            "drum",
            "drums",
            "kit",
            "batterie",
            "perc",
            "percussion",
            "percs",
        ]) {
            return Some(Drums);
        }
        if has(&[
            "whistle",
            "whistling",
            "whistles",
            "siffle",
            "sifflet",
            "sifflement",
            "siffleur",
            "sifflets",
        ]) {
            return Some(Whistling);
        }
        // Plucked strings that sit where an acoustic guitar does.
        if has(&[
            "cuatro",
            "quatro",
            "quattro",
            "ukulele",
            "uke",
            "charango",
            "tres",
            "mandolin",
            "mandoline",
            "mandolina",
            "bouzouki",
            "banjo",
            "oud",
            "sitar",
            "guitalele",
            "requinto",
            "bandurria",
            "dobro",
        ]) {
            return Some(AcousticGuitar);
        }
        let guitar = has(&[
            "gtr", "guitar", "guitars", "guit", "guitare", "guitares", "egtr",
        ]);
        if has(&["agtr", "nylon"])
            || guitar && has(&["ac", "acou", "acoustic", "acoustique", "folk"])
        {
            return Some(AcousticGuitar);
        }
        if guitar {
            return Some(ElectricGuitar);
        }
        if has(&["bass", "basse", "bss", "contrebasse", "upright"]) {
            return Some(Bass);
        }
        if has(&[
            "piano", "pno", "keys", "key", "rhodes", "wurli", "organ", "orgue", "clav", "epiano",
            "clavier", "claviers",
        ]) {
            return Some(Keys);
        }
        if has(&["synth", "synths", "pad", "pads", "arp", "pluck"]) {
            return Some(Synth);
        }
        if has(&[
            "strings",
            "string",
            "cordes",
            "violin",
            "violon",
            "violins",
            "viola",
            "cello",
            "violoncelle",
            "quartet",
        ]) {
            return Some(Strings);
        }
        if has(&[
            "brass",
            "cuivres",
            "trumpet",
            "trompette",
            "tpt",
            "trombone",
            "tbn",
            "sax",
            "saxophone",
            "horn",
            "horns",
            "flute",
            "flûte",
            "clarinet",
            "clarinette",
        ]) {
            return Some(Brass);
        }
        if has(&[
            "vox",
            "vocal",
            "vocals",
            "voc",
            "voice",
            "voix",
            "chant",
            "lead",
            "bv",
            "bvs",
            "choir",
            "choeur",
            "chœur",
            "choeurs",
            "chœurs",
            "backing",
            "sing",
            "singer",
            "bgv",
            "bgvs",
            "harmony",
            "harmonies",
            "harmo",
        ]) {
            if has(&[
                "bv",
                "bvs",
                "bgv",
                "bgvs",
                "choir",
                "choeur",
                "chœur",
                "choeurs",
                "chœurs",
                "backing",
                "backs",
                "harmony",
                "harmonies",
                "harmo",
            ]) {
                return Some(BackingVocals);
            }
            if has(&[
                "female", "fem", "femme", "woman", "girl", "soprano", "mezzo",
            ]) {
                return Some(VocalFemale);
            }
            if has(&[
                "male", "homme", "man", "guy", "tenor", "ténor", "baritone", "baryton",
            ]) {
                return Some(VocalMale);
            }
            return Some(Self::voice(pitch));
        }
        None
    }

    /// A voice, a man's or a woman's by its pitch, or whistling.
    pub fn voice(pitch: Option<f32>) -> Self {
        if pitch.is_some_and(|f| f >= WHISTLE_FROM) {
            Whistling
        } else if pitch.is_some_and(|f| f >= FEMALE_FROM) {
            VocalFemale
        } else {
            VocalMale
        }
    }

    /// What the sound tells beyond a name: a guitar with highs no amplifier gives is an
    /// acoustic one; a voice that never sings below a whistle's pitch whistles.
    pub fn refine(self, analysis: &Analysis) -> Self {
        match self {
            ElectricGuitar if region_share(&analysis.report.third_octaves, REGIONS[3]) > -24.0 => {
                AcousticGuitar
            }
            VocalMale | VocalFemale | BackingVocals
                if analysis.pitch.is_some_and(|f| f >= WHISTLE_FROM) =>
            {
                Whistling
            }
            other => other,
        }
    }

    /// A rough guess from the sound alone: percussive or held, low or not.
    pub fn from_sound(analysis: &Analysis) -> Self {
        let r = &analysis.report;
        let loudest = (0..r.bands.len())
            .max_by(|&a, &b| r.bands[a].total_cmp(&r.bands[b]))
            .unwrap_or(0);
        let percussive = r.onsets_per_second > 1.5 && r.crest > 16.0;
        match (percussive, loudest) {
            (true, 0 | 1) => Kick,
            (true, 6) if r.bands[1] < -30.0 => HiHat,
            (true, 6) => Overheads,
            (true, _) => Drums,
            (false, 0 | 1) => Bass,
            _ => Other,
        }
    }
}

/// How far the rules go, from 0 (nothing) to 1 (all the way); 0.5 is the usual dose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strength(pub f32);

impl Default for Strength {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl Strength {
    pub const GENTLE: Strength = Strength(0.25);
    pub const NORMAL: Strength = Strength(0.5);
    pub const BOLD: Strength = Strength(0.75);

    /// A word for the dose.
    pub fn name(self) -> &'static str {
        match self.0 {
            x if x < 0.005 => "Off",
            x if x < 0.12 => "Barely",
            x if x < 0.38 => "Gentle",
            x if x < 0.63 => "Normal",
            x if x < 0.88 => "Bold",
            _ => "All the way",
        }
    }

    /// 0 at nothing, 0.5 gentle, 1 normal, 1.35 bold, 1.7 all the way.
    fn factor(self) -> f32 {
        let v = self.0.clamp(0.0, 1.0);
        if v < 0.5 { 2.0 * v } else { 0.3 + 1.4 * v }
    }

    /// Asks for nothing.
    pub fn off(self) -> bool {
        self.0 < 0.005
    }
}

/// What the rules need to know about a track, measured once.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub report: Report,
    /// The narrow peaks, with their level through the audio.
    pub resonances: Vec<Resonance>,
    /// Share of the time the track is silent.
    pub silence: f32,
    /// Level between the notes and of the loud moments (10 ms RMS, dBFS), when it has both.
    pub floor: Option<(f32, f32)>,
    /// Peak level of the loud moments, dBFS: what a compressor sees.
    pub peaks: f32,
    /// Median fundamental, Hz, when the track is pitched.
    pub pitch: Option<f32>,
    pub sibilance: Option<Sibilance>,
    /// Level of the lows (dBFS) above which a low burst is a plosive, when it has some.
    pub plosives: Option<f32>,
    /// Level every [`RIDE_STEP`] seconds (dBFS), for the riding.
    pub levels: Vec<f32>,
    /// [`EXCERPT`] seconds taken all along where the track plays, in mono: what the level kept
    /// is measured on.
    pub excerpt: std::sync::Arc<Vec<f32>>,
    pub rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resonance {
    pub freq: f32,
    /// How far it stands above its octave, dB.
    pub above: f32,
    /// Its level as a Strada-EQ band hears it: usual (median), dBFS.
    pub usual: f32,
    /// Spread of that level (10th to 90th percentile): small when it is always there.
    pub spread: f32,
}

/// The S and T sounds: where they are, how loud against the whole, how often.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sibilance {
    pub freq: f32,
    /// Level of the highs against the whole during them, dB.
    pub level: f32,
    /// Share of the playing time.
    pub share: f32,
}

/// Q of the bands cutting a resonance.
pub const RESONANCE_Q: f32 = 5.0;
/// Corner of the band holding the plosives down.
pub const PLOSIVE_HZ: f32 = 120.0;
/// Seconds of the excerpt the level is measured on.
pub const EXCERPT: f64 = 20.0;
/// Seconds between two points of the riding.
pub const RIDE_STEP: f64 = 0.2;

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn percentile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return -120.0;
    }
    sorted[((sorted.len() - 1) as f32 * q).round() as usize]
}

fn sorted(mut values: Vec<f32>) -> Vec<f32> {
    values.sort_by(f32::total_cmp);
    values
}

/// RMS level (dBFS) of `x` over windows of `window` samples, every `hop`.
fn window_levels(x: &[f32], window: usize, hop: usize) -> Vec<f32> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + window <= x.len() {
        let sum: f32 = x[at..at + window].iter().map(|m| m * m).sum();
        out.push(db((sum / window as f32).sqrt()));
        at += hop;
    }
    out
}

/// The level a dynamic band of Strada-EQ detects through `filter`, every 10 ms.
fn detector_levels(x: &[f32], rate: u32, filter: Coefficients) -> Vec<f32> {
    let mut state = Biquad::default();
    let (attack, release) = (
        time_coefficient(5.0, rate as f32),
        time_coefficient(120.0, rate as f32),
    );
    let step = (rate / 100).max(1) as usize;
    let mut envelope = 0.0f32;
    let mut out = Vec::with_capacity(x.len() / step);
    for (i, &m) in x.iter().enumerate() {
        let level = state.process(&filter, f64::from(m)).abs() as f32;
        let k = if level > envelope { attack } else { release };
        envelope = level + k * (envelope - level);
        if (i + 1) % step == 0 {
            out.push(db(envelope));
        }
    }
    out
}

/// `x` through a fourth-order high-pass at `freq`.
fn high_passed(x: &[f32], rate: u32, freq: f64) -> Vec<f32> {
    let c = Coefficients::design(FilterKind::HighPass, freq, 0.707, 0.0, f64::from(rate));
    let (mut a, mut b) = (Biquad::default(), Biquad::default());
    x.iter()
        .map(|&m| b.process(&c, a.process(&c, f64::from(m))) as f32)
        .collect()
}

/// The median fundamental of the loud moments, when most of them are pitched.
fn pitch(x: &[f32], rate: u32, short: &[f32], loud: f32) -> Option<f32> {
    const WINDOW: usize = 2048;
    const TRIES: usize = 160;
    if x.len() < WINDOW * 2 {
        return None;
    }
    let ten = (rate / 100).max(1) as usize;
    let mut difference = vec![0.0; WINDOW / 2];
    let mut found = Vec::new();
    for k in 0..TRIES {
        let at = k * (x.len() - WINDOW) / TRIES;
        // The track's own sound, not the bleed between its phrases.
        if short.get(at / ten).is_none_or(|v| *v < loud - 12.0) {
            continue;
        }
        let w = &x[at..at + WINDOW];
        // Up to a whistle's pitch, above what the tuner looks for.
        let loud_enough = w.iter().map(|s| s * s).sum::<f32>() / WINDOW as f32 > 1e-7;
        if let Some((f, clarity)) = loud_enough
            .then(|| {
                crate::tuner::yin(
                    w,
                    WINDOW / 2,
                    (rate / 3000) as usize,
                    (rate / 60) as usize,
                    &mut difference,
                )
            })
            .flatten()
            .map(|(lag, clarity)| (rate as f32 / lag, clarity))
            && clarity >= 0.8
            && (60.0..=3000.0).contains(&f)
        {
            found.push(f);
        }
    }
    (found.len() >= 6).then(|| percentile(&sorted(found), 0.5))
}

pub fn analyse(left: &[f32], right: &[f32], rate: u32) -> Analysis {
    let report = report::report(left, right, rate);
    let mid: Vec<f32> = left.iter().zip(right).map(|(l, r)| (l + r) / 2.0).collect();
    let ten = (rate / 100).max(1) as usize;
    let short = window_levels(&mid, ten, ten);
    let silence = if short.is_empty() {
        1.0
    } else {
        short.iter().filter(|v| **v < -60.0).count() as f32 / short.len() as f32
    };
    let heard = sorted(short.iter().copied().filter(|v| *v > -90.0).collect());
    let floor = (heard.len() > 50).then(|| (percentile(&heard, 0.2), percentile(&heard, 0.97)));
    let loud = floor.map_or(-20.0, |f| f.1);
    let playing = |i: usize| short.get(i).is_some_and(|v| *v > loud - 30.0);
    let peaks = percentile(
        &sorted(
            mid.chunks(ten)
                .map(|c| db(c.iter().fold(0.0f32, |m, s| m.max(s.abs()))))
                .collect(),
        ),
        0.97,
    );

    let resonances = report
        .resonances
        .iter()
        .map(|&(freq, above)| {
            let filter = Coefficients::design(
                FilterKind::BandPass,
                f64::from(freq),
                f64::from(RESONANCE_Q),
                0.0,
                f64::from(rate),
            );
            let levels = sorted(
                detector_levels(&mid, rate, filter)
                    .into_iter()
                    .filter(|v| *v > -70.0)
                    .collect(),
            );
            Resonance {
                freq,
                above,
                usual: percentile(&levels, 0.5),
                spread: percentile(&levels, 0.9) - percentile(&levels, 0.1),
            }
        })
        .collect();

    // Sibilance: the moments when the highs carry the sound.
    let highs = window_levels(&high_passed(&mid, rate, 5000.0), ten, ten);
    let relative: Vec<f32> = highs
        .iter()
        .zip(&short)
        .enumerate()
        .filter(|(i, _)| playing(*i))
        .map(|(_, (h, s))| h - s)
        .collect();
    let hissing = sorted(relative.iter().copied().filter(|v| *v > -8.0).collect());
    let sibilance = (hissing.len() >= 5).then(|| {
        let bump = bumps(&report.curve);
        let freq = report
            .curve
            .iter()
            .zip(&bump)
            .filter(|(c, _)| (5000.0..=9500.0).contains(&c.0))
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(6500.0, |(c, _)| c.0);
        Sibilance {
            freq,
            level: percentile(&hissing, 0.5),
            share: hissing.len() as f32 / relative.len().max(1) as f32,
        }
    });

    // Plosives: short bursts of lows well above the lows' usual level.
    let lows = detector_levels(
        &mid,
        rate,
        Coefficients::design(
            FilterKind::LowPass,
            f64::from(PLOSIVE_HZ),
            0.707,
            0.0,
            f64::from(rate),
        ),
    );
    let usual = percentile(
        &sorted(
            lows.iter()
                .enumerate()
                .filter(|(i, _)| playing(*i))
                .map(|(_, v)| *v)
                .collect(),
        ),
        0.5,
    );
    let (mut bursts, mut last) = (0, None::<usize>);
    for (i, v) in lows.iter().enumerate() {
        if *v > usual + 10.0 && *v > -45.0 {
            if last.is_none_or(|l| i - l > 10) {
                bursts += 1;
            }
            last = Some(i);
        }
    }
    let plosives = (bursts >= 3).then_some(usual + 6.0);

    let step = ((f64::from(rate) * RIDE_STEP) as usize).max(1);
    let levels = window_levels(&mid, 2 * step, step);
    // A sample of where the track plays, taken all along it: what its level is heard on. The
    // loudest stretch alone would call for too much gain after a compressor.
    const PIECE: usize = 10;
    let pieces = (EXCERPT / RIDE_STEP) as usize / PIECE;
    let power: Vec<f32> = levels.iter().map(|v| 10f32.powf(v / 10.0)).collect();
    let blocks: Vec<(usize, f32)> = power
        .as_chunks::<PIECE>()
        .0
        .iter()
        .enumerate()
        .map(|(k, c)| (k, c.iter().sum::<f32>()))
        .collect();
    let top = blocks.iter().map(|b| b.1).fold(0.0f32, f32::max);
    let playing: Vec<usize> = blocks
        .iter()
        .filter(|b| b.1 > top * 0.03)
        .map(|b| b.0)
        .collect();
    let mut excerpt = Vec::new();
    for n in 0..pieces.min(playing.len()) {
        let block = playing[n * playing.len() / pieces.min(playing.len())];
        let from = (block * PIECE * step).min(mid.len());
        let to = (from + PIECE * step).min(mid.len());
        excerpt.extend_from_slice(&mid[from..to]);
    }
    if excerpt.is_empty() {
        let to = ((EXCERPT * f64::from(rate)) as usize).min(mid.len());
        excerpt.extend_from_slice(&mid[..to]);
    }
    let excerpt = std::sync::Arc::new(excerpt);
    Analysis {
        excerpt,
        rate,
        pitch: pitch(&mid, rate, &short, loud),
        report,
        resonances,
        silence,
        floor,
        peaks,
        sibilance,
        plosives,
        levels,
    }
}

/// How far each point of the report's curve stands above its octave: the local bumps.
fn bumps(curve: &[(f32, f32)]) -> Vec<f32> {
    (0..curve.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(12), (i + 13).min(curve.len()));
            curve[i].1 - curve[a..b].iter().map(|c| c.1).sum::<f32>() / (b - a) as f32
        })
        .collect()
}

/// A band of the Strada-EQ: a narrow cut at a resonance, or a low shelf holding plosives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub freq: f32,
    /// Negative dB: the gain of a fixed cut, the range of a dynamic one.
    pub depth: f32,
    /// The threshold of a dynamic cut, dBFS.
    pub threshold: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The channel strip's filters.
    Filters {
        hpf: Option<f32>,
        lpf: Option<f32>,
        steep: bool,
    },
    Resonance(Cut),
    Plosives(Cut),
    /// A Strada-DeEss; `sure` when sibilance was heard on a voice (else only offered).
    DeEss {
        freq: f32,
        threshold: f32,
        range: f32,
        sure: bool,
    },
    Gate {
        threshold: f32,
        range: f32,
        release: f32,
    },
    /// A band of the channel strip's EQ (0 low, 1 low mid, 2 high mid, 3 high).
    Eq {
        band: usize,
        freq: f32,
        gain: f32,
        q: f32,
        bell: bool,
        /// What the move is for: "Weight", "Mud", "Click"...
        purpose: &'static str,
    },
    Compressor {
        ratio: f32,
        attack: f32,
        release: f32,
        threshold: f32,
        /// Gain reduction aimed at on the loud moments, dB.
        reduction: f32,
    },
    /// A Strada-Transient: attack and sustain, %.
    Transient {
        attack: f32,
        sustain: f32,
    },
    /// Gain changes (seconds, dB) on the channel strip's output.
    Ride(Vec<(f64, f32)>),
    /// Long silences: *Strip silence* does better than a gate. A suggestion, never applied.
    StripSilence(f32),
}

impl Step {
    /// Whether it is part of Bring out (else of Clean up).
    pub fn brings_out(&self) -> bool {
        matches!(
            self,
            Step::Eq { .. } | Step::Compressor { .. } | Step::Transient { .. } | Step::Ride(_)
        )
    }

    /// Whether it is proposed ticked.
    pub fn default_on(&self) -> bool {
        match self {
            Step::StripSilence(_) | Step::Ride(_) => false,
            Step::DeEss { sure, .. } => *sure,
            _ => true,
        }
    }

    /// Whether its amount can be dosed.
    pub fn adjustable(&self) -> bool {
        !matches!(self, Step::Filters { .. } | Step::StripSilence(_))
    }

    /// The step at `amount` times what was proposed (0 to 2).
    pub fn scaled(&self, amount: f32) -> Step {
        let deeper = |cut: &Cut| Cut {
            depth: (cut.depth * amount).clamp(-24.0, 0.0),
            ..*cut
        };
        match self {
            Step::Resonance(cut) => Step::Resonance(deeper(cut)),
            Step::Plosives(cut) => Step::Plosives(deeper(cut)),
            &Step::DeEss {
                freq,
                threshold,
                range,
                sure,
            } => Step::DeEss {
                freq,
                threshold,
                range: (range * amount).clamp(0.0, 24.0),
                sure,
            },
            &Step::Gate {
                threshold,
                range,
                release,
            } => Step::Gate {
                threshold,
                range: (range * amount).clamp(-80.0, 0.0),
                release,
            },
            &Step::Eq {
                band,
                freq,
                gain,
                q,
                bell,
                purpose,
            } => Step::Eq {
                band,
                freq,
                gain: ((gain * amount).clamp(-12.0, 12.0) * 2.0).round() / 2.0,
                q,
                bell,
                purpose,
            },
            &Step::Compressor {
                ratio,
                attack,
                release,
                threshold,
                reduction,
            } => {
                // The threshold moves so that the loud moments lose the new reduction.
                let wanted = reduction * amount;
                Step::Compressor {
                    ratio,
                    attack,
                    release,
                    threshold: (threshold + (reduction - wanted) * ratio / (ratio - 1.0))
                        .clamp(-60.0, 0.0),
                    reduction: wanted,
                }
            }
            &Step::Transient { attack, sustain } => Step::Transient {
                attack: (attack * amount).clamp(-100.0, 100.0),
                sustain: (sustain * amount).clamp(-100.0, 100.0),
            },
            Step::Ride(points) => Step::Ride(
                points
                    .iter()
                    .map(|&(t, db)| (t, (db * amount).clamp(-9.0, 9.0)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// The step for a track played `db` louder (its input gain raised): the thresholds follow.
    pub fn shifted(&self, db: f32) -> Step {
        let cut = |c: &Cut| Cut {
            threshold: c.threshold.map(|t| (t + db).clamp(-60.0, 0.0)),
            ..*c
        };
        match self {
            Step::Resonance(c) => Step::Resonance(cut(c)),
            Step::Plosives(c) => Step::Plosives(cut(c)),
            &Step::DeEss {
                freq,
                threshold,
                range,
                sure,
            } => Step::DeEss {
                freq,
                threshold: (threshold + db).clamp(-30.0, -1.0),
                range,
                sure,
            },
            &Step::Gate {
                threshold,
                range,
                release,
            } => Step::Gate {
                threshold: (threshold + db).clamp(-80.0, 0.0),
                range,
                release,
            },
            &Step::Compressor {
                ratio,
                attack,
                release,
                threshold,
                reduction,
            } => Step::Compressor {
                ratio,
                attack,
                release,
                threshold: (threshold + db).clamp(-60.0, 0.0),
                reduction,
            },
            other => other.clone(),
        }
    }

    /// The channel strip's settings it makes.
    pub fn strip_params(&self) -> Vec<(&'static str, f32)> {
        match *self {
            Step::Filters { hpf, lpf, steep } => vec![
                ("filters", 1.0),
                ("hpf", hpf.unwrap_or(20.0)),
                ("lpf", lpf.unwrap_or(20_000.0)),
                ("slope", if steep { 1.0 } else { 0.0 }),
            ],
            Step::Gate {
                threshold,
                range,
                release,
            } => vec![
                ("gate", 1.0),
                ("gate_threshold", threshold),
                ("gate_range", range),
                ("gate_attack", 0.5),
                ("gate_release", release),
            ],
            Step::Eq {
                band,
                freq,
                gain,
                q,
                bell,
                ..
            } => {
                let shape = if bell { 1.0 } else { 0.0 };
                let mut out = vec![("eq", 1.0)];
                out.extend(match band {
                    0 => [("low_freq", freq), ("low_gain", gain), ("low_bell", shape)],
                    1 => [("lmf_freq", freq), ("lmf_gain", gain), ("lmf_q", q)],
                    2 => [("hmf_freq", freq), ("hmf_gain", gain), ("hmf_q", q)],
                    _ => [
                        ("high_freq", freq),
                        ("high_gain", gain),
                        ("high_bell", shape),
                    ],
                });
                out
            }
            Step::Compressor {
                ratio,
                attack,
                release,
                threshold,
                ..
            } => vec![
                ("comp", 1.0),
                ("comp_ratio", ratio),
                ("comp_attack", attack),
                ("comp_release", release),
                ("comp_threshold", threshold),
                ("comp_makeup", 0.0),
                ("comp_mix", 100.0),
            ],
            _ => Vec::new(),
        }
    }
}

/// The channel strip's settings of `steps`.
pub fn strip_settings<'a>(steps: impl Iterator<Item = &'a Step>) -> Vec<(String, f32)> {
    steps
        .flat_map(Step::strip_params)
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
}

/// The effects `steps` add after the channel strip, with their settings: a Strada-EQ for the
/// resonances and the plosives, a Strada-DeEss.
pub fn inserts<'a>(
    steps: impl Iterator<Item = &'a Step>,
) -> Vec<(&'static str, Vec<(String, f32)>)> {
    let (mut eq, mut deess, mut transient) = (Vec::new(), None, None);
    let mut n = 0;
    for step in steps {
        let (cut, kind, q) = match step {
            &Step::Transient { attack, sustain } => {
                transient = Some(vec![
                    ("attack".to_owned(), attack),
                    ("sustain".to_owned(), sustain),
                    ("speed".to_owned(), 10.0),
                    ("clip".to_owned(), 1.0),
                    ("sc_hpf".to_owned(), 60.0),
                ]);
                continue;
            }
            Step::Resonance(cut) => (cut, 0.0, RESONANCE_Q),
            // A low shelf.
            Step::Plosives(cut) => (cut, 1.0, 0.7),
            Step::DeEss {
                freq,
                threshold,
                range,
                ..
            } => {
                deess = Some(vec![
                    ("frequency".to_owned(), *freq),
                    ("threshold".to_owned(), *threshold),
                    ("range".to_owned(), *range),
                    ("mode".to_owned(), 0.0),
                    ("detector".to_owned(), 0.0),
                    ("relative".to_owned(), 1.0),
                    ("lookahead".to_owned(), 2.0),
                ]);
                continue;
            }
            _ => continue,
        };
        n += 1;
        let p = |name: &str| format!("b{n}_{name}");
        eq.extend([
            (p("used"), 1.0),
            (p("type"), kind),
            (p("freq"), cut.freq),
            (p("q"), q),
        ]);
        match cut.threshold {
            Some(threshold) => eq.extend([
                (p("gain"), 0.0),
                (p("dyn"), 1.0),
                (p("thresh"), threshold),
                (p("range"), cut.depth),
            ]),
            None => eq.extend([(p("gain"), cut.depth), (p("dyn"), 0.0)]),
        }
    }
    let mut out = Vec::new();
    if !eq.is_empty() {
        out.push(("eq", eq));
    }
    if let Some(deess) = deess {
        out.push(("deesser", deess));
    }
    if let Some(transient) = transient {
        out.push(("transient", transient));
    }
    out
}

/// `excerpt` through the input gain `trim_db`, then the channel strip and the effects `steps`
/// set: its loudness (LUFS).
fn through(excerpt: &[f32], rate: u32, trim_db: f32, steps: &[Step]) -> f32 {
    use crate::loudness::LevelMeter;
    let trim = crate::db_to_gain(trim_db);
    let trimmed: Vec<f32> = excerpt.iter().map(|x| x * trim).collect();
    let (mut left, mut right) = (trimmed.clone(), trimmed);
    let strip = strip_settings(steps.iter());
    let inserts = inserts(steps.iter());
    let chain =
        std::iter::once(("channel", &strip[..])).chain(inserts.iter().map(|(e, p)| (*e, &p[..])));
    for (effect, params) in chain {
        let (Some(mut e), Some(info)) = (crate::create(effect), crate::info(effect)) else {
            continue;
        };
        e.prepare(rate, 256);
        e.set_watched(false);
        for (id, value) in params {
            if let Some(i) = info.params.iter().position(|p| p.id == id.as_str()) {
                e.set_param(i, *value);
            }
        }
        for (l, r) in left.chunks_mut(256).zip(right.chunks_mut(256)) {
            e.process(l, r);
        }
    }
    let mut meter = LevelMeter::new(rate);
    for (l, r) in left.iter().zip(&right) {
        meter.push(*l, *r);
    }
    meter.integrated()
}

/// The gain (dB) on the strip's output that keeps the loudness `excerpt` had, once through
/// the input gain `trim_db` and `steps`: Express Mix changes the color of a track, its fader
/// its level.
pub fn level_kept(excerpt: &[f32], rate: u32, trim_db: f32, steps: &[Step]) -> f32 {
    let plain = through(excerpt, rate, trim_db, &[]);
    let loud = through(excerpt, rate, trim_db, steps);
    let heard = |v: f32| v.is_finite() && v > -70.0;
    // The input gain is given back whole; only what the processing changes is capped.
    if heard(plain) && heard(loud) {
        (plain - loud).clamp(-12.0, 12.0) - trim_db
    } else {
        -trim_db
    }
}

/// The regions of the spectrum the tone looks at, as the third octaves they hold (centers, Hz):
/// the lows, the low mids, the presence and the air, in the order of the strip's EQ bands.
pub const REGIONS: [(f32, f32); 4] = [
    (60.0, 130.0),
    (155.0, 410.0),
    (1_950.0, 5_100.0),
    (6_200.0, 16_500.0),
];

/// Share (dB) of a track's energy in `region`, from third octaves given as shares.
pub fn region_share(thirds: &[(f32, f32)], (lo, hi): (f32, f32)) -> f32 {
    let power: f64 = thirds
        .iter()
        .filter(|t| (lo..=hi).contains(&t.0))
        .map(|t| 10f64.powf(f64::from(t.1) / 10.0))
        .sum();
    (10.0 * power.max(1e-12).log10()) as f32
}

/// The third octaves of a track as shares of its energy, once through `steps`.
fn shaped(thirds: &[(f32, f32)], steps: &[Step]) -> Vec<(f32, f32)> {
    let moved: Vec<(f32, f32)> = thirds
        .iter()
        .map(|&(f, s)| (f, s + mix::response_db(steps, f)))
        .collect();
    let total: f64 = moved
        .iter()
        .map(|t| 10f64.powf(f64::from(t.1) / 10.0))
        .sum();
    let total = (10.0 * total.max(1e-12).log10()) as f32;
    moved.into_iter().map(|(f, s)| (f, s - total)).collect()
}

/// The tone an instrument is brought to: for each of [`REGIONS`], its share of the energy in a
/// mix that sounds right (dB; `None`: left as it is), and how far a move may go up and down at
/// the usual dose. Measured on the stems of a mix made by hand, completed from practice.
struct Tone {
    target: [Option<f32>; 4],
    boost: [f32; 4],
    cut: [f32; 4],
    /// Where the presence is lifted, Hz.
    presence: f32,
    /// The weight (dB) given to what carries the low end, whose share of its lows says
    /// nothing: they are almost all it has.
    weight: f32,
}

const fn tone(target: [Option<f32>; 4], boost: [f32; 4], cut: [f32; 4], presence: f32) -> Tone {
    Tone {
        target,
        boost,
        cut,
        presence,
        weight: 0.0,
    }
}

/// Ratio, attack and release (ms), the reduction aimed at (dB), and the loudness range (LU)
/// above which it compresses (0: always).
struct Comp(f32, f32, f32, f32, f32);

/// What each instrument calls for.
struct Rules {
    /// Range of the high-pass, Hz.
    hpf: (f32, f32),
    steep: bool,
    /// Range of the low-pass, for the few instruments where it is safe.
    lpf: Option<(f32, f32)>,
    /// Where resonances are cut.
    zone: (f32, f32),
    /// Gate range (dB) and release (ms), when a gate or an expander helps (bleed between the
    /// hits or the phrases), and where its threshold sits between the floor and the hits.
    gate: Option<(f32, f32, f32)>,
    /// Strada-Transient's attack and sustain (%), on percussive sounds.
    transient: Option<(f32, f32)>,
    comp: Comp,
    tone: Tone,
}

const NONE: [Option<f32>; 4] = [None; 4];

fn rules(i: Instrument) -> Rules {
    let rules = |hpf, steep, zone, gate, comp, tone| Rules {
        hpf,
        steep,
        lpf: None,
        zone,
        gate,
        transient: None,
        comp,
        tone,
    };
    match i {
        VocalMale => rules(
            (40.0, 70.0),
            false,
            (180.0, 8000.0),
            Some((-6.0, 250.0, 0.3)),
            Comp(3.0, 10.0, 100.0, 4.0, 4.0),
            tone(
                [Some(-25.0), Some(-4.0), Some(-11.0), Some(-23.0)],
                [0.0, 1.0, 3.0, 2.0],
                [3.0, 3.0, 3.0, 3.0],
                3500.0,
            ),
        ),
        VocalFemale => rules(
            (60.0, 90.0),
            false,
            (250.0, 9000.0),
            Some((-6.0, 250.0, 0.3)),
            Comp(3.0, 10.0, 100.0, 4.0, 4.0),
            tone(
                [Some(-35.0), Some(-5.0), Some(-10.0), Some(-21.0)],
                [0.0, 1.5, 3.0, 2.0],
                [3.0, 3.0, 3.0, 3.0],
                3500.0,
            ),
        ),
        BackingVocals => rules(
            (70.0, 110.0),
            false,
            (200.0, 9000.0),
            Some((-12.0, 250.0, 0.35)),
            Comp(3.0, 15.0, 120.0, 3.0, 4.0),
            tone(
                [Some(-28.0), Some(-8.0), Some(-11.0), Some(-23.0)],
                [0.0, 1.0, 1.5, 1.5],
                [3.0, 3.0, 3.0, 3.0],
                3000.0,
            ),
        ),
        Whistling => rules(
            (200.0, 400.0),
            true,
            (800.0, 6000.0),
            Some((-12.0, 300.0, 0.35)),
            Comp(2.0, 20.0, 150.0, 2.0, 6.0),
            tone(NONE, [0.0; 4], [0.0; 4], 3000.0),
        ),
        Kick => Rules {
            lpf: Some((7000.0, 10_000.0)),
            transient: Some((20.0, -10.0)),
            ..rules(
                (20.0, 25.0),
                false,
                (120.0, 1000.0),
                Some((-20.0, 120.0, 0.35)),
                Comp(4.0, 20.0, 80.0, 3.0, 0.0),
                Tone {
                    weight: 2.5,
                    ..tone(
                        [Some(-2.0), Some(-21.0), Some(-33.0), None],
                        [3.0, 0.0, 6.0, 0.0],
                        [2.0, 4.0, 2.0, 0.0],
                        3000.0,
                    )
                },
            )
        },
        Snare => Rules {
            transient: Some((20.0, 0.0)),
            ..rules(
                (40.0, 80.0),
                false,
                (300.0, 1500.0),
                Some((-12.0, 150.0, 0.35)),
                Comp(4.0, 15.0, 60.0, 3.0, 0.0),
                tone(
                    [None, Some(-5.5), Some(-8.5), None],
                    [0.0, 1.0, 4.0, 0.0],
                    [0.0, 3.0, 2.0, 0.0],
                    4000.0,
                ),
            )
        },
        Toms => Rules {
            transient: Some((15.0, -15.0)),
            ..rules(
                (35.0, 60.0),
                false,
                (150.0, 1000.0),
                Some((-15.0, 250.0, 0.35)),
                Comp(3.0, 15.0, 120.0, 3.0, 4.0),
                tone(
                    [Some(-3.0), Some(-6.0), Some(-28.0), None],
                    [2.0, 0.0, 3.0, 0.0],
                    [2.0, 4.0, 2.0, 0.0],
                    3500.0,
                ),
            )
        },
        HiHat => rules(
            (150.0, 300.0),
            true,
            (1000.0, 10_000.0),
            None,
            Comp(2.0, 20.0, 100.0, 2.0, 9.0),
            tone(
                [None, Some(-30.0), Some(-6.0), Some(-5.0)],
                [0.0, 0.0, 1.0, 1.5],
                [0.0, 4.0, 3.0, 2.0],
                4000.0,
            ),
        ),
        Overheads => rules(
            (60.0, 120.0),
            false,
            (300.0, 8000.0),
            None,
            Comp(2.0, 30.0, 150.0, 2.0, 6.0),
            tone(
                [Some(-15.0), Some(-8.0), Some(-10.0), Some(-11.0)],
                [0.0, 0.0, 1.5, 2.0],
                [4.0, 3.0, 3.0, 2.0],
                4000.0,
            ),
        ),
        Drums => rules(
            (20.0, 30.0),
            false,
            (200.0, 6000.0),
            None,
            Comp(2.0, 30.0, 120.0, 3.0, 4.0),
            tone(
                [Some(-5.0), Some(-8.0), Some(-12.0), Some(-16.0)],
                [2.0, 0.0, 2.0, 2.0],
                [2.0, 3.0, 2.0, 2.0],
                3500.0,
            ),
        ),
        Bass => Rules {
            lpf: Some((5000.0, 12_000.0)),
            ..rules(
                (25.0, 35.0),
                false,
                (150.0, 3000.0),
                None,
                Comp(4.0, 25.0, 120.0, 4.0, 0.0),
                Tone {
                    weight: 2.0,
                    ..tone(
                        [Some(-1.0), Some(-9.0), None, None],
                        [3.0, 1.0, 0.0, 0.0],
                        [2.0, 3.0, 0.0, 0.0],
                        1000.0,
                    )
                },
            )
        },
        ElectricGuitar => Rules {
            lpf: Some((8000.0, 14_000.0)),
            ..rules(
                (60.0, 90.0),
                false,
                (150.0, 6000.0),
                Some((-10.0, 150.0, 0.3)),
                Comp(2.5, 25.0, 200.0, 2.5, 5.0),
                tone(
                    [Some(-16.0), Some(-4.0), Some(-10.0), Some(-26.0)],
                    [0.0, 1.0, 2.0, 0.0],
                    [3.0, 3.0, 3.0, 3.0],
                    2500.0,
                ),
            )
        },
        AcousticGuitar => rules(
            (50.0, 80.0),
            false,
            (100.0, 6000.0),
            Some((-8.0, 250.0, 0.3)),
            Comp(3.0, 15.0, 100.0, 3.0, 4.0),
            tone(
                [Some(-16.0), Some(-2.0), Some(-13.5), Some(-20.0)],
                [0.0, 1.0, 2.5, 2.0],
                [2.5, 2.5, 3.0, 3.0],
                3200.0,
            ),
        ),
        Keys => rules(
            (25.0, 45.0),
            false,
            (150.0, 6000.0),
            None,
            Comp(2.0, 20.0, 150.0, 2.5, 6.0),
            tone(
                [Some(-14.0), Some(-4.0), Some(-13.0), Some(-24.0)],
                [1.0, 1.0, 2.0, 1.5],
                [3.0, 3.0, 3.0, 3.0],
                3000.0,
            ),
        ),
        Synth => rules(
            (25.0, 60.0),
            false,
            (150.0, 8000.0),
            None,
            Comp(2.0, 20.0, 150.0, 2.5, 7.0),
            tone(
                [Some(-12.0), Some(-5.0), Some(-12.0), Some(-22.0)],
                [1.0, 1.0, 2.0, 1.5],
                [3.0, 3.0, 3.0, 3.0],
                3000.0,
            ),
        ),
        Strings => rules(
            (35.0, 60.0),
            false,
            (200.0, 8000.0),
            None,
            Comp(2.0, 30.0, 200.0, 2.0, 7.0),
            tone(
                [Some(-18.0), Some(-5.0), Some(-12.0), Some(-22.0)],
                [0.0, 1.0, 1.5, 1.5],
                [3.0, 3.0, 3.0, 3.0],
                3000.0,
            ),
        ),
        Brass => rules(
            (50.0, 90.0),
            false,
            (200.0, 6000.0),
            None,
            Comp(3.0, 10.0, 120.0, 3.0, 5.0),
            tone(
                [Some(-25.0), Some(-8.0), Some(-9.0), Some(-20.0)],
                [0.0, 0.0, 1.5, 1.0],
                [3.0, 3.0, 3.0, 3.0],
                2500.0,
            ),
        ),
        Other => rules(
            (25.0, 40.0),
            false,
            (150.0, 8000.0),
            None,
            Comp(2.0, 20.0, 150.0, 2.0, 6.0),
            tone(NONE, [0.0; 4], [0.0; 4], 3000.0),
        ),
        HandDrums => Rules {
            transient: Some((15.0, 0.0)),
            ..rules(
                (50.0, 80.0),
                false,
                (200.0, 2000.0),
                Some((-12.0, 150.0, 0.35)),
                Comp(3.0, 10.0, 100.0, 3.0, 0.0),
                tone(
                    [Some(-10.0), Some(-4.0), Some(-14.0), Some(-22.0)],
                    [1.5, 0.0, 2.0, 1.5],
                    [3.0, 3.0, 3.0, 3.0],
                    3500.0,
                ),
            )
        },
        Cajon => Rules {
            transient: Some((20.0, -10.0)),
            ..rules(
                (30.0, 45.0),
                false,
                (150.0, 1500.0),
                None,
                Comp(4.0, 15.0, 80.0, 3.0, 0.0),
                Tone {
                    weight: 1.5,
                    ..tone(
                        [Some(-3.0), Some(-8.0), Some(-18.0), None],
                        [2.0, 0.0, 3.0, 0.0],
                        [2.0, 3.0, 2.0, 0.0],
                        3500.0,
                    )
                },
            )
        },
        Shaker | Tambourine => rules(
            (200.0, 400.0),
            true,
            (1500.0, 10_000.0),
            None,
            Comp(2.0, 20.0, 100.0, 2.0, 9.0),
            tone(
                [None, Some(-40.0), Some(-6.0), Some(-4.0)],
                [0.0, 0.0, 0.0, 1.5],
                [0.0, 6.0, 3.0, 2.0],
                4000.0,
            ),
        ),
        Cowbell => rules(
            (150.0, 300.0),
            true,
            (500.0, 5000.0),
            None,
            Comp(2.0, 15.0, 100.0, 2.0, 9.0),
            tone(
                [None, Some(-20.0), Some(-8.0), Some(-20.0)],
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 4.0, 3.0, 3.0],
                3000.0,
            ),
        ),
        Claps => Rules {
            transient: Some((10.0, 0.0)),
            ..rules(
                (100.0, 200.0),
                false,
                (300.0, 6000.0),
                None,
                Comp(3.0, 10.0, 100.0, 3.0, 0.0),
                tone(
                    [None, Some(-12.0), Some(-7.0), Some(-14.0)],
                    [0.0, 1.0, 2.0, 1.5],
                    [0.0, 3.0, 3.0, 3.0],
                    3500.0,
                ),
            )
        },
    }
}

/// The filters: well under the lowest and above the highest energy that counts, within the
/// ranges given; a lighter dose keeps further from the sound.
fn filters(
    a: &Analysis,
    hpf: (f32, f32),
    lpf: Option<(f32, f32)>,
    steep: bool,
    k: f32,
) -> Option<Step> {
    let near = 0.5 + 0.5 * k.min(1.0);
    let curve = &a.report.curve;
    let top = curve
        .iter()
        .filter(|p| (20.0..=2000.0).contains(&p.0))
        .map(|p| p.1)
        .fold(f32::MIN, f32::max);
    if top == f32::MIN {
        return None;
    }
    let lowest = curve.iter().find(|p| p.1 >= top - 24.0).map(|p| p.0);
    let highest = curve.iter().rev().find(|p| p.1 >= top - 40.0).map(|p| p.0);
    // An octave under the lowest energy that counts: the body stays whole.
    let hpf = lowest
        .map(|f| (f * 0.5).clamp(hpf.0, hpf.1) * near)
        .filter(|f| *f > 21.0);
    let lpf = lpf
        .and_then(|(lo, hi)| highest.map(|f| (f * 1.5).clamp(lo, hi) / near))
        .filter(|f| *f < 19_000.0);
    (hpf.is_some() || lpf.is_some()).then_some(Step::Filters { hpf, lpf, steep })
}

/// The cleaning of a track: filters, the resonances that really stand out, plosives and
/// sibilance, a gate or a gentle expander when there is bleed, and the suggestion to strip
/// long silences.
pub fn clean(a: &Analysis, instrument: Instrument, strength: Strength) -> Vec<Step> {
    if strength.off() {
        return Vec::new();
    }
    let rules = rules(instrument);
    let k = strength.factor();
    let mut steps: Vec<Step> = filters(a, rules.hpf, rules.lpf, rules.steep, k)
        .into_iter()
        .collect();
    let mut cuts = 0;
    for r in &a.resonances {
        if cuts == 3 || !(rules.zone.0..=rules.zone.1).contains(&r.freq) || r.above < 7.0 {
            continue;
        }
        let depth = (-(r.above - 4.0).clamp(1.5, 6.0) * 0.8 * k).max(-8.0);
        let depth = (depth * 2.0).round() / 2.0;
        if depth > -1.5 {
            continue;
        }
        // Always there: a fixed cut; else only while it stands out.
        let threshold = (r.spread > 4.0).then(|| (r.usual + 2.0).clamp(-60.0, 0.0));
        steps.push(Step::Resonance(Cut {
            freq: r.freq,
            depth,
            threshold,
        }));
        cuts += 1;
    }
    if instrument.is_vocal()
        && let Some(threshold) = a.plosives
        && k >= 0.125
    {
        steps.push(Step::Plosives(Cut {
            freq: PLOSIVE_HZ,
            depth: (-8.0 * k).max(-12.0),
            threshold: Some(threshold.clamp(-60.0, 0.0)),
        }));
    }
    if let Some(s) = a.sibilance {
        let sure = instrument.is_vocal() && (0.005..0.4).contains(&s.share);
        if (sure || matches!(instrument, Overheads | HiHat)) && k >= 0.125 {
            steps.push(Step::DeEss {
                freq: (s.freq * 0.85).clamp(4000.0, 9000.0),
                threshold: (s.level - 4.0 * k).clamp(-30.0, -1.0),
                range: (6.0 * k).min(12.0),
                sure,
            });
        }
    }
    // Something between the hits or the phrases (bleed, hiss), well under them.
    if let (Some((range, release, at)), Some((floor, hits))) = (rules.gate, a.floor)
        && hits - floor > 12.0
        && floor > -80.0
        && range * k.min(1.0) <= -3.0
    {
        steps.push(Step::Gate {
            threshold: (floor + (hits - floor) * at).clamp(-80.0, 0.0),
            // A lighter dose closes less.
            range: (range * k.min(1.0)).round(),
            release,
        });
    }
    if a.silence > 0.25 && a.report.seconds > 10.0 {
        steps.push(Step::StripSilence(a.silence));
    }
    steps
}

/// The EQ bringing a track to its instrument's tone: in each region, part of the way to the
/// target, nothing where it is already close, within the instrument's limits; the style then
/// leans the tone its way. `before` is the cleaning kept, heard first.
fn tone_moves(
    a: &Analysis,
    instrument: Instrument,
    k: f32,
    flavor: Flavor,
    before: &[Step],
) -> Vec<Step> {
    const DEAD: f32 = 1.5;
    const PART: f32 = 0.6;
    let t = rules(instrument).tone;
    let lean = flavor.lean(instrument);
    let dose = k * flavor.get(|t| t.moves);
    let curve = &a.report.curve;
    let peak = |lo: f32, hi: f32| {
        let bump = bumps(curve);
        curve
            .iter()
            .zip(&bump)
            .filter(|(c, _)| (lo..=hi).contains(&c.0))
            .max_by(|x, y| x.1.total_cmp(y.1))
            .map(|(c, _)| c.0)
    };
    let loudest = |lo: f32, hi: f32| {
        curve
            .iter()
            .filter(|c| (lo..=hi).contains(&c.0))
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map_or(lo, |c| c.0)
    };
    let carries_lows = matches!(instrument, Kick | Bass | Toms | Cajon | Drums | HandDrums);
    let mut heard: Vec<Step> = before.to_vec();
    let mut steps = Vec::new();
    for band in 0..4 {
        let Some(target) = t.target[band] else {
            continue;
        };
        let thirds = shaped(&a.report.third_octaves, &heard);
        let now = region_share(&thirds, REGIONS[band]);
        let off = target - now;
        let toward = if off.abs() < DEAD {
            0.0
        } else {
            (off - DEAD * off.signum()) * PART
        };
        let weight = if band == 0 { t.weight * dose } else { 0.0 };
        let (low, high) = (
            -t.cut[band] * k.max(0.5),
            (t.boost[band] + lean[band].max(0.0)) * k,
        );
        let wanted =
            (toward * dose + weight + lean[band] * flavor.amount.clamp(0.0, 1.0)).clamp(low, high);
        if wanted.abs() < 0.75 {
            continue;
        }
        let up = wanted > 0.0;
        let (freq, q, bell) = match band {
            0 if carries_lows => (
                if instrument == Kick {
                    loudest(40.0, 90.0)
                } else {
                    loudest(45.0, 120.0)
                },
                1.0,
                true,
            ),
            0 => (if up { 100.0 } else { 150.0 }, 0.7, false),
            1 if up => (250.0, 1.0, true),
            1 => (peak(160.0, 450.0).unwrap_or(300.0), 1.2, true),
            2 if up => (t.presence, 0.8, true),
            2 => (peak(2000.0, 5000.0).unwrap_or(3500.0), 1.2, true),
            _ => (if up { 10_000.0 } else { 8000.0 }, 0.7, false),
        };
        let purpose = match (band, up) {
            (0, true) => "Weight",
            (0, false) => "Boom",
            (1, true) => "Warmth",
            (1, false) => "Mud",
            (2, true) => "Presence",
            (2, false) => "Harshness",
            (_, true) => "Air",
            (_, false) => "Softness",
        };
        let step = |gain: f32| Step::Eq {
            band,
            freq: freq.round(),
            gain,
            q,
            bell,
            purpose,
        };
        // The gain that changes the region's share by what is wanted.
        let got = region_share(&shaped(&thirds, &[step(wanted)]), REGIONS[band]) - now;
        // A broad band moves its region less than its gain: a little more is asked of it, never
        // far past the instrument's limits.
        let gain = if band == 0 && carries_lows {
            wanted
        } else if got.abs() > 0.1 && got.signum() == wanted.signum() {
            (wanted * wanted / got).clamp(low * 1.5, high * 1.25)
        } else {
            wanted
        };
        let gain = (gain * 2.0).round() / 2.0;
        if gain.abs() >= 1.0 {
            heard.push(step(gain));
            steps.push(step(gain));
        }
    }
    steps
}

/// Bringing a track out: its tone, a compressor when it moves, its attacks, and the riding of
/// its level (offered). `before` is the cleaning kept.
pub fn bring_out(
    a: &Analysis,
    instrument: Instrument,
    strength: Strength,
    flavor: Flavor,
    before: &[Step],
) -> Vec<Step> {
    if strength.off() {
        return Vec::new();
    }
    let rules = rules(instrument);
    let k = strength.factor();
    let r = &a.report;
    let mut steps = tone_moves(a, instrument, k, flavor, before);
    let Comp(ratio, attack, release, reduction, lra) = rules.comp;
    let moves = lra == 0.0 || r.loudness_range.map_or(r.crest > 20.0, |v| v > lra);
    let reduction = reduction * k * flavor.get(|t| t.comp);
    if moves && r.integrated.is_some() && reduction >= 0.5 {
        let attack = attack * flavor.get(|t| t.attack);
        steps.push(compressor(a, (ratio, attack, release), reduction));
    }
    // More attack where the hits are soft (a low crest), less where they already snap.
    if let Some((attack, sustain)) = rules.transient {
        let soft = ((20.0 - r.crest) / 6.0).clamp(0.4, 1.5);
        let attack = (attack * soft * k * flavor.get(|t| t.transient)).round();
        if attack >= 5.0 {
            steps.push(Step::Transient {
                attack,
                sustain: (sustain * k).round(),
            });
        }
    }
    if let Some(points) = riding(&a.levels, k) {
        steps.push(Step::Ride(points));
    }
    steps
}

/// A compressor bringing the loud moments down by `reduction` dB.
fn compressor(a: &Analysis, (ratio, attack, release): (f32, f32, f32), reduction: f32) -> Step {
    Step::Compressor {
        ratio,
        attack,
        release,
        threshold: (a.peaks - reduction * ratio / (ratio - 1.0)).clamp(-60.0, -1.0),
        reduction,
    }
}

/// Gain changes bringing the level toward its median where the track plays, within 6 dB,
/// smoothed; `None` when there is nothing to ride.
fn riding(levels: &[f32], k: f32) -> Option<Vec<(f64, f32)>> {
    let playing = sorted(levels.iter().copied().filter(|v| *v > -60.0).collect());
    if playing.len() < 10 {
        return None;
    }
    let median = percentile(&playing, 0.5);
    let raw: Vec<f32> = levels
        .iter()
        .map(|v| {
            if *v > median - 20.0 {
                ((median - v) * 0.7 * k).clamp(-6.0, 6.0)
            } else {
                0.0
            }
        })
        .collect();
    let smooth: Vec<f32> = (0..raw.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(2), (i + 3).min(raw.len()));
            raw[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect();
    let mut points: Vec<(f64, f32)> = Vec::new();
    for (i, g) in smooth.iter().enumerate() {
        if points.last().is_none_or(|p| (p.1 - g).abs() >= 0.5) {
            points.push((i as f64 * RIDE_STEP + RIDE_STEP, (g * 10.0).round() / 10.0));
        }
    }
    (points.iter().any(|p| p.1.abs() >= 1.0)).then_some(points)
}

/// The style asked of the mix: what it should sound like, not a genre. It leans the tone,
/// doses the dynamics, moves instruments forward or back, brings its console color, its
/// ambience and its width. `Transparent` asks nothing but a lighter hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Character {
    #[default]
    Transparent,
    Warm,
    Crisp,
    Punchy,
    Big,
}

/// What a style does to the dynamics, as factors of the usual dose (1: unchanged).
#[derive(Clone, Copy, Debug)]
pub struct Traits {
    /// The tone moves.
    pub moves: f32,
    /// Compression's reduction, and its attack time.
    pub comp: f32,
    pub attack: f32,
    /// Transient shaping.
    pub transient: f32,
}

impl Character {
    pub const STYLES: [Character; 5] = [
        Character::Transparent,
        Character::Warm,
        Character::Crisp,
        Character::Punchy,
        Character::Big,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Character::Transparent => "Transparent",
            Character::Warm => "Warm",
            Character::Crisp => "Clean",
            Character::Punchy => "Punchy",
            Character::Big => "Big",
        }
    }

    /// What it sounds like, in English.
    pub fn description(self) -> &'static str {
        match self {
            Character::Transparent => "As in the room, nothing pushed",
            Character::Warm => "Round and soft, vintage",
            Character::Crisp => "Defined and modern, never harsh",
            Character::Punchy => "The rhythm up front, energy",
            Character::Big => "Large, wide and airy",
        }
    }

    pub fn traits(self) -> Traits {
        let t = |moves, comp, attack, transient| Traits {
            moves,
            comp,
            attack,
            transient,
        };
        match self {
            Character::Transparent => t(0.6, 0.6, 1.2, 0.5),
            Character::Warm => t(1.0, 1.0, 1.5, 0.5),
            Character::Crisp => t(1.0, 1.2, 0.9, 1.0),
            Character::Punchy => t(1.0, 1.4, 1.5, 1.8),
            Character::Big => t(1.0, 1.1, 1.2, 1.2),
        }
    }

    /// Its console color, as an index of [`crate::console::MODELS`] (0: none).
    pub fn console(self) -> usize {
        match self {
            Character::Transparent => 0,
            Character::Warm => 1,
            Character::Crisp => 2,
            Character::Punchy => 3,
            Character::Big => 4,
        }
    }

    /// Where it puts the depth, the ambience and the width (0 to 1).
    pub fn place(self) -> (f32, f32, f32) {
        match self {
            Character::Transparent => (0.3, 0.35, 0.4),
            Character::Warm => (0.4, 0.45, 0.4),
            Character::Crisp => (0.3, 0.15, 0.5),
            Character::Punchy => (0.3, 0.55, 0.5),
            Character::Big => (0.55, 0.7, 0.55),
        }
    }
}

/// A style at an intensity (0: nothing of it, 1: all of it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flavor {
    pub character: Character,
    pub amount: f32,
}

impl Flavor {
    /// A trait at this intensity.
    pub fn get(self, f: impl Fn(&Traits) -> f32) -> f32 {
        1.0 + (f(&self.character.traits()) - 1.0) * self.amount.clamp(0.0, 1.0)
    }

    /// How the style leans an instrument's tone in each of [`REGIONS`], dB at full intensity.
    fn lean(self, i: Instrument) -> [f32; 4] {
        let rhythm = matches!(i, Kick | Snare | Toms | Drums | Bass | Cajon | HandDrums);
        let lows = matches!(i, Kick | Bass | Toms | Cajon | Drums);
        let lean = match self.character {
            Character::Transparent => [0.0; 4],
            Character::Warm => [1.0, 1.0, -0.5, 0.0],
            Character::Crisp => [-0.5, -1.0, 0.5, 0.25],
            Character::Punchy if rhythm => [2.0, -1.5, 2.5, 0.0],
            Character::Punchy => [0.0, -0.5, 0.5, 0.0],
            Character::Big => [1.5, -0.5, 0.5, 1.5],
        };
        // Only what carries the low end gets more of it.
        [
            if lows || lean[0] < 0.0 { lean[0] } else { 0.0 },
            lean[1],
            lean[2],
            lean[3],
        ]
    }
}

/// The ambience of the space: from dry and close to vast. The knob goes through them in order,
/// each over a quarter of its course, the size growing within each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ambience {
    Studio,
    Natural,
    Live,
    Space,
}

impl Ambience {
    pub const ALL: [Ambience; 4] = [
        Ambience::Studio,
        Ambience::Natural,
        Ambience::Live,
        Ambience::Space,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Ambience::Studio => "Studio",
            Ambience::Natural => "Natural",
            Ambience::Live => "Live",
            Ambience::Space => "Spatial",
        }
    }

    /// The ambience at a place of the knob (0 to 1), and the size within it (0 to 1).
    pub fn at(v: f32) -> (Ambience, f32) {
        let v = v.clamp(0.0, 1.0) * 4.0;
        let k = (v.floor() as usize).min(3);
        (Self::ALL[k], (v - k as f32).clamp(0.0, 1.0))
    }
}

/// The buses tracks are grouped into, in the usual order of a mix: their key and name.
pub const GROUPS: [(&str, &str); 8] = [
    ("drums", "Drums"),
    ("percussion", "Percussion"),
    ("bass", "Bass"),
    ("guitars", "Guitars"),
    ("keys", "Keys and synths"),
    ("strings", "Strings and winds"),
    ("backing", "Backing vocals"),
    ("vocals", "Vocals"),
];

/// The group of an instrument, by its key in [`GROUPS`]; `None` for what stays alone.
pub fn group(i: Instrument) -> Option<&'static str> {
    Some(match i {
        Kick | Snare | Toms | HiHat | Overheads | Drums => "drums",
        HandDrums | Cajon | Shaker | Tambourine | Cowbell | Claps => "percussion",
        Bass => "bass",
        ElectricGuitar | AcousticGuitar => "guitars",
        Keys | Synth => "keys",
        Strings | Brass => "strings",
        BackingVocals | Whistling => "backing",
        VocalMale | VocalFemale => "vocals",
        Other => return None,
    })
}

/// The families a bus can hold, each named by one of its instruments.
pub const BUSES: [Instrument; 8] = [
    BackingVocals,
    Drums,
    HandDrums,
    Bass,
    ElectricGuitar,
    Keys,
    Strings,
    Other,
];

/// The family of a bus holding `i`, as in [`BUSES`].
pub fn bus_family(i: Instrument) -> Instrument {
    BUSES
        .into_iter()
        .find(|b| b.category() == i.category())
        .unwrap_or(Other)
}

/// Cleaning a bus: only a gentle high-pass under what its tracks play. What is destructive on
/// a sum (gates, de-essers, resonance cuts) belongs to its tracks.
pub fn clean_bus(a: &Analysis, family: Instrument, strength: Strength) -> Vec<Step> {
    if strength.off() {
        return Vec::new();
    }
    let hpf = match bus_family(family).category() {
        "Voices" => (40.0, 60.0),
        "Guitars" => (30.0, 45.0),
        _ => (20.0, 25.0),
    };
    filters(a, hpf, None, false, strength.factor())
        .into_iter()
        .collect()
}

/// Bringing a bus out: a slow compressor that glues it, and on the drums the gentle lift of
/// lows and highs a tube EQ gives.
pub fn bring_out_bus(
    a: &Analysis,
    family: Instrument,
    strength: Strength,
    flavor: Flavor,
) -> Vec<Step> {
    if strength.off() {
        return Vec::new();
    }
    let k = strength.factor();
    let mut steps = Vec::new();
    if bus_family(family) == Drums {
        let lean = flavor.lean(Drums);
        let intensity = flavor.amount.clamp(0.0, 1.0);
        let weight = (1.5 * k + lean[0] * intensity).clamp(0.0, 4.0);
        let air = (1.0 * k + lean[3] * intensity).clamp(-2.0, 3.0);
        let freq = a
            .report
            .curve
            .iter()
            .filter(|c| (45.0..=90.0).contains(&c.0))
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .map_or(60.0, |c| c.0);
        for (band, freq, gain, bell, purpose) in [
            (0, freq.round(), weight, true, "Weight"),
            (3, 10_000.0, air, false, "Air"),
        ] {
            let gain = (gain * 2.0).round() / 2.0;
            if gain.abs() >= 1.0 {
                steps.push(Step::Eq {
                    band,
                    freq,
                    gain,
                    q: 0.8,
                    bell,
                    purpose,
                });
            }
        }
    }
    let reduction = 1.5 * k * flavor.get(|t| t.comp);
    if a.report.integrated.is_some() && reduction >= 0.5 {
        let attack = 30.0 * flavor.get(|t| t.attack);
        steps.push(compressor(a, (2.0, attack, 200.0), reduction));
    }
    steps
}

/// The level a track is brought to by its input gain before anything else: where an analog
/// console and its compressors expect it (dBFS, average where it plays).
pub const WORKING_DB: f32 = -18.0;

/// The input gain (dB) bringing a track to [`WORKING_DB`], from its average level where it
/// plays; `None` when it hardly plays.
pub fn staging(a: &Analysis) -> Option<f32> {
    let playing: Vec<f32> = a.levels.iter().copied().filter(|v| *v > -60.0).collect();
    if playing.len() < 5 {
        return None;
    }
    let power = playing.iter().map(|v| 10f32.powf(v / 10.0)).sum::<f32>() / playing.len() as f32;
    let level = 10.0 * power.log10();
    Some((((WORKING_DB - level) * 10.0).round() / 10.0).clamp(-24.0, 24.0))
}

/// The send (dB) to the space at full depth; `None` for what stays dry, the low end and the
/// close drums that carry the rhythm.
fn space_send(i: Instrument) -> Option<f32> {
    match i {
        BackingVocals | Strings | Whistling => Some(-10.0),
        VocalMale | VocalFemale | AcousticGuitar | Keys | Brass | Claps => Some(-13.0),
        Snare | Synth | HandDrums => Some(-15.0),
        ElectricGuitar | Toms | Shaker | Tambourine | Cajon => Some(-17.0),
        Cowbell | Drums => Some(-20.0),
        Kick | Bass | HiHat | Overheads | Other => None,
    }
}

/// The drums and the percussion, kept nearer in a vast space.
fn rhythmic(i: Instrument) -> bool {
    matches!(i.category(), "Drums" | "Percussion")
}

/// The send (dB) of an instrument to its space, at `depth` (0 to 1) in `ambience`.
pub fn space_send_db(i: Instrument, depth: f32, ambience: Ambience) -> Option<f32> {
    let full = space_send(i)?
        + match ambience {
            Ambience::Studio => -2.0,
            Ambience::Natural => 0.0,
            Ambience::Live => 1.0,
            Ambience::Space if rhythmic(i) => -7.0,
            Ambience::Space => -1.0,
        };
    (depth > 0.01).then(|| (full + 20.0 * depth.min(1.0).log10()).max(-40.0))
}

/// The send (dB) of the lead voice to the echo, which comes with a live or a vast ambience.
pub fn echo_send_db(i: Instrument, depth: f32, ambience: Ambience) -> Option<f32> {
    let full = match ambience {
        Ambience::Live => -15.0,
        Ambience::Space => -12.0,
        Ambience::Studio | Ambience::Natural => return None,
    };
    (matches!(i, VocalMale | VocalFemale) && depth > 0.01)
        .then(|| (full + 20.0 * depth.min(1.0).log10()).max(-40.0))
}

/// The buses of the space: a room, a plate and a hall shared by the tracks that call for them
/// (only the natural ambience uses all three), and the echo of the lead voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    Room,
    Plate,
    Hall,
    Echo,
}

impl Space {
    pub const ALL: [Space; 4] = [Space::Room, Space::Plate, Space::Hall, Space::Echo];

    /// Its name in an ambience, in English: what its bus is called.
    pub fn name(self, ambience: Ambience) -> &'static str {
        match (self, ambience) {
            (Space::Echo, _) => "Echo",
            (_, Ambience::Studio) => "Studio",
            (_, Ambience::Live) => "Live",
            (_, Ambience::Space) => "Spatial",
            (Space::Room, _) => "Room",
            (Space::Plate, _) => "Plate",
            (Space::Hall, _) => "Hall",
        }
    }

    /// The space of an instrument in an ambience; `None` for what stays dry.
    pub fn of(i: Instrument, ambience: Ambience) -> Option<Self> {
        space_send(i)?;
        Some(match ambience {
            Ambience::Studio => Space::Room,
            Ambience::Live | Ambience::Space => Space::Hall,
            Ambience::Natural => match i {
                VocalMale | VocalFemale | BackingVocals | Whistling | Snare => Space::Plate,
                Toms | Drums | HandDrums | Cajon | Shaker | Tambourine | Cowbell | Claps => {
                    Space::Room
                }
                _ => Space::Hall,
            },
        })
    }

    /// The effect of its bus.
    pub fn effect(self) -> &'static str {
        match self {
            Space::Echo => "delay",
            _ => "reverb",
        }
    }

    /// Its effect's settings in an ambience at a size (0 to 1), for a style and a tempo. The
    /// size moves only what changes without a click (the decay, the tone, the early
    /// reflections); the room and the pre-delay belong to the ambience. The tails die with the
    /// song's pulse, their lows cut so that they never muddy, their highs soft.
    pub fn settings(
        self,
        ambience: Ambience,
        size: f32,
        flavor: Flavor,
        bpm: f64,
    ) -> Vec<(&'static str, f32)> {
        let beat = (60.0 / bpm.clamp(50.0, 220.0)) as f32;
        let s = size.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * s;
        if self == Space::Echo {
            return match ambience {
                Ambience::Space => vec![
                    ("sync", 1.0),
                    ("note", 6.0),
                    ("feedback", lerp(25.0, 40.0)),
                    ("pingpong", 1.0),
                    ("low_cut", 300.0),
                    ("high_cut", lerp(4500.0, 3500.0)),
                    ("model", 0.0),
                    ("mix", 100.0),
                    ("duck", 50.0),
                ],
                _ => vec![
                    ("sync", 0.0),
                    ("time", (beat * 1000.0 / 5.0).clamp(90.0, 140.0).round()),
                    ("feedback", lerp(8.0, 18.0)),
                    ("pingpong", 0.0),
                    ("low_cut", 250.0),
                    ("high_cut", 5000.0),
                    ("model", 1.0),
                    ("wow", 15.0),
                    ("mix", 100.0),
                    ("duck", 30.0),
                ],
            };
        }
        // Type, decay (s), pre-delay (ms), low and high cut (Hz), room size, early
        // reflections, treble, diffusion, modulation.
        let (kind, decay, predelay, low, high, room, early, treble, diffusion, modulation) =
            match (ambience, self) {
                (Ambience::Studio, _) => (
                    crate::reverb::ROOM,
                    lerp(0.35, 0.7),
                    2.0,
                    220.0,
                    lerp(8000.0, 9500.0),
                    25.0,
                    lerp(65.0, 50.0),
                    55.0,
                    70.0,
                    10.0,
                ),
                (Ambience::Natural, Space::Room) => (
                    crate::reverb::ROOM,
                    lerp(0.5, 0.9),
                    8.0,
                    180.0,
                    9000.0,
                    35.0,
                    lerp(60.0, 45.0),
                    50.0,
                    70.0,
                    10.0,
                ),
                (Ambience::Natural, Space::Plate) => (
                    crate::reverb::PLATE,
                    lerp(beat * 2.0, beat * 3.2).clamp(0.9, 2.2),
                    (beat * 1000.0 / 16.0).clamp(15.0, 40.0),
                    220.0,
                    lerp(7000.0, 8500.0),
                    50.0,
                    20.0,
                    50.0,
                    85.0,
                    25.0,
                ),
                (Ambience::Natural, _) => (
                    crate::reverb::HALL,
                    lerp(beat * 2.8, beat * 4.0).clamp(1.2, 2.8),
                    25.0,
                    220.0,
                    lerp(6500.0, 8000.0),
                    55.0,
                    30.0,
                    40.0,
                    80.0,
                    30.0,
                ),
                (Ambience::Live, _) => (
                    crate::reverb::HALL,
                    lerp(1.6, 2.8),
                    (beat * 1000.0 / 24.0).clamp(15.0, 35.0),
                    200.0,
                    lerp(7500.0, 9000.0),
                    80.0,
                    lerp(55.0, 70.0),
                    45.0,
                    75.0,
                    20.0,
                ),
                (Ambience::Space, _) => (
                    crate::reverb::SPACE,
                    lerp(3.5, 7.0),
                    (beat * 1000.0 / 8.0).clamp(40.0, 90.0),
                    300.0,
                    lerp(7000.0, 5500.0),
                    90.0,
                    15.0,
                    35.0,
                    90.0,
                    lerp(45.0, 65.0),
                ),
            };
        // A warm style darkens the tails, a crisp or big one opens them.
        let top = match flavor.character {
            Character::Warm => 0.9,
            Character::Crisp => 1.05,
            Character::Big => 1.15,
            _ => 1.0,
        };
        let top = 1.0 + (top - 1.0) * flavor.amount.clamp(0.0, 1.0);
        vec![
            ("type", kind as f32),
            ("decay", (decay * 20.0).round() / 20.0),
            ("predelay", predelay.round()),
            ("low_cut", low),
            (
                "high_cut",
                ((high * top).min(12_000.0) / 100.0).round() * 100.0,
            ),
            ("width", 100.0),
            ("mix", 100.0),
            ("size", room),
            ("early", early.clamp(0.0, 100.0)),
            ("treble", treble),
            ("diffusion", diffusion),
            ("modulation", modulation.clamp(0.0, 100.0)),
        ]
    }
}

/// How far apart (pan, 0 to 1) the tracks of an instrument go when there are several; `None`
/// for what stays in the center.
fn spread_of(i: Instrument) -> Option<f32> {
    match i {
        ElectricGuitar => Some(0.8),
        AcousticGuitar | Shaker | Tambourine => Some(0.7),
        BackingVocals | Synth | Strings | HandDrums | Cowbell => Some(0.6),
        Toms | Keys | Brass => Some(0.5),
        Claps => Some(0.4),
        Overheads => Some(1.0),
        _ => None,
    }
}

/// The side a track's name gives: -1 left, 1 right.
fn side(name: &str) -> f32 {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    if has(&["l", "left", "gauche", "lft"]) {
        -1.0
    } else if has(&["r", "right", "droite", "rgt"]) {
        1.0
    } else {
        0.0
    }
}

/// Pans for tracks (instrument, name) played together: those of an instrument found several
/// times are spread evenly around the center, subtly, by `space` (0 to 1), their names'
/// left and right respected; `None` for the others.
pub fn pans(tracks: &[(Instrument, &str)], space: f32) -> Vec<Option<f32>> {
    let mut out = vec![None; tracks.len()];
    if space <= 0.01 {
        return out;
    }
    for i in Instrument::ALL {
        let Some(width) = spread_of(i) else {
            continue;
        };
        let mut group: Vec<usize> = (0..tracks.len()).filter(|&k| tracks[k].0 == i).collect();
        if group.len() < 2 {
            continue;
        }
        group.sort_by(|&a, &b| side(tracks[a].1).total_cmp(&side(tracks[b].1)));
        let width = width * space.min(1.0);
        let last = (group.len() - 1) as f32;
        for (j, &k) in group.iter().enumerate() {
            out[k] = Some(((j as f32 / last * 2.0 - 1.0) * width * 100.0).round() / 100.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Noise, filtered by `filter`, `seconds` long.
    fn noise(seconds: f32, mut filter: impl FnMut(f32) -> f32) -> Vec<f32> {
        let mut seed = 1u32;
        (0..(seconds * RATE as f32) as usize)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                filter((seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0) * 0.2
            })
            .collect()
    }

    fn sine(freq: f32, t: usize) -> f32 {
        (std::f32::consts::TAU * freq * t as f32 / RATE as f32).sin()
    }

    #[test]
    fn a_track_name_says_its_instrument() {
        let name = |n: &str| Instrument::from_name(n, None);
        assert_eq!(name("Lead vox"), Some(VocalMale));
        assert_eq!(
            Instrument::from_name("Lead vox", Some(220.0)),
            Some(VocalFemale)
        );
        assert_eq!(name("Voix femme"), Some(VocalFemale));
        assert_eq!(name("02-KICK in"), Some(Kick));
        assert_eq!(name("Grosse caisse"), Some(Kick));
        assert_eq!(name("Hi-hat"), Some(HiHat));
        assert_eq!(name("Charley"), Some(HiHat));
        assert_eq!(name("Gtr L"), Some(ElectricGuitar));
        assert_eq!(name("Guitare acoustique"), Some(AcousticGuitar));
        assert_eq!(name("Bass DI"), Some(Bass));
        assert_eq!(name("Lead gtr"), Some(ElectricGuitar));
        assert_eq!(name("Pad chaud"), Some(Synth));
        assert_eq!(name("Take 3"), None);
        assert_eq!(name("Choeurs 2"), Some(BackingVocals));
        assert_eq!(name("BV high"), Some(BackingVocals));
        assert_eq!(name("Congas"), Some(HandDrums));
        assert_eq!(name("Cajón"), Some(Cajon));
        assert_eq!(name("Shaker"), Some(Shaker));
        assert_eq!(name("Tamb"), Some(Tambourine));
        assert_eq!(name("Cowbell"), Some(Cowbell));
        assert_eq!(name("Claps"), Some(Claps));
        for i in Instrument::ALL {
            assert_eq!(Instrument::from_id(i.id()), Some(i));
        }
    }

    #[test]
    fn the_high_pass_sits_under_the_energy_that_counts() {
        let x = noise(6.0, |x| x);
        let left = high_passed(&x, RATE, 300.0);
        let a = analyse(&left, &left, RATE);
        let steps = clean(&a, VocalMale, Strength::NORMAL);
        let Some(Step::Filters { hpf: Some(hpf), .. }) = steps.first() else {
            panic!("{steps:?}");
        };
        assert!(
            *hpf > 60.0 && *hpf <= 70.0,
            "the energy starts high: the filter goes as high as a voice allows ({hpf})"
        );
    }

    #[test]
    fn a_resonance_coming_and_going_gets_a_dynamic_cut() {
        let left: Vec<f32> = noise(8.0, |x| x)
            .into_iter()
            .enumerate()
            .map(|(t, x)| {
                let on = (t / (RATE as usize / 2)).is_multiple_of(2);
                x * 0.3 + if on { sine(2500.0, t) * 0.3 } else { 0.0 }
            })
            .collect();
        let a = analyse(&left, &left, RATE);
        let cut = clean(&a, VocalMale, Strength::NORMAL)
            .into_iter()
            .find_map(|s| match s {
                Step::Resonance(c) => Some(c),
                _ => None,
            })
            .expect("the tone is cut");
        assert!((cut.freq / 2500.0).log2().abs() < 0.1, "{cut:?}");
        assert!(cut.threshold.is_some() && cut.depth <= -2.0 && cut.depth >= -12.0);
    }

    #[test]
    fn a_kick_gets_weight_less_mud_and_a_compressor() {
        // A kick every half second: a 52 Hz thump, a boxy ring at 260 Hz, a click.
        let left: Vec<f32> = (0..RATE as usize * 8)
            .map(|t| {
                let s = (t % (RATE as usize / 2)) as f32 / RATE as f32;
                sine(52.0, t) * (-s * 14.0).exp() * 0.6
                    + sine(260.0, t) * (-s * 18.0).exp() * 0.35
                    + sine(2600.0, t) * (-s * 120.0).exp() * 0.2
            })
            .collect();
        let a = analyse(&left, &left, RATE);
        let steps = bring_out(&a, Kick, Strength::NORMAL, Flavor::default(), &[]);
        let eq = |band: usize| {
            steps.iter().find_map(|s| match s {
                Step::Eq {
                    band: b,
                    freq,
                    gain,
                    ..
                } if *b == band => Some((*freq, *gain)),
                _ => None,
            })
        };
        let (weight, mud) = (eq(0).unwrap(), eq(1).unwrap());
        assert!(
            (40.0..=65.0).contains(&weight.0) && weight.1 >= 2.0,
            "{weight:?}"
        );
        assert!((220.0..=320.0).contains(&mud.0) && mud.1 <= -2.5, "{mud:?}");
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, Step::Compressor { reduction, .. } if *reduction >= 3.0)),
            "a kick is always levelled"
        );
    }

    #[test]
    fn the_space_leaves_the_low_end_dry_and_grows_with_its_knob() {
        let natural = Ambience::Natural;
        for dry in [Kick, Bass, HiHat, Other] {
            assert_eq!(space_send_db(dry, 1.0, natural), None, "{dry:?}");
        }
        assert_eq!(
            space_send_db(VocalMale, 0.0, natural),
            None,
            "nothing at zero"
        );
        let half = space_send_db(VocalMale, 0.5, natural).unwrap();
        let full = space_send_db(VocalMale, 1.0, natural).unwrap();
        assert!(half < full && full <= -10.0);
        assert!(
            space_send_db(BackingVocals, 1.0, natural) > space_send_db(VocalMale, 1.0, natural)
        );
        // A vast space keeps the drums nearer; the lead voice gets an echo live and in space.
        assert!(space_send_db(Snare, 1.0, Ambience::Space) < space_send_db(Snare, 1.0, natural));
        assert!(echo_send_db(VocalFemale, 0.5, Ambience::Live).is_some());
        assert_eq!(echo_send_db(VocalFemale, 0.5, natural), None);
        assert_eq!(echo_send_db(BackingVocals, 0.5, Ambience::Space), None);
    }

    #[test]
    fn pairs_are_spread_subtly_their_names_sides_respected() {
        let tracks = [
            (ElectricGuitar, "Gtr R"),
            (Kick, "Kick"),
            (ElectricGuitar, "Gtr L"),
            (VocalMale, "Lead"),
            (VocalMale, "Lead double"),
            (Toms, "Tom 1"),
            (Toms, "Tom 2"),
            (Toms, "Floor"),
        ];
        let pans = pans(&tracks, 1.0);
        assert_eq!(pans[0], Some(0.8));
        assert_eq!(pans[2], Some(-0.8));
        assert_eq!((pans[1], pans[3], pans[4]), (None, None, None));
        assert_eq!(
            (pans[5], pans[6], pans[7]),
            (Some(-0.5), Some(0.0), Some(0.5))
        );
        assert!(super::pans(&tracks, 0.0).iter().all(Option::is_none));
        assert_eq!(super::pans(&tracks, 0.5)[0], Some(0.4));
    }

    #[test]
    fn a_bus_gets_a_high_pass_and_some_glue_never_what_its_tracks_get() {
        let left = noise(6.0, |x| x);
        let a = analyse(&left, &left, RATE);
        assert!(
            clean_bus(&a, Kick, Strength::NORMAL)
                .iter()
                .all(|s| matches!(s, Step::Filters { lpf: None, .. }))
        );
        let out = bring_out_bus(&a, Kick, Strength::NORMAL, Flavor::default());
        assert!(
            out.iter()
                .any(|s| matches!(s, Step::Compressor { ratio, .. } if *ratio == 2.0))
        );
        assert!(
            out.iter()
                .all(|s| matches!(s, Step::Eq { .. } | Step::Compressor { .. }))
        );
        assert_eq!(bus_family(Kick), Drums);
        assert_eq!(bus_family(VocalMale), BackingVocals);
    }

    #[test]
    fn a_style_leans_the_tone_brings_its_console_and_nothing_without_intensity() {
        let left = noise(6.0, |x| x);
        let a = analyse(&left, &left, RATE);
        let clean = Flavor {
            character: Character::Crisp,
            amount: 1.0,
        };
        let warm = Flavor {
            character: Character::Warm,
            amount: 1.0,
        };
        assert!(clean.lean(VocalMale)[2] > 0.0 && warm.lean(VocalMale)[2] < 0.0);
        assert!(warm.lean(VocalMale)[3] < clean.lean(VocalMale)[3]);
        // Only what carries the low end gets more of it.
        let punchy = Flavor {
            character: Character::Punchy,
            amount: 1.0,
        };
        assert!(punchy.lean(Kick)[0] > 0.0 && punchy.lean(VocalMale)[0] == 0.0);
        let none = Flavor {
            character: Character::Crisp,
            amount: 0.0,
        };
        assert_eq!(
            bring_out(&a, VocalMale, Strength::NORMAL, none, &[]),
            bring_out(&a, VocalMale, Strength::NORMAL, Flavor::default(), &[]),
            "no intensity, no change"
        );
        assert_eq!(Character::Transparent.console(), 0);
        assert!(Character::STYLES[1..].iter().all(|c| c.console() > 0));
        assert_eq!(Ambience::at(0.1).0, Ambience::Studio);
        assert_eq!(Ambience::at(0.6).0, Ambience::Live);
        assert_eq!(Ambience::at(1.0), (Ambience::Space, 1.0));
    }

    #[test]
    fn a_quiet_track_is_raised_to_the_working_level_and_its_thresholds_follow() {
        let quiet: Vec<f32> = noise(6.0, |x| x).iter().map(|x| x * 0.05).collect();
        let a = analyse(&quiet, &quiet, RATE);
        let up = staging(&a).expect("it plays");
        assert!(up > 6.0, "{up}");
        let gate = Step::Gate {
            threshold: -50.0,
            range: -20.0,
            release: 100.0,
        };
        assert!(matches!(gate.shifted(up), Step::Gate { threshold, .. } if threshold > -50.0));
    }

    #[test]
    fn stronger_goes_further() {
        let left = noise(6.0, |x| x);
        let a = analyse(&left, &left, RATE);
        let total = |s: Strength| -> f32 {
            bring_out(&a, VocalMale, s, Flavor::default(), &[])
                .iter()
                .map(|s| match s {
                    Step::Eq { gain, .. } => gain.abs(),
                    _ => 0.0,
                })
                .sum()
        };
        assert!(total(Strength::GENTLE) < total(Strength::NORMAL));
        assert!(total(Strength::NORMAL) < total(Strength::BOLD));
    }

    #[test]
    fn a_voice_that_hisses_gets_a_de_esser() {
        // A held tone with a burst of highs every second.
        let hiss = high_passed(&noise(8.0, |x| x), RATE, 6000.0);
        let left: Vec<f32> = hiss
            .iter()
            .enumerate()
            .map(|(t, h)| {
                let s = t % RATE as usize;
                sine(180.0, t) * 0.2 + if s < RATE as usize / 10 { h * 3.0 } else { 0.0 }
            })
            .collect();
        let a = analyse(&left, &left, RATE);
        assert!(
            a.pitch.is_some_and(|f| (f - 180.0).abs() < 10.0),
            "{:?}",
            a.pitch
        );
        let steps = clean(&a, VocalFemale, Strength::NORMAL);
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, Step::DeEss { sure: true, .. })),
            "{steps:?}"
        );
        let added = inserts(steps.iter());
        assert!(added.iter().any(|(effect, _)| *effect == "deesser"));
    }

    #[test]
    fn only_a_track_that_moves_gets_a_compressor() {
        let flat = noise(30.0, |x| x);
        let moving: Vec<f32> = noise(30.0, |x| x)
            .into_iter()
            .enumerate()
            .map(|(t, x)| {
                x * if (t / (RATE as usize * 5)).is_multiple_of(2) {
                    1.0
                } else {
                    0.2
                }
            })
            .collect();
        let comp = |s: &[f32]| {
            bring_out(
                &analyse(s, s, RATE),
                Keys,
                Strength::NORMAL,
                Flavor::default(),
                &[],
            )
            .iter()
            .any(|s| matches!(s, Step::Compressor { .. }))
        };
        assert!(!comp(&flat));
        assert!(comp(&moving));
    }

    #[test]
    fn riding_lifts_the_quiet_parts_within_six_db() {
        let mut levels = vec![-20.0; 50];
        levels.extend(vec![-32.0; 50]);
        levels.extend(vec![-80.0; 20]);
        let points = riding(&levels, 1.0).expect("something to ride");
        assert!(points.iter().all(|p| p.1.abs() <= 6.0));
        let at = |s: f64| points.iter().rev().find(|p| p.0 <= s).map_or(0.0, |p| p.1);
        assert!(at(15.0) > 3.0, "the quiet half goes up");
        assert!(at(22.0).abs() < 0.5, "silence is left alone");
    }

    #[test]
    fn the_level_kept_undoes_what_the_preparation_adds() {
        let x = noise(8.0, |x| x);
        let boost = [Step::Eq {
            band: 2,
            freq: 3000.0,
            gain: 9.0,
            q: 0.7,
            bell: true,
            purpose: "Presence",
        }];
        let gain = level_kept(&x, RATE, 0.0, &boost);
        assert!(gain < -1.0, "a big boost is brought back down ({gain})");
        assert_eq!(
            level_kept(&x, RATE, 0.0, &[]),
            0.0,
            "nothing set: nothing to undo"
        );
    }

    /// Cost of each effect as the preparation sets it, against real time (diagnostic).
    #[test]
    #[ignore = "a measure, run by hand with --ignored --nocapture"]
    fn prepare_cost() {
        let seconds = 10.0;
        // Bursts, so that gates, compressors and dynamic bands keep moving.
        let dry: Vec<f32> = noise(seconds, |x| x)
            .into_iter()
            .enumerate()
            .map(|(t, x)| {
                let s = (t % (RATE as usize / 4)) as f32 / RATE as f32;
                x * 3.0 * (-s * 12.0).exp() + sine(110.0, t) * 0.1
            })
            .collect();
        let run = |name: &str, effect: &str, params: &[(String, f32)]| {
            let mut e = crate::create(effect).unwrap();
            let info = crate::info(effect).unwrap();
            e.prepare(RATE, 256);
            // As in a session: its window closed.
            e.set_watched(false);
            for (id, v) in params {
                let i = info
                    .params
                    .iter()
                    .position(|p| p.id == id.as_str())
                    .unwrap();
                e.set_param(i, *v);
            }
            let (mut l, mut r) = (dry.clone(), dry.clone());
            let start = std::time::Instant::now();
            for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                e.process(a, b);
            }
            let cost = start.elapsed().as_secs_f32() / seconds * 100.0;
            // Then silence, second by second: the worst one.
            let mut worst = 0.0f32;
            for _ in 0..40 {
                let (mut l, mut r) = (vec![0.0f32; RATE as usize], vec![0.0f32; RATE as usize]);
                let start = std::time::Instant::now();
                for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                    e.process(a, b);
                }
                worst = worst.max(start.elapsed().as_secs_f32() * 100.0);
            }
            println!("{name:<42} {cost:6.2} % playing, {worst:6.2} % worst second of silence");
        };
        let own = |list: &[(&str, f32)]| -> Vec<(String, f32)> {
            list.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
        };
        run("channel strip, nothing on", "channel", &[]);
        let kick = [
            Step::Filters {
                hpf: Some(25.0),
                lpf: Some(8000.0),
                steep: false,
            },
            Step::Gate {
                threshold: -40.0,
                range: -30.0,
                release: 120.0,
            },
            Step::Eq {
                band: 0,
                freq: 52.0,
                gain: 4.5,
                q: 1.0,
                bell: true,
                purpose: "Weight",
            },
            Step::Eq {
                band: 1,
                freq: 260.0,
                gain: -5.0,
                q: 1.5,
                bell: true,
                purpose: "Mud",
            },
            Step::Eq {
                band: 2,
                freq: 2600.0,
                gain: 4.0,
                q: 1.2,
                bell: true,
                purpose: "Click",
            },
            Step::Compressor {
                ratio: 4.0,
                attack: 20.0,
                release: 80.0,
                threshold: -18.0,
                reduction: 3.5,
            },
        ];
        for (name, steps) in [
            ("channel strip, filters", &kick[..1]),
            ("channel strip, filters + gate", &kick[..2]),
            ("channel strip, filters + gate + eq", &kick[..5]),
            ("channel strip, all of a kick", &kick[..]),
        ] {
            run(name, "channel", &strip_settings(steps.iter()));
        }
        for character in 0..crate::channel::CHARACTERS.len() {
            let mut params = strip_settings(kick.iter());
            params.push(("character".to_owned(), character as f32));
            run(
                &format!(
                    "channel strip, kick, {}",
                    crate::channel::CHARACTERS[character]
                ),
                "channel",
                &params,
            );
        }
        let mut meter = crate::loudness::LevelMeter::new(RATE);
        let start = std::time::Instant::now();
        for x in &dry {
            meter.push(*x, *x);
        }
        let _ = meter.short_term();
        println!(
            "{:<42} {:6.2} % of real time",
            "one level meter (Match has two per effect)",
            start.elapsed().as_secs_f32() / seconds * 100.0
        );
        run("Strada-EQ, nothing on", "eq", &[]);
        let cut = |freq: f32, dynamic: bool| {
            Step::Resonance(Cut {
                freq,
                depth: -8.0,
                threshold: dynamic.then_some(-40.0),
            })
        };
        for dynamic in [false, true] {
            let steps = [
                cut(400.0, dynamic),
                cut(1200.0, dynamic),
                cut(3100.0, dynamic),
                cut(6000.0, dynamic),
            ];
            let params = &inserts(steps.iter())[0].1;
            let name = if dynamic {
                "Strada-EQ, 4 dynamic cuts"
            } else {
                "Strada-EQ, 4 fixed cuts"
            };
            run(name, "eq", params);
        }
        let plosives = [Step::Plosives(Cut {
            freq: PLOSIVE_HZ,
            depth: -8.0,
            threshold: Some(-40.0),
        })];
        run(
            "Strada-EQ, plosives (dynamic low shelf)",
            "eq",
            &inserts(plosives.iter())[0].1,
        );
        run("Strada-DeEss, default", "deesser", &[]);
        run(
            "Strada-DeEss, as prepared",
            "deesser",
            &own(&[
                ("frequency", 5500.0),
                ("threshold", -10.0),
                ("range", 8.0),
                ("relative", 1.0),
                ("lookahead", 2.0),
            ]),
        );
    }

    #[test]
    fn at_zero_nothing_is_proposed() {
        let x = noise(4.0, |v| v);
        let a = analyse(&x, &x, RATE);
        let off = Strength(0.0);
        assert!(off.off() && off.name() == "Off");
        assert!(clean(&a, VocalMale, off).is_empty());
        assert!(bring_out(&a, VocalMale, off, Flavor::default(), &[]).is_empty());
        assert!(clean_bus(&a, Drums, off).is_empty());
        assert!(bring_out_bus(&a, Drums, off, Flavor::default()).is_empty());
        // A light touch does less than the usual dose, never nothing at all.
        let moves = |s: Strength| -> f32 {
            bring_out(&a, VocalMale, s, Flavor::default(), &[])
                .iter()
                .map(|s| match s {
                    Step::Eq { gain, .. } => gain.abs(),
                    _ => 0.0,
                })
                .sum()
        };
        assert!(moves(Strength::GENTLE) > 0.0 && moves(Strength::GENTLE) < moves(Strength::NORMAL));
    }

    #[test]
    fn what_the_cuts_remove_is_given_back_and_the_input_gain_too() {
        // Low noise: a high-pass in the middle of it takes a lot away.
        let mut low = 0.0;
        let x = noise(4.0, |v| {
            low += 0.02 * (v - low);
            low * 6.0
        });
        let cut = [Step::Filters {
            hpf: Some(400.0),
            lpf: None,
            steep: true,
        }];
        let lost = through(&x, RATE, 0.0, &[]) - through(&x, RATE, 0.0, &cut);
        assert!(lost > 3.0, "{lost}");
        let kept = level_kept(&x, RATE, 0.0, &cut);
        assert!((kept - lost).abs() < 0.3, "{kept} for {lost}");
        // The same through an input gain 7 dB hotter: all of it comes back.
        let hot = level_kept(&x, RATE, 7.0, &cut);
        assert!((hot - (kept - 7.0)).abs() < 0.3, "{hot}");
    }

    #[test]
    fn each_ambience_has_its_spaces_and_the_size_and_the_tempo_set_the_tails() {
        let natural = Ambience::Natural;
        assert_eq!(Space::of(Snare, natural), Some(Space::Plate));
        assert_eq!(Space::of(VocalFemale, natural), Some(Space::Plate));
        assert_eq!(Space::of(Toms, natural), Some(Space::Room));
        assert_eq!(Space::of(Keys, natural), Some(Space::Hall));
        assert_eq!(Space::of(Kick, natural), None, "the low end stays dry");
        assert_eq!(Space::of(Keys, Ambience::Studio), Some(Space::Room));
        assert_eq!(Space::of(Snare, Ambience::Live), Some(Space::Hall));
        let none = Flavor::default();
        let get = |space: Space, ambience: Ambience, size: f32, bpm: f64, id: &str| {
            space
                .settings(ambience, size, none, bpm)
                .iter()
                .find(|p| p.0 == id)
                .map(|p| p.1)
        };
        let decay = |space, ambience, size, bpm| get(space, ambience, size, bpm, "decay").unwrap();
        assert!(decay(Space::Plate, natural, 0.5, 70.0) > decay(Space::Plate, natural, 0.5, 160.0));
        assert!(decay(Space::Hall, natural, 0.5, 120.0) > decay(Space::Room, natural, 0.5, 120.0));
        assert!(
            decay(Space::Hall, Ambience::Space, 0.0, 120.0)
                > decay(Space::Hall, natural, 1.0, 120.0)
        );
        assert!(
            decay(Space::Room, Ambience::Studio, 1.0, 120.0)
                > decay(Space::Room, Ambience::Studio, 0.0, 120.0)
        );
        // Within an ambience, the size moves nothing that would click: the room and the
        // pre-delay stay.
        for id in ["size", "predelay", "type"] {
            assert_eq!(
                get(Space::Hall, Ambience::Live, 0.0, 120.0, id),
                get(Space::Hall, Ambience::Live, 1.0, 120.0, id)
            );
        }
        assert_eq!(
            get(Space::Hall, Ambience::Space, 0.5, 120.0, "type"),
            Some(crate::reverb::SPACE as f32)
        );
        assert_eq!(Space::Echo.effect(), "delay");
        assert!(get(Space::Echo, Ambience::Space, 0.5, 120.0, "sync") == Some(1.0));
    }
}
