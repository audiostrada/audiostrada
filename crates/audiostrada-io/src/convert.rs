//! Conversion between interleaved hardware sample formats and planar f32.

use crate::ChannelBuffers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SampleFormat {
    F32,
    S32,
    S24Packed,
    S16,
}

impl SampleFormat {
    pub(crate) fn bytes(self) -> usize {
        match self {
            SampleFormat::F32 | SampleFormat::S32 => 4,
            SampleFormat::S24Packed => 3,
            SampleFormat::S16 => 2,
        }
    }

    #[inline]
    pub(crate) fn read(self, b: &[u8]) -> f32 {
        match self {
            SampleFormat::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            SampleFormat::S32 => {
                i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0
            }
            SampleFormat::S24Packed => {
                (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
            }
            SampleFormat::S16 => i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0,
        }
    }

    #[inline]
    pub(crate) fn write(self, v: f32, b: &mut [u8]) {
        let c = v.clamp(-1.0, 1.0);
        match self {
            SampleFormat::F32 => b[..4].copy_from_slice(&v.to_le_bytes()),
            SampleFormat::S32 => {
                b[..4].copy_from_slice(&((f64::from(c) * 2_147_483_647.0) as i32).to_le_bytes())
            }
            SampleFormat::S24Packed => {
                let i = (c * 8_388_607.0) as i32;
                b[..3].copy_from_slice(&i.to_le_bytes()[..3]);
            }
            SampleFormat::S16 => b[..2].copy_from_slice(&((c * 32_767.0) as i16).to_le_bytes()),
        }
    }
}

/// Splits `frames` interleaved frames of `channels` channels into the storage inputs.
pub(crate) fn deinterleave(
    format: SampleFormat,
    src: &[u8],
    channels: usize,
    frames: usize,
    dst: &mut ChannelBuffers,
    dst_channels: usize,
) {
    let size = format.bytes();
    for ch in 0..channels.min(dst_channels) {
        let out = &mut dst.input_mut(ch)[..frames];
        for (f, sample) in out.iter_mut().enumerate() {
            let at = (f * channels + ch) * size;
            *sample = format.read(&src[at..at + size]);
        }
    }
}

/// Interleaves the storage outputs into `dst`; device channels without a source are silent.
pub(crate) fn interleave(
    format: SampleFormat,
    src: &ChannelBuffers,
    src_channels: usize,
    dst: &mut [u8],
    channels: usize,
    frames: usize,
) {
    let size = format.bytes();
    for ch in 0..channels {
        for f in 0..frames {
            let at = (f * channels + ch) * size;
            let v = if ch < src_channels {
                src.output(ch)[f]
            } else {
                0.0
            };
            format.write(v, &mut dst[at..at + size]);
        }
    }
}

/// Interleaved capture samples waiting for the render clock, when capture and playback
/// run on separate device clocks.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Fifo {
    data: Vec<f32>,
    channels: usize,
    read: usize,
    len: usize,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Fifo {
    pub(crate) fn new(channels: usize, frames: usize) -> Self {
        Self {
            data: vec![0.0; channels * frames],
            channels,
            read: 0,
            len: 0,
        }
    }

    pub(crate) fn frames(&self) -> usize {
        self.len / self.channels
    }

    /// Appends little-endian f32 samples, dropping the oldest frames on overflow.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> bool {
        let cap = self.data.len();
        let mut overflow = false;
        for chunk in bytes.as_chunks::<4>().0 {
            if self.len == cap {
                self.read = (self.read + 1) % cap;
                self.len -= 1;
                overflow = true;
            }
            self.data[(self.read + self.len) % cap] = f32::from_le_bytes(*chunk);
            self.len += 1;
        }
        overflow
    }

    /// Moves `frames` frames into the storage inputs; returns false on underflow (zero filled).
    pub(crate) fn pop(
        &mut self,
        storage: &mut ChannelBuffers,
        inputs: usize,
        frames: usize,
    ) -> bool {
        let available = self.frames().min(frames);
        let cap = self.data.len();
        for f in 0..frames {
            for ch in 0..self.channels.min(inputs) {
                storage.input_mut(ch)[f] = if f < available {
                    self.data[(self.read + f * self.channels + ch) % cap]
                } else {
                    0.0
                };
            }
        }
        self.read = (self.read + available * self.channels) % cap;
        self.len -= available * self.channels;
        available == frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_round_trip() {
        let values = [0.0, 0.5, -0.5, 0.999, -1.0];
        for format in [
            SampleFormat::F32,
            SampleFormat::S32,
            SampleFormat::S24Packed,
            SampleFormat::S16,
        ] {
            let mut bytes = vec![0u8; format.bytes()];
            for v in values {
                format.write(v, &mut bytes);
                let back = format.read(&bytes);
                assert!((back - v).abs() < 1e-4, "{format:?} {v} -> {back}");
            }
        }
    }

    #[test]
    fn out_of_range_is_clamped_for_integers() {
        let mut bytes = [0u8; 2];
        SampleFormat::S16.write(2.0, &mut bytes);
        assert_eq!(i16::from_le_bytes(bytes), i16::MAX);
    }

    #[test]
    fn fifo_keeps_order_and_reports_underflow() {
        let bytes: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut fifo = Fifo::new(2, 2);
        assert!(fifo.push(&bytes), "third frame overflows a two frame FIFO");
        assert_eq!(fifo.frames(), 2);
        let mut storage = ChannelBuffers::new(2, 0, 4);
        assert!(!fifo.pop(&mut storage, 2, 3));
        assert_eq!(&storage.input_mut(0)[..3], &[3.0, 5.0, 0.0]);
        assert_eq!(&storage.input_mut(1)[..3], &[4.0, 6.0, 0.0]);
        assert_eq!(fifo.frames(), 0);
    }

    #[test]
    fn interleaving_maps_channels() {
        let mut storage = ChannelBuffers::new(2, 1, 4);
        let src: Vec<u8> = [1i16, -1, 2, -2]
            .iter()
            .flat_map(|v| (v * 8_192).to_le_bytes())
            .collect();
        deinterleave(SampleFormat::S16, &src, 2, 2, &mut storage, 2);
        assert_eq!(&storage.input_mut(0)[..2], &[0.25, 0.5]);
        assert_eq!(&storage.input_mut(1)[..2], &[-0.25, -0.5]);

        let mut out = vec![0u8; 2 * 3 * 4];
        storage.buffers(2).output(0).copy_from_slice(&[0.25, -0.25]);
        interleave(SampleFormat::F32, &storage, 1, &mut out, 3, 2);
        let read = |i: usize| f32::from_le_bytes(out[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(
            [read(0), read(1), read(2), read(3)],
            [0.25, 0.0, 0.0, -0.25]
        );
    }
}
