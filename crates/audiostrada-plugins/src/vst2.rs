//! VST2 host: plugins as a library exporting `VSTPluginMain` (or `main`), one C structure per
//! instance (`AEffect`) driven through a dispatcher of numbered commands.
//!
//! Steinberg no longer licenses the VST2 SDK; the binary interface below is written from what
//! is publicly known of it (as free hosts do), without the SDK's headers.
//!
//! A shell file (Waves' WaveShell) holds many plugins: each one is created by answering its
//! identifier when the library asks the host which one it wants (`CURRENT_ID`).

use std::cell::{Cell, UnsafeCell};
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::window::{HostWindow, NativeHandle};
use crate::{Format, MidiEvent, ParamDesc, PluginDescriptor, PluginProcessor, PluginRequest};

/// The binary interface: structures, commands to the plugin (`eff`), requests to the host
/// (`host`), flags.
#[allow(
    dead_code,
    reason = "the interface is listed whole, the host uses part of it"
)]
mod abi {
    use std::ffi::c_void;

    pub type Dispatcher =
        unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
    pub type Process = unsafe extern "C" fn(*mut AEffect, *mut *mut f32, *mut *mut f32, i32);
    pub type ProcessDouble = unsafe extern "C" fn(*mut AEffect, *mut *mut f64, *mut *mut f64, i32);
    pub type SetParameter = unsafe extern "C" fn(*mut AEffect, i32, f32);
    pub type GetParameter = unsafe extern "C" fn(*mut AEffect, i32) -> f32;
    pub type HostCallback =
        unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
    pub type Entry = unsafe extern "C" fn(HostCallback) -> *mut AEffect;

    /// `'VstP'`.
    pub const MAGIC: i32 = 0x5673_7450;

    #[repr(C)]
    pub struct AEffect {
        pub magic: i32,
        pub dispatcher: Option<Dispatcher>,
        pub process: Option<Process>,
        pub set_parameter: Option<SetParameter>,
        pub get_parameter: Option<GetParameter>,
        pub num_programs: i32,
        pub num_params: i32,
        pub num_inputs: i32,
        pub num_outputs: i32,
        pub flags: i32,
        pub reserved1: isize,
        pub reserved2: isize,
        pub initial_delay: i32,
        pub real_qualities: i32,
        pub off_qualities: i32,
        pub io_ratio: f32,
        /// The plugin's own object.
        pub object: *mut c_void,
        /// Free for the host: AudioStrada keeps its side of the instance there.
        pub user: *mut c_void,
        pub unique_id: i32,
        pub version: i32,
        pub process_replacing: Option<Process>,
        pub process_double_replacing: Option<ProcessDouble>,
        pub future: [u8; 56],
    }

    pub const FLAG_HAS_EDITOR: i32 = 1;
    pub const FLAG_CAN_REPLACING: i32 = 1 << 4;
    pub const FLAG_PROGRAM_CHUNKS: i32 = 1 << 5;
    pub const FLAG_IS_SYNTH: i32 = 1 << 8;

    // Commands to the plugin.
    pub const EFF_OPEN: i32 = 0;
    pub const EFF_CLOSE: i32 = 1;
    pub const EFF_SET_PROGRAM: i32 = 2;
    pub const EFF_GET_PROGRAM: i32 = 3;
    pub const EFF_GET_PARAM_LABEL: i32 = 6;
    pub const EFF_GET_PARAM_DISPLAY: i32 = 7;
    pub const EFF_GET_PARAM_NAME: i32 = 8;
    pub const EFF_SET_SAMPLE_RATE: i32 = 10;
    pub const EFF_SET_BLOCK_SIZE: i32 = 11;
    pub const EFF_MAINS_CHANGED: i32 = 12;
    pub const EFF_EDIT_GET_RECT: i32 = 13;
    pub const EFF_EDIT_OPEN: i32 = 14;
    pub const EFF_EDIT_CLOSE: i32 = 15;
    pub const EFF_EDIT_IDLE: i32 = 19;
    pub const EFF_GET_CHUNK: i32 = 23;
    pub const EFF_SET_CHUNK: i32 = 24;
    pub const EFF_PROCESS_EVENTS: i32 = 25;
    pub const EFF_GET_PLUG_CATEGORY: i32 = 35;
    pub const EFF_GET_EFFECT_NAME: i32 = 45;
    pub const EFF_GET_VENDOR_STRING: i32 = 47;
    pub const EFF_GET_PRODUCT_STRING: i32 = 48;
    pub const EFF_GET_VENDOR_VERSION: i32 = 49;
    pub const EFF_CAN_DO: i32 = 51;
    pub const EFF_GET_VST_VERSION: i32 = 58;
    pub const EFF_SHELL_GET_NEXT_PLUGIN: i32 = 70;
    pub const EFF_START_PROCESS: i32 = 71;
    pub const EFF_STOP_PROCESS: i32 = 72;
    pub const EFF_SET_PROCESS_PRECISION: i32 = 77;

    // Requests to the host.
    pub const HOST_AUTOMATE: i32 = 0;
    pub const HOST_VERSION: i32 = 1;
    pub const HOST_CURRENT_ID: i32 = 2;
    pub const HOST_IDLE: i32 = 3;
    pub const HOST_WANT_MIDI: i32 = 6;
    pub const HOST_GET_TIME: i32 = 7;
    pub const HOST_PROCESS_EVENTS: i32 = 8;
    pub const HOST_IO_CHANGED: i32 = 13;
    pub const HOST_NEED_IDLE: i32 = 14;
    pub const HOST_SIZE_WINDOW: i32 = 15;
    pub const HOST_GET_SAMPLE_RATE: i32 = 16;
    pub const HOST_GET_BLOCK_SIZE: i32 = 17;
    pub const HOST_GET_INPUT_LATENCY: i32 = 18;
    pub const HOST_GET_OUTPUT_LATENCY: i32 = 19;
    pub const HOST_GET_CURRENT_PROCESS_LEVEL: i32 = 23;
    pub const HOST_GET_AUTOMATION_STATE: i32 = 24;
    pub const HOST_GET_VENDOR_STRING: i32 = 32;
    pub const HOST_GET_PRODUCT_STRING: i32 = 33;
    pub const HOST_GET_VENDOR_VERSION: i32 = 34;
    pub const HOST_CAN_DO: i32 = 37;
    pub const HOST_GET_LANGUAGE: i32 = 38;
    pub const HOST_GET_DIRECTORY: i32 = 41;
    pub const HOST_UPDATE_DISPLAY: i32 = 42;
    pub const HOST_BEGIN_EDIT: i32 = 43;
    pub const HOST_END_EDIT: i32 = 44;

