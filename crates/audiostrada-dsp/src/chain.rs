//! Chains: one effect running several built-in effects in a row behind a few big knobs
//! (Strada-Master, Strada-Vocals). Their own parameters come first; each stage's follow under
//! its effect id ("eq.b1_freq"). Those are the base the knobs act around: a knob offsets or
//! scales them on their way to the stage and never overwrites them.

use std::sync::LazyLock;

use crate::analyzer::{Analyzer, BANDS};
use crate::loudness::{LevelMeter, SameLoudness};
use crate::{Effect, EffectInfo, ParamInfo, choice, param, switch};

/// The chain's own meters, before its stages': loudness and peaks, what following the target
/// added, then the input and output spectra (computed while the window is open).
pub const IN_LOUDNESS: usize = 0;
pub const OUT_LOUDNESS: usize = 1;
pub const INTEGRATED: usize = 2;
pub const TRUE_PEAK: usize = 3;
pub const IN_PEAK: usize = 4;
pub const OUT_PEAK: usize = 5;
pub const FOLLOWED: usize = 6;
pub const IN_SPECTRUM: usize = 7;
pub const OUT_SPECTRUM: usize = IN_SPECTRUM + BANDS;
pub const OWN_METERS: usize = OUT_SPECTRUM + BANDS;

const METER_NAMES: [&str; OWN_METERS] = {
    let mut names = [""; OWN_METERS];
    names[IN_LOUDNESS] = "Input loudness";
    names[OUT_LOUDNESS] = "Output loudness";
    names[INTEGRATED] = "Integrated loudness";
    names[TRUE_PEAK] = "True peak";
    names[IN_PEAK] = "Input peak";
    names[OUT_PEAK] = "Output peak";
    names[FOLLOWED] = "Followed";
    names
};

/// Parameters for the editor or for listening only: never automated nor shown to a host.
pub const HIDDEN: [&str; 3] = ["order", "assisted", "compare"];

/// Strada-Master's knobs and choices, in the order of its parameters.
pub mod master {
    pub const TONE: usize = 0;
    pub const GLUE: usize = 1;
    pub const COLOR: usize = 2;
    pub const WIDTH: usize = 3;
    pub const SUB: usize = 4;
    pub const LOUDNESS: usize = 5;
    pub const STYLE: usize = 6;
    pub const TARGET: usize = 7;
    pub const FOLLOW: usize = 8;
    pub const ORDER: usize = 9;
    pub const ASSISTED: usize = 10;
    pub const COMPARE: usize = 11;
    pub const STYLES: [&str; 4] = ["Clean", "Warm", "Open", "Punchy"];
    pub const TARGETS: [&str; 7] = [
        "Streaming",
        "Apple Music, podcast",
        "CD",
        "Club",
        "Vinyl",
        "TV, radio",
        "Pre-master",
    ];
    /// Loudness aimed at by each target, in LUFS.
    pub const TARGET_LUFS: [f32; 7] = [-14.0, -16.0, -9.0, -7.0, -12.0, -23.0, -18.0];
    /// The stages, in the order the sound goes through them; the last stays last.
    pub const STAGES: [&str; 6] = ["eq", "mb_comp", "tape", "imager", "maximizer", "limiter"];
    pub const MOVABLE: usize = 5;
}

/// Strada-Vocals' knobs and choices, in the order of its parameters.
pub mod vocals {
    pub const TUNE: usize = 0;
    pub const TONE: usize = 1;
    pub const DRIVE: usize = 2;
    pub const COMP: usize = 3;
    pub const DEESS: usize = 4;
    pub const DOUBLE: usize = 5;
    pub const SPACE: usize = 6;
    pub const VOICE: usize = 7;
    pub const ROOM: usize = 8;
    pub const TAKE: usize = 9;
    pub const ORDER: usize = 10;
    pub const ASSISTED: usize = 11;
    pub const VOICES: [&str; 3] = ["Singing", "Rap", "Spoken"];
    pub const ROOMS: [&str; 5] = ["Room", "Plate", "Hall", "Slapback", "Echo"];
    /// The stages, in the order the sound goes through them.
    pub const STAGES: [&str; 8] = [
        "pitch_correct",
        "eq",
        "compressor",
        "drive",
        "deesser",
        "doubler",
        "delay",
        "reverb",
    ];
    pub const MOVABLE: usize = 8;
    /// Stages left out while recording a take: they add latency or space.
    pub const OFF_IN_TAKES: [&str; 3] = ["pitch_correct", "delay", "reverb"];
}

