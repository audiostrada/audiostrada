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
}

use Instrument::*;

/// Above this fundamental (Hz, the median through the track), a voice is taken for a woman's.
const FEMALE_FROM: f32 = 165.0;

impl Instrument {
    pub const ALL: [Instrument; 16] = [
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
    ];

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
        }
    }

    pub fn is_vocal(self) -> bool {
        matches!(self, VocalMale | VocalFemale)
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
            "vox", "vocal", "vocals", "voc", "voice", "voix", "chant", "lead", "bv", "bvs",
            "choir", "choeur", "chœur", "choeurs", "backing", "sing", "singer",
        ]) {
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

/// How far the rules go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Strength {
    Light,
    #[default]
    Normal,
    Strong,
}

impl Strength {
    pub const ALL: [Strength; 3] = [Strength::Light, Strength::Normal, Strength::Strong];

    pub fn name(self) -> &'static str {
        match self {
            Strength::Light => "Gentle",
            Strength::Normal => "Normal",
            Strength::Strong => "Bold",
        }
    }

    fn factor(self) -> f32 {
        match self {
            Strength::Light => 0.65,
            Strength::Normal => 1.0,
            Strength::Strong => 1.4,
        }
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
            Step::Eq { .. } | Step::Compressor { .. } | Step::Ride(_)
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
            Step::Ride(points) => Step::Ride(
                points
                    .iter()
                    .map(|&(t, db)| (t, (db * amount).clamp(-9.0, 9.0)))
                    .collect(),
            ),
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
    let (mut eq, mut deess) = (Vec::new(), None);
    let mut n = 0;
    for step in steps {
        let (cut, kind, q) = match step {
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
    out
}

/// The gain (dB) bringing `excerpt`, once through the channel strip set with `strip` then
/// through `inserts`, back to the loudness it had.
pub fn level_kept(
    excerpt: &[f32],
    rate: u32,
    strip: &[(String, f32)],
    inserts: &[(&'static str, Vec<(String, f32)>)],
) -> f32 {
    use crate::loudness::LevelMeter;
    let (mut left, mut right) = (excerpt.to_vec(), excerpt.to_vec());
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
    /// Gate range and release, when a gate may help.
    gate: Option<(f32, f32)>,
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
            gate: Some((-30.0, 120.0)),
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
            gate: None,
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
            gate: Some((-20.0, 250.0)),
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
            gate: None,
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
            comp: Comp(2.0, 20.0, 150.0, 2.5, 6.0),
            moves: &const {
                [
                    bell(1, 250.0, 500.0, -2.5, 1.2, "Mud"),
                    bell(2, 2000.0, 4000.0, 2.0, 1.0, "Presence"),
                    shelf(3, 10_000.0, 2.0, "Air"),
                ]
            },
        },
    }
}

/// The cleaning of a track: filters, resonances, plosives and sibilance, a gate when there is
/// bleed, and the suggestion to strip long silences.
pub fn clean(a: &Analysis, instrument: Instrument, strength: Strength) -> Vec<Step> {
    let rules = rules(instrument);
    let k = strength.factor();
    let curve = &a.report.curve;
    let mut steps = Vec::new();
    let top = curve
        .iter()
        .filter(|p| (20.0..=2000.0).contains(&p.0))
        .map(|p| p.1)
        .fold(f32::MIN, f32::max);
    if top > f32::MIN {
        // Just under the lowest and above the highest energy that counts.
        let lowest = curve.iter().find(|p| p.1 >= top - 24.0).map(|p| p.0);
        let highest = curve.iter().rev().find(|p| p.1 >= top - 40.0).map(|p| p.0);
        let hpf = lowest
            .map(|f| (f * 0.8).clamp(rules.hpf.0, rules.hpf.1))
            .filter(|f| *f > 21.0);
        let lpf = rules
            .lpf
            .and_then(|(lo, hi)| highest.map(|f| (f * 1.4).clamp(lo, hi)));
        if hpf.is_some() || lpf.is_some() {
            steps.push(Step::Filters {
                hpf,
                lpf,
                steep: rules.steep,
            });
        }
    }
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
    if let (Some((range, release)), Some((floor, hits))) = (rules.gate, a.floor)
        && hits - floor > 18.0
        && floor > -75.0
    {
        steps.push(Step::Gate {
            threshold: (floor + (hits - floor) * 0.35).clamp(-80.0, 0.0),
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
pub fn bring_out(a: &Analysis, instrument: Instrument, strength: Strength) -> Vec<Step> {
    let rules = rules(instrument);
    let k = strength.factor();
    let r = &a.report;
    let mut steps = Vec::new();
    let bump = bumps(&r.curve);
    for m in rules.moves {
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
        let gain = ((m.gain * amount * k).clamp(-9.0, 9.0) * 2.0).round() / 2.0;
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
    let Comp(ratio, attack, release, reduction, lra) = rules.comp;
    let moves = lra == 0.0 || r.loudness_range.map_or(r.crest > 20.0, |v| v > lra);
    if moves && r.integrated.is_some() {
        let reduction = reduction * k;
        steps.push(Step::Compressor {
            ratio,
            attack,
            release,
            threshold: (a.peaks - reduction * ratio / (ratio - 1.0)).clamp(-60.0, -1.0),
            reduction,
        });
    }
    if let Some(points) = riding(&a.levels, k) {
        steps.push(Step::Ride(points));
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
        for i in Instrument::ALL {
            assert_eq!(Instrument::from_id(i.id()), Some(i));
        }
    }

    #[test]
    fn the_high_pass_sits_under_the_energy_that_counts() {
        let x = noise(6.0, |x| x);
        let left = high_passed(&x, RATE, 300.0);
        let a = analyse(&left, &left, RATE);
        let steps = clean(&a, VocalMale, Strength::Normal);
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
        let cut = clean(&a, VocalMale, Strength::Normal)
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
        let steps = bring_out(&a, Kick, Strength::Normal);
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
    fn stronger_goes_further() {
        let left = noise(6.0, |x| x);
        let a = analyse(&left, &left, RATE);
        let total = |s: Strength| -> f32 {
            bring_out(&a, VocalMale, s)
                .iter()
                .map(|s| match s {
                    Step::Eq { gain, .. } => gain.abs(),
                    _ => 0.0,
                })
                .sum()
        };
        assert!(total(Strength::Light) < total(Strength::Normal));
        assert!(total(Strength::Normal) < total(Strength::Strong));
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
        let steps = clean(&a, VocalFemale, Strength::Normal);
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
            bring_out(&analyse(s, s, RATE), Keys, Strength::Normal)
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
        let gain = level_kept(&x, RATE, &strip_settings(boost.iter()), &[]);
        assert!(gain < -1.0, "a big boost is brought back down ({gain})");
        assert_eq!(
            level_kept(&x, RATE, &[], &[]),
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
