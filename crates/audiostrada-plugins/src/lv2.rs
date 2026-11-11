//! LV2 host. lilv is loaded at run time, so building needs no LV2 library; without it, LV2
//! plugins are reported as unavailable.
//!
//! Plugins get the URID map/unmap, options and bounded block length features; atom ports get
//! empty sequences. Control input ports are the parameters and their values are the state.
//! Plugin windows are X11 UIs (Windows UIs on Windows) using the float protocol.

mod ui;

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::{Format, MidiEvent, ParamDesc, PluginDescriptor, PluginProcessor, PluginRequest};

const ATOM_BUFFER: usize = 8192;

// lilv and LV2 C types.
type World = c_void;
type Node = c_void;
type Plugins = c_void;
type Plugin = c_void;
type Port = c_void;
type Iter = c_void;
type PluginClass = c_void;
type Uis = c_void;
type Ui = c_void;

#[repr(C)]
struct LilvInstance {
    descriptor: *const Lv2Descriptor,
    handle: *mut c_void,
    pimpl: *mut c_void,
}

#[repr(C)]
struct Lv2Descriptor {
    uri: *const c_char,
    instantiate: *const c_void,
    connect_port: unsafe extern "C" fn(*mut c_void, u32, *mut c_void),
    activate: Option<unsafe extern "C" fn(*mut c_void)>,
    run: unsafe extern "C" fn(*mut c_void, u32),
    deactivate: Option<unsafe extern "C" fn(*mut c_void)>,
    cleanup: unsafe extern "C" fn(*mut c_void),
    extension_data: Option<unsafe extern "C" fn(*const c_char) -> *const c_void>,
}

#[repr(C)]
struct Feature {
    uri: *const c_char,
    data: *mut c_void,
}

#[repr(C)]
struct UridMap {
    handle: *mut c_void,
    map: unsafe extern "C" fn(*mut c_void, *const c_char) -> u32,
}

