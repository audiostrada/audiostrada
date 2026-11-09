//! CLAP host, built on clack.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::window::{HostWindow, NativeHandle};
use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfoBuffer, AudioPortRescanFlags, HostAudioPorts, HostAudioPortsImpl,
    PluginAudioPorts,
};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui, Window,
};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamInfoBuffer,
    ParamInfoFlags, ParamRescanFlags, PluginParams,
};
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl, PluginPosixFd};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::events::Match;
use clack_host::events::event_types::{
    MidiEvent as ClapMidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent, TransportEvent,
    TransportFlags,
};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{EventFlags, EventHeader};
use clack_host::prelude::*;
use clack_host::utils::{BeatTime, SecondsTime};

use crate::{Format, MidiEvent, ParamDesc, PluginDescriptor, PluginProcessor, PluginRequest};

/// Events of each kind per block (parameter changes, MIDI); the input buffer holds both.
const MAX_EVENTS: usize = 512;

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Lists the plugins of a `.clap` file.
pub fn scan(path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    crate::check_binary(path)?;
    // SAFETY: loading a library runs its initialisers; scanning happens in a separate process.
    let entry = unsafe { PluginEntry::load(path) }.map_err(|e| e.to_string())?;
    let factory = entry
        .get_plugin_factory()
        .ok_or("the file has no plugin factory")?;
    let lossy = |s: Option<&std::ffi::CStr>| {
        s.map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    Ok(factory
        .plugin_descriptors()
        .filter_map(|d| {
            let id = d.id()?.to_string_lossy().into_owned();
            let tags: Vec<String> = d
                .features()
                .map(|f| f.to_string_lossy().into_owned())
                .collect();
            let instrument = tags.iter().any(|t| t == "instrument");
            Some(PluginDescriptor {
                format: Format::Clap,
                name: lossy(d.name()),
                vendor: lossy(d.vendor()),
                version: lossy(d.version()),
                path: path.to_path_buf(),
                instrument,
                tags,
                id,
            })
        })
        .collect())
}

struct ClapHost;

#[derive(Default)]
struct Shared {
    callback: AtomicBool,
    restart: AtomicBool,
    gui_closed: AtomicBool,
    /// Size requested by an embedded editor, packed as width << 32 | height.
    resize: std::sync::atomic::AtomicU64,
}

impl SharedHandler<'_> for Shared {
    fn request_restart(&self) {
        self.restart.store(true, Ordering::Relaxed);
    }

    fn request_process(&self) {}

    fn request_callback(&self) {
        self.callback.store(true, Ordering::Relaxed);
    }
}

impl HostLogImpl for Shared {
    fn log(&self, severity: LogSeverity, message: &str) {
        tracing::debug!(target: "audiostrada::plugin", "[{severity}] {message}");
    }
}

impl HostParamsImplShared for Shared {
    fn request_flush(&self) {}
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, new_size: GuiSize) -> Result<(), HostError> {
        self.resize.store(
            (u64::from(new_size.width) << 32) | u64::from(new_size.height),
            Ordering::Relaxed,
        );
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {
        self.gui_closed.store(true, Ordering::Relaxed);
    }
}

struct Timer {
    id: u32,
    period: Duration,
    next: Instant,
}

struct MainThread {
    requests: RefCell<Vec<PluginRequest>>,
    timers: RefCell<Vec<Timer>>,
    next_timer: Cell<u32>,
    #[cfg(unix)]
    fds: RefCell<Vec<(std::os::fd::RawFd, FdFlags)>>,
}

impl MainThread {
    fn new() -> Self {
        Self {
            requests: RefCell::new(Vec::new()),
            timers: RefCell::new(Vec::new()),
            next_timer: Cell::new(1),
            #[cfg(unix)]
            fds: RefCell::new(Vec::new()),
        }
    }
}

