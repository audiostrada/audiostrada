//! Decoding of audio files into memory with Symphonia.

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::{AudioData, MediaError};

/// Decodes a whole file at its native sample rate.
pub fn load(path: &Path) -> Result<AudioData, MediaError> {
    let file = File::open(path)?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| MediaError::Unsupported("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| MediaError::Unsupported("missing codec parameters".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut packet_planes: Vec<Vec<f32>> = Vec::new();
    let mut sample_rate = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(SymphoniaError::ResetRequired) => break,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(MediaError::Decode(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(e)) => {
                tracing::debug!("skipping corrupt packet in {}: {e}", path.display());
                continue;
            }
            Err(e) => return Err(MediaError::Decode(e.to_string())),
        };
        sample_rate = decoded.spec().rate();
        decoded.copy_to_vecs_planar(&mut packet_planes);
        channels.resize_with(packet_planes.len(), Vec::new);
        for (channel, plane) in channels.iter_mut().zip(&packet_planes) {
            channel.extend_from_slice(plane);
        }
    }
    if channels.is_empty() || sample_rate == 0 {
        return Err(MediaError::Decode("no audio decoded".into()));
    }
    Ok(AudioData::new(channels, sample_rate))
}