    // Process levels.
    pub const LEVEL_USER: isize = 1;
    pub const LEVEL_REALTIME: isize = 2;
    pub const LEVEL_OFFLINE: isize = 4;

    // Plugin categories.
    pub const CATEGORY_EFFECT: isize = 1;
    pub const CATEGORY_SYNTH: isize = 2;
    pub const CATEGORY_ANALYSIS: isize = 3;
    pub const CATEGORY_MASTERING: isize = 4;
    pub const CATEGORY_SPATIAL: isize = 5;
    pub const CATEGORY_ROOM: isize = 6;
    pub const CATEGORY_SURROUND: isize = 7;
    pub const CATEGORY_RESTORATION: isize = 8;
    pub const CATEGORY_OFFLINE: isize = 9;
    pub const CATEGORY_SHELL: isize = 10;
    pub const CATEGORY_GENERATOR: isize = 11;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Rect {
        pub top: i16,
        pub left: i16,
        pub bottom: i16,
        pub right: i16,
    }

    pub const MIDI_TYPE: i32 = 1;

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct MidiEvent {
        pub kind: i32,
        pub byte_size: i32,
        pub delta_frames: i32,
        pub flags: i32,
        pub note_length: i32,
        pub note_offset: i32,
        pub data: [u8; 4],
        pub detune: i8,
        pub note_off_velocity: u8,
        pub reserved: [u8; 2],
    }

    /// The events of a block: a count, then as many pointers (a C array of any length).
    #[repr(C)]
    pub struct Events<const N: usize> {
        pub count: i32,
        pub reserved: isize,
        pub events: [*mut MidiEvent; N],
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    pub struct TimeInfo {
        pub sample_pos: f64,
        pub sample_rate: f64,
        pub nano_seconds: f64,
        pub ppq_pos: f64,
        pub tempo: f64,
        pub bar_start_pos: f64,
        pub cycle_start_pos: f64,
        pub cycle_end_pos: f64,
        pub time_sig_numerator: i32,
        pub time_sig_denominator: i32,
        pub smpte_offset: i32,
        pub smpte_frame_rate: i32,
        pub samples_to_next_clock: i32,
        pub flags: i32,
    }

    pub const TIME_CHANGED: i32 = 1;
    pub const TIME_PLAYING: i32 = 1 << 1;
    pub const TIME_PPQ_VALID: i32 = 1 << 9;
    pub const TIME_TEMPO_VALID: i32 = 1 << 10;
    pub const TIME_BARS_VALID: i32 = 1 << 11;
    pub const TIME_SIG_VALID: i32 = 1 << 13;
}

use abi::AEffect;

const MAX_EVENTS: usize = 512;
/// Plugins write names longer than the 8 or 24 characters the interface allowed.
const TEXT: usize = 256;

thread_local! {
    /// The plugin a shell library is asked to create while its entry point runs.
    static WANTED: Cell<i32> = const { Cell::new(0) };
    /// In a plugin's audio processing, on this thread.
    static PROCESSING: Cell<bool> = const { Cell::new(false) };
}

/// The host's side of an instance, which the plugin reaches through `AEffect::user`.
#[derive(Default)]
struct Host {
    /// The plugin this instance was created as (a shell's own plugins ask for it).
    id: AtomicU32,
    sample_rate: AtomicU32,
    block_size: AtomicU32,
    offline: AtomicBool,
    /// Where the song is, written by the audio side before each block.
    time: UnsafeCell<abi::TimeInfo>,
    /// Values the plugin set itself (its window, a program), for the host to record.
    automated: Mutex<Vec<(u32, f64)>>,
    restart: AtomicBool,
    rescan: AtomicBool,
    /// Size its editor asked for, packed as width << 32 | height.
    resize: AtomicU64,
}

// SAFETY: `time` is written by the audio thread alone and read by the plugin, as with every VST2
// host; everything else is atomic or locked.
unsafe impl Sync for Host {}

impl Host {
    fn automate(&self, index: i32, value: f32) {
        if index < 0 {
            return;
        }
        if let Ok(mut automated) = self.automated.lock() {
            match automated.iter_mut().find(|(i, _)| *i == index as u32) {
                Some(held) => held.1 = f64::from(value),
                None => automated.push((index as u32, f64::from(value))),
            }
        }
    }
}

/// Writes `text` as a C string in a buffer of `size` bytes from the plugin.
///
/// # Safety
/// `ptr` must be null or point to `size` writable bytes.
unsafe fn write_text(ptr: *mut c_void, size: usize, text: &str) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let n = text.len().min(size - 1);
    // SAFETY: the caller vouches for `size` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr as *mut u8, n);
        *(ptr as *mut u8).add(n) = 0;
    }
}

/// What the host can do, when a plugin asks.
fn host_can_do(what: &str) -> isize {
    match what {
        "sendVstEvents" | "sendVstMidiEvent" | "sendVstTimeInfo" | "sizeWindow"
        | "startStopProcess" | "shellCategory" | "supplyIdle" => 1,
        _ => 0,
    }
}