impl HostTimerImpl for MainThread {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        let id = self.next_timer.get();
        self.next_timer.set(id.wrapping_add(1).max(1));
        let period = Duration::from_millis(u64::from(period_ms.max(15)));
        self.timers.borrow_mut().push(Timer {
            id,
            period,
            next: Instant::now() + period,
        });
        Ok(TimerId(id))
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        self.timers.borrow_mut().retain(|t| t.id != timer_id.0);
        Ok(())
    }
}

#[cfg(unix)]
impl HostPosixFdImpl for MainThread {
    fn register_fd(&self, fd: std::os::fd::RawFd, flags: FdFlags) -> Result<(), HostError> {
        let mut fds = self.fds.borrow_mut();
        fds.retain(|(f, _)| *f != fd);
        fds.push((fd, flags));
        Ok(())
    }

    fn modify_fd(&self, fd: std::os::fd::RawFd, flags: FdFlags) -> Result<(), HostError> {
        self.register_fd(fd, flags)
    }

    fn unregister_fd(&self, fd: std::os::fd::RawFd) -> Result<(), HostError> {
        self.fds.borrow_mut().retain(|(f, _)| *f != fd);
        Ok(())
    }
}

impl MainThread {
    fn push(&self, request: PluginRequest) {
        let mut requests = self.requests.borrow_mut();
        if !requests.contains(&request) {
            requests.push(request);
        }
    }
}

impl MainThreadHandler<'_> for MainThread {}

impl HostLatencyImpl for MainThread {
    fn changed(&self) {
        tracing::debug!("CLAP plugin: latency changed");
        self.push(PluginRequest::LatencyChanged);
    }
}

impl HostParamsImplMainThread for MainThread {
    fn rescan(&self, flags: ParamRescanFlags) {
        tracing::debug!(?flags, "CLAP plugin: parameters to rescan");
        self.push(PluginRequest::RescanParams);
    }

    fn clear(&self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

impl HostStateImpl for MainThread {
    fn mark_dirty(&self) {}
}

impl HostAudioPortsImpl for MainThread {
    fn is_rescan_flag_supported(&self, _flag: AudioPortRescanFlags) -> bool {
        false
    }

    fn rescan(&self, flags: AudioPortRescanFlags) {
        tracing::debug!(?flags, "CLAP plugin: audio ports to rescan, restarting it");
        self.push(PluginRequest::Restart);
    }
}

impl HostHandlers for ClapHost {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder
            .register::<HostLog>()
            .register::<HostLatency>()
            .register::<HostParams>()
            .register::<HostState>()
            .register::<HostAudioPorts>()
            .register::<HostGui>()
            .register::<HostTimer>();
        #[cfg(unix)]
        builder.register::<HostPosixFd>();
    }
}

/// Main-thread side of a CLAP plugin.
pub struct ClapInstance {
    descriptor: PluginDescriptor,
    instance: PluginInstance<ClapHost>,
    gui: Option<GuiConfiguration<'static>>,
    gui_open: bool,
    /// Host window of an embedded editor.
    window: Option<HostWindow>,
}

impl ClapInstance {
    pub fn new(descriptor: &PluginDescriptor) -> Result<Self, String> {
        // SAFETY: loading the library runs its initialisers, as for any plugin host.
        let entry = unsafe { PluginEntry::load(&descriptor.path) }.map_err(|e| e.to_string())?;
        let info = HostInfo::new(
            "AudioStrada",
            "AudioStrada contributors",
            "",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|e| e.to_string())?;
        let id = CString::new(descriptor.id.as_str()).map_err(|e| e.to_string())?;
        let instance = PluginInstance::<ClapHost>::new(
            |_| Shared::default(),
            |_| MainThread::new(),
            &entry,
            &id,
            &info,
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            descriptor: descriptor.clone(),
            instance,
            gui: None,
            gui_open: false,
            window: None,
        })
    }

