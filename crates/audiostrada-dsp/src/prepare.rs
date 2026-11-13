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
}

use Instrument::*;

/// Above this fundamental (Hz, the median through the track), a voice is taken for a woman's.
const FEMALE_FROM: f32 = 165.0;

impl Instrument {
    /// In the order of their categories.
    pub const ALL: [Instrument; 23] = [
        VocalMale,
        VocalFemale,
        BackingVocals,
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
            VocalMale | VocalFemale | BackingVocals => "Voices",
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
        }
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

    /// A voice, a man's or a woman's by its pitch.
    pub fn voice(pitch: Option<f32>) -> Self {
        if pitch.is_some_and(|f| f >= FEMALE_FROM) {
            VocalFemale
        } else {
            VocalMale
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

/// How far the rules go, from 0 (a light touch) to 1 (all the way); 0.5 is the usual dose.
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
            x if x < 0.12 => "Barely",
            x if x < 0.38 => "Gentle",
            x if x < 0.63 => "Normal",
            x if x < 0.88 => "Bold",
            _ => "All the way",
        }
    }

    /// 0.65 gentle, 1 normal, 1.35 bold.
    fn factor(self) -> f32 {
        0.3 + 1.4 * self.0.clamp(0.0, 1.0)
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
    /// The loudest [`EXCERPT`] seconds, in mono: what the level kept is measured on.
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
    const TRIES: usize = 40;
    if x.len() < WINDOW * 2 {
        return None;
    }
    let ten = (rate / 100).max(1) as usize;
    let mut difference = vec![0.0; WINDOW / 2];
    let mut found = Vec::new();
    for k in 0..TRIES {
        let at = k * (x.len() - WINDOW) / TRIES;
        if short.get(at / ten).is_none_or(|v| *v < loud - 20.0) {
            continue;
        }
        if let Some((f, clarity)) =
            crate::tuner::detect(&x[at..at + WINDOW], rate as f32, &mut difference)
            && clarity >= 0.8
            && (60.0..=1000.0).contains(&f)
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
    // The loudest stretch: where a change of level is heard.
    let span = (EXCERPT / RIDE_STEP) as usize;
    let power: Vec<f32> = levels.iter().map(|v| 10f32.powf(v / 10.0)).collect();
    let from = (0..power.len().saturating_sub(span))
        .map(|i| (i, power[i..i + span].iter().sum::<f32>()))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(i, _)| i * step);
    let to = (from + (EXCERPT * f64::from(rate)) as usize).min(mid.len());
    let excerpt = std::sync::Arc::new(mid[from.min(to)..to].to_vec());
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

/// The gain (dB) bringing `excerpt`, once through the input gain `trim_db`, the channel strip
/// set with `strip` then `inserts`, back to the loudness it had.
pub fn level_kept(
    excerpt: &[f32],
    rate: u32,
    trim_db: f32,
    strip: &[(String, f32)],
    inserts: &[(&'static str, Vec<(String, f32)>)],
) -> f32 {
    use crate::loudness::LevelMeter;
    // Through the input gain set first, as the track will play.
    let trim = crate::db_to_gain(trim_db);
    let trimmed: Vec<f32> = excerpt.iter().map(|x| x * trim).collect();
    let (mut left, mut right) = (trimmed.clone(), trimmed);
    let mut before = LevelMeter::new(rate);
    for x in excerpt {
        before.push(*x, *x);
    }
    let chain =
        std::iter::once(("channel", strip)).chain(inserts.iter().map(|(e, p)| (*e, &p[..])));
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
    let mut after = LevelMeter::new(rate);
    for (l, r) in left.iter().zip(&right) {
        after.push(*l, *r);
    }
    let (a, b) = (before.integrated(), after.integrated());
    if a.is_finite() && b.is_finite() && a > -70.0 && b > -70.0 {
        (a - b).clamp(-12.0, 12.0)
    } else {
        0.0
    }
}

/// A move on a band of the channel strip's EQ.
struct Move {
    band: usize,
    /// Where it looks for its place; a single frequency when it does not look.
    range: (f32, f32),
    /// Nominal gain, dB.
    gain: f32,
    q: f32,
    bell: bool,
    purpose: &'static str,
}

const fn bell(band: usize, lo: f32, hi: f32, gain: f32, q: f32, purpose: &'static str) -> Move {
    Move {
        band,
        range: (lo, hi),
        gain,
        q,
        bell: true,
        purpose,
    }
}

const fn shelf(band: usize, freq: f32, gain: f32, purpose: &'static str) -> Move {
    Move {
        band,
        range: (freq, freq),
        gain,
        q: 0.7,
        bell: false,
        purpose,
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
    /// Gate range and release, when a gate may help, and where its threshold sits between the
    /// floor between the hits and the hits (0 at the floor).
    gate: Option<(f32, f32, f32)>,
    /// Strada-Transient's attack and sustain (%), on percussive sounds.
    transient: Option<(f32, f32)>,
    comp: Comp,
    moves: &'static [Move],
}

fn rules(i: Instrument) -> Rules {
    match i {
        VocalMale => Rules {
            hpf: (70.0, 110.0),
            steep: true,
            lpf: None,
            zone: (180.0, 8000.0),
            gate: None,
            transient: None,
            comp: Comp(3.0, 10.0, 100.0, 4.0, 4.0),
            moves: &const {
                [
                    bell(0, 100.0, 180.0, 1.5, 1.0, "Warmth"),
                    bell(1, 220.0, 450.0, -3.5, 1.4, "Mud"),
                    bell(2, 2500.0, 4500.0, 3.5, 1.0, "Presence"),
                    shelf(3, 10_000.0, 3.0, "Air"),
                ]
            },
        },
        VocalFemale => Rules {
            hpf: (90.0, 160.0),
            steep: true,
            lpf: None,
            zone: (250.0, 9000.0),
            gate: None,
            transient: None,
            comp: Comp(3.0, 10.0, 100.0, 4.0, 4.0),
            moves: &const {
                [
                    shelf(0, 180.0, -1.5, "Boom"),
                    bell(1, 300.0, 600.0, -3.0, 1.4, "Mud"),
                    bell(2, 3000.0, 5500.0, 3.5, 1.0, "Presence"),
                    shelf(3, 12_000.0, 3.5, "Air"),
                ]
            },
        },
        Kick => Rules {
            hpf: (20.0, 28.0),
            steep: false,
            lpf: Some((7000.0, 10_000.0)),
            zone: (120.0, 1000.0),
            gate: Some((-30.0, 120.0, 0.35)),
            transient: Some((25.0, -10.0)),
            comp: Comp(4.0, 20.0, 80.0, 3.5, 0.0),
            moves: &const {
                [
                    bell(0, 40.0, 65.0, 4.5, 1.0, "Weight"),
                    bell(1, 220.0, 320.0, -5.0, 1.5, "Mud"),
                    bell(2, 2000.0, 3500.0, 4.0, 1.2, "Click"),
                ]
            },
        },
        Snare => Rules {
            hpf: (60.0, 120.0),
            steep: false,
            lpf: None,
            zone: (300.0, 1500.0),
            gate: Some((-15.0, 150.0, 0.35)),
            transient: Some((25.0, 0.0)),
            comp: Comp(4.0, 15.0, 60.0, 3.0, 0.0),
            moves: &const {
                [
                    bell(0, 150.0, 250.0, 3.0, 1.0, "Body"),
                    bell(1, 400.0, 900.0, -3.5, 1.5, "Boxiness"),
                    bell(2, 2500.0, 5000.0, 3.5, 1.0, "Crack"),
                    shelf(3, 10_000.0, 2.5, "Air"),
                ]
            },
        },
        Toms => Rules {
            hpf: (40.0, 80.0),
            steep: false,
            lpf: None,
            zone: (150.0, 1000.0),
            gate: Some((-20.0, 250.0, 0.35)),
            transient: Some((20.0, -20.0)),
            comp: Comp(3.0, 15.0, 120.0, 3.0, 4.0),
            moves: &const {
                [
                    bell(0, 70.0, 150.0, 3.5, 1.0, "Weight"),
                    bell(1, 300.0, 600.0, -5.0, 1.4, "Boxiness"),
                    bell(2, 3000.0, 5000.0, 3.5, 1.0, "Attack"),
                    shelf(3, 9000.0, -2.5, "Bleed"),
                ]
            },
        },
        HiHat => Rules {
            hpf: (200.0, 500.0),
            steep: true,
            lpf: None,
            zone: (1000.0, 10_000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 20.0, 100.0, 2.0, 9.0),
            moves: &const {
                [
                    bell(1, 500.0, 1200.0, -3.0, 1.2, "Clang"),
                    bell(2, 2500.0, 5000.0, -2.5, 1.2, "Harshness"),
                    shelf(3, 10_000.0, 3.0, "Air"),
                ]
            },
        },
        Overheads => Rules {
            hpf: (80.0, 250.0),
            steep: true,
            lpf: None,
            zone: (300.0, 8000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 30.0, 150.0, 2.5, 6.0),
            moves: &const {
                [
                    shelf(0, 150.0, -2.5, "Boom"),
                    bell(1, 300.0, 600.0, -3.5, 1.2, "Boxiness"),
                    bell(2, 2500.0, 5000.0, -2.0, 1.2, "Harshness"),
                    shelf(3, 12_000.0, 3.0, "Air"),
                ]
            },
        },
        Drums => Rules {
            hpf: (20.0, 30.0),
            steep: false,
            lpf: None,
            zone: (200.0, 6000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 30.0, 120.0, 3.0, 4.0),
            moves: &const {
                [
                    bell(0, 50.0, 90.0, 3.0, 1.0, "Weight"),
                    bell(1, 300.0, 600.0, -3.5, 1.2, "Boxiness"),
                    bell(2, 3000.0, 5000.0, 2.0, 1.0, "Attack"),
                    shelf(3, 12_000.0, 2.5, "Air"),
                ]
            },
        },
        Bass => Rules {
            hpf: (25.0, 40.0),
            steep: false,
            lpf: Some((5000.0, 12_000.0)),
            zone: (150.0, 3000.0),
            gate: None,
            transient: None,
            comp: Comp(4.0, 25.0, 120.0, 4.0, 0.0),
            moves: &const {
                [
                    bell(0, 60.0, 110.0, 3.0, 1.0, "Weight"),
                    bell(1, 200.0, 350.0, -3.5, 1.4, "Mud"),
                    bell(2, 700.0, 1500.0, 3.0, 1.0, "Growl"),
                ]
            },
        },
        ElectricGuitar => Rules {
            hpf: (70.0, 120.0),
            steep: true,
            lpf: Some((7000.0, 12_000.0)),
            zone: (150.0, 6000.0),
            gate: Some((-15.0, 150.0, 0.15)),
            transient: None,
            comp: Comp(2.5, 25.0, 200.0, 2.5, 5.0),
            moves: &const {
                [
                    shelf(0, 120.0, -2.5, "Boom"),
                    bell(1, 300.0, 600.0, -3.0, 1.3, "Mud"),
                    bell(2, 1500.0, 3000.0, 3.0, 1.0, "Bite"),
                    shelf(3, 8000.0, -2.0, "Fizz"),
                ]
            },
        },
        AcousticGuitar => Rules {
            hpf: (70.0, 140.0),
            steep: true,
            lpf: None,
            zone: (100.0, 6000.0),
            gate: None,
            transient: None,
            comp: Comp(3.0, 15.0, 100.0, 3.0, 4.0),
            moves: &const {
                [
                    shelf(0, 150.0, -2.5, "Boom"),
                    bell(1, 200.0, 400.0, -3.5, 1.4, "Mud"),
                    bell(2, 2500.0, 5000.0, 2.5, 1.0, "Presence"),
                    shelf(3, 12_000.0, 3.5, "Air"),
                ]
            },
        },
        Keys => Rules {
            hpf: (30.0, 70.0),
            steep: false,
            lpf: None,
            zone: (150.0, 6000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 20.0, 150.0, 2.5, 6.0),
            moves: &const {
                [
                    shelf(0, 120.0, -1.5, "Boom"),
                    bell(1, 250.0, 500.0, -2.5, 1.2, "Mud"),
                    bell(2, 2000.0, 4000.0, 2.5, 1.0, "Presence"),
                    shelf(3, 10_000.0, 2.5, "Air"),
                ]
            },
        },
        Synth => Rules {
            hpf: (30.0, 120.0),
            steep: false,
            lpf: None,
            zone: (150.0, 8000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 20.0, 150.0, 2.5, 7.0),
            moves: &const {
                [
                    bell(1, 250.0, 500.0, -2.5, 1.2, "Mud"),
                    bell(2, 2000.0, 5000.0, 2.0, 1.0, "Presence"),
                    shelf(3, 10_000.0, 2.5, "Air"),
                ]
            },
        },
        Strings => Rules {
            hpf: (40.0, 100.0),
            steep: false,
            lpf: None,
            zone: (200.0, 8000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 30.0, 200.0, 2.0, 7.0),
            moves: &const {
                [
                    shelf(0, 150.0, -2.0, "Boom"),
                    bell(1, 250.0, 500.0, -3.0, 1.2, "Mud"),
                    bell(2, 2000.0, 4000.0, 2.0, 1.0, "Presence"),
                    shelf(3, 10_000.0, 3.0, "Air"),
                ]
            },
        },
        Brass => Rules {
            hpf: (60.0, 120.0),
            steep: false,
            lpf: None,
            zone: (200.0, 6000.0),
            gate: None,
            transient: None,
            comp: Comp(3.0, 10.0, 120.0, 3.0, 5.0),
            moves: &const {
                [
                    shelf(0, 150.0, -2.0, "Boom"),
                    bell(1, 400.0, 800.0, -2.5, 1.2, "Boxiness"),
                    bell(2, 1500.0, 3000.0, 2.5, 1.0, "Bite"),
                    shelf(3, 8000.0, -1.5, "Harshness"),
                ]
            },
        },
        Other => Rules {
            hpf: (30.0, 80.0),
            steep: false,
            lpf: None,
            zone: (150.0, 8000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 20.0, 150.0, 2.5, 6.0),
            moves: &const {
                [
                    bell(1, 250.0, 500.0, -2.5, 1.2, "Mud"),
                    bell(2, 2000.0, 4000.0, 2.0, 1.0, "Presence"),
                    shelf(3, 10_000.0, 2.0, "Air"),
                ]
            },
        },
        BackingVocals => Rules {
            hpf: (100.0, 180.0),
            steep: true,
            lpf: None,
            zone: (200.0, 9000.0),
            gate: None,
            transient: None,
            comp: Comp(4.0, 10.0, 100.0, 4.5, 3.0),
            moves: &const {
                [
                    shelf(0, 200.0, -2.0, "Boom"),
                    bell(1, 250.0, 500.0, -3.5, 1.4, "Mud"),
                    bell(2, 2000.0, 4000.0, 1.5, 1.0, "Presence"),
                    shelf(3, 12_000.0, 3.0, "Air"),
                ]
            },
        },
        HandDrums => Rules {
            hpf: (60.0, 100.0),
            steep: false,
            lpf: None,
            zone: (200.0, 2000.0),
            gate: Some((-12.0, 150.0, 0.35)),
            transient: Some((20.0, 0.0)),
            comp: Comp(3.0, 10.0, 100.0, 3.0, 0.0),
            moves: &const {
                [
                    bell(0, 150.0, 250.0, 2.5, 1.0, "Body"),
                    bell(1, 400.0, 800.0, -3.0, 1.4, "Boxiness"),
                    bell(2, 2000.0, 5000.0, 3.0, 1.0, "Slap"),
                    shelf(3, 10_000.0, 2.0, "Air"),
                ]
            },
        },
        Cajon => Rules {
            hpf: (35.0, 55.0),
            steep: false,
            lpf: None,
            zone: (150.0, 1500.0),
            gate: None,
            transient: Some((25.0, -10.0)),
            comp: Comp(4.0, 15.0, 80.0, 3.5, 0.0),
            moves: &const {
                [
                    bell(0, 70.0, 110.0, 3.5, 1.0, "Thump"),
                    bell(1, 300.0, 600.0, -3.5, 1.4, "Boxiness"),
                    bell(2, 2500.0, 5000.0, 3.0, 1.0, "Slap"),
                ]
            },
        },
        Shaker => Rules {
            hpf: (250.0, 500.0),
            steep: true,
            lpf: None,
            zone: (1500.0, 10_000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 20.0, 100.0, 2.0, 9.0),
            moves: &const {
                [
                    bell(2, 3000.0, 6000.0, -2.0, 1.2, "Harshness"),
                    shelf(3, 12_000.0, 2.0, "Air"),
                ]
            },
        },
        Tambourine => Rules {
            hpf: (250.0, 500.0),
            steep: true,
            lpf: None,
            zone: (1000.0, 10_000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 15.0, 100.0, 2.0, 9.0),
            moves: &const {
                [
                    bell(1, 800.0, 2000.0, -2.0, 1.2, "Clang"),
                    bell(2, 3000.0, 6000.0, -2.0, 1.2, "Harshness"),
                    shelf(3, 12_000.0, 2.5, "Air"),
                ]
            },
        },
        Cowbell => Rules {
            hpf: (200.0, 400.0),
            steep: true,
            lpf: None,
            zone: (500.0, 5000.0),
            gate: None,
            transient: None,
            comp: Comp(2.0, 15.0, 100.0, 2.0, 9.0),
            moves: &const {
                [
                    bell(1, 600.0, 1200.0, -2.5, 1.4, "Clang"),
                    bell(2, 2000.0, 4000.0, 1.5, 1.0, "Presence"),
                ]
            },
        },
        Claps => Rules {
            hpf: (120.0, 250.0),
            steep: true,
            lpf: None,
            zone: (300.0, 6000.0),
            gate: None,
            transient: Some((15.0, 0.0)),
            comp: Comp(3.0, 10.0, 100.0, 3.0, 0.0),
            moves: &const {
                [
                    bell(1, 800.0, 1500.0, 2.0, 1.0, "Body"),
                    bell(2, 3000.0, 5000.0, 2.0, 1.0, "Snap"),
                    shelf(3, 10_000.0, 2.0, "Air"),
                ]
            },
        },
    }
}

/// The filters: just under the lowest and above the highest energy that counts, within the
/// ranges given.
fn filters(a: &Analysis, hpf: (f32, f32), lpf: Option<(f32, f32)>, steep: bool) -> Option<Step> {
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
    let hpf = lowest
        .map(|f| (f * 0.8).clamp(hpf.0, hpf.1))
        .filter(|f| *f > 21.0);
    let lpf = lpf.and_then(|(lo, hi)| highest.map(|f| (f * 1.4).clamp(lo, hi)));
    (hpf.is_some() || lpf.is_some()).then_some(Step::Filters { hpf, lpf, steep })
}

/// The cleaning of a track: filters, resonances, plosives and sibilance, a gate when there is
/// bleed, and the suggestion to strip long silences.
pub fn clean(a: &Analysis, instrument: Instrument, strength: Strength) -> Vec<Step> {
    let rules = rules(instrument);
    let k = strength.factor();
    let mut steps: Vec<Step> = filters(a, rules.hpf, rules.lpf, rules.steep)
        .into_iter()
        .collect();
    let mut cuts = 0;
    for r in &a.resonances {
        if cuts == 4 || !(rules.zone.0..=rules.zone.1).contains(&r.freq) || r.above < 6.0 {
            continue;
        }
        let depth = (-(r.above - 2.0).clamp(2.0, 8.0) * k).max(-12.0);
        // Always there: a fixed cut; else only while it stands out.
        let threshold = (r.spread > 4.0).then(|| (r.usual + 2.0).clamp(-60.0, 0.0));
        steps.push(Step::Resonance(Cut {
            freq: r.freq,
            depth: (depth * 2.0).round() / 2.0,
            threshold,
        }));
        cuts += 1;
    }
    if instrument.is_vocal()
        && let Some(threshold) = a.plosives
    {
        steps.push(Step::Plosives(Cut {
            freq: PLOSIVE_HZ,
            depth: (-8.0 * k).max(-12.0),
            threshold: Some(threshold.clamp(-60.0, 0.0)),
        }));
    }
    if let Some(s) = a.sibilance {
        let sure = instrument.is_vocal() && (0.005..0.4).contains(&s.share);
        if sure || matches!(instrument, Overheads | HiHat) {
            steps.push(Step::DeEss {
                freq: (s.freq * 0.85).clamp(4000.0, 9000.0),
                threshold: (s.level - 4.0 * k).clamp(-30.0, -1.0),
                range: (8.0 * k).min(16.0),
                sure,
            });
        }
    }
    // Something between the hits (bleed, hiss), well under them.
    if let (Some((range, release, at)), Some((floor, hits))) = (rules.gate, a.floor)
        && hits - floor > 12.0
        && floor > -80.0
    {
        steps.push(Step::Gate {
            threshold: (floor + (hits - floor) * at).clamp(-80.0, 0.0),
            range,
            release,
        });
    }
    if a.silence > 0.25 && a.report.seconds > 10.0 {
        steps.push(Step::StripSilence(a.silence));
    }
    steps
}

/// Bringing a track out: the instrument's usual moves on the channel strip's EQ, each placed
/// and dosed on what the track has there, a compressor, and the riding of its level (offered).
pub fn bring_out(
    a: &Analysis,
    instrument: Instrument,
    strength: Strength,
    flavor: Flavor,
) -> Vec<Step> {
    let rules = rules(instrument);
    let k = strength.factor();
    let r = &a.report;
    let mut steps = place_moves(a, instrument, rules.moves, k, flavor);
    let Comp(ratio, attack, release, reduction, lra) = rules.comp;
    let moves = lra == 0.0 || r.loudness_range.map_or(r.crest > 20.0, |v| v > lra);
    if moves && r.integrated.is_some() {
        let attack = attack * flavor.get(|t| t.attack);
        let reduction = reduction * k * flavor.get(|t| t.comp);
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

/// The EQ moves, each placed where the track has the most of it and dosed on how much.
fn place_moves(
    a: &Analysis,
    instrument: Instrument,
    moves: &[Move],
    k: f32,
    flavor: Flavor,
) -> Vec<Step> {
    let r = &a.report;
    let mut steps = Vec::new();
    let bump = bumps(&r.curve);
    for m in moves {
        // Where the track has the most of it, and how much it stands out there.
        let found = r
            .curve
            .iter()
            .zip(&bump)
            .filter(|(c, _)| (m.range.0..=m.range.1).contains(&c.0))
            .max_by(|x, y| x.1.total_cmp(y.1))
            .map(|(c, d)| (c.0, *d));
        let (freq, amount) = match found {
            Some((freq, d)) if m.range.0 < m.range.1 => (
                freq,
                if m.gain < 0.0 {
                    // The bigger the bump, the deeper the cut.
                    (0.6 + 0.2 * d).clamp(0.5, 1.6)
                } else {
                    // Already standing out: less of a boost.
                    (1.0 - 0.1 * d).clamp(0.8, 1.2)
                },
            ),
            _ => (m.range.0, 1.0),
        };
        let gain = m.gain * amount * k * flavor.of_move(instrument, m.purpose, m.gain);
        let gain = (gain.clamp(-9.0, 9.0) * 2.0).round() / 2.0;
        if gain.abs() >= 1.0 {
            steps.push(Step::Eq {
                band: m.band,
                freq: freq.round(),
                gain,
                q: m.q,
                bell: m.bell,
                purpose: m.purpose,
            });
        }
    }
    steps
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

/// The style asked of the mix: a variable of every move's dose and of the dynamics, and of the
/// balance between the instruments (the console color stays a choice of its own). `Natural`
/// asks nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Character {
    #[default]
    Natural,
    Pop,
    Rock,
    Acoustic,
    /// Electronic music and hip-hop.
    Electronic,
}

/// What a style does, as factors of the usual dose (1: unchanged).
#[derive(Clone, Copy, Debug)]
pub struct Traits {
    /// Low boosts: weight, warmth, body.
    pub weight: f32,
    /// Cuts of mud and boxiness.
    pub mud: f32,
    /// Boosts of presence, bite, click, attack.
    pub presence: f32,
    /// Air on top.
    pub air: f32,
    /// Cuts of harshness, fizz, clang.
    pub harsh: f32,
    /// Compression's reduction, and its attack time.
    pub comp: f32,
    pub attack: f32,
    /// Transient shaping.
    pub transient: f32,
    /// The voices brought forward: their presence and air.
    pub vocals: f32,
    /// The kick and the bass: their weight and click.
    pub rhythm: f32,
}

impl Character {
    /// The styles offered, `Natural` first: none of them.
    pub const STYLES: [Character; 5] = [
        Character::Natural,
        Character::Pop,
        Character::Rock,
        Character::Acoustic,
        Character::Electronic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Character::Natural => "None",
            Character::Pop => "Pop",
            Character::Rock => "Rock",
            Character::Acoustic => "Acoustic",
            Character::Electronic => "Electro / Hip-hop",
        }
    }

    /// What it sounds like, in English.
    pub fn description(self) -> &'static str {
        match self {
            Character::Natural => "The usual moves, nothing pushed",
            Character::Pop => "Vocals up front, bright and polished, firm compression",
            Character::Rock => "Drums and guitars forward, punch and bite",
            Character::Acoustic => "Natural and open, light compression, soft highs",
            Character::Electronic => "Full lows, kick and bass on top, dense compression",
        }
    }

    pub fn traits(self) -> Traits {
        let t =
            |weight, mud, presence, air, harsh, comp, attack, transient, vocals, rhythm| Traits {
                weight,
                mud,
                presence,
                air,
                harsh,
                comp,
                attack,
                transient,
                vocals,
                rhythm,
            };
        match self {
            Character::Natural => t(1.0, 1.0, 1.0, 1.0, 1.0, 0.8, 1.0, 1.0, 1.0, 1.0),
            Character::Pop => t(1.1, 1.2, 1.2, 1.4, 1.1, 1.3, 1.0, 1.1, 1.4, 1.1),
            Character::Rock => t(1.1, 1.2, 1.4, 1.0, 1.0, 1.3, 0.8, 1.4, 1.0, 1.0),
            Character::Acoustic => t(1.0, 1.0, 0.9, 1.1, 1.2, 0.6, 1.3, 0.7, 1.1, 0.9),
            Character::Electronic => t(1.4, 1.25, 1.05, 1.2, 1.15, 1.45, 0.85, 1.25, 1.2, 1.45),
        }
    }
}

/// A style at an intensity (0: natural, 1: all of it).
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

    /// The factor of an EQ move on `instrument`, by what it is for.
    fn of_move(self, instrument: Instrument, purpose: &str, gain: f32) -> f32 {
        let base = match purpose {
            "Warmth" | "Weight" | "Body" | "Thump" => self.get(|t| t.weight),
            "Mud" | "Boxiness" | "Boom" => self.get(|t| t.mud),
            "Presence" | "Click" | "Crack" | "Attack" | "Bite" | "Slap" | "Snap" | "Growl" => {
                self.get(|t| t.presence)
            }
            "Air" if gain > 0.0 => self.get(|t| t.air),
            "Harshness" | "Fizz" | "Clang" | "Bleed" => self.get(|t| t.harsh),
            _ => 1.0,
        };
        let forward = gain > 0.0 && purpose != "Mud";
        let focus = match instrument {
            VocalMale | VocalFemale if forward => self.get(|t| t.vocals),
            Kick | Bass if forward => self.get(|t| t.rhythm),
            _ => 1.0,
        };
        base * focus
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
        BackingVocals => "backing",
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

/// What a bus of a family calls for: a high-pass range and light moves.
fn bus_rules(family: Instrument) -> ((f32, f32), &'static [Move]) {
    match family.category() {
        "Voices" => (
            (80.0, 120.0),
            &const {
                [
                    bell(1, 250.0, 500.0, -2.0, 1.2, "Mud"),
                    shelf(3, 12_000.0, 2.0, "Air"),
                ]
            },
        ),
        "Drums" => (
            (20.0, 30.0),
            &const {
                [
                    bell(0, 50.0, 90.0, 1.5, 0.8, "Weight"),
                    bell(1, 300.0, 600.0, -2.0, 1.2, "Boxiness"),
                    shelf(3, 10_000.0, 1.5, "Air"),
                ]
            },
        ),
        "Percussion" => (
            (80.0, 150.0),
            &const {
                [
                    bell(2, 3000.0, 6000.0, -1.5, 1.0, "Harshness"),
                    shelf(3, 10_000.0, 1.5, "Air"),
                ]
            },
        ),
        "Bass" => (
            (25.0, 35.0),
            &const { [bell(1, 200.0, 350.0, -1.5, 1.2, "Mud")] },
        ),
        "Guitars" => (
            (60.0, 90.0),
            &const {
                [
                    bell(1, 250.0, 500.0, -2.0, 1.2, "Mud"),
                    bell(2, 2000.0, 4000.0, 1.5, 1.0, "Presence"),
                ]
            },
        ),
        "Keys and synths" | "Strings and winds" => (
            (30.0, 60.0),
            &const {
                [
                    bell(1, 250.0, 500.0, -1.5, 1.2, "Mud"),
                    shelf(3, 10_000.0, 1.5, "Air"),
                ]
            },
        ),
        _ => ((20.0, 30.0), &[]),
    }
}

/// Cleaning a bus: only a gentle high-pass. What is destructive on a sum (gates, de-essers,
/// resonance cuts) belongs to its tracks.
pub fn clean_bus(a: &Analysis, family: Instrument) -> Vec<Step> {
    let (hpf, _) = bus_rules(bus_family(family));
    filters(a, hpf, None, false).into_iter().collect()
}

/// Bringing a bus out: light moves for its family and a slow compressor that glues it.
pub fn bring_out_bus(
    a: &Analysis,
    family: Instrument,
    strength: Strength,
    flavor: Flavor,
) -> Vec<Step> {
    let k = strength.factor();
    let (_, moves) = bus_rules(bus_family(family));
    let mut steps = place_moves(a, family, moves, k, flavor);
    if a.report.integrated.is_some() {
        let attack = 30.0 * flavor.get(|t| t.attack);
        let reduction = 2.0 * k * flavor.get(|t| t.comp);
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

/// The console color that suits an instrument, as an index of [`crate::console::MODELS`]
/// (0: none).
pub fn console(i: Instrument) -> usize {
    const WARM: usize = 1;
    const BRIGHT: usize = 2;
    const PUNCH: usize = 3;
    const SMOOTH: usize = 4;
    match i {
        Kick | Snare | Toms | Drums | HandDrums | Cajon | ElectricGuitar => PUNCH,
        VocalMale | VocalFemale | Bass | Keys => WARM,
        HiHat | Overheads | Shaker | Tambourine | Cowbell | Claps | AcousticGuitar | Synth => {
            BRIGHT
        }
        BackingVocals | Strings | Brass => SMOOTH,
        Other => 0,
    }
}

/// The send (dB) to the shared reverb at full space; `None` for what stays dry, the low end
/// and the close drums that carry the rhythm.
fn space_send(i: Instrument) -> Option<f32> {
    match i {
        BackingVocals | Strings => Some(-10.0),
        VocalMale | VocalFemale | AcousticGuitar | Keys | Brass | Claps => Some(-13.0),
        Snare | Synth | HandDrums => Some(-15.0),
        ElectricGuitar | Toms | Shaker | Tambourine | Cajon => Some(-17.0),
        Cowbell | Drums => Some(-20.0),
        Kick | Bass | HiHat | Overheads | Other => None,
    }
}

/// The send (dB) of an instrument to the shared reverb, at `space` (0 to 1).
pub fn space_send_db(i: Instrument, space: f32) -> Option<f32> {
    let full = space_send(i)?;
    (space > 0.01).then(|| (full + 20.0 * space.min(1.0).log10()).max(-40.0))
}

/// The shared reverb: a short, light hall, its lows and highs cut so that it never muddies.
pub const SPACE_REVERB: [(&str, f32); 12] = [
    ("type", crate::reverb::HALL as f32),
    ("decay", 1.5),
    ("predelay", 25.0),
    ("low_cut", 250.0),
    ("high_cut", 7000.0),
    ("width", 100.0),
    ("mix", 100.0),
    ("size", 45.0),
    ("early", 35.0),
    ("treble", 40.0),
    ("diffusion", 80.0),
    ("modulation", 30.0),
];

/// How far apart (pan, 0 to 1) the tracks of an instrument go when there are several; `None`
/// for what stays in the center.
fn spread_of(i: Instrument) -> Option<f32> {
    match i {
        ElectricGuitar => Some(0.4),
        AcousticGuitar | Shaker | Tambourine => Some(0.35),
        BackingVocals | Synth | Strings | HandDrums | Cowbell => Some(0.3),
        Toms | Keys | Brass => Some(0.25),
        Claps => Some(0.2),
        Overheads => Some(0.5),
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
            *hpf > 100.0 && *hpf <= 110.0,
            "the energy starts high: so does the filter ({hpf})"
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
    fn a_kick_gets_weight_less_mud_a_click_and_a_compressor() {
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
        let steps = bring_out(&a, Kick, Strength::NORMAL, Flavor::default());
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
        let (weight, mud, click) = (eq(0).unwrap(), eq(1).unwrap(), eq(2).unwrap());
        assert!(
            (40.0..=65.0).contains(&weight.0) && weight.1 >= 2.5,
            "{weight:?}"
        );
        assert!((220.0..=320.0).contains(&mud.0) && mud.1 <= -2.5, "{mud:?}");
        assert!(
            (2000.0..=3500.0).contains(&click.0) && click.1 >= 2.0,
            "{click:?}"
        );
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, Step::Compressor { reduction, .. } if *reduction >= 3.0)),
            "a kick is always levelled"
        );
    }

    #[test]
    fn the_space_leaves_the_low_end_dry_and_grows_with_its_knob() {
        for dry in [Kick, Bass, HiHat, Other] {
            assert_eq!(space_send_db(dry, 1.0), None, "{dry:?}");
        }
        assert_eq!(space_send_db(VocalMale, 0.0), None, "off by default");
        let half = space_send_db(VocalMale, 0.5).unwrap();
        let full = space_send_db(VocalMale, 1.0).unwrap();
        assert!(half < full && full <= -10.0);
        assert!(space_send_db(BackingVocals, 1.0) > space_send_db(VocalMale, 1.0));
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
        assert_eq!(pans[0], Some(0.4));
        assert_eq!(pans[2], Some(-0.4));
        assert_eq!((pans[1], pans[3], pans[4]), (None, None, None));
        assert_eq!(
            (pans[5], pans[6], pans[7]),
            (Some(-0.25), Some(0.0), Some(0.25))
        );
        assert!(super::pans(&tracks, 0.0).iter().all(Option::is_none));
        assert_eq!(super::pans(&tracks, 0.5)[0], Some(0.2));
    }

    #[test]
    fn a_bus_gets_a_high_pass_and_some_glue_never_what_its_tracks_get() {
        let left = noise(6.0, |x| x);
        let a = analyse(&left, &left, RATE);
        assert!(
            clean_bus(&a, Kick)
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
    fn a_pop_mix_brings_the_voice_forward_more_than_a_natural_one() {
        let left = noise(6.0, |x| x);
        let a = analyse(&left, &left, RATE);
        let gain = |flavor: Flavor, purpose: &str| -> f32 {
            bring_out(&a, VocalMale, Strength::NORMAL, flavor)
                .iter()
                .find_map(|s| match s {
                    Step::Eq {
                        gain, purpose: p, ..
                    } if *p == purpose => Some(*gain),
                    _ => None,
                })
                .unwrap_or(0.0)
        };
        let natural = Flavor::default();
        let pop = Flavor {
            character: Character::Pop,
            amount: 1.0,
        };
        assert!(gain(pop, "Presence") > gain(natural, "Presence"));
        assert!(gain(pop, "Air") > gain(natural, "Air"));
        let none = Flavor {
            character: Character::Pop,
            amount: 0.0,
        };
        assert_eq!(
            gain(none, "Air"),
            gain(natural, "Air"),
            "no intensity, no change"
        );
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
            bring_out(&a, VocalMale, s, Flavor::default())
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
        let gain = level_kept(&x, RATE, 0.0, &strip_settings(boost.iter()), &[]);
        assert!(gain < -1.0, "a big boost is brought back down ({gain})");
        assert_eq!(
            level_kept(&x, RATE, 0.0, &[], &[]),
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
}