#[repr(C)]
struct UridUnmap {
    handle: *mut c_void,
    unmap: unsafe extern "C" fn(*mut c_void, u32) -> *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct OptionEntry {
    context: u32,
    subject: u32,
    key: u32,
    size: u32,
    kind: u32,
    value: *const c_void,
}

macro_rules! lilv_api {
    ($($name:ident: fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        #[allow(non_snake_case, reason = "C function names")]
        struct Lilv {
            _library: libloading::Library,
            $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Lilv {
            fn load() -> Result<Self, String> {
                let names: &[&str] = if cfg!(target_os = "windows") {
                    &["lilv-0.dll"]
                } else if cfg!(target_os = "macos") {
                    &["liblilv-0.0.dylib", "liblilv-0.dylib"]
                } else {
                    &["liblilv-0.so.0", "liblilv-0.so"]
                };
                // SAFETY: lilv is a well-behaved C library; its symbols match the types below.
                let library = names
                    .iter()
                    // SAFETY: as above.
                    .find_map(|n| unsafe { libloading::Library::new(*n) }.ok())
                    .ok_or("the lilv library is not installed (needed for LV2 plugins)")?;
                // SAFETY: as above.
                unsafe {
                    Ok(Self {
                        $($name: *library
                            .get(concat!(stringify!($name), "\0").as_bytes())
                            .map_err(|e| e.to_string())?,)*
                        _library: library,
                    })
                }
            }
        }
    };
}

lilv_api! {
    lilv_world_new: fn() -> *mut World;
    lilv_world_load_all: fn(*mut World);
    lilv_world_load_bundle: fn(*mut World, *const Node);
    lilv_new_uri: fn(*mut World, *const c_char) -> *mut Node;
    lilv_new_file_uri: fn(*mut World, *const c_char, *const c_char) -> *mut Node;
    lilv_node_free: fn(*mut Node);
    lilv_node_as_uri: fn(*const Node) -> *const c_char;
    lilv_node_as_string: fn(*const Node) -> *const c_char;
    lilv_world_get_all_plugins: fn(*const World) -> *const Plugins;
    lilv_plugins_begin: fn(*const Plugins) -> *mut Iter;
    lilv_plugins_get: fn(*const Plugins, *const Iter) -> *const Plugin;
    lilv_plugins_next: fn(*const Plugins, *mut Iter) -> *mut Iter;
    lilv_plugins_is_end: fn(*const Plugins, *const Iter) -> bool;
    lilv_plugins_get_by_uri: fn(*const Plugins, *const Node) -> *const Plugin;
    lilv_plugin_get_uri: fn(*const Plugin) -> *const Node;
    lilv_plugin_get_bundle_uri: fn(*const Plugin) -> *const Node;
    lilv_plugin_get_name: fn(*const Plugin) -> *mut Node;
    lilv_plugin_get_author_name: fn(*const Plugin) -> *mut Node;
    lilv_plugin_get_class: fn(*const Plugin) -> *const PluginClass;
    lilv_plugin_class_get_label: fn(*const PluginClass) -> *const Node;
    lilv_plugin_get_num_ports: fn(*const Plugin) -> u32;
    lilv_plugin_get_port_by_index: fn(*const Plugin, u32) -> *const Port;
    lilv_plugin_get_port_ranges_float: fn(*const Plugin, *mut f32, *mut f32, *mut f32);
    lilv_port_is_a: fn(*const Plugin, *const Port, *const Node) -> bool;
    lilv_port_has_property: fn(*const Plugin, *const Port, *const Node) -> bool;
    lilv_port_get_name: fn(*const Plugin, *const Port) -> *mut Node;
    lilv_port_supports_event: fn(*const Plugin, *const Port, *const Node) -> bool;
    lilv_plugin_instantiate: fn(*const Plugin, f64, *const *const Feature) -> *mut LilvInstance;
    lilv_instance_free: fn(*mut LilvInstance);
    lilv_file_uri_parse: fn(*const c_char, *mut *mut c_char) -> *mut c_char;
    lilv_free: fn(*mut c_void);
    lilv_plugin_get_uis: fn(*const Plugin) -> *mut Uis;
    lilv_uis_free: fn(*mut Uis);
    lilv_uis_begin: fn(*const Uis) -> *mut Iter;
    lilv_uis_get: fn(*const Uis, *const Iter) -> *const Ui;
    lilv_uis_next: fn(*const Uis, *mut Iter) -> *mut Iter;
    lilv_uis_is_end: fn(*const Uis, *const Iter) -> bool;
    lilv_ui_get_uri: fn(*const Ui) -> *const Node;
    lilv_ui_is_a: fn(*const Ui, *const Node) -> bool;
    lilv_ui_get_bundle_uri: fn(*const Ui) -> *const Node;
    lilv_ui_get_binary_uri: fn(*const Ui) -> *const Node;
    lilv_world_load_resource: fn(*mut World, *const Node) -> i32;
}

/// The lilv world, loaded once; lilv is not thread-safe, so every use goes through the lock.
struct Host {
    lilv: Lilv,
    world: *mut World,
    loaded: Vec<PathBuf>,
    nodes: HashMap<&'static str, *mut Node>,
}

// SAFETY: every access goes through the HOST mutex.
unsafe impl Send for Host {}

static HOST: OnceLock<Result<Mutex<Host>, String>> = OnceLock::new();

fn host() -> Result<std::sync::MutexGuard<'static, Host>, String> {
    let host = HOST.get_or_init(|| {
        let lilv = Lilv::load()?;
        // SAFETY: plain lilv calls.
        let world = unsafe { (lilv.lilv_world_new)() };
        if world.is_null() {
            return Err("lilv failed to start".into());
        }
        // SAFETY: as above.
        unsafe { (lilv.lilv_world_load_all)(world) };
        Ok(Mutex::new(Host {
            lilv,
            world,
            loaded: Vec::new(),
            nodes: HashMap::new(),
        }))
    });
    match host {
        Ok(m) => m.lock().map_err(|_| "LV2 host poisoned".into()),
        Err(e) => Err(e.clone()),
    }
}

const AUDIO_PORT: &str = "http://lv2plug.in/ns/lv2core#AudioPort";
const CONTROL_PORT: &str = "http://lv2plug.in/ns/lv2core#ControlPort";
const INPUT_PORT: &str = "http://lv2plug.in/ns/lv2core#InputPort";
const ATOM_PORT: &str = "http://lv2plug.in/ns/ext/atom#AtomPort";
const TOGGLED: &str = "http://lv2plug.in/ns/lv2core#toggled";
const INTEGER: &str = "http://lv2plug.in/ns/lv2core#integer";
const ENUMERATION: &str = "http://lv2plug.in/ns/lv2core#enumeration";
const REPORTS_LATENCY: &str = "http://lv2plug.in/ns/lv2core#reportsLatency";
const NOT_ON_GUI: &str = "http://lv2plug.in/ns/ext/port-props#notOnGUI";
const MIDI_EVENT: &str = "http://lv2plug.in/ns/ext/midi#MidiEvent";

impl Host {
    fn node(&mut self, uri: &'static str) -> *const Node {
        let (lilv, world) = (&self.lilv, self.world);
        *self.nodes.entry(uri).or_insert_with(|| {
            let c = CString::new(uri).unwrap_or_default();
            // SAFETY: plain lilv call; the node lives as long as the world.
            unsafe { (lilv.lilv_new_uri)(world, c.as_ptr()) }
        })
    }

    fn load_bundle(&mut self, path: &Path) {
        if self.loaded.iter().any(|p| p == path) {
            return;
        }
        let Ok(c) = CString::new(path.to_string_lossy().as_bytes().to_vec()) else {
            return;
        };
        let mut c = c.into_bytes();
        if !c.ends_with(b"/") {
            c.push(b'/');
        }
        let c = CString::new(c).unwrap_or_default();
        // SAFETY: plain lilv calls on the world.
        unsafe {
            let uri = (self.lilv.lilv_new_file_uri)(self.world, std::ptr::null(), c.as_ptr());
            if !uri.is_null() {
                (self.lilv.lilv_world_load_bundle)(self.world, uri);
                (self.lilv.lilv_node_free)(uri);
            }
        }
        self.loaded.push(path.to_path_buf());
    }

    /// Takes a string out of an owned node and frees it.
    fn owned_string(&self, node: *mut Node) -> String {
        if node.is_null() {
            return String::new();
        }
        // SAFETY: the node is a valid owned lilv node.
        unsafe {
            let s = CStr::from_ptr((self.lilv.lilv_node_as_string)(node))
                .to_string_lossy()
                .into_owned();
            (self.lilv.lilv_node_free)(node);
            s
        }
    }

    fn string(&self, node: *const Node) -> String {
        if node.is_null() {
            return String::new();
        }
        // SAFETY: the node is a valid lilv node.
        unsafe {
            CStr::from_ptr((self.lilv.lilv_node_as_string)(node))
                .to_string_lossy()
                .into_owned()
        }
    }

    fn bundle_path(&self, plugin: *const Plugin) -> PathBuf {
        // SAFETY: plain lilv calls; the parsed path is freed with lilv_free.
        unsafe {
            let uri = (self.lilv.lilv_node_as_uri)((self.lilv.lilv_plugin_get_bundle_uri)(plugin));
            let path = (self.lilv.lilv_file_uri_parse)(uri, std::ptr::null_mut());
            if path.is_null() {
                return PathBuf::new();
            }
            let out = PathBuf::from(CStr::from_ptr(path).to_string_lossy().trim_end_matches('/'));
            (self.lilv.lilv_free)(path as *mut c_void);
            out
        }
    }

    fn plugins(&self) -> Vec<*const Plugin> {
        let mut out = Vec::new();
        // SAFETY: standard lilv iteration.
        unsafe {
            let all = (self.lilv.lilv_world_get_all_plugins)(self.world);
            let mut it = (self.lilv.lilv_plugins_begin)(all);
            while !(self.lilv.lilv_plugins_is_end)(all, it) {
                out.push((self.lilv.lilv_plugins_get)(all, it));
                it = (self.lilv.lilv_plugins_next)(all, it);
            }
        }
        out
    }

    fn find(&mut self, uri: &str) -> Option<*const Plugin> {
        let c = CString::new(uri).ok()?;
        // SAFETY: plain lilv calls; the temporary node is freed.
        unsafe {
            let node = (self.lilv.lilv_new_uri)(self.world, c.as_ptr());
            let all = (self.lilv.lilv_world_get_all_plugins)(self.world);
            let plugin = (self.lilv.lilv_plugins_get_by_uri)(all, node);
            (self.lilv.lilv_node_free)(node);
            (!plugin.is_null()).then_some(plugin)
        }
    }
}

/// Lists the plugins of a `.lv2` bundle.
pub fn scan(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let mut host = host()?;
    host.load_bundle(path);
    let wanted = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut out = Vec::new();
    for plugin in host.plugins() {
        let bundle = host.bundle_path(plugin);
        if bundle.canonicalize().unwrap_or(bundle) != wanted {
            continue;
        }
        // SAFETY: plain lilv calls on a plugin of the world.
        let (uri, name, author, class) = unsafe {
            (
                host.string((host.lilv.lilv_plugin_get_uri)(plugin)),
                host.owned_string((host.lilv.lilv_plugin_get_name)(plugin)),
                host.owned_string((host.lilv.lilv_plugin_get_author_name)(plugin)),
                host.string((host.lilv.lilv_plugin_class_get_label)((host
                    .lilv
                    .lilv_plugin_get_class)(
                    plugin
                ))),
            )
        };
        // Without the LV2 core spec installed, class labels are unknown: guess from the ports.
        let (audio, input, atom, midi) = (
            host.node(AUDIO_PORT),
            host.node(INPUT_PORT),
            host.node(ATOM_PORT),
            host.node(MIDI_EVENT),
        );
        let (mut audio_in, mut midi_in) = (false, false);
        // SAFETY: indices below the port count of a plugin of the world.
        unsafe {
            for i in 0..(host.lilv.lilv_plugin_get_num_ports)(plugin) {
                let port = (host.lilv.lilv_plugin_get_port_by_index)(plugin, i);
                let is = |n| (host.lilv.lilv_port_is_a)(plugin, port, n);
                if is(input) {
                    audio_in |= is(audio);
                    midi_in |= is(atom) && (host.lilv.lilv_port_supports_event)(plugin, port, midi);
                }
            }
        }
        out.push(PluginDescriptor {
            format: Format::Lv2,
            id: uri,
            name,
            vendor: author,
            version: String::new(),
            path: path.to_path_buf(),
            instrument: class.contains("Instrument") || (midi_in && !audio_in),
            tags: if class.is_empty() {
                Vec::new()
            } else {
                vec![class]
            },
        });
    }
    Ok(out)
}

/// URIDs shared by every LV2 instance of the process.
struct Urids {
    ids: HashMap<String, u32>,
    uris: Vec<CString>,
}

static URIDS: Mutex<Option<Urids>> = Mutex::new(None);

fn map_uri(uri: &str) -> u32 {
    let Ok(mut guard) = URIDS.lock() else {
        return 0;
    };
    let urids = guard.get_or_insert_with(|| Urids {
        ids: HashMap::new(),
        uris: Vec::new(),
    });
    if let Some(&id) = urids.ids.get(uri) {
        return id;
    }
    urids.uris.push(CString::new(uri).unwrap_or_default());
    let id = urids.uris.len() as u32;
    urids.ids.insert(uri.to_owned(), id);
    id
}

unsafe extern "C" fn urid_map(_handle: *mut c_void, uri: *const c_char) -> u32 {
    if uri.is_null() {
        return 0;
    }
    // SAFETY: plugins pass null-terminated URIs.
    map_uri(&unsafe { CStr::from_ptr(uri) }.to_string_lossy())
}

unsafe extern "C" fn urid_unmap(_handle: *mut c_void, urid: u32) -> *const c_char {
    let Ok(guard) = URIDS.lock() else {
        return std::ptr::null();
    };
    guard
        .as_ref()
        .and_then(|u| u.uris.get(urid.wrapping_sub(1) as usize))
        // URIs are never removed, so the pointer stays valid.
        .map_or(std::ptr::null(), |c| c.as_ptr())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PortKind {
    AudioIn,
    AudioOut,
    ControlIn,
    ControlOut,
    AtomIn,
    AtomOut,
    Other,
}

#[derive(Clone, Debug)]
struct PortInfo {
    index: u32,
    kind: PortKind,
    name: String,
    min: f32,
    max: f32,
    default: f32,
    stepped: bool,
    hidden: bool,
    latency: bool,
    /// Atom input accepting MIDI events.
    midi: bool,
}

const WORK_MESSAGE: usize = 1024;
const WORK_QUEUE: usize = 64;
const WORKER_INTERFACE: &CStr = c"http://lv2plug.in/ns/ext/worker#interface";

/// A request to or a response from the plugin's worker.
struct WorkMessage {
    len: usize,
    data: [u8; WORK_MESSAGE],
}

impl WorkMessage {
    fn new(size: u32, data: *const c_void) -> Option<Self> {
        let len = size as usize;
        if len > WORK_MESSAGE || (data.is_null() && len > 0) {
            return None;
        }
        let mut message = Self {
            len,
            data: [0; WORK_MESSAGE],
        };
        if len > 0 {
            // SAFETY: the plugin gives `size` readable bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(data as *const u8, message.data.as_mut_ptr(), len)
            };
        }
        Some(message)
    }
}

type WorkStatus = u32;
type Respond = unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> WorkStatus;

#[repr(C)]
struct WorkerSchedule {
    handle: *mut c_void,
    schedule_work: unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> WorkStatus,
}

#[repr(C)]
struct WorkerInterface {
    work: unsafe extern "C" fn(*mut c_void, Respond, *mut c_void, u32, *const c_void) -> WorkStatus,
    work_response: unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> WorkStatus,
    end_run: Option<unsafe extern "C" fn(*mut c_void) -> WorkStatus>,
}

/// Called by the plugin from `run` (schedule work) or `work` (respond): queues a message on
/// the sender `handle`, used by one thread only (the audio thread or the worker).
unsafe extern "C" fn send_work(handle: *mut c_void, size: u32, data: *const c_void) -> WorkStatus {
    // SAFETY: the handle is a sender owned by the calling thread's side.
    let Some(sender) = (unsafe { (handle as *mut audiostrada_rt::Sender<WorkMessage>).as_mut() })
    else {
        return 1;
    };
    let Some(message) = WorkMessage::new(size, data) else {
        return 2;
    };
    if sender.try_send(message).is_ok() {
        0
    } else {
        2
    }
}

/// Queues between the audio thread and the worker thread, until the worker starts.
struct WorkerQueues {
    requests: audiostrada_rt::Receiver<WorkMessage>,
    responses_tx: audiostrada_rt::Sender<WorkMessage>,
    responses_rx: audiostrada_rt::Receiver<WorkMessage>,
}

/// Features handed to the plugin, kept alive as long as the instance.
struct Features {
    _schedule: Box<WorkerSchedule>,
    _requests: Box<audiostrada_rt::Sender<WorkMessage>>,
    queues: Option<WorkerQueues>,
    _map: Box<UridMap>,
    _unmap: Box<UridUnmap>,
    _options: Box<[OptionEntry]>,
    _block: Box<[i32; 2]>,
    _rate: Box<f32>,
    _uris: Vec<CString>,
    _features: Vec<Feature>,
    pointers: Vec<*const Feature>,
}

fn features(rate: u32, max_frames: usize) -> Features {
    let uris: Vec<CString> = [
        "http://lv2plug.in/ns/ext/urid#map",
        "http://lv2plug.in/ns/ext/urid#unmap",
        "http://lv2plug.in/ns/ext/options#options",
        "http://lv2plug.in/ns/ext/buf-size#boundedBlockLength",
        "http://lv2plug.in/ns/ext/worker#schedule",
    ]
    .iter()
    .map(|u| CString::new(*u).unwrap_or_default())
    .collect();
    let (requests_tx, requests) = audiostrada_rt::channel(WORK_QUEUE);
    let (responses_tx, responses_rx) = audiostrada_rt::channel(WORK_QUEUE);
    let mut requests_tx = Box::new(requests_tx);
    let mut schedule = Box::new(WorkerSchedule {
        handle: &mut *requests_tx as *mut audiostrada_rt::Sender<WorkMessage> as *mut c_void,
        schedule_work: send_work,
    });
    let mut map = Box::new(UridMap {
        handle: std::ptr::null_mut(),
        map: urid_map,
    });
    let mut unmap = Box::new(UridUnmap {
        handle: std::ptr::null_mut(),
        unmap: urid_unmap,
    });
    let block = Box::new([1i32, max_frames.max(1) as i32]);
    let sample_rate = Box::new(rate as f32);
    let int = map_uri("http://lv2plug.in/ns/ext/atom#Int");
    let float = map_uri("http://lv2plug.in/ns/ext/atom#Float");
    let option = |key: &str, size: u32, kind: u32, value: *const c_void| OptionEntry {
        context: 0,
        subject: 0,
        key: map_uri(key),
        size,
        kind,
        value,
    };
    let mut options: Box<[OptionEntry]> = vec![
        option(
            "http://lv2plug.in/ns/ext/buf-size#minBlockLength",
            4,
            int,
            &block[0] as *const i32 as *const c_void,
        ),
        option(
            "http://lv2plug.in/ns/ext/buf-size#maxBlockLength",
            4,
            int,
            &block[1] as *const i32 as *const c_void,
        ),
        option(
            "http://lv2plug.in/ns/ext/buf-size#nominalBlockLength",
            4,
            int,
            &block[1] as *const i32 as *const c_void,
        ),
        option(
            "http://lv2plug.in/ns/ext/parameters#sampleRate",
            4,
            float,
            &*sample_rate as *const f32 as *const c_void,
        ),
        OptionEntry {
            context: 0,
            subject: 0,
            key: 0,
            size: 0,
            kind: 0,
            value: std::ptr::null(),
        },
    ]
    .into_boxed_slice();
    let feature_list = vec![
        Feature {
            uri: uris[0].as_ptr(),
            data: &mut *map as *mut UridMap as *mut c_void,
        },
        Feature {
            uri: uris[1].as_ptr(),
            data: &mut *unmap as *mut UridUnmap as *mut c_void,
        },
        Feature {
            uri: uris[2].as_ptr(),
            data: options.as_mut_ptr() as *mut c_void,
        },
        Feature {
            uri: uris[3].as_ptr(),
            data: std::ptr::null_mut(),
        },
        Feature {
            uri: uris[4].as_ptr(),
            data: &mut *schedule as *mut WorkerSchedule as *mut c_void,
        },
    ];
    let mut pointers: Vec<*const Feature> =
        feature_list.iter().map(|f| f as *const Feature).collect();
    pointers.push(std::ptr::null());
    Features {
        _schedule: schedule,
        _requests: requests_tx,
        queues: Some(WorkerQueues {
            requests,
            responses_tx,
            responses_rx,
        }),
        _map: map,
        _unmap: unmap,
        _options: options,
        _block: block,
        _rate: sample_rate,
        _uris: uris,
        _features: feature_list,
        pointers,
    }
}

/// A control port's range and default, with 0..1 and the minimum when the plugin gives none.
fn port_range(min: f32, max: f32, default: f32) -> (f32, f32, f32) {
    let (lo, hi) = if min.is_finite() && max.is_finite() && max > min {
        (min, max)
    } else {
        (0.0, 1.0)
    };
    let default = if default.is_finite() {
        default.clamp(lo, hi)
    } else {
        lo
    };
    (lo, hi, default)
}

/// Control values as saved in projects: `LV2P`, then the port index and f32 bits of each.
fn encode_controls(controls: impl Iterator<Item = (u32, u32)>) -> Vec<u8> {
    let mut out = b"LV2P".to_vec();
    for (index, bits) in controls {
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
    }
    out
}

/// The (port index, f32 bits) pairs of [`encode_controls`].
fn decode_controls(data: &[u8]) -> Option<impl Iterator<Item = (usize, u32)>> {
    let body = data.strip_prefix(b"LV2P")?;
    Some(body.as_chunks::<8>().0.iter().map(|entry| {
        let index = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
        (
            index,
            u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]),
        )
    }))
}