const fn factorial(n: usize) -> usize {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

const MASTER_OWN: [ParamInfo; 12] = [
    param("tone", "Tone", -100.0, 100.0, 0.0, "%"),
    param("glue", "Mix glue", 0.0, 100.0, 50.0, "%"),
    param("color", "Color", 0.0, 100.0, 50.0, "%"),
    param("width", "Width", 0.0, 200.0, 100.0, "%"),
    param("sub", "Sub", -100.0, 100.0, 0.0, "%"),
    param("loudness", "Loudness", -12.0, 18.0, 4.0, "dB"),
    choice("style", "Style", &master::STYLES, 0.0),
    choice("target", "Target", &master::TARGETS, 0.0),
    switch("follow", "Follow the target", 0.0),
    param(
        "order",
        "Order",
        0.0,
        (factorial(master::MOVABLE) - 1) as f32,
        0.0,
        "",
    ),
    switch("assisted", "Assistant offered", 0.0),
    switch("compare", "Before, same loudness", 0.0),
];

const VOCALS_OWN: [ParamInfo; 12] = [
    param("tune", "Pitch correction", 0.0, 100.0, 0.0, "%"),
    param("tone", "Tone", -100.0, 100.0, 0.0, "%"),
    param("drive", "Saturation", 0.0, 100.0, 0.0, "%"),
    param("comp", "Comp", 0.0, 100.0, 50.0, "%"),
    param("deess", "De-ess", 0.0, 100.0, 50.0, "%"),
    param("double", "Double", 0.0, 100.0, 0.0, "%"),
    param("space", "Space", 0.0, 100.0, 50.0, "%"),
    choice("voice", "Voice", &vocals::VOICES, 0.0),
    choice("room", "Space type", &vocals::ROOMS, 1.0),
    switch("take", "Take", 0.0),
    param(
        "order",
        "Order",
        0.0,
        (factorial(vocals::MOVABLE) - 1) as f32,
        0.0,
        "",
    ),
    switch("assisted", "Assistant offered", 0.0),
];

/// The stages' starting settings in Strada-Master, where they differ from the effects' own.
const MASTER_DEFAULTS: &[(&str, f32)] = &[
    ("mb_comp.b1_thresh", -20.0),
    ("mb_comp.b2_thresh", -22.0),
    ("mb_comp.b3_thresh", -24.0),
    ("mb_comp.b1_ratio", 1.5),
    ("mb_comp.b2_ratio", 1.6),
    ("mb_comp.b3_ratio", 1.8),
    ("tape.machine", 1.0),
    ("tape.speed", 2.0),
    ("tape.tape", 0.0),
    ("tape.mix", 60.0),
    ("imager.mid_width", 100.0),
    ("imager.high_width", 115.0),
    ("eq.b13_used", 1.0),
    ("eq.b13_type", 2.0),
    ("eq.b13_freq", 12_000.0),
    ("eq.b13_q", 0.7),
    ("eq.b14_used", 1.0),
    ("eq.b14_type", 1.0),
    ("eq.b14_freq", 200.0),
    ("eq.b14_q", 0.6),
    ("eq.b15_used", 1.0),
    ("eq.b15_type", 2.0),
    ("eq.b15_freq", 4_000.0),
    ("eq.b15_q", 0.6),
    ("eq.b16_used", 1.0),
    ("eq.b16_type", 1.0),
    ("eq.b16_freq", 70.0),
    ("eq.b16_q", 0.7),
    ("maximizer.gain", 0.0),
    ("limiter.ceiling", -1.0),
    ("limiter.true_peak", 1.0),
    ("limiter.auto", 1.0),
];

/// The stages' starting settings in Strada-Vocals, where they differ from the effects' own.
const VOCALS_DEFAULTS: &[(&str, f32)] = &[
    ("pitch_correct.speed", 40.0),
    ("pitch_correct.humanize", 30.0),
    ("eq.b1_used", 1.0),
    ("eq.b1_type", 3.0),
    ("eq.b1_freq", 90.0),
    ("eq.b13_used", 1.0),
    ("eq.b13_type", 2.0),
    ("eq.b13_freq", 12_000.0),
    ("eq.b13_q", 0.7),
    ("eq.b14_used", 1.0),
    ("eq.b14_type", 1.0),
    ("eq.b14_freq", 250.0),
    ("eq.b14_q", 0.6),
    ("eq.b15_used", 1.0),
    ("eq.b15_type", 2.0),
    ("eq.b15_freq", 3_500.0),
    ("eq.b15_q", 0.6),
    ("compressor.threshold", -20.0),
    ("compressor.ratio", 3.0),
    ("compressor.attack", 8.0),
    ("compressor.makeup", 3.0),
    ("drive.mode", 3.0),
    ("drive.drive", 30.0),
    ("drive.auto_gain", 1.0),
    ("deesser.threshold", -32.0),
    ("deesser.range", 8.0),
    ("doubler.mix", 30.0),
    ("delay.sync", 1.0),
    ("delay.mix", 12.0),
    ("delay.feedback", 25.0),
    ("delay.duck", 60.0),
    ("delay.high_cut", 5000.0),
    ("reverb.type", 1.0),
    ("reverb.decay", 1.4),
    ("reverb.mix", 16.0),
    ("reverb.duck", 40.0),
    ("reverb.predelay", 30.0),
];

pub static MASTER: LazyLock<EffectInfo> = LazyLock::new(|| {
    build(
        "mastering",
        "Strada-Master",
        &MASTER_OWN,
        &master::STAGES,
        MASTER_DEFAULTS,
        "Mastering chain: tone, glue, tape color, width, sub and loudness in six big knobs, \
         over Strada-EQ, MB-Comp, Tape, Wide, Max and Limit.",
    )
});

pub static VOCALS: LazyLock<EffectInfo> = LazyLock::new(|| {
    build(
        "vocals",
        "Strada-Vocals",
        &VOCALS_OWN,
        &vocals::STAGES,
        VOCALS_DEFAULTS,
        "Vocal chain: tuning, tone and saturation, compression, de-essing, doubling and space \
         in six big knobs, over Strada-Correct, EQ, Comp, Drive, DeEss, Doubler, Delay and Verb.",
    )
});

fn leak<T>(v: Vec<T>) -> &'static [T] {
    Box::leak(v.into_boxed_slice())
}

