//! A gain cell as the vintage compressors have them, shared by the compressor's characters and
//! the Vintage pack: an optical cell (an electroluminescent panel lit by the signal, shining on
//! a photoresistor) or a field-effect transistor, with the same parts and their own constants:
//!
//! - its detection follows the rectified signal, quickly but not instantly: the optical cell's
//!   light reads a tone's mean, and the crests of low notes, whose half periods it has time to
//!   follow (the lows get more reduction than the mids);
//! - the cell answers in two parts: a fast one, which lets go in tens of milliseconds, and a
//!   memory that charges while it is lit and lets go over seconds. A brief peak barely charges
//!   it; a long loud passage does, and the release slows: the program-dependent release;
//! - the fast part itself lets go slower once the memory is charged.
//!
//! - the element itself colors the signal the more it works ([`Grain`]).
//!
//! The constants were fitted on measurements of reference models of the units (see
//! `measure_dynamics`). The amplifier around a cell, its second harmonic then its third and an
//! output stage running out of headroom above full scale, is [`shape`].

use crate::gain_to_db;

/// A tone's mean, rectified, against its peak: the light is calibrated so that a steady tone
/// reads its peak, as the thresholds are given.
const MEAN_TO_PEAK: f32 = std::f32::consts::FRAC_PI_2;

/// The cell's constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// The light's rise and fall, ms.
    pub light_rise_ms: f32,
    pub light_fall_ms: f32,
    /// Width of the knee, dB.
    pub knee_db: f32,
    /// The fast part's attack, and its release with the memory empty and full, ms.
    pub attack_ms: f32,
    pub release_ms: f32,
    pub release_charged_ms: f32,
    /// Share of the reduction the memory holds once charged.
    pub memory: f32,
    /// How fast the memory charges, and lets go, ms.
    pub charge_ms: f32,
    pub memory_release_ms: f32,
    /// The element's own harmonics once it is at work, against the signal's envelope
    /// (second, third); the reduction at which they are all there; how fast that envelope
    /// follows, ms (it follows the crests of low notes, which it distorts more).
    pub grain_even: f32,
    pub grain_odd: f32,
    pub grain_onset_db: f32,
    pub grain_ms: f32,
    /// The memory holds the reduction rather than sharing it: the cell attacks fully at once
    /// and the memory, once charged, only slows its release (the variable-mu tube's way).
    pub peak_memory: bool,
}

/// The optical cell, as fitted on a reference model of the leveling amplifier.
pub const OPTO: Tuning = Tuning {
    light_rise_ms: 0.71,
    light_fall_ms: 0.71,
    knee_db: 8.0,
    attack_ms: 0.03,
    release_ms: 10.5,
    release_charged_ms: 90.0,
    memory: 0.205,
    charge_ms: 71.5,
    memory_release_ms: 2_018.0,
    grain_even: 0.003,
    grain_odd: 0.005,
    grain_onset_db: 6.0,
    grain_ms: 1_000.0,
    peak_memory: false,
};

/// The static curve: where it starts and how it grips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    /// Level of a steady tone, its peak in dBFS, where the reduction starts.
    pub threshold: f32,
    /// Reduction per dB over the threshold: 1 - 1/ratio.
    pub slope: f32,
    /// Change of that slope per dB over: negative as the cell eases off when deep, positive
    /// as a limiter grips harder.
    pub bend: f32,
    pub knee: f32,
}

impl Curve {
    /// A plain ratio, as the compressor sets it.
    pub fn ratio(threshold: f32, ratio: f32) -> Self {
        Self {
            threshold,
            slope: 1.0 - 1.0 / ratio.max(1.0),
            bend: 0.0,
            knee: OPTO.knee_db,
        }
    }

    /// Reduction in dB (positive) for a level in dBFS.
    pub fn reduction(&self, level: f32) -> f32 {
        let over = level - self.threshold;
        let half = 0.5 * self.knee;
        let u = if over <= -half {
            return 0.0;
        } else if over < half {
            (over + half).powi(2) / (2.0 * self.knee)
        } else {
            over
        };
        // Past where an easing curve would turn back, it stays flat.
        let u = if self.bend < 0.0 {
            u.min(-self.slope / (2.0 * self.bend))
        } else {
            u
        };
        (self.slope * u + self.bend * u * u).max(0.0)
    }
}