/// Main-thread side of an LV2 plugin.
pub struct Lv2Instance {
    descriptor: PluginDescriptor,
    plugin: *const Plugin,
    ports: Vec<PortInfo>,
    /// Control input values shared with the audio side (f32 bits).
    controls: Arc<[AtomicU32]>,
    /// Latency reported by the plugin (f32 bits).
    latency: Arc<AtomicU32>,
    instance: Option<(*mut LilvInstance, Features)>,
    processor_alive: Arc<AtomicBool>,
    active: bool,
    /// The plugin's window if this system can show it (looked up once), and the open one.
    ui_choice: Option<Option<ui::UiChoice>>,
    ui: Option<ui::Lv2Ui>,
    rate: u32,
}

impl std::fmt::Debug for Lv2Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lv2Instance")
            .field("plugin", &self.descriptor.id)
            .finish_non_exhaustive()
    }
}

impl Lv2Instance {
    pub fn new(descriptor: &PluginDescriptor) -> Result<Self, String> {
        let mut host = host()?;
        host.load_bundle(&descriptor.path);
        let plugin = host
            .find(&descriptor.id)
            .ok_or("plugin not found by lilv")?;
        let midi_event = host.node(MIDI_EVENT);
        let classes = [
            host.node(AUDIO_PORT),
            host.node(CONTROL_PORT),
            host.node(INPUT_PORT),
            host.node(ATOM_PORT),
        ];
        let properties = [
            host.node(TOGGLED),
            host.node(INTEGER),
            host.node(ENUMERATION),
            host.node(REPORTS_LATENCY),
            host.node(NOT_ON_GUI),
        ];
        // SAFETY: plain lilv calls on a plugin of the world.
        let count = unsafe { (host.lilv.lilv_plugin_get_num_ports)(plugin) } as usize;
        let (mut min, mut max, mut def) = (
            vec![f32::NAN; count],
            vec![f32::NAN; count],
            vec![f32::NAN; count],
        );
        // SAFETY: arrays have one entry per port.
        unsafe {
            (host.lilv.lilv_plugin_get_port_ranges_float)(
                plugin,
                min.as_mut_ptr(),
                max.as_mut_ptr(),
                def.as_mut_ptr(),
            )
        };
        let mut ports = Vec::with_capacity(count);
        for i in 0..count {
            // SAFETY: indices below the port count.
            unsafe {
                let port = (host.lilv.lilv_plugin_get_port_by_index)(plugin, i as u32);
                let is = |n: *const Node| (host.lilv.lilv_port_is_a)(plugin, port, n);
                let has = |n: *const Node| (host.lilv.lilv_port_has_property)(plugin, port, n);
                let input = is(classes[2]);
                let kind = if is(classes[0]) {
                    if input {
                        PortKind::AudioIn
                    } else {
                        PortKind::AudioOut
                    }
                } else if is(classes[1]) {
                    if input {
                        PortKind::ControlIn
                    } else {
                        PortKind::ControlOut
                    }
                } else if is(classes[3]) {
                    if input {
                        PortKind::AtomIn
                    } else {
                        PortKind::AtomOut
                    }
                } else {
                    PortKind::Other
                };
                let name = host.owned_string((host.lilv.lilv_port_get_name)(plugin, port));
                let (lo, hi, default) = port_range(min[i], max[i], def[i]);
                ports.push(PortInfo {
                    index: i as u32,
                    kind,
                    name,
                    min: lo,
                    max: hi,
                    default,
                    stepped: has(properties[0]) || has(properties[1]) || has(properties[2]),
                    hidden: has(properties[4]),
                    latency: has(properties[3]),
                    midi: kind == PortKind::AtomIn
                        && (host.lilv.lilv_port_supports_event)(plugin, port, midi_event),
                });
            }
        }
        let controls: Arc<[AtomicU32]> = ports
            .iter()
            .map(|p| AtomicU32::new(p.default.to_bits()))
            .collect();
        Ok(Self {
            descriptor: descriptor.clone(),
            plugin,
            ports,
            controls,
            latency: Arc::new(AtomicU32::new(0)),
            instance: None,
            processor_alive: Arc::new(AtomicBool::new(false)),
            active: false,
            ui_choice: None,
            ui: None,
            rate: 48_000,
        })
    }