/// The plugin's requests to the host.
unsafe extern "C" fn host_callback(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    // SAFETY: the plugin passes itself or null (while its entry point runs); `user` is null or
    // our `Host`, alive while the plugin is (see `Vst2Instance`).
    let host = unsafe {
        effect
            .as_ref()
            .and_then(|e| (e.user as *const Host).as_ref())
    };
    match opcode {
        abi::HOST_AUTOMATE => {
            if let Some(host) = host {
                host.automate(index, opt);
            }
            0
        }
        abi::HOST_VERSION => 2400,
        abi::HOST_CURRENT_ID => match host {
            Some(host) => host.id.load(Ordering::Relaxed) as i32 as isize,
            None => WANTED.with(Cell::get) as isize,
        },
        abi::HOST_IDLE | abi::HOST_PROCESS_EVENTS => 0,
        abi::HOST_WANT_MIDI | abi::HOST_NEED_IDLE => 1,
        abi::HOST_GET_TIME => host.map_or(0, |h| h.time.get() as isize),
        abi::HOST_IO_CHANGED => {
            if let Some(host) = host {
                host.restart.store(true, Ordering::Relaxed);
            }
            1
        }
        abi::HOST_SIZE_WINDOW => {
            if let Some(host) = host
                && index > 0
                && value > 0
            {
                let packed = (u64::from(index as u32) << 32) | (value as u64 & 0xFFFF_FFFF);
                host.resize.store(packed, Ordering::Relaxed);
            }
            1
        }
        abi::HOST_GET_SAMPLE_RATE => {
            host.map_or(48_000, |h| h.sample_rate.load(Ordering::Relaxed)) as isize
        }
        abi::HOST_GET_BLOCK_SIZE => {
            host.map_or(1024, |h| h.block_size.load(Ordering::Relaxed)) as isize
        }
        abi::HOST_GET_INPUT_LATENCY | abi::HOST_GET_OUTPUT_LATENCY => 0,
        abi::HOST_GET_CURRENT_PROCESS_LEVEL => {
            if host.is_some_and(|h| h.offline.load(Ordering::Relaxed)) {
                abi::LEVEL_OFFLINE
            } else if PROCESSING.with(Cell::get) {
                abi::LEVEL_REALTIME
            } else {
                abi::LEVEL_USER
            }
        }
        // Automation off: the plugin's values are its own.
        abi::HOST_GET_AUTOMATION_STATE => 1,
        abi::HOST_GET_VENDOR_STRING | abi::HOST_GET_PRODUCT_STRING => {
            // SAFETY: these strings are at most 64 bytes, as the interface sets.
            unsafe { write_text(ptr, 64, "AudioStrada") };
            1
        }
        abi::HOST_GET_VENDOR_VERSION => 1,
        abi::HOST_CAN_DO => {
            if ptr.is_null() {
                return 0;
            }
            // SAFETY: the plugin passes a C string.
            let what = unsafe { CStr::from_ptr(ptr as *const c_char) };
            host_can_do(&what.to_string_lossy())
        }
        // English.
        abi::HOST_GET_LANGUAGE => 1,
        abi::HOST_GET_DIRECTORY => 0,
        abi::HOST_UPDATE_DISPLAY => {
            if let Some(host) = host {
                host.rescan.store(true, Ordering::Relaxed);
            }
            1
        }
        abi::HOST_BEGIN_EDIT | abi::HOST_END_EDIT => 1,
        _ => 0,
    }
}

/// Libraries loaded, kept for the life of the process: plugins rarely survive being unloaded.
static LIBRARIES: Mutex<Vec<(PathBuf, &'static libloading::Library)>> = Mutex::new(Vec::new());

/// The file holding a plugin's code: the library itself, or the one inside a macOS bundle.
fn library_path(path: &Path) -> PathBuf {
    if cfg!(target_os = "macos") && path.is_dir() {
        let macos = path.join("Contents").join("MacOS");
        if let Some(file) = std::fs::read_dir(&macos)
            .ok()
            .and_then(|mut entries| entries.find_map(|e| e.ok().map(|e| e.path())))
        {
            return file;
        }
    }
    path.to_path_buf()
}

fn library(path: &Path) -> Result<&'static libloading::Library, String> {
    let mut libraries = LIBRARIES
        .lock()
        .map_err(|_| "plugin library list poisoned")?;
    if let Some((_, library)) = libraries.iter().find(|(p, _)| p == path) {
        return Ok(library);
    }
    let file = library_path(path);
    crate::check_binary(&file)?;
    // SAFETY: loading a plugin library runs its initialisers, as in any host.
    let library = unsafe { libloading::Library::new(&file) }
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let library: &'static libloading::Library = Box::leak(Box::new(library));
    libraries.push((path.to_path_buf(), library));
    Ok(library)
}

/// The library's entry point, if it is a VST2 plugin.
fn entry(library: &libloading::Library) -> Option<abi::Entry> {
    let names: &[&[u8]] = if cfg!(target_os = "macos") {
        &[b"VSTPluginMain\0", b"main_macho\0", b"main\0"]
    } else {
        &[b"VSTPluginMain\0", b"main\0"]
    };
    names.iter().find_map(|name| {
        // SAFETY: the symbol has the VST2 entry point's type.
        unsafe { library.get::<abi::Entry>(*name) }.ok().map(|f| *f)
    })
}

/// Creates a plugin from its library; `wanted` picks one of a shell's plugins (0: the first).
fn create(path: &Path, wanted: i32) -> Result<*mut AEffect, String> {
    let library = library(path)?;
    let entry = entry(library).ok_or("not a VST2 plugin")?;
    WANTED.with(|w| w.set(wanted));
    // SAFETY: calling the plugin's entry point with our callback, as the interface specifies.
    let effect = unsafe { entry(host_callback) };
    WANTED.with(|w| w.set(0));
    // SAFETY: the plugin returns its structure or null.
    match unsafe { effect.as_ref() } {
        None => Err("the plugin refused to start".into()),
        Some(e) if e.magic != abi::MAGIC => Err("the library returned no VST2 plugin".into()),
        Some(e) if e.dispatcher.is_none() => Err("the plugin has no dispatcher".into()),
        Some(_) => Ok(effect),
    }
}

/// Sends a command to the plugin.
///
/// # Safety
/// `effect` must be a live plugin, `ptr` what the command expects.
unsafe fn dispatch(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    opt: f32,
) -> isize {
    // SAFETY: the caller vouches for the plugin; `create` checked the dispatcher.
    unsafe {
        match (*effect).dispatcher {
            Some(dispatcher) => dispatcher(effect, opcode, index, value, ptr, opt),
            None => 0,
        }
    }
}

