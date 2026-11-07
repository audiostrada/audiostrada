//! Multi-resolution peaks for waveform display, with a compact cache file format. Each peak
//! keeps the extremes, the mean square (for an RMS outline) and how the energy splits between
//! lows, mids and highs (for spectral colors).

use std::fs;
use std::io;
use std::path::Path;

use crate::AudioData;

const MAGIC: &[u8; 4] = b"SLPK";
const VERSION: u32 = 2;
/// Frames summarized by one peak at each level, finest first.
const LEVELS: [u32; 3] = [256, 4096, 65_536];
/// Band edges in Hz: lows below the first, highs above the second.
const LOW_EDGE: f32 = 250.0;
const HIGH_EDGE: f32 = 4000.0;
/// Bytes per peak in the file.
const PEAK_BYTES: usize = 16;

/// One stretch of a channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub lo: f32,
    pub hi: f32,
    /// Mean of the squared samples.
    pub mean_square: f32,
    /// Shares of the energy in the lows, mids and highs, out of 255.
    pub bands: [u8; 3],
}

impl Peak {
    pub const EMPTY: Peak = Peak {
        lo: f32::MAX,
        hi: f32::MIN,
        mean_square: 0.0,
        bands: [0; 3],
    };

    pub fn rms(&self) -> f32 {
        self.mean_square.sqrt()
    }

    /// Shares of the energy in the lows, mids and highs, summing to 1 (all 0 when silent).
    pub fn spectrum(&self) -> [f32; 3] {
        let sum: f32 = self.bands.iter().map(|&b| f32::from(b)).sum();
        if sum <= 0.0 {
            return [0.0; 3];
        }
        self.bands.map(|b| f32::from(b) / sum)
    }

