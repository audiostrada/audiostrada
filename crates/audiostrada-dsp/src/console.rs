//! Console color: the input stage of an analog console's channel, or the summing of its buses,
//! as a chain of analog stages (`crate::analog`): transformers, amplifiers, a VCA, each console
//! its own recipe, calibrated on measured plugins of the console it is after (`recipes`).
//!
//! The amount pushes the circuit and brings its voicing out: at 25 % the console is as its
//! references measure it, at 50 % it is run hot (12 dB harder) as on records, past it comes a
//! controlled exaggeration (see `analog::chain::drive_db`). Its voicing (a few tenths of a dB
//! of broad curves), the width of its buses and the glue of its mix bus grow with it. The
//! level is kept all along: what comes out is as loud as what went in, so switching the color
//! on or off compares at equal loudness. Each channel has its own tolerances, from a seed (its
//! track), and its noise floor when hiss is asked for.

pub mod recipes;

pub use crate::analog::chain::MEASURED;
use crate::analog::{
    ClassA, OpAmp, Stage, Transformer, Variation, Vca, chain::Chain, noise::Noise,
};
use crate::biquad::{Biquad, Coefficients};
pub use recipes::{Reading, Recipe, Target, recipe, targets};

/// How hard an amount pushes a channel's circuit, or a bus's, dB.
/// Past the console as measured, `hotter` dB more come in by 50 %.
fn push_db(amount: f32, bus: bool, hotter: f32) -> f32 {
    let base = if bus {
        crate::analog::chain::bus_drive_db(amount)
    } else {
        crate::analog::chain::drive_db(amount)
    };
    base + hotter * ((amount - MEASURED) / MEASURED).clamp(0.0, 1.0)
}

/// How much of a console's voicing is heard at an amount: none at 0 %, a quarter at 25 %, its
/// own at 50 %, three times as much at 100 %.
pub fn voicing(amount: f32) -> f32 {
    let amount = amount.clamp(0.0, 100.0);
    // Slow at first: as measured (25 %), a console is nearly flat, as its references.
    if amount <= 50.0 {
        (amount / 50.0).powi(2)
    } else {
        1.0 + (amount - 50.0) / 25.0
    }
}

/// The glue of a mix bus: a slow compressor holding the loud moments a little, both sides
/// together.
#[derive(Clone, Copy, Debug, Default)]
struct Glue {
    /// Square of the threshold; 0 when off.
    threshold: f32,
    /// Half of 1 - 1/ratio: the exponent of the gain on squared levels.
    slope: f32,
    attack: f32,
    release: f32,
    level: f32,
}

impl Glue {
    /// `depth`: 0 off, 1 at amount 50 %, 2 at 100 %.
    fn set(&mut self, settings: Option<(f32, f32)>, depth: f32, rate: f64) {
        let Some((threshold_db, ratio)) = settings.filter(|_| depth > 0.0) else {
            self.threshold = 0.0;
            return;
        };
        // Deeper: a lower threshold and a firmer ratio.
        let threshold = crate::db_to_gain(threshold_db - 3.0 * (depth - 1.0));
        let ratio = 1.0 + (ratio - 1.0) * depth;
        self.threshold = threshold * threshold;
        self.slope = (1.0 - 1.0 / ratio) / 2.0;
        self.attack = crate::analog::one_pole(30.0, rate) as f32;
        self.release = crate::analog::one_pole(300.0, rate) as f32;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.threshold <= 0.0 {
            return;
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let power = (*l * *l).max(*r * *r);
            let c = if power > self.level {
                self.attack
            } else {
                self.release
            };
            self.level += c * (power - self.level);
            if self.level > self.threshold {
                let gain = (self.threshold / self.level).powf(self.slope);
                *l *= gain;
                *r *= gain;
            }
        }
    }
}

/// The consoles, in the order of the project model; the first colors nothing.
pub const MODELS: [&str; 5] = [
    "Clean",
    "British Warm",
    "British Bright",
    "American Punch",
    "Nashville",
];

/// The character a channel strip's drive takes from a console: how hard it pushes and how much
/// of even harmonics it makes.
pub fn strip_character(model: usize) -> Option<(f32, f32)> {
    match model {
        1 => Some((1.2, 0.08)),
        2 => Some((1.0, 0.02)),
        3 => Some((1.45, 0.1)),
        4 => Some((0.7, 0.07)),
        _ => None,
    }
}

/// One side of a console's channel: its stages in order, the ones it has.
#[derive(Clone, Debug, Default)]
pub struct Circuit {
    input: Option<Transformer>,
    class_a: Option<ClassA>,
    op_amp: Option<OpAmp>,
    vca: Option<Vca>,
    output: Option<Transformer>,
}

