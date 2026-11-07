#![allow(clippy::unwrap_used)]

use std::f32::consts::TAU;

use audiostrada_media::{AudioData, load, load_at, resample, write_wav};

fn sine(frequency: f32, rate: u32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (i as f32 * frequency * TAU / rate as f32).sin() * 0.5)
        .collect()
}

fn temp(name: &str) -> std::path::PathBuf {
    audiostrada_core::scratch::dir(&format!("media-{name}"))
}

#[test]
fn wav_round_trip_is_bit_exact() {
    let path = temp("round-trip.wav");
    let data = AudioData::new(
        vec![sine(440.0, 44_100, 10_000), sine(880.0, 44_100, 10_000)],
        44_100,
    );
    write_wav(&path, &data).unwrap();
    let back = load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(back, data);
}

#[test]
fn resampling_keeps_duration_and_level() {
    let data = AudioData::new(vec![sine(1000.0, 44_100, 44_100)], 44_100);
    let out = resample(&data, 48_000).unwrap();
    assert_eq!(out.sample_rate(), 48_000);
    assert!(
        (out.frames() as i64 - 48_000).abs() <= 2,
        "{} frames",
        out.frames()
    );
    let middle = &out.channel(0)[4_000..44_000];
    let peak = middle.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!((peak - 0.5).abs() < 0.01, "peak {peak}");
}

#[test]
fn load_at_converts_rate() {
    let path = temp("rate.wav");
    write_wav(
        &path,
        &AudioData::new(vec![sine(440.0, 96_000, 9_600)], 96_000),
    )
    .unwrap();
    let out = load_at(&path, 48_000).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(out.sample_rate(), 48_000);
    assert!((out.frames() as i64 - 4_800).abs() <= 2);
}

#[test]
fn missing_and_invalid_files_are_errors() {
    assert!(load(std::path::Path::new("/nonexistent/file.wav")).is_err());
    let path = temp("garbage.wav");
    std::fs::write(&path, b"not audio at all").unwrap();
    assert!(load(&path).is_err());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn streamed_wav_matches_written_frames() {
    let path = temp("stream.wav");
    let mut writer = audiostrada_media::WavWriter::create(&path, 2, 48_000).unwrap();
    for i in 0..1000 {
        writer.write_frame(&[i as f32 / 1000.0, -0.5]).unwrap();
    }
    assert_eq!(writer.finish().unwrap(), 1000);
    let back = load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(back.frames(), 1000);
    assert_eq!(back.channel(0)[500], 0.5);
    assert_eq!(back.channel(1)[999], -0.5);
}

#[test]
fn exports_decode_back_within_quantisation() {
    use audiostrada_media::{FileFormat, encode};
    let data = AudioData::new(
        vec![sine(440.0, 48_000, 96_000), sine(660.0, 48_000, 96_000)],
        48_000,
    );
    // Lossy formats are checked by their own unit test.
    for format in FileFormat::ALL
        .into_iter()
        .filter(|f| !matches!(f, FileFormat::Mp3 | FileFormat::Opus))
    {
        let path = temp(&format!("export.{}", format.extension()));
        encode(&path, &data, format, true).unwrap();
        let back = load(&path).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(back.frames(), 96_000, "{format:?}");
        assert_eq!(back.channels(), 2);
        let tolerance = match format.bits() {
            Some(16) => 2.0 / 32768.0,
            Some(_) => 2.0 / 8_388_608.0,
            None => 0.0,
        };
        let worst = (0..4800)
            .map(|i| (back.channel(1)[i] - data.channel(1)[i]).abs())
            .fold(0.0, f32::max);
        assert!(worst <= tolerance, "{format:?}: {worst}");
    }
}