/// A text the plugin writes into a buffer for it.
///
/// # Safety
/// As for [`dispatch`].
unsafe fn text(effect: *mut AEffect, opcode: i32, index: i32) -> String {
    let mut buffer = [0u8; TEXT];
    // SAFETY: the buffer is larger than any the interface allowed.
    unsafe {
        dispatch(
            effect,
            opcode,
            index,
            0,
            buffer.as_mut_ptr() as *mut c_void,
            0.0,
        )
    };
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(TEXT);
    String::from_utf8_lossy(&buffer[..end]).trim().to_owned()
}

fn category_tags(category: isize, synth: bool) -> Vec<String> {
    let tags: &[&str] = match category {
        _ if synth => &["Instrument"],
        abi::CATEGORY_SYNTH => &["Instrument"],
        abi::CATEGORY_ANALYSIS => &["Fx", "Analyzer"],
        abi::CATEGORY_MASTERING => &["Fx", "Mastering"],
        abi::CATEGORY_SPATIAL => &["Fx", "Spatial"],
        abi::CATEGORY_ROOM => &["Fx", "Reverb"],
        abi::CATEGORY_SURROUND => &["Fx", "Surround"],
        abi::CATEGORY_RESTORATION => &["Fx", "Restoration"],
        abi::CATEGORY_GENERATOR => &["Generator"],
        _ => &["Fx"],
    };
    tags.iter().map(|t| (*t).to_owned()).collect()
}

/// A plugin's version as people read it. Most pack it a byte per number (`0x050C0153` is
/// 5.12.1.83, `0x10500` 1.5.0); older ones write its digits (`1000` is 1.0.0.0).
fn version_text(version: isize) -> String {
    if version <= 0 {
        return String::new();
    }
    if version < 0x1_0000 {
        let digits: Vec<String> = version.to_string().chars().map(String::from).collect();
        return digits.join(".");
    }
    let v = version as u32;
    let numbers = [v >> 24, (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF];
    let start = usize::from(numbers[0] == 0);
    let mut parts: Vec<String> = numbers[start..].iter().map(u32::to_string).collect();
    if start == 0 && numbers[3] == 0 {
        parts.pop();
    }
    parts.join(".")
}

/// Identifier of a VST2 plugin in projects: its unique id, in hex.
fn hex_id(id: i32) -> String {
    format!("{:08X}", id as u32)
}

/// Lists the plugins of a library: one, or those of a shell. A library that is not a VST2
/// plugin (a VST folder holds their helper libraries too) has none.
pub fn scan(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    #[cfg(windows)]
    if !pe::exports_entry(&library_path(path))? {
        return Ok(Vec::new());
    }
    let library = library(path)?;
    if entry(library).is_none() {
        return Ok(Vec::new());
    }
    let effect = create(path, 0)?;
    // SAFETY: the plugin was just created; commands follow the interface.
    unsafe {
        dispatch(effect, abi::EFF_OPEN, 0, 0, null_mut(), 0.0);
        let flags = (*effect).flags;
        let category = dispatch(effect, abi::EFF_GET_PLUG_CATEGORY, 0, 0, null_mut(), 0.0);
        let vendor = text(effect, abi::EFF_GET_VENDOR_STRING, 0);
        let version = dispatch(effect, abi::EFF_GET_VENDOR_VERSION, 0, 0, null_mut(), 0.0);
        let version = version_text(version);
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut out = Vec::new();
        if category == abi::CATEGORY_SHELL {
            loop {
                let mut name = [0u8; TEXT];
                let id = dispatch(
                    effect,
                    abi::EFF_SHELL_GET_NEXT_PLUGIN,
                    0,
                    0,
                    name.as_mut_ptr() as *mut c_void,
                    0.0,
                );
                if id == 0 || out.len() > 4096 {
                    break;
                }
                let end = name.iter().position(|&b| b == 0).unwrap_or(TEXT);
                let name = String::from_utf8_lossy(&name[..end]).trim().to_owned();
                out.push(PluginDescriptor {
                    format: Format::Vst2,
                    id: hex_id(id as i32),
                    name: if name.is_empty() {
                        hex_id(id as i32)
                    } else {
                        name
                    },
                    vendor: vendor.clone(),
                    version: version.clone(),
                    path: path.to_path_buf(),
                    // A shell says nothing of its plugins without creating each.
                    instrument: false,
                    tags: vec!["Fx".into()],
                });
            }
        } else {
            let name = [
                text(effect, abi::EFF_GET_EFFECT_NAME, 0),
                text(effect, abi::EFF_GET_PRODUCT_STRING, 0),
            ]
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or(stem);
            let synth = flags & abi::FLAG_IS_SYNTH != 0 || category == abi::CATEGORY_SYNTH;
            out.push(PluginDescriptor {
                format: Format::Vst2,
                id: hex_id((*effect).unique_id),
                name,
                vendor,
                version,
                path: path.to_path_buf(),
                instrument: synth,
                tags: category_tags(category, synth),
            });
        }
        dispatch(effect, abi::EFF_CLOSE, 0, 0, null_mut(), 0.0);
        Ok(out)
    }
}

/// Main-thread side of a VST2 plugin.
pub struct Vst2Instance {
    descriptor: PluginDescriptor,
    effect: *mut AEffect,
    host: Arc<Host>,
    active: bool,
    processor_alive: Arc<AtomicBool>,
    editor: Option<HostWindow>,
    /// Values when it was created: the parameters' defaults.
    defaults: Vec<f32>,
}

impl std::fmt::Debug for Vst2Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst2Instance")
            .field("name", &self.descriptor.name)
            .finish_non_exhaustive()
    }
}

