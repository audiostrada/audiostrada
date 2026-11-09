//! The CLAP host against a real plugin built with the tests (audiostrada-test-plugin): found,
//! loaded, its parameter read and set, its sound processed, its state saved and restored.

use audiostrada_plugins::{Format, PluginInstance};

fn plugin() -> audiostrada_plugins::PluginDescriptor {
    let file =
        audiostrada_test_plugin::built_library().expect("the test plugin is built with the tests");
    let found = audiostrada_plugins::scan_file(Format::Clap, &file).expect("scanned");
    assert_eq!(found.len(), 1);
    found.into_iter().next().expect("one plugin")
}

fn instance() -> Box<dyn PluginInstance> {
    audiostrada_plugins::instantiate_in_process(&plugin()).expect("loaded")
}

#[test]
fn the_scan_describes_the_plugin() {
    let d = plugin();
    assert_eq!(d.format, Format::Clap);
    assert_eq!(d.id, "com.audiostrada.test-gain");
    assert_eq!(
        (d.name.as_str(), d.vendor.as_str(), d.version.as_str()),
        ("Test Gain", "AudioStrada", "1.0.0")
    );
    assert!(!d.instrument);
    assert!(d.tags.iter().any(|t| t == "audio-effect"), "{:?}", d.tags);
}

#[test]
fn parameters_are_listed_read_and_shown() {
    let mut p = instance();
    let params = p.params();
    assert_eq!(params.len(), 1);
    let gain = &params[0];
    assert_eq!(
        (gain.name.as_str(), gain.min, gain.max, gain.default),
        ("Gain", -60.0, 12.0, 0.0)
    );
    assert_eq!(p.param_value(gain.id), Some(0.0));
    assert_eq!(p.value_text(gain.id, -6.0).as_deref(), Some("-6.0 dB"));
    assert_eq!(p.latency(), 0);
    assert!(!p.has_gui());
    assert!(p.poll().is_empty());
}

#[test]
fn sound_goes_through_at_the_gain_set() {
    let mut p = instance();
    let mut audio = p.activate(48_000, 256).expect("activated");
    let mut left = vec![0.5f32; 256];
    let mut right = vec![-0.25f32; 256];
    audio.process(&mut left, &mut right, &[], &[], &mut |_, _| {});
    assert!((left[100] - 0.5).abs() < 1e-6, "{}", left[100]);
    assert!((right[100] + 0.25).abs() < 1e-6, "{}", right[100]);
    // -6.0206 dB halves it.
    let mut left = vec![0.5f32; 256];
    let mut right = vec![0.5f32; 256];
    audio.process(&mut left, &mut right, &[(0, -6.0206)], &[], &mut |_, _| {});
    assert!((left[200] - 0.25).abs() < 1e-3, "{}", left[200]);
    assert!((right[0] - 0.25).abs() < 1e-3, "{}", right[0]);
    audio.reset();
    audio.set_offline(true);
    drop(audio);
    let mut waited = 0;
    while !p.try_deactivate() {
        std::thread::sleep(std::time::Duration::from_millis(5));
        waited += 1;
        assert!(waited < 200, "deactivated");
    }
}

#[test]
fn the_sidechain_input_hears_the_key_then_silence() {
    let mut p = instance();
    assert!(p.has_sidechain());
    let mut audio = p.activate(48_000, 64).expect("activated");
    let (mut left, mut right) = (vec![0.5f32; 64], vec![0.5f32; 64]);
    let key = (vec![0.25f32; 64], vec![-0.5f32; 64]);
    audio.process_keyed(
        &mut left,
        &mut right,
        (&key.0, &key.1),
        &[],
        &[],
        &mut |_, _| {},
    );
    assert!((left[10] - 0.75).abs() < 1e-6, "{}", left[10]);
    assert!(right[10].abs() < 1e-6, "{}", right[10]);
    let (mut left, mut right) = (vec![0.5f32; 64], vec![0.5f32; 64]);
    audio.process(&mut left, &mut right, &[], &[], &mut |_, _| {});
    assert!(
        (left[10] - 0.5).abs() < 1e-6,
        "the key is gone: {}",
        left[10]
    );
}

#[test]
fn the_state_comes_back() {
    let mut p = instance();
    // CLAP parameters change through the audio side.
    let mut audio = p.activate(48_000, 64).expect("activated");
    let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
    audio.process(&mut l, &mut r, &[(0, -12.0)], &[], &mut |_, _| {});
    drop(audio);
    while !p.try_deactivate() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(p.param_value(0), Some(-12.0));
    let saved = p.save_state().expect("a state");
    let mut other = instance();
    assert_eq!(other.param_value(0), Some(0.0));
    assert!(other.load_state(&saved));
    assert_eq!(other.param_value(0), Some(-12.0));
    assert!(!other.load_state(&[1, 2]), "too short");
}
