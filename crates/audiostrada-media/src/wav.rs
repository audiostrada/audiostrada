//! WAV writing (32-bit float), whole files or streamed while recording.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use crate::AudioData;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const HEADER_LEN: u32 = 36;

pub fn write_wav(path: &Path, data: &AudioData) -> std::io::Result<()> {
    let mut writer = WavWriter::create(path, data.channels() as u16, data.sample_rate())?;
    let mut frame = vec![0.0; data.channels()];
    for i in 0..data.frames() {
        for (ch, sample) in frame.iter_mut().enumerate() {
            *sample = data.channel(ch)[i];
        }
        writer.write_frame(&frame)?;
    }
    writer.finish().map(|_| ())
}

/// Sets the sizes of a WAV file from [`WavWriter`] that was not finished (AudioStrada stopped
/// while recording); returns its frames.
pub fn repair_wav(path: &Path) -> std::io::Result<u64> {
    use std::io::Read;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut header = [0u8; 44];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..16] != b"WAVEfmt " || &header[36..40] != b"data" {
        return Err(std::io::Error::other("not a WAV file from a recording"));
    }
    let block = u64::from(u16::from_le_bytes([header[32], header[33]]).max(1));
    let len = file.metadata()?.len();
    let frames = (len - 44) / block;
    let data_len = (frames * block).min(u64::from(u32::MAX - HEADER_LEN)) as u32;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&(HEADER_LEN + data_len).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&data_len.to_le_bytes())?;
    file.set_len(44 + u64::from(data_len))?;
    file.sync_all()?;
    Ok(frames)
}

/// Streams interleaved frames to a WAV file; sizes are written by [`finish`](Self::finish).
#[derive(Debug)]
pub struct WavWriter {
    out: BufWriter<File>,
    channels: u16,
    /// Bytes per sample.
    width: u16,
    frames: u64,
}

impl WavWriter {
    pub fn create(path: &Path, channels: u16, sample_rate: u32) -> std::io::Result<Self> {
        Self::with_format(path, channels, sample_rate, WAVE_FORMAT_IEEE_FLOAT, 32)
    }

    /// Integer PCM file of `bits` (16 or 24) per sample.
    pub fn create_pcm(
        path: &Path,
        channels: u16,
        sample_rate: u32,
        bits: u16,
    ) -> std::io::Result<Self> {
        Self::with_format(path, channels, sample_rate, WAVE_FORMAT_PCM, bits)
    }

    fn with_format(
        path: &Path,
        channels: u16,
        sample_rate: u32,
        tag: u16,
        bits: u16,
    ) -> std::io::Result<Self> {
        let mut out = BufWriter::with_capacity(1 << 16, File::create(path)?);
        let width = bits / 8;
        let block_align = channels * width;
        out.write_all(b"RIFF")?;
        out.write_all(&HEADER_LEN.to_le_bytes())?;
        out.write_all(b"WAVEfmt ")?;
        out.write_all(&16u32.to_le_bytes())?;
        out.write_all(&tag.to_le_bytes())?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&sample_rate.to_le_bytes())?;
        out.write_all(&(sample_rate * u32::from(block_align)).to_le_bytes())?;
        out.write_all(&block_align.to_le_bytes())?;
        out.write_all(&bits.to_le_bytes())?;
        out.write_all(b"data")?;
        out.write_all(&0u32.to_le_bytes())?;
        Ok(Self {
            out,
            channels,
            width,
            frames: 0,
        })
    }

    /// Writes one frame of integer samples (PCM files).
    pub fn write_pcm_frame(&mut self, samples: &[i32]) -> std::io::Result<()> {
        debug_assert_eq!(samples.len(), usize::from(self.channels));
        for s in samples {
            self.out
                .write_all(&s.to_le_bytes()[..usize::from(self.width)])?;
        }
        self.frames += 1;
        Ok(())
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Writes one frame of `channels` samples.
    pub fn write_frame(&mut self, samples: &[f32]) -> std::io::Result<()> {
        debug_assert_eq!(samples.len(), usize::from(self.channels));
        for s in samples {
            self.out.write_all(&s.to_le_bytes())?;
        }
        self.frames += 1;
        Ok(())
    }

    /// Writes the chunk sizes and closes the file; returns the number of frames.
    pub fn finish(mut self) -> std::io::Result<u64> {
        let data_len = (self.frames * u64::from(self.channels) * u64::from(self.width))
            .min(u64::from(u32::MAX - HEADER_LEN)) as u32;
        self.out.seek(SeekFrom::Start(4))?;
        self.out.write_all(&(HEADER_LEN + data_len).to_le_bytes())?;
        self.out.seek(SeekFrom::Start(40))?;
        self.out.write_all(&data_len.to_le_bytes())?;
        self.out.flush()?;
        self.out.get_ref().sync_all()?;
        Ok(self.frames)
    }
}
