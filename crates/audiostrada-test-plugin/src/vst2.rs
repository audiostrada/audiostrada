//! The same gain as a VST2 plugin, for the VST2 host's tests: the library also exports
//! `VSTPluginMain`. One parameter, 0 to 1 for -60 to +12 dB, saved in its chunk; it tells the
//! host about a value it sets itself (as from its window) through `AUTOMATE`.
//!
//! The binary interface is written from what is publicly known of it, as in the host.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

/// The plugin's VST2 unique id, `'AsTg'`.
pub const VST2_ID: i32 = 0x4173_5467;
/// The gain parameter's value for 0 dB.
pub const VST2_UNITY: f32 = 60.0 / 72.0;

type HostCallback = unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;

#[repr(C)]
pub struct AEffect {
    magic: i32,
    dispatcher: unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize,
    process: unsafe extern "C" fn(*mut AEffect, *mut *mut f32, *mut *mut f32, i32),
    set_parameter: unsafe extern "C" fn(*mut AEffect, i32, f32),
    get_parameter: unsafe extern "C" fn(*mut AEffect, i32) -> f32,
    num_programs: i32,
    num_params: i32,
    num_inputs: i32,
    num_outputs: i32,
    flags: i32,
    reserved1: isize,
    reserved2: isize,
    initial_delay: i32,
    real_qualities: i32,
    off_qualities: i32,
    io_ratio: f32,
    object: *mut c_void,
    user: *mut c_void,
    unique_id: i32,
    version: i32,
    process_replacing: unsafe extern "C" fn(*mut AEffect, *mut *mut f32, *mut *mut f32, i32),
    process_double_replacing: *mut c_void,
    future: [u8; 56],
}

/// The plugin: the structure handed to the host first, so that its address is the plugin's.
#[repr(C)]
struct Plugin {
    effect: AEffect,
    host: HostCallback,
    /// The parameter's value, as f32 bits.
    gain: AtomicU32,
    /// The chunk last handed to the host, kept until the next one.
    chunk: Vec<u8>,
}

