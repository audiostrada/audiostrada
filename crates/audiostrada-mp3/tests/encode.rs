use audiostrada_media::{AudioData, FileFormat};

#[test]
fn audiostrada_exports_mp3_through_the_helper() {
    audiostrada_media::set_mp3_encoder(env!("CARGO_BIN_EXE_audiostrada-mp3").into());
    let rate = 44_100;
    let tone: Vec<f32> = (0..rate * 2)
        .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
        .collect();
    let data = AudioData::new(vec![tone.clone(), tone], rate as u32);
    let dir = std::env::temp_dir().join(format!("audiostrada-mp3-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone.mp3");
    audiostrada_media::encode(&path, &data, FileFormat::Mp3, true).unwrap();
    let back = audiostrada_media::load(&path).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(back.channels(), 2);
    assert!((back.frames() as f32 / rate as f32 - 2.0).abs() < 0.1);
    let x = &back.channel(0)[back.frames() / 4..3 * back.frames() / 4];
    let up = x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    let hz = up as f32 * rate as f32 / x.len() as f32;
    assert!((hz - 440.0).abs() < 5.0, "{hz}");
}