fn build(
    id: &'static str,
    name: &'static str,
    own: &'static [ParamInfo],
    stages: &'static [&'static str],
    defaults: &[(&str, f32)],
    description: &'static str,
) -> EffectInfo {
    let mut params = own.to_vec();
    let mut meters = METER_NAMES.to_vec();
    for &stage in stages {
        let info = crate::base_info(stage).expect("a chain stage is a built-in effect");
        for p in info.params {
            let prefixed = format!("{stage}.{}", p.id);
            let default = defaults
                .iter()
                .find(|(k, _)| *k == prefixed)
                .map_or(p.default, |&(_, v)| v);
            params.push(ParamInfo {
                id: Box::leak(prefixed.into_boxed_str()),
                default,
                ..*p
            });
        }
        meters.extend_from_slice(info.meters);
    }
    EffectInfo {
        id,
        name,
        params: leak(params),
        meters: leak(meters),
        instrument: false,
        files: &[],
        category: "Chains",
        description,
    }
}

/// The stage and parameter of a prefixed id: `"eq.b1_freq"` gives `("eq", "b1_freq")`.
pub fn split(id: &str) -> Option<(&str, &str)> {
    id.split_once('.')
}

/// The stages of a chain, in their default order, and how many of them can move.
pub fn stages_of(chain: &str) -> Option<(&'static [&'static str], usize)> {
    match chain {
        "mastering" => Some((&master::STAGES, master::MOVABLE)),
        "vocals" => Some((&vocals::STAGES, vocals::MOVABLE)),
        _ => None,
    }
}

/// The order of the first `n` stages coded as `code` (its rank among the permutations); the
/// stages after them keep their place.
pub fn order(code: f32, n: usize) -> [usize; 8] {
    let mut out: [usize; 8] = std::array::from_fn(|i| i);
    let mut pool: [usize; 8] = std::array::from_fn(|i| i);
    let n = n.min(8);
    let mut code = (code.round().max(0.0) as usize).min(factorial(n) - 1);
    let mut left = n;
    for slot in out.iter_mut().take(n) {
        let f = factorial(left - 1);
        let k = code / f;
        code %= f;
        *slot = pool[k];
        pool.copy_within(k + 1..left, k);
        left -= 1;
    }
    out
}

/// The code of an order of `n` stages, the inverse of [`order`].
pub fn order_code(order: &[usize]) -> f32 {
    let n = order.len();
    let mut code = 0;
    for i in 0..n {
        let smaller = order[i + 1..].iter().filter(|&&o| o < order[i]).count();
        code += smaller * factorial(n - 1 - i);
    }
    code as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Master,
    Vocals,
}

/// How a knob reaches a stage's parameter, the knob's value being `k`.
#[derive(Clone, Copy, Debug)]
enum Rule {
    Base,
    /// `base + (k - center) * scale`.
    Add {
        knob: usize,
        center: f32,
        scale: f32,
    },
    /// `base`, faded out below `full`.
    Fade {
        knob: usize,
        full: f32,
    },
    /// `base * 2^((center - k) / per)`.
    Exp {
        knob: usize,
        center: f32,
        per: f32,
    },
    /// `base` at `at`, nothing at zero, and `above` more of it for each `at` further up.
    Curve {
        knob: usize,
        at: f32,
        above: f32,
    },
    /// `base + k * scale` while `k` is below zero: a trim for what a gain cannot lower.
    Below {
        knob: usize,
        scale: f32,
    },
    /// `base + k * scale` while `k` is above zero.
    Above {
        knob: usize,
        scale: f32,
    },
}

impl Rule {
    fn knob(self) -> Option<usize> {
        match self {
            Rule::Base => None,
            Rule::Add { knob, .. }
            | Rule::Fade { knob, .. }
            | Rule::Exp { knob, .. }
            | Rule::Curve { knob, .. }
            | Rule::Below { knob, .. }
            | Rule::Above { knob, .. } => Some(knob),
        }
    }

    fn apply(self, base: f32, knobs: &[f32]) -> f32 {
        match self {
            Rule::Base => base,
            Rule::Add {
                knob,
                center,
                scale,
            } => base + (knobs[knob] - center) * scale,
            Rule::Fade { knob, full } => base * (knobs[knob] / full).clamp(0.0, 1.0),
            Rule::Exp { knob, center, per } => base * 2f32.powf((center - knobs[knob]) / per),
            Rule::Curve { knob, at, above } => {
                let k = knobs[knob] / at;
                base * if k < 1.0 {
                    k.max(0.0)
                } else {
                    1.0 + (k - 1.0) * above
                }
            }
            Rule::Below { knob, scale } => base + knobs[knob].min(0.0) * scale,
            Rule::Above { knob, scale } => base + knobs[knob].max(0.0) * scale,
        }
    }

    /// The base that plays `value` with the knobs as they are; a knob that silences the base
    /// leaves no way back, and `value` is kept.
    fn base(self, value: f32, knobs: &[f32]) -> f32 {
        let factor = |f: f32| if f > 1e-6 { value / f } else { value };
        match self {
            Rule::Base => value,
            Rule::Add { .. } | Rule::Below { .. } | Rule::Above { .. } => {
                value - (self.apply(0.0, knobs))
            }
            Rule::Fade { .. } | Rule::Exp { .. } | Rule::Curve { .. } => {
                factor(self.apply(1.0, knobs))
            }
        }
    }
}