    pub fn is_empty(&self) -> bool {
        self.lo > self.hi
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Level {
    frames_per_peak: u32,
    /// Per channel.
    peaks: Vec<Vec<Peak>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    channels: usize,
    frames: u64,
    sample_rate: u32,
    levels: Vec<Level>,
}

/// Shares of `energies`, out of 255.
fn shares(energies: [f32; 3]) -> [u8; 3] {
    let sum: f32 = energies.iter().sum();
    if sum <= 1e-20 {
        return [0; 3];
    }
    energies.map(|e| (e / sum * 255.0).round() as u8)
}

/// The finest peaks of one channel, with its energy split by simple one-pole filters.
fn finest(samples: &[f32], rate: u32, frames_per_peak: usize) -> Vec<Peak> {
    let coefficient = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / rate.max(1) as f32).exp();
    let (a_low, a_high) = (coefficient(LOW_EDGE), coefficient(HIGH_EDGE));
    let (mut low, mut below_high) = (0.0f32, 0.0f32);
    samples
        .chunks(frames_per_peak)
        .map(|chunk| {
            let mut peak = Peak::EMPTY;
            let mut energy = [0.0f32; 3];
            let mut square = 0.0f32;
            for &s in chunk {
                peak.lo = peak.lo.min(s);
                peak.hi = peak.hi.max(s);
                square += s * s;
                low += a_low * (s - low);
                below_high += a_high * (s - below_high);
                let (l, m, h) = (low, below_high - low, s - below_high);
                energy[0] += l * l;
                energy[1] += m * m;
                energy[2] += h * h;
            }
            peak.mean_square = square / chunk.len().max(1) as f32;
            peak.bands = shares(energy);
            peak
        })
        .collect()
}

/// One peak for several, weighted by how much each holds.
fn merge(peaks: &[Peak]) -> Peak {
    let mut out = Peak::EMPTY;
    let mut energy = [0.0f32; 3];
    for p in peaks {
        out.lo = out.lo.min(p.lo);
        out.hi = out.hi.max(p.hi);
        out.mean_square += p.mean_square;
        for (e, share) in energy.iter_mut().zip(p.spectrum()) {
            *e += share * p.mean_square;
        }
    }
    out.mean_square /= peaks.len().max(1) as f32;
    out.bands = shares(energy);
    out
}

impl Peaks {
    pub fn compute(data: &AudioData) -> Self {
        let mut levels: Vec<Level> = Vec::with_capacity(LEVELS.len());
        for &frames_per_peak in &LEVELS {
            let peaks = (0..data.channels())
                .map(|ch| match levels.last() {
                    // Coarser levels are built from the previous one, not from the samples.
                    Some(prev) => {
                        let ratio = (frames_per_peak / prev.frames_per_peak) as usize;
                        prev.peaks[ch].chunks(ratio).map(merge).collect()
                    }
                    None => finest(
                        data.channel(ch),
                        data.sample_rate(),
                        frames_per_peak as usize,
                    ),
                })
                .collect();
            levels.push(Level {
                frames_per_peak,
                peaks,
            });
        }
        Self {
            channels: data.channels(),
            frames: data.frames() as u64,
            sample_rate: data.sample_rate(),
            levels,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Finest resolution available, in frames per peak.
    pub fn finest(&self) -> u32 {
        self.levels.first().map_or(LEVELS[0], |l| l.frames_per_peak)
    }

    /// Fills `out` with one peak per pixel, starting at source frame `start` with
    /// `frames_per_pixel` frames per pixel. Pixels past the end are empty.
    pub fn query(&self, channel: usize, start: f64, frames_per_pixel: f64, out: &mut [Peak]) {
        out.fill(Peak::EMPTY);
        let Some(level) = self
            .levels
            .iter()
            .rev()
            .find(|l| f64::from(l.frames_per_peak) <= frames_per_pixel)
            .or(self.levels.first())
        else {
            return;
        };
        let Some(peaks) = level.peaks.get(channel) else {
            return;
        };
        let per_peak = f64::from(level.frames_per_peak);
        for (pixel, slot) in out.iter_mut().enumerate() {
            let from = start + pixel as f64 * frames_per_pixel;
            let to = from + frames_per_pixel;
            if to <= 0.0 {
                continue;
            }
            let first = (from.max(0.0) / per_peak).floor() as usize;
            let last = ((to / per_peak).ceil() as usize)
                .max(first + 1)
                .min(peaks.len());
            if first < last {
                *slot = merge(&peaks[first..last]);
            }
        }
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        for value in [
            VERSION,
            self.channels as u32,
            self.sample_rate,
            self.levels.len() as u32,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.frames.to_le_bytes());
        for level in &self.levels {
            out.extend_from_slice(&level.frames_per_peak.to_le_bytes());
            let count = level.peaks.first().map_or(0, Vec::len) as u32;
            out.extend_from_slice(&count.to_le_bytes());
            for channel in &level.peaks {
                for p in channel {
                    out.extend_from_slice(&p.lo.to_le_bytes());
                    out.extend_from_slice(&p.hi.to_le_bytes());
                    out.extend_from_slice(&p.mean_square.to_le_bytes());
                    out.extend_from_slice(&p.bands);
                    out.push(0);
                }
            }
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, out)
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let invalid =
            |what: &str| io::Error::new(io::ErrorKind::InvalidData, format!("peak file: {what}"));
        let bytes = fs::read(path)?;
        let mut at = 0;
        let mut take = |n: usize| -> io::Result<&[u8]> {
            let slice = bytes.get(at..at + n).ok_or_else(|| invalid("truncated"))?;
            at += n;
            Ok(slice)
        };
        if take(4)? != MAGIC {
            return Err(invalid("bad magic"));
        }
        let mut u32_at = || -> io::Result<u32> {
            Ok(u32::from_le_bytes(
                take(4)?.try_into().map_err(|_| invalid("u32"))?,
            ))
        };
        if u32_at()? != VERSION {
            return Err(invalid("unsupported version"));
        }
        let channels = u32_at()? as usize;
        let sample_rate = u32_at()?;
        let level_count = u32_at()? as usize;
        let frames = u64::from_le_bytes(take(8)?.try_into().map_err(|_| invalid("u64"))?);
        if channels > 1024 || level_count > 16 {
            return Err(invalid("implausible header"));
        }
        let float = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            let frames_per_peak =
                u32::from_le_bytes(take(4)?.try_into().map_err(|_| invalid("u32"))?);
            let count =
                u32::from_le_bytes(take(4)?.try_into().map_err(|_| invalid("u32"))?) as usize;
            let data = take(count * channels * PEAK_BYTES)?;
            // An empty level (silent recording) has no bytes to split per channel.
            let peaks = if count == 0 {
                vec![Vec::new(); channels]
            } else {
                data.chunks(count * PEAK_BYTES)
                    .map(|channel| {
                        channel
                            .as_chunks::<PEAK_BYTES>()
                            .0
                            .iter()
                            .map(|p| Peak {
                                lo: float(&p[0..4]),
                                hi: float(&p[4..8]),
                                mean_square: float(&p[8..12]),
                                bands: [p[12], p[13], p[14]],
                            })
                            .collect()
                    })
                    .collect()
            };
            levels.push(Level {
                frames_per_peak,
                peaks,
            });
        }
        Ok(Self {
            channels,
            frames,
            sample_rate,
            levels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_data() -> AudioData {
        let left: Vec<f32> = (0..100_000).map(|i| (i % 1000) as f32 / 1000.0).collect();
        let right: Vec<f32> = left.iter().map(|s| -s).collect();
        AudioData::new(vec![left, right], 48_000)
    }

    fn sine(hz: f32) -> AudioData {
        let wave = (0..96_000)
            .map(|i| (i as f32 * std::f32::consts::TAU * hz / 48_000.0).sin())
            .collect();
        AudioData::new(vec![wave], 48_000)
    }

    #[test]
    fn levels_summarize_min_and_max() {
        let peaks = Peaks::compute(&ramp_data());
        let mut out = [Peak::EMPTY; 4];
        peaks.query(0, 0.0, 1000.0, &mut out);
        for p in &out {
            assert!(p.lo <= 0.01 && p.hi >= 0.99, "{p:?}");
        }
        peaks.query(1, 0.0, 70_000.0, &mut out);
        assert!(out[0].lo <= -0.99 && out[0].hi >= -0.01);
        assert!(out[3].is_empty(), "past the end");
    }

    #[test]
    fn fine_query_tracks_the_signal() {
        let peaks = Peaks::compute(&ramp_data());
        let mut out = [Peak::EMPTY; 2];
        // 256-frame peaks from frame 256: values 0.256..0.511.
        peaks.query(0, 256.0, 256.0, &mut out);
        assert!((out[0].lo - 0.256).abs() < 1e-6 && (out[0].hi - 0.511).abs() < 1e-6);
    }

    #[test]
    fn rms_and_bands_tell_lows_from_highs() {
        let mut out = [Peak::EMPTY; 1];
        let low = Peaks::compute(&sine(60.0));
        low.query(0, 48_000.0, 40_000.0, &mut out);
        assert!(
            (out[0].rms() - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "{:?}",
            out[0]
        );
        let spectrum = out[0].spectrum();
        assert!(spectrum[0] > 0.8, "a 60 Hz sine is low: {spectrum:?}");
        let high = Peaks::compute(&sine(10_000.0));
        high.query(0, 48_000.0, 40_000.0, &mut out);
        let spectrum = out[0].spectrum();
        assert!(spectrum[2] > 0.6, "a 10 kHz sine is high: {spectrum:?}");
    }

    #[test]
    fn file_round_trip_and_corruption() {
        let peaks = Peaks::compute(&ramp_data());
        let path = audiostrada_core::scratch::dir("peaks.peaks");
        peaks.write(&path).unwrap();
        assert_eq!(Peaks::read(&path).unwrap(), peaks);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(Peaks::read(&path).is_err());
        fs::write(&path, b"XXXX").unwrap();
        assert!(Peaks::read(&path).is_err());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn empty_audio_round_trips() {
        let peaks = Peaks::compute(&AudioData::new(vec![Vec::new(), Vec::new()], 44_100));
        assert_eq!((peaks.channels(), peaks.frames()), (2, 0));
        assert_eq!(peaks.sample_rate(), 44_100);
        let path = audiostrada_core::scratch::dir("empty.peaks");
        peaks.write(&path).unwrap();
        assert_eq!(Peaks::read(&path).unwrap(), peaks);
        let mut out = [Peak::EMPTY; 3];
        peaks.query(0, 0.0, 100.0, &mut out);
        assert!(out.iter().all(Peak::is_empty));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn query_edges() {
        let peaks = Peaks::compute(&ramp_data());
        assert_eq!(peaks.finest(), LEVELS[0]);
        let mut out = [Peak::EMPTY; 2];
        peaks.query(5, 0.0, 256.0, &mut out);
        assert!(out.iter().all(Peak::is_empty), "unknown channel");
        // Pixels before the start stay empty; a zoom finer than the finest level uses it.
        peaks.query(0, -512.0, 256.0, &mut out);
        assert!(out[0].is_empty());
        let mut fine = [Peak::EMPTY; 1];
        peaks.query(0, 0.0, 10.0, &mut fine);
        assert!((fine[0].lo - 0.0).abs() < 1e-6 && (fine[0].hi - 0.255).abs() < 1e-6);
    }

    #[test]
    fn bad_headers_are_rejected() {
        let path = audiostrada_core::scratch::dir("bad.peaks");
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        assert!(Peaks::read(&path).is_err(), "version");
        let mut bytes = MAGIC.to_vec();
        for v in [VERSION, 5000, 48_000, 1] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&0u64.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        assert!(Peaks::read(&path).is_err(), "channels");
        fs::remove_file(&path).unwrap();
        assert!(Peaks::read(&path).is_err(), "missing");
    }
}
