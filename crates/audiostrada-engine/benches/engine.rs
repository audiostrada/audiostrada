use std::hint::black_box;
use std::sync::Arc;

use audiostrada_engine::nodes::{Clip, ClipPlayer, SoloBus, Strip};
use audiostrada_engine::{Engine, EngineConfig, EngineProcessor, Port};
use audiostrada_io::ChannelBuffers;
use audiostrada_media::AudioData;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

fn session(tracks: usize, workers: usize) -> EngineProcessor {
    let mut engine = Engine::new(EngineConfig {
        workers: Some(workers),
    });
    let solo = SoloBus::default();
    let out = engine.graph().hardware_output();
    let data = Arc::new(AudioData::new(
        vec![
            (0..RATE as usize * 10)
                .map(|i| ((i % 100) as f32 / 50.0) - 1.0)
                .collect();
            2
        ],
        RATE,
    ));
    for _ in 0..tracks {
        let (player, _controls) = ClipPlayer::new(vec![Clip::new(0, Arc::clone(&data))]);
        let player = engine.add_node("player", player, 0, 2);
        let (strip, _controls) = Strip::new(&solo, false);
        let strip = engine.add_node("strip", strip, 2, 2);
        for ch in 0..2 {
            engine
                .connect(Port::new(player, ch), Port::new(strip, ch))
                .expect("valid");
            engine
                .connect(Port::new(strip, ch), Port::new(out, ch))
                .expect("valid");
        }
    }
    engine.set_loop(Some((0, u64::from(RATE) * 9)));
    engine.play();
    engine.processor(RATE, BLOCK)
}

fn tracks(c: &mut Criterion) {
    let mut group = c.benchmark_group("render_block_256");
    for tracks in [1, 16, 64] {
        for workers in [0, 3] {
            let mut processor = session(tracks, workers);
            let mut storage = ChannelBuffers::new(0, 2, BLOCK);
            group.bench_with_input(
                BenchmarkId::new(format!("{workers}_workers"), tracks),
                &tracks,
                |b, _| b.iter(|| processor.render(black_box(&mut storage.buffers(BLOCK)))),
            );
        }
    }
    group.finish();
}

/// A mixing session: tracks with EQ and compressor, sends to a reverb bus, through the
/// project player, as a song would use the engine.
fn mix_session(
    tracks: usize,
    workers: usize,
) -> (audiostrada_engine::project::ProjectPlayer, EngineProcessor) {
    use audiostrada_core::routing::{self, Channel};
    let dir = std::env::temp_dir().join(format!("audiostrada-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("loop.wav");
    let samples: Vec<f32> = (0..RATE as usize * 10)
        .map(|i| ((i as f32) * 0.01).sin() * 0.3)
        .collect();
    audiostrada_media::write_wav(&file, &AudioData::new(vec![samples.clone(), samples], RATE))
        .expect("wav");
    let mut p = audiostrada_core::Project::new("Bench", RATE);
    let source = p.add_source(audiostrada_core::Source {
        path: file,
        channels: 2,
        sample_rate: RATE,
        frames: u64::from(RATE) * 10,
    });
    let reverb = p.add_track("Reverb");
    routing::add_insert(&mut p, Channel::Track(reverb), "reverb", Default::default());
    for i in 0..tracks {
        let t = p.add_track(&format!("T{i}"));
        p.add_clip(t, source, 0);
        routing::add_insert(&mut p, Channel::Track(t), "eq", Default::default());
        routing::add_insert(&mut p, Channel::Track(t), "compressor", Default::default());
        routing::add_send(&mut p, t, reverb).expect("send");
    }
    p.loop_range = Some((0, u64::from(RATE) * 9));
    let mut media = audiostrada_engine::media::MediaPool::new(RATE, None);
    media.retain(&audiostrada_engine::project::media_paths(&p, None));
    assert!(media.wait(std::time::Duration::from_secs(30)));
    let mut player = audiostrada_engine::project::ProjectPlayer::new(EngineConfig {
        workers: Some(workers),
    });
    let processor = player.processor(RATE, BLOCK);
    player.sync(&p, None, &media);
    player.engine_mut().play();
    (player, processor)
}

fn mix(c: &mut Criterion) {
    let mut group = c.benchmark_group("mix_block_256");
    for tracks in [8, 32] {
        for workers in [0, 3] {
            let (_player, mut processor) = mix_session(tracks, workers);
            let mut storage = ChannelBuffers::new(0, 2, BLOCK);
            group.bench_with_input(
                BenchmarkId::new(format!("{workers}_workers"), tracks),
                &tracks,
                |b, _| b.iter(|| processor.render(black_box(&mut storage.buffers(BLOCK)))),
            );
        }
    }
    group.finish();
}

criterion_group!(benches, tracks, mix);
criterion_main!(benches);