fn kind_of(chain: &str) -> Option<Kind> {
    match chain {
        "mastering" => Some(Kind::Master),
        "vocals" => Some(Kind::Vocals),
        _ => None,
    }
}

/// What a stage parameter of `chain` (`"eq.b1_gain"`) plays with its `base` and the chain's
/// knobs, `own` being the chain's own values in order.
pub fn played(chain: &str, id: &str, base: f32, own: &[f32]) -> f32 {
    let (Some(kind), Some((stage, param))) = (kind_of(chain), split(id)) else {
        return base;
    };
    let [a, b] = rules(kind, stage, param);
    b.apply(a.apply(base, own), own)
}

/// The base of a stage parameter that plays `value` with the knobs as they are: the inverse of
/// [`played`], for an editor showing what is played.
pub fn base_for(chain: &str, id: &str, value: f32, own: &[f32]) -> f32 {
    let (Some(kind), Some((stage, param))) = (kind_of(chain), split(id)) else {
        return value;
    };
    let [a, b] = rules(kind, stage, param);
    a.base(b.base(value, own), own)
}

/// The knobs reaching a stage's parameter, at most two.
fn rules(kind: Kind, stage: &str, id: &str) -> [Rule; 2] {
    use Rule::*;
    let one = |r| [r, Base];
    match kind {
        Kind::Master => {
            use master::*;
            match (stage, id) {
                ("mb_comp", t) if t.ends_with("_thresh") => one(Add {
                    knob: GLUE,
                    center: 50.0,
                    scale: -0.09,
                }),
                ("mb_comp", "mix") => one(Fade {
                    knob: GLUE,
                    full: 25.0,
                }),
                ("mb_comp", "output") => one(Add {
                    knob: GLUE,
                    center: 50.0,
                    scale: 0.025,
                }),
                ("tape", "drive") => one(Add {
                    knob: COLOR,
                    center: 50.0,
                    scale: 0.12,
                }),
                ("tape", "output") => one(Add {
                    knob: COLOR,
                    center: 50.0,
                    scale: -0.06,
                }),
                ("tape", "mix") => one(Fade {
                    knob: COLOR,
                    full: 25.0,
                }),
                ("imager", "mid_width" | "high_width") => one(Curve {
                    knob: WIDTH,
                    at: 100.0,
                    above: 0.75,
                }),
                ("eq", "b14_gain") => one(Add {
                    knob: TONE,
                    center: 0.0,
                    scale: -0.0225,
                }),
                ("eq", "b15_gain") => one(Add {
                    knob: TONE,
                    center: 0.0,
                    scale: 0.0225,
                }),
                ("eq", "b13_gain") => one(Above {
                    knob: TONE,
                    scale: 0.011,
                }),
                // The last band of the EQ is the Sub knob's low shelf.
                ("eq", "b16_gain") => one(Add {
                    knob: SUB,
                    center: 0.0,
                    scale: 0.08,
                }),
                ("imager", "mono_bass") => one(Add {
                    knob: SUB,
                    center: 0.0,
                    scale: 0.3,
                }),
                ("imager", "output") => one(Below {
                    knob: LOUDNESS,
                    scale: 1.0,
                }),
                ("maximizer", "gain") => one(Add {
                    knob: LOUDNESS,
                    center: 0.0,
                    scale: 1.0,
                }),
                _ => one(Base),
            }
        }
        Kind::Vocals => {
            use vocals::*;
            match (stage, id) {
                ("eq", "b14_gain") => one(Add {
                    knob: TONE,
                    center: 0.0,
                    scale: -0.03,
                }),
                ("eq", "b15_gain") => one(Add {
                    knob: TONE,
                    center: 0.0,
                    scale: 0.03,
                }),
                ("eq", "b13_gain") => one(Above {
                    knob: TONE,
                    scale: 0.0225,
                }),
                ("pitch_correct", "amount") => one(Fade {
                    knob: TUNE,
                    full: 50.0,
                }),
                ("pitch_correct", "speed") => one(Exp {
                    knob: TUNE,
                    center: 50.0,
                    per: 33.0,
                }),
                ("compressor", "threshold") => one(Add {
                    knob: COMP,
                    center: 50.0,
                    scale: -0.18,
                }),
                ("compressor", "mix") => one(Fade {
                    knob: COMP,
                    full: 20.0,
                }),
                // Part of what a lower threshold takes away comes back.
                ("compressor", "makeup") => one(Add {
                    knob: COMP,
                    center: 50.0,
                    scale: 0.09,
                }),
                ("drive", "mix") => one(Fade {
                    knob: DRIVE,
                    full: 40.0,
                }),
                ("drive", "drive") => one(Add {
                    knob: DRIVE,
                    center: 40.0,
                    scale: 0.3,
                }),
                ("deesser", "threshold") => one(Add {
                    knob: DEESS,
                    center: 50.0,
                    scale: -0.18,
                }),
                ("deesser", "range") => one(Fade {
                    knob: DEESS,
                    full: 25.0,
                }),
                ("doubler", "mix") => one(Curve {
                    knob: DOUBLE,
                    at: 50.0,
                    above: 0.6,
                }),
                ("delay" | "reverb", "mix") => one(Curve {
                    knob: SPACE,
                    at: 50.0,
                    above: 0.6,
                }),
                _ => one(Base),
            }
        }
    }
}

