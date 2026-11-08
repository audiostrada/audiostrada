//! The VST2 host against a real plugin built with the tests (audiostrada-test-plugin, whose
//! library is also a VST2 plugin): found, loaded, its parameter read and set, its sound
//! processed, its state saved and restored, the values it sets itself reported.

use audiostrada_plugins::{Format, PluginDescriptor, PluginInstance, PluginRequest};
use audiostrada_test_plugin::{VST2_ID, VST2_UNITY};

fn plugin() -> PluginDescriptor {
    let file =
        audiostrada_test_plugin::built_library().expect("the test plugin is built with the tests");
    let found = audiostrada_plugins::scan_file(Format::Vst2, &file).expect("scanned");
    assert_eq!(found.len(), 1);
    found.into_iter().next().expect("one plugin")
}

fn instance() -> Box<dyn PluginInstance> {
    audiostrada_plugins::instantiate_in_process(&plugin()).expect("loaded")
}

fn deactivate(p: &mut Box<dyn PluginInstance>) {
    let mut waited = 0;
    while !p.try_deactivate() {
        std::thread::sleep(std::time::Duration::from_millis(5));
        waited += 1;
        assert!(waited < 200, "deactivated");
    }
}

#[test]
fn the_scan_describes_the_plugin() {
    let d = plugin();
    assert_eq!(d.format, Format::Vst2);
    assert_eq!(d.id, format!("{:08X}", VST2_ID as u32));
    assert_eq!(d.key(), "vst2:41735467");
    assert_eq!(
        (d.name.as_str(), d.vendor.as_str(), d.version.as_str()),
        ("Test Gain", "AudioStrada", "1.0.0.0")
    );
    assert!(!d.instrument);
    assert_eq!(d.tags, ["Fx"]);
}

#[test]
fn parameters_are_listed_read_and_shown() {
    let mut p = instance();
    let params = p.params();
    assert_eq!(params.len(), 1);
    let gain = &params[0];
    assert_eq!((gain.name.as_str(), gain.min, gain.max), ("Gain", 0.0, 1.0));
    assert_eq!(gain.default, f64::from(VST2_UNITY));
    assert_eq!(p.param_value(gain.id), Some(f64::from(VST2_UNITY)));
    // Only the current value has a text: VST2 shows no other.
    assert_eq!(
        p.value_text(gain.id, f64::from(VST2_UNITY)).as_deref(),
        Some("0.0 dB")
    );
    assert_eq!(p.value_text(gain.id, 0.1), None);
    assert_eq!(p.param_value(7), None);
    assert_eq!(p.latency(), 0);
    assert!(!p.has_gui());
    assert!(p.open_gui("x").is_err());
    assert!(p.poll().is_empty());
}

#[test]
fn sound_goes_through_at_the_gain_set() {
    let mut p = instance();
    let mut audio = p.activate(48_000, 256).expect("activated");
    let mut left = vec![0.5f32; 256];
    let mut right = vec![-0.25f32; 256];
    audio.process(&mut left, &mut right, &[], &[], &mut |_, _| {});
    assert!((left[100] - 0.5).abs() < 1e-5, "{}", left[100]);
    assert!((right[100] + 0.25).abs() < 1e-5, "{}", right[100]);
    // 0.5: -24 dB.
    let (mut left, mut right) = (vec![1.0f32; 256], vec![1.0f32; 256]);
    audio.process(&mut left, &mut right, &[(0, 0.5)], &[], &mut |_, _| {});
    let expected = 10f32.powf(-24.0 / 20.0);
    assert!((left[200] - expected).abs() < 1e-4, "{}", left[200]);
    assert!((right[0] - expected).abs() < 1e-4, "{}", right[0]);
    assert_eq!(p.param_value(0), Some(0.5));
    // Shorter blocks, and notes it does not use, go through too.
    let (mut left, mut right) = (vec![1.0f32; 64], vec![1.0f32; 64]);
    let notes = [audiostrada_plugins::MidiEvent::note_on(3, 0, 60, 100)];
    audio.process(&mut left, &mut right, &[], &notes, &mut |_, _| {});
    assert!((left[63] - expected).abs() < 1e-4);
    assert!(!p.try_deactivate(), "its audio side is still alive");
    drop(audio);
    deactivate(&mut p);
    // Activated again, the same instance.
    let audio = p.activate(44_100, 128).expect("activated again");
    drop(audio);
    deactivate(&mut p);
}

#[test]
fn the_state_comes_back() {
    let mut p = instance();
    let mut audio = p.activate(48_000, 64).expect("activated");
    let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
    audio.process(&mut l, &mut r, &[(0, 0.25)], &[], &mut |_, _| {});
    drop(audio);
    deactivate(&mut p);
    let state = p.save_state().expect("a state");
    assert!(state.starts_with(b"AS2C"), "saved as the plugin's chunk");
    let mut other = instance();
    assert_eq!(other.param_value(0), Some(f64::from(VST2_UNITY)));
    assert!(other.load_state(&state));
    assert_eq!(other.param_value(0), Some(0.25));
    assert!(!other.load_state(b"nothing"));
}

#[test]
fn values_the_plugin_sets_itself_are_reported() {
    // Its program 1 sets the gain to 0.5 and tells the host, as its window would.
    let program_one = [b"AS2P".as_slice(), &1i32.to_le_bytes()].concat();
    // Without an audio side: from the main thread.
    let mut p = instance();
    assert!(p.load_state(&program_one));
    assert_eq!(p.poll(), [PluginRequest::ParamChanged(0, 0.5)]);
    assert!(p.poll().is_empty());
    // With one: from the audio side, as a plugin's changes come.
    let mut p = instance();
    let mut audio = p.activate(48_000, 64).expect("activated");
    assert!(p.load_state(&program_one));
    assert!(p.poll().is_empty());
    let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
    let mut reported = Vec::new();
    audio.process(&mut l, &mut r, &[], &[], &mut |id, v| {
        reported.push((id, v))
    });
    assert_eq!(reported, [(0, 0.5)]);
    drop(audio);
    deactivate(&mut p);
}

#[test]
fn a_library_that_is_no_plugin_has_none() {
    let dir = std::env::temp_dir().join(format!("audiostrada-vst2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join(if cfg!(windows) {
        "helper.dll"
    } else {
        "helper.so"
    });
    std::fs::write(&file, b"not a library").expect("written");
    let found = audiostrada_plugins::scan_file(Format::Vst2, &file);
    // Windows reads its exports without loading it; elsewhere loading it fails.
    if cfg!(windows) {
        assert_eq!(found, Ok(Vec::new()));
    } else {
        assert!(found.is_err());
    }
    std::fs::remove_dir_all(&dir).expect("removed");
}
