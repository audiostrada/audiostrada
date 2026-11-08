//! Built-in amp models: triode preamp stages, the passive tone stack of each amp (D.T. Yeh's
//! circuit analysis, component values from the Guitarix tone stack library), a push-pull power
//! stage with sag, presence and an output transformer, all run four times oversampled.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::db_to_gain;
use crate::drive::{FACTOR, Oversampler, Taps};

/// Passive treble/middle/bass network: treble pot R1, bass pot R2, middle pot R3, slope
/// resistor R4 and capacitors C1 (treble), C2, C3.
#[derive(Clone, Copy, Debug)]
pub struct Stack {
    r1: f64,
    r2: f64,
    r3: f64,
    r4: f64,
    c1: f64,
    c2: f64,
    c3: f64,
}

/// Resistors in kilohms, C1 in picofarads, C2 and C3 in nanofarads.
const fn stack(r1: f64, r2: f64, r3: f64, r4: f64, c1: f64, c2: f64, c3: f64) -> Stack {
    Stack {
        r1: r1 * 1e3,
        r2: r2 * 1e3,
        r3: r3 * 1e3,
        r4: r4 * 1e3,
        c1: c1 * 1e-12,
        c2: c2 * 1e-9,
        c3: c3 * 1e-9,
    }
}

const BASSMAN: Stack = stack(250.0, 1000.0, 25.0, 56.0, 250.0, 20.0, 20.0);
const TWIN: Stack = stack(250.0, 250.0, 10.0, 100.0, 120.0, 100.0, 47.0);
const BLACKFACE: Stack = stack(250.0, 250.0, 10.0, 100.0, 250.0, 100.0, 47.0);
// Deluxe Reverb: a fixed 6.8 k resistor in place of the middle pot.
const DELUXE: Stack = stack(250.0, 250.0, 6.8, 100.0, 250.0, 100.0, 47.0);
const BLUES: Stack = stack(250.0, 250.0, 25.0, 100.0, 250.0, 22.0, 22.0);
const JTM45: Stack = stack(250.0, 1000.0, 25.0, 33.0, 270.0, 22.0, 22.0);
const PLEXI: Stack = stack(250.0, 1000.0, 25.0, 33.0, 500.0, 22.0, 22.0);
const JCM800: Stack = stack(220.0, 1000.0, 22.0, 33.0, 470.0, 22.0, 22.0);
const AC30: Stack = stack(1000.0, 1000.0, 10.0, 100.0, 50.0, 22.0, 22.0);
const SOLDANO: Stack = stack(250.0, 1000.0, 25.0, 47.0, 470.0, 20.0, 20.0);
const MESA: Stack = stack(250.0, 250.0, 25.0, 100.0, 250.0, 100.0, 47.0);
const TRIPLE: Stack = stack(250.0, 1000.0, 33.0, 51.0, 220.0, 15.0, 47.0);
const AMPEG: Stack = stack(250.0, 1000.0, 25.0, 32.0, 470.0, 22.0, 22.0);
const FLIPTOP: Stack = stack(250.0, 250.0, 10.0, 100.0, 100.0, 100.0, 47.0);

