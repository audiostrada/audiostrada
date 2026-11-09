//! AudioStrada plugin hosting: discovery, a plugin database, and hosts for the plugin formats
//! (CLAP, VST3, VST2, LV2, AU).
//!
//! A plugin instance is split in two: a main-thread side ([`PluginInstance`], GUI, parameters,
//! state) that stays on the UI thread, and an audio side ([`PluginProcessor`]) that the engine
//! runs on its audio threads.

#[cfg(target_os = "macos")]
pub mod au;
pub mod clap;
pub mod guard;
pub mod lv2;
pub mod sandbox;
pub mod scan;
pub mod vst2;
pub mod vst3;
pub mod window;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub use audiostrada_midi::MidiEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    Clap,
    Vst3,
    Lv2,
    Au,
    Vst2,
}

impl Format {
    pub const ALL: [Format; 5] = [
        Format::Clap,
        Format::Vst3,
        Format::Vst2,
        Format::Lv2,
        Format::Au,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Format::Clap => "CLAP",
            Format::Vst3 => "VST3",
            Format::Vst2 => "VST2",
            Format::Lv2 => "LV2",
            Format::Au => "AU",
        }
    }

    /// Prefix of insert effect identifiers in projects.
    pub fn prefix(self) -> &'static str {
        match self {
            Format::Clap => "clap",
            Format::Vst3 => "vst3",
            Format::Vst2 => "vst2",
            Format::Lv2 => "lv2",
            Format::Au => "au",
        }
    }

    pub fn from_prefix(prefix: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.prefix() == prefix)
    }
}

/// A plugin found on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub format: Format,
    /// Identifier unique within the format.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// File or bundle holding the plugin.
    pub path: PathBuf,
    /// Generates sound from notes rather than processing audio.
    pub instrument: bool,
    /// What the plugin says it is: CLAP features, VST3 subcategories, LV2 class.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl PluginDescriptor {
    /// Identifier stored in projects, e.g. `clap:org.example.reverb`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.format.prefix(), self.id)
    }
}

/// Splits a project effect identifier into a format and a plugin id.
pub fn parse_key(key: &str) -> Option<(Format, &str)> {
    let (prefix, id) = key.split_once(':')?;
    Some((Format::from_prefix(prefix)?, id))
}

/// A plugin parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamDesc {
    pub id: u32,
    pub name: String,
    /// Group path, `/`-separated.
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub stepped: bool,
    pub hidden: bool,
    pub read_only: bool,
}

/// Something the plugin asked for, handled by the host on the main thread.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PluginRequest {
    /// Deactivate and reactivate (latency, ports or configuration changed).
    Restart,
    /// The parameter list changed.
    RescanParams,
    LatencyChanged,
    /// The plugin's GUI window was closed by the user.
    GuiClosed,
    /// A parameter was changed in the plugin's window (formats where this goes through the
    /// main thread, like VST3).
    ParamChanged(u32, f64),
    /// The plugin's process crashed or stopped responding (sandboxed plugins).
    Crashed,
    /// One of AudioStrada's shortcuts was pressed in the plugin's window: X keysym and modifiers
    /// (see `window::set_grabbed_keys`).
    Key(u32, u16),
}

/// Main-thread side of a plugin instance. Not `Send`: it stays on the UI thread.
pub trait PluginInstance: std::fmt::Debug {
    fn descriptor(&self) -> &PluginDescriptor;
    /// Activates the plugin for a format and returns its audio side.
    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String>;
    /// Deactivates once the audio side was dropped; false while it is still alive.
    fn try_deactivate(&mut self) -> bool;
    fn params(&mut self) -> Vec<ParamDesc>;
    fn param_value(&mut self, id: u32) -> Option<f64>;
    fn value_text(&mut self, id: u32, value: f64) -> Option<String>;
    /// Tells the main-thread side about a value sent to the audio side (VST3 controllers).
    fn set_param(&mut self, _id: u32, _value: f64) {}
    fn latency(&mut self) -> u32;
    fn save_state(&mut self) -> Option<Vec<u8>>;
    fn load_state(&mut self, data: &[u8]) -> bool;
    /// Main-thread housekeeping, called regularly; returns what the plugin asked for.
    fn poll(&mut self) -> Vec<PluginRequest>;
    fn has_gui(&mut self) -> bool;
    /// Opens the plugin's own window; `title` is a suggestion.
    fn open_gui(&mut self, title: &str) -> Result<(), String>;
    fn close_gui(&mut self);
    fn gui_open(&self) -> bool;
    /// Brings the plugin's open window to the front.
    fn raise_gui(&mut self) {}
    /// The plugin has a sidechain input, which a track can key.
    fn has_sidechain(&mut self) -> bool {
        false
    }
}