/// How fast following the target moves the gain (dB/s), and how far.
const FOLLOW_RATE: f32 = 3.0;
const FOLLOW_RANGE: f32 = 12.0;
/// Seconds let go after a change before hearing it, then heard to measure it.
const FOLLOW_WAIT: f32 = 0.5;
const FOLLOW_HEARD: f32 = 4.0;
/// Corrections after a change at most, and the gap left alone (LU).
const FOLLOW_ROUNDS: u8 = 3;
const FOLLOW_CLOSE: f32 = 0.3;

/// Strada-Master following its target: reached once, then kept through every change of the
/// settings. What it holds is the chain's gain, the output against the input heard over the
/// same seconds, so the music getting louder or softer never moves it.
#[derive(Clone, Copy, Debug, Default)]
struct Follow {
    on: bool,
    /// The gain held, output against input (LU); unknown until first measured.
    wanted: Option<f32>,
    /// Measured since the last change: input and output power by seconds, and seconds heard
    /// (silence left out), after `wait` seconds.
    sums: (f64, f64, f32),
    wait: f32,
    /// Gliding the gain to this, else measuring.
    goal: Option<f32>,
    rounds: u8,
    done: bool,
}

impl Follow {
    /// Something changed: measured again, and corrected afresh.
    fn again(&mut self) {
        self.sums = (0.0, 0.0, 0.0);
        self.wait = FOLLOW_WAIT;
        self.goal = None;
        self.rounds = 0;
        self.done = false;
    }
}

pub struct Chain {
    kind: Kind,
    info: &'static EffectInfo,
    own: usize,
    values: Vec<f32>,
    /// The knobs as the stages get them: Loudness with what following the target added.
    knobs: Vec<f32>,
    /// What each stage parameter was last given, to forward only changes.
    sent: Vec<f32>,
    rules: Vec<[Rule; 2]>,
    /// Stage and index in the stage of each parameter after the chain's own.
    owner: Vec<(usize, usize)>,
    ids: &'static [&'static str],
    stages: Vec<Box<dyn Effect>>,
    first_meter: Vec<usize>,
    order: [usize; 8],
    movable: usize,
    rate: f32,
    watched: bool,
    spectra: [Analyzer; 2],
    levels: [LevelMeter; 2],
    followed: f32,
    following: Follow,
    same: SameLoudness,
    /// The input, delayed as the stages delay it, for "Before, same loudness".
    dry: [Vec<f32>; 2],
    dry_at: usize,
}