    fn free_instance(&mut self) {
        // The window may use the instance directly (instance-access).
        self.ui = None;
        if let Some((instance, _features)) = self.instance.take() {
            if let Ok(host) = host() {
                // SAFETY: the audio side is gone; the instance is freed once.
                unsafe {
                    if self.active
                        && let Some(deactivate) = (*(*instance).descriptor).deactivate
                    {
                        deactivate((*instance).handle);
                    }
                    (host.lilv.lilv_instance_free)(instance);
                }
            }
            self.active = false;
        }
    }
}

impl crate::PluginInstance for Lv2Instance {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String> {
        if self.processor_alive.load(Ordering::Relaxed) {
            return Err("the plugin is still running".into());
        }
        self.free_instance();
        self.rate = sample_rate;
        let mut features = features(sample_rate, max_frames);
        let instance = {
            let host = host()?;
            // SAFETY: the feature array is null-terminated and outlives the instance.
            unsafe {
                (host.lilv.lilv_plugin_instantiate)(
                    self.plugin,
                    f64::from(sample_rate),
                    features.pointers.as_ptr(),
                )
            }
        };
        if instance.is_null() {
            return Err(
                "the plugin failed to instantiate (it may need host features AudioStrada lacks)"
                    .into(),
            );
        }
        let max = max_frames.max(1);
        let sequence = map_uri("http://lv2plug.in/ns/ext/atom#Sequence");
        let chunk = map_uri("http://lv2plug.in/ns/ext/atom#Chunk");
        let midi_event = map_uri("http://lv2plug.in/ns/ext/midi#MidiEvent");
        let audio_in: Vec<u32> = self
            .ports
            .iter()
            .filter(|p| p.kind == PortKind::AudioIn)
            .map(|p| p.index)
            .collect();
        let audio_out: Vec<u32> = self
            .ports
            .iter()
            .filter(|p| p.kind == PortKind::AudioOut)
            .map(|p| p.index)
            .collect();
        let mut processor = Lv2Processor {
            instance,
            inputs: vec![vec![0.0; max]; audio_in.len()],
            outputs: vec![vec![0.0; max]; audio_out.len()],
            controls: self.ports.iter().map(|p| p.default).collect(),
            atoms: Vec::new(),
            scratch: vec![0.0; max],
            ports: self.ports.clone(),
            audio_in,
            audio_out,
            shared: Arc::clone(&self.controls),
            latency: Arc::clone(&self.latency),
            sequence,
            chunk,
            midi_event,
            alive: Arc::clone(&self.processor_alive),
            worker: None,
        };
        // SAFETY: extension_data is a plain query on the plugin descriptor.
        let interface = unsafe {
            (*(*instance).descriptor)
                .extension_data
                .map_or(std::ptr::null(), |f| f(WORKER_INTERFACE.as_ptr()))
        } as *const WorkerInterface;
        if !interface.is_null()
            && let Some(queues) = features.queues.take()
        {
            // SAFETY: the instance outlives the worker, which the processor joins on drop.
            let handle = unsafe { (*instance).handle };
            let worker = Worker::start(interface, handle, queues.requests, queues.responses_tx);
            processor.worker = Some((worker, queues.responses_rx, interface));
        }
        for (i, c) in processor.controls.iter_mut().enumerate() {
            *c = f32::from_bits(self.controls[i].load(Ordering::Relaxed));
        }
        processor.atoms = self
            .ports
            .iter()
            .filter(|p| matches!(p.kind, PortKind::AtomIn | PortKind::AtomOut))
            .map(|p| {
                (
                    p.index,
                    p.kind == PortKind::AtomIn,
                    p.midi,
                    vec![0u64; ATOM_BUFFER / 8].into_boxed_slice(),
                )
            })
            .collect();
        processor.connect_all();
        // SAFETY: the instance was just created and every port is connected.
        unsafe {
            if let Some(activate) = (*(*instance).descriptor).activate {
                activate((*instance).handle);
            }
        }
        self.active = true;
        self.instance = Some((instance, features));
        self.processor_alive.store(true, Ordering::Relaxed);
        Ok(Box::new(processor))
    }