impl Circuit {
    /// The circuit of `recipe`, its parts off by `v`.
    pub fn new(recipe: &Recipe, v: Variation, rate: f64) -> Self {
        // A hotter part saturates sooner.
        let hotter = 20.0 * v.drive.log10();
        let iron = |s: crate::analog::transformer::Settings| {
            Transformer::new(
                crate::analog::transformer::Settings {
                    corner_hz: s.corner_hz * f64::from(v.corner),
                    saturation_dbu: s.saturation_dbu - hotter,
                    top_hz: s.top_hz * f64::from(v.top),
                    ..s
                },
                rate,
            )
        };
        Self {
            input: recipe.input.map(iron),
            class_a: recipe.class_a.map(|s| {
                ClassA::new(
                    crate::analog::amplifier::ClassASettings {
                        headroom_dbu: s.headroom_dbu - hotter,
                        asymmetry: s.asymmetry * v.asymmetry,
                        ..s
                    },
                    rate,
                )
            }),
            op_amp: recipe
                .op_amp
                .map(|(rails, slew)| OpAmp::new(rails, slew, rate)),
            vca: recipe
                .vca
                .map(|(h2, h3)| Vca::new(h2 + 20.0 * v.asymmetry.log10(), h3 + hotter, rate)),
            output: recipe.output.map(iron),
        }
    }
}

impl Stage for Circuit {
    fn process(&mut self, mut x: f32) -> f32 {
        if let Some(s) = &mut self.input {
            x = s.process(x);
        }
        if let Some(s) = &mut self.class_a {
            x = s.process(x);
        }
        if let Some(s) = &mut self.op_amp {
            x = s.process(x);
        }
        if let Some(s) = &mut self.vca {
            x = s.process(x);
        }
        if let Some(s) = &mut self.output {
            x = s.process(x);
        }
        x
    }

    fn pushed(&mut self, mut x: f32, over: f32) -> f32 {
        if let Some(s) = &mut self.input {
            x = s.process(x);
        }
        if let Some(s) = &mut self.class_a {
            x = s.process(x);
        }
        // Its rails would square everything: they stay where a hot console has them.
        if let Some(s) = &mut self.op_amp {
            x = s.process(x / over) * over;
        }
        if let Some(s) = &mut self.vca {
            x = s.process(x);
        }
        if let Some(s) = &mut self.output {
            x = s.process(x);
        }
        x
    }

    fn reset(&mut self) {
        if let Some(s) = &mut self.input {
            s.reset();
        }
        if let Some(s) = &mut self.class_a {
            s.reset();
        }
        if let Some(s) = &mut self.op_amp {
            s.reset();
        }
        if let Some(s) = &mut self.vca {
            s.reset();
        }
        if let Some(s) = &mut self.output {
            s.reset();
        }
    }
}

/// The coloring of one channel or bus.
#[derive(Debug)]
pub struct ConsoleColor {
    rate: f64,
    model: usize,
    amount: f32,
    bus: bool,
    noise: bool,
    seed: Option<u64>,
    recipe: Option<Recipe>,
    sides: [Chain<Circuit>; 2],
    tone: [Coefficients; 3],
    tone_state: [[Biquad; 3]; 2],
    /// The moves of its voicing that change something.
    tone_on: [bool; 3],
    /// Gain of the sides against the middle, on a bus (1: as it is).
    width: f32,
    glue: Glue,
    /// The level kept: power going in and coming out as the ear weighs it, smoothed over a
    /// second, and the gain that makes them equal.
    heard: (f64, f64),
    level: f32,
    /// The weighting's lowpass and its states: going in and coming out, each side.
    weigh: f32,
    weighed: [[f32; 2]; 2],
    /// What leaks from one side of a bus into the other: a highpass, and its gain.
    bleed: Coefficients,
    bleed_gain: f32,
    bleed_state: [Biquad; 2],
}

impl ConsoleColor {
    pub fn new(rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        let side = || Chain::new(Circuit::default(), 0.0, None, rate);
        Self {
            rate,
            model: 0,
            amount: 0.0,
            bus: false,
            noise: false,
            seed: None,
            recipe: None,
            sides: [side(), side()],
            tone: [Coefficients::IDENTITY; 3],
            tone_state: Default::default(),
            tone_on: [false; 3],
            width: 1.0,
            glue: Glue::default(),
            heard: (0.0, 0.0),
            level: 1.0,
            weigh: crate::analog::one_pole(1_500.0, rate) as f32,
            weighed: [[0.0; 2]; 2],
            bleed: Coefficients::IDENTITY,
            bleed_gain: 0.0,
            bleed_state: Default::default(),
        }
    }