impl std::fmt::Debug for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chain")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Chain {
    pub fn master() -> Self {
        Self::new(Kind::Master, &MASTER, &master::STAGES, master::MOVABLE)
    }

    pub fn vocals() -> Self {
        Self::new(Kind::Vocals, &VOCALS, &vocals::STAGES, vocals::MOVABLE)
    }

    fn new(
        kind: Kind,
        info: &'static EffectInfo,
        ids: &'static [&'static str],
        movable: usize,
    ) -> Self {
        let own = info.params.len()
            - ids
                .iter()
                .map(|s| crate::base_info(s).map_or(0, |i| i.params.len()))
                .sum::<usize>();
        let mut owner = Vec::new();
        let mut first_meter = Vec::new();
        let mut rule_list = vec![[Rule::Base; 2]; info.params.len()];
        let (mut param_at, mut meter_at) = (own, OWN_METERS);
        let mut stages = Vec::new();
        for (s, &id) in ids.iter().enumerate() {
            let sub = crate::base_info(id).expect("a chain stage is a built-in effect");
            first_meter.push(meter_at);
            for (k, p) in sub.params.iter().enumerate() {
                owner.push((s, k));
                rule_list[param_at + k] = rules(kind, id, p.id);
            }
            param_at += sub.params.len();
            meter_at += sub.meters.len();
            stages.push(crate::create(id).expect("a chain stage can be made"));
        }
        let values: Vec<f32> = info.params.iter().map(|p| p.default).collect();
        let mut chain = Self {
            kind,
            info,
            own,
            knobs: values[..own].to_vec(),
            sent: vec![f32::NAN; values.len()],
            values,
            rules: rule_list,
            owner,
            ids,
            stages,
            first_meter,
            order: std::array::from_fn(|i| i),
            movable,
            rate: 48_000.0,
            watched: false,
            spectra: [Analyzer::default(), Analyzer::default()],
            levels: [LevelMeter::new(48_000), LevelMeter::new(48_000)],
            followed: 0.0,
            following: Follow::default(),
            same: SameLoudness::default(),
            dry: [Vec::new(), Vec::new()],
            dry_at: 0,
        };
        for i in own..chain.values.len() {
            chain.forward(i);
        }
        chain
    }

    /// Gives a stage parameter its value with the knobs applied.
    fn forward(&mut self, index: usize) {
        let p = &self.info.params[index];
        let [a, b] = self.rules[index];
        let value = b
            .apply(a.apply(self.values[index], &self.knobs), &self.knobs)
            .clamp(p.min, p.max);
        if self.sent[index] == value {
            return;
        }
        self.sent[index] = value;
        let (stage, local) = self.owner[index - self.own];
        self.stages[stage].set_param(local, value);
    }

    /// Gives the stages what knob `knob` reaches.
    fn forward_knob(&mut self, knob: usize) {
        for i in self.own..self.values.len() {
            if self.rules[i].iter().any(|r| r.knob() == Some(knob)) {
                self.forward(i);
            }
        }
    }

    /// Whether stage `s` is heard: Strada-Vocals leaves some out while a take is recorded.
    fn plays(&self, s: usize) -> bool {
        !(self.kind == Kind::Vocals
            && self.values[vocals::TAKE] >= 0.5
            && vocals::OFF_IN_TAKES.contains(&self.ids[s]))
    }

    /// Strada-Master following its target (see [`Follow`]); the gain goes back to nothing once
    /// the switch is off.
    fn follow(&mut self, frames: usize) {
        if self.kind != Kind::Master {
            return;
        }
        let dt = frames as f32 / self.rate;
        let step = FOLLOW_RATE * dt;
        let before = self.followed;
        let f = &mut self.following;
        if !f.on {
            self.followed -= self.followed.clamp(-step, step);
        } else if let Some(goal) = f.goal {
            self.followed += (goal - self.followed).clamp(-step, step);
            if (goal - self.followed).abs() < 1e-3 {
                let rounds = f.rounds;
                f.again();
                f.rounds = rounds;
            }
        } else if !f.done {
            let (input, output) = (self.levels[0].momentary(), self.levels[1].momentary());
            if f.wait > 0.0 {
                f.wait -= dt;
            } else if input > -50.0 {
                let power = |lufs: f32| 10f64.powf(f64::from(lufs) / 10.0) * f64::from(dt);
                f.sums.0 += power(input);
                f.sums.1 += power(output);
                f.sums.2 += dt;
            }
            if f.sums.2 >= FOLLOW_HEARD {
                let lu = |p: f64| (10.0 * p.max(1e-30).log10()) as f32;
                let (heard_in, heard_out) = (lu(f.sums.0), lu(f.sums.1));
                let gain = heard_out - heard_in;
                let target = master::TARGET_LUFS
                    [(self.values[master::TARGET].round().max(0.0) as usize).min(6)];
                let wanted = *f
                    .wanted
                    .get_or_insert(gain + target - (heard_out - lu(f64::from(f.sums.2))));
                let gap = wanted - gain;
                let rounds = f.rounds;
                if gap.abs() > FOLLOW_CLOSE && rounds < FOLLOW_ROUNDS {
                    f.again();
                    f.rounds = rounds + 1;
                    f.goal = Some((self.followed + gap).clamp(-FOLLOW_RANGE, FOLLOW_RANGE));
                } else {
                    f.done = true;
                }
            }
        }
        if (self.followed - before).abs() > 1e-4 {
            self.knobs[master::LOUDNESS] = self.values[master::LOUDNESS] + self.followed;
            self.forward_knob(master::LOUDNESS);
        }
    }

    /// What a change of parameter `index` (from `was`) tells following the target.
    fn followed_change(&mut self, index: usize, was: f32) {
        if self.kind != Kind::Master {
            return;
        }
        let value = self.values[index];
        let f = &mut self.following;
        match index {
            master::FOLLOW => {
                f.on = value >= 0.5;
                f.wanted = None;
                f.again();
            }
            master::COMPARE | master::ASSISTED => {}
            _ if !f.on => {}
            master::TARGET => {
                let lufs = |v: f32| master::TARGET_LUFS[(v.round().max(0.0) as usize).min(6)];
                if let Some(w) = &mut f.wanted {
                    *w += lufs(value) - lufs(was);
                }
                f.again();
            }
            master::LOUDNESS => {
                if let Some(w) = &mut f.wanted {
                    *w += value - was;
                }
                f.again();
            }
            _ => f.again(),
        }
    }

    fn comparing(&self) -> bool {
        self.kind == Kind::Master && self.values[master::COMPARE] >= 0.5
    }
}

