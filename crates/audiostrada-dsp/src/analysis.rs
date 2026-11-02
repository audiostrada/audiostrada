//! Offline analysis of audio: where it is silent (Strip Silence), where notes and hits start
//! (Beat Detective).

use crate::db_to_gain;

/// Settings of a silence search, in samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SilenceSettings {
    /// Below this peak (dBFS, on every channel) the audio counts as silent.
    pub threshold_db: f32,
    /// Silences shorter than this do not split the audio.
    pub min_silence: usize,
    /// Audio kept before each region and after it, so attacks and tails are not cut.
    pub pad_before: usize,
    pub pad_after: usize,
}

/// Ranges `start..end` (in frames) that hold audio, padded and merged.
pub fn non_silent(channels: &[&[f32]], settings: &SilenceSettings) -> Vec<(usize, usize)> {
    let frames = channels.iter().map(|c| c.len()).max().unwrap_or(0);
    let threshold = db_to_gain(settings.threshold_db);
    let loud = |i: usize| {
        channels.iter().any(|c| {
            c.get(i)
                .is_some_and(|v| v.is_finite() && v.abs() > threshold)
        })
    };
    let mut raw: Vec<(usize, usize)> = Vec::new();
    let mut region: Option<(usize, usize)> = None;
    for i in 0..frames {
        if !loud(i) {
            continue;
        }
        region = match region {
            // A long enough silence since the last loud sample ends the region.
            Some((start, last)) if i - last > settings.min_silence.max(1) => {
                raw.push((start, last + 1));
                Some((i, i))
            }
            Some((start, _)) => Some((start, i)),
            None => Some((i, i)),
        };
    }
    if let Some((start, last)) = region {
        raw.push((start, last + 1));
    }
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(raw.len());
    for (start, end) in raw {
        let start = start.saturating_sub(settings.pad_before);
        let end = (end + settings.pad_after).min(frames);
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// Attacks (onsets) in audio, in frames: a rise of energy across frequency bands (spectral
/// flux on log-spaced bands, compared with the neighbouring bands of the previous frame so a
/// vibrato does not count), above a moving threshold that `sensitivity` (0 to 1) lowers. Each
/// is placed where the wave starts to rise.
pub fn onsets(channels: &[&[f32]], rate: u32, sensitivity: f32) -> Vec<usize> {
    detect_onsets(channels, rate).pick(sensitivity)
}

/// The possible attacks of some audio, from which [`Onsets::pick`] keeps those a sensitivity
/// lets through, without analysing again.
#[derive(Clone, Debug, Default)]
pub struct Onsets {
    candidates: Vec<Candidate>,
    gap: usize,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    frame: usize,
    flux: f32,
    mean: f32,
    start: usize,
}

impl Onsets {
    /// Attacks, in frames, at `sensitivity` (0 to 1).
    pub fn pick(&self, sensitivity: f32) -> Vec<usize> {
        let delta = 0.08 * 2f32.powf((0.5 - sensitivity.clamp(0.0, 1.0)) * 4.0);
        let mut found: Vec<usize> = Vec::new();
        let mut last: Option<usize> = None;
        for c in &self.candidates {
            if c.flux < c.mean * 1.5 + delta || last.is_some_and(|l| c.frame - l < self.gap) {
                continue;
            }
            last = Some(c.frame);
            found.push(c.start);
        }
        found.dedup();
        found
    }
}

/// The analysis behind [`onsets`], done once for every sensitivity.
pub fn detect_onsets(channels: &[&[f32]], rate: u32) -> Onsets {
    use realfft::RealFftPlanner;
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let rate = rate.max(8000) as f32;
    let size = ((rate * 0.021) as usize)
        .clamp(256, 4096)
        .next_power_of_two();
    let hop = size / 4;
    if frames < size || channels.is_empty() {
        return Onsets::default();
    }
    let n = channels.len() as f32;
    let mono: Vec<f32> = (0..frames)
        .map(|i| {
            channels
                .iter()
                .map(|c| if c[i].is_finite() { c[i] } else { 0.0 })
                .sum::<f32>()
                / n
        })
        .collect();
    // 24 bands, log-spaced from 40 Hz to the top.
    let bins = size / 2 + 1;
    let bin_of = |hz: f32| ((hz / rate * size as f32) as usize).min(bins - 1);
    let edges: Vec<usize> = (0..=24)
        .map(|b| bin_of(40.0 * (rate / 2.0 / 40.0).powf(b as f32 / 24.0)))
        .collect();
    let window: Vec<f32> = (0..size)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
    let mut frame = vec![0.0f32; size];
    let mut spectrum = fft.make_output_vec();
    let count = (frames - size) / hop + 1;
    let mut flux = Vec::with_capacity(count);
    let mut previous: Vec<f32> = Vec::new();
    for m in 0..count {
        for (i, f) in frame.iter_mut().enumerate() {
            *f = mono[m * hop + i] * window[i];
        }
        let _ = fft.process(&mut frame, &mut spectrum);
        let bands: Vec<f32> = edges
            .windows(2)
            .map(|e| {
                let energy: f32 = spectrum[e[0]..=e[1].max(e[0])]
                    .iter()
                    .map(|c| c.norm())
                    .sum();
                (1.0 + 100.0 * energy / size as f32).ln()
            })
            .collect();
        let rise = if previous.is_empty() {
            0.0
        } else {
            bands
                .iter()
                .enumerate()
                .map(|(b, v)| {
                    let before = previous[b.saturating_sub(1)..(b + 2).min(previous.len())]
                        .iter()
                        .copied()
                        .fold(0.0, f32::max);
                    (v - before).max(0.0)
                })
                .sum()
        };
        flux.push(rise);
        previous = bands;
    }
    let frames_of = |seconds: f32| ((seconds * rate) as usize / hop).max(1);
    let (near, before, after, gap) = (
        frames_of(0.03),
        frames_of(0.1),
        frames_of(0.07),
        frames_of(0.03),
    );
    let mut candidates = Vec::new();
    for m in 0..count {
        let span = |a: usize, b: usize| &flux[m.saturating_sub(a)..(m + b + 1).min(count)];
        let peak = span(near, near).iter().copied().fold(0.0, f32::max);
        if flux[m] <= 0.0 || flux[m] < peak {
            continue;
        }
        let around = span(before, after);
        candidates.push(Candidate {
            frame: m,
            flux: flux[m],
            mean: around.iter().sum::<f32>() / around.len() as f32,
            start: attack_start(&mono, m * hop, size),
        });
    }
    Onsets { candidates, gap }
}

/// Where the wave starts to rise before the loudest point of `at..at + size` (and a hop
/// before it).
fn attack_start(mono: &[f32], at: usize, size: usize) -> usize {
    let from = at.saturating_sub(size / 4);
    let to = (at + size).min(mono.len());
    let (peak_at, peak) = mono[from..to]
        .iter()
        .enumerate()
        .fold(
            (0, 0.0f32),
            |(i, p), (j, v)| if v.abs() > p { (j, v.abs()) } else { (i, p) },
        );
    let mut start = from + peak_at;
    // Back to where the level falls under a tenth of the peak.
    while start > from && mono[start - 1].abs() > peak * 0.1 {
        start -= 1;
    }
    // A wave crosses zero inside a note: look for a quiet stretch, not one quiet sample.
    while start > from {
        let quiet = mono[start.saturating_sub(16).max(from)..start]
            .iter()
            .all(|v| v.abs() <= peak * 0.1);
        if quiet {
            break;
        }
        start -= 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(min_silence: usize, pad: usize) -> SilenceSettings {
        SilenceSettings {
            threshold_db: -40.0,
            min_silence,
            pad_before: pad,
            pad_after: pad,
        }
    }

    #[test]
    fn finds_the_audio_between_silences() {
        // Tone, silence, tone, a short dip, tone.
        let mut left = vec![0.0f32; 1000];
        left[100..200].fill(0.5);
        left[500..600].fill(0.5);
        left[610..700].fill(0.5);
        let right = vec![0.0f32; 1000];
        let found = non_silent(&[&left, &right], &settings(50, 0));
        assert_eq!(
            found,
            [(100, 200), (500, 700)],
            "a 10-sample dip does not split"
        );
        let padded = non_silent(&[&left, &right], &settings(50, 20));
        assert_eq!(padded, [(80, 220), (480, 720)]);
        // Pads that touch merge the regions.
        let merged = non_silent(&[&left, &right], &settings(50, 200));
        assert_eq!(merged, [(0, 900)]);
    }

    #[test]
    fn any_channel_counts_and_silence_gives_nothing() {
        let left = vec![0.0f32; 100];
        let mut right = vec![0.0f32; 100];
        right[40] = -0.9;
        assert_eq!(non_silent(&[&left, &right], &settings(10, 0)), [(40, 41)]);
        assert!(non_silent(&[&left], &settings(10, 0)).is_empty());
    }

    /// Hits: decaying noise bursts at known places over faint noise.
    fn hits(at: &[usize], frames: usize) -> Vec<f32> {
        let mut seed = 7u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
        };
        let mut out: Vec<f32> = (0..frames).map(|_| noise() * 0.001).collect();
        for &start in at {
            for i in 0..4800 {
                if let Some(s) = out.get_mut(start + i) {
                    *s += noise() * 0.8 * (-(i as f32) / 600.0).exp();
                }
            }
        }
        out
    }

    #[test]
    fn finds_hits_where_they_start() {
        let at = [4800, 30_000, 41_000, 70_000];
        let audio = hits(&at, 96_000);
        let found = onsets(&[&audio], 48_000, 0.5);
        assert_eq!(found.len(), at.len(), "{found:?}");
        for (f, a) in found.iter().zip(at) {
            assert!(f.abs_diff(a) <= 96, "found {f}, expected {a} (2 ms)");
        }
    }

    #[test]
    fn a_steady_tone_has_one_attack() {
        let tone: Vec<f32> = (0..96_000)
            .map(|i| {
                if i < 2400 {
                    0.0
                } else {
                    0.5 * (i as f32 * 0.05).sin()
                }
            })
            .collect();
        let found = onsets(&[&tone], 48_000, 0.5);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].abs_diff(2400) <= 96);
    }

    #[test]
    #[ignore = "prints the attacks of AUDIOSTRADA_TEST_RAW (mono f32 at 48 kHz)"]
    fn print_attacks() {
        let path = std::env::var("AUDIOSTRADA_TEST_RAW").unwrap();
        let bytes = std::fs::read(path).unwrap();
        let audio: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        for sensitivity in [0.3, 0.5, 0.8] {
            let found = onsets(&[&audio], 48_000, sensitivity);
            let ms: Vec<u64> = found.iter().map(|&f| f as u64 * 1000 / 48_000).collect();
            println!("{sensitivity}: {} attacks at {ms:?} ms", found.len());
        }
    }
}
