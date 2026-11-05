//! Strada-Pitch: two voices shifted by up to two octaves each (semitones and cents), with
//! their level and side, through a delay whose feedback runs the shifted sound back into the
//! shifter: harmonies, octaves, and the spiralling arpeggios of the classic harmonizers.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::line::Line;
use crate::shift::{Shifter, ratio};
use crate::{Effect, EffectInfo, ParamInfo, db_to_gain, param};

pub const INFO: EffectInfo = EffectInfo {
    id: "pitch",
    name: "Strada-Pitch",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Pitch shifter with two voices, delay and feedback: harmonies, octaves, \
                  spiralling arpeggios.",
};

pub mod p {
    pub const A_SEMIS: usize = 0;
    pub const A_CENTS: usize = 1;
    pub const A_LEVEL: usize = 2;
    pub const A_PAN: usize = 3;
    pub const B_SEMIS: usize = 4;
    pub const B_CENTS: usize = 5;
    pub const B_LEVEL: usize = 6;
    pub const B_PAN: usize = 7;
    pub const WINDOW: usize = 8;
    pub const DELAY: usize = 9;
    pub const FEEDBACK: usize = 10;
    pub const TONE: usize = 11;
    pub const MIX: usize = 12;
    pub const OUTPUT: usize = 13;
}
const COUNT: usize = 14;

const PARAMS: [ParamInfo; COUNT] = [
    param("a_semis", "Voice A semitones", -24.0, 24.0, 12.0, "st"),
    param("a_cents", "Voice A cents", -50.0, 50.0, 0.0, "ct"),
    param("a_level", "Voice A level", 0.0, 100.0, 100.0, "%"),
    param("a_pan", "Voice A pan", -100.0, 100.0, 0.0, "%"),
    param("b_semis", "Voice B semitones", -24.0, 24.0, -12.0, "st"),
    param("b_cents", "Voice B cents", -50.0, 50.0, 0.0, "ct"),
    param("b_level", "Voice B level", 0.0, 100.0, 0.0, "%"),
    param("b_pan", "Voice B pan", -100.0, 100.0, 0.0, "%"),
    param("window", "Window", 10.0, 100.0, 40.0, "ms"),
    param("delay", "Delay", 0.0, 1000.0, 0.0, "ms"),
    param("feedback", "Feedback", 0.0, 95.0, 0.0, "%"),
    param("tone", "Tone", 1_000.0, 20_000.0, 20_000.0, "Hz"),
    param("mix", "Mix", 0.0, 100.0, 100.0, "%"),
    param("output", "Output", -12.0, 12.0, 0.0, "dB"),
];

const MAX_WINDOW_MS: f32 = 100.0;
const MAX_DELAY_MS: f32 = 1000.0;

/// The delay the shifted sound has with these settings, which the dry sound waits for.
pub fn latency_of(values: &[f32], rate: f32) -> u32 {
    let window = values.get(p::WINDOW).copied().unwrap_or(40.0) * 0.001 * rate;
    Shifter::latency(window).round() as u32
}

#[derive(Debug)]
pub struct Pitch {
    rate: f32,
    values: [f32; COUNT],
    /// Per voice and side.
    shifters: [[Shifter; 2]; 2],
    echo: [Line; 2],
    fed: [f32; 2],
    tone: Coefficients,
    filters: [Biquad; 2],
    dry: [Line; 2],
}

impl Default for Pitch {
    fn default() -> Self {
        let mut pitch = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            shifters: Default::default(),
            echo: Default::default(),
            fed: [0.0; 2],
            tone: Coefficients::IDENTITY,
            filters: [Biquad::default(); 2],
            dry: Default::default(),
        };
        pitch.allocate();
        pitch
    }
}

impl Pitch {
    fn allocate(&mut self) {
        let window = (MAX_WINDOW_MS * 0.001 * self.rate) as usize;
        self.shifters = std::array::from_fn(|_| [Shifter::new(window), Shifter::new(window)]);
        let echo = (MAX_DELAY_MS * 0.001 * self.rate) as usize + 4;
        self.echo = [Line::new(echo), Line::new(echo)];
        let dry = latency_of(&[0.0; COUNT].map(|_| MAX_WINDOW_MS), self.rate) as usize + 4;
        self.dry = [Line::new(dry), Line::new(dry)];
        self.update();
    }

    fn update(&mut self) {
        let tone = f64::from(self.values[p::TONE].min(self.rate * 0.45));
        self.tone =
            Coefficients::design(FilterKind::LowPass, tone, 0.707, 0.0, f64::from(self.rate));
    }
}