impl Effect for Chain {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rate = sample_rate as f32;
        for stage in &mut self.stages {
            stage.prepare(sample_rate, max_frames);
        }
        for spectrum in &mut self.spectra {
            spectrum.prepare(sample_rate, max_frames);
        }
        self.levels = [LevelMeter::new(sample_rate), LevelMeter::new(sample_rate)];
        let room = sample_rate as usize + max_frames;
        self.dry = [vec![0.0; room], vec![0.0; room]];
        self.dry_at = 0;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        let Some(v) = self.values.get_mut(index) else {
            return;
        };
        if *v == value {
            return;
        }
        let was = std::mem::replace(v, value);
        self.followed_change(index, was);
        if index >= self.own {
            self.forward(index);
            return;
        }
        let order = match self.kind {
            Kind::Master => master::ORDER,
            Kind::Vocals => vocals::ORDER,
        };
        if index == order {
            self.order = self::order(value, self.movable);
            return;
        }
        self.knobs[index] = if self.kind == Kind::Master && index == master::LOUDNESS {
            value + self.followed
        } else {
            value
        };
        self.forward_knob(index);
    }

    fn set_watched(&mut self, watched: bool) {
        if self.watched && !watched {
            self.spectra.iter_mut().for_each(Effect::reset);
        }
        self.watched = watched;
        for stage in &mut self.stages {
            stage.set_watched(watched);
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        for stage in &mut self.stages {
            stage.set_tempo(bpm);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.follow(n);
        for (l, r) in left.iter().zip(right.iter()) {
            self.levels[0].push(*l, *r);
        }
        if self.watched {
            self.spectra[0].process(left, right);
        }
        let room = self.dry[0].len();
        let keeps_dry = self.kind == Kind::Master && room > n;
        if keeps_dry {
            for (i, (l, r)) in left.iter().zip(right.iter()).enumerate() {
                let at = (self.dry_at + i) % room;
                self.dry[0][at] = *l;
                self.dry[1][at] = *r;
            }
        }
        let count = self.stages.len();
        for k in 0..count {
            let s = if k < self.movable { self.order[k] } else { k };
            if !self.plays(s) {
                continue;
            }
            self.stages[s].process(left, right);
        }
        for (l, r) in left.iter().zip(right.iter()) {
            self.levels[1].push(*l, *r);
        }
        if self.watched {
            self.spectra[1].process(left, right);
        }
        let (from, to) = self.same.next(&self.levels[0], &self.levels[1]);
        if keeps_dry {
            if self.comparing() {
                let delay = (self.latency() as usize).min(room - n);
                for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                    let at = (self.dry_at + i + room - delay) % room;
                    let g = from + (to - from) * i as f32 / n.max(1) as f32;
                    *l = self.dry[0][at] * g;
                    *r = self.dry[1][at] * g;
                }
            }
            self.dry_at = (self.dry_at + n) % room;
        }
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
        self.spectra.iter_mut().for_each(Effect::reset);
        self.levels.iter_mut().for_each(LevelMeter::reset);
        if !self.following.done {
            self.following.again();
        }
        for d in &mut self.dry {
            d.fill(0.0);
        }
    }

    fn latency(&self) -> u32 {
        (0..self.stages.len())
            .filter(|&s| self.plays(s))
            .map(|s| self.stages[s].latency())
            .sum()
    }

    fn meters(&self, out: &mut [f32]) {
        let own = [
            self.levels[0].short_term(),
            self.levels[1].short_term(),
            self.levels[1].integrated(),
            self.levels[1].highest_true_peak(),
            self.levels[0].peaks()[0].max(self.levels[0].peaks()[1]),
            self.levels[1].peaks()[0].max(self.levels[1].peaks()[1]),
            self.followed,
        ];
        for (o, v) in out.iter_mut().zip(own) {
            *o = v;
        }
        if let Some(o) = out.get_mut(IN_SPECTRUM..OUT_SPECTRUM) {
            self.spectra[0].meters(o);
        }
        if let Some(o) = out.get_mut(OUT_SPECTRUM..OWN_METERS) {
            self.spectra[1].meters(o);
        }
        for (s, stage) in self.stages.iter().enumerate() {
            let start = self.first_meter[s];
            let end = self
                .first_meter
                .get(s + 1)
                .copied()
                .unwrap_or(self.info.meters.len());
            if let Some(o) = out.get_mut(start..end) {
                stage.meters(o);
            }
        }
    }
}