impl Vst2Instance {
    pub fn new(descriptor: &PluginDescriptor) -> Result<Self, String> {
        let wanted = u32::from_str_radix(&descriptor.id, 16).map_err(|_| "invalid VST2 id")? as i32;
        let effect = create(&descriptor.path, wanted)?;
        let host = Arc::new(Host::default());
        host.id.store(wanted as u32, Ordering::Relaxed);
        host.sample_rate.store(48_000, Ordering::Relaxed);
        host.block_size.store(1024, Ordering::Relaxed);
        // SAFETY: the plugin was just created; `user` is the host's field, the host lives as
        // long as the plugin (freed after it is closed).
        unsafe {
            (*effect).user = Arc::into_raw(Arc::clone(&host)) as *mut c_void;
            dispatch(effect, abi::EFF_OPEN, 0, 0, null_mut(), 0.0);
        }
        let mut instance = Self {
            descriptor: descriptor.clone(),
            effect,
            host,
            active: false,
            processor_alive: Arc::new(AtomicBool::new(false)),
            editor: None,
            defaults: Vec::new(),
        };
        instance.defaults = (0..instance.param_count())
            .map(|i| instance.get(i))
            .collect();
        Ok(instance)
    }

    fn flags(&self) -> i32 {
        // SAFETY: the plugin is alive while the instance is.
        unsafe { (*self.effect).flags }
    }

    fn param_count(&self) -> i32 {
        // SAFETY: as above.
        unsafe { (*self.effect).num_params.max(0) }
    }

    fn get(&self, index: i32) -> f32 {
        // SAFETY: as above, with an index below the parameter count.
        unsafe {
            match (*self.effect).get_parameter {
                Some(get) => get(self.effect, index),
                None => 0.0,
            }
        }
    }

    fn set(&self, index: i32, value: f32) {
        // SAFETY: as above.
        unsafe {
            if let Some(set) = (*self.effect).set_parameter {
                set(self.effect, index, value);
            }
        }
    }

    fn dispatch(&self, opcode: i32, index: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
        // SAFETY: the plugin is alive while the instance is.
        unsafe { dispatch(self.effect, opcode, index, value, ptr, opt) }
    }

    /// Size of the plugin's editor, if it says.
    fn editor_size(&self) -> Option<(u32, u32)> {
        let mut rect: *mut abi::Rect = null_mut();
        self.dispatch(
            abi::EFF_EDIT_GET_RECT,
            0,
            0,
            &mut rect as *mut *mut abi::Rect as *mut c_void,
            0.0,
        );
        // SAFETY: the plugin points to its own rectangle, or leaves null.
        let rect = unsafe { rect.as_ref() }?;
        let (w, h) = (
            i32::from(rect.right) - i32::from(rect.left),
            i32::from(rect.bottom) - i32::from(rect.top),
        );
        (w > 0 && h > 0).then_some((w as u32, h as u32))
    }
}

/// Plugin state saved as its own chunk (`AS2C`) or as its program and parameter values
/// (`AS2P`), for plugins without chunks.
const CHUNK: &[u8; 4] = b"AS2C";
const VALUES: &[u8; 4] = b"AS2P";

impl crate::PluginInstance for Vst2Instance {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String> {
        let max = max_frames.max(1);
        self.host.sample_rate.store(sample_rate, Ordering::Relaxed);
        self.host.block_size.store(max as u32, Ordering::Relaxed);
        // SAFETY: plain data, written before the audio side exists.
        unsafe {
            let time = &mut *self.host.time.get();
            time.sample_rate = f64::from(sample_rate);
            time.tempo = 120.0;
            time.time_sig_numerator = 4;
            time.time_sig_denominator = 4;
            time.flags = abi::TIME_TEMPO_VALID | abi::TIME_SIG_VALID;
        }
        self.dispatch(
            abi::EFF_SET_SAMPLE_RATE,
            0,
            0,
            null_mut(),
            sample_rate as f32,
        );
        self.dispatch(abi::EFF_SET_BLOCK_SIZE, 0, max as isize, null_mut(), 0.0);
        self.dispatch(abi::EFF_SET_PROCESS_PRECISION, 0, 0, null_mut(), 0.0);
        self.dispatch(abi::EFF_MAINS_CHANGED, 0, 1, null_mut(), 0.0);
        self.dispatch(abi::EFF_START_PROCESS, 0, 0, null_mut(), 0.0);
        self.active = true;
        self.processor_alive.store(true, Ordering::Relaxed);
        // SAFETY: the plugin is alive.
        let e = unsafe { &*self.effect };
        let (inputs, outputs) = (e.num_inputs.max(0) as usize, e.num_outputs.max(0) as usize);
        // Replacing its outputs when it says it can (or has nothing else), else the old
        // accumulating `process`.
        let (process, accumulating) = match (e.process_replacing, e.process) {
            (Some(replacing), _) if e.flags & abi::FLAG_CAN_REPLACING != 0 => (replacing, false),
            (_, Some(process)) => (process, true),
            (Some(replacing), None) => (replacing, false),
            (None, None) => return Err("the plugin cannot process audio".into()),
        };
        let mut buffers_in: Vec<Vec<f32>> = vec![vec![0.0; max]; inputs];
        let mut buffers_out: Vec<Vec<f32>> = vec![vec![0.0; max]; outputs];
        let in_ptrs = buffers_in.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let out_ptrs = buffers_out.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let mut midi = vec![abi::MidiEvent::default(); MAX_EVENTS].into_boxed_slice();
        let mut events = Box::new(abi::Events::<MAX_EVENTS> {
            count: 0,
            reserved: 0,
            events: [null_mut(); MAX_EVENTS],
        });
        for (slot, event) in events.events.iter_mut().zip(midi.iter_mut()) {
            *slot = event;
        }
        Ok(Box::new(Vst2Processor {
            effect: self.effect,
            host: Arc::clone(&self.host),
            process,
            accumulating,
            buffers_in,
            buffers_out,
            in_ptrs,
            out_ptrs,
            midi,
            events,
            alive: Arc::clone(&self.processor_alive),
            playing: false,
        }))
    }

    fn try_deactivate(&mut self) -> bool {
        if self.processor_alive.load(Ordering::Relaxed) {
            return false;
        }
        if self.active {
            self.dispatch(abi::EFF_STOP_PROCESS, 0, 0, null_mut(), 0.0);
            self.dispatch(abi::EFF_MAINS_CHANGED, 0, 0, null_mut(), 0.0);
            self.active = false;
        }
        true
    }

