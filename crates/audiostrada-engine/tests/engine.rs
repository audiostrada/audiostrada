#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use audiostrada_engine::nodes::{Clip, ClipPlayer, SoloBus, Strip};
use audiostrada_engine::{Engine, EngineConfig, EngineProcessor, Port, ProcessContext, Processor};
use audiostrada_io::ChannelBuffers;
use audiostrada_media::AudioData;
use audiostrada_rt::{alloc_violations, rt_section};

audiostrada_rt::install_alloc_checker!();

const RATE: u32 = 48_000;
const BLOCK: usize = 64;

fn engine(workers: usize) -> Engine {
    Engine::new(EngineConfig {
        workers: Some(workers),
    })
}

/// Renders `blocks` blocks and returns the output channels concatenated.
fn render(processor: &mut EngineProcessor, outputs: usize, blocks: usize) -> Vec<Vec<f32>> {
    let mut storage = ChannelBuffers::new(0, outputs, BLOCK);
    let mut out = vec![Vec::new(); outputs];
    for _ in 0..blocks {
        processor.render(&mut storage.buffers(BLOCK));
        for (ch, channel) in out.iter_mut().enumerate() {
            channel.extend_from_slice(&storage.output(ch)[..BLOCK]);
        }
    }
    out
}

fn ramp(frames: usize) -> Arc<AudioData> {
    Arc::new(AudioData::new(
        vec![(0..frames).map(|i| i as f32 / frames as f32).collect()],
        RATE,
    ))
}

/// Delays its single channel by a fixed number of samples (a stand-in for a latent plugin).
struct Delay {
    line: Vec<f32>,
    pos: usize,
}

impl Delay {
    fn new(samples: usize) -> Self {
        Self {
            line: vec![0.0; samples],
            pos: 0,
        }
    }
}

impl Processor for Delay {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let (src, dst) = ctx.buffers.io(0, 0);
        for (d, &s) in dst.iter_mut().zip(src) {
            *d = self.line[self.pos];
            self.line[self.pos] = s;
            self.pos = (self.pos + 1) % self.line.len();
        }
    }
}

/// Outputs a running sample counter, to observe processor state across graph changes.
struct Counter {
    count: Arc<AtomicU64>,
}

impl Processor for Counter {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let frames = ctx.buffers.frames();
        let start = self.count.fetch_add(frames as u64, Ordering::Relaxed);
        for (i, s) in ctx.buffers.output(0).iter_mut().enumerate() {
            *s = (start + i as u64) as f32;
        }
    }
}

#[test]
fn playback_is_bit_exact() {
    let mut engine = engine(0);
    let data = ramp(1000);
    let (player, _controls) = ClipPlayer::new(vec![Clip::new(100, Arc::clone(&data))]);
    let player = engine.add_node("player", player, 0, 2);
    let out = engine.graph().hardware_output();
    engine
        .connect(Port::new(player, 0), Port::new(out, 0))
        .unwrap();
    engine
        .connect(Port::new(player, 1), Port::new(out, 1))
        .unwrap();
    engine.play();
    let mut processor = engine.processor(RATE, BLOCK);
    let rendered = render(&mut processor, 2, 20);
    for channel in &rendered {
        assert!(channel[..100].iter().all(|&s| s == 0.0));
        assert_eq!(&channel[100..1100], data.channel(0));
        assert!(channel[1100..].iter().all(|&s| s == 0.0));
    }
    assert_eq!(engine.shared().position(), 20 * BLOCK as u64);
}

#[test]
fn stopped_transport_is_silent() {
    let mut engine = engine(0);
    let (player, _controls) = ClipPlayer::new(vec![Clip::new(0, ramp(500))]);
    let player = engine.add_node("player", player, 0, 1);
    let out = engine.graph().hardware_output();
    engine
        .connect(Port::new(player, 0), Port::new(out, 0))
        .unwrap();
    let mut processor = engine.processor(RATE, BLOCK);
    assert!(render(&mut processor, 1, 4)[0].iter().all(|&s| s == 0.0));
    assert_eq!(engine.shared().position(), 0);
}

#[test]
fn latency_is_compensated_on_parallel_paths() {
    const LATENCY: usize = 100;
    let mut engine = engine(0);
    let (player, _controls) = ClipPlayer::new(vec![Clip::new(0, ramp(2000))]);
    let player = engine.add_node("player", player, 0, 1);
    let delay = engine.add_node("latent plugin", Delay::new(LATENCY), 1, 1);
    engine.set_latency(delay, LATENCY as u32).unwrap();
    let out = engine.graph().hardware_output();
    engine
        .connect(Port::new(player, 0), Port::new(delay, 0))
        .unwrap();
    engine
        .connect(Port::new(delay, 0), Port::new(out, 0))
        .unwrap();
    // Dry path straight to the output: must be delayed to stay aligned with the latent one.
    engine
        .connect(Port::new(player, 0), Port::new(out, 0))
        .unwrap();
    engine.play();
    let mut processor = engine.processor(RATE, BLOCK);
    let rendered = &render(&mut processor, 1, 40)[0];
    let source = ramp(2000);
    for (i, &s) in rendered.iter().enumerate().take(2000 + LATENCY) {
        let expected = if i < LATENCY {
            0.0
        } else {
            2.0 * source.channel(0)[i - LATENCY]
        };
        assert_eq!(s, expected, "frame {i}");
    }
    assert_eq!(engine.shared().latency(), LATENCY as u32);
}