/// One-pole coefficients of a tuning at a sample rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Times {
    rise: f32,
    fall: f32,
    attack: f32,
    release: f32,
    release_charged: f32,
    charge: f32,
    memory_release: f32,
    memory: f32,
    grain: f32,
    grain_even: f32,
    grain_odd: f32,
    grain_onset: f32,
    peak: bool,
}

fn coefficient(ms: f32, rate: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * rate)).exp()
    }
}

impl Times {
    pub fn new(t: &Tuning, rate: f32) -> Self {
        Self {
            rise: coefficient(t.light_rise_ms, rate),
            fall: coefficient(t.light_fall_ms, rate),
            attack: coefficient(t.attack_ms, rate),
            release: coefficient(t.release_ms, rate),
            release_charged: coefficient(t.release_charged_ms, rate),
            charge: coefficient(t.charge_ms, rate),
            memory_release: coefficient(t.memory_release_ms, rate),
            memory: t.memory.clamp(0.0, 1.0),
            grain: coefficient(t.grain_ms, rate),
            grain_even: t.grain_even,
            grain_odd: t.grain_odd,
            grain_onset: t.grain_onset_db.max(0.1),
            peak: t.peak_memory,
        }
    }
}

impl Default for Times {
    fn default() -> Self {
        Self::new(&OPTO, 48_000.0)
    }
}

/// The panel and the photoresistor of one side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cell {
    light: f32,
    fast: f32,
    slow: f32,
}

impl Cell {
    /// Lights the panel with one sample of what the cell hears and returns the reduction to
    /// apply, in dB (positive).
    pub fn push(&mut self, t: &Times, heard: f32, curve: &Curve) -> f32 {
        let x = heard.abs() * MEAN_TO_PEAK;
        let c = if x > self.light { t.rise } else { t.fall };
        self.light = x + c * (self.light - x);
        let target = curve.reduction(gain_to_db(self.light));
        let c = if target > self.fast {
            t.attack
        } else {
            // The charged memory slows the fast part down too.
            let charged = (self.slow / self.fast.max(1e-3)).clamp(0.0, 1.0);
            t.release + (t.release_charged - t.release) * charged
        };
        self.fast = target + c * (self.fast - target);
        let c = if target > self.slow {
            t.charge
        } else {
            t.memory_release
        };
        self.slow = target + c * (self.slow - target);
        self.reduction(t)
    }

