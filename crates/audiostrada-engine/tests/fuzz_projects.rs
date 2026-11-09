//! Damaged project files must be rejected or played, never crash: random mutations of a rich
//! project, as bytes and as JSON values, go through the loader, editing operations and the
//! engine. AUDIOSTRADA_FUZZ_CASES sets the number of cases (default 2000)
//! and AUDIOSTRADA_FUZZ_SEED another sequence.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use audiostrada_core::automation::{AutomationTarget, CurveShape, lane_mut};
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{Note, Project, Source, edit, file};
use audiostrada_engine::EngineConfig;
use audiostrada_engine::media::MediaPool;
use audiostrada_engine::project::ProjectPlayer;
use audiostrada_io::ChannelBuffers;
use serde_json::Value;

/// Small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn seed_project() -> Project {
    let mut p = Project::new("Fuzz", 48_000);
    let s = p.add_source(Source {
        path: "missing.wav".into(),
        channels: 2,
        sample_rate: 44_100,
        frames: 441_000,
    });
    let drums = routing::add_bus(&mut p, "Drums");
    let kick = p.add_track("Kick");
    let bus = p.add_track("Bus");
    let keys = p.add_track("Keys");
    let vca = routing::add_vca(&mut p, "VCA");
    let clip = p.add_clip(kick, s, 1000).unwrap();
    edit::set_fades(&mut p, clip, 500, 800);
    edit::stretch_clip(&mut p, clip, 300_000);
    routing::set_parent(&mut p, kick, Some(drums)).unwrap();
    routing::add_send(&mut p, kick, bus).unwrap();
    routing::set_vca(&mut p, drums, Some(vca)).unwrap();
    let eq = routing::add_insert(&mut p, Channel::Track(kick), "eq", Default::default()).unwrap();
    routing::add_insert(&mut p, Channel::Master, "compressor", Default::default());
    routing::add_insert(&mut p, Channel::Track(keys), "synth", Default::default());
    routing::add_insert(
        &mut p,
        Channel::Track(bus),
        "clap:org.missing",
        Default::default(),
    );
    p.add_midi_clip(
        keys,
        0,
        96_000,
        vec![Note {
            start: 100,
            length: 9000,
            key: 60,
            velocity: 100,
            channel: 0,
            ..Default::default()
        }],
    );
    let t = p.track_mut(kick).unwrap();
    let lane = lane_mut(t, &AutomationTarget::Gain);
    lane.add(0, -10.0, CurveShape::Linear);
    lane.add(48_000, 0.0, CurveShape::Smooth);
    let lane = lane_mut(
        t,
        &AutomationTarget::Param {
            insert: eq,
            param: "b1_gain".into(),
        },
    );
    lane.add(1000, 6.0, CurveShape::Step);
    edit::add_marker(&mut p, 24_000);
    p.loop_range = Some((0, 96_000));
    p.punch_range = Some((10_000, 20_000));
    p.metronome.enabled = true;
    p
}

/// A value that tends to break assumptions.
fn nasty(rng: &mut Rng) -> Value {
    match rng.below(12) {
        0 => Value::from(0),
        1 => Value::from(-1),
        2 => Value::from(u64::MAX),
        3 => Value::from(i64::MIN),
        4 => Value::from(1e300),
        5 => Value::from(-1e300),
        6 => Value::from(0.5),
        7 => Value::Null,
        8 => Value::from(""),
        9 => Value::Array(Vec::new()),
        10 => Value::from(4_294_967_296u64),
        _ => Value::from(true),
    }
}