impl Effect for Pitch {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
        self.reset();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
            if index == p::TONE {
                self.update();
            }
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let v = self.values;
        let window = v[p::WINDOW] * 0.001 * self.rate;
        let voices = [
            (
                ratio(v[p::A_SEMIS].round() + v[p::A_CENTS] / 100.0),
                v[p::A_LEVEL] / 100.0,
                v[p::A_PAN] / 100.0,
            ),
            (
                ratio(v[p::B_SEMIS].round() + v[p::B_CENTS] / 100.0),
                v[p::B_LEVEL] / 100.0,
                v[p::B_PAN] / 100.0,
            ),
        ];
        // Each voice's gain on each side: a balance law, full in the middle.
        let sides = voices
            .map(|(_, level, pan)| [level * (1.0 - pan).min(1.0), level * (1.0 + pan).min(1.0)]);
        let delay = v[p::DELAY] * 0.001 * self.rate;
        let feedback = v[p::FEEDBACK] / 100.0;
        let wet = v[p::MIX] / 100.0;
        let output = db_to_gain(v[p::OUTPUT]);
        let aligned = latency_of(&v, self.rate) as usize + 1;
        let tone = self.tone;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = [*l, *r];
            let mut out = [0.0f32; 2];
            for ch in 0..2 {
                let x = input[ch] + feedback * self.fed[ch];
                let mut shifted = 0.0;
                for (k, (factor, level, _)) in voices.iter().enumerate() {
                    if *level > 0.0 {
                        shifted += sides[k][ch] * self.shifters[k][ch].process(x, *factor, window);
                    } else {
                        self.shifters[k][ch].process(x, *factor, window);
                    }
                }
                let shifted = self.filters[ch].process(&tone, f64::from(shifted)) as f32;
                self.echo[ch].push(shifted);
                let echoed = if delay >= 1.0 {
                    self.echo[ch].read(delay)
                } else {
                    shifted
                };
                // Kept below runaway: soft above full scale.
                self.fed[ch] = echoed.clamp(-2.0, 2.0);
                self.dry[ch].push(input[ch]);
                let dry = self.dry[ch].tap(aligned);
                out[ch] = (dry + wet * (echoed - dry)) * output;
            }
            *l = out[0];
            *r = out[1];
        }
    }

    fn latency(&self) -> u32 {
        latency_of(&self.values, self.rate)
    }

    fn reset(&mut self) {
        for side in &mut self.shifters {
            for s in side {
                s.clear();
            }
        }
        for line in self.echo.iter_mut().chain(self.dry.iter_mut()) {
            line.clear();
        }
        self.fed = [0.0; 2];
        self.filters = [Biquad::default(); 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shift::tests::dominant;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    fn run(settings: &[(&str, f32)], seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let mut pitch = Pitch::default();
        pitch.prepare(48_000, 512);
        for (id, v) in settings {
            pitch.set_param(index(id), *v);
        }
        let n = (seconds * 48_000.0) as usize;
        let mut l: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.3)
            .collect();
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            pitch.process(a, b);
        }
        (l, r)
    }

    #[test]
    fn voice_a_plays_its_interval_on_its_side() {
        let (l, r) = run(&[("a_semis", 7.0), ("a_pan", -100.0)], 1.0);
        let f = dominant(&l[24_000..]);
        assert!((f / 659.26 - 1.0).abs() < 0.006, "{f}");
        let energy = |x: &[f32]| x[24_000..].iter().map(|v| v * v).sum::<f32>();
        assert!(energy(&r) < energy(&l) * 0.01);
    }

    #[test]
    fn the_dry_sound_lines_up_with_the_latency() {
        // Mix at 0: only the dry sound, delayed by the latency the host compensates.
        let mut pitch = Pitch::default();
        pitch.prepare(48_000, 512);
        pitch.set_param(index("mix"), 0.0);
        let latency = pitch.latency() as usize;
        let mut l = vec![0.0f32; 4096];
        l[0] = 1.0;
        let mut r = l.clone();
        pitch.process(&mut l, &mut r);
        let at = l.iter().position(|v| *v > 0.5).unwrap();
        assert_eq!(at, latency);
    }

    #[test]
    fn feedback_keeps_ringing_and_stays_bounded() {
        let mut pitch = Pitch::default();
        pitch.prepare(48_000, 512);
        for (id, v) in [("a_semis", 12.0), ("delay", 120.0), ("feedback", 90.0)] {
            pitch.set_param(index(id), v);
        }
        let mut l: Vec<f32> = (0..9_600)
            .map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect();
        l.resize(96_000, 0.0);
        let mut r = l.clone();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            pitch.process(a, b);
        }
        let tail = l[48_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(tail > 0.01 && tail < 4.0, "{tail}");
    }
}
