//! Wavetables: a row of single-cycle frames the oscillator glides through. Each frame is kept in
//! band-limited copies, one per octave (the harmonics a note can play without folding back),
//! so high notes stay clean. The factory tables are computed from their spectra at start, once
//! for every synth; a WAV file can be loaded as a table (2048 samples per frame).

use std::sync::{Arc, OnceLock};

/// Samples of one frame.
pub const SIZE: usize = 2048;
/// Frames of a factory table; a position between two is a crossfade.
pub const FRAMES: usize = 16;
/// Band-limited copies: copy `m` keeps the harmonics up to `SIZE / 2 >> m`.
const MIPS: usize = 9;

pub const TABLES: [&str; 9] = [
    "Basic shapes",
    "Harmonics",
    "Pulse width",
    "Vocal",
    "Metallic",
    "Sync",
    "Fold",
    "Organ",
    "Loaded file",
];
/// The table loaded from a file.
pub const LOADED: usize = TABLES.len() - 1;

/// A wavetable ready to play: `frames` of [`SIZE`] samples in each of the [`MIPS`] copies.
#[derive(Debug)]
pub struct Table {
    frames: usize,
    /// `mips[m][frame * SIZE + i]`.
    mips: Vec<Vec<f32>>,
}

impl Table {
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// A sample at `phase` (0 to 1) and `position` (0 to 1 across the frames), for a note whose
    /// phase step is `dt`: the copy whose top harmonic stays under Nyquist.
    #[inline]
    pub fn read(&self, phase: f32, position: f32, dt: f32) -> f32 {
        // Highest harmonic the note can play: half a cycle per sample.
        let reach = (0.5 / dt.max(1e-9)).max(1.0);
        let mip = ((SIZE as f32 / 2.0 / reach).log2().ceil().max(0.0) as usize).min(MIPS - 1);
        let table = &self.mips[mip];
        let at = position.clamp(0.0, 1.0) * (self.frames - 1) as f32;
        let (f0, mix) = (at.floor() as usize, at.fract());
        let f1 = (f0 + 1).min(self.frames - 1);
        let x = phase * SIZE as f32;
        let (i0, frac) = (x.floor() as usize % SIZE, x.fract());
        let i1 = (i0 + 1) % SIZE;
        let sample = |f: usize| {
            let row = &table[f * SIZE..(f + 1) * SIZE];
            row[i0] + (row[i1] - row[i0]) * frac
        };
        let a = sample(f0);
        if mix == 0.0 || f0 == f1 {
            a
        } else {
            a + (sample(f1) - a) * mix
        }
    }

    /// The full-band frame nearest `position`, for drawing.
    pub fn frame(&self, position: f32) -> &[f32] {
        let f = (position.clamp(0.0, 1.0) * (self.frames - 1) as f32).round() as usize;
        &self.mips[0][f * SIZE..(f + 1) * SIZE]
    }

    /// From frames given by their harmonic amplitudes and phases (index 1 is the fundamental).
    fn from_spectra(spectra: &[Vec<(f32, f32)>]) -> Self {
        let frames = spectra.len();
        let mut mips = vec![vec![0.0f32; frames * SIZE]; MIPS];
        for (f, spectrum) in spectra.iter().enumerate() {
            for (m, mip) in mips.iter_mut().enumerate() {
                let top = (SIZE / 2) >> m;
                let mut re = vec![0.0f64; SIZE];
                let mut im = vec![0.0f64; SIZE];
                for (h, &(amp, phase)) in spectrum.iter().enumerate().take(top + 1).skip(1) {
                    // A sine of amplitude `amp`: half in bin h, half mirrored.
                    let (s, c) = f64::from(phase).sin_cos();
                    let a = f64::from(amp) * 0.5;
                    re[h] += a * s;
                    im[h] -= a * c;
                    re[SIZE - h] += a * s;
                    im[SIZE - h] += a * c;
                }
                fft(&mut re, &mut im, true);
                for (i, v) in re.iter().enumerate() {
                    mip[f * SIZE + i] = *v as f32;
                }
            }
        }
        let mut table = Self { frames, mips };
        table.normalize();
        table
    }