    fn try_deactivate(&mut self) -> bool {
        if self.processor_alive.load(Ordering::Relaxed) {
            return false;
        }
        self.free_instance();
        true
    }

    fn params(&mut self) -> Vec<ParamDesc> {
        self.ports
            .iter()
            .filter(|p| p.kind == PortKind::ControlIn)
            .map(|p| ParamDesc {
                id: p.index,
                name: p.name.clone(),
                module: String::new(),
                min: f64::from(p.min),
                max: f64::from(p.max),
                default: f64::from(p.default),
                stepped: p.stepped,
                hidden: p.hidden,
                read_only: false,
            })
            .collect()
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        self.controls
            .get(id as usize)
            .map(|v| f64::from(f32::from_bits(v.load(Ordering::Relaxed))))
    }

    fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
        let port = self.ports.get(id as usize)?;
        Some(if port.stepped {
            format!("{value:.0}")
        } else {
            format!("{value:.3}")
        })
    }

    fn set_param(&mut self, id: u32, value: f64) {
        if let Some(v) = self.controls.get(id as usize) {
            v.store((value as f32).to_bits(), Ordering::Relaxed);
        }
    }

    fn latency(&mut self) -> u32 {
        f32::from_bits(self.latency.load(Ordering::Relaxed)).max(0.0) as u32
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        Some(encode_controls(
            self.ports
                .iter()
                .filter(|p| p.kind == PortKind::ControlIn)
                .map(|p| {
                    let bits = self.controls[p.index as usize].load(Ordering::Relaxed);
                    (p.index, bits)
                }),
        ))
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        let Some(controls) = decode_controls(data) else {
            return false;
        };
        for (index, bits) in controls {
            if self
                .ports
                .get(index)
                .is_some_and(|p| p.kind == PortKind::ControlIn)
            {
                self.controls[index].store(bits, Ordering::Relaxed);
            }
        }
        true
    }

    fn poll(&mut self) -> Vec<PluginRequest> {
        let Some(ui) = &mut self.ui else {
            return Vec::new();
        };
        let Some(writes) = ui.poll(&self.ports, &self.controls) else {
            self.ui = None;
            return vec![PluginRequest::GuiClosed];
        };
        let mut requests: Vec<PluginRequest> = ui
            .take_keys()
            .into_iter()
            .map(|(sym, mods)| PluginRequest::Key(sym, mods))
            .collect();
        for (port, value) in writes {
            if self
                .ports
                .get(port as usize)
                .is_some_and(|p| p.kind == PortKind::ControlIn)
            {
                self.controls[port as usize].store(value.to_bits(), Ordering::Relaxed);
                requests.push(PluginRequest::ParamChanged(port, f64::from(value)));
            }
        }
        requests
    }

    fn has_gui(&mut self) -> bool {
        if self.ui_choice.is_none() {
            self.ui_choice = Some(host().ok().and_then(|mut h| ui::find(&mut h, self.plugin)));
        }
        matches!(self.ui_choice, Some(Some(_)))
    }

    fn open_gui(&mut self, title: &str) -> Result<(), String> {
        if self.ui.is_some() {
            return Ok(());
        }
        if !self.has_gui() {
            return Err("this plugin has no window for this system".into());
        }
        let Some(Some(choice)) = &self.ui_choice else {
            return Err("this plugin has no window for this system".into());
        };
        // SAFETY: the instance pointer stays valid while the window is open (closed first).
        let handle = self
            .instance
            .as_ref()
            .map_or(std::ptr::null_mut(), |(i, _)| unsafe { (**i).handle });
        self.ui = Some(ui::Lv2Ui::open(
            choice,
            &self.descriptor.id,
            title,
            handle,
            self.ports.len(),
            self.rate,
        )?);
        Ok(())
    }

    fn close_gui(&mut self) {
        self.ui = None;
    }

    fn gui_open(&self) -> bool {
        self.ui.is_some()
    }

    fn raise_gui(&mut self) {
        if let Some(ui) = &mut self.ui {
            ui.raise();
        }
    }
}

