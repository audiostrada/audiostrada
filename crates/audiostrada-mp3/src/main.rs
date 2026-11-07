//! `audiostrada-mp3 RATE`: reads 16-bit stereo interleaved PCM (little endian) on standard
//! input and writes MP3 at 320 kbps on standard output. Kept out of the AudioStrada binary,
//! which links no LGPL code.

use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(rate) = std::env::args().nth(1).and_then(|r| r.parse::<u32>().ok()) else {
        eprintln!("usage: audiostrada-mp3 RATE < pcm > mp3");
        return ExitCode::from(2);
    };
    let mut bytes = Vec::new();
    if let Err(e) = std::io::stdin().lock().read_to_end(&mut bytes) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    let pcm: Vec<i16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect();
    let config = shine_rs::Mp3EncoderConfig::new()
        .sample_rate(rate)
        .bitrate(320)
        .channels(2)
        .stereo_mode(shine_rs::StereoMode::Stereo);
    match shine_rs::encode_pcm_to_mp3(config, &pcm) {
        Ok(mp3) => match std::io::stdout().lock().write_all(&mp3) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
