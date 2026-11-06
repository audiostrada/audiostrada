//! A minimal CLAP plugin for the plugin host's tests: a stereo gain whose one parameter, in dB,
//! moves by events in the audio thread or from the host, and is saved in its state.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, clap_event_param_value, clap_input_events,
    clap_output_events,
};
use clap_sys::ext::audio_ports::{
    CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS, CLAP_PORT_STEREO, clap_audio_port_info,
    clap_plugin_audio_ports,
};
use clap_sys::ext::params::{
    CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE, clap_param_info, clap_plugin_params,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::plugin_features::{CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_STEREO};
use clap_sys::process::{CLAP_PROCESS_CONTINUE, clap_process, clap_process_status};
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;

/// The plugin's id, as hosts find it.
pub const ID: &CStr = c"com.audiostrada.test-gain";
/// Its one parameter.
pub const GAIN: clap_id = 0;
pub const MIN_DB: f64 = -60.0;
pub const MAX_DB: f64 = 12.0;

struct Features([*const c_char; 3]);
// SAFETY: pointers to static strings.
unsafe impl Sync for Features {}
static FEATURES: Features = Features([
    CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr(),
    CLAP_PLUGIN_FEATURE_STEREO.as_ptr(),
    std::ptr::null(),
]);

struct Descriptor(clap_plugin_descriptor);
// SAFETY: pointers to static strings.
unsafe impl Sync for Descriptor {}
static DESCRIPTOR: Descriptor = Descriptor(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: ID.as_ptr(),
    name: c"Test Gain".as_ptr(),
    vendor: c"AudioStrada".as_ptr(),
    url: c"".as_ptr(),
    manual_url: c"".as_ptr(),
    support_url: c"".as_ptr(),
    version: c"1.0.0".as_ptr(),
    description: c"A gain for tests".as_ptr(),
    features: &FEATURES.0 as *const _ as *const *const c_char,
});

/// The plugin's data: its gain in dB, as f64 bits (read by the audio and main threads).
struct Gain {
    plugin: clap_plugin,
    db: AtomicU64,
}

impl Gain {
    /// # Safety
    /// `plugin` must be one this library created.
    unsafe fn of<'a>(plugin: *const clap_plugin) -> &'a Gain {
        // SAFETY: plugin_data points to the Gain that holds `plugin`.
        unsafe { &*((*plugin).plugin_data as *const Gain) }
    }

    fn db(&self) -> f64 {
        f64::from_bits(self.db.load(Ordering::Relaxed))
    }

    fn set_db(&self, db: f64) {
        self.db
            .store(db.clamp(MIN_DB, MAX_DB).to_bits(), Ordering::Relaxed);
    }
}

unsafe extern "C" fn init(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    // SAFETY: the Gain was leaked by create_plugin and is freed once.
    unsafe { drop(Box::from_raw((*plugin).plugin_data as *mut Gain)) };
}

unsafe extern "C" fn activate(_: *const clap_plugin, _: f64, _: u32, _: u32) -> bool {
    true
}

unsafe extern "C" fn nothing(_: *const clap_plugin) {}

unsafe extern "C" fn start_processing(_: *const clap_plugin) -> bool {
    true
}

/// Applies the parameter events of `events`.
///
/// # Safety
/// `events` must be a valid list from the host.
unsafe fn apply(gain: &Gain, events: *const clap_input_events) {
    if events.is_null() {
        return;
    }
    // SAFETY: the host's list and its callbacks.
    unsafe {
        let list = &*events;
        let (Some(size), Some(get)) = (list.size, list.get) else {
            return;
        };
        for i in 0..size(events) {
            let header = get(events, i);
            if !header.is_null()
                && (*header).space_id == CLAP_CORE_EVENT_SPACE_ID
                && (*header).type_ == CLAP_EVENT_PARAM_VALUE
            {
                let event = &*(header as *const clap_event_param_value);
                if event.param_id == GAIN {
                    gain.set_db(event.value);
                }
            }
        }
    }
}

unsafe extern "C" fn process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    // SAFETY: the host passes a valid process block for this plugin.
    unsafe {
        let gain = Gain::of(plugin);
        let process = &*process;
        apply(gain, process.in_events);
        let factor = 10f32.powf(gain.db() as f32 / 20.0);
        let frames = process.frames_count as usize;
        if process.audio_inputs_count == 0 || process.audio_outputs_count == 0 {
            return CLAP_PROCESS_CONTINUE;
        }
        let input: &clap_audio_buffer = &*process.audio_inputs;
        let output: &clap_audio_buffer = &*process.audio_outputs;
        let channels = input.channel_count.min(output.channel_count) as usize;
        for c in 0..channels {
            let from = *input.data32.add(c);
            let to = *output.data32.add(c);
            for i in 0..frames {
                *to.add(i) = *from.add(i) * factor;
            }
        }
    }
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn ports_count(_: *const clap_plugin, _input: bool) -> u32 {
    1
}

unsafe extern "C" fn port_get(
    _: *const clap_plugin,
    index: u32,
    _input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if index != 0 {
        return false;
    }
    // SAFETY: the host gives a writable info.
    unsafe {
        let info = &mut *info;
        info.id = 0;
        write_name(&mut info.name, "Main");
        info.flags = CLAP_AUDIO_PORT_IS_MAIN;
        info.channel_count = 2;
        info.port_type = CLAP_PORT_STEREO.as_ptr();
        info.in_place_pair = 0;
    }
    true
}

static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ports_count),
    get: Some(port_get),
};