/// Audio side of a plugin instance; runs on audio threads, must not allocate or lock.
/// Where the song is, for plugins that follow the host: arpeggiators, synced delays and LFOs,
/// step sequencers. Before it was given, every plugin saw a song stopped at 120 BPM.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    pub playing: bool,
    /// Position of the block's first frame, in samples and in quarter notes.
    pub samples: i64,
    pub beats: f64,
    /// Where the bar it falls in started, in quarter notes, and its number from 0.
    pub bar_start: f64,
    pub bar: i32,
    pub tempo: f64,
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            playing: false,
            samples: 0,
            beats: 0.0,
            bar_start: 0.0,
            bar: 0,
            tempo: 120.0,
            numerator: 4,
            denominator: 4,
        }
    }
}

pub trait PluginProcessor: Send + std::fmt::Debug {
    /// The transport for the next block, given before it.
    fn set_transport(&mut self, _transport: &Transport) {}
    /// Processes stereo audio in place. `params_in` are value changes to apply at the start of
    /// the block; changes made by the plugin (GUI, internal) are reported through `params_out`.
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    );
    /// Like [`process`](Self::process), with `key` in the plugin's sidechain input (see
    /// [`PluginInstance::has_sidechain`]).
    fn process_keyed(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: (&[f32], &[f32]),
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        let _ = key;
        self.process(left, right, params_in, midi, params_out);
    }
    /// Clears tails after a transport jump.
    fn reset(&mut self);
    /// Offline rendering: the plugin may take longer than real time.
    fn set_offline(&mut self, _offline: bool) {}
}

/// Copies the first `n` frames of a stereo block to a plugin's inputs: left to the first
/// channel, right to the others; a mono input gets both, mixed.
pub(crate) fn fill_inputs(inputs: &mut [Vec<f32>], left: &[f32], right: &[f32], n: usize) {
    if let [mono] = inputs {
        for ((m, l), r) in mono[..n].iter_mut().zip(&left[..n]).zip(&right[..n]) {
            *m = 0.5 * (l + r);
        }
        return;
    }
    for (ch, buffer) in inputs.iter_mut().enumerate() {
        buffer[..n].copy_from_slice(if ch == 0 { &left[..n] } else { &right[..n] });
    }
}

/// Fills a plugin's sidechain input with the key, or with silence once there is none
/// (`keyed` tells whether it holds a key from the last block).
pub(crate) fn fill_key(
    inputs: &mut [Vec<f32>],
    key: Option<(&[f32], &[f32])>,
    keyed: &mut bool,
    n: usize,
) {
    match key {
        Some((l, r)) => {
            let n = n.min(l.len()).min(r.len());
            fill_inputs(inputs, l, r, n);
            *keyed = true;
        }
        None if *keyed => {
            for buffer in inputs.iter_mut() {
                buffer.fill(0.0);
            }
            *keyed = false;
        }
        None => {}
    }
}

/// Copies the first `n` frames of a plugin's outputs back to stereo; a mono output feeds both.
pub(crate) fn take_outputs(outputs: &[Vec<f32>], left: &mut [f32], right: &mut [f32], n: usize) {
    match outputs {
        [mono] => {
            left[..n].copy_from_slice(&mono[..n]);
            right[..n].copy_from_slice(&mono[..n]);
        }
        [l, r, ..] => {
            left[..n].copy_from_slice(&l[..n]);
            right[..n].copy_from_slice(&r[..n]);
        }
        [] => {}
    }
}

/// Frame of `event` within a block of `n` frames (`n` > 0).
pub(crate) fn block_time(event: &MidiEvent, n: usize) -> u32 {
    event.time.min(n.saturating_sub(1) as u32)
}

static SANDBOX: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Runs plugins loaded from now on in child processes of `host` (the AudioStrada executable,
/// which serves `plugin-host`), or in this process when `None`.
/// This program's file, to start its scanner and plugin hosts. When it was replaced while
/// running (an update), Linux names the old one "… (deleted)": the new one, at the same path,
/// is used.
pub fn own_executable() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if exe.exists() {
        return Some(exe);
    }
    let name = exe.to_string_lossy();
    let replaced = PathBuf::from(name.strip_suffix(" (deleted)").unwrap_or(&name));
    Some(if replaced.exists() { replaced } else { exe })
}