/// The tone stack's transfer function for knobs from 0 to 1 (the bass pot is logarithmic):
/// H(s) = (b1 s + b2 s^2 + b3 s^3) / (1 + a1 s + a2 s^2 + a3 s^3).
fn coefficients(c: &Stack, t: f64, m: f64, bass: f64) -> ([f64; 3], [f64; 3]) {
    let l = ((bass - 1.0) * 3.4).exp();
    let Stack {
        r1,
        r2,
        r3,
        r4,
        c1,
        c2,
        c3,
    } = *c;
    let b1 = t * c1 * r1 + m * c3 * r3 + l * (c1 * r2 + c2 * r2) + (c1 * r3 + c2 * r3);
    let b2 = t * (c1 * c2 * r1 * r4 + c1 * c3 * r1 * r4)
        - m * m * (c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
        + m * (c1 * c3 * r1 * r3 + c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
        + l * (c1 * c2 * r1 * r2 + c1 * c2 * r2 * r4 + c1 * c3 * r2 * r4)
        + l * m * (c1 * c3 * r2 * r3 + c2 * c3 * r2 * r3)
        + (c1 * c2 * r1 * r3 + c1 * c2 * r3 * r4 + c1 * c3 * r3 * r4);
    let c123 = c1 * c2 * c3;
    let b3 = l * m * (c123 * r1 * r2 * r3 + c123 * r2 * r3 * r4)
        - m * m * (c123 * r1 * r3 * r3 + c123 * r3 * r3 * r4)
        + m * (c123 * r1 * r3 * r3 + c123 * r3 * r3 * r4)
        + t * c123 * r1 * r3 * r4
        - t * m * c123 * r1 * r3 * r4
        + t * l * c123 * r1 * r2 * r4;
    let a1 =
        (c1 * r1 + c1 * r3 + c2 * r3 + c2 * r4 + c3 * r4) + m * c3 * r3 + l * (c1 * r2 + c2 * r2);
    let a2 = m * (c1 * c3 * r1 * r3 - c2 * c3 * r3 * r4 + c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
        + l * m * (c1 * c3 * r2 * r3 + c2 * c3 * r2 * r3)
        - m * m * (c1 * c3 * r3 * r3 + c2 * c3 * r3 * r3)
        + l * (c1 * c2 * r2 * r4 + c1 * c2 * r1 * r2 + c1 * c3 * r2 * r4 + c2 * c3 * r2 * r4)
        + (c1 * c2 * r1 * r4
            + c1 * c3 * r1 * r4
            + c1 * c2 * r3 * r4
            + c1 * c2 * r1 * r3
            + c1 * c3 * r3 * r4
            + c2 * c3 * r3 * r4);
    let a3 = l * m * (c123 * r1 * r2 * r3 + c123 * r2 * r3 * r4)
        - m * m * (c123 * r1 * r3 * r3 + c123 * r3 * r3 * r4)
        + m * (c123 * r3 * r3 * r4 + c123 * r1 * r3 * r3 - c123 * r1 * r3 * r4)
        + l * c123 * r1 * r2 * r4
        + c123 * r1 * r3 * r4;
    ([b1, b2, b3], [a1, a2, a3])
}

/// The tone stack, discretized as trapezoidal integrators in observable canonical form (as
/// in the Guitarix library), which stays accurate where a direct form loses precision.
#[derive(Clone, Copy, Debug, Default)]
struct ToneStack {
    g: f64,
    nx: f64,
    dn: f64,
    be1: f64,
    be2: f64,
    al0: f64,
    al1: f64,
    s: [f64; 3],
}

impl ToneStack {
    fn design(&mut self, c: &Stack, t: f64, m: f64, bass: f64, rate: f64) {
        let ([b1, b2, b3], [a1, a2, a3]) = coefficients(c, t, m, bass);
        let (r2, r3) = (1.0 / a2, 1.0 / a3);
        let rw = a3 * r2;
        self.g = a2 * r3 * 0.5 / rate;
        self.al1 = a1 * r2 * rw;
        self.al0 = r2 * rw * rw;
        self.be2 = b2 * r2;
        self.be1 = b1 * r2 * rw;
        let g = self.g;
        self.dn = 1.0 / (1.0 + g * (1.0 + g * (self.al1 + g * self.al0)));
        self.nx = b3 * r3 + g * (self.be2 + g * self.be1);
    }

    fn tick(&mut self, x: f64) -> f64 {
        let g = self.g;
        let [s1, s2, s3] = self.s;
        let y = (self.nx * x + s1 + g * (s2 + g * s3)) * self.dn;
        let z3 = s3 - g * self.al0 * y;
        let z2 = g * (self.be1 * x - self.al1 * y + z3) + s2;
        let z1 = g * (self.be2 * x - y + z2) + s1;
        self.s = [2.0 * z1 - s1, 2.0 * z2 - s2, 2.0 * z3 - s3];
        y
    }
}

/// Gain of the tone stack at `freq`.
fn stack_gain(c: &Stack, t: f64, m: f64, bass: f64, freq: f64) -> f64 {
    let ([b1, b2, b3], [a1, a2, a3]) = coefficients(c, t, m, bass);
    let w = std::f64::consts::TAU * freq;
    let num = (-b2 * w * w).hypot(b1 * w - b3 * w * w * w);
    let den = (1.0 - a2 * w * w).hypot(a1 * w - a3 * w * w * w);
    num / den
}

/// One-pole filter (topology-preserving), low-pass or high-pass.
#[derive(Clone, Copy, Debug, Default)]
struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    fn set(&mut self, freq: f32, rate: f32) {
        let g = (std::f32::consts::PI * freq.min(rate * 0.45) / rate).tan();
        self.a = g / (1.0 + g);
    }

    fn low(&mut self, x: f32) -> f32 {
        let v = (x - self.z) * self.a;
        let y = v + self.z;
        self.z = y + v;
        y
    }

    fn high(&mut self, x: f32) -> f32 {
        x - self.low(x)
    }
}

/// A triode: grid conduction rounds the top, cutoff clips the bottom earlier and harder.
fn triode(x: f32) -> f32 {
    if x >= 0.0 {
        x.tanh()
    } else {
        (1.6 * x).tanh() / 1.6
    }
}

/// A preamp stage: gain (dB), coupling high-pass and Miller low-pass (Hz), operating point.
#[derive(Clone, Copy, Debug)]
struct Stage {
    gain: f32,
    hp: f32,
    lp: f32,
    bias: f32,
}

const fn st(gain: f32, hp: f32, lp: f32, bias: f32) -> Stage {
    Stage { gain, hp, lp, bias }
}

/// A built-in amp.
#[derive(Debug)]
pub struct Model {
    pub name: &'static str,
    /// Look of the face: blackface, tweed, voxy, plexi, boutique, modern or bass.
    pub face: &'static str,
    pub based_on: &'static str,
    pub blurb: &'static str,
    /// Matching cabinet (index in the rig's cabinets).
    pub cab: usize,
    stages: &'static [Stage],
    /// Index of the stage the tone stack comes before (the number of stages: after them).
    stack_at: usize,
    stack: Stack,
    /// The gain knob turns down the signal after the first stage by up to this (dB).
    pot: f32,
    /// Bright capacitor: treble lift (dB) at low gain settings.
    bright: f32,
    /// Power stage drive (dB), plus `track` at full gain on amps without a master volume.
    power: f32,
    track: f32,
    sag: f32,
    /// Negative feedback: how much presence and resonance act.
    nfb: f32,
    lp: f32,
}

/// Rig cabinets, by index.
pub const CAB_V30: usize = 0;
pub const CAB_GREENBACK: usize = 1;
pub const CAB_BLUE: usize = 2;
pub const CAB_US212: usize = 3;
pub const CAB_US410: usize = 4;
pub const CAB_US112: usize = 5;
pub const CAB_BASS410: usize = 6;
pub const CAB_BASS810: usize = 7;
pub const CAB_BASS115: usize = 8;

/// The built-in amps, after "Capture" in the rig's model list. Chosen among the most played
/// and most downloaded amps (TONE3000 download counts, classic amp lists). Those with captures
/// in [`crate::rig::FACTORY`] play them; the others are modeled here.
pub const MODELS: [Model; 16] = [
    Model {
        name: "US Twin",
        face: "blackface",
        based_on: "Fender Twin Reverb, normal and vibrato channels (captures by Tim R)",
        blurb: "Huge clean headroom, sparkling highs, scooped mids.",
        cab: CAB_US212,
        stages: &[
            st(4.0, 20.0, 14_000.0, 0.15),
            st(12.0, 20.0, 12_000.0, 0.2),
            st(4.0, 15.0, 14_000.0, 0.05),
        ],
        stack_at: 1,
        stack: TWIN,
        pot: 30.0,
        bright: 6.0,
        power: -2.0,
        track: 9.0,
        sag: 0.1,
        nfb: 1.0,
        lp: 10_000.0,
    },
    Model {
        name: "US Super 4x10",
        face: "blackface",
        based_on: "Fender Super Reverb (1965)",
        blurb: "The most downloaded capture: clean that breaks up when you dig in.",
        cab: CAB_US410,
        stages: &[
            st(6.0, 20.0, 12_000.0, 0.15),
            st(16.0, 20.0, 11_000.0, 0.2),
            st(6.0, 15.0, 12_000.0, 0.05),
        ],
        stack_at: 1,
        stack: BLACKFACE,
        pot: 30.0,
        bright: 5.0,
        power: 2.0,
        track: 8.0,
        sag: 0.2,
        nfb: 0.9,
        lp: 9000.0,
    },
    Model {
        name: "US Deluxe",
        face: "blackface",
        based_on: "Fender Deluxe Reverb (1965)",
        blurb: "Small combo, sweet early breakup, the studio favourite.",
        cab: CAB_US112,
        stages: &[
            st(8.0, 20.0, 11_000.0, 0.15),
            st(18.0, 25.0, 10_000.0, 0.2),
            st(8.0, 15.0, 11_000.0, 0.05),
        ],
        stack_at: 1,
        stack: DELUXE,
        pot: 30.0,
        bright: 0.0,
        power: 5.0,
        track: 10.0,
        sag: 0.35,
        nfb: 0.8,
        lp: 8000.0,
    },
    Model {
        name: "Tweed Deluxe 57",
        face: "tweed",
        based_on: "Fender Tweed Deluxe 5E3 (1957)",
        blurb: "Fat, raw, compressed breakup that cleans up with the guitar volume.",
        cab: CAB_US112,
        stages: &[
            st(10.0, 40.0, 12_000.0, 0.25),
            st(16.0, 60.0, 10_000.0, 0.3),
            st(8.0, 30.0, 11_000.0, 0.1),
        ],
        stack_at: 1,
        stack: BASSMAN,
        pot: 28.0,
        bright: 8.0,
        power: 7.0,
        track: 12.0,
        sag: 0.6,
        nfb: 0.0,
        lp: 8500.0,
    },
    Model {
        name: "Tweed Bass 59",
        face: "tweed",
        based_on: "Fender Bassman 5F6-A (1959)",
        blurb: "Punchy mids and smooth drive: the root of the British amps.",
        cab: CAB_US410,
        stages: &[
            st(8.0, 60.0, 12_000.0, 0.2),
            st(14.0, 120.0, 11_000.0, 0.25),
            st(6.0, 30.0, 12_000.0, 0.1),
        ],
        stack_at: 2,
        stack: BASSMAN,
        pot: 30.0,
        bright: 6.0,
        power: 4.0,
        track: 10.0,
        sag: 0.4,
        nfb: 0.6,
        lp: 9000.0,
    },
    Model {
        name: "Voxy 15 Top Boost",
        face: "voxy",
        based_on: "Vox AC15 (captures by Phillipe P)",
        blurb: "Chiming class A breakup, from jangle to the Top Boost's bite.",
        cab: CAB_BLUE,
        stages: &[
            st(12.0, 40.0, 13_000.0, 0.2),
            st(20.0, 60.0, 12_000.0, 0.25),
            st(6.0, 20.0, 12_000.0, 0.15),
        ],
        stack_at: 2,
        stack: AC30,
        pot: 30.0,
        bright: 2.0,
        power: 6.0,
        track: 14.0,
        sag: 0.5,
        nfb: 0.0,
        lp: 9000.0,
    },
    Model {
        name: "Brit 45 Bluesbreaker",
        face: "plexi",
        based_on: "Marshall JTM45 / 1962 Bluesbreaker (1966)",
        blurb: "Warm, round crunch with a soft attack.",
        cab: CAB_GREENBACK,
        stages: &[
            st(10.0, 40.0, 12_000.0, 0.2),
            st(14.0, 100.0, 11_000.0, 0.25),
            st(4.0, 30.0, 12_000.0, 0.0),
        ],
        stack_at: 2,
        stack: JTM45,
        pot: 30.0,
        bright: 9.0,
        power: 6.0,
        track: 10.0,
        sag: 0.5,
        nfb: 0.6,
        lp: 9000.0,
    },
    Model {
        name: "Brit Plexi 100",
        face: "plexi",
        based_on: "Ceriatone King Kong, a Plexi-style head (captures by George B)",
        blurb: "The classic rock roar: crank it for crunch and sustain.",
        cab: CAB_GREENBACK,
        stages: &[
            st(12.0, 40.0, 12_000.0, 0.2),
            st(16.0, 110.0, 10_000.0, 0.25),
            st(4.0, 30.0, 11_000.0, 0.0),
        ],
        stack_at: 2,
        stack: PLEXI,
        pot: 30.0,
        bright: 8.0,
        power: 7.0,
        track: 10.0,
        sag: 0.4,
        nfb: 0.7,
        lp: 9000.0,
    },
    Model {
        name: "Brit 900",
        face: "plexi",
        based_on: "Marshall JCM900 50 W Dual Reverb (captures by Tim R)",
        blurb: "From British clean to hard rock and lead, eight captures of gain.",
        cab: CAB_V30,
        stages: &[
            st(12.0, 30.0, 9000.0, 0.2),
            st(20.0, 120.0, 7000.0, 0.35),
            st(12.0, 40.0, 7000.0, 0.25),
            st(2.0, 20.0, 9000.0, 0.0),
        ],
        stack_at: 3,
        stack: JCM800,
        pot: 40.0,
        bright: 4.0,
        power: 3.0,
        track: 0.0,
        sag: 0.3,
        nfb: 0.8,
        lp: 7000.0,
    },
    Model {
        name: "Brit Hot Mod",
        face: "plexi",
        based_on: "Friedman Dirty Shirley Mini (captures by Sascha S)",
        blurb: "A hot-rodded Plexi voice, clean to tight crunch.",
        cab: CAB_V30,
        stages: &[
            st(12.0, 40.0, 9000.0, 0.2),
            st(20.0, 150.0, 7000.0, 0.35),
            st(16.0, 60.0, 6500.0, 0.3),
            st(2.0, 20.0, 9000.0, 0.0),
        ],
        stack_at: 3,
        stack: JCM800,
        pot: 42.0,
        bright: 3.0,
        power: 3.0,
        track: 0.0,
        sag: 0.25,
        nfb: 0.8,
        lp: 6800.0,
    },
    Model {
        name: "Boutique 59",
        face: "boutique",
        based_on: "Magnatone Super 59 Mk II (captures by Tim R)",
        blurb: "Lush boutique cleans that bloom into warm drive.",
        cab: CAB_US212,
        stages: &[
            st(8.0, 20.0, 11_000.0, 0.15),
            st(18.0, 60.0, 8000.0, 0.25),
            st(12.0, 40.0, 7000.0, 0.2),
            st(4.0, 20.0, 10_000.0, 0.0),
        ],
        stack_at: 1,
        stack: BLUES,
        pot: 34.0,
        bright: 3.0,
        power: 3.0,
        track: 0.0,
        sag: 0.2,
        nfb: 0.9,
        lp: 8000.0,
    },
    Model {
        name: "Cali Mark IV",
        face: "modern",
        based_on: "Mesa/Boogie Mark IV (capture by Roman A)",
        blurb: "Focused, singing Californian lead with a tight low end.",
        cab: CAB_V30,
        stages: &[
            st(12.0, 30.0, 9000.0, 0.2),
            st(20.0, 100.0, 7000.0, 0.35),
            st(18.0, 60.0, 6000.0, 0.35),
            st(12.0, 40.0, 6000.0, 0.3),
            st(2.0, 20.0, 9000.0, 0.0),
        ],
        stack_at: 4,
        stack: MESA,
        pot: 50.0,
        bright: 2.0,
        power: 4.0,
        track: 0.0,
        sag: 0.45,
        nfb: 0.6,
        lp: 6500.0,
    },
    Model {
        name: "Panama 52",
        face: "modern",
        based_on: "Peavey 5150 II (captures by Tim R)",
        blurb: "Tight high gain, from clean-ish to the classic lead channel.",
        cab: CAB_V30,
        stages: &[
            st(12.0, 60.0, 9000.0, 0.2),
            st(22.0, 200.0, 7000.0, 0.4),
            st(18.0, 80.0, 6000.0, 0.35),
            st(14.0, 60.0, 6000.0, 0.3),
            st(2.0, 20.0, 9000.0, 0.0),
        ],
        stack_at: 4,
        stack: SOLDANO,
        pot: 50.0,
        bright: 3.0,
        power: 3.0,
        track: 0.0,
        sag: 0.2,
        nfb: 0.8,
        lp: 6500.0,
    },
    Model {
        name: "Brit Lead 100",
        face: "modern",
        based_on: "Laney GH100S (captures by Phillipe P)",
        blurb: "British high gain: from just past breakup to a saturated lead.",
        cab: CAB_V30,
        stages: &[
            st(12.0, 40.0, 9000.0, 0.2),
            st(20.0, 150.0, 7500.0, 0.35),
            st(18.0, 70.0, 6500.0, 0.35),
            st(12.0, 50.0, 6500.0, 0.3),
            st(2.0, 20.0, 9000.0, 0.0),
        ],
        stack_at: 4,
        stack: TRIPLE,
        pot: 50.0,
        bright: 2.0,
        power: 3.0,
        track: 0.0,
        sag: 0.25,
        nfb: 0.8,
        lp: 7000.0,
    },
    Model {
        name: "Bass Classic",
        face: "bass",
        based_on: "Ampeg SVT",
        blurb: "The rock bass amp: warm, huge, growls when pushed.",
        cab: CAB_BASS810,
        stages: &[
            st(6.0, 10.0, 10_000.0, 0.1),
            st(12.0, 10.0, 8000.0, 0.2),
            st(4.0, 10.0, 10_000.0, 0.05),
        ],
        stack_at: 1,
        stack: AMPEG,
        pot: 30.0,
        bright: 0.0,
        power: 2.0,
        track: 6.0,
        sag: 0.3,
        nfb: 0.7,
        lp: 6000.0,
    },
    Model {
        name: "Bass Flip 15",
        face: "bass",
        based_on: "Ampeg B-15 Portaflex",
        blurb: "Round, vintage studio bass tone.",
        cab: CAB_BASS115,
        stages: &[
            st(6.0, 10.0, 8000.0, 0.15),
            st(12.0, 10.0, 7000.0, 0.2),
            st(4.0, 10.0, 8000.0, 0.05),
        ],
        stack_at: 1,
        stack: FLIPTOP,
        pot: 30.0,
        bright: 0.0,
        power: 3.0,
        track: 8.0,
        sag: 0.4,
        nfb: 0.5,
        lp: 5000.0,
    },
];

const MAX_STAGES: usize = 5;

/// Coefficients and state of a model, at the oversampled rate.
#[derive(Debug, Default)]
struct Chain {
    count: usize,
    gains: [f32; MAX_STAGES],
    bias: [f32; MAX_STAGES],
    rest: [f32; MAX_STAGES],
    hp: [OnePole; MAX_STAGES],
    lp: [OnePole; MAX_STAGES],
    input: OnePole,
    stack_at: usize,
    stack: ToneStack,
    bright: (Coefficients, Biquad),
    /// The bright shelf's boost (dB) as heard, gliding to what the gain knob asks: its
    /// coefficients changed at once clicked, the strongest step of all.
    bright_db: f32,
    bright_target: f32,
    presence: (Coefficients, Biquad),
    resonance: (Coefficients, Biquad),
    drive: f32,
    /// What the knobs ask for: the stage gains, the drive and the makeup glide there (see
    /// `GLIDE_MS`), as a step in a chain of saturating stages clicks.
    target_gains: [f32; MAX_STAGES],
    target_drive: f32,
    target_makeup: f32,
    glide: f32,
    sag: f32,
    env: f32,
    attack: f32,
    release: f32,
    out_hp: OnePole,
    out_lp: OnePole,
    makeup: f32,
}

impl Chain {
    fn tick(&mut self, x: f32) -> f32 {
        let g = self.glide;
        for (now, target) in self
            .gains
            .iter_mut()
            .zip(&self.target_gains)
            .take(self.count)
        {
            *now = target + g * (*now - target);
        }
        self.drive = self.target_drive + g * (self.drive - self.target_drive);
        self.makeup = self.target_makeup + g * (self.makeup - self.target_makeup);
        let mut v = self.input.high(x);
        for i in 0..self.count {
            if i == self.stack_at {
                v = self.stack.tick(f64::from(v)) as f32;
            }
            if i > 0 {
                v = self.hp[i].high(v);
            }
            // Each stage inverts, so the asymmetry alternates as in a real preamp.
            v = -(triode(v * self.gains[i] + self.bias[i]) - self.rest[i]);
            v = self.lp[i].low(v);
            if i == 0 {
                v = self.bright.1.process(&self.bright.0, f64::from(v)) as f32;
            }
        }
        if self.stack_at >= self.count {
            v = self.stack.tick(f64::from(v)) as f32;
        }
        let mut p = f64::from(v);
        p = self.presence.1.process(&self.presence.0, p);
        p = self.resonance.1.process(&self.resonance.0, p);
        let drive = self.drive / (1.0 + self.sag * self.env);
        let y = (p as f32 * drive).tanh();
        let level = y.abs();
        let c = if level > self.env {
            self.attack
        } else {
            self.release
        };
        self.env = level + c * (self.env - level);
        self.out_lp.low(self.out_hp.high(y)) * self.makeup
    }
}

/// How long the gain-driven values take to reach a knob's new setting (time constant).
const GLIDE_MS: f32 = 10.0;
/// Samples between two designs of the gliding bright shelf (32 still clicked a little).
const BRIGHT_STEP: usize = 8;

/// The bright channel's high shelf for a boost in dB, at the oversampled rate `os`.
fn bright(db: f32, os: f64) -> Coefficients {
    Coefficients::design(FilterKind::HighShelf, 2500.0, 0.7, f64::from(db), os)
}

/// A built-in amp running one of `MODELS`.
#[derive(Debug)]
pub struct AmpSim {
    stage: Oversampler,
    chain: Chain,
    model: Option<usize>,
    /// The oversampled rate the filters are designed at.
    os: f64,
}

impl Default for AmpSim {
    fn default() -> Self {
        Self {
            stage: Oversampler::new(Taps::Short),
            chain: Chain::default(),
            model: None,
            os: 48_000.0 * FACTOR as f64,
        }
    }
}

impl AmpSim {
    /// Sets the model (index in `MODELS`) and its knobs (0 to 10).
    pub fn update(&mut self, model: usize, knobs: Knobs, rate: f64) {
        let Some(m) = MODELS.get(model) else {
            return;
        };
        // Another model starts where its knobs are; the same one glides to them.
        let jump = self.model != Some(model);
        if jump {
            self.reset();
            self.model = Some(model);
        }
        let os = rate * FACTOR as f64;
        let fs = os as f32;
        let k = |v: f32| (v / 10.0).clamp(0.0, 1.0);
        let gain = k(knobs.gain);
        let c = &mut self.chain;
        c.count = m.stages.len().min(MAX_STAGES);
        let pot = db_to_gain(-m.pot * (1.0 - gain).powf(1.6));
        for (i, s) in m.stages.iter().take(c.count).enumerate() {
            c.target_gains[i] = db_to_gain(s.gain) * if i == 1 { pot } else { 1.0 };
            c.bias[i] = s.bias;
            c.rest[i] = triode(s.bias);
            c.hp[i].set(s.hp, fs);
            c.lp[i].set(s.lp, fs);
        }
        c.input.set(30.0, fs);
        c.stack_at = m.stack_at;
        let (t, mid, bass) = (
            f64::from(k(knobs.treble)),
            f64::from(k(knobs.mid)),
            f64::from(k(knobs.bass)),
        );
        c.stack.design(&m.stack, t, mid, bass, os);
        let design = |kind, f: f64, q, g: f32| Coefficients::design(kind, f, q, f64::from(g), os);
        self.os = os;
        c.bright_target = m.bright * (1.0 - 0.6 * gain);
        if jump {
            c.bright_db = c.bright_target;
            c.bright.0 = bright(c.bright_db, os);
        }
        let presence = (k(knobs.presence) - 0.5) * 2.0 * 8.0 * (0.25 + 0.75 * m.nfb);
        c.presence.0 = design(FilterKind::HighShelf, 3500.0, 0.7, presence);
        c.resonance.0 = design(FilterKind::Peak, 90.0, 0.9, 3.0 * m.nfb);
        let drive_db = m.power + m.track * gain;
        c.target_drive = db_to_gain(drive_db);
        c.glide = (-1.0 / (GLIDE_MS * 0.001 * fs)).exp();
        c.sag = m.sag * 3.0;
        c.attack = (-1.0 / (0.005 * fs)).exp();
        c.release = (-1.0 / (0.15 * fs)).exp();
        c.out_hp.set(50.0, fs);
        c.out_lp.set(m.lp, fs);
        // Level matching: a -10 dBFS note comes out at about the same level at any setting.
        let mut a = 0.3f32;
        let mid_gain = [200.0, 500.0, 1500.0]
            .iter()
            .map(|f| stack_gain(&m.stack, t, mid, bass, *f) as f32)
            .sum::<f32>()
            / 3.0;
        for i in 0..c.count {
            if i == c.stack_at {
                a *= mid_gain;
            }
            a = (triode(a * c.target_gains[i] + c.bias[i]) - c.rest[i]).abs();
        }
        if c.stack_at >= c.count {
            a *= mid_gain;
        }
        let out = (a * c.target_drive).tanh();
        c.target_makeup = 0.5 / out.max(1e-4);
        if jump {
            c.gains = c.target_gains;
            c.drive = c.target_drive;
            c.makeup = c.target_makeup;
        }
    }

    pub fn process(&mut self, buffer: &mut [f32]) {
        // The bright shelf glides by steps of a few samples, each one too small to hear.
        let step =
            (-(BRIGHT_STEP as f32) / (GLIDE_MS * 0.001 * self.os as f32 / FACTOR as f32)).exp();
        for chunk in buffer.chunks_mut(BRIGHT_STEP) {
            let c = &mut self.chain;
            if (c.bright_db - c.bright_target).abs() > 0.001 {
                c.bright_db = c.bright_target + step * (c.bright_db - c.bright_target);
                c.bright.0 = bright(c.bright_db, self.os);
            }
            let chain = &mut self.chain;
            for s in chunk {
                *s = self.stage.run(*s, |x| chain.tick(x));
            }
        }
    }

    pub fn reset(&mut self) {
        self.stage.clear();
        let c = &mut self.chain;
        c.stack.s = [0.0; 3];
        for p in c.hp.iter_mut().chain(c.lp.iter_mut()) {
            p.z = 0.0;
        }
        for p in [&mut c.input, &mut c.out_hp, &mut c.out_lp] {
            p.z = 0.0;
        }
        c.bright.1 = Biquad::default();
        c.presence.1 = Biquad::default();
        c.resonance.1 = Biquad::default();
        c.env = 0.0;
    }
}

/// The amp's knobs, from 0 to 10.
#[derive(Clone, Copy, Debug)]
pub struct Knobs {
    pub gain: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub presence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knobs(gain: f32) -> Knobs {
        Knobs {
            gain,
            bass: 5.0,
            mid: 5.0,
            treble: 5.0,
            presence: 5.0,
        }
    }

    /// Output level and share of harmonics for a 220 Hz sine at -10 dBFS.
    fn measure(model: usize, gain: f32) -> (f32, f32) {
        measure_at(model, gain, 0.3)
    }

    /// How much quieter a loud note gets than a soft one, in dB (0 when clean).
    fn compression(model: usize, gain: f32) -> f32 {
        let loud = measure_at(model, gain, 0.3).0;
        let soft = measure_at(model, gain, 0.03).0;
        20.0 * (10.0 * soft / loud).log10()
    }

    fn measure_at(model: usize, gain: f32, amplitude: f32) -> (f32, f32) {
        let mut amp = AmpSim::default();
        amp.update(model, knobs(gain), 48_000.0);
        let f = 220.0;
        let tau = std::f32::consts::TAU / 48_000.0;
        let mut buffer: Vec<f32> = (0..48_000)
            .map(|i| {
                let t = i as f32 * tau;
                amplitude
                    * (0.6 * (t * f).sin() + 0.3 * (t * f * 3.0).sin() + 0.1 * (t * f * 7.0).sin())
            })
            .collect();
        amp.process(&mut buffer);
        let tail = &buffer[24_000..];
        let power = tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, s) in tail.iter().enumerate() {
            let a = (i + 24_000) as f32 * f * std::f32::consts::TAU / 48_000.0;
            re += s * a.cos();
            im += s * a.sin();
        }
        let n = tail.len() as f32;
        let fundamental = 2.0 * (re * re + im * im) / (n * n);
        (
            power.sqrt(),
            (1.0 - fundamental / power.max(1e-12)).max(0.0),
        )
    }

    #[test]
    fn tone_stack_scoops_the_mids_and_follows_its_knobs() {
        let at = |t, m, b, f| stack_gain(&BASSMAN, t, m, b, f);
        assert!(at(0.5, 0.5, 0.5, 500.0) < at(0.5, 0.5, 0.5, 80.0));
        assert!(at(0.5, 0.5, 0.5, 500.0) < at(0.5, 0.5, 0.5, 4000.0));
        assert!(at(1.0, 0.5, 0.5, 5000.0) > at(0.0, 0.5, 0.5, 5000.0) * 2.0);
        assert!(at(0.5, 0.5, 1.0, 60.0) > at(0.5, 0.5, 0.0, 60.0) * 2.0);
        assert!(at(0.5, 1.0, 0.5, 600.0) > at(0.5, 0.0, 0.5, 600.0) * 1.5);
        // The discrete filter matches the analog response.
        let mut ts = ToneStack::default();
        ts.design(&BASSMAN, 0.5, 0.5, 0.5, 192_000.0);
        let f = 1000.0;
        let mut peak = 0.0f64;
        for i in 0..192_000 {
            let y = ts.tick((i as f64 * f * std::f64::consts::TAU / 192_000.0).sin());
            if i > 96_000 {
                peak = peak.max(y.abs());
            }
        }
        let expected = at(0.5, 0.5, 0.5, f);
        assert!((peak / expected - 1.0).abs() < 0.02, "{peak} vs {expected}");
    }

    #[test]
    fn every_model_is_bounded_level_and_gains_with_its_knob() {
        for (index, model) in MODELS.iter().enumerate() {
            let low = measure(index, 1.0);
            let mid = measure(index, 5.0);
            let high = measure(index, 9.0);
            for (level, _) in [low, mid, high] {
                assert!(
                    level.is_finite() && level > 0.05 && level < 1.0,
                    "{}",
                    model.name
                );
            }
            assert!(
                high.0 / low.0 < 3.0 && low.0 / high.0 < 3.0,
                "{} level {low:?} {high:?}",
                model.name
            );
            assert!(
                compression(index, 9.0) > compression(index, 1.0) + 1.0,
                "{} saturates more at high gain",
                model.name
            );
        }
    }

    #[test]
    #[ignore = "prints the voicing table"]
    fn print_voicing() {
        for (index, model) in MODELS.iter().enumerate() {
            let row: Vec<String> = [1.0, 3.0, 5.0, 7.0, 9.0]
                .iter()
                .map(|g| {
                    let (level, _) = measure(index, *g);
                    format!("{:5.1} dB {:4.2}", compression(index, *g), level)
                })
                .collect();
            println!("{:22} {}", model.name, row.join(" | "));
        }
    }
}
