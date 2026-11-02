//! Offline time stretching and pitch shifting: a phase vocoder with identity phase locking
//! (Laroche and Dolson) and phase resets on transients, so attacks stay sharp.

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

use crate::resample::Resampler;

const SIZE: usize = 4096;
const SYNTH_HOP: usize = SIZE / 4;

fn wrap(phase: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    phase - tau * (phase / tau).round()
}

/// Stretches `input` to `ratio` times its length, keeping its pitch.
pub fn stretch(input: &[f32], ratio: f64) -> Vec<f32> {
    let ratio = ratio.clamp(0.1, 10.0);
    if (ratio - 1.0).abs() < 1e-6 || input.is_empty() {
        return input.to_vec();
    }
    let bins = SIZE / 2 + 1;
    let window: Vec<f32> = (0..SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
        .collect();
    let mut planner = RealFftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(SIZE);
    let inverse = planner.plan_fft_inverse(SIZE);
    let mut frame = vec![0.0f32; SIZE];
    let mut spectrum = forward.make_output_vec();
    let out_len = (input.len() as f64 * ratio).round() as usize;
    let frames = out_len / SYNTH_HOP + 2;
    // Pad so that frames read zeros past both ends.
    let pad = SIZE;
    let mut padded = vec![0.0f32; pad + input.len() + 2 * SIZE];
    padded[pad..pad + input.len()].copy_from_slice(input);
    let mut output = vec![0.0f32; out_len + 3 * SIZE];
    let mut weight = vec![0.0f32; out_len + 3 * SIZE];
    let mut last_phase = vec![0.0f32; bins];
    let mut synth_phase = vec![0.0f32; bins];
    let mut last_magnitude = vec![0.0f32; bins];
    let mut magnitude = vec![0.0f32; bins];
    let mut phase = vec![0.0f32; bins];
    let mut peaks: Vec<usize> = Vec::with_capacity(bins);
    let mut previous_position: Option<usize> = None;
    let mut flux_history: Vec<f32> = Vec::new();

    for m in 0..frames {
        // Analysis position in the padded input (centered windows).
        let center = (m * SYNTH_HOP) as f64 / ratio;
        let position = (pad as f64 + center - (SIZE / 2) as f64).round().max(0.0) as usize;
        if position + SIZE > padded.len() {
            break;
        }
        for (i, f) in frame.iter_mut().enumerate() {
            *f = padded[position + i] * window[i];
        }
        let _ = forward.process(&mut frame, &mut spectrum);
        let mut flux = 0.0;
        for k in 0..bins {
            magnitude[k] = spectrum[k].norm();
            phase[k] = spectrum[k].arg();
            flux += (magnitude[k] - last_magnitude[k]).max(0.0);
        }
        // A sudden rise of energy above the recent median marks a transient.
        let mut sorted = flux_history.clone();
        sorted.sort_by(f32::total_cmp);
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(f32::MAX);
        let transient = flux > 2.5 * median && flux > 1e-3;
        flux_history.push(flux);
        if flux_history.len() > 12 {
            flux_history.remove(0);
        }

        match previous_position {
            Some(previous) if !transient => {
                let delta = (position as f32 - previous as f32).max(1.0);
                peaks.clear();
                for k in 2..bins - 2 {
                    let v = magnitude[k];
                    if v > magnitude[k - 1]
                        && v >= magnitude[k + 1]
                        && v > magnitude[k - 2]
                        && v >= magnitude[k + 2]
                    {
                        peaks.push(k);
                    }
                }
                let advance = |k: usize| {
                    let omega = std::f32::consts::TAU * k as f32 / SIZE as f32;
                    let deviation = wrap(phase[k] - last_phase[k] - omega * delta);
                    (omega + deviation / delta) * SYNTH_HOP as f32
                };
                if peaks.is_empty() {
                    for (k, p) in synth_phase.iter_mut().enumerate() {
                        *p = wrap(*p + advance(k));
                    }
                } else {
                    // Each peak carries the bins around it, up to halfway to the next peak.
                    let mut start = 0;
                    for (i, &peak) in peaks.iter().enumerate() {
                        let end = peaks
                            .get(i + 1)
                            .map_or(bins, |&next| (peak + next).div_ceil(2));
                        let peak_phase = wrap(synth_phase[peak] + advance(peak));
                        for k in start..end {
                            synth_phase[k] = if k == peak {
                                peak_phase
                            } else {
                                wrap(peak_phase + phase[k] - phase[peak])
                            };
                        }
                        start = end;
                    }
                }
            }
            _ => synth_phase.copy_from_slice(&phase),
        }
        last_phase.copy_from_slice(&phase);
        last_magnitude.copy_from_slice(&magnitude);
        previous_position = Some(position);

        for k in 0..bins {
            spectrum[k] = Complex::from_polar(magnitude[k], synth_phase[k]);
        }
        spectrum[0].im = 0.0;
        spectrum[bins - 1].im = 0.0;
        let _ = inverse.process(&mut spectrum, &mut frame);
        let at = m * SYNTH_HOP;
        for i in 0..SIZE {
            let w = window[i];
            output[at + i] += frame[i] * w / SIZE as f32;
            weight[at + i] += w * w;
        }
    }
    let offset = SIZE / 2;
    (0..out_len)
        .map(|i| {
            let w = weight[offset + i];
            if w > 1e-3 {
                output[offset + i] / w
            } else {
                0.0
            }
        })
        .collect()
}

/// Stretches by `ratio` and transposes by `semitones`, at `rate` Hz.
pub fn stretch_and_shift(input: &[f32], ratio: f64, semitones: f32, rate: u32) -> Vec<f32> {
    if semitones.abs() < 1e-3 {
        return stretch(input, ratio);
    }
    let factor = 2f64.powf(f64::from(semitones) / 12.0);
    let longer = stretch(input, ratio * factor);
    // Played back faster by `factor`: same length as asked, pitch moved.
    let from = (f64::from(rate) * factor).round() as u32;
    let mut shifted = Resampler::convert(&longer, from, rate);
    shifted.resize((input.len() as f64 * ratio).round() as usize, 0.0);
    shifted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.5 * (std::f32::consts::TAU * f * i as f32 / 48_000.0).sin())
            .collect()
    }

    /// Frequency from upward zero crossings in the middle of the signal.
    fn frequency(x: &[f32]) -> f32 {
        let part = &x[x.len() / 4..3 * x.len() / 4];
        let crossings = part
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f32 * 48_000.0 / part.len() as f32
    }

    fn rms(x: &[f32]) -> f32 {
        let part = &x[x.len() / 4..3 * x.len() / 4];
        (part.iter().map(|s| s * s).sum::<f32>() / part.len() as f32).sqrt()
    }

    #[test]
    fn keeps_pitch_and_level_while_stretching() {
        let input = sine(440.0, 96_000);
        for ratio in [0.75, 1.5, 2.0] {
            let out = stretch(&input, ratio);
            assert_eq!(out.len(), (96_000.0 * ratio) as usize);
            assert!(
                (frequency(&out) - 440.0).abs() < 3.0,
                "{ratio}: {}",
                frequency(&out)
            );
            assert!(
                (rms(&out) - rms(&input)).abs() < 0.05,
                "{ratio}: {}",
                rms(&out)
            );
        }
    }

    #[test]
    fn shifts_pitch_without_changing_length() {
        let input = sine(440.0, 96_000);
        let up = stretch_and_shift(&input, 1.0, 12.0, 48_000);
        assert_eq!(up.len(), input.len());
        assert!((frequency(&up) - 880.0).abs() < 6.0, "{}", frequency(&up));
        let down = stretch_and_shift(&input, 1.25, -7.0, 48_000);
        assert_eq!(down.len(), 120_000);
        let expected = 440.0 * 2f32.powf(-7.0 / 12.0);
        assert!(
            (frequency(&down) - expected).abs() < 4.0,
            "{}",
            frequency(&down)
        );
    }

    #[test]
    fn keeps_attacks_sharp() {
        // Clicks every 0.25 s; stretched by 2 they must stay short.
        let mut input = vec![0.0f32; 48_000];
        for k in 0..4 {
            for i in 0..64 {
                input[k * 12_000 + i] = 0.8 * (1.0 - i as f32 / 64.0);
            }
        }
        let out = stretch(&input, 2.0);
        let near = &out[24_000 - 2000..24_000 + 6000];
        let peak = near.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let spread = near.iter().filter(|s| s.abs() > peak * 0.25).count();
        assert!(peak > 0.2, "{peak}");
        assert!(
            spread < 1500,
            "{spread} samples above a quarter of the peak"
        );
    }
}