    /// Channel count of the main input and output ports.
    /// Every audio port the plugin declares, inputs then outputs: its channel counts and which
    /// one is the main port. The host must give a buffer to each (a sidechain too): Pro-Q 4
    /// wrote through the sidechain it was not given, and crashed, once its window was open.
    fn ports(&mut self) -> (Ports, Ports) {
        let handle = self.instance.plugin_handle();
        let Some(ports) = handle.get_extension::<PluginAudioPorts>() else {
            return (
                Ports {
                    channels: vec![2],
                    main: 0,
                },
                Ports {
                    channels: vec![2],
                    main: 0,
                },
            );
        };
        let list = |input: bool| -> Ports {
            let mut infos = Vec::new();
            for i in 0..ports.count(&handle, input) {
                let mut buffer = AudioPortInfoBuffer::new();
                if let Some(p) = ports.get(&handle, i, input, &mut buffer) {
                    infos.push((p.flags.contains(AudioPortFlags::IS_MAIN), p.channel_count));
                }
            }
            Ports {
                main: infos.iter().position(|(main, _)| *main).unwrap_or(0),
                channels: infos.iter().map(|(_, c)| *c as usize).collect(),
            }
        };
        (list(true), list(false))
    }

    /// Calls the plugin timers that are due.
    fn run_timers(&mut self) {
        let now = Instant::now();
        let due: Vec<u32> = self.instance.access_handler(|h| {
            let mut timers = h.timers.borrow_mut();
            timers
                .iter_mut()
                .filter(|t| t.next <= now)
                .map(|t| {
                    t.next = now + t.period;
                    t.id
                })
                .collect()
        });
        if due.is_empty() {
            return;
        }
        let handle = self.instance.plugin_handle();
        if let Some(timer) = handle.get_extension::<PluginTimer>() {
            for id in due {
                timer.on_timer(&handle, TimerId(id));
            }
        }
    }

    /// Tells the plugin which of its file descriptors are ready.
    #[cfg(unix)]
    fn run_fds(&mut self) {
        let fds = self.instance.access_handler(|h| h.fds.borrow().clone());
        if fds.is_empty() {
            return;
        }
        let mut polled: Vec<libc::pollfd> = fds
            .iter()
            .map(|&(fd, flags)| libc::pollfd {
                fd,
                events: (if flags.contains(FdFlags::READ) {
                    libc::POLLIN
                } else {
                    0
                }) | (if flags.contains(FdFlags::WRITE) {
                    libc::POLLOUT
                } else {
                    0
                }),
                revents: 0,
            })
            .collect();
        // SAFETY: the array is valid for its length; a zero timeout never blocks.
        let ready = unsafe { libc::poll(polled.as_mut_ptr(), polled.len() as libc::nfds_t, 0) };
        if ready <= 0 {
            return;
        }
        let handle = self.instance.plugin_handle();
        let Some(posix) = handle.get_extension::<PluginPosixFd>() else {
            return;
        };
        for p in polled.iter().filter(|p| p.revents != 0) {
            let mut flags = FdFlags::empty();
            if p.revents & libc::POLLIN != 0 {
                flags |= FdFlags::READ;
            }
            if p.revents & libc::POLLOUT != 0 {
                flags |= FdFlags::WRITE;
            }
            if p.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                flags |= FdFlags::ERROR;
            }
            posix.on_fd(&handle, p.fd, flags);
        }
    }

    /// Window support on this platform: floating if offered, embedded otherwise.
    fn gui_configuration(&mut self) -> Option<GuiConfiguration<'static>> {
        let handle = self.instance.plugin_handle();
        let gui = handle.get_extension::<PluginGui>()?;
        let api = GuiApiType::default_for_current_platform()?;
        [true, false]
            .into_iter()
            .map(|is_floating| GuiConfiguration {
                api_type: api,
                is_floating,
            })
            .find(|c| gui.is_api_supported(&handle, *c))
    }
}

impl std::fmt::Debug for ClapInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapInstance")
            .field("plugin", &self.descriptor.id)
            .finish_non_exhaustive()
    }
}

