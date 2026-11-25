//! The rotary speaker of the organs: a crossover at 800 Hz, a horn turning for the highs and a
//! drum for the lows, each at its own speed, slow or fast, reaching it with its own inertia
//! (the light horn in a moment, the heavy drum in seconds). Each turns its sound toward and
//! away from two microphones: the pitch rises and falls with the motion (Doppler, a delay
//! swept by the rotation) and the level with the angle.

use std::f32::consts::{PI, TAU};

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::line::Line;

/// The speeds, in parameter order: off (straight through), stopped, slow and fast.
pub(crate) const SPEEDS: [&str; 4] = ["Off", "Stop", "Slow", "Fast"];
pub(crate) const OFF: usize = 0;
pub(crate) const STOP: usize = 1;
pub(crate) const SLOW: usize = 2;
pub(crate) const FAST: usize = 3;

const CROSSOVER: f64 = 800.0;

/// One rotor: its speeds (Hz), the time it takes to speed up and slow down (s), the sweep of
/// its delay (ms) and how much its level swings.
#[derive(Debug)]
struct Rotor {
    slow: f32,
    fast: f32,
    rise: f32,
    fall: f32,
    sweep_ms: f32,
    swing: f32,
    turn: f32,
    speed: f32,
    /// How much its rise and fall times are stretched.
    inertia: f32,
    /// Its level in the sum.
    gain: f32,
    line: Line,
}

impl Rotor {
    fn new(slow: f32, fast: f32, (rise, fall): (f32, f32), sweep_ms: f32, swing: f32) -> Self {
        Self {
            slow,
            fast,
            rise,
            fall,
            sweep_ms,
            swing,
            turn: 0.0,
            speed: slow,
            inertia: 1.0,
            gain: 1.0,
            line: Line::default(),
        }
    }

    /// What the microphone at `side` (radians from the front) hears now.
    #[inline]
    fn hear(&self, side: f32, base: f32, sweep: f32) -> f32 {
        let angle = TAU * self.turn + side;
        let (sin, cos) = angle.sin_cos();
        let level = 1.0 - self.swing * (0.5 - 0.5 * cos);
        self.line.read(base + sweep * sin) * level * self.gain
    }

    #[inline]
    fn spin(&mut self, target: f32, rate: f32) {
        let time = self.inertia
            * if target > self.speed {
                self.rise
            } else {
                self.fall
            };
        self.speed += (target - self.speed) * (1.0 - (-1.0 / (time * rate)).exp());
        self.turn = (self.turn + self.speed / rate).fract();
    }
}

#[derive(Debug)]
pub(crate) struct Rotary {
    rate: f32,
    low: Coefficients,
    high: Coefficients,
    lows: [Biquad; 2],
    highs: [Biquad; 2],
    /// The cabinet and its horn mouth: little above 8 kHz.
    cabinet: Coefficients,
    cabinets: [Biquad; 2],
    horn: Rotor,
    drum: Rotor,
    /// How much of the rotary is heard, gliding when it is switched on or off.
    share: f32,
}

impl Default for Rotary {
    fn default() -> Self {
        let mut rotary = Self {
            rate: 48_000.0,
            low: Coefficients::IDENTITY,
            high: Coefficients::IDENTITY,
            lows: [Biquad::default(); 2],
            highs: [Biquad::default(); 2],
            cabinet: Coefficients::IDENTITY,
            cabinets: [Biquad::default(); 2],
            // Level swings as measured on a cabinet: about 10 dB for the drum, more for the horn.
            horn: Rotor::new(0.83, 6.75, (0.7, 0.9), 0.45, 0.8),
            drum: Rotor::new(0.67, 5.9, (3.5, 4.5), 0.25, 0.68),
            share: 1.0,
        };
        rotary.prepare(48_000);
        rotary
    }
}

impl Rotary {
    pub fn prepare(&mut self, rate: u32) {
        self.rate = rate as f32;
        let r = f64::from(rate);
        // Two Butterworth stages each side: a Linkwitz-Riley crossover, flat when summed.
        self.low = Coefficients::design(FilterKind::LowPass, CROSSOVER, 0.707, 0.0, r);
        self.high = Coefficients::design(FilterKind::HighPass, CROSSOVER, 0.707, 0.0, r);
        self.cabinet = Coefficients::design(FilterKind::LowPass, 5500.0, 0.707, 0.0, r);
        let frames = (0.012 * self.rate) as usize + 4;
        self.horn.line = Line::new(frames);
        self.drum.line = Line::new(frames);
        self.reset();
    }

    /// Stretches the time the rotors take to change speed (1 as built).
    pub fn set_inertia(&mut self, factor: f32) {
        self.horn.inertia = factor.max(0.05);
        self.drum.inertia = factor.max(0.05);
    }

