//! Console color: the input stage of an analog console's channel, or the summing of its buses,
//! as a chain of analog stages (`crate::analog`): transformers, amplifiers, a VCA, each console
//! its own recipe, calibrated on measured plugins of the console it is after (`recipes`).
//!
//! The amount pushes the circuit, 12 dB softer at 0 % or harder at 100 %, at an equal level: a
//! signal at the working level (-18 dBFS) keeps its loudness, so switching the color on or off
//! compares at equal loudness. Each channel has its own tolerances, from a seed (its track),
//! and its noise floor when hiss is asked for.

pub mod recipes;

use crate::analog::{
    ClassA, OpAmp, Stage, Transformer, Variation, Vca, chain::Chain, noise::Noise,
};
use crate::biquad::{Biquad, Coefficients, FilterKind};
pub use recipes::{Reading, Recipe, Target, recipe, targets};

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
    tone: [Coefficients; 2],
    tone_state: [[Biquad; 2]; 2],
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
            tone: [Coefficients::IDENTITY; 2],
            tone_state: Default::default(),
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
            for side in &mut self.sides {
                side.set_amount(amount);
            }
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
                .map_or(Variation::NONE, |s| Variation::of(s, side, 1.0));
            let seed = self.seed.unwrap_or(0) as u32 ^ (side as u32 + 1).wrapping_mul(0x9e37_79b9);
            let noise = self.noise.then(|| Noise::new(r.noise, rate, seed));
            *chain = Chain::new(Circuit::new(&r, v, rate), v.gain_db, noise, rate);
            chain.set_amount(self.amount);
        }
        let nyquist = rate * 0.45;
        self.tone = [
            Coefficients::design(FilterKind::LowShelf, r.low.0, 0.7, r.low.1, rate),
            Coefficients::design(
                FilterKind::HighShelf,
                r.high.0.min(nyquist),
                0.7,
                r.high.1,
                rate,
            ),
        ];
        // A first-order highpass at 10 kHz is 20 dB down at 1 kHz.
        self.bleed = Coefficients::first_order(true, 10_000.0, rate);
        self.bleed_gain = if self.bus && self.noise {
            crate::db_to_gain(r.crosstalk_db + 20.0)
        } else {
            0.0
        };
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
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.recipe.is_none() {
            return;
        }
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
                for (c, s) in self.tone.iter().zip(states.iter_mut()) {
                    y = s.process(c, y);
                }
                *x = y as f32;
            }
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