    /// The reduction the cell holds, dB (positive).
    pub fn reduction(&self, t: &Times) -> f32 {
        if t.peak {
            self.fast.max(t.memory * self.slow)
        } else {
            (1.0 - t.memory) * self.fast + t.memory * self.slow
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// What the element (photoresistor, FET) does to the signal through it, per side: its harmonics grow as it
/// takes over, from none at rest to all of them past [`Tuning::grain_onset_db`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grain {
    envelope: f32,
}

impl Grain {
    /// One sample out of the cell, `reduction` (dB) being what the cell holds. It makes a
    /// little offset with the second harmonic, which the caller filters out.
    pub fn pass(&mut self, t: &Times, x: f32, reduction: f32) -> f32 {
        let level = x.abs() * MEAN_TO_PEAK;
        self.envelope = level + t.grain * (self.envelope - level);
        let share = (reduction / t.grain_onset).powi(2).min(1.0);
        if share <= 0.0 || self.envelope < 1e-7 {
            return x;
        }
        let u = (x / self.envelope).clamp(-1.5, 1.5);
        x + share * self.envelope * (t.grain_even * u * u - t.grain_odd * u * u * u)
    }

    pub fn clear(&mut self) {
        self.envelope = 0.0;
    }
}

/// The amplifier's color: the second harmonic, then the third.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Amp {
    pub even: f32,
    pub odd: f32,
    /// The output stage's ceiling (1 is full scale), and how sharply it is reached.
    pub ceiling: f32,
    pub sharpness: f32,
}

/// The leveling amplifier's tube stages, as fitted on the reference.
pub const OPTO_AMP: Amp = Amp {
    even: 0.0151,
    odd: 0.0229,
    ceiling: 1.879,
    sharpness: 4.44,
};

/// The field-effect transistor's cell, as fitted on a reference model of the limiting
/// amplifier; its attack and release are the faceplate's, which the caller sets.
pub const FET: Tuning = Tuning {
    light_rise_ms: 0.0014,
    light_fall_ms: 0.142,
    knee_db: 7.5,
    attack_ms: 1.0,
    release_ms: 431.0,
    release_charged_ms: 591.0,
    memory: 0.269,
    charge_ms: 878.0,
    memory_release_ms: 1_566.0,
    grain_even: 0.002,
    grain_odd: 0.002,
    grain_onset_db: 0.2,
    grain_ms: 0.106,
    peak_memory: false,
};

/// The same with all four ratio buttons pressed: the bias of the detector goes wrong, the
/// release collapses and the reduction keeps creeping in.
pub const FET_ALL: Tuning = Tuning {
    attack_ms: 1.54,
    release_ms: 18.3,
    release_charged_ms: 32.2,
    memory: 0.175,
    charge_ms: 688.0,
    memory_release_ms: 596.0,
    grain_even: 0.0085,
    grain_odd: 0.0,
    ..FET
};

/// The limiting amplifier's input stage, as fitted on the reference: its second harmonic grows
/// with the input level, its third stays out of hearing, it never runs out of headroom.
pub const FET_AMP: Amp = Amp {
    even: 0.00453,
    odd: 0.0,
    ceiling: 16.0,
    sharpness: 4.0,
};
/// Slope and bend of the FET cell with all buttons pressed, as fitted on the reference.
pub const FET_ALL_CURVE: (f32, f32) = (0.911, 0.0015);

/// The FET cell's curve at a ratio as the compressor sets it: at the top of its range, all
/// buttons pressed.
pub fn fet_curve(threshold: f32, ratio: f32) -> Curve {
    if ratio >= 19.5 {
        Curve {
            threshold,
            slope: FET_ALL_CURVE.0,
            bend: FET_ALL_CURVE.1,
            knee: FET_ALL.knee_db,
        }
    } else {
        Curve {
            threshold,
            slope: 1.0 - 1.0 / ratio.max(1.0),
            bend: 0.0,
            knee: FET.knee_db,
        }
    }
}

/// The amplifier on one sample: `amount` 1 is the unit's color, 0 none. It makes a little
/// offset with the second harmonic, which the caller filters out.
pub fn shape(x: f32, tube: &Amp, amount: f32) -> f32 {
    if amount <= 0.0 {
        return x;
    }
    let (even, odd) = (tube.even * amount, tube.odd * amount);
    // The odd part flattens out where its slope would reach zero.
    let edge = (1.0 / (3.0 * odd.max(1e-6))).sqrt();
    let u = x.clamp(-edge, edge);
    let y = x + even * x * x - odd * u * u * u;
    let c = tube.ceiling.max(0.1);
    y / (1.0 + (y.abs() / c).powf(tube.sharpness)).powf(1.0 / tube.sharpness)
}

/// How a compressor character with its own attack, release, knee and ratio controls turns them
/// into a cell: the times it really takes, how it leans on the program, its knee and ratio as
/// they are heard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    /// What the attack and release set become, as factors.
    pub attack_scale: f32,
    pub release_scale: f32,
    pub light_rise_ms: f32,
    pub light_fall_ms: f32,
    /// The memory that makes the release lean on the program: its share, how fast it charges
    /// (ms) and lets go (a factor of the release).
    pub memory: f32,
    pub charge_ms: f32,
    pub memory_release_scale: f32,
    /// The same with the automatic release, its letting go in ms whatever the release says.
    pub auto_memory: f32,
    pub auto_charge_ms: f32,
    pub auto_memory_release_ms: f32,
    /// The knee heard for the one set: scaled, then widened.
    pub knee_scale: f32,
    pub knee_add: f32,
    /// How far the ratio heard is from the one set: 1 - 1/(1 + (ratio - 1) × scale).
    pub ratio_scale: f32,
    /// Where it starts for the threshold set, dB above it.
    pub threshold_add: f32,
}