    /// The horn against the drum, from -1 (drum alone) to 1 (horn alone).
    pub fn set_balance(&mut self, balance: f32) {
        self.horn.gain = (1.0 + balance).clamp(0.0, 1.0);
        self.drum.gain = (1.0 - balance).clamp(0.0, 1.0);
    }

    /// Where the horn and the drum have turned (0 to 1) and their speeds (Hz).
    pub fn motion(&self) -> [f32; 4] {
        [
            self.horn.turn,
            self.drum.turn,
            self.horn.speed,
            self.drum.speed,
        ]
    }

    pub fn reset(&mut self) {
        self.lows = [Biquad::default(); 2];
        self.highs = [Biquad::default(); 2];
        self.cabinets = [Biquad::default(); 2];
        self.horn.line.clear();
        self.drum.line.clear();
    }

    /// Turns the mono sum of `left` and `right` through the rotors at `speed` (in
    /// [`SPEEDS`]), the microphones `width` apart (0 to 1).
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], speed: usize, width: f32) {
        let rate = self.rate;
        let wanted = if speed == OFF { 0.0 } else { 1.0 };
        let glide = 1.0 - (-1.0 / (0.03 * rate)).exp();
        let targets = |rotor: &Rotor| match speed {
            STOP => 0.0,
            FAST => rotor.fast,
            _ => rotor.slow,
        };
        let (horn_target, drum_target) = (targets(&self.horn), targets(&self.drum));
        let ms = rate / 1000.0;
        let (horn_sweep, drum_sweep) = (self.horn.sweep_ms * ms, self.drum.sweep_ms * ms);
        let (horn_base, drum_base) = (horn_sweep + ms, drum_sweep + ms);
        let side = width.clamp(0.0, 1.0) * PI / 2.0;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.share += glide * (wanted - self.share);
            self.horn.spin(horn_target, rate);
            self.drum.spin(drum_target, rate);
            if self.share < 1e-4 && wanted == 0.0 {
                continue;
            }
            let x = f64::from(0.5 * (*l + *r));
            let mut lo = x;
            for f in &mut self.lows {
                lo = f.process(&self.low, lo);
            }
            let mut hi = x;
            for f in &mut self.highs {
                hi = f.process(&self.high, hi);
            }
            for f in &mut self.cabinets {
                hi = f.process(&self.cabinet, hi);
            }
            self.horn.line.push(hi as f32);
            self.drum.line.push(lo as f32);
            let mut mics = [0.0f32; 2];
            for (k, mic) in mics.iter_mut().enumerate() {
                let at = if k == 0 { -side } else { side };
                *mic = self.horn.hear(at, horn_base, horn_sweep)
                    + self.drum.hear(at, drum_base, drum_sweep);
            }
            *l += self.share * (mics[0] - *l);
            *r += self.share * (mics[1] - *r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_horn_speeds_up_fast_and_the_drum_slowly() {
        let mut rotary = Rotary::default();
        let (mut l, mut r) = (vec![0.1; 48_000], vec![0.1; 48_000]);
        rotary.process(&mut l, &mut r, FAST, 1.0);
        assert!(
            rotary.horn.speed > 0.7 * rotary.horn.fast,
            "{}",
            rotary.horn.speed
        );
        assert!(
            rotary.drum.speed < 0.5 * rotary.drum.fast,
            "{}",
            rotary.drum.speed
        );
        let (mut l, mut r) = (vec![0.0; 48_000 * 4], vec![0.0; 48_000 * 4]);
        rotary.process(&mut l, &mut r, STOP, 1.0);
        assert!(
            rotary.horn.speed < 0.02 * rotary.horn.fast && rotary.drum.speed < 1.0,
            "{} {}",
            rotary.horn.speed,
            rotary.drum.speed
        );
    }

    #[test]
    fn a_tone_comes_out_moving_and_straight_when_off() {
        let mut rotary = Rotary::default();
        let tone: Vec<f32> = (0..48_000)
            .map(|i| (TAU * 2000.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (tone.clone(), tone.clone());
        rotary.process(&mut l, &mut r, FAST, 1.0);
        let envelope = |x: &[f32]| {
            x.chunks(480)
                .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
                .collect::<Vec<_>>()
        };
        let e = envelope(&l[24_000..]);
        let (lo, hi) = e
            .iter()
            .fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(hi - lo > 0.1, "the horn swings the level: {lo} {hi}");
        assert!(l[24_000..] != r[24_000..], "the two microphones differ");
        let mut rotary = Rotary::default();
        let (mut l, mut r) = (tone.clone(), tone.clone());
        rotary.share = 0.0;
        rotary.process(&mut l, &mut r, OFF, 1.0);
        assert_eq!(l, tone);
    }
}