/// Where a stage's parameters and meters start in its chain, by effect id.
pub fn stage_range(info: &EffectInfo, stage: &str) -> Option<(usize, usize)> {
    let prefix = format!("{stage}.");
    let param = info.params.iter().position(|p| p.id.starts_with(&prefix))?;
    let (stages, _) = stages_of(info.id)?;
    let mut meter = OWN_METERS;
    for &s in stages {
        if s == stage {
            return Some((param, meter));
        }
        meter += crate::base_info(s).map_or(0, |i| i.meters.len());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chain: &mut Chain, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        run_at(chain, seconds, 0.3)
    }

    fn run_at(chain: &mut Chain, seconds: f32, amplitude: f32) -> (Vec<f32>, Vec<f32>) {
        chain.prepare(48_000, 512);
        let mut out = (Vec::new(), Vec::new());
        let blocks = (seconds * 48_000.0 / 512.0) as usize;
        for b in 0..blocks {
            let mut l: Vec<f32> = (0..512)
                .map(|i| {
                    let t = (b * 512 + i) as f32 / 48_000.0;
                    amplitude * (std::f32::consts::TAU * 220.0 * t).sin()
                })
                .collect();
            let mut r = l.clone();
            chain.process(&mut l, &mut r);
            out.0.extend(l);
            out.1.extend(r);
        }
        out
    }

    fn index(info: &EffectInfo, id: &str) -> usize {
        info.params.iter().position(|p| p.id == id).unwrap()
    }

    #[test]
    fn every_stage_parameter_is_listed_under_its_effect_id() {
        for (info, stages) in [
            (&*MASTER, &master::STAGES[..]),
            (&*VOCALS, &vocals::STAGES[..]),
        ] {
            for stage in stages {
                let sub = crate::base_info(stage).unwrap();
                for p in sub.params {
                    let id = format!("{stage}.{}", p.id);
                    assert!(
                        info.params.iter().any(|q| q.id == id),
                        "{} lacks {id}",
                        info.id
                    );
                }
            }
            let mut ids: Vec<_> = info.params.iter().map(|p| p.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), info.params.len(), "{} repeats an id", info.id);
        }
    }

    #[test]
    fn both_chains_play_a_finite_sound_at_their_defaults() {
        for mut chain in [Chain::master(), Chain::vocals()] {
            let (l, r) = run(&mut chain, 1.0);
            assert!(l.iter().chain(&r).all(|x| x.is_finite()));
            assert!(l.iter().any(|x| x.abs() > 0.01));
        }
    }

    #[test]
    fn a_knob_acts_around_the_base_without_changing_it() {
        let mut chain = Chain::master();
        let thresh = index(&MASTER, "mb_comp.b2_thresh");
        chain.set_param(thresh, -20.0);
        chain.set_param(master::GLUE, 100.0);
        assert_eq!(chain.values[thresh], -20.0);
        assert!((chain.sent[thresh] - -24.5).abs() < 1e-4);
        chain.set_param(master::GLUE, 50.0);
        assert!((chain.sent[thresh] - -20.0).abs() < 1e-4);
    }

    #[test]
    fn the_sub_knob_moves_the_eq_s_last_band() {
        let mut chain = Chain::master();
        let gain = index(&MASTER, "eq.b16_gain");
        chain.set_param(master::SUB, 100.0);
        assert!((chain.sent[gain] - 8.0).abs() < 1e-4);
        chain.set_param(master::SUB, -50.0);
        assert!((chain.sent[gain] - -4.0).abs() < 1e-4);
    }

    #[test]
    fn an_editor_shows_what_is_played_and_writes_the_base_back() {
        let mut own: Vec<f32> = MASTER.params.iter().take(12).map(|p| p.default).collect();
        own[master::GLUE] = 100.0;
        own[master::TONE] = 60.0;
        for (id, base) in [
            ("mb_comp.b2_thresh", -20.0),
            ("eq.b15_gain", 1.0),
            ("eq.b13_gain", 0.0),
        ] {
            let heard = played("mastering", id, base, &own);
            assert!(
                (base_for("mastering", id, heard, &own) - base).abs() < 1e-4,
                "{id}"
            );
        }
        assert!((played("mastering", "mb_comp.b2_thresh", -20.0, &own) - -24.5).abs() < 1e-4);
    }

    #[test]
    fn a_take_leaves_out_the_tuning_and_the_space() {
        let mut chain = Chain::vocals();
        chain.prepare(48_000, 512);
        let latency = chain.latency();
        assert!(latency > 0);
        chain.set_param(vocals::TAKE, 1.0);
        assert!(chain.latency() < latency);
    }

    #[test]
    fn every_order_reads_back_from_its_code() {
        for code in 0..factorial(5) {
            let order = order(code as f32, 5);
            let mut seen = order[..5].to_vec();
            seen.sort_unstable();
            assert_eq!(seen, [0, 1, 2, 3, 4]);
            assert_eq!(order_code(&order[..5]), code as f32);
            assert_eq!(order[5..], [5, 6, 7]);
        }
    }

    #[test]
    fn following_the_target_brings_the_loudness_to_it() {
        let mut chain = Chain::master();
        chain.set_param(master::FOLLOW, 1.0);
        chain.set_param(master::LOUDNESS, 0.0);
        run(&mut chain, 40.0);
        let short = chain.levels[1].short_term();
        assert!(
            (short - -14.0).abs() < 1.5,
            "{short} LUFS, followed {}",
            chain.followed
        );
    }

    #[test]
    fn following_the_target_leaves_the_music_s_own_changes_alone() {
        let mut chain = Chain::master();
        chain.set_param(master::FOLLOW, 1.0);
        run(&mut chain, 30.0);
        let held = chain.followed;
        run_at(&mut chain, 15.0, 0.1);
        assert!(
            (chain.followed - held).abs() < 0.01,
            "{held} then {}",
            chain.followed
        );
    }

    #[test]
    fn following_the_target_makes_up_for_a_change_of_setting() {
        let mut chain = Chain::master();
        chain.set_param(master::FOLLOW, 1.0);
        run(&mut chain, 30.0);
        let before = chain.levels[1].short_term();
        let glue = index(&MASTER, "glue");
        chain.set_param(glue, 100.0);
        run(&mut chain, 30.0);
        let after = chain.levels[1].short_term();
        assert!((after - before).abs() < 1.0, "{before} then {after} LUFS");
    }

    #[test]
    fn before_at_the_same_loudness_plays_the_input_at_the_output_loudness() {
        let mut chain = Chain::master();
        chain.set_param(master::LOUDNESS, 8.0);
        run(&mut chain, 4.0);
        let processed = chain.levels[1].short_term();
        chain.set_param(master::COMPARE, 1.0);
        let (l, _) = run(&mut chain, 4.0);
        let rms =
            (l[l.len() / 2..].iter().map(|x| x * x).sum::<f32>() / (l.len() / 2) as f32).sqrt();
        let input_rms = 0.3 / 2f32.sqrt();
        assert!(
            rms > input_rms * 1.5,
            "{rms} against {input_rms}, processed at {processed}"
        );
    }

    #[test]
    fn the_stage_ranges_point_at_their_parameters_and_meters() {
        let (param, meter) = stage_range(&MASTER, "tape").unwrap();
        assert_eq!(MASTER.params[param].id, "tape.drive");
        let before: usize = ["eq", "mb_comp"]
            .iter()
            .map(|s| crate::base_info(s).unwrap().meters.len())
            .sum();
        assert_eq!(meter, OWN_METERS + before);
    }
}