impl crate::PluginInstance for ClapInstance {
    fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    fn activate(
        &mut self,
        sample_rate: u32,
        max_frames: usize,
    ) -> Result<Box<dyn PluginProcessor>, String> {
        let (in_ports, out_ports) = self.ports();
        let configuration = PluginAudioConfiguration {
            sample_rate: f64::from(sample_rate),
            min_frames_count: 1,
            max_frames_count: max_frames.max(1) as u32,
        };
        let processor = self
            .instance
            .activate(|_, _| (), configuration)
            .map_err(|e| e.to_string())?;
        let max = max_frames.max(1);
        let buffers = |ports: &Ports| -> Vec<Vec<Vec<f32>>> {
            ports
                .channels
                .iter()
                .map(|&c| vec![vec![0.0; max]; c])
                .collect()
        };
        let total = |ports: &Ports| ports.channels.iter().sum::<usize>();
        Ok(Box::new(ClapProcessor {
            processor: processor.into(),
            inputs: buffers(&in_ports),
            outputs: buffers(&out_ports),
            main_in: in_ports.main,
            main_out: out_ports.main,
            key_in: (0..in_ports.channels.len()).find(|&i| i != in_ports.main),
            keyed: false,
            input_ports: AudioPorts::with_capacity(total(&in_ports), in_ports.channels.len()),
            output_ports: AudioPorts::with_capacity(total(&out_ports), out_ports.channels.len()),
            events_in: EventBuffer::with_capacity(2 * MAX_EVENTS),
            events_out: EventBuffer::with_capacity(MAX_EVENTS),
            steady: 0,
            transport: None,
        }))
    }

    fn try_deactivate(&mut self) -> bool {
        !self.instance.is_active() || self.instance.try_deactivate().is_ok()
    }

    fn params(&mut self) -> Vec<ParamDesc> {
        let handle = self.instance.plugin_handle();
        let Some(params) = handle.get_extension::<PluginParams>() else {
            return Vec::new();
        };
        let mut buffer = ParamInfoBuffer::new();
        (0..params.count(&handle))
            .filter_map(|i| {
                let info = params.get_info(&handle, i, &mut buffer)?;
                Some(ParamDesc {
                    id: info.id.get(),
                    name: text(info.name),
                    module: text(info.module),
                    min: info.min_value,
                    max: info.max_value,
                    default: info.default_value,
                    stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
                    hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
                    read_only: info.flags.contains(ParamInfoFlags::IS_READONLY),
                })
            })
            .collect()
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        let handle = self.instance.plugin_handle();
        handle
            .get_extension::<PluginParams>()?
            .get_value(&handle, ClapId::new(id))
    }

    fn value_text(&mut self, id: u32, value: f64) -> Option<String> {
        let handle = self.instance.plugin_handle();
        let params = handle.get_extension::<PluginParams>()?;
        let mut buffer = [0u8; 128];
        let written = params
            .value_to_text(&handle, ClapId::new(id), value, &mut buffer)
            .ok()?;
        Some(text(written))
    }

    fn latency(&mut self) -> u32 {
        let handle = self.instance.plugin_handle();
        handle
            .get_extension::<PluginLatency>()
            .map_or(0, |l| l.get(&handle))
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        let handle = self.instance.plugin_handle();
        let state = handle.get_extension::<PluginState>()?;
        let mut data = Vec::new();
        state.save(&handle, &mut data).ok()?;
        Some(data)
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        let handle = self.instance.plugin_handle();
        handle
            .get_extension::<PluginState>()
            .is_some_and(|state| state.load(&handle, &mut &data[..]).is_ok())
    }

