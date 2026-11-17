//! Offline sample rate conversion (FFT based, high quality).

use rubato::audioadapter_buffers::direct::SequentialSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::{AudioData, MediaError};

const CHUNK: usize = 1024;

/// Converts `data` to `rate`; returns a copy when the rate already matches.
pub fn resample(data: &AudioData, rate: u32) -> Result<AudioData, MediaError> {
    if data.sample_rate() == rate || data.frames() == 0 {
        let channels = (0..data.channels())
            .map(|c| data.channel(c).to_vec())
            .collect();
        return Ok(AudioData::new(channels, rate));
    }
    // One thread per channel: a long file takes seconds per channel.
    let channels = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..data.channels())
            .map(|c| scope.spawn(move || channel(data.channel(c), data.sample_rate(), rate)))
            .collect();
        jobs.into_iter()
            .map(|job| {
                job.join()
                    .unwrap_or_else(|_| Err(MediaError::Resample("resampling panicked".into())))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    Ok(AudioData::new(channels, rate))
}

fn channel(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, MediaError> {
    let error = |e: &dyn std::fmt::Display| MediaError::Resample(e.to_string());
    let frames = samples.len();
    let mut resampler = Fft::<f32>::new(from as usize, to as usize, CHUNK, 1, FixedSync::Input)
        .map_err(|e| error(&e))?;
    let out_len = resampler.process_all_needed_output_len(frames);
    let mut output = vec![0.0; out_len];
    let produced = {
        let input = SequentialSlice::new(samples, 1, frames).map_err(|e| error(&e))?;
        let mut target =
            SequentialSlice::new_mut(&mut output, 1, out_len).map_err(|e| error(&e))?;
        resampler
            .process_all_into_buffer(&input, &mut target, frames, None)
            .map_err(|e| error(&e))?
            .1
    };
    output.truncate(produced);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_rate_or_empty_audio_is_copied() {
        let data = AudioData::new(vec![vec![0.1, 0.2, 0.3]], 48_000);
        assert_eq!(resample(&data, 48_000).unwrap(), data);
        let empty = resample(&AudioData::new(vec![Vec::new(); 2], 44_100), 48_000).unwrap();
        assert_eq!((empty.channels(), empty.frames()), (2, 0));
        assert_eq!(empty.sample_rate(), 48_000);
    }

    #[test]
    fn length_follows_the_rate_ratio() {
        let data = AudioData::new(vec![vec![0.25; 44_100]; 2], 44_100);
        let out = resample(&data, 22_050).unwrap();
        assert_eq!(out.sample_rate(), 22_050);
        assert!(out.frames().abs_diff(22_050) <= 1, "{}", out.frames());
        assert!((out.channel(1)[11_000] - 0.25).abs() < 1e-3);
    }
}