    /// From frames of samples: their spectra are read, then band-limited per copy.
    pub fn from_samples(samples: &[f32]) -> Result<Self, String> {
        let frames = (samples.len() / SIZE).clamp(0, 256);
        if frames == 0 {
            return Err(format!(
                "a wavetable needs at least one frame of {SIZE} samples"
            ));
        }
        let spectra: Vec<Vec<(f32, f32)>> = (0..frames)
            .map(|f| spectrum_of(&samples[f * SIZE..(f + 1) * SIZE]))
            .collect();
        Ok(Self::from_spectra(&spectra))
    }

    /// The same peak for every table, frame by frame, so switching keeps the level.
    fn normalize(&mut self) {
        for f in 0..self.frames {
            let peak = self.mips[0][f * SIZE..(f + 1) * SIZE]
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
                .max(1e-6);
            for mip in &mut self.mips {
                for s in &mut mip[f * SIZE..(f + 1) * SIZE] {
                    *s /= peak;
                }
            }
        }
    }
}

/// Harmonic amplitudes and phases of one cycle (index 0 unused: no DC).
fn spectrum_of(cycle: &[f32]) -> Vec<(f32, f32)> {
    let mut re: Vec<f64> = cycle.iter().map(|&s| f64::from(s)).collect();
    let mut im = vec![0.0f64; SIZE];
    fft(&mut re, &mut im, false);
    (0..=SIZE / 2)
        .map(|h| {
            if h == 0 {
                return (0.0, 0.0);
            }
            let (r, i) = (re[h] / (SIZE as f64 / 2.0), im[h] / (SIZE as f64 / 2.0));
            // Back to the sine phase used by `from_spectra`.
            ((r * r + i * i).sqrt() as f32, (r.atan2(-i)) as f32)
        })
        .collect()
}

/// In-place radix-2 FFT; the inverse is not scaled (a sum of the bins).
fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let angle = sign * std::f64::consts::TAU / len as f64;
        let (ws, wc) = angle.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                (cr, ci) = (cr * wc - ci * ws, cr * ws + ci * wc);
            }
        }
        len <<= 1;
    }
}

/// The factory tables, computed once and shared.
pub fn factory() -> &'static [Arc<Table>] {
    static TABLES: OnceLock<Vec<Arc<Table>>> = OnceLock::new();
    TABLES.get_or_init(|| (0..LOADED).map(|k| Arc::new(build(k))).collect())
}

