//! Strada-Doubler: two or four copies of the sound, each a little late and a few cents off,
//! spread across the sides and drifting slowly as a second take would: the double-tracked
//! voice or guitar, from tight micro-shift widening to loose artificial double tracking.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::line::Line;
use crate::shift::{Shifter, ratio};
use crate::{Effect, EffectInfo, ParamInfo, choice, db_to_gain, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "doubler",
    name: "Strada-Doubler",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Stereo",
    description: "Double tracking: detuned, delayed and drifting copies spread across the \
                  sides, for wide vocals and guitars.",
};

pub const VOICES: [&str; 2] = ["2", "4"];

pub mod p {
    pub const VOICES: usize = 0;
    pub const DETUNE: usize = 1;
    pub const DELAY: usize = 2;
    pub const WIDTH: usize = 3;
    pub const DRIFT: usize = 4;
    pub const LOW_CUT: usize = 5;
    pub const MIX: usize = 6;
    pub const OUTPUT: usize = 7;
}
const COUNT: usize = 8;

const PARAMS: [ParamInfo; COUNT] = [
    choice("voices", "Voices", &VOICES, 0.0),
    param("detune", "Detune", 0.0, 50.0, 8.0, "ct"),
    param("delay", "Delay", 0.0, 60.0, 10.0, "ms"),
    param("width", "Width", 0.0, 100.0, 80.0, "%"),
    param("drift", "Drift", 0.0, 100.0, 30.0, "%"),
    param("low_cut", "Low cut", 20.0, 1_000.0, 150.0, "Hz"),
    param("mix", "Doubles", 0.0, 100.0, 50.0, "%"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

/// Each voice: its share of the detune, of the delay, and its side.
pub const SHAPES: [(f32, f32, f32); 4] = [
    (1.0, 1.0, -1.0),
    (-1.0, 0.72, 1.0),
    (0.55, 1.6, -0.5),
    (-0.55, 1.3, 0.5),
];
const WINDOW_MS: f32 = 22.0;
const MAX_DELAY_MS: f32 = 60.0;
/// Most the drift moves a voice: in time (ms) and in pitch (cents).
const DRIFT_MS: f32 = 3.0;
const DRIFT_CENTS: f32 = 6.0;

#[derive(Debug, Default)]
struct Voice {
    shifter: Shifter,
    line: Line,
    filter: Biquad,
    /// The slow random wander: where it heads, where it is, and when it turns.
    target: f32,
    wander: f32,
    countdown: usize,
}

#[derive(Debug)]
pub struct Doubler {
    rate: f32,
    values: [f32; COUNT],
    voices: [Voice; 4],
    low_cut: Coefficients,
    noise: u32,
}

impl Default for Doubler {
    fn default() -> Self {
        let mut d = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            voices: Default::default(),
            low_cut: Coefficients::IDENTITY,
            noise: 0x1357_9bdf,
        };
        d.allocate();
        d
    }
}

impl Doubler {
    fn allocate(&mut self) {
        let window = (WINDOW_MS * 0.001 * self.rate) as usize;
        let delay = ((MAX_DELAY_MS + DRIFT_MS * 2.0) * 0.001 * self.rate) as usize + 8;
        for voice in &mut self.voices {
            voice.shifter = Shifter::new(window);
            voice.line = Line::new(delay);
        }
        self.update();
    }

    fn update(&mut self) {
        let cut = f64::from(self.values[p::LOW_CUT]);
        self.low_cut =
            Coefficients::design(FilterKind::HighPass, cut, 0.707, 0.0, f64::from(self.rate));
    }

    #[inline]
    fn random(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

impl Effect for Doubler {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            if index == p::LOW_CUT {
                self.update();
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let v = self.values;
        let count = if v[p::VOICES] >= 0.5 { 4 } else { 2 };
        let detune = v[p::DETUNE];
        let delay = v[p::DELAY] * 0.001 * self.rate;
        let width = v[p::WIDTH] / 100.0;
        let drift = v[p::DRIFT] / 100.0;
        let level = v[p::MIX] / 100.0 / (count as f32 / 2.0).sqrt();
        let output = db_to_gain(v[p::OUTPUT]);
        let window = WINDOW_MS * 0.001 * self.rate;
        let turn = (0.35 * self.rate) as usize;
        let glide = 1.0 - (-1.0 / (0.25 * self.rate)).exp();
        let low_cut = self.low_cut;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mono = 0.5 * (*l + *r);
            let mut doubles = [0.0f32; 2];
            for (k, &(cents, late, side)) in SHAPES.iter().enumerate().take(count) {
                if self.voices[k].countdown == 0 {
                    let target = self.random();
                    let voice = &mut self.voices[k];
                    voice.target = target;
                    voice.countdown = turn + k * 997;
                }
                let voice = &mut self.voices[k];
                voice.countdown -= 1;
                voice.wander += glide * (voice.target - voice.wander);
                let wander = voice.wander * drift;
                voice.line.push(mono);
                let at = 1.0
                    + delay * late
                    + (1.0 + wander) * DRIFT_MS * 0.001 * self.rate * drift.max(0.05);
                let late_sound = voice.line.read(at);
                let shift = ratio((cents * detune + wander * DRIFT_CENTS) / 100.0);
                let shifted = voice.shifter.process(late_sound, shift, window);
                let y = voice.filter.process(&low_cut, f64::from(shifted)) as f32 * level;
                let pan = side * width;
                doubles[0] += y * (1.0 - pan).min(1.0);
                doubles[1] += y * (1.0 + pan).min(1.0);
            }
            *l = (*l + doubles[0]) * output;
            *r = (*r + doubles[1]) * output;
        }
    }

    fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.shifter.clear();
            voice.line.clear();
            voice.filter = Biquad::default();
            voice.wander = 0.0;
            voice.countdown = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn run(settings: &[(&str, f32)]) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut d = Doubler::default();
        d.prepare(48_000, 512);
        for (id, v) in settings {
            d.set_param(index(id), *v);
        }
        let input: Vec<f32> = (0..48_000)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                0.3 * (std::f32::consts::TAU * 330.0 * t).sin()
                    * (1.0 + (std::f32::consts::TAU * 3.0 * t).sin())
                    / 2.0
            })
            .collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            d.process(a, b);
        }
        (input, l, r)
    }

    #[test]
    fn the_doubles_widen_a_mono_sound() {
        let (_, l, r) = run(&[("width", 100.0), ("mix", 100.0)]);
        let difference: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).powi(2)).sum();
        let total: f32 = l.iter().map(|a| a * a).sum();
        assert!(difference > total * 0.1, "{difference} {total}");
        let (_, l, r) = run(&[("width", 0.0), ("mix", 100.0)]);
        assert!(l.iter().zip(&r).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn without_doubles_the_sound_is_untouched() {
        let (input, l, _) = run(&[("mix", 0.0), ("voices", 1.0)]);
        assert!(input.iter().zip(&l).all(|(a, b)| (a - b).abs() < 1e-6));
        let (_, l, _) = run(&[
            ("mix", 100.0),
            ("voices", 1.0),
            ("drift", 100.0),
            ("detune", 50.0),
        ]);
        assert!(l.iter().all(|v| v.abs() < 2.0));
    }
}