pub fn set_sandbox(host: Option<PathBuf>) {
    if let Ok(mut s) = SANDBOX.lock() {
        *s = host;
    }
}

pub fn sandbox_host() -> Option<PathBuf> {
    SANDBOX.lock().ok().and_then(|s| s.clone())
}

/// For the processes that load plugins (scanner, sandbox host): on Windows a plugin that
/// crashes, or misses a DLL, would otherwise open a system dialog and keep the process alive
/// until it is answered, instead of failing at once.
pub fn quiet_failures() {
    #[cfg(windows)]
    // SAFETY: plain Win32 call on this process's own error mode.
    unsafe {
        use windows::Win32::System::Diagnostics::Debug::{
            SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX, SetErrorMode,
        };
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    }
}

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Allows or forbids loading third-party plugins; built-in effects are not concerned.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Loads a plugin described by `descriptor`, sandboxed when [`set_sandbox`] says so.
pub fn instantiate(descriptor: &PluginDescriptor) -> Result<Box<dyn PluginInstance>, String> {
    if !enabled() {
        return Err("third-party plugins are disabled".into());
    }
    match sandbox_host() {
        Some(exe) => sandbox::SandboxInstance::new(&exe, descriptor)
            .map(|i| Box::new(i) as Box<dyn PluginInstance>),
        None if guard::crashed(descriptor) => Err(
            "this plugin crashed AudioStrada while loading; turn the plugin sandbox on to use it"
                .into(),
        ),
        None => instantiate_in_process(descriptor),
    }
}