fn build(kind: usize) -> Table {
    let top = SIZE / 2;
    let spectra: Vec<Vec<(f32, f32)>> = (0..FRAMES)
        .map(|f| {
            let t = f as f32 / (FRAMES - 1) as f32;
            match kind {
                // Sine to triangle to saw to square, each step a crossfade of spectra.
                0 => {
                    let shapes = [sine(), triangle(), saw(), square()];
                    let at = t * 3.0;
                    let (k, mix) = ((at.floor() as usize).min(2), at - at.floor().min(2.0));
                    blend(&shapes[k], &shapes[k + 1], mix)
                }
                // A saw built up harmonic by harmonic.
                1 => {
                    let count = 1.0 + t * 63.0;
                    (0..=top)
                        .map(|h| {
                            let h = h as f32;
                            let keep = (count - h + 1.0).clamp(0.0, 1.0);
                            (if h > 0.0 { keep / h } else { 0.0 }, 0.0)
                        })
                        .collect()
                }
                // A pulse from square to a thin needle.
                2 => {
                    let duty = 0.5 - t * 0.45;
                    (0..=top)
                        .map(|h| {
                            let h = h as f32;
                            if h == 0.0 {
                                (0.0, 0.0)
                            } else {
                                let a = 2.0 / (h * std::f32::consts::PI)
                                    * (h * std::f32::consts::PI * duty).sin();
                                (a.abs(), if a < 0.0 { std::f32::consts::PI } else { 0.0 })
                            }
                        })
                        .collect()
                }
                // A saw through the formants of the vowels a, e, i, o, u, on a 110 Hz voice.
                3 => {
                    const VOWELS: [[f32; 3]; 5] = [
                        [800.0, 1150.0, 2900.0],
                        [400.0, 1600.0, 2700.0],
                        [350.0, 1700.0, 2700.0],
                        [450.0, 800.0, 2830.0],
                        [325.0, 700.0, 2530.0],
                    ];
                    let at = t * 4.0;
                    let (k, mix) = ((at.floor() as usize).min(3), at - at.floor().min(3.0));
                    let formants: Vec<f32> = (0..3)
                        .map(|i| VOWELS[k][i] + (VOWELS[k + 1][i] - VOWELS[k][i]) * mix)
                        .collect();
                    (0..=top)
                        .map(|h| {
                            if h == 0 {
                                return (0.0, 0.0);
                            }
                            let hz = h as f32 * 110.0;
                            let peak: f32 = formants
                                .iter()
                                .enumerate()
                                .map(|(i, f)| {
                                    let width = 80.0 + 60.0 * i as f32;
                                    (-((hz - f) / width).powi(2)).exp() / (1.0 + i as f32)
                                })
                                .sum();
                            ((0.15 + peak) / h as f32, 0.0)
                        })
                        .collect()
                }
                // Clusters of upper partials that move up the spectrum: bell-like, then harsh.
                4 => (0..=top)
                    .map(|h| {
                        if h == 0 {
                            return (0.0, 0.0);
                        }
                        let center = 3.0 + t * 40.0;
                        let near = (-((h as f32 - center) / 2.5).powi(2)).exp();
                        let far = (-((h as f32 - center * 2.3) / 4.0).powi(2)).exp() * 0.6;
                        let base = if h == 1 { 0.6 } else { 0.0 };
                        (base + near + far, (h * h) as f32 * 0.7)
                    })
                    .collect(),
                // Hard sync: a saw restarted by the cycle, its own pitch rising to five times.
                5 => {
                    let ratio = 1.0 + t * 4.0;
                    let cycle: Vec<f32> = (0..SIZE)
                        .map(|i| {
                            let phase = (i as f32 / SIZE as f32 * ratio).fract();
                            2.0 * phase - 1.0
                        })
                        .collect();
                    spectrum_of(&cycle)
                }
                // A sine folded back on itself harder and harder.
                6 => {
                    let gain = 1.0 + t * 6.0;
                    let cycle: Vec<f32> = (0..SIZE)
                        .map(|i| {
                            let s = (i as f32 / SIZE as f32 * std::f32::consts::TAU).sin() * gain;
                            (s * std::f32::consts::FRAC_PI_2).sin()
                        })
                        .collect();
                    spectrum_of(&cycle)
                }
                // Drawbars pulled out one after the other: 16', 8', 5 1/3', 4', 2 2/3', 2'...
                _ => {
                    const DRAWBARS: [usize; 9] = [1, 2, 3, 4, 6, 8, 10, 12, 16];
                    let open = 1.0 + t * (DRAWBARS.len() - 1) as f32;
                    (0..=top)
                        .map(|h| {
                            let level = DRAWBARS
                                .iter()
                                .position(|&d| d == h)
                                .map_or(0.0, |i| (open - i as f32).clamp(0.0, 1.0));
                            (level * if h == 1 { 1.0 } else { 0.8 }, 0.0)
                        })
                        .collect()
                }
            }
        })
        .collect();
    Table::from_spectra(&spectra)
}

fn sine() -> Vec<(f32, f32)> {
    let mut s = vec![(0.0, 0.0); SIZE / 2 + 1];
    s[1] = (1.0, 0.0);
    s
}