fn write_name(to: &mut [c_char], text: &str) {
    let n = text.len().min(to.len() - 1);
    for (slot, byte) in to.iter_mut().zip(text.bytes().take(n)) {
        *slot = byte as c_char;
    }
    to[n] = 0;
}

unsafe extern "C" fn params_count(_: *const clap_plugin) -> u32 {
    1
}

unsafe extern "C" fn param_info(
    _: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    if index != 0 {
        return false;
    }
    // SAFETY: the host gives a writable info.
    unsafe {
        let info = &mut *info;
        info.id = GAIN;
        info.flags = CLAP_PARAM_IS_AUTOMATABLE;
        info.cookie = std::ptr::null_mut();
        write_name(&mut info.name, "Gain");
        write_name(&mut info.module, "");
        info.min_value = MIN_DB;
        info.max_value = MAX_DB;
        info.default_value = 0.0;
    }
    true
}

unsafe extern "C" fn param_value(plugin: *const clap_plugin, id: clap_id, out: *mut f64) -> bool {
    if id != GAIN {
        return false;
    }
    // SAFETY: a plugin of this library and a writable value.
    unsafe { *out = Gain::of(plugin).db() };
    true
}

unsafe extern "C" fn value_to_text(
    _: *const clap_plugin,
    id: clap_id,
    value: f64,
    out: *mut c_char,
    capacity: u32,
) -> bool {
    if id != GAIN || capacity == 0 {
        return false;
    }
    let text = format!("{value:.1} dB");
    // SAFETY: the host gives `capacity` writable bytes.
    let to = unsafe { std::slice::from_raw_parts_mut(out, capacity as usize) };
    write_name(to, &text);
    true
}

unsafe extern "C" fn text_to_value(
    _: *const clap_plugin,
    id: clap_id,
    text: *const c_char,
    out: *mut f64,
) -> bool {
    // SAFETY: the host gives a C string and a writable value.
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    match text.trim().trim_end_matches("dB").trim().parse::<f64>() {
        Ok(v) if id == GAIN => {
            // SAFETY: as above.
            unsafe { *out = v };
            true
        }
        _ => false,
    }
}

unsafe extern "C" fn flush(
    plugin: *const clap_plugin,
    events: *const clap_input_events,
    _out: *const clap_output_events,
) {
    // SAFETY: a plugin of this library and the host's events.
    unsafe { apply(Gain::of(plugin), events) };
}

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(param_info),
    get_value: Some(param_value),
    value_to_text: Some(value_to_text),
    text_to_value: Some(text_to_value),
    flush: Some(flush),
};

unsafe extern "C" fn state_save(plugin: *const clap_plugin, stream: *const clap_ostream) -> bool {
    // SAFETY: a plugin of this library and the host's stream.
    unsafe {
        let bytes = Gain::of(plugin).db().to_le_bytes();
        let Some(write) = (*stream).write else {
            return false;
        };
        write(stream, bytes.as_ptr() as *const c_void, bytes.len() as u64) == bytes.len() as i64
    }
}

unsafe extern "C" fn state_load(plugin: *const clap_plugin, stream: *const clap_istream) -> bool {
    let mut bytes = [0u8; 8];
    // SAFETY: a plugin of this library and the host's stream.
    unsafe {
        let Some(read) = (*stream).read else {
            return false;
        };
        let mut got = 0;
        while got < bytes.len() {
            let n = read(
                stream,
                bytes[got..].as_mut_ptr() as *mut c_void,
                (bytes.len() - got) as u64,
            );
            if n <= 0 {
                return false;
            }
            got += n as usize;
        }
        Gain::of(plugin).set_db(f64::from_le_bytes(bytes));
    }
    true
}

static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(state_save),
    load: Some(state_load),
};

unsafe extern "C" fn get_extension(_: *const clap_plugin, id: *const c_char) -> *const c_void {
    // SAFETY: the host passes a C string.
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        &AUDIO_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn plugin_count(_: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C" fn plugin_descriptor(
    _: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        &DESCRIPTOR.0
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn create_plugin(
    _: *const clap_plugin_factory,
    _host: *const clap_host,
    id: *const c_char,
) -> *const clap_plugin {
    // SAFETY: the host passes a C string.
    if unsafe { CStr::from_ptr(id) } != ID {
        return std::ptr::null();
    }
    let gain = Box::into_raw(Box::new(Gain {
        plugin: clap_plugin {
            desc: &DESCRIPTOR.0,
            plugin_data: std::ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(nothing),
            start_processing: Some(start_processing),
            stop_processing: Some(nothing),
            reset: Some(nothing),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(nothing),
        },
        db: AtomicU64::new(0f64.to_bits()),
    }));
    // SAFETY: just allocated; it points to itself for the callbacks.
    unsafe {
        (*gain).plugin.plugin_data = gain as *mut c_void;
        &(*gain).plugin
    }
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(plugin_count),
    get_plugin_descriptor: Some(plugin_descriptor),
    create_plugin: Some(create_plugin),
};

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}

unsafe extern "C" fn entry_deinit() {}

unsafe extern "C" fn get_factory(id: *const c_char) -> *const c_void {
    // SAFETY: the host passes a C string.
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID {
        &FACTORY as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

/// The entry point CLAP hosts look for.
#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(get_factory),
};

/// Where the library built next to the running test is, if it was built.
pub fn built_library() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = format!(
        "{}audiostrada_test_plugin{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    exe.ancestors()
        .skip(1)
        .take(3)
        .map(|dir| dir.join(&name))
        .find(|p| p.is_file())
}
