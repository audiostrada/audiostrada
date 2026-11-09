//! Sandboxed plugins against the real `audiostrada plugin-host`, with the plugins of
//! AUDIOSTRADA_TEST_CLAP and AUDIOSTRADA_TEST_INSTRUMENTS.

use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use audiostrada_plugins::sandbox::SandboxInstance;
use audiostrada_plugins::{Format, PluginInstance, scan};
#[cfg(unix)]
use audiostrada_plugins::{MidiEvent, PluginRequest};

fn exe() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_audiostrada"))
}

/// A folder of its own holding the test plugin built with the tests, as `test-gain.clap`:
/// Windows locks a loaded library, so tests running together cannot share one copy.
fn test_plugin_dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = audiostrada_core::scratch::dir(&format!("test-clap-{n}"));
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let library =
        audiostrada_test_plugin::built_library().expect("the test plugin is built with the tests");
    std::fs::copy(library, dir.join("test-gain.clap")).expect("the plugin copied");
    dir
}

fn plugins(var: &str, instrument: bool) -> Vec<audiostrada_plugins::PluginDescriptor> {
    // Without plugins of their own, effects are tested with the plugin built with the tests.
    let dir = match std::env::var_os(var) {
        Some(dir) => PathBuf::from(dir),
        None if !instrument => test_plugin_dir(),
        None => {
            eprintln!("{var} not set, skipping");
            return Vec::new();
        }
    };
    let dirs = [dir];
    let mut found = Vec::new();
    for format in [Format::Clap, Format::Vst3, Format::Lv2] {
        for file in scan::find_files(format, &dirs) {
            found.extend(
                audiostrada_plugins::scan_file(format, &file)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|d| d.instrument == instrument)
                    .take(1),
            );
        }
    }
    found
}

#[test]
fn sandboxed_effect_matches_in_process() {
    for descriptor in plugins("AUDIOSTRADA_TEST_CLAP", false).into_iter().take(2) {
        let input: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let render = |instance: &mut dyn PluginInstance| {
            let mut processor = instance.activate(48_000, 256).unwrap();
            let mut out = Vec::new();
            for _ in 0..20 {
                let (mut l, mut r) = (input.clone(), input.clone());
                processor.process(&mut l, &mut r, &[], &[], &mut |_, _| {});
                out.extend(l);
            }
            drop(processor);
            assert!(instance.try_deactivate());
            out
        };
        let mut local = audiostrada_plugins::instantiate_in_process(&descriptor).unwrap();
        let expected = render(local.as_mut());
        let mut sandboxed = SandboxInstance::new(exe(), &descriptor).unwrap();
        assert_eq!(sandboxed.params().len(), local.params().len());
        let got = render(&mut sandboxed);
        let diff = expected
            .iter()
            .zip(&got)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        eprintln!("{}: max difference {diff}", descriptor.name);
        assert!(diff < 1e-4, "{}: {diff}", descriptor.name);
        let state = sandboxed.save_state().expect("state");
        assert!(sandboxed.load_state(&state));
    }
}

#[cfg(unix)]
#[test]
fn crashed_host_is_reported_and_bypassed() {
    let Some(descriptor) = plugins("AUDIOSTRADA_TEST_INSTRUMENTS", true)
        .into_iter()
        .next()
    else {
        return;
    };
    let mut instance = SandboxInstance::new(exe(), &descriptor).unwrap();
    let mut processor = instance.activate(48_000, 256).unwrap();
    let mut energy = 0.0;
    for block in 0..20 {
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        let notes = [MidiEvent::note_on(0, 0, 60, 110)];
        let midi: &[MidiEvent] = if block == 0 { &notes } else { &[] };
        processor.process(&mut l, &mut r, &[], midi, &mut |_, _| {});
        energy += l.iter().map(|s| s * s).sum::<f32>();
    }
    assert!(
        energy > 0.01,
        "{} played through the sandbox",
        descriptor.name
    );
    assert!(instance.poll().is_empty() || !instance.crashed());

    // SAFETY: plain kill(2) of our own child process.
    unsafe { libc::kill(instance.pid() as i32, libc::SIGKILL) };
    let start = Instant::now();
    let mut requests = Vec::new();
    while !requests.contains(&PluginRequest::Crashed) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
        requests = instance.poll();
    }
    assert!(requests.contains(&PluginRequest::Crashed));
    assert!(instance.poll().is_empty(), "reported once");
    let (mut l, mut r) = (vec![0.25f32; 256], vec![0.25f32; 256]);
    let t = Instant::now();
    processor.process(&mut l, &mut r, &[], &[], &mut |_, _| {});
    assert!(
        t.elapsed() < Duration::from_millis(5),
        "no waiting after a crash"
    );
    assert!(l.iter().all(|&s| s == 0.25), "dry signal passes");
    drop(processor);
    assert!(instance.try_deactivate());
}