fn saw() -> Vec<(f32, f32)> {
    (0..=SIZE / 2)
        .map(|h| {
            if h == 0 {
                (0.0, 0.0)
            } else {
                (
                    2.0 / (std::f32::consts::PI * h as f32),
                    std::f32::consts::PI,
                )
            }
        })
        .collect()
}

fn square() -> Vec<(f32, f32)> {
    (0..=SIZE / 2)
        .map(|h| {
            if h % 2 == 1 {
                (4.0 / (std::f32::consts::PI * h as f32), 0.0)
            } else {
                (0.0, 0.0)
            }
        })
        .collect()
}

fn triangle() -> Vec<(f32, f32)> {
    (0..=SIZE / 2)
        .map(|h| {
            if h % 2 == 1 {
                let sign = if (h / 2) % 2 == 0 {
                    0.0
                } else {
                    std::f32::consts::PI
                };
                (8.0 / (std::f32::consts::PI * h as f32).powi(2), sign)
            } else {
                (0.0, 0.0)
            }
        })
        .collect()
}

/// A crossfade of two spectra, harmonic by harmonic (as complex values, so phases blend).
fn blend(a: &[(f32, f32)], b: &[(f32, f32)], mix: f32) -> Vec<(f32, f32)> {
    a.iter()
        .zip(b)
        .map(|(&(aa, ap), &(ba, bp))| {
            let (x, y) = (
                aa * ap.cos() * (1.0 - mix) + ba * bp.cos() * mix,
                aa * ap.sin() * (1.0 - mix) + ba * bp.sin() * mix,
            );
            ((x * x + y * y).sqrt(), y.atan2(x))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fft_goes_there_and_back() {
        let cycle: Vec<f32> = (0..SIZE)
            .map(|i| {
                let t = i as f32 / SIZE as f32 * std::f32::consts::TAU;
                0.7 * t.sin() + 0.2 * (3.0 * t + 0.5).sin()
            })
            .collect();
        let spectrum = spectrum_of(&cycle);
        assert!((spectrum[1].0 - 0.7).abs() < 1e-3 && (spectrum[3].0 - 0.2).abs() < 1e-3);
        let table = Table::from_spectra(&[spectrum]);
        let peak = cycle.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        for (a, b) in table.frame(0.0).iter().zip(&cycle) {
            assert!((a - b / peak).abs() < 1e-3);
        }
    }

    #[test]
    fn high_notes_read_a_copy_without_harmonics_past_nyquist() {
        let tables = factory();
        assert_eq!(tables.len(), LOADED);
        // The saw corner of the basic shapes, at 5 kHz on 48 kHz: about four harmonics fit.
        let saw = &tables[0];
        let dt = 5000.0 / 48_000.0;
        let cycle: Vec<f32> = (0..SIZE)
            .map(|i| saw.read(i as f32 / SIZE as f32, 0.67, dt))
            .collect();
        let spectrum = spectrum_of(&cycle);
        let above: f32 = spectrum[5..].iter().map(|(a, _)| a * a).sum();
        assert!(above < 1e-6, "{above}");
        assert!(spectrum[1].0 > 0.3);
        // Every table is finite and at full level.
        for table in tables {
            for p in [0.0, 0.5, 1.0] {
                let peak = table.frame(p).iter().fold(0.0f32, |m, s| m.max(s.abs()));
                assert!((peak - 1.0).abs() < 1e-3, "{peak}");
            }
        }
    }

    #[test]
    fn a_file_becomes_a_table() {
        let samples: Vec<f32> = (0..SIZE * 3)
            .map(|i| ((i % SIZE) as f32 / SIZE as f32 * std::f32::consts::TAU).sin())
            .collect();
        let table = Table::from_samples(&samples).unwrap();
        assert_eq!(table.frames(), 3);
        assert!(Table::from_samples(&[0.0; 100]).is_err());
    }
}