    fn poll(&mut self) -> Vec<PluginRequest> {
        let (callback, restart, closed) = self.instance.access_shared_handler(|s| {
            (
                s.callback.swap(false, Ordering::Relaxed),
                s.restart.swap(false, Ordering::Relaxed),
                s.gui_closed.swap(false, Ordering::Relaxed),
            )
        });
        if callback {
            self.instance.call_on_main_thread_callback();
        }
        self.run_timers();
        #[cfg(unix)]
        self.run_fds();
        let mut requests = self
            .instance
            .access_handler(|h| std::mem::take(&mut *h.requests.borrow_mut()));
        if restart {
            tracing::debug!(plugin = %self.descriptor.name, "CLAP plugin: asked for a restart");
        }
        if restart && !requests.contains(&PluginRequest::Restart) {
            requests.push(PluginRequest::Restart);
        }
        let resize = self
            .instance
            .access_shared_handler(|s| s.resize.swap(0, Ordering::Relaxed));
        let mut window_closed = false;
        if let Some(window) = &mut self.window {
            if resize != 0 {
                window.resize((resize >> 32) as u32, resize as u32);
            }
            window_closed = window.poll();
            requests.extend(
                window
                    .take_keys()
                    .into_iter()
                    .map(|(sym, mods)| PluginRequest::Key(sym, mods)),
            );
        }
        if (closed || window_closed) && self.gui_open {
            self.close_gui();
            requests.push(PluginRequest::GuiClosed);
        }
        requests
    }

    fn has_sidechain(&mut self) -> bool {
        self.ports().0.channels.len() > 1
    }

    fn has_gui(&mut self) -> bool {
        if self.gui.is_none() {
            self.gui = self.gui_configuration();
        }
        self.gui.is_some()
    }

    fn open_gui(&mut self, title: &str) -> Result<(), String> {
        if self.gui_open {
            return Ok(());
        }
        let configuration = self
            .gui_configuration()
            .ok_or("this plugin has no window for this system")?;
        let handle = self.instance.plugin_handle();
        let gui = handle
            .get_extension::<PluginGui>()
            .ok_or("this plugin has no window")?;
        gui.create(&handle, configuration)
            .map_err(|e| format!("cannot create the window: {e:?}"))?;
        if configuration.is_floating {
            if let Ok(title) = CString::new(title) {
                gui.suggest_title(&handle, &title);
            }
            let parent = crate::window::owner();
            if parent != 0 {
                let window = if cfg!(windows) {
                    // SAFETY: AudioStrada's main window lives as long as the application.
                    unsafe { Window::from_win32_hwnd(parent as usize as *mut std::ffi::c_void) }
                } else {
                    Window::from_x11_handle(parent as std::ffi::c_ulong)
                };
                // SAFETY: as above.
                let _ = unsafe { gui.set_transient(&handle, window) };
            }
        } else {
            let size = gui.get_size(&handle).unwrap_or(GuiSize {
                width: 640,
                height: 480,
            });
            let window = match HostWindow::new(title, size.width, size.height) {
                Ok(window) => window,
                Err(e) => {
                    gui.destroy(&handle);
                    return Err(e);
                }
            };
            let parent = match window.handle() {
                #[allow(clippy::useless_conversion, reason = "c_ulong is 32 bits on Windows")]
                NativeHandle::X11(id) => Window::from_x11_handle(std::ffi::c_ulong::from(id)),
                // SAFETY: the window stays alive until the editor is destroyed.
                NativeHandle::Win32(hwnd) => unsafe { Window::from_win32_hwnd(hwnd) },
                // SAFETY: as above.
                NativeHandle::Cocoa(view) => unsafe { Window::from_cocoa_nsview(view) },
            };
            // SAFETY: the host window outlives the editor (destroyed in close_gui first).
            if let Err(e) = unsafe { gui.set_parent(&handle, parent) } {
                gui.destroy(&handle);
                return Err(format!("cannot embed the window: {e:?}"));
            }
            self.window = Some(window);
        }
        if let Err(e) = gui.show(&handle) {
            gui.destroy(&handle);
            self.window = None;
            return Err(format!("cannot show the window: {e:?}"));
        }
        self.gui_open = true;
        Ok(())
    }

    fn close_gui(&mut self) {
        if !self.gui_open {
            return;
        }
        let handle = self.instance.plugin_handle();
        if let Some(gui) = handle.get_extension::<PluginGui>() {
            let _ = gui.hide(&handle);
            gui.destroy(&handle);
        }
        self.window = None;
        self.gui_open = false;
    }