    /// The console (index into [`MODELS`]), its amount (0 to 100 %), summing a bus rather than
    /// a channel's input, its hiss and crosstalk heard, and the seed of its tolerances (`None`:
    /// a perfect channel).
    pub fn set(&mut self, model: usize, amount: f32, bus: bool, noise: bool, seed: Option<u64>) {
        let recipe = recipe(model, bus).filter(|_| amount > 0.0);
        let circuit = (model, bus, noise, seed) != (self.model, self.bus, self.noise, self.seed);
        if circuit || recipe.is_some() != self.recipe.is_some() {
            (self.model, self.bus, self.noise, self.seed) = (model, bus, noise, seed);
            self.amount = amount;
            self.recipe = recipe;
            self.design();
            self.reset();
        } else if amount != self.amount {
            self.amount = amount;
            let push = push_db(amount, self.bus, self.hotter());
            for side in &mut self.sides {
                side.set_push(push);
            }
            self.voice();
        }
    }

    /// A console of `recipe` rather than of a model's: what a fit tries.
    pub fn of_recipe(rate: u32, recipe: Recipe, amount: f32, bus: bool) -> Self {
        let mut color = Self::new(rate);
        (color.model, color.amount, color.bus) = (usize::MAX, amount, bus);
        color.recipe = Some(recipe);
        color.design();
        color.reset();
        color
    }

    fn hotter(&self) -> f32 {
        self.recipe.map_or(0.0, |r| r.hotter)
    }

    /// Changes something.
    pub fn active(&self) -> bool {
        self.recipe.is_some()
    }

    /// Builds the circuits; allocates nothing.
    fn design(&mut self) {
        let Some(r) = self.recipe else {
            return;
        };
        let rate = self.rate;
        for (side, chain) in self.sides.iter_mut().enumerate() {
            let v = self
                .seed
                .map_or(Variation::NONE, |s| Variation::of(s, side, r.spread));
            let seed = self.seed.unwrap_or(0) as u32 ^ (side as u32 + 1).wrapping_mul(0x9e37_79b9);
            let noise = self.noise.then(|| Noise::new(r.noise, rate, seed));
            *chain = Chain::new(Circuit::new(&r, v, rate), v.gain_db, noise, rate);
            chain.set_push(push_db(self.amount, self.bus, r.hotter));
        }
        // A first-order highpass at 10 kHz is 20 dB down at 1 kHz.
        self.bleed = Coefficients::first_order(true, 10_000.0, rate);
        self.bleed_gain = if self.bus && self.noise {
            crate::db_to_gain(r.crosstalk_db + 20.0)
        } else {
            0.0
        };
        self.voice();
    }

    /// What follows the amount besides the push: the voicing, the width and the glue of a
    /// bus. Allocates nothing.
    fn voice(&mut self) {
        let Some(r) = self.recipe else {
            return;
        };
        let (rate, depth) = (self.rate, voicing(self.amount));
        for (k, v) in r.tone.iter().enumerate() {
            let gain = v.gain_db * f64::from(depth);
            self.tone_on[k] = gain.abs() > 1e-3;
            self.tone[k] = Coefficients::design(v.kind, v.freq.min(rate * 0.45), v.q, gain, rate);
        }
        let bus = self.bus;
        self.width = if bus {
            crate::db_to_gain(r.width_db * depth)
        } else {
            1.0
        };
        // The glue comes in past the console as measured: 1 at 50 %, 2 at 100 %.
        let glued = if self.amount <= 50.0 {
            (self.amount - MEASURED).max(0.0) / MEASURED
        } else {
            self.amount / 50.0
        };
        self.glue.set(r.glue.filter(|_| bus), glued, rate);
    }

