//! Writing finished mixes: WAV (16/24-bit PCM, 32-bit float), FLAC (16/24-bit), MP3 (320 kbps,
//! by the separate `audiostrada-mp3` process) and Opus (192 kbps in Ogg), with TPDF dither when
//! reducing to integers.

use std::path::{Path, PathBuf};

use flacenc::component::BitRepr;
use flacenc::error::Verify;

use crate::{AudioData, MediaError, WavWriter};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum FileFormat {
    Wav16,
    #[default]
    Wav24,
    Wav32Float,
    Flac16,
    Flac24,
    Mp3,
    Opus,
}

impl FileFormat {
    pub const ALL: [FileFormat; 7] = [
        FileFormat::Wav16,
        FileFormat::Wav24,
        FileFormat::Wav32Float,
        FileFormat::Flac16,
        FileFormat::Flac24,
        FileFormat::Mp3,
        FileFormat::Opus,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FileFormat::Wav16 => "WAV 16-bit",
            FileFormat::Wav24 => "WAV 24-bit",
            FileFormat::Wav32Float => "WAV 32-bit float",
            FileFormat::Flac16 => "FLAC 16-bit",
            FileFormat::Flac24 => "FLAC 24-bit",
            FileFormat::Mp3 => "MP3 320 kbps",
            FileFormat::Opus => "Opus 192 kbps",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            FileFormat::Flac16 | FileFormat::Flac24 => "flac",
            FileFormat::Mp3 => "mp3",
            FileFormat::Opus => "opus",
            _ => "wav",
        }
    }

    /// Integer bit depth, or `None` for floating point.
    pub fn bits(self) -> Option<u32> {
        match self {
            FileFormat::Wav16 | FileFormat::Flac16 | FileFormat::Mp3 => Some(16),
            FileFormat::Wav24 | FileFormat::Flac24 => Some(24),
            FileFormat::Wav32Float | FileFormat::Opus => None,
        }
    }
}

/// Triangular dither generator (two uniform values per sample).
#[derive(Debug)]
pub struct Dither {
    state: u64,
}