    fn gui_open(&self) -> bool {
        self.gui_open
    }

    fn raise_gui(&mut self) {
        if let Some(window) = &mut self.window {
            window.raise();
        }
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        crate::PluginInstance::close_gui(self);
    }
}

/// The audio ports of one direction: channels per port, and the main one.
struct Ports {
    channels: Vec<usize>,
    main: usize,
}

/// Audio side of a CLAP plugin.
pub struct ClapProcessor {
    processor: PluginAudioProcessor<ClapHost>,
    /// A buffer per channel of each port; the track plays into and out of the main ports,
    /// the others (sidechains, aux outputs) get silence and are left unheard.
    inputs: Vec<Vec<Vec<f32>>>,
    outputs: Vec<Vec<Vec<f32>>>,
    main_in: usize,
    main_out: usize,
    /// The first other input: the sidechain, fed by a key track.
    key_in: Option<usize>,
    /// The sidechain holds the key of the last block.
    keyed: bool,
    input_ports: AudioPorts,
    output_ports: AudioPorts,
    events_in: EventBuffer,
    events_out: EventBuffer,
    steady: u64,
    transport: Option<TransportEvent>,
}

impl std::fmt::Debug for ClapProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapProcessor")
            .field("steady", &self.steady)
            .finish_non_exhaustive()
    }
}

impl ClapProcessor {
    fn run(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<(&[f32], &[f32])>,
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        let max = self
            .outputs
            .get(self.main_out)
            .and_then(|p| p.first())
            .map_or(0, Vec::len);
        let n = left.len().min(right.len()).min(max);
        if n == 0 || self.processor.ensure_processing_started().is_err() {
            return;
        }
        self.events_in.clear();
        for &(id, value) in params_in.iter().take(MAX_EVENTS) {
            self.events_in.push(&ParamValueEvent::new(
                0,
                ClapId::new(id),
                Pckn::match_all(),
                value,
            ));
        }
        for event in midi.iter().take(MAX_EVENTS) {
            let time = crate::block_time(event, n);
            let channel = u16::from(event.channel());
            if let Some((key, velocity)) = event.as_note_on() {
                let pckn = Pckn::new(0u16, channel, u16::from(key), Match::All);
                self.events_in
                    .push(&NoteOnEvent::new(time, pckn, f64::from(velocity) / 127.0));
            } else if let Some(key) = event.as_note_off() {
                let pckn = Pckn::new(0u16, channel, u16::from(key), Match::All);
                self.events_in.push(&NoteOffEvent::new(time, pckn, 0.0));
            } else {
                self.events_in
                    .push(&ClapMidiEvent::new(time, 0, event.data));
            }
        }
        if let Some(main) = self.inputs.get_mut(self.main_in) {
            crate::fill_inputs(main, left, right, n);
        }
        if let Some(port) = self.key_in.and_then(|i| self.inputs.get_mut(i)) {
            crate::fill_key(port, key, &mut self.keyed, n);
        }
        let inputs = self
            .input_ports
            .with_input_buffers(self.inputs.iter_mut().map(|port| AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(
                    port.iter_mut().map(|b| InputChannel::variable(&mut b[..n])),
                ),
            }));
        let mut outputs = self
            .output_ports
            .with_output_buffers(self.outputs.iter_mut().map(|port| AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(
                    port.iter_mut().map(|b| &mut b[..n]),
                ),
            }));
        let result = self.processor.ensure_processing_started().map(|p| {
            p.process(
                &inputs,
                &mut outputs,
                &self.events_in.as_input(),
                &mut self.events_out.as_output(),
                Some(self.steady),
                self.transport.as_ref(),
            )
        });
        self.steady += n as u64;
        if matches!(result, Ok(Ok(_)))
            && let Some(main) = self.outputs.get(self.main_out)
        {
            crate::take_outputs(main, left, right, n);
        }
        for event in self.events_out.iter() {
            if let Some(CoreEventSpace::ParamValue(e)) = event.as_core_event()
                && let Some(id) = e.param_id()
            {
                params_out(id.get(), e.value());
            }
        }
        self.events_out.clear();
    }
}