    fn params(&mut self) -> Vec<ParamDesc> {
        (0..self.param_count())
            .map(|i| ParamDesc {
                id: i as u32,
                // SAFETY: the plugin is alive, the index below its parameter count.
                name: unsafe { text(self.effect, abi::EFF_GET_PARAM_NAME, i) },
                module: String::new(),
                min: 0.0,
                max: 1.0,
                default: f64::from(self.defaults.get(i as usize).copied().unwrap_or(0.0)),
                stepped: false,
                hidden: false,
                read_only: false,
            })
            .collect()
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        ((id as i32) < self.param_count()).then(|| f64::from(self.get(id as i32)))
    }

    /// The plugin only shows its current value: other values have no text.
    fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
        let index = id as i32;
        if index >= self.param_count() || (f64::from(self.get(index)) - value).abs() > 1e-5 {
            return None;
        }
        // SAFETY: the plugin is alive, the index below its parameter count.
        let (display, label) = unsafe {
            (
                text(self.effect, abi::EFF_GET_PARAM_DISPLAY, index),
                text(self.effect, abi::EFF_GET_PARAM_LABEL, index),
            )
        };
        Some(if label.is_empty() {
            display
        } else {
            format!("{display} {label}")
        })
    }

    fn latency(&mut self) -> u32 {
        // SAFETY: the plugin is alive.
        unsafe { (*self.effect).initial_delay.max(0) as u32 }
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        if self.flags() & abi::FLAG_PROGRAM_CHUNKS != 0 {
            let mut data: *mut u8 = null_mut();
            // Index 0: the whole bank, every program.
            let size = self.dispatch(
                abi::EFF_GET_CHUNK,
                0,
                0,
                &mut data as *mut *mut u8 as *mut c_void,
                0.0,
            );
            if size > 0 && !data.is_null() {
                let mut out = CHUNK.to_vec();
                // SAFETY: the plugin points to `size` bytes it keeps until the next call.
                out.extend_from_slice(unsafe { std::slice::from_raw_parts(data, size as usize) });
                return Some(out);
            }
        }
        let program = self.dispatch(abi::EFF_GET_PROGRAM, 0, 0, null_mut(), 0.0) as i32;
        let mut out = VALUES.to_vec();
        out.extend_from_slice(&program.to_le_bytes());
        for i in 0..self.param_count() {
            out.extend_from_slice(&self.get(i).to_le_bytes());
        }
        Some(out)
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        if let Some(chunk) = data.strip_prefix(CHUNK) {
            let mut chunk = chunk.to_vec();
            self.dispatch(
                abi::EFF_SET_CHUNK,
                0,
                chunk.len() as isize,
                chunk.as_mut_ptr() as *mut c_void,
                0.0,
            );
            return true;
        }
        let Some(rest) = data.strip_prefix(VALUES) else {
            return false;
        };
        let Some((program, values)) = rest.split_first_chunk::<4>() else {
            return false;
        };
        self.dispatch(
            abi::EFF_SET_PROGRAM,
            0,
            i32::from_le_bytes(*program) as isize,
            null_mut(),
            0.0,
        );
        for (i, value) in values.as_chunks::<4>().0.iter().enumerate() {
            if (i as i32) < self.param_count() {
                self.set(i as i32, f32::from_le_bytes(*value));
            }
        }
        true
    }

    fn poll(&mut self) -> Vec<PluginRequest> {
        let mut requests = Vec::new();
        if self.host.restart.swap(false, Ordering::Relaxed) {
            requests.push(PluginRequest::Restart);
        }
        if self.host.rescan.swap(false, Ordering::Relaxed) {
            requests.push(PluginRequest::RescanParams);
        }
        // Without an audio side to report them, the values the plugin set are told here.
        if !self.processor_alive.load(Ordering::Relaxed)
            && let Ok(mut automated) = self.host.automated.lock()
        {
            requests.extend(
                automated
                    .drain(..)
                    .map(|(id, v)| PluginRequest::ParamChanged(id, v)),
            );
        }
        let mut closed = false;
        if self.editor.is_some() {
            self.dispatch(abi::EFF_EDIT_IDLE, 0, 0, null_mut(), 0.0);
        }
        if let Some(window) = &mut self.editor {
            let resize = self.host.resize.swap(0, Ordering::Relaxed);
            if resize != 0 {
                window.resize((resize >> 32) as u32, resize as u32);
            }
            closed = window.poll();
            requests.extend(
                window
                    .take_keys()
                    .into_iter()
                    .map(|(sym, mods)| PluginRequest::Key(sym, mods)),
            );
        }
        if closed {
            self.close_gui();
            requests.push(PluginRequest::GuiClosed);
        }
        requests
    }

    fn has_gui(&mut self) -> bool {
        self.flags() & abi::FLAG_HAS_EDITOR != 0
    }

    fn open_gui(&mut self, title: &str) -> Result<(), String> {
        if self.editor.is_some() {
            return Ok(());
        }
        if !self.has_gui() {
            return Err("this plugin has no editor".into());
        }
        let (width, height) = self.editor_size().unwrap_or((640, 480));
        let mut window = HostWindow::new(title, width, height)?;
        let parent = match window.handle() {
            NativeHandle::X11(id) => id as usize as *mut c_void,
            NativeHandle::Win32(hwnd) | NativeHandle::Cocoa(hwnd) => hwnd,
        };
        self.dispatch(abi::EFF_EDIT_OPEN, 0, 0, parent, 0.0);
        // Some plugins know their size once their editor is open.
        if let Some((w, h)) = self.editor_size()
            && (w, h) != (width, height)
        {
            window.resize(w, h);
        }
        self.editor = Some(window);
        Ok(())
    }

    fn close_gui(&mut self) {
        // The editor leaves the window before the window goes.
        if let Some(window) = self.editor.take() {
            self.dispatch(abi::EFF_EDIT_CLOSE, 0, 0, null_mut(), 0.0);
            drop(window);
        }
    }

    fn gui_open(&self) -> bool {
        self.editor.is_some()
    }

    fn raise_gui(&mut self) {
        if let Some(window) = &mut self.editor {
            window.raise();
        }
    }
}