#[test]
fn graph_changes_keep_running_processors() {
    let mut engine = engine(0);
    let count = Arc::new(AtomicU64::new(0));
    let counter = engine.add_node(
        "counter",
        Counter {
            count: Arc::clone(&count),
        },
        0,
        1,
    );
    let out = engine.graph().hardware_output();
    engine
        .connect(Port::new(counter, 0), Port::new(out, 0))
        .unwrap();
    let mut processor = engine.processor(RATE, BLOCK);
    render(&mut processor, 1, 3);

    let extra = engine.add_node("silence", |_: &mut ProcessContext<'_>| {}, 0, 1);
    engine
        .connect(Port::new(extra, 0), Port::new(out, 0))
        .unwrap();
    engine.commit();
    let rendered = &render(&mut processor, 1, 1)[0];
    assert_eq!(
        rendered[0],
        (3 * BLOCK) as f32,
        "counter state survives the new schedule"
    );

    engine.remove_node(extra).unwrap();
    engine.commit();
    render(&mut processor, 1, 1);
    engine.collect_garbage();
    assert_eq!(count.load(Ordering::Relaxed), 5 * BLOCK as u64);
}

#[test]
fn processors_come_back_when_the_stream_closes() {
    let mut engine = engine(0);
    let count = Arc::new(AtomicU64::new(0));
    let counter = engine.add_node(
        "counter",
        Counter {
            count: Arc::clone(&count),
        },
        0,
        1,
    );
    let out = engine.graph().hardware_output();
    engine
        .connect(Port::new(counter, 0), Port::new(out, 0))
        .unwrap();
    let mut processor = engine.processor(RATE, BLOCK);
    render(&mut processor, 1, 2);
    drop(processor);

    let mut processor = engine.processor(44_100, BLOCK);
    let rendered = &render(&mut processor, 1, 1)[0];
    assert_eq!(rendered[0], (2 * BLOCK) as f32);
    assert_eq!(engine.sample_rate(), 44_100);
}

/// A wide graph: many players through strips, summed on the output.
fn mix(workers: usize) -> Vec<Vec<f32>> {
    let mut engine = engine(workers);
    let solo = SoloBus::default();
    let out = engine.graph().hardware_output();
    for track in 0..16 {
        let data = Arc::new(AudioData::new(
            vec![
                (0..4000)
                    .map(|i| ((i * (track + 1)) % 97) as f32 / 97.0 - 0.5)
                    .collect(),
            ],
            RATE,
        ));
        let (player, _controls) = ClipPlayer::new(vec![Clip::new(track as u64 * 10, data)]);
        let player = engine.add_node("player", player, 0, 1);
        let (strip, controls) = Strip::new(&solo, false);
        controls.gain_db.set(-(track as f32));
        controls.pan.set(track as f32 / 8.0 - 1.0);
        let strip = engine.add_node("strip", strip, 1, 2);
        engine
            .connect(Port::new(player, 0), Port::new(strip, 0))
            .unwrap();
        engine
            .connect(Port::new(strip, 0), Port::new(out, 0))
            .unwrap();
        engine
            .connect(Port::new(strip, 1), Port::new(out, 1))
            .unwrap();
    }
    engine.play();
    let mut processor = engine.processor(RATE, BLOCK);
    render(&mut processor, 2, 80)
}

#[test]
fn parallel_rendering_matches_serial() {
    let serial = mix(0);
    assert!(serial[0].iter().any(|&s| s != 0.0));
    for workers in [1, 3, 7] {
        assert_eq!(mix(workers), serial, "{workers} workers");
    }
}

#[test]
fn steady_state_rendering_does_not_allocate() {
    for workers in [0, 3] {
        let mut engine = engine(workers);
        let solo = SoloBus::default();
        let out = engine.graph().hardware_output();
        for _ in 0..8 {
            let (player, _controls) = ClipPlayer::new(vec![Clip::new(0, ramp(10_000))]);
            let player = engine.add_node("player", player, 0, 1);
            let (strip, _controls) = Strip::new(&solo, false);
            let strip = engine.add_node("strip", strip, 1, 2);
            engine
                .connect(Port::new(player, 0), Port::new(strip, 0))
                .unwrap();
            engine
                .connect(Port::new(strip, 0), Port::new(out, 0))
                .unwrap();
        }
        engine.play();
        engine.set_loop(Some((0, 5_000)));
        let mut processor = engine.processor(RATE, BLOCK);
        let mut storage = ChannelBuffers::new(0, 2, BLOCK);
        let before = alloc_violations();
        rt_section(|| {
            for _ in 0..200 {
                processor.render(&mut storage.buffers(BLOCK));
            }
        });
        assert_eq!(alloc_violations(), before, "{workers} workers");
    }
}

#[test]
fn loop_and_locate() {
    let mut engine = engine(0);
    let data = ramp(1000);
    let (player, _controls) = ClipPlayer::new(vec![Clip::new(0, Arc::clone(&data))]);
    let player = engine.add_node("player", player, 0, 1);
    let out = engine.graph().hardware_output();
    engine
        .connect(Port::new(player, 0), Port::new(out, 0))
        .unwrap();
    engine.set_loop(Some((100, 200)));
    engine.locate(150);
    engine.play();
    let mut processor = engine.processor(RATE, BLOCK);
    let rendered = &render(&mut processor, 1, 4)[0];
    let expected: Vec<f32> = (0..4 * BLOCK)
        .map(|i| data.channel(0)[100 + (50 + i) % 100])
        .collect();
    assert_eq!(rendered, &expected);

    engine.set_loop(None);
    engine.locate(900);
    let rendered = &render(&mut processor, 1, 1)[0];
    assert_eq!(&rendered[..], &data.channel(0)[900..900 + BLOCK]);
}

#[test]
fn mute_and_solo() {
    let mut engine = engine(0);
    let solo = SoloBus::default();
    let out = engine.graph().hardware_output();
    let data = ramp(10_000);
    let mut controls = Vec::new();
    for _ in 0..2 {
        let (player, _p) = ClipPlayer::new(vec![Clip::new(0, Arc::clone(&data))]);
        let player = engine.add_node("player", player, 0, 1);
        let (strip, c) = Strip::new(&solo, false);
        let strip = engine.add_node("strip", strip, 1, 2);
        engine
            .connect(Port::new(player, 0), Port::new(strip, 0))
            .unwrap();
        engine
            .connect(Port::new(strip, 0), Port::new(out, 0))
            .unwrap();
        controls.push(c);
    }
    engine.play();
    let mut processor = engine.processor(RATE, BLOCK);
    let close = |block: &[f32], start: usize, factor: f32| {
        block
            .iter()
            .zip(&data.channel(0)[start..start + BLOCK])
            .all(|(o, s)| (o - s * factor).abs() < 1e-5)
    };
    let blocks = render(&mut processor, 1, 2);
    assert!(close(&blocks[0][BLOCK..], BLOCK, 2.0), "both tracks");

    controls[0].set_solo(true);
    let blocks = render(&mut processor, 1, 2);
    assert!(
        close(&blocks[0][BLOCK..], 3 * BLOCK, 1.0),
        "only the soloed track"
    );

    controls[0].set_mute(true);
    let blocks = render(&mut processor, 1, 2);
    assert!(
        blocks[0][BLOCK..].iter().all(|&s| s == 0.0),
        "soloed track muted"
    );
}

#[test]
fn rate_change_keeps_the_time_position() {
    let mut engine = engine(0);
    engine.set_loop(Some((48_000, 96_000)));
    engine.locate(48_000);
    engine.play();
    let mut processor = engine.processor(48_000, BLOCK);
    render(&mut processor, 1, 500);
    assert_eq!(engine.shared().position(), 48_000 + 500 * BLOCK as u64);
    drop(processor);

    let processor = engine.processor(96_000, BLOCK);
    assert_eq!(engine.position(), 2 * (48_000 + 500 * BLOCK as u64));
    assert_eq!(engine.looping(), Some((96_000, 192_000)));
    drop(processor);
}

/// A node taking the given number of microseconds every block.
struct Busy(Arc<AtomicU64>);

impl Processor for Busy {
    fn process(&mut self, _ctx: &mut ProcessContext<'_>) {
        let cost = std::time::Duration::from_micros(self.0.load(Ordering::Relaxed));
        let started = std::time::Instant::now();
        while started.elapsed() < cost {
            std::hint::spin_loop();
        }
    }
}

#[test]
fn workers_join_heavy_graphs_only() {
    if std::thread::available_parallelism().map_or(1, |n| n.get()) < 2 {
        return;
    }
    let mut engine = Engine::new(EngineConfig::default());
    let cost = Arc::new(AtomicU64::new(0));
    let out = engine.graph().hardware_output();
    for _ in 0..8 {
        let node = engine.add_node("busy", Busy(Arc::clone(&cost)), 0, 1);
        engine
            .connect(Port::new(node, 0), Port::new(out, 0))
            .unwrap();
    }
    let mut processor = engine.processor(RATE, BLOCK);
    render(&mut processor, 1, 20);
    assert!(
        !processor.parallel(),
        "a light graph stays on the audio thread"
    );
    // 8 nodes of 200 µs: more than the 1.3 ms of a block.
    cost.store(200, Ordering::Relaxed);
    render(&mut processor, 1, 5);
    assert!(processor.parallel(), "a heavy one takes the workers");
    cost.store(0, Ordering::Relaxed);
    render(&mut processor, 1, 5);
    assert!(!processor.parallel(), "and lets them go");
}
