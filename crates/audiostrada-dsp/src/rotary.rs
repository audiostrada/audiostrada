//! Strada-Rotary: the rotary speaker of the organs as an effect of its own, for any track. Its
//! tube amplifier first, then the horn and the drum turning slow, fast or coming to a stop,
//! each with its inertia, heard by two microphones.

use crate::keys::rotary::{FAST, Rotary, SLOW, STOP};
use crate::{Effect, EffectInfo, ParamInfo, choice, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "rotary",
    name: "Strada-Rotary",
    params: &PARAMS,
    meters: &METERS,
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Rotary speaker cabinet: horn and drum turning slow or fast with their inertia, \
                  and the drive of its tube amplifier.",
};

pub const SPEEDS: [&str; 3] = ["Stop", "Slow", "Fast"];

pub mod p {
    pub const SPEED: usize = 0;
    pub const DRIVE: usize = 1;
    pub const BALANCE: usize = 2;
    pub const WIDTH: usize = 3;
    pub const INERTIA: usize = 4;
    pub const MIX: usize = 5;
    pub const OUTPUT: usize = 6;
    pub const COUNT: usize = 7;
}

const PARAMS: [ParamInfo; p::COUNT] = [
    choice("speed", "Speed", &SPEEDS, 1.0),
    param("drive", "Drive", 0.0, 100.0, 15.0, "%"),
    param("balance", "Horn and drum", -100.0, 100.0, 0.0, "%"),
    param("width", "Width", 0.0, 100.0, 70.0, "%"),
    param("inertia", "Inertia", 25.0, 400.0, 100.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

/// Where the horn and the drum have turned (0 to 1), then their speeds (Hz).
pub mod meter {
    pub const HORN: usize = 0;
    pub const DRUM: usize = 1;
    pub const HORN_SPEED: usize = 2;
    pub const DRUM_SPEED: usize = 3;
    pub const COUNT: usize = 4;
}

static METERS: [&str; meter::COUNT] = [""; meter::COUNT];

#[derive(Debug)]
pub struct RotaryEffect {
    values: [f32; p::COUNT],
    rotary: Rotary,
    dry: [Vec<f32>; 2],
    /// Mix and output gain, gliding toward their settings.
    mix: f32,
    gain: f32,
    glide: f32,
}

impl Default for RotaryEffect {
    fn default() -> Self {
        let mut effect = Self {
            values: PARAMS.map(|p| p.default),
            rotary: Rotary::default(),
            dry: Default::default(),
            mix: 1.0,
            gain: 1.0,
            glide: 0.0,
        };
        effect.prepare(48_000, 1024);
        effect
    }
}

impl RotaryEffect {
    fn speed(&self) -> usize {
        match self.values[p::SPEED].round() as i32 {
            0 => STOP,
            2 => FAST,
            _ => SLOW,
        }
    }
}

impl Effect for RotaryEffect {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.rotary.prepare(sample_rate);
        self.dry = [vec![0.0; max_frames.max(64)], vec![0.0; max_frames.max(64)]];
        self.glide = 1.0 - (-1.0 / (0.02 * sample_rate as f32)).exp();
        self.mix = self.values[p::MIX] / 100.0;
        self.gain = crate::db_to_gain(self.values[p::OUTPUT]);
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.rotary.set_inertia(self.values[p::INERTIA] / 100.0);
        self.rotary.set_balance(self.values[p::BALANCE] / 100.0);
        let drive = self.values[p::DRIVE] / 100.0;
        let push = crate::db_to_gain(drive * 20.0);
        let makeup = 1.0 / push.powf(0.75);
        let bias = 0.15 * drive;
        let offset = bias.tanh();
        let (mix, gain) = (
            self.values[p::MIX] / 100.0,
            crate::db_to_gain(self.values[p::OUTPUT]),
        );
        let speed = self.speed();
        let width = self.values[p::WIDTH] / 100.0;
        let size = self.dry[0].len();
        let mut at = 0;
        while at < left.len() {
            let end = (at + size).min(left.len());
            let (l, r) = (&mut left[at..end], &mut right[at..end]);
            let n = l.len();
            self.dry[0][..n].copy_from_slice(l);
            self.dry[1][..n].copy_from_slice(r);
            // The tube amplifier, a little uneven, before the rotors.
            if drive > 1e-3 {
                for x in l.iter_mut().chain(r.iter_mut()) {
                    *x = ((*x * push + bias).tanh() - offset) * makeup;
                }
            }
            self.rotary.process(l, r, speed, width);
            for i in 0..n {
                self.mix += self.glide * (mix - self.mix);
                self.gain += self.glide * (gain - self.gain);
                l[i] = (self.dry[0][i] + self.mix * (l[i] - self.dry[0][i])) * self.gain;
                r[i] = (self.dry[1][i] + self.mix * (r[i] - self.dry[1][i])) * self.gain;
            }
            at = end;
        }
    }

    fn reset(&mut self) {
        self.rotary.reset();
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip(self.rotary.motion()) {
            *o = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn run(effect: &mut RotaryEffect, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * RATE as f32) as usize;
        let (mut left, mut right) = (Vec::new(), Vec::new());
        let mut k = 0usize;
        while left.len() < n {
            let mut l: Vec<f32> = (0..512)
                .map(|i| {
                    (std::f32::consts::TAU * 1000.0 * (k + i) as f32 / RATE as f32).sin() * 0.3
                })
                .collect();
            let mut r = l.clone();
            effect.process(&mut l, &mut r);
            left.extend_from_slice(&l);
            right.extend_from_slice(&r);
            k += 512;
        }
        (left, right)
    }

    #[test]
    fn fast_turns_the_horn_and_the_speed_shows_in_the_meters() {
        let mut e = RotaryEffect::default();
        e.prepare(RATE, 512);
        e.set_param(p::SPEED, 2.0);
        let (l, r) = run(&mut e, 2.0);
        assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 1.0));
        let mut m = [0.0; meter::COUNT];
        e.meters(&mut m);
        assert!(m[meter::HORN_SPEED] > 5.0, "{}", m[meter::HORN_SPEED]);
        // The two microphones hear the horn differently: the sides differ.
        let side: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
        assert!(side > 1.0, "{side}");
    }

    #[test]
    fn a_dry_mix_leaves_the_sound_as_it_was() {
        let mut e = RotaryEffect::default();
        e.prepare(RATE, 512);
        e.set_param(p::MIX, 0.0);
        e.set_param(p::DRIVE, 0.0);
        e.prepare(RATE, 512);
        let (l, _) = run(&mut e, 0.1);
        let k = 100;
        let want = (std::f32::consts::TAU * 1000.0 * k as f32 / RATE as f32).sin() * 0.3;
        assert!((l[k] - want).abs() < 1e-5);
    }

    #[test]
    fn more_inertia_takes_longer_to_reach_fast() {
        let reach = |inertia: f32| {
            let mut e = RotaryEffect::default();
            e.prepare(RATE, 512);
            e.set_param(p::INERTIA, inertia);
            e.set_param(p::SPEED, 2.0);
            run(&mut e, 0.5);
            let mut m = [0.0; meter::COUNT];
            e.meters(&mut m);
            m[meter::HORN_SPEED]
        };
        assert!(reach(50.0) > reach(300.0) + 1.0);
    }
}
