//! Granular pitch shifter shared by Strada-Pitch and Strada-Doubler: two taps read a delay
//! line at the shifted speed, each faded by a sine-squared window half a grain apart, so their
//! sum keeps the level. When a tap starts a new grain, it starts where the sound best matches
//! what the other tap is playing (cross-correlation, as WSOLA), so the grains blend in phase
//! and the pitch stays true. Works on any sound, chords included; the window sets the
//! trade-off between smearing (long) and roughness (short).

use crate::line::Line;

/// The nearest a tap reads behind the input.
const NEAREST: f32 = 4.0;
/// How far back a new grain may start to match the other tap, and how much it compares.
const SEARCH: usize = 768;
const COMPARED: usize = 96;

#[derive(Clone, Copy, Debug, Default)]
struct Tap {
    delay: f32,
    age: f32,
}

#[derive(Debug, Default)]
pub(crate) struct Shifter {
    line: Line,
    taps: [Tap; 2],
}

impl Shifter {
    /// A shifter whose window reaches `frames`.
    pub(crate) fn new(frames: usize) -> Self {
        let mut s = Self {
            line: Line::new(frames + SEARCH + 2 * COMPARED + 16),
            taps: [Tap::default(); 2],
        };
        s.clear();
        s
    }

    /// The delay (frames) the shifted sound has on average for a window.
    pub(crate) fn latency(window: f32) -> f32 {
        NEAREST + window / 2.0 + (window / 2.0).min(SEARCH as f32) / 2.0
    }

    /// `x` in, shifted by `ratio` (2 is an octave up) over a window of `window` frames.
    #[inline]
    pub(crate) fn process(&mut self, x: f32, ratio: f32, window: f32) -> f32 {
        self.line.push(x);
        let longest = (self.line.len() - SEARCH - 2 * COMPARED - 12) as f32;
        let window = window.clamp(16.0, longest);
        let drift = 1.0 - ratio;
        let step = drift.abs() / window;
        let mut y = 0.0;
        for k in 0..2 {
            let tap = &mut self.taps[k];
            tap.delay = (tap.delay + drift).clamp(NEAREST, longest + SEARCH as f32);
            tap.age += step;
            if tap.age >= 1.0 {
                tap.age -= 1.0;
                let start = if ratio > 1.0 {
                    NEAREST + window
                } else {
                    NEAREST
                };
                let other = self.taps[1 - k].delay;
                let reach = ((window * 0.5) as usize).min(SEARCH);
                self.taps[k].delay = start + self.best_offset(start, other, reach) as f32;
            }
            let tap = self.taps[k];
            let w = (std::f32::consts::PI * tap.age).sin().powi(2);
            y += w * self.cubic(tap.delay);
        }
        y
    }

    /// The offset (frames, up to `reach`) at which the sound before `start` looks most like
    /// the sound before `other`.
    fn best_offset(&self, start: f32, other: f32, reach: usize) -> usize {
        let (start, other) = (start as usize, other as usize);
        let mut best = (0, f32::MIN);
        for offset in (0..=reach).step_by(2) {
            let mut sum = 0.0;
            for n in (0..COMPARED).step_by(2) {
                sum += self.line.tap(start + offset + n) * self.line.tap(other + n);
            }
            if sum > best.1 {
                best = (offset, sum);
            }
        }
        best.0
    }

    /// Read `delay` frames back, between samples by a cubic Hermite curve.
    #[inline]
    fn cubic(&self, delay: f32) -> f32 {
        let i = delay.floor();
        let t = delay - i;
        let i = i as usize;
        let (y0, y1, y2, y3) = (
            self.line.tap(i - 1),
            self.line.tap(i),
            self.line.tap(i + 1),
            self.line.tap(i + 2),
        );
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    pub(crate) fn clear(&mut self) {
        self.line.clear();
        self.taps = [
            Tap {
                delay: NEAREST,
                age: 0.0,
            },
            Tap {
                delay: NEAREST + 960.0,
                age: 0.5,
            },
        ];
    }
}

/// The frequency ratio of a shift in semitones.
pub(crate) fn ratio(semitones: f32) -> f32 {
    2f32.powf(semitones / 12.0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The strongest frequency of `x` at 48 kHz between 50 Hz and 4 kHz, by correlation.
    pub(crate) fn dominant(x: &[f32]) -> f32 {
        let mut best = (0.0, 0.0);
        let mut f = 50.0f32;
        while f < 4_000.0 {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in x.iter().enumerate() {
                let a = std::f32::consts::TAU * f * i as f32 / 48_000.0;
                re += v * a.cos();
                im += v * a.sin();
            }
            let m = re * re + im * im;
            if m > best.1 {
                best = (f, m);
            }
            f *= 1.002;
        }
        best.0
    }

    #[test]
    fn shifts_by_the_ratio_and_keeps_the_level() {
        for (semitones, expected) in [(12.0, 880.0), (-12.0, 220.0), (7.0, 659.26), (-5.0, 329.63)]
        {
            let mut s = Shifter::new(4800);
            let out: Vec<f32> = (0..48_000)
                .map(|i| {
                    let x = (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.5;
                    s.process(x, ratio(semitones), 1920.0)
                })
                .collect();
            let f = dominant(&out[24_000..]);
            assert!((f / expected - 1.0).abs() < 0.006, "{semitones}: {f}");
            let rms = (out[24_000..].iter().map(|v| v * v).sum::<f32>() / 24_000.0).sqrt();
            assert!(
                (rms / (0.5 / 2f32.sqrt()) - 1.0).abs() < 0.2,
                "{semitones}: {rms}"
            );
        }
    }
}
