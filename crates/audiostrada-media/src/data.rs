/// Decoded audio held in memory, planar f32.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    channels: Vec<Box<[f32]>>,
    sample_rate: u32,
    frames: usize,
}

impl AudioData {
    /// Builds audio from planar channels; shorter channels are padded with silence.
    pub fn new(channels: Vec<Vec<f32>>, sample_rate: u32) -> Self {
        let frames = channels.iter().map(Vec::len).max().unwrap_or(0);
        let channels = channels
            .into_iter()
            .map(|mut c| {
                c.resize(frames, 0.0);
                c.into_boxed_slice()
            })
            .collect();
        Self {
            channels,
            sample_rate,
            frames,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, index: usize) -> &[f32] {
        &self.channels[index]
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_are_padded_to_the_longest() {
        let data = AudioData::new(vec![vec![1.0; 4], vec![2.0; 2]], 48_000);
        assert_eq!(data.frames(), 4);
        assert_eq!(data.channel(1), &[2.0, 2.0, 0.0, 0.0]);
        assert_eq!(data.duration_secs(), 4.0 / 48_000.0);
    }
}
