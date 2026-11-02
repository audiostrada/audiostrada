//! The AudioStrada DSP filter node: one mono float port per channel, processed on the PipeWire data thread.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use pipewire as pw;
use pw::properties::PropertiesBox;
use pw::spa::sys as spa_sys;
use pw::sys as pw_sys;

use crate::stream::CallbackRunner;
use crate::{AudioBuffers, BackendKind, IoError};

pub(super) const MAX_FRAMES: usize = 8192;

/// State only touched by the data thread once the filter is connected.
struct ProcessState {
    in_ports: Vec<*mut c_void>,
    out_ports: Vec<*mut c_void>,
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
    silence: Box<[f32]>,
    scratch: Vec<Box<[f32]>>,
    runner: CallbackRunner,
    rate: u32,
    xrun_total: Option<u64>,
}

impl ProcessState {
    fn process(&mut self, position: &spa_sys::spa_io_position) {
        let clock = &position.clock;
        let frames = (clock.duration as usize).min(MAX_FRAMES);
        if frames == 0 {
            return;
        }
        if clock.rate.denom != 0 && clock.rate.denom != self.rate {
            self.rate = clock.rate.denom;
            self.runner.set_sample_rate(self.rate);
        }
        if let Some(previous) = self.xrun_total
            && clock.xrun > previous
        {
            self.runner.status().add_xruns(1);
        }
        self.xrun_total = Some(clock.xrun);

        for (ptr, &port) in self.in_ptrs.iter_mut().zip(&self.in_ports) {
            // SAFETY: `port` comes from pw_filter_add_port and we are in the process callback.
            let buf = unsafe { pw_sys::pw_filter_get_dsp_buffer(port, frames as u32) };
            *ptr = if buf.is_null() {
                self.silence.as_ptr()
            } else {
                buf.cast_const().cast()
            };
        }
        for ((ptr, &port), scratch) in self
            .out_ptrs
            .iter_mut()
            .zip(&self.out_ports)
            .zip(&mut self.scratch)
        {
            // SAFETY: as above.
            let buf = unsafe { pw_sys::pw_filter_get_dsp_buffer(port, frames as u32) };
            *ptr = if buf.is_null() {
                scratch.as_mut_ptr()
            } else {
                buf.cast()
            };
        }
        // SAFETY: DSP buffers hold `frames` samples; each output is its own port buffer or scratch.
        let mut buffers = unsafe { AudioBuffers::from_raw(&self.in_ptrs, &self.out_ptrs, frames) };
        self.runner.run(&mut buffers);
    }
}

unsafe extern "C" fn on_process(data: *mut c_void, position: *mut spa_sys::spa_io_position) {
    // SAFETY: `data` is the ProcessState registered with this listener, only used on this thread.
    let state = unsafe { &mut *data.cast::<ProcessState>() };
    // SAFETY: PipeWire passes a valid position, or null when none is available.
    if let Some(position) = unsafe { position.as_ref() } {
        state.process(position);
    }
}

unsafe extern "C" fn on_state_changed(
    _data: *mut c_void,
    _old: pw_sys::pw_filter_state,
    state: pw_sys::pw_filter_state,
    error: *const c_char,
) {
    // SAFETY: PipeWire returns a static string for any state.
    let name = unsafe { CStr::from_ptr(pw_sys::pw_filter_state_as_string(state)) };
    if error.is_null() {
        tracing::debug!("PipeWire filter state: {}", name.to_string_lossy());
    } else {
        // SAFETY: non-null error messages are valid C strings for the duration of the call.
        let error = unsafe { CStr::from_ptr(error) };
        tracing::error!(
            "PipeWire filter {}: {}",
            name.to_string_lossy(),
            error.to_string_lossy()
        );
    }
}

pub(super) struct FilterStream {
    filter: *mut pw_sys::pw_filter,
    process: *mut ProcessState,
    _hooks: Box<[spa_sys::spa_hook; 2]>,
    _events: Box<[pw_sys::pw_filter_events; 2]>,
    pub inputs: usize,
    pub outputs: usize,
    pub input_device: Option<u32>,
    pub output_device: Option<u32>,
    /// Follow the system default device instead of a fixed one.
    pub follow_input: bool,
    pub follow_output: bool,
    pub links: Vec<pw::link::Link>,
    pub linked: bool,
}

