//! Flanger: a very short delay swept by a triangle LFO, with positive or negative feedback for
//! the jet sound, and the right channel's LFO offset for stereo motion; the rate free or synced
//! to the tempo. Through zero, as with two tape machines: the dry sound is delayed too, and
//! the sweep passes it, cancelling it for a moment (this delays the output by 5 ms).

use crate::delay::{NOTE_NAMES, synced_seconds};
use crate::line::Line;
use crate::{Effect, EffectInfo, ParamInfo, choice, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "flanger",
    name: "Strada-Flanger",
    params: &PARAMS,
    meters: &[],
    instrument: false,
    files: &[],
    category: "Modulation",
    description: "Classic jet flanger with positive or negative feedback and stereo sweep.",
};

const PARAMS: [ParamInfo; COUNT] = [
    param("rate", "Rate", 0.02, 5.0, 0.25, "Hz"),
    param("depth", "Depth", 0.0, 100.0, 70.0, "%"),
    param("delay", "Manual", 0.1, 8.0, 1.0, "ms"),
    param("feedback", "Feedback", -95.0, 95.0, 60.0, "%"),
    param("stereo", "Stereo", 0.0, 180.0, 90.0, "°"),
    param("mix", "Mix", 0.0, 100.0, 50.0, "%"),
    param("output", "Output", -12.0, 6.0, 0.0, "dB"),
    switch("sync", "Sync to tempo", 0.0),
    choice("note", "Note", &NOTE_NAMES, 12.0),
    switch("through_zero", "Through zero", 0.0),
];
const COUNT: usize = 10;
/// Delay of the dry sound through zero, in ms; the sweep goes either side of it.
const ZERO_MS: f32 = 5.0;

/// Delay the flanger adds, in samples, for parameter `values` (in [`INFO`] order).
pub fn latency_of(values: &[f32], rate: f32) -> u32 {
    if values.get(9).is_some_and(|&v| v >= 0.5) {
        (ZERO_MS * 0.001 * rate).round() as u32
    } else {
        0
    }
}

const MAX_MS: f32 = 20.0;

#[derive(Debug)]
pub struct Flanger {
    rate: f32,
    values: [f32; COUNT],
    bpm: f64,
    lines: [Line; 2],
    /// The dry sound, delayed through zero.
    dry: [Line; 2],
    phase: f32,
}

impl Default for Flanger {
    fn default() -> Self {
        let mut f = Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            bpm: 120.0,
            lines: Default::default(),
            dry: Default::default(),
            phase: 0.0,
        };
        f.allocate();
        f
    }
}

/// Triangle wave from 0 to 1 for a phase in cycles.
#[inline]
pub(crate) fn triangle(phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    if p < 0.5 { p * 2.0 } else { 2.0 - p * 2.0 }
}

impl Flanger {
    fn allocate(&mut self) {
        let frames = (MAX_MS * 0.001 * self.rate) as usize + 4;
        self.lines = [Line::new(frames), Line::new(frames)];
        let zero = (ZERO_MS * 0.001 * self.rate) as usize + 4;
        self.dry = [Line::new(zero), Line::new(zero)];
    }
}

impl Effect for Flanger {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.allocate();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
    }

    fn latency(&self) -> u32 {
        latency_of(&self.values, self.rate)
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let [
            rate,
            depth,
            manual,
            feedback,
            stereo,
            mix,
            output,
            sync,
            note,
            through_zero,
        ] = self.values;
        let ms = 0.001 * self.rate;
        let zero = through_zero >= 0.5;
        // Through zero the sweep goes either side of the dry sound's delay.
        let (base, sweep) = if zero {
            let reach = (depth / 100.0) * (ZERO_MS - 0.2) * ms;
            (ZERO_MS * ms - reach, reach * 2.0)
        } else {
            (
                manual * ms,
                (depth / 100.0) * (MAX_MS * 0.6 - manual).max(0.0) * ms * 0.5,
            )
        };
        let zero_at = ZERO_MS * ms;
        let hz = if sync >= 0.5 {
            1.0 / synced_seconds(note, self.bpm).max(0.01)
        } else {
            rate
        };
        let step = hz / self.rate;
        let offset = stereo / 360.0;
        let feedback = feedback / 100.0;
        let (wet, dry) = (mix / 100.0, 1.0 - mix / 100.0);
        let gain = crate::db_to_gain(output);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut input = [*l, *r];
            let mut out = [0.0; 2];
            for ch in 0..2 {
                let phase = self.phase + if ch == 1 { offset } else { 0.0 };
                let echo = self.lines[ch].read((base + sweep * triangle(phase)).max(1.0));
                self.lines[ch].push((input[ch] + echo * feedback).clamp(-4.0, 4.0));
                if zero {
                    self.dry[ch].push(input[ch]);
                    input[ch] = self.dry[ch].read(zero_at);
                }
                // Normalized so heavy feedback does not blow the level up.
                let comb = (input[ch] + echo) / (1.0 + feedback.abs() * 0.5);
                out[ch] = (input[ch] * dry + comb * wet) * gain;
            }
            *l = out[0];
            *r = out[1];
            self.phase = (self.phase + step).fract();
        }
    }

    fn reset(&mut self) {
        for line in self.lines.iter_mut().chain(&mut self.dry) {
            line.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_notches_and_stays_bounded() {
        for feedback in [-95.0, 0.0, 95.0] {
            let mut f = Flanger::default();
            f.set_param(3, feedback);
            f.set_param(0, 2.0);
            f.prepare(48_000, 512);
            let mut seed = 1u32;
            let mut noise = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            };
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
                .map(|_| {
                    let n = noise();
                    (n, n)
                })
                .unzip();
            f.process(&mut l, &mut r);
            assert!(
                l.iter().chain(&r).all(|s| s.is_finite() && s.abs() < 3.0),
                "{feedback}"
            );
            let diff: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
            assert!(diff > 10.0, "stereo sweep");
        }
        assert_eq!(triangle(0.25), 0.5);
        assert_eq!(triangle(0.5), 1.0);
    }

    #[test]
    fn through_zero_sweeps_across_the_dry_sound() {
        let mut f = Flanger::default();
        f.set_param(9, 1.0);
        f.set_param(3, 0.0);
        f.set_param(5, 50.0);
        f.set_param(1, 100.0);
        f.set_param(0, 1.0);
        f.prepare(48_000, 4800);
        assert_eq!(f.latency(), 240);
        let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..48_000)
            .map(|i| {
                let s = 0.5 * (i as f32 * 0.21).sin() * (i as f32 * 0.013).cos();
                (s, s)
            })
            .unzip();
        f.process(&mut l, &mut r);
        // The level dips and rises as the sweep passes the dry sound.
        let windows: Vec<f32> = l[4800..]
            .chunks(480)
            .map(|c| c.iter().map(|s| s * s).sum::<f32>())
            .collect();
        let (lo, hi) = windows
            .iter()
            .fold((f32::MAX, 0.0f32), |(a, b), &w| (a.min(w), b.max(w)));
        assert!(hi > lo * 1.5, "the sweep moves the level: {lo} {hi}");
        assert!(l.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    }
}