impl Plugin {
    /// # Safety
    /// `effect` must be one this library created and has not closed.
    unsafe fn of<'a>(effect: *mut AEffect) -> &'a mut Plugin {
        // SAFETY: `effect` is the first field of a leaked `Plugin`.
        unsafe { &mut *(effect as *mut Plugin) }
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    fn set_gain(&self, value: f32) {
        self.gain
            .store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn db(&self) -> f32 {
        -60.0 + 72.0 * self.gain()
    }
}

const EFF_CLOSE: i32 = 1;
const EFF_SET_PROGRAM: i32 = 2;
const EFF_GET_PARAM_LABEL: i32 = 6;
const EFF_GET_PARAM_DISPLAY: i32 = 7;
const EFF_GET_PARAM_NAME: i32 = 8;
const EFF_GET_CHUNK: i32 = 23;
const EFF_SET_CHUNK: i32 = 24;
const EFF_GET_PLUG_CATEGORY: i32 = 35;
const EFF_GET_EFFECT_NAME: i32 = 45;
const EFF_GET_VENDOR_STRING: i32 = 47;
const EFF_GET_VENDOR_VERSION: i32 = 49;
const EFF_GET_VST_VERSION: i32 = 58;
const HOST_AUTOMATE: i32 = 0;

/// Writes `text` as a C string where the host asked.
///
/// # Safety
/// `ptr` must be null or point to a buffer of at least 64 bytes.
unsafe fn write(ptr: *mut c_void, text: &str) -> isize {
    if ptr.is_null() {
        return 0;
    }
    let n = text.len().min(63);
    // SAFETY: the host's buffer holds at least 64 bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr as *mut u8, n);
        *(ptr as *mut u8).add(n) = 0;
    }
    1
}

unsafe extern "C" fn dispatcher(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    _opt: f32,
) -> isize {
    // SAFETY: the host passes the plugin this library created.
    let plugin = unsafe { Plugin::of(effect) };
    // SAFETY: the host passes buffers and pointers as each command expects.
    unsafe {
        match opcode {
            EFF_CLOSE => {
                drop(Box::from_raw(effect as *mut Plugin));
                1
            }
            // Program 1 is "Quiet": -24 dB, set by the plugin itself, as a window would.
            EFF_SET_PROGRAM if value == 1 => {
                plugin.set_gain(36.0 / 72.0);
                (plugin.host)(
                    effect,
                    HOST_AUTOMATE,
                    0,
                    0,
                    std::ptr::null_mut(),
                    plugin.gain(),
                );
                1
            }
            EFF_GET_PARAM_NAME if index == 0 => write(ptr, "Gain"),
            EFF_GET_PARAM_LABEL if index == 0 => write(ptr, "dB"),
            EFF_GET_PARAM_DISPLAY if index == 0 => write(ptr, &format!("{:.1}", plugin.db())),
            EFF_GET_EFFECT_NAME => write(ptr, "Test Gain"),
            EFF_GET_VENDOR_STRING => write(ptr, "AudioStrada"),
            EFF_GET_VENDOR_VERSION => 1000,
            EFF_GET_VST_VERSION => 2400,
            // An effect.
            EFF_GET_PLUG_CATEGORY => 1,
            EFF_GET_CHUNK if !ptr.is_null() => {
                plugin.chunk = plugin.gain().to_le_bytes().to_vec();
                *(ptr as *mut *const u8) = plugin.chunk.as_ptr();
                plugin.chunk.len() as isize
            }
            EFF_SET_CHUNK if !ptr.is_null() && value >= 4 => {
                let bytes = std::slice::from_raw_parts(ptr as *const u8, 4);
                plugin.set_gain(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                1
            }
            _ => 0,
        }
    }
}

unsafe extern "C" fn set_parameter(effect: *mut AEffect, index: i32, value: f32) {
    // SAFETY: the host passes the plugin this library created.
    let plugin = unsafe { Plugin::of(effect) };
    if index == 0 {
        plugin.set_gain(value);
    }
}

unsafe extern "C" fn get_parameter(effect: *mut AEffect, index: i32) -> f32 {
    // SAFETY: as above.
    let plugin = unsafe { Plugin::of(effect) };
    if index == 0 { plugin.gain() } else { 0.0 }
}

unsafe extern "C" fn process_replacing(
    effect: *mut AEffect,
    inputs: *mut *mut f32,
    outputs: *mut *mut f32,
    frames: i32,
) {
    // SAFETY: the host passes the plugin, two input and two output buffers of `frames`.
    unsafe {
        let plugin = Plugin::of(effect);
        let gain = 10f32.powf(plugin.db() / 20.0);
        for ch in 0..2 {
            let input = std::slice::from_raw_parts(*inputs.add(ch), frames.max(0) as usize);
            let output = std::slice::from_raw_parts_mut(*outputs.add(ch), frames.max(0) as usize);
            for (o, i) in output.iter_mut().zip(input) {
                *o = i * gain;
            }
        }
    }
}

/// The accumulating `process` of old hosts.
unsafe extern "C" fn process_adding(
    effect: *mut AEffect,
    inputs: *mut *mut f32,
    outputs: *mut *mut f32,
    frames: i32,
) {
    // SAFETY: as for `process_replacing`.
    unsafe {
        let plugin = Plugin::of(effect);
        let gain = 10f32.powf(plugin.db() / 20.0);
        for ch in 0..2 {
            let input = std::slice::from_raw_parts(*inputs.add(ch), frames.max(0) as usize);
            let output = std::slice::from_raw_parts_mut(*outputs.add(ch), frames.max(0) as usize);
            for (o, i) in output.iter_mut().zip(input) {
                *o += i * gain;
            }
        }
    }
}

/// The VST2 entry point.
///
/// # Safety
/// Called by a VST2 host with its callback.
#[unsafe(no_mangle)]
#[allow(non_snake_case, reason = "the name VST2 hosts look for")]
pub unsafe extern "C" fn VSTPluginMain(host: HostCallback) -> *mut c_void {
    let plugin = Box::new(Plugin {
        effect: AEffect {
            magic: 0x5673_7450,
            dispatcher,
            process: process_adding,
            set_parameter,
            get_parameter,
            num_programs: 2,
            num_params: 1,
            num_inputs: 2,
            num_outputs: 2,
            // Replacing, state in chunks.
            flags: (1 << 4) | (1 << 5),
            reserved1: 0,
            reserved2: 0,
            initial_delay: 0,
            real_qualities: 0,
            off_qualities: 0,
            io_ratio: 1.0,
            object: std::ptr::null_mut(),
            user: std::ptr::null_mut(),
            unique_id: VST2_ID,
            version: 1000,
            process_replacing,
            process_double_replacing: std::ptr::null_mut(),
            future: [0; 56],
        },
        host,
        gain: AtomicU32::new(VST2_UNITY.to_bits()),
        chunk: Vec::new(),
    });
    Box::into_raw(plugin) as *mut c_void
}