#[test]
#[ignore = "needs a display"]
fn sandboxed_editor_opens() {
    for descriptor in plugins("AUDIOSTRADA_TEST_CLAP", false).into_iter().take(1) {
        let mut instance = SandboxInstance::new(exe(), &descriptor).unwrap();
        let processor = instance.activate(48_000, 256).unwrap();
        assert!(instance.has_gui());
        instance.open_gui(&descriptor.name).unwrap();
        assert!(instance.gui_open());
        for _ in 0..50 {
            instance.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Ok(path) = std::env::var("AUDIOSTRADA_TEST_SCREENSHOT_CMD") {
            let _ = std::process::Command::new("sh")
                .arg("-c")
                .arg(path)
                .status();
        }
        instance.close_gui();
        assert!(!instance.gui_open());
        drop(processor);
        assert!(instance.try_deactivate());
    }
}

#[test]
fn the_scan_finds_the_plugin_out_of_process() {
    let dir = test_plugin_dir();
    let file = dir.join("test-gain.clap");
    let found =
        scan::scan_out_of_process(exe(), Format::Clap, &file, Duration::from_secs(30)).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "com.audiostrada.test-gain");
    let cache = dir.join("plugins.json");
    let progress = std::sync::Arc::default();
    let database = scan::scan_in_background(
        cache.clone(),
        vec![dir.clone()],
        exe().to_path_buf(),
        true,
        std::sync::Arc::clone(&progress),
    )
    .join()
    .unwrap();
    assert_eq!(progress.counts(), Some((1, 1)));
    assert!(
        database
            .plugins()
            .any(|p| p.id == "com.audiostrada.test-gain")
    );
    // Saved, and read back.
    assert!(
        scan::Database::load(&cache)
            .plugins()
            .any(|p| p.id == "com.audiostrada.test-gain")
    );
}

#[test]
fn a_sandboxed_plugin_hears_its_sidechain_key() {
    let file = test_plugin_dir().join("test-gain.clap");
    let found =
        scan::scan_out_of_process(exe(), Format::Clap, &file, Duration::from_secs(30)).unwrap();
    let mut instance = SandboxInstance::new(exe(), &found[0]).unwrap();
    assert!(instance.has_sidechain());
    let mut processor = instance.activate(48_000, 64).unwrap();
    let key = (vec![0.25f32; 64], vec![0.25f32; 64]);
    let (mut l, mut r) = (vec![0.5f32; 64], vec![0.5f32; 64]);
    processor.process_keyed(&mut l, &mut r, (&key.0, &key.1), &[], &[], &mut |_, _| {});
    assert!(
        (l[10] - 0.75).abs() < 1e-6 && (r[10] - 0.75).abs() < 1e-6,
        "{}",
        l[10]
    );
    let (mut l, mut r) = (vec![0.5f32; 64], vec![0.5f32; 64]);
    processor.process(&mut l, &mut r, &[], &[], &mut |_, _| {});
    assert!((l[10] - 0.5).abs() < 1e-6, "{}", l[10]);
    drop(processor);
    assert!(instance.try_deactivate());
}

/// The test plugin's VST2 side, in a folder of its own as a VST2 library of this system.
fn test_vst2_dir() -> (PathBuf, PathBuf) {
    let dir = test_plugin_dir();
    let name = if cfg!(windows) {
        "test-gain.dll"
    } else if cfg!(target_os = "macos") {
        "test-gain.vst"
    } else {
        "test-gain.so"
    };
    let file = dir.join(name);
    std::fs::copy(dir.join("test-gain.clap"), &file).expect("the plugin copied");
    (dir, file)
}

#[test]
fn a_sandboxed_vst2_plugin_sounds_and_saves_as_in_process() {
    let (dir, file) = test_vst2_dir();
    // Found out of process, by the scan of the folders too.
    let found =
        scan::scan_out_of_process(exe(), Format::Vst2, &file, Duration::from_secs(30)).unwrap();
    assert_eq!(found.len(), 1);
    let descriptor = &found[0];
    assert_eq!(descriptor.key(), "vst2:41735467");
    let database = scan::scan_in_background(
        dir.join("plugins.json"),
        vec![dir.clone()],
        exe().to_path_buf(),
        true,
        Default::default(),
    )
    .join()
    .unwrap();
    assert!(database.find("vst2:41735467").is_some());
    let render = |instance: &mut dyn PluginInstance| {
        let mut processor = instance.activate(48_000, 256).unwrap();
        let mut out = Vec::new();
        for block in 0..10 {
            let (mut l, mut r) = (vec![0.5f32; 256], vec![0.5f32; 256]);
            let changes: &[(u32, f64)] = if block == 4 { &[(0, 0.5)] } else { &[] };
            processor.process(&mut l, &mut r, changes, &[], &mut |_, _| {});
            out.extend(l);
        }
        drop(processor);
        while !instance.try_deactivate() {
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    };
    let mut local = audiostrada_plugins::instantiate_in_process(descriptor).unwrap();
    let expected = render(local.as_mut());
    let mut sandboxed = SandboxInstance::new(exe(), descriptor).unwrap();
    assert_eq!(sandboxed.params().len(), 1);
    let got = render(&mut sandboxed);
    assert_eq!(expected.len(), got.len());
    assert!(
        expected.iter().zip(&got).all(|(a, b)| (a - b).abs() < 1e-6),
        "the same sound"
    );
    assert!(
        (got[0] - 0.5).abs() < 1e-5 && got[9 * 256] < 0.04,
        "the gain set"
    );
    let state = sandboxed.save_state().expect("state");
    let mut other = SandboxInstance::new(exe(), descriptor).unwrap();
    assert!(other.load_state(&state));
    assert_eq!(other.param_value(0), Some(0.5));
}