impl Character {
    /// The cell for these settings.
    pub fn tuning(&self, attack_ms: f32, release_ms: f32, auto: bool) -> Tuning {
        let release = release_ms * self.release_scale;
        Tuning {
            light_rise_ms: self.light_rise_ms,
            light_fall_ms: self.light_fall_ms,
            knee_db: 0.0,
            attack_ms: attack_ms * self.attack_scale,
            release_ms: release,
            release_charged_ms: release,
            memory: if auto { self.auto_memory } else { self.memory },
            charge_ms: if auto {
                self.auto_charge_ms
            } else {
                self.charge_ms
            },
            memory_release_ms: if auto {
                self.auto_memory_release_ms
            } else {
                release * self.memory_release_scale
            },
            grain_even: 0.0,
            grain_odd: 0.0,
            grain_onset_db: 1.0,
            grain_ms: 1.0,
            peak_memory: false,
        }
    }

    /// The curve for these settings.
    pub fn curve(&self, threshold: f32, ratio: f32, knee: f32) -> Curve {
        let heard = 1.0 + (ratio.max(1.0) - 1.0) * self.ratio_scale;
        Curve {
            threshold: threshold + self.threshold_add,
            slope: 1.0 - 1.0 / heard,
            bend: 0.0,
            knee: knee * self.knee_scale + self.knee_add,
        }
    }
}

/// The clean character, as fitted on a reference digital compressor in its clean style.
pub const CLEAN: Character = Character {
    attack_scale: 0.5,
    release_scale: 0.42,
    light_rise_ms: 0.01,
    light_fall_ms: 0.42,
    memory: 0.1,
    charge_ms: 595.0,
    memory_release_scale: 8.0,
    auto_memory: 0.55,
    auto_charge_ms: 11.0,
    auto_memory_release_ms: 212.0,
    knee_scale: 0.7,
    knee_add: 4.0,
    ratio_scale: 1.0,
    threshold_add: 5.0,
};

/// The VCA bus character, as fitted on a reference model of the console's bus compressor.
pub const BUS: Character = Character {
    attack_scale: 0.84,
    release_scale: 0.46,
    light_rise_ms: 0.0015,
    light_fall_ms: 0.77,
    memory: 0.0125,
    charge_ms: 273.0,
    memory_release_scale: 0.27,
    auto_memory: 0.64,
    auto_charge_ms: 631.0,
    auto_memory_release_ms: 3_568.0,
    knee_scale: 0.0,
    knee_add: 13.0,
    ratio_scale: 0.875,
    threshold_add: 0.0,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_starts_at_its_threshold_and_follows_its_slope() {
        let curve = Curve::ratio(-20.0, 4.0);
        assert_eq!(curve.reduction(-20.0 - curve.knee), 0.0);
        let deep = curve.reduction(0.0) - curve.reduction(-10.0);
        assert!((deep - 7.5).abs() < 0.01, "{deep}");
        let easing = Curve {
            bend: -0.01,
            ..curve
        };
        assert!(easing.reduction(0.0) < curve.reduction(0.0));
        assert!(easing.reduction(60.0) >= easing.reduction(40.0));
    }

    #[test]
    fn the_memory_charges_with_the_time_spent_lit() {
        let t = Times::new(&OPTO, 48_000.0);
        let curve = Curve::ratio(-30.0, 4.0);
        let tone = |i: usize| 0.5 * (i as f32 * std::f32::consts::TAU / 48.0).sin();
        let held_after = |ms: usize| {
            let mut cell = Cell::default();
            for i in 0..ms * 48 {
                cell.push(&t, tone(i), &curve);
            }
            for _ in 0..48 * 500 {
                cell.push(&t, 0.0, &curve);
            }
            cell.reduction(&t)
        };
        assert!(held_after(2_000) > 3.0 * held_after(5));
    }

    #[test]
    fn the_amplifier_is_clean_when_quiet_and_bounded() {
        assert!((shape(1e-3, &OPTO_AMP, 1.0) - 1e-3).abs() < 1e-6);
        for x in [-10.0f32, -2.0, 2.0, 10.0] {
            assert!(shape(x, &OPTO_AMP, 1.0).abs() <= OPTO_AMP.ceiling * 1.01, "{x}");
        }
    }
}