impl FilterStream {
    pub(super) fn new(
        core: &pw::core::Core,
        props: PropertiesBox,
        inputs: usize,
        outputs: usize,
        devices: (Option<u32>, Option<u32>),
        follow: (bool, bool),
        runner: CallbackRunner,
    ) -> Result<Self, IoError> {
        let error = |msg: &str| IoError::backend(BackendKind::PipeWire, msg);
        // SAFETY: the core is connected; ownership of `props` is transferred.
        let filter = unsafe {
            pw_sys::pw_filter_new(core.as_raw_ptr(), c"AudioStrada".as_ptr(), props.into_raw())
        };
        if filter.is_null() {
            return Err(error("cannot create filter"));
        }
        let add_port = |output: bool, index: usize| {
            let props = pw::properties::properties! {
                "format.dsp" => "32 bit float mono audio",
                "port.name" => format!("{}_{}", if output { "out" } else { "in" }, index + 1),
            };
            let direction = if output {
                spa_sys::SPA_DIRECTION_OUTPUT
            } else {
                spa_sys::SPA_DIRECTION_INPUT
            };
            // SAFETY: `filter` is valid; ownership of `props` is transferred.
            unsafe {
                pw_sys::pw_filter_add_port(
                    filter,
                    direction,
                    pw_sys::pw_filter_port_flags_PW_FILTER_PORT_FLAG_MAP_BUFFERS,
                    size_of::<u64>(),
                    props.into_raw(),
                    ptr::null_mut(),
                    0,
                )
            }
        };
        let in_ports: Vec<_> = (0..inputs).map(|i| add_port(false, i)).collect();
        let out_ports: Vec<_> = (0..outputs).map(|i| add_port(true, i)).collect();
        if in_ports.iter().chain(&out_ports).any(|p| p.is_null()) {
            // SAFETY: the filter was created above and is not connected.
            unsafe { pw_sys::pw_filter_destroy(filter) };
            return Err(error("cannot add filter ports"));
        }

        let process = Box::into_raw(Box::new(ProcessState {
            in_ptrs: vec![ptr::null(); inputs],
            out_ptrs: vec![ptr::null_mut(); outputs],
            in_ports,
            out_ports,
            silence: vec![0.0; MAX_FRAMES].into_boxed_slice(),
            scratch: (0..outputs)
                .map(|_| vec![0.0; MAX_FRAMES].into_boxed_slice())
                .collect(),
            runner,
            rate: 0,
            xrun_total: None,
        }));

        // SAFETY: an all-zero hook and events table (null callbacks) is the C initial state.
        let hooks: Box<[spa_sys::spa_hook; 2]> = Box::new(unsafe { std::mem::zeroed() });
        let mut events: Box<[pw_sys::pw_filter_events; 2]> =
            Box::new(unsafe { std::mem::zeroed() });
        events[0].version = pw_sys::PW_VERSION_FILTER_EVENTS;
        events[0].process = Some(on_process);
        events[1].version = pw_sys::PW_VERSION_FILTER_EVENTS;
        events[1].state_changed = Some(on_state_changed);

        let stream = Self {
            filter,
            process,
            inputs,
            outputs,
            input_device: devices.0,
            output_device: devices.1,
            follow_input: follow.0,
            follow_output: follow.1,
            links: Vec::new(),
            linked: false,
            _hooks: hooks,
            _events: events,
        };
        let hooks = stream._hooks.as_ptr().cast_mut();
        // SAFETY: hooks, events and process state are heap allocated and live as long as the filter.
        let res = unsafe {
            pw_sys::pw_filter_add_listener(filter, hooks, &stream._events[0], process.cast());
            pw_sys::pw_filter_add_listener(
                filter,
                hooks.add(1),
                &stream._events[1],
                ptr::null_mut(),
            );
            pw_sys::pw_filter_connect(
                filter,
                pw_sys::pw_filter_flags_PW_FILTER_FLAG_RT_PROCESS,
                ptr::null_mut(),
                0,
            )
        };
        if res < 0 {
            return Err(error(&format!("cannot connect filter (error {res})")));
        }
        Ok(stream)
    }

    pub(super) fn node_id(&self) -> Option<u32> {
        // SAFETY: the filter is valid until drop.
        let id = unsafe { pw_sys::pw_filter_get_node_id(self.filter) };
        (id != spa_sys::SPA_ID_INVALID).then_some(id)
    }
}

impl Drop for FilterStream {
    fn drop(&mut self) {
        // SAFETY: destroying the filter stops the data thread callbacks before the state is freed.
        unsafe {
            pw_sys::pw_filter_destroy(self.filter);
            drop(Box::from_raw(self.process));
        }
    }
}