impl Drop for Lv2Instance {
    fn drop(&mut self) {
        self.ui = None;
        if self.processor_alive.load(Ordering::Relaxed) {
            // The audio side still runs the plugin: leak it rather than free under it.
            if let Some(instance) = self.instance.take() {
                std::mem::forget(instance);
            }
            return;
        }
        self.free_instance();
    }
}

/// The plugin's worker thread: runs `work` requests outside the audio thread.
struct Worker {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct WorkerJob {
    interface: *const WorkerInterface,
    handle: *mut c_void,
    requests: audiostrada_rt::Receiver<WorkMessage>,
    responses: Box<audiostrada_rt::Sender<WorkMessage>>,
}

// SAFETY: LV2 allows `work` to run on another thread concurrently with `run`.
unsafe impl Send for WorkerJob {}

impl Worker {
    fn start(
        interface: *const WorkerInterface,
        handle: *mut c_void,
        requests: audiostrada_rt::Receiver<WorkMessage>,
        responses: audiostrada_rt::Sender<WorkMessage>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let mut job = WorkerJob {
            interface,
            handle,
            requests,
            responses: Box::new(responses),
        };
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("audiostrada-lv2-worker".into())
            .spawn(move || {
                let job = &mut job;
                while !flag.load(Ordering::Relaxed) {
                    let mut idle = true;
                    while let Some(message) = job.requests.try_recv() {
                        idle = false;
                        let responses = &mut *job.responses
                            as *mut audiostrada_rt::Sender<WorkMessage>
                            as *mut c_void;
                        // SAFETY: the interface and handle stay valid until the worker is joined.
                        unsafe {
                            ((*job.interface).work)(
                                job.handle,
                                send_work,
                                responses,
                                message.len as u32,
                                message.data.as_ptr() as *const c_void,
                            );
                        }
                    }
                    if idle {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            })
            .ok();
        Self { stop, thread }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Audio side of an LV2 plugin.
pub struct Lv2Processor {
    instance: *mut LilvInstance,
    inputs: Vec<Vec<f32>>,
    outputs: Vec<Vec<f32>>,
    /// One value per port (only control ports are used).
    controls: Vec<f32>,
    /// Port, input, accepts MIDI, buffer.
    atoms: Vec<(u32, bool, bool, Box<[u64]>)>,
    scratch: Vec<f32>,
    ports: Vec<PortInfo>,
    audio_in: Vec<u32>,
    audio_out: Vec<u32>,
    shared: Arc<[AtomicU32]>,
    latency: Arc<AtomicU32>,
    sequence: u32,
    chunk: u32,
    midi_event: u32,
    alive: Arc<AtomicBool>,
    worker: Option<(
        Worker,
        audiostrada_rt::Receiver<WorkMessage>,
        *const WorkerInterface,
    )>,
}

// SAFETY: the instance is only run by one audio thread at a time and the main thread does not
// touch it while the processor is alive.
unsafe impl Send for Lv2Processor {}

impl std::fmt::Debug for Lv2Processor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lv2Processor").finish_non_exhaustive()
    }
}

impl Lv2Processor {
    fn connect(&self, port: u32, data: *mut c_void) {
        // SAFETY: the instance is alive and `data` outlives its use by the plugin.
        unsafe {
            ((*(*self.instance).descriptor).connect_port)((*self.instance).handle, port, data)
        };
    }

    fn connect_all(&mut self) {
        for i in 0..self.audio_in.len() {
            let ptr = self.inputs[i].as_mut_ptr() as *mut c_void;
            self.connect(self.audio_in[i], ptr);
        }
        for i in 0..self.audio_out.len() {
            let ptr = self.outputs[i].as_mut_ptr() as *mut c_void;
            self.connect(self.audio_out[i], ptr);
        }
        for p in self.ports.clone() {
            match p.kind {
                PortKind::ControlIn | PortKind::ControlOut => {
                    let ptr = &mut self.controls[p.index as usize] as *mut f32 as *mut c_void;
                    self.connect(p.index, ptr);
                }
                PortKind::Other => {
                    let ptr = self.scratch.as_mut_ptr() as *mut c_void;
                    self.connect(p.index, ptr);
                }
                _ => {}
            }
        }
        for i in 0..self.atoms.len() {
            let ptr = self.atoms[i].3.as_mut_ptr() as *mut c_void;
            let port = self.atoms[i].0;
            self.connect(port, ptr);
        }
    }

    /// Empty input sequences, output buffers ready for writing.
    fn reset_atoms(&mut self) {
        let (sequence, chunk) = (self.sequence, self.chunk);
        for (_, input, _, buffer) in &mut self.atoms {
            // Atom header: size (body bytes) and type, then the sequence body (unit, pad).
            let (size, kind) = if *input {
                (8u32, sequence)
            } else {
                ((ATOM_BUFFER - 8) as u32, chunk)
            };
            buffer[0] = u64::from(size) | (u64::from(kind) << 32);
            buffer[1] = 0;
        }
    }

    /// Writes MIDI events to the input sequences that accept them.
    fn write_midi(&mut self, midi: &[MidiEvent], n: usize) {
        let urid = self.midi_event;
        for (_, _, _, buffer) in self.atoms.iter_mut().filter(|a| a.2) {
            write_sequence(buffer, midi, n, urid);
        }
    }
}

/// Appends MIDI events (atom type `urid`) to an empty atom sequence: frame time, atom header
/// and data take one word each.
fn write_sequence(buffer: &mut [u64], midi: &[MidiEvent], n: usize, urid: u32) {
    let mut word = 2;
    for e in midi {
        if word + 3 > buffer.len() {
            break;
        }
        let len: u64 = if matches!(e.data[0] & 0xF0, 0xC0 | 0xD0) {
            2
        } else {
            3
        };
        buffer[word] = u64::from(crate::block_time(e, n));
        buffer[word + 1] = len | (u64::from(urid) << 32);
        buffer[word + 2] = u64::from(u32::from_le_bytes([e.data[0], e.data[1], e.data[2], 0]));
        word += 3;
    }
    let size = (word * 8 - 8) as u64;
    buffer[0] = size | (buffer[0] & 0xFFFF_FFFF_0000_0000);
}

impl PluginProcessor for Lv2Processor {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        _params_out: &mut dyn FnMut(u32, f64),
    ) {
        let max = self.scratch.len();
        let n = left.len().min(right.len()).min(max);
        if n == 0 {
            return;
        }
        for &(id, value) in params_in {
            if let Some(v) = self.shared.get(id as usize) {
                v.store((value as f32).to_bits(), Ordering::Relaxed);
            }
        }
        for p in &self.ports {
            if p.kind == PortKind::ControlIn {
                self.controls[p.index as usize] =
                    f32::from_bits(self.shared[p.index as usize].load(Ordering::Relaxed));
            }
        }
        crate::fill_inputs(&mut self.inputs, left, right, n);
        self.reset_atoms();
        self.write_midi(midi, n);
        // SAFETY: every port is connected to a buffer of at least `max` frames.
        unsafe { ((*(*self.instance).descriptor).run)((*self.instance).handle, n as u32) };
        if let Some((_, responses, interface)) = &mut self.worker {
            // SAFETY: the interface belongs to the live instance; responses come from its worker.
            unsafe {
                let handle = (*self.instance).handle;
                while let Some(message) = responses.try_recv() {
                    ((**interface).work_response)(
                        handle,
                        message.len as u32,
                        message.data.as_ptr() as *const c_void,
                    );
                }
                if let Some(end_run) = (**interface).end_run {
                    end_run(handle);
                }
            }
        }
        crate::take_outputs(&self.outputs, left, right, n);
        // Output controls (meters) for the plugin's window.
        for p in self.ports.iter().filter(|p| p.kind == PortKind::ControlOut) {
            self.shared[p.index as usize]
                .store(self.controls[p.index as usize].to_bits(), Ordering::Relaxed);
        }
        if let Some(p) = self
            .ports
            .iter()
            .find(|p| p.latency && p.kind == PortKind::ControlOut)
        {
            self.latency
                .store(self.controls[p.index as usize].to_bits(), Ordering::Relaxed);
        }
    }

    fn reset(&mut self) {}
}

impl Drop for Lv2Processor {
    fn drop(&mut self) {
        self.worker = None;
        self.alive.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginInstance as _;

    #[test]
    fn urids_round_trip() {
        let a = map_uri("http://example.org/a");
        assert_eq!(map_uri("http://example.org/a"), a);
        assert_ne!(map_uri("http://example.org/b"), a);
        // SAFETY: unmap returns a valid string for a mapped URID.
        let back = unsafe { CStr::from_ptr(urid_unmap(std::ptr::null_mut(), a)) };
        assert_eq!(back.to_str().unwrap(), "http://example.org/a");
    }

    #[test]
    fn ranges_states_and_sequences() {
        assert_eq!(port_range(-1.0, 1.0, 2.0), (-1.0, 1.0, 1.0));
        assert_eq!(port_range(f32::NAN, 1.0, f32::NAN), (0.0, 1.0, 0.0));
        assert_eq!(port_range(5.0, 5.0, 0.5), (0.0, 1.0, 0.5));
        let state = encode_controls([(3, 1.5f32.to_bits()), (7, 0)].into_iter());
        let decoded: Vec<_> = decode_controls(&state).unwrap().collect();
        assert_eq!(decoded, vec![(3, 1.5f32.to_bits()), (7, 0)]);
        assert!(decode_controls(b"SV3\0").is_none());
        let mut buffer = [0u64; 8];
        buffer[0] = 8 | (9u64 << 32);
        let midi = [
            MidiEvent::note_on(500, 1, 60, 100),
            MidiEvent {
                time: 2,
                data: [0xC0, 5, 0],
            },
            MidiEvent::note_on(0, 0, 61, 1),
        ];
        write_sequence(&mut buffer, &midi, 64, 42);
        assert_eq!(buffer[0], 56 | (9u64 << 32), "two events fit");
        assert_eq!(buffer[2..5], [63, 3 | (42u64 << 32), 0x0064_3C91]);
        assert_eq!(buffer[5..7], [2, 2 | (42u64 << 32)]);
    }

    #[test]
    fn work_messages_are_bounded() {
        let (mut tx, mut rx) = audiostrada_rt::channel::<WorkMessage>(1);
        let handle = &mut tx as *mut audiostrada_rt::Sender<WorkMessage> as *mut c_void;
        let data = [1u8, 2, 3];
        // SAFETY: the handle is a live sender and `data` holds the given size.
        unsafe {
            assert_eq!(send_work(handle, 3, data.as_ptr().cast()), 0);
            assert_eq!(send_work(handle, 3, data.as_ptr().cast()), 2, "queue full");
            assert_eq!(send_work(std::ptr::null_mut(), 0, std::ptr::null()), 1);
        }
        let message = rx.try_recv().unwrap();
        assert_eq!(&message.data[..message.len], &data);
        assert!(WorkMessage::new(WORK_MESSAGE as u32 + 1, data.as_ptr().cast()).is_none());
        assert!(WorkMessage::new(1, std::ptr::null()).is_none());
        assert_eq!(
            WorkMessage::new(0, std::ptr::null()).map(|m| m.len),
            Some(0)
        );
    }

    #[test]
    fn hosts_real_plugins_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_LV2") else {
            eprintln!("AUDIOSTRADA_TEST_LV2 not set, skipping");
            return;
        };
        let mut hosted = 0;
        for bundle in crate::scan::find_files(Format::Lv2, &[dir.into()]) {
            for descriptor in scan(&bundle)
                .unwrap()
                .into_iter()
                .filter(|d| !d.instrument)
                .take(3)
            {
                let mut instance = Lv2Instance::new(&descriptor).unwrap();
                let params = instance.params();
                let mut processor = instance.activate(48_000, 256).unwrap();
                let (mut left, mut right) = (vec![0.5f32; 256], vec![0.5f32; 256]);
                let first = params.first().map(|p| (p.id, p.default));
                for _ in 0..20 {
                    processor.process(&mut left, &mut right, first.as_slice(), &[], &mut |_, _| {});
                }
                assert!(
                    left.iter().chain(&right).all(|s| s.is_finite()),
                    "{}",
                    descriptor.name
                );
                let state = instance.save_state().unwrap();
                assert!(instance.load_state(&state));
                assert!(!instance.try_deactivate());
                drop(processor);
                assert!(instance.try_deactivate());
                eprintln!("hosted {} ({} params)", descriptor.name, params.len());
                hosted += 1;
            }
        }
        assert!(hosted > 0);
    }