/// Visits a random node and replaces, removes or duplicates it.
fn mutate(value: &mut Value, rng: &mut Rng, depth: usize) {
    let stop = depth > 2 && rng.below(4) == 0;
    match value {
        Value::Object(map) if !map.is_empty() && !stop => {
            let key = map.keys().nth(rng.below(map.len())).cloned().unwrap();
            if rng.below(8) == 0 {
                map.remove(&key);
            } else if let Some(child) = map.get_mut(&key) {
                mutate(child, rng, depth + 1);
            }
        }
        Value::Array(items) if !items.is_empty() && !stop => {
            let i = rng.below(items.len());
            match rng.below(8) {
                0 => {
                    items.remove(i);
                }
                1 => {
                    let copy = items[i].clone();
                    items.push(copy);
                }
                _ => mutate(&mut items[i], rng, depth + 1),
            }
        }
        // Mostly keep the type, so that deeper code sees the damage.
        Value::Number(_) if rng.below(10) > 0 => {
            *value = match rng.below(9) {
                0 => Value::from(0),
                1 => Value::from(1),
                2 => Value::from(u32::MAX),
                3 => Value::from(u64::MAX),
                4 => Value::from(1e300),
                5 => Value::from(-1e300),
                6 => Value::from(0.000_001),
                7 => Value::from(-3),
                _ => Value::from(rng.next() % 1_000_000),
            }
        }
        Value::String(_) if rng.below(10) > 0 => {
            *value = Value::from(["", "x", "clap:zz", "eq", "../../etc", "nam"][rng.below(6)]);
        }
        Value::Bool(b) if rng.below(10) > 0 => *b = !*b,
        _ => *value = nasty(rng),
    }
}

/// Exercises a loaded project: editing helpers and a short playback.
fn exercise(project: &Project, rng: &mut Rng) {
    let _ = project.length();
    let _ = routing::visible_tracks(project);
    let _ = routing::solo_audible(project);
    let _ = audiostrada_engine::project::media_paths(project, None);
    for t in &project.tracks {
        let _ = routing::effective_gain(project, t.id);
    }
    let rate = project.sample_rate;
    for s in [0, 1000, 48_000, u64::MAX / 4] {
        let beat = project.tempo.sample_to_beat(s, rate);
        let _ = project.tempo.beat_to_sample(beat, rate);
        let _ = project.tempo.beat_to_bbt(beat);
    }
    let mut copy = project.clone();
    let ids: Vec<_> = copy
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect();
    if !ids.is_empty() {
        let at = rng.next() % 200_000;
        edit::split(&mut copy, at, &ids);
        edit::move_clips(&mut copy, &ids, (rng.next() % 1000) as i64 - 500, 0);
        edit::stretch_clip(&mut copy, ids[0], rng.next() % 500_000);
        edit::duplicate(&mut copy, &ids);
        if let Some(t) = copy.tracks.first().map(|t| t.id) {
            edit::duplicate_track(&mut copy, t);
        }
        edit::delete_clips(&mut copy, &ids, true);
    }

    let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
    let media = MediaPool::new(48_000, None);
    let mut processor = player.processor(48_000, 128);
    let mut storage = ChannelBuffers::new(0, 2, 128);
    player.sync(project, None, &media);
    player.engine_mut().play();
    for _ in 0..4 {
        processor.render(&mut storage.buffers(128));
        player.update();
    }
    player.sync(&copy, None, &media);
    processor.render(&mut storage.buffers(128));
    assert!(storage.output(0).iter().all(|s| s.is_finite()));
}

#[test]
fn damaged_projects_never_crash() {
    let cases: usize = std::env::var("AUDIOSTRADA_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let seed = seed_project();
    let json = file::to_json(&seed).unwrap();
    let base: Value = serde_json::from_slice(&json).unwrap();
    let seed = std::env::var("AUDIOSTRADA_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5111_0b00_c0ff_ee01u64);
    let mut rng = Rng(seed | 1);
    let (mut loaded, mut rejected) = (0, 0);
    let started = std::time::Instant::now();
    for case in 0..cases {
        let bytes = if case % 4 == 0 {
            // Byte damage: truncation and flipped bytes.
            let mut b = json.clone();
            match rng.below(3) {
                0 => b.truncate(rng.below(b.len())),
                _ => {
                    for _ in 0..1 + rng.below(4) {
                        let i = rng.below(b.len());
                        b[i] = (rng.next() & 0xFF) as u8;
                    }
                }
            }
            b
        } else {
            let mut v = base.clone();
            for _ in 0..1 + rng.below(3) {
                mutate(&mut v, &mut rng, 0);
            }
            serde_json::to_vec(&v).unwrap()
        };
        match file::from_json(&bytes) {
            Ok(project) => {
                loaded += 1;
                exercise(&project, &mut rng);
            }
            Err(_) => rejected += 1,
        }
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "too slow at case {case}"
        );
    }
    eprintln!("{loaded} damaged projects loaded and played, {rejected} rejected");
    assert!(loaded > cases / 40, "some mutations should stay loadable");
}