impl PluginProcessor for ClapProcessor {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        self.run(left, right, None, params_in, midi, params_out);
    }

    fn process_keyed(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: (&[f32], &[f32]),
        params_in: &[(u32, f64)],
        midi: &[MidiEvent],
        params_out: &mut dyn FnMut(u32, f64),
    ) {
        self.run(left, right, Some(key), params_in, midi, params_out);
    }
    fn reset(&mut self) {
        self.processor.reset();
    }

    fn set_transport(&mut self, t: &crate::Transport) {
        let mut flags = TransportFlags::HAS_TEMPO
            | TransportFlags::HAS_BEATS_TIMELINE
            | TransportFlags::HAS_TIME_SIGNATURE;
        if t.playing {
            flags |= TransportFlags::IS_PLAYING;
        }
        self.transport = Some(TransportEvent {
            header: EventHeader::new_core(0, EventFlags::empty()),
            flags,
            song_pos_beats: BeatTime::from_float(t.beats),
            song_pos_seconds: SecondsTime::from_float(0.0),
            tempo: t.tempo,
            tempo_inc: 0.0,
            loop_start_beats: BeatTime::from_float(0.0),
            loop_end_beats: BeatTime::from_float(0.0),
            loop_start_seconds: SecondsTime::from_float(0.0),
            loop_end_seconds: SecondsTime::from_float(0.0),
            bar_start: BeatTime::from_float(t.bar_start),
            bar_number: t.bar,
            time_signature_numerator: t.numerator as u16,
            time_signature_denominator: t.denominator as u16,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginInstance as _;

    /// Real plugins for tests, from `AUDIOSTRADA_TEST_CLAP` (a folder of `.clap` files).
    fn test_plugins() -> Vec<PluginDescriptor> {
        let Some(dir) = std::env::var_os("AUDIOSTRADA_TEST_CLAP") else {
            return Vec::new();
        };
        crate::scan::find_files(Format::Clap, &[dir.into()])
            .iter()
            .filter_map(|p| scan(p).ok())
            .flatten()
            .collect()
    }

    #[test]
    fn hosts_real_plugins_when_available() {
        let plugins = test_plugins();
        if plugins.is_empty() {
            eprintln!("AUDIOSTRADA_TEST_CLAP not set, skipping");
            return;
        }
        // A few plugins of every file.
        let mut picked: Vec<&PluginDescriptor> = Vec::new();
        for p in plugins.iter().filter(|p| !p.instrument) {
            if picked.iter().filter(|q| q.path == p.path).count() < 3 {
                picked.push(p);
            }
        }
        for descriptor in picked {
            let mut instance = ClapInstance::new(descriptor).unwrap();
            let params = instance.params();
            let mut processor = instance.activate(48_000, 256).unwrap();
            let (mut left, mut right) = (vec![0.5f32; 256], vec![0.5f32; 256]);
            let mut changed = Vec::new();
            let first = params
                .iter()
                .find(|p| !p.read_only)
                .map(|p| (p.id, p.default));
            for _ in 0..20 {
                processor.process(
                    &mut left,
                    &mut right,
                    first.as_slice(),
                    &[],
                    &mut |id, v| changed.push((id, v)),
                );
            }
            assert!(
                left.iter().chain(&right).all(|s| s.is_finite()),
                "{}",
                descriptor.name
            );
            let state = instance.save_state();
            if let Some(state) = &state {
                assert!(instance.load_state(state), "{}", descriptor.name);
            }
            instance.poll();
            assert!(!instance.try_deactivate(), "processor still alive");
            drop(processor);
            assert!(instance.try_deactivate());
            eprintln!(
                "hosted {} ({} params, state {:?} bytes, latency {})",
                descriptor.name,
                params.len(),
                state.map(|s| s.len()),
                instance.latency()
            );
        }
    }
}
