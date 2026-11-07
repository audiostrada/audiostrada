//! A large project edited at random while it plays: the graph keeps up, the output stays
//! finite, and nothing allocates on the audio thread. AUDIOSTRADA_STRESS_STEPS sets the number of
//! edits (default 150).
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use audiostrada_core::automation::{AutomationTarget, CurveShape, lane_mut};
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{History, Project, Source, TrackId, edit};
use audiostrada_engine::EngineConfig;
use audiostrada_engine::media::MediaPool;
use audiostrada_engine::project::{ProjectPlayer, media_paths};
use audiostrada_io::ChannelBuffers;
use audiostrada_media::{AudioData, write_wav};
use audiostrada_rt::{alloc_violations, rt_section};

audiostrada_rt::install_alloc_checker!();

/// Every built-in effect that needs no file (the amp model loads one).
fn effects() -> Vec<&'static str> {
    audiostrada_dsp::EFFECTS
        .iter()
        .filter(|i| !i.instrument && i.files.is_empty())
        .map(|i| i.id)
        .collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn sources(dir: &std::path::Path) -> Vec<Source> {
    (0..3)
        .map(|k| {
            let path = dir.join(format!("s{k}.wav"));
            let samples: Vec<f32> = (0..96_000)
                .map(|i| 0.3 * ((i as f32) * (0.01 + 0.02 * k as f32)).sin())
                .collect();
            write_wav(&path, &AudioData::new(vec![samples], 48_000)).unwrap();
            Source {
                path,
                channels: 1,
                sample_rate: 48_000,
                frames: 96_000,
            }
        })
        .collect()
}

fn big_project(rng: &mut Rng, sources: &[Source]) -> Project {
    let mut p = Project::new("Stress", 48_000);
    let ids: Vec<_> = sources.iter().map(|s| p.add_source(s.clone())).collect();
    let buses: Vec<TrackId> = (0..4).map(|i| p.add_track(&format!("Bus {i}"))).collect();
    let vca = routing::add_vca(&mut p, "VCA");
    for i in 0..48 {
        let t = p.add_track(&format!("T{i}"));
        for k in 0..1 + rng.below(3) {
            p.add_clip(
                t,
                ids[rng.below(ids.len())],
                (k * 60_000 + rng.below(20_000)) as u64,
            );
        }
        for _ in 0..rng.below(3) {
            let effect = {
                let all = effects();
                all[rng.below(all.len())]
            };
            routing::add_insert(&mut p, Channel::Track(t), effect, Default::default());
        }
        if rng.below(2) == 0 {
            let _ = routing::add_send(&mut p, t, buses[rng.below(buses.len())]);
        }
        if rng.below(4) == 0 {
            let _ = routing::set_vca(&mut p, t, Some(vca));
        }
        let lane = lane_mut(p.track_mut(t).unwrap(), &AutomationTarget::Gain);
        lane.add(0, -6.0, CurveShape::Linear);
        lane.add(96_000, 0.0, CurveShape::Smooth);
    }
    p
}

fn random_edit(history: &mut History, rng: &mut Rng) {
    let project = history.project();
    let tracks: Vec<TrackId> = project.tracks.iter().map(|t| t.id).collect();
    let t = tracks[rng.below(tracks.len())];
    let clips: Vec<_> = project
        .track(t)
        .map(|x| x.clips.iter().map(|c| c.id).collect())
        .unwrap_or_default();
    let choice = rng.below(9);
    let delta = rng.below(20_000) as i64 - 10_000;
    let effect = {
        let all = effects();
        all[rng.below(all.len())]
    };
    let other = tracks[rng.below(tracks.len())];
    let value = rng.below(100) as f32 / 10.0 - 5.0;
    history.edit("Stress", |p| match choice {
        0 => {
            if let Some(x) = p.track_mut(t) {
                x.mute = !x.mute;
            }
        }
        1 => {
            if let Some(x) = p.track_mut(t) {
                x.solo = !x.solo;
            }
        }
        2 => {
            routing::add_insert(p, Channel::Track(t), effect, Default::default());
        }
        3 => {
            if let Some(i) = p.track(t).and_then(|x| x.inserts.first()).map(|i| i.id) {
                routing::remove_insert(p, Channel::Track(t), i);
            }
        }
        4 => edit::move_clips(p, &clips, delta, 0),
        5 => {
            let _ = routing::add_send(p, t, other);
        }
        6 => {
            if let Some(x) = p.track_mut(t) {
                x.gain_db = value;
                x.pan = value / 5.0;
            }
        }
        7 => {
            if p.tracks.len() > 8 {
                p.remove_track(t);
            }
        }
        _ => {
            let new = p.add_track("New");
            routing::add_insert(p, Channel::Track(new), effect, Default::default());
        }
    });
}