impl Default for Dither {
    fn default() -> Self {
        Self {
            state: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl Dither {
    fn uniform(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Converts to an integer of `bits`, with ±1 LSB triangular dither when `dither` is set.
    pub fn quantize(&mut self, sample: f32, bits: u32, dither: bool) -> i32 {
        let scale = (1i64 << (bits - 1)) as f32;
        let noise = if dither {
            self.uniform() - self.uniform()
        } else {
            0.0
        };
        let value = (sample * scale + noise).round();
        value.clamp(-scale, scale - 1.0) as i32
    }
}

/// Writes `data` to `path` in `format`.
pub fn encode(
    path: &Path,
    data: &AudioData,
    format: FileFormat,
    dither: bool,
) -> Result<(), MediaError> {
    match format {
        FileFormat::Wav32Float => crate::write_wav(path, data)?,
        FileFormat::Wav16 | FileFormat::Wav24 => {
            let bits = format.bits().unwrap_or(24);
            let mut writer = WavWriter::create_pcm(
                path,
                data.channels() as u16,
                data.sample_rate(),
                bits as u16,
            )?;
            let mut d = Dither::default();
            let mut frame = vec![0i32; data.channels()];
            for i in 0..data.frames() {
                for (ch, v) in frame.iter_mut().enumerate() {
                    *v = d.quantize(data.channel(ch)[i], bits, dither);
                }
                writer.write_pcm_frame(&frame)?;
            }
            writer.finish()?;
        }
        FileFormat::Flac16 | FileFormat::Flac24 => {
            let bits = format.bits().unwrap_or(24);
            let mut d = Dither::default();
            let mut samples = Vec::with_capacity(data.frames() * data.channels());
            for i in 0..data.frames() {
                for ch in 0..data.channels() {
                    samples.push(d.quantize(data.channel(ch)[i], bits, dither));
                }
            }
            let config = flacenc::config::Encoder::default()
                .into_verified()
                .map_err(|e| MediaError::Unsupported(format!("{e:?}")))?;
            let source = flacenc::source::MemSource::from_samples(
                &samples,
                data.channels(),
                bits as usize,
                data.sample_rate() as usize,
            );
            let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
                .map_err(|e| MediaError::Unsupported(format!("FLAC encoding failed: {e:?}")))?;
            let mut sink = flacenc::bitsink::ByteSink::new();
            stream
                .write(&mut sink)
                .map_err(|e| MediaError::Unsupported(format!("FLAC encoding failed: {e:?}")))?;
            // The minimum block size excludes the last block (fixed-size stream); some decoders
            // need it right to find the frames.
            let mut bytes = sink.as_slice().to_vec();
            if bytes.len() > 12 && &bytes[..4] == b"fLaC" {
                bytes[8] = bytes[10];
                bytes[9] = bytes[11];
            }
            std::fs::write(path, bytes)?;
        }
        FileFormat::Mp3 => std::fs::write(path, mp3(data, dither)?)?,
        FileFormat::Opus => std::fs::write(path, opus(data)?)?,
    }
    Ok(())
}

/// At most two channels: the first two, or the mono channel twice when `stereo`.
fn channels(data: &AudioData, stereo: bool) -> Vec<&[f32]> {
    match data.channels() {
        0 => Vec::new(),
        1 if stereo => vec![data.channel(0), data.channel(0)],
        1 => vec![data.channel(0)],
        _ => vec![data.channel(0), data.channel(1)],
    }
}

fn mp3(data: &AudioData, dither: bool) -> Result<Vec<u8>, MediaError> {
    // MPEG-1 layer III rates; others are resampled.
    let resampled;
    let data = if [32_000, 44_100, 48_000].contains(&data.sample_rate()) {
        data
    } else {
        resampled = crate::resample(data, 48_000)?;
        &resampled
    };
    let chans = channels(data, true);
    let mut d = Dither::default();
    let mut pcm = Vec::with_capacity(data.frames() * 2);
    for i in 0..data.frames() {
        for c in &chans {
            pcm.push(d.quantize(c[i], 16, dither) as i16);
        }
    }
    let mut bytes = Vec::with_capacity(pcm.len() * 2);
    for s in pcm {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    run_mp3_encoder(data.sample_rate(), bytes)
}

static MP3_ENCODER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Overrides where the `audiostrada-mp3` helper is looked for.
pub fn set_mp3_encoder(path: PathBuf) {
    let _ = MP3_ENCODER.set(path);
}

/// `audiostrada-mp3` next to the running program, else on the PATH.
fn mp3_encoder() -> PathBuf {
    if let Some(path) = MP3_ENCODER.get() {
        return path.clone();
    }
    let name = format!("audiostrada-mp3{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// MP3 encoding runs in a separate process: the LGPL encoder is not linked into AudioStrada.
fn run_mp3_encoder(rate: u32, pcm: Vec<u8>) -> Result<Vec<u8>, MediaError> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let failed = |e: String| MediaError::Unsupported(format!("MP3 encoding failed: {e}"));
    let program = mp3_encoder();
    let mut command = Command::new(&program);
    command
        .arg(rate.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| failed(format!("{}: {e}", program.display())))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| failed("no input".into()))?;
    let writer = std::thread::spawn(move || stdin.write_all(&pcm));
    let output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| failed("writer panicked".into()))??;
    if !output.status.success() {
        return Err(failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(output.stdout)
}

/// Opus in an Ogg stream (RFC 7845): header and tags pages, then 20 ms packets.
fn opus(data: &AudioData) -> Result<Vec<u8>, MediaError> {
    const RATE: u32 = 48_000;
    const FRAME: usize = 960;
    const PRE_SKIP: u16 = 312;
    let original_rate = data.sample_rate();
    let resampled;
    let data = if data.sample_rate() == RATE {
        data
    } else {
        resampled = crate::resample(data, RATE)?;
        &resampled
    };
    let chans = channels(data, false);
    let count = chans.len().max(1);
    let failed = |e: &str| MediaError::Unsupported(format!("Opus encoding failed: {e}"));
    let mut encoder = opus_rs::OpusEncoder::new(RATE as i32, count, opus_rs::Application::Audio)
        .map_err(failed)?;
    encoder.bitrate_bps = 192_000;
    let mut writer = ogg::PacketWriter::new(Vec::new());
    let serial = 0x5111_0001;
    let mut head = b"OpusHead".to_vec();
    head.push(1);
    head.push(count as u8);
    head.extend_from_slice(&PRE_SKIP.to_le_bytes());
    head.extend_from_slice(&original_rate.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes());
    head.push(0);
    let mut tags = b"OpusTags".to_vec();
    let vendor = b"AudioStrada";
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    let io = |e: std::io::Error| MediaError::Io(e);
    writer
        .write_packet(head, serial, ogg::PacketWriteEndInfo::EndPage, 0)
        .map_err(io)?;
    writer
        .write_packet(tags, serial, ogg::PacketWriteEndInfo::EndPage, 0)
        .map_err(io)?;
    // Padding at the end flushes the encoder's lookahead.
    let total = data.frames() + usize::from(PRE_SKIP);
    let frames = total.div_ceil(FRAME).max(1);
    let mut input = vec![0.0f32; FRAME * count];
    let mut packet = vec![0u8; 4000];
    for f in 0..frames {
        for i in 0..FRAME {
            let at = f * FRAME + i;
            for (c, samples) in chans.iter().enumerate() {
                input[i * count + c] = samples.get(at).copied().unwrap_or(0.0);
            }
        }
        let size = encoder.encode(&input, FRAME, &mut packet).map_err(failed)?;
        let last = f + 1 == frames;
        let granule = if last {
            total as u64
        } else {
            ((f + 1) * FRAME) as u64
        };
        let end = if last {
            ogg::PacketWriteEndInfo::EndStream
        } else {
            ogg::PacketWriteEndInfo::NormalPacket
        };
        writer
            .write_packet(packet[..size].to_vec(), serial, end, granule)
            .map_err(io)?;
    }
    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f32) -> AudioData {
        let n = (rate as f32 * seconds) as usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect();
        AudioData::new(vec![samples.clone(), samples], rate)
    }

    fn crossings(x: &[f32], rate: u32) -> f32 {
        let part = &x[x.len() / 4..3 * x.len() / 4];
        let n = part
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        n as f32 * rate as f32 / part.len() as f32
    }

    #[test]
    fn formats_describe_themselves() {
        for format in FileFormat::ALL {
            let integer = format.bits().is_some();
            assert_eq!(
                integer,
                !matches!(format, FileFormat::Wav32Float | FileFormat::Opus),
                "{format:?}"
            );
        }
        assert_eq!(FileFormat::Flac24.extension(), "flac");
        assert_eq!(FileFormat::Wav32Float.extension(), "wav");
        assert_eq!(FileFormat::Opus.extension(), "opus");
        assert_eq!(FileFormat::default(), FileFormat::Wav24);
    }

    #[test]
    fn quantize_rounds_clamps_and_dithers() {
        let mut d = Dither::default();
        assert_eq!(d.quantize(0.5, 16, false), 16_384);
        assert_eq!(d.quantize(1.0, 16, false), 32_767);
        assert_eq!(d.quantize(-2.0, 16, false), -32_768);
        assert_eq!(d.quantize(-1.0, 24, false), -8_388_608);
        let dithered: Vec<i32> = (0..1000).map(|_| d.quantize(0.0, 16, true)).collect();
        assert!(dithered.iter().all(|v| v.abs() <= 1), "at most one LSB");
        assert!(dithered.iter().any(|&v| v != 0), "noise is added");
    }

    #[test]
    fn at_most_two_channels_are_encoded() {
        let mono = AudioData::new(vec![vec![0.1; 4]], 48_000);
        assert_eq!(channels(&mono, true).len(), 2);
        assert_eq!(channels(&mono, false).len(), 1);
        let quad = AudioData::new(vec![vec![0.0; 4]; 4], 48_000);
        assert_eq!(channels(&quad, false).len(), 2);
        assert!(channels(&AudioData::new(Vec::new(), 48_000), true).is_empty());
    }

    #[test]
    fn lossy_formats_decode_back() {
        let dir = audiostrada_core::scratch::dir("lossy");
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = opus(&tone(44_100, 1.0)).unwrap();
        let mut reader = ogg::PacketReader::new(std::io::Cursor::new(bytes));
        let head = reader.read_packet().unwrap().unwrap();
        assert_eq!(&head.data[..8], b"OpusHead");
        assert_eq!(head.data[9], 2, "stereo");
        let tags = reader.read_packet().unwrap().unwrap();
        assert_eq!(&tags.data[..8], b"OpusTags");
        let mut decoder = opus_rs::OpusDecoder::new(48_000, 2).unwrap();
        let mut left = Vec::new();
        let mut frame = vec![0.0f32; 960 * 2];
        while let Some(packet) = reader.read_packet().unwrap() {
            let n = decoder.decode(&packet.data, 960, &mut frame).unwrap();
            left.extend(frame[..n * 2].iter().step_by(2));
        }
        assert!(
            (left.len() as f32 / 48_000.0 - 1.0).abs() < 0.05,
            "{}",
            left.len()
        );
        assert!((crossings(&left[312..], 48_000) - 440.0).abs() < 5.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