impl Drop for Vst2Instance {
    fn drop(&mut self) {
        crate::PluginInstance::close_gui(self);
        if !crate::PluginInstance::try_deactivate(self) {
            // The audio side still uses the plugin: leave it open rather than free it under it.
            return;
        }
        // SAFETY: closing frees the plugin; its `user` pointer is ours, released after.
        unsafe {
            let user = (*self.effect).user;
            dispatch(self.effect, abi::EFF_CLOSE, 0, 0, null_mut(), 0.0);
            if !user.is_null() {
                drop(Arc::from_raw(user as *const Host));
            }
        }
    }
}

/// Audio side of a VST2 plugin.
pub struct Vst2Processor {
    effect: *mut AEffect,
    host: Arc<Host>,
    process: abi::Process,
    /// The old `process`, which adds to its outputs.
    accumulating: bool,
    buffers_in: Vec<Vec<f32>>,
    buffers_out: Vec<Vec<f32>>,
    in_ptrs: Vec<*mut f32>,
    out_ptrs: Vec<*mut f32>,
    /// Events of a block, and the list pointing to them handed to the plugin.
    midi: Box<[abi::MidiEvent]>,
    events: Box<abi::Events<MAX_EVENTS>>,
    alive: Arc<AtomicBool>,
    playing: bool,
}

// SAFETY: the plugin is processed by one audio thread at a time, as VST2 hosts do; its main
// thread side only calls what the interface allows meanwhile.
unsafe impl Send for Vst2Processor {}

impl std::fmt::Debug for Vst2Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vst2Processor").finish_non_exhaustive()
    }
}

impl PluginProcessor for Vst2Processor {
    fn set_transport(&mut self, t: &crate::Transport) {
        let mut flags = abi::TIME_TEMPO_VALID
            | abi::TIME_PPQ_VALID
            | abi::TIME_BARS_VALID
            | abi::TIME_SIG_VALID;
        if t.playing {
            flags |= abi::TIME_PLAYING;
        }
        if t.playing != self.playing {
            flags |= abi::TIME_CHANGED;
            self.playing = t.playing;
        }
        // SAFETY: written by this audio thread alone (see `Host`).
        let time = unsafe { &mut *self.host.time.get() };
        time.sample_pos = t.samples as f64;
        time.ppq_pos = t.beats;
        time.bar_start_pos = t.bar_start;
        time.tempo = t.tempo;
        time.time_sig_numerator = t.numerator as i32;
        time.time_sig_denominator = t.denominator as i32;
        time.flags = flags;
    }

    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        let max = self
            .buffers_out
            .first()
            .or(self.buffers_in.first())
            .map_or(left.len(), Vec::len);
        let n = left.len().min(right.len()).min(max);
        if n == 0 {
            return;
        }
        PROCESSING.with(|p| p.set(true));
        // SAFETY: the plugin is alive while its audio side is (see `Vst2Instance::drop`).
        unsafe {
            if let Some(set) = (*self.effect).set_parameter {
                for &(id, value) in params_in {
                    set(self.effect, id as i32, value as f32);
                }
            }
        }
        let count = midi.len().min(MAX_EVENTS);
        for (slot, e) in self.midi.iter_mut().zip(&midi[..count]) {
            *slot = abi::MidiEvent {
                kind: abi::MIDI_TYPE,
                byte_size: std::mem::size_of::<abi::MidiEvent>() as i32,
                delta_frames: crate::block_time(e, n) as i32,
                data: [e.data[0], e.data[1], e.data[2], 0],
                ..abi::MidiEvent::default()
            };
        }
        if count > 0 {
            self.events.count = count as i32;
            // SAFETY: the list and its events live through the call.
            unsafe {
                dispatch(
                    self.effect,
                    abi::EFF_PROCESS_EVENTS,
                    0,
                    0,
                    &mut *self.events as *mut abi::Events<MAX_EVENTS> as *mut c_void,
                    0.0,
                );
            }
        }
        crate::fill_inputs(&mut self.buffers_in, left, right, n);
        if self.accumulating {
            for buffer in &mut self.buffers_out {
                buffer[..n].fill(0.0);
            }
        }
        // SAFETY: the pointers lead to buffers of at least `n` frames, alive for the call.
        unsafe {
            (self.process)(
                self.effect,
                self.in_ptrs.as_mut_ptr(),
                self.out_ptrs.as_mut_ptr(),
                n as i32,
            );
        }
        PROCESSING.with(|p| p.set(false));
        if self.buffers_out.is_empty() {
            left[..n].fill(0.0);
            right[..n].fill(0.0);
        } else {
            crate::take_outputs(&self.buffers_out, left, right, n);
        }
        // SAFETY: as in `set_transport`.
        unsafe { (*self.host.time.get()).sample_pos += n as f64 };
        // What the plugin set itself (its window), recorded by the host; never waiting here.
        if let Ok(mut automated) = self.host.automated.try_lock() {
            for (id, value) in automated.drain(..) {
                params_out(id, value);
            }
        }
    }

    fn reset(&mut self) {}

    fn set_offline(&mut self, offline: bool) {
        self.host.offline.store(offline, Ordering::Relaxed);
    }
}

impl Drop for Vst2Processor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

/// Reading a Windows library's export table without loading it: VST folders hold helper
/// libraries besides plugins, and loading one runs its code.
#[cfg(windows)]
mod pe {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;

