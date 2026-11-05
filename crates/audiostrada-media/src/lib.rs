//! AudioStrada media decoding, encoding, resampling and peak files.

mod data;
mod decode;
mod encode;
mod peaks;
mod resample;
mod wav;

use std::path::Path;

pub use data::AudioData;
pub use decode::load;
pub use encode::{Dither, FileFormat, encode};
pub use peaks::{Peak, Peaks};
pub use resample::resample;
pub use wav::{WavWriter, write_wav};

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("unsupported file: {0}")]
    Unsupported(String),
    #[error("decoding failed: {0}")]
    Decode(String),
    #[error("resampling failed: {0}")]
    Resample(String),
}

/// Decodes a file and converts it to `rate`.
pub fn load_at(path: &Path, rate: u32) -> Result<AudioData, MediaError> {
    resample(&load(path)?, rate)
}
