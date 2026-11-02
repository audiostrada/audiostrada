//! Multi-resolution min/max peaks for waveform display, with a compact cache file format.

use std::fs;
use std::io;
use std::path::Path;

use crate::AudioData;

const MAGIC: &[u8; 4] = b"SLPK";
const VERSION: u32 = 1;
/// Frames summarized by one peak at each level, finest first.
const LEVELS: [u32; 3] = [256, 4096, 65_536];

#[derive(Clone, Debug, PartialEq)]
struct Level {
    frames_per_peak: u32,
    /// Per channel: (min, max) pairs.
    peaks: Vec<Vec<(f32, f32)>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    channels: usize,
    frames: u64,
    sample_rate: u32,
    levels: Vec<Level>,
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
                    None => data
                        .channel(ch)
                        .chunks(frames_per_peak as usize)
                        .map(|c| {
                            c.iter()
                                .fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)))
                        })
                        .collect(),
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

    /// Fills `out` with one (min, max) per pixel, starting at source frame `start` with
    /// `frames_per_pixel` frames per pixel. Pixels past the end are (0, 0).
    pub fn query(&self, channel: usize, start: f64, frames_per_pixel: f64, out: &mut [(f32, f32)]) {
        out.fill((0.0, 0.0));
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
                for &(lo, hi) in channel {
                    out.extend_from_slice(&lo.to_le_bytes());
                    out.extend_from_slice(&hi.to_le_bytes());
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
        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            let frames_per_peak =
                u32::from_le_bytes(take(4)?.try_into().map_err(|_| invalid("u32"))?);
            let count =
                u32::from_le_bytes(take(4)?.try_into().map_err(|_| invalid("u32"))?) as usize;
            let data = take(count * channels * 8)?;
            // An empty level (silent recording) has no bytes to split per channel.
            let peaks = if count == 0 {
                vec![Vec::new(); channels]
            } else {
                data.chunks(count * 8)
                    .map(|channel| {
                        channel
                            .as_chunks::<8>()
                            .0
                            .iter()
                            .map(|p| {
                                let lo = f32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                                let hi = f32::from_le_bytes([p[4], p[5], p[6], p[7]]);
                                (lo, hi)
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

fn merge(peaks: &[(f32, f32)]) -> (f32, f32) {
    peaks
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &(l, h)| {
            (lo.min(l), hi.max(h))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_data() -> AudioData {
        let left: Vec<f32> = (0..100_000).map(|i| (i % 1000) as f32 / 1000.0).collect();
        let right: Vec<f32> = left.iter().map(|s| -s).collect();
        AudioData::new(vec![left, right], 48_000)
    }

    #[test]
    fn levels_summarize_min_and_max() {
        let peaks = Peaks::compute(&ramp_data());
        let mut out = [(0.0, 0.0); 4];
        peaks.query(0, 0.0, 1000.0, &mut out);
        for &(lo, hi) in &out {
            assert!(lo <= 0.01 && hi >= 0.99, "{lo} {hi}");
        }
        peaks.query(1, 0.0, 70_000.0, &mut out);
        assert!(out[0].0 <= -0.99 && out[0].1 >= -0.01);
        assert_eq!(out[3], (0.0, 0.0), "past the end");
    }

    #[test]
    fn fine_query_tracks_the_signal() {
        let peaks = Peaks::compute(&ramp_data());
        let mut out = [(0.0, 0.0); 2];
        // 256-frame peaks from frame 256: values 0.256..0.511.
        peaks.query(0, 256.0, 256.0, &mut out);
        assert!((out[0].0 - 0.256).abs() < 1e-6 && (out[0].1 - 0.511).abs() < 1e-6);
    }

    #[test]
    fn file_round_trip_and_corruption() {
        let peaks = Peaks::compute(&ramp_data());
        let path =
            std::env::temp_dir().join(format!("audiostrada-peaks-{}.peaks", std::process::id()));
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
        let path =
            std::env::temp_dir().join(format!("audiostrada-empty-{}.peaks", std::process::id()));
        peaks.write(&path).unwrap();
        assert_eq!(Peaks::read(&path).unwrap(), peaks);
        let mut out = [(1.0, 1.0); 3];
        peaks.query(0, 0.0, 100.0, &mut out);
        assert_eq!(out, [(0.0, 0.0); 3]);
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn query_edges() {
        let peaks = Peaks::compute(&ramp_data());
        assert_eq!(peaks.finest(), LEVELS[0]);
        let mut out = [(1.0, 1.0); 2];
        peaks.query(5, 0.0, 256.0, &mut out);
        assert_eq!(out, [(0.0, 0.0); 2], "unknown channel");
        // Pixels before the start stay empty; a zoom finer than the finest level uses it.
        peaks.query(0, -512.0, 256.0, &mut out);
        assert_eq!(out[0], (0.0, 0.0));
        let mut fine = [(0.0, 0.0); 1];
        peaks.query(0, 0.0, 10.0, &mut fine);
        assert!((fine[0].0 - 0.0).abs() < 1e-6 && (fine[0].1 - 0.255).abs() < 1e-6);
    }

    #[test]
    fn bad_headers_are_rejected() {
        let path =
            std::env::temp_dir().join(format!("audiostrada-bad-{}.peaks", std::process::id()));
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
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