    pub fn reset(&mut self) {
        for side in &mut self.sides {
            side.reset();
        }
        for s in self
            .bleed_state
            .iter_mut()
            .chain(self.tone_state.iter_mut().flatten())
        {
            s.reset();
        }
        self.glue.level = 0.0;
        self.heard = (0.0, 0.0);
        self.level = 1.0;
        self.weighed = [[0.0; 2]; 2];
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.recipe.is_none() || left.is_empty() {
            return;
        }
        // Power as loudness meters weigh it: the highs count 4 dB more.
        let a = self.weigh;
        let power = |l: &[f32], r: &[f32], state: &mut [f32; 2]| -> f64 {
            let mut sum = 0.0f64;
            for (side, x) in [l, r].into_iter().enumerate() {
                let low = &mut state[side];
                for &x in x {
                    *low += a * (x - *low);
                    let y = x + 0.58 * (x - *low);
                    sum += f64::from(y) * f64::from(y);
                }
            }
            sum / (2 * l.len()) as f64
        };
        let before = power(left, right, &mut self.weighed[0]);
        if self.bleed_gain > 0.0 {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                let into_l = self.bleed_state[0].process(&self.bleed, f64::from(*r)) as f32;
                let into_r = self.bleed_state[1].process(&self.bleed, f64::from(*l)) as f32;
                *l += into_l * self.bleed_gain;
                *r += into_r * self.bleed_gain;
            }
        }
        for (side, buffer) in [&mut *left, &mut *right].into_iter().enumerate() {
            let chain = &mut self.sides[side];
            let states = &mut self.tone_state[side];
            for x in buffer.iter_mut() {
                let mut y = f64::from(chain.process(*x));
                for ((c, s), on) in self.tone.iter().zip(states.iter_mut()).zip(self.tone_on) {
                    if on {
                        y = s.process(c, y);
                    }
                }
                *x = y as f32;
            }
        }
        if self.width != 1.0 {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                let (mid, side) = ((*l + *r) / 2.0, (*l - *r) / 2.0 * self.width);
                (*l, *r) = (mid + side, mid - side);
            }
        }
        self.glue.process(left, right);
        let after = power(left, right, &mut self.weighed[1]);
        self.keep_level(before, after, left, right);
    }

    /// Brings what comes out to the loudness of what went in, slowly (a second): the color
    /// is heard, never a change of level.
    fn keep_level(&mut self, before: f64, after: f64, left: &mut [f32], right: &mut [f32]) {
        let a = (left.len() as f64 / self.rate).min(1.0);
        self.heard.0 += a * (before - self.heard.0);
        self.heard.1 += a * (after - self.heard.1);
        // Near silence, the gain stays where it was.
        let target = if self.heard.0 > 1e-8 && self.heard.1 > 1e-8 {
            ((self.heard.0 / self.heard.1).sqrt() as f32).clamp(0.71, 1.41)
        } else {
            self.level
        };
        // Smoothed again: no ripple of a low tone's power may reach the gain.
        let step = (target - self.level) * (2.0 * a as f32).min(1.0) / left.len() as f32;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.level += step;
            *l *= self.level;
            *r *= self.level;
        }
    }
}

/// Levels of the 2nd to 6th harmonics of a 200 Hz tone at `level_db` through a console, in dB
/// below the fundamental: its fingerprint.
pub fn harmonics(model: usize, amount: f32, level_db: f32) -> [f32; 5] {
    const RATE: u32 = 48_000;
    const PERIOD: usize = 240;
    let mut color = ConsoleColor::new(RATE);
    color.set(model, amount, false, false, None);
    let a = crate::db_to_gain(level_db);
    let tone: Vec<f32> = (0..PERIOD * 40)
        .map(|i| a * (i as f32 * std::f32::consts::TAU / PERIOD as f32).sin())
        .collect();
    let (mut l, mut r) = (tone.clone(), tone);
    color.process(&mut l, &mut r);
    let tail = &l[PERIOD * 20..];
    let amplitude = |k: usize| {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &s) in tail.iter().enumerate() {
            let w = std::f64::consts::TAU * (k * i) as f64 / PERIOD as f64;
            re += f64::from(s) * w.cos();
            im += f64::from(s) * w.sin();
        }
        (re * re + im * im).sqrt()
    };
    let fundamental = amplitude(1).max(1e-12);
    let mut out = [0.0; 5];
    for (k, o) in out.iter_mut().enumerate() {
        *o = (20.0 * (amplitude(k + 2) / fundamental).max(1e-9).log10()) as f32;
    }
    out
}

/// What a console measures at `targets`' tones: a reading per target.
pub fn read(color: &mut ConsoleColor, targets: &[Target]) -> Vec<f32> {
    let mut plan: Vec<(f32, f32)> = Vec::new();
    for t in targets {
        if !plan.contains(&(t.freq, t.level)) {
            plan.push((t.freq, t.level));
        }
    }
    color.reset();
    let tones = crate::measure::tones_at(&mut |l, r| color.process(l, r), &plan, 0);
    targets
        .iter()
        .map(|t| {
            let tone = tones
                .iter()
                .find(|x| x.freq == t.freq && x.level == t.level)
                .expect("a tone of the plan");
            match t.reading {
                Reading::Thd => tone.thd_db,
                Reading::H2 => tone.harmonics[0].unwrap_or(-200.0),
                Reading::H3 => tone.harmonics[1].unwrap_or(-200.0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