fn stress(workers: usize) {
    let steps: usize = std::env::var("AUDIOSTRADA_STRESS_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let dir = audiostrada_core::scratch::dir(&format!("stress-{workers}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ workers as u64);
    let sources = sources(&dir);
    let mut history = History::new(big_project(&mut rng, &sources));
    let mut media = MediaPool::new(48_000, None);
    media.retain(&media_paths(history.project(), None));
    assert!(media.wait(Duration::from_secs(20)));
    let mut player = ProjectPlayer::new(EngineConfig {
        workers: Some(workers),
    });
    let mut processor = player.processor(48_000, 256);
    let mut storage = ChannelBuffers::new(0, 2, 256);
    player.sync(history.project(), None, &media);
    player.engine_mut().play();
    let before = alloc_violations();
    let started = std::time::Instant::now();
    for step in 0..steps {
        random_edit(&mut history, &mut rng);
        player.sync(history.project(), None, &media);
        // Jumps reset every effect on the audio thread.
        if step % 10 == 0 {
            player.locate(rng.below(96_000) as u64);
        }
        for _ in 0..2 {
            rt_section(|| processor.render(&mut storage.buffers(256)));
            player.update();
        }
        assert!(
            storage
                .output(0)
                .iter()
                .chain(storage.output(1))
                .all(|s| s.is_finite()),
            "step {step}"
        );
    }
    let elapsed = started.elapsed();
    eprintln!(
        "{workers} workers: {steps} edits on {} tracks in {elapsed:.2?}",
        history.project().tracks.len()
    );
    assert_eq!(
        alloc_violations() - before,
        0,
        "allocations on the audio thread"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edits_while_playing_single_thread() {
    stress(0);
}

#[test]
fn edits_while_playing_with_workers() {
    stress(2);
}

/// Plays the project in AUDIOSTRADA_TEST_PROJECT, checking the audio thread does not allocate.
#[test]
fn project_plays_without_allocating() {
    let Some(file) = std::env::var_os("AUDIOSTRADA_TEST_PROJECT") else {
        return;
    };
    let file = std::path::PathBuf::from(file);
    let mut project = audiostrada_core::file::load(&file).unwrap();
    // Usual session state on top: loop, metronome, an armed track hearing its input.
    project.loop_range = Some((0, 48_000));
    project.metronome.enabled = true;
    if let Some(t) = project.tracks.first().map(|t| t.id) {
        project.track_mut(t).unwrap().armed = true;
    }
    let paths = audiostrada_core::ProjectPaths::new(&file);
    let mut media = MediaPool::new(48_000, None);
    media.retain(&media_paths(&project, Some(&paths)));
    assert!(media.wait(Duration::from_secs(30)));
    for workers in [0, 2] {
        let mut player = ProjectPlayer::new(EngineConfig {
            workers: Some(workers),
        });
        let mut processor = player.processor(48_000, 256);
        let mut storage = ChannelBuffers::new(2, 2, 256);
        player.sync(&project, Some(&paths), &media);
        let before = alloc_violations();
        for block in 0..4000 {
            if block == 200 {
                player.engine_mut().play();
            }
            if block % 700 == 0 {
                player.locate(block as u64 * 37);
            }
            if block == 3000 {
                player.engine_mut().stop();
            }
            rt_section(|| processor.render(&mut storage.buffers(256)));
            player.update();
            let now = alloc_violations() - before;
            assert_eq!(now, 0, "{workers} workers: allocation at block {block}");
        }
    }
}

/// Each effect alone: playing and jumping (which resets it) must not allocate.
#[test]
fn effects_play_and_reset_without_allocating() {
    let dir = audiostrada_core::scratch::dir("reset");
    std::fs::create_dir_all(&dir).unwrap();
    let sources = sources(&dir);
    let mut failed = Vec::new();
    for effect in effects() {
        let mut p = Project::new("Reset", 48_000);
        let source = p.add_source(sources[0].clone());
        let t = p.add_track("T");
        p.add_clip(t, source, 0);
        routing::add_insert(&mut p, Channel::Track(t), effect, Default::default());
        let mut media = MediaPool::new(48_000, None);
        media.retain(&media_paths(&p, None));
        assert!(media.wait(Duration::from_secs(20)));
        let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
        let mut processor = player.processor(48_000, 256);
        let mut storage = ChannelBuffers::new(0, 2, 256);
        player.sync(&p, None, &media);
        player.engine_mut().play();
        // First blocks may set things up; count from then on.
        for _ in 0..4 {
            rt_section(|| processor.render(&mut storage.buffers(256)));
            player.update();
        }
        let before = alloc_violations();
        for block in 0..200 {
            if block % 50 == 0 {
                player.locate(block as u64 * 300);
            }
            rt_section(|| processor.render(&mut storage.buffers(256)));
            player.update();
        }
        if alloc_violations() != before {
            failed.push(effect);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        failed.is_empty(),
        "allocating on the audio thread: {failed:?}"
    );
}

/// The amp rig with captures and pedals: playing, switching captures and jumping must not
/// allocate.
#[test]
fn amp_rig_plays_switches_and_resets_without_allocating() {
    let dir = audiostrada_core::scratch::dir("rig");
    let pack = dir.join("Amp");
    std::fs::create_dir_all(&pack).unwrap();
    write_models(&pack);
    let sources = sources(&dir);
    let mut history = History::new(Project::new("Rig", 48_000));
    let mut rig = None;
    let mut track = None;
    history.edit("Rig", |p| {
        let source = p.add_source(sources[0].clone());
        let t = p.add_track("Guitar");
        p.add_clip(t, source, 0);
        let id = routing::add_insert(p, Channel::Track(t), "amp_rig", Default::default()).unwrap();
        let insert = p
            .track_mut(t)
            .unwrap()
            .inserts
            .iter_mut()
            .find(|i| i.id == id)
            .unwrap();
        insert.files.insert("amp".into(), pack.clone());
        insert.params.insert("model".into(), 0.0);
        for (slot, kind) in [
            (1, 2.0),
            (2, 6.0),
            (3, 10.0),
            (4, 11.0),
            (6, 15.0),
            (8, 19.0),
        ] {
            insert.params.insert(format!("p{slot}_type"), kind);
        }
        rig = Some(id);
        track = Some(t);
    });
    let (rig, track) = (rig.unwrap(), track.unwrap());
    let mut media = MediaPool::new(48_000, None);
    media.retain(&media_paths(history.project(), None));
    assert!(media.wait(Duration::from_secs(20)));
    let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
    let mut processor = player.processor(48_000, 256);
    let mut storage = ChannelBuffers::new(0, 2, 256);
    player.sync(history.project(), None, &media);
    player.engine_mut().play();
    for _ in 0..4 {
        rt_section(|| processor.render(&mut storage.buffers(256)));
        player.update();
    }
    let before = alloc_violations();
    for block in 0..400 {
        if block % 100 == 50 {
            let gain = if block % 200 == 50 { 0.0 } else { 10.0 };
            history.edit("Gain", |p| {
                routing::set_insert_param(p, Channel::Track(track), rig, "gain", gain)
            });
            // Captures, then a built-in amp, then captures again.
            let model = if block == 250 { 4.0 } else { 0.0 };
            history.edit("Model", |p| {
                routing::set_insert_param(p, Channel::Track(track), rig, "model", model)
            });
            player.sync(history.project(), None, &media);
        }
        if block % 100 == 0 {
            player.locate(block as u64 * 200);
        }
        rt_section(|| processor.render(&mut storage.buffers(256)));
        player.update();
        assert!(storage.output(0).iter().all(|s| s.is_finite()));
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        alloc_violations() - before,
        0,
        "allocations on the audio thread"
    );
}

/// Two tiny captures with loudness and gain metadata, as `c0.nam` and `c1.nam` in `dir`.
fn write_models(dir: &std::path::Path) {
    for (i, gain) in [0.2, 0.8].iter().enumerate() {
        let model = format!(
            r#"{{"version": "0.5.4", "architecture": "WaveNet",
            "metadata": {{"gain": {gain}, "loudness": -12.0}},
            "config": {{"layers": [{{"input_size": 1, "condition_size": 1, "channels": 1,
            "head_size": 1, "kernel_size": 1, "dilations": [1], "activation": "ReLU",
            "gated": false, "head_bias": false}}], "head": null, "head_scale": 1.0}},
            "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]}}"#
        );
        std::fs::write(dir.join(format!("c{i}.nam")), model).unwrap();
    }
}

/// The single-capture NAM effect must not allocate either (its metadata is read once).
#[test]
fn nam_plays_without_allocating() {
    let dir = audiostrada_core::scratch::dir("nam");
    std::fs::create_dir_all(&dir).unwrap();
    write_models(&dir);
    let sources = sources(&dir);
    let mut p = Project::new("Nam", 48_000);
    let source = p.add_source(sources[0].clone());
    let t = p.add_track("Guitar");
    p.add_clip(t, source, 0);
    let id = routing::add_insert(&mut p, Channel::Track(t), "amp_rig", Default::default()).unwrap();
    if let Some(insert) = p
        .track_mut(t)
        .unwrap()
        .inserts
        .iter_mut()
        .find(|i| i.id == id)
    {
        insert.params.insert("model".into(), 0.0);
        insert.files.insert("amp".into(), dir.join("c0.nam"));
    }
    let mut media = MediaPool::new(48_000, None);
    media.retain(&media_paths(&p, None));
    assert!(media.wait(Duration::from_secs(20)));
    let mut player = ProjectPlayer::new(EngineConfig { workers: Some(0) });
    let mut processor = player.processor(48_000, 256);
    let mut storage = ChannelBuffers::new(0, 2, 256);
    player.sync(&p, None, &media);
    player.engine_mut().play();
    for _ in 0..4 {
        rt_section(|| processor.render(&mut storage.buffers(256)));
        player.update();
    }
    let before = alloc_violations();
    for block in 0..200 {
        if block % 50 == 0 {
            player.locate(block as u64 * 300);
        }
        rt_section(|| processor.render(&mut storage.buffers(256)));
        player.update();
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        alloc_violations() - before,
        0,
        "allocations on the audio thread"
    );
}