    /// Plays notes on the instruments of the bundle in AUDIOSTRADA_TEST_LV2_INSTRUMENT.
    #[test]
    fn plays_real_instruments_when_available() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_LV2_INSTRUMENT") else {
            eprintln!("AUDIOSTRADA_TEST_LV2_INSTRUMENT not set, skipping");
            return;
        };
        let instruments: Vec<PluginDescriptor> =
            crate::scan::find_files(Format::Lv2, &[dir.into()])
                .iter()
                .flat_map(|b| scan(b).unwrap())
                .filter(|d| d.instrument)
                .collect();
        assert!(!instruments.is_empty());
        for descriptor in instruments {
            let mut instance = Lv2Instance::new(&descriptor).unwrap();
            let mut processor = instance.activate(48_000, 256).unwrap();
            let mut energy = 0.0f32;
            for block in 0..400 {
                let notes = match block {
                    10 => vec![MidiEvent::note_on(0, 0, 60, 100)],
                    300 => vec![MidiEvent::note_off(0, 0, 60)],
                    _ => Vec::new(),
                };
                let (mut left, mut right) = (vec![0.0f32; 256], vec![0.0f32; 256]);
                processor.process(&mut left, &mut right, &[], &notes, &mut |_, _| {});
                energy += left.iter().chain(&right).map(|s| s * s).sum::<f32>();
            }
            eprintln!("{}: energy {energy}", descriptor.name);
            assert!(energy > 0.01, "{} stays silent", descriptor.name);
        }
    }

    /// Opens a plugin window from the bundle in AUDIOSTRADA_TEST_LV2_UI (needs a display).
    #[test]
    fn opens_plugin_windows() {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_LV2_UI") else {
            eprintln!("AUDIOSTRADA_TEST_LV2_UI not set, skipping");
            return;
        };
        let descriptor = crate::scan::find_files(Format::Lv2, &[dir.into()])
            .into_iter()
            .flat_map(|b| scan(&b).unwrap())
            .find(|d| {
                !d.instrument
                    && Lv2Instance::new(d).is_ok_and(|mut i| crate::PluginInstance::has_gui(&mut i))
            })
            .expect("a plugin with a window");
        let mut instance = Lv2Instance::new(&descriptor).unwrap();
        let mut processor = instance.activate(48_000, 256).unwrap();
        instance.open_gui(&descriptor.name).unwrap();
        assert!(instance.gui_open());
        let (mut left, mut right) = (vec![0.1f32; 256], vec![0.1f32; 256]);
        for _ in 0..60 {
            processor.process(&mut left, &mut right, &[], &[], &mut |_, _| {});
            let requests = instance.poll();
            assert!(!requests.contains(&PluginRequest::GuiClosed));
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        instance.close_gui();
        assert!(!instance.gui_open());
        drop(processor);
        assert!(instance.try_deactivate());
        eprintln!("opened the window of {}", descriptor.name);
    }
}