/// Loads a plugin in this process.
pub fn instantiate_in_process(
    descriptor: &PluginDescriptor,
) -> Result<Box<dyn PluginInstance>, String> {
    match descriptor.format {
        Format::Clap => {
            clap::ClapInstance::new(descriptor).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
        Format::Vst3 => {
            vst3::Vst3Instance::new(descriptor).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
        Format::Vst2 => {
            vst2::Vst2Instance::new(descriptor).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
        Format::Lv2 => {
            lv2::Lv2Instance::new(descriptor).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
        #[cfg(target_os = "macos")]
        Format::Au => {
            au::AuInstance::new(descriptor).map(|i| Box::new(i) as Box<dyn PluginInstance>)
        }
        #[cfg(not(target_os = "macos"))]
        other => Err(format!("{} plugins are not supported yet", other.name())),
    }
}

/// Lists the plugins of a file or bundle, in this process. Prefer
/// [`scan::scan_out_of_process`], which survives crashing plugins.
pub fn scan_file(format: Format, path: &std::path::Path) -> Result<Vec<PluginDescriptor>, String> {
    match format {
        Format::Clap => clap::scan(path),
        Format::Vst3 => vst3::scan(path),
        Format::Vst2 => vst2::scan(path),
        Format::Lv2 => lv2::scan(path),
        #[cfg(target_os = "macos")]
        Format::Au => au::scan(path),
        #[cfg(not(target_os = "macos"))]
        other => Err(format!("{} plugins are not supported yet", other.name())),
    }
}

/// Explains why a plugin library cannot load here when it was built for another system.
pub(crate) fn check_binary(path: &Path) -> Result<(), String> {
    let mut magic = [0u8; 4];
    let read = std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
        .is_ok();
    if !read {
        return Ok(());
    }
    let windows = magic.starts_with(b"MZ");
    let elf = magic == *b"\x7fELF";
    if cfg!(windows) && elf {
        return Err("this is a Linux plugin, it cannot run on Windows".into());
    }
    if !cfg!(windows) && windows {
        return Err(if cfg!(target_os = "macos") {
            "this is a Windows plugin, it cannot run on macOS".into()
        } else {
            "this is a Windows plugin, it cannot run on Linux".into()
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_real_instruments_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_INSTRUMENTS") else {
            eprintln!("AUDIOSTRADA_TEST_INSTRUMENTS not set, skipping");
            return;
        };
        let dirs = [std::path::PathBuf::from(dir)];
        let mut played = 0;
        for format in [Format::Clap, Format::Vst3, Format::Lv2] {
            for file in scan::find_files(format, &dirs) {
                for descriptor in scan_file(format, &file).unwrap_or_default() {
                    if !descriptor.instrument {
                        continue;
                    }
                    let mut instance = instantiate(&descriptor).unwrap();
                    let mut processor = instance.activate(48_000, 256).unwrap();
                    let mut energy = 0.0;
                    for block in 0..40 {
                        let (mut left, mut right) = (vec![0.0f32; 256], vec![0.0f32; 256]);
                        let notes = [MidiEvent::note_on(10, 0, 57, 110)];
                        let midi: &[MidiEvent] = if block == 2 { &notes } else { &[] };
                        processor.process(&mut left, &mut right, &[], midi, &mut |_, _| {});
                        energy += left.iter().chain(&right).map(|s| s * s).sum::<f32>();
                    }
                    eprintln!("{} {}: energy {energy}", format.name(), descriptor.name);
                    assert!(
                        energy > 0.01,
                        "{} {} stayed silent",
                        format.name(),
                        descriptor.name
                    );
                    drop(processor);
                    played += 1;
                }
            }
        }
        assert!(played > 0);
    }

    #[test]
    fn stereo_blocks_map_to_plugin_channels() {
        let (mut left, mut right) = (vec![1.0f32; 4], vec![2.0f32; 4]);
        let mut inputs = vec![vec![0.0f32; 4]; 3];
        fill_inputs(&mut inputs, &left, &right, 3);
        assert_eq!(inputs[0], [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(inputs[2], [2.0, 2.0, 2.0, 0.0]);
        // A mono input hears both sides.
        let mut mono = vec![vec![0.0f32; 4]];
        fill_inputs(&mut mono, &left, &right, 4);
        assert_eq!(mono[0], [1.5; 4]);
        // A key, then silence once it is gone.
        let (mut key, mut keyed) = (vec![vec![0.0f32; 4]; 2], false);
        fill_key(&mut key, Some((&left, &right)), &mut keyed, 4);
        assert!(keyed && key[1] == [2.0; 4]);
        fill_key(&mut key, None, &mut keyed, 4);
        assert!(!keyed && key[0] == [0.0; 4] && key[1] == [0.0; 4]);
        take_outputs(&[vec![5.0; 4]], &mut left, &mut right, 2);
        assert_eq!((left[..3].to_vec(), right[1]), (vec![5.0, 5.0, 1.0], 5.0));
        take_outputs(
            &[vec![6.0; 4], vec![7.0; 4], vec![8.0; 4]],
            &mut left,
            &mut right,
            4,
        );
        assert_eq!((left[3], right[3]), (6.0, 7.0));
        take_outputs(&[], &mut left, &mut right, 4);
        assert_eq!(left[0], 6.0);
        assert_eq!(block_time(&MidiEvent::note_on(300, 0, 60, 1), 256), 255);
        assert_eq!(block_time(&MidiEvent::note_on(3, 0, 60, 1), 256), 3);
    }

    #[test]
    fn keys_and_formats_round_trip() {
        for format in Format::ALL {
            assert_eq!(Format::from_prefix(format.prefix()), Some(format));
        }
        let descriptor = PluginDescriptor {
            format: Format::Vst3,
            id: "ABC".into(),
            name: String::new(),
            vendor: String::new(),
            version: String::new(),
            path: PathBuf::new(),
            instrument: false,
            tags: Vec::new(),
        };
        assert_eq!(descriptor.key(), "vst3:ABC");
        assert_eq!(parse_key("lv2:urn:x"), Some((Format::Lv2, "urn:x")));
        assert_eq!(parse_key("nope:x"), None);
        assert!(
            instantiate_in_process(&PluginDescriptor {
                format: Format::Au,
                ..descriptor.clone()
            })
            .unwrap_err()
            .contains("AU")
        );
        assert!(scan_file(Format::Au, Path::new("x")).is_err());
    }

    #[test]
    fn foreign_binaries_are_explained() {
        let dir = audiostrada_core::scratch::dir("binary");
        std::fs::create_dir_all(&dir).unwrap();
        let windows = dir.join("w.vst3");
        std::fs::write(&windows, b"MZ\x90\x00rest").unwrap();
        let linux = dir.join("l.so");
        std::fs::write(&linux, b"\x7fELFrest").unwrap();
        if cfg!(windows) {
            assert!(super::check_binary(&windows).is_ok());
            assert!(super::check_binary(&linux).is_err());
        } else {
            assert!(
                super::check_binary(&windows)
                    .unwrap_err()
                    .contains("Windows plugin")
            );
            assert!(super::check_binary(&linux).is_ok());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