    fn read_at(file: &mut File, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buffer)
    }

    fn u16_at(b: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
    }

    fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    }

    /// Whether the library exports a VST2 entry point. A plugin for another processor (a
    /// 32-bit one) is an error saying so.
    pub fn exports_entry(path: &Path) -> Result<bool, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut head = vec![0u8; 4096];
        let read = file.read(&mut head).map_err(|e| e.to_string())?;
        head.truncate(read);
        Ok(match parse(&mut file, &head) {
            Some(Exports {
                entry: true,
                machine,
            }) => {
                let native = if cfg!(target_arch = "aarch64") {
                    0xAA64
                } else {
                    0x8664
                };
                if machine != native {
                    return Err(if machine == 0x14C {
                        "32-bit plugin: AudioStrada runs 64-bit plugins".into()
                    } else {
                        "plugin built for another processor".into()
                    });
                }
                true
            }
            _ => false,
        })
    }

    struct Exports {
        entry: bool,
        machine: u16,
    }

    fn parse(file: &mut File, head: &[u8]) -> Option<Exports> {
        if head.get(..2)? != b"MZ" {
            return None;
        }
        let pe = u32_at(head, 0x3C)? as usize;
        if head.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let coff = pe + 4;
        let machine = u16_at(head, coff)?;
        let sections = u16_at(head, coff + 2)? as usize;
        let optional_size = u16_at(head, coff + 16)? as usize;
        let optional = coff + 20;
        let directories = match u16_at(head, optional)? {
            0x10B => optional + 96,
            0x20B => optional + 112,
            _ => return None,
        };
        let export_rva = u32_at(head, directories)?;
        if export_rva == 0 {
            return Some(Exports {
                entry: false,
                machine,
            });
        }
        // Sections: where each range of addresses is in the file.
        let table = optional + optional_size;
        let mut ranges = Vec::new();
        for i in 0..sections.min(96) {
            let s = table + i * 40;
            let size = u32_at(head, s + 8)?.max(u32_at(head, s + 16)?);
            ranges.push((u32_at(head, s + 12)?, size, u32_at(head, s + 20)?));
        }
        let offset = |rva: u32| -> Option<u64> {
            ranges
                .iter()
                .find(|(start, size, _)| rva >= *start && rva < start + size)
                .map(|(start, _, raw)| u64::from(rva - start + raw))
        };
        let mut directory = [0u8; 40];
        read_at(file, offset(export_rva)?, &mut directory).ok()?;
        let names = u32_at(&directory, 24)? as usize;
        let names_rva = u32_at(&directory, 32)?;
        let mut pointers = vec![0u8; names.min(65_536) * 4];
        read_at(file, offset(names_rva)?, &mut pointers).ok()?;
        let entry = pointers.as_chunks::<4>().0.iter().any(|p| {
            let rva = u32::from_le_bytes(*p);
            let mut name = [0u8; 16];
            offset(rva)
                .and_then(|at| {
                    // A name near the end of the file is shorter than the buffer.
                    file.seek(SeekFrom::Start(at)).ok()?;
                    file.read(&mut name).ok()
                })
                .is_some_and(|_| {
                    name.starts_with(b"VSTPluginMain\0") || name.starts_with(b"main\0")
                })
        });
        Some(Exports { entry, machine })
    }
}

/// Whether a library found in a VST folder may be a plugin, without loading it.
pub fn may_be_plugin(path: &Path) -> bool {
    #[cfg(windows)]
    {
        // A 32-bit plugin is kept, for its scan to say why it cannot load.
        !matches!(pe::exports_entry(path), Ok(false))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginInstance as _;

    #[test]
    fn versions_read_as_numbers() {
        assert_eq!(version_text(0x050C_0153), "5.12.1.83");
        assert_eq!(version_text(0x1_0500), "1.5.0");
        assert_eq!(version_text(0x4_0005), "4.0.5");
        assert_eq!(version_text(1000), "1.0.0.0");
        assert_eq!(version_text(200), "2.0.0");
        assert_eq!(version_text(0), "");
        assert_eq!(hex_id(0x4173_5467), "41735467");
        assert_eq!(hex_id(-1), "FFFFFFFF");
        assert_eq!(host_can_do("sendVstTimeInfo"), 1);
        assert_eq!(host_can_do("offline"), 0);
        assert_eq!(category_tags(abi::CATEGORY_ROOM, false), ["Fx", "Reverb"]);
        assert_eq!(category_tags(abi::CATEGORY_EFFECT, true), ["Instrument"]);
    }

    /// Real plugins, from `AUDIOSTRADA_TEST_VST2` (a folder of VST2 plugins): loaded, run with
    /// notes and parameter changes, their state saved and loaded back, their windows opened
    /// when `AUDIOSTRADA_TEST_VST2_GUI` is set. A shell's first plugins only.
    #[test]
    fn hosts_real_plugins_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_VST2") else {
            eprintln!("AUDIOSTRADA_TEST_VST2 not set, skipping");
            return;
        };
        let gui = std::env::var_os("AUDIOSTRADA_TEST_VST2_GUI").is_some();
        let files = crate::scan::find_files(Format::Vst2, &[dir.into()]);
        let mut hosted = 0;
        for file in files {
            let found = match scan(&file) {
                Ok(found) => found,
                Err(e) => {
                    eprintln!("{}: {e}", file.display());
                    continue;
                }
            };
            for descriptor in found.iter().take(2) {
                let mut instance = Vst2Instance::new(descriptor).expect("loaded");
                let params = crate::PluginInstance::params(&mut instance);
                let mut audio = instance.activate(48_000, 256).expect("activated");
                let first = params.first().map(|p| (p.id, p.default));
                let notes = [MidiEvent::note_on(10, 0, 57, 110)];
                let mut energy = 0.0f32;
                for block in 0..40 {
                    let (mut left, mut right) = (vec![0.1f32; 256], vec![0.1f32; 256]);
                    let midi: &[MidiEvent] = if block == 2 { &notes } else { &[] };
                    let changes: &[(u32, f64)] = if block == 3 { first.as_slice() } else { &[] };
                    audio.process(&mut left, &mut right, changes, midi, &mut |_, _| {});
                    assert!(
                        left.iter().chain(&right).all(|s| s.is_finite()),
                        "{}",
                        descriptor.name
                    );
                    energy += left.iter().chain(&right).map(|s| s * s).sum::<f32>();
                }
                if gui && instance.has_gui() {
                    instance.open_gui(&descriptor.name).expect("its window");
                    for _ in 0..20 {
                        instance.poll();
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                    instance.close_gui();
                }
                let state = instance.save_state().expect("a state");
                assert!(instance.load_state(&state), "{}", descriptor.name);
                drop(audio);
                assert!(instance.try_deactivate());
                eprintln!(
                    "hosted {} ({} params, state {} bytes, latency {}, energy {energy:.1})",
                    descriptor.name,
                    params.len(),
                    state.len(),
                    instance.latency()
                );
                hosted += 1;
            }
        }
        assert!(hosted > 0);
    }
}
