//! Chorus: short delays swept by sine LFOs, one to three voices per side (three make an
//! ensemble), the sides apart by the stereo phase, a tone filter on the voices, the rate free or
//! synced to the tempo; with feedback it turns into a flanger.

use crate::delay::{NOTE_NAMES, synced_seconds};
use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, choice, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "chorus",
    name: "Strada-Chorus",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Short modulated delays for width and shimmer; turns into a flanger with feedback.",
};

pub const VOICES: [&str; 3] = ["1 voice", "2 voices", "Ensemble"];

const PARAMS: [ParamInfo; COUNT] = [
    param("rate", "Rate", 0.05, 8.0, 0.8, "Hz"),
    param("depth", "Depth", 0.0, 100.0, 40.0, "%"),
    param("delay", "Delay", 1.0, 30.0, 12.0, "ms"),
    param("feedback", "Feedback", 0.0, 90.0, 0.0, "%"),
    param("mix", "Mix", 0.0, 100.0, 50.0, "%"),
    switch("sync", "Sync to tempo", 0.0),
    choice("note", "Note", &NOTE_NAMES, 11.0),
    choice("voices", "Voices", &VOICES, 0.0),
    param("stereo", "Stereo", 0.0, 180.0, 90.0, "°"),
    param("tone", "Tone", 1000.0, 20_000.0, 20_000.0, "Hz"),
];
const COUNT: usize = 10;

const MAX_MS: f32 = 45.0;

#[derive(Debug)]
pub struct Chorus {
    rate: f32,
    values: [f32; COUNT],
    bpm: f64,
    lines: [Line; 2],
    phase: f32,
    /// The tone filter on each side's voices.
    tone: [f32; 2],
}

impl Default for Chorus {
    fn default() -> Self {
        let mut chorus = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            bpm: 120.0,
            lines: Default::default(),
            phase: 0.0,
            tone: [0.0; 2],
        };
        chorus.prepare(48_000, 0);
        chorus
    }
}

impl Effect for Chorus {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        let frames = (MAX_MS * 0.001 * self.rate) as usize + 4;
        self.lines = [Line::new(frames), Line::new(frames)];
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [
            rate,
            depth,
            delay,
            feedback,
            mix,
            sync,
            note,
            voices,
            stereo,
            tone,
        ] = self.values;
        let (feedback, wet) = (feedback / 100.0, mix / 100.0);
        let voices = (voices.round().max(0.0) as usize).min(2) + 1;
        let base = delay * 0.001 * self.rate;
        let room = MAX_MS * 0.001 * self.rate - 2.0;
        let sweep = (depth / 100.0) * base.min(room - base * 1.3) * 0.9;
        let hz = if sync >= 0.5 {
            1.0 / synced_seconds(note, self.bpm).max(0.01)
        } else {
            rate
        };
        let step = hz / self.rate;
        let offset = stereo / 360.0;
        let pole = crate::pole(tone, self.rate);
        let level = 1.0 / (voices as f32).sqrt();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let dry = [*l, *r];
            let mut out = [0.0; 2];
            for ch in 0..2 {
                // Voices spread over the cycle, each a little longer than the last.
                let mut echo = 0.0;
                for v in 0..voices {
                    let phase = self.phase + offset * ch as f32 + v as f32 / voices as f32;
                    let angle = phase * std::f32::consts::TAU;
                    let length = base * (1.0 + 0.15 * v as f32) + sweep * angle.sin();
                    echo += self.lines[ch].read(length.clamp(1.0, room));
                }
                let echo = echo * level;
                self.tone[ch] = echo + pole * (self.tone[ch] - echo);
                let echo = self.tone[ch];
                self.lines[ch].push(dry[ch] + echo * feedback);
                out[ch] = dry[ch] * (1.0 - wet * 0.5) + echo * wet;
            }
            *l = out[0];
            *r = out[1];
            self.phase = (self.phase + step).fract();
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.tone = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widens_a_mono_signal() {
        let mut chorus = Chorus::default();
        chorus.set_param(4, 100.0);
        chorus.prepare(48_000, 4800);
        let input: Vec<f32> = (0..48_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
            .collect();
        let (mut l, mut r) = (input.clone(), input);
        chorus.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 1.5));
        let difference: f32 = l[4800..]
            .iter()
            .zip(&r[4800..])
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(difference > 100.0, "left and right differ: {difference}");
    }
}
