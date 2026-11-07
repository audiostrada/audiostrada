//! ASIO backend (Windows, 64-bit): loads a driver through COM and processes in its buffer switch.
//!
//! Interface layouts follow the Steinberg ASIO SDK 2.3 headers (GPLv3 licensed). ASIO callbacks
//! carry no user pointer, so a single stream at a time is supported, as with every ASIO host.

use std::ffi::{CStr, c_char, c_void};
use std::fmt;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CLSIDFromString, COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetDesktopWindow, GetMessageW, MSG, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, TranslateMessage, WM_QUIT,
};
use windows::core::{GUID, PCWSTR, PWSTR, w};

use crate::convert::SampleFormat;
use crate::stream::CallbackRunner;
use crate::timing::asio_buffer_size;
use crate::{
    AudioBackend, AudioCallback, AudioStream, BackendKind, CallbackFactory, ChannelBuffers,
    DeviceInfo, IoError, StreamConfig, StreamInfo, StreamStatus,
};

const ASE_OK: i32 = 0;
const ASIO_TRUE: i32 = 1;

// asioMessage selectors (asio.h).
const SELECTOR_SUPPORTED: i32 = 1;
const ENGINE_VERSION: i32 = 2;
const RESET_REQUEST: i32 = 3;
const RESYNC_REQUEST: i32 = 5;
const LATENCIES_CHANGED: i32 = 6;
const SUPPORTS_TIME_INFO: i32 = 7;
const OVERLOAD: i32 = 15;

// ASIOSampleType values (asio.h).
const ST_INT16_LSB: i32 = 16;
const ST_INT24_LSB: i32 = 17;
const ST_INT32_LSB: i32 = 18;
const ST_FLOAT32_LSB: i32 = 19;
const ST_FLOAT64_LSB: i32 = 20;
const ST_INT32_LSB16: i32 = 24;
const ST_INT32_LSB18: i32 = 25;
const ST_INT32_LSB20: i32 = 26;
const ST_INT32_LSB24: i32 = 27;

fn backend_error(message: impl ToString) -> IoError {
    IoError::backend(BackendKind::Asio, message)
}

#[repr(C)]
struct IAsio {
    vtbl: *const IAsioVtbl,
}

type This = *mut IAsio;

/// IUnknown followed by the IASIO virtual methods, in declaration order (iasiodrv.h).
#[repr(C)]
#[allow(dead_code)]
struct IAsioVtbl {
    query_interface: unsafe extern "system" fn(This, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(This) -> u32,
    release: unsafe extern "system" fn(This) -> u32,
    init: unsafe extern "system" fn(This, *mut c_void) -> i32,
    get_driver_name: unsafe extern "system" fn(This, *mut c_char),
    get_driver_version: unsafe extern "system" fn(This) -> i32,
    get_error_message: unsafe extern "system" fn(This, *mut c_char),
    start: unsafe extern "system" fn(This) -> i32,
    stop: unsafe extern "system" fn(This) -> i32,
    get_channels: unsafe extern "system" fn(This, *mut i32, *mut i32) -> i32,
    get_latencies: unsafe extern "system" fn(This, *mut i32, *mut i32) -> i32,
    get_buffer_size: unsafe extern "system" fn(This, *mut i32, *mut i32, *mut i32, *mut i32) -> i32,
    can_sample_rate: unsafe extern "system" fn(This, f64) -> i32,
    get_sample_rate: unsafe extern "system" fn(This, *mut f64) -> i32,
    set_sample_rate: unsafe extern "system" fn(This, f64) -> i32,
    get_clock_sources: unsafe extern "system" fn(This, *mut c_void, *mut i32) -> i32,
    set_clock_source: unsafe extern "system" fn(This, i32) -> i32,
    get_sample_position: unsafe extern "system" fn(This, *mut c_void, *mut c_void) -> i32,
    get_channel_info: unsafe extern "system" fn(This, *mut ChannelInfo) -> i32,
    create_buffers:
        unsafe extern "system" fn(This, *mut BufferInfo, i32, i32, *const Callbacks) -> i32,
    dispose_buffers: unsafe extern "system" fn(This) -> i32,
    control_panel: unsafe extern "system" fn(This) -> i32,
    future: unsafe extern "system" fn(This, i32, *mut c_void) -> i32,
    output_ready: unsafe extern "system" fn(This) -> i32,
}

#[repr(C)]
#[allow(dead_code)]
struct ChannelInfo {
    channel: i32,
    is_input: i32,
    is_active: i32,
    channel_group: i32,
    sample_type: i32,
    name: [c_char; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BufferInfo {
    is_input: i32,
    channel_num: i32,
    buffers: [*mut c_void; 2],
}

#[repr(C)]
struct Callbacks {
    buffer_switch: unsafe extern "C" fn(i32, i32),
    sample_rate_did_change: unsafe extern "C" fn(f64),
    asio_message: unsafe extern "C" fn(i32, i32, *mut c_void, *mut f64) -> i32,
    buffer_switch_time_info: unsafe extern "C" fn(*mut c_void, i32, i32) -> *mut c_void,
}

static CALLBACKS: Callbacks = Callbacks {
    buffer_switch,
    sample_rate_did_change,
    asio_message,
    buffer_switch_time_info,
};

/// State of the running stream, only touched by the driver's callback thread once published.
static STATE: AtomicPtr<State> = AtomicPtr::new(ptr::null_mut());

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoCreateInstance(
        clsid: *const GUID,
        outer: *mut c_void,
        context: u32,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> i32;
}

/// Sample encodings found in ASIO driver buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Encoding {
    Plain(SampleFormat),
    F64,
    /// 32-bit container holding a value of the given bit depth.
    Aligned(u32),
}

impl Encoding {
    fn from_asio(sample_type: i32) -> Option<Self> {
        Some(match sample_type {
            ST_INT16_LSB => Encoding::Plain(SampleFormat::S16),
            ST_INT24_LSB => Encoding::Plain(SampleFormat::S24Packed),
            ST_INT32_LSB => Encoding::Plain(SampleFormat::S32),
            ST_FLOAT32_LSB => Encoding::Plain(SampleFormat::F32),
            ST_FLOAT64_LSB => Encoding::F64,
            ST_INT32_LSB16 => Encoding::Aligned(16),
            ST_INT32_LSB18 => Encoding::Aligned(18),
            ST_INT32_LSB20 => Encoding::Aligned(20),
            ST_INT32_LSB24 => Encoding::Aligned(24),
            _ => return None,
        })
    }

    fn bytes(self) -> usize {
        match self {
            Encoding::Plain(f) => f.bytes(),
            Encoding::F64 => 8,
            Encoding::Aligned(_) => 4,
        }
    }

    #[inline]
    fn read(self, b: &[u8]) -> f32 {
        match self {
            Encoding::Plain(f) => f.read(b),
            Encoding::F64 => {
                f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32
            }
            Encoding::Aligned(bits) => {
                i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / (1i64 << (bits - 1)) as f32
            }
        }
    }

    #[inline]
    fn write(self, v: f32, b: &mut [u8]) {
        match self {
            Encoding::Plain(f) => f.write(v, b),
            Encoding::F64 => b[..8].copy_from_slice(&f64::from(v).to_le_bytes()),
            Encoding::Aligned(bits) => {
                let max = ((1i64 << (bits - 1)) - 1) as f32;
                b[..4].copy_from_slice(&((v.clamp(-1.0, 1.0) * max) as i32).to_le_bytes());
            }
        }
    }
}

struct State {
    driver: This,
    inputs: Vec<(BufferInfo, Encoding)>,
    outputs: Vec<(BufferInfo, Encoding)>,
    frames: usize,
    storage: ChannelBuffers,
    runner: CallbackRunner,
    output_ready: bool,
}

impl State {
    fn process(&mut self, index: usize) {
        let frames = self.frames;
        for (ch, (info, encoding)) in self.inputs.iter().enumerate() {
            let size = encoding.bytes();
            // SAFETY: the driver's buffer half `index` holds `frames` samples of this encoding.
            let src = unsafe {
                std::slice::from_raw_parts(info.buffers[index].cast::<u8>(), frames * size)
            };
            for (f, sample) in self.storage.input_mut(ch)[..frames].iter_mut().enumerate() {
                *sample = encoding.read(&src[f * size..]);
            }
        }
        self.runner.run(&mut self.storage.buffers(frames));
        for (ch, (info, encoding)) in self.outputs.iter().enumerate() {
            let size = encoding.bytes();
            // SAFETY: as above, for the output buffer half the driver handed us.
            let dst = unsafe {
                std::slice::from_raw_parts_mut(info.buffers[index].cast::<u8>(), frames * size)
            };
            for (f, sample) in self.storage.output(ch)[..frames].iter().enumerate() {
                encoding.write(*sample, &mut dst[f * size..]);
            }
        }
        if self.output_ready {
            // SAFETY: the driver outlives the published state.
            unsafe { ((*(*self.driver).vtbl).output_ready)(self.driver) };
        }
    }
}

unsafe extern "C" fn buffer_switch(index: i32, _direct: i32) {
    let state = STATE.load(Ordering::Acquire);
    // SAFETY: published state is only used by the driver callback thread until unpublished.
    if let Some(state) = unsafe { state.as_mut() } {
        state.process(index as usize & 1);
    }
}

unsafe extern "C" fn buffer_switch_time_info(
    params: *mut c_void,
    index: i32,
    direct: i32,
) -> *mut c_void {
    // SAFETY: same contract as buffer_switch.
    unsafe { buffer_switch(index, direct) };
    params
}

unsafe extern "C" fn sample_rate_did_change(_rate: f64) {}

unsafe extern "C" fn asio_message(
    selector: i32,
    value: i32,
    _message: *mut c_void,
    _opt: *mut f64,
) -> i32 {
    let state = STATE.load(Ordering::Acquire);
    // SAFETY: only the status counters are read, which are atomics.
    let status = unsafe { state.as_ref() }.map(|s| s.runner.status());
    match selector {
        SELECTOR_SUPPORTED => matches!(
            value,
            ENGINE_VERSION
                | RESET_REQUEST
                | RESYNC_REQUEST
                | LATENCIES_CHANGED
                | SUPPORTS_TIME_INFO
                | OVERLOAD
        ) as i32,
        ENGINE_VERSION => 2,
        RESET_REQUEST => {
            if let Some(status) = status {
                status.set_failed();
            }
            1
        }
        RESYNC_REQUEST | OVERLOAD => {
            if let Some(status) = status {
                status.add_xruns(1);
            }
            1
        }
        LATENCIES_CHANGED | SUPPORTS_TIME_INFO => 1,
        _ => 0,
    }
}

/// Installed drivers from `HKLM\SOFTWARE\ASIO`: (name, CLSID).
fn installed_drivers() -> Vec<(String, GUID)> {
    let mut drivers = Vec::new();
    let mut root = HKEY::default();
    // SAFETY: registry calls with valid buffers; every opened key is closed.
    unsafe {
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\ASIO"),
            None,
            KEY_READ,
            &mut root,
        )
        .0 != 0
        {
            return drivers;
        }
        for index in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(
                root,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .0 != 0
            {
                break;
            }
            let mut sub = HKEY::default();
            if RegOpenKeyExW(root, PCWSTR(name.as_ptr()), None, KEY_READ, &mut sub).0 != 0 {
                continue;
            }
            let mut data = [0u16; 128];
            let mut size = (data.len() * 2) as u32;
            let found = RegQueryValueExW(
                sub,
                w!("CLSID"),
                None,
                None,
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
            .0 == 0;
            let _ = RegCloseKey(sub);
            if found && let Ok(clsid) = CLSIDFromString(PCWSTR(data.as_ptr())) {
                drivers.push((String::from_utf16_lossy(&name[..len as usize]), clsid));
            }
        }
        let _ = RegCloseKey(root);
    }
    drivers
}

#[derive(Debug)]
pub struct AsioBackend {
    watcher: crate::watch::DeviceWatcher,
}

impl AsioBackend {
    pub fn connect() -> Result<Self, IoError> {
        if installed_drivers().is_empty() {
            return Err(IoError::BackendUnavailable(BackendKind::Asio));
        }
        Ok(Self {
            watcher: crate::watch::DeviceWatcher::start(|| {
                installed_drivers().into_iter().map(|d| d.0).collect()
            }),
        })
    }
}

impl AudioBackend for AsioBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Asio
    }

    fn device_generation(&self) -> u64 {
        self.watcher.generation()
    }

    /// Channel counts are only known once a driver is loaded, so they are reported as 0.
    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        Ok(installed_drivers()
            .into_iter()
            .enumerate()
            .map(|(i, (name, _))| DeviceInfo {
                id: name.clone(),
                name,
                inputs: 0,
                outputs: 0,
                is_default_input: i == 0,
                is_default_output: i == 0,
            })
            .collect())
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        if !STATE.load(Ordering::Acquire).is_null() {
            return Err(backend_error("an ASIO stream is already running"));
        }
        let drivers = installed_drivers();
        let wanted = match (&config.input_device, &config.output_device) {
            (Some(a), Some(b)) if a != b => {
                return Err(IoError::UnsupportedConfig(
                    "ASIO uses a single driver for input and output".into(),
                ));
            }
            (Some(name), _) | (None, Some(name)) => Some(name.as_str()),
            (None, None) => None,
        };
        let (name, clsid) = match wanted {
            Some(name) => drivers
                .into_iter()
                .find(|(n, _)| n == name)
                .ok_or_else(|| IoError::DeviceNotFound(name.into()))?,
            None => drivers
                .into_iter()
                .next()
                .ok_or(IoError::BackendUnavailable(BackendKind::Asio))?,
        };
        let status = Arc::new(StreamStatus::default());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (callback_tx, callback_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let thread = {
            let config = config.clone();
            let status = Arc::clone(&status);
            thread::Builder::new()
                .name("audiostrada-asio-control".into())
                .spawn(move || {
                    control_thread(
                        clsid,
                        name,
                        &config,
                        status,
                        &ready_tx,
                        &callback_rx,
                        &started_tx,
                    )
                })
                .map_err(backend_error)?
        };
        let result = match ready_rx.recv() {
            Ok(Ok(info)) => {
                let _ = callback_tx.send(factory(&info));
                match started_rx.recv() {
                    Ok(Ok(thread_id)) => Ok((info, thread_id)),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(backend_error("driver thread exited during start")),
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(backend_error("driver thread exited during setup")),
        };
        match result {
            Ok((info, thread_id)) => Ok(Box::new(AsioStream {
                info,
                status,
                thread_id,
                thread: Some(thread),
            })),
            Err(e) => {
                let _ = thread.join();
                Err(e)
            }
        }
    }
}

/// Loads the driver on an STA thread, negotiates the format, waits for the callback, then keeps
/// the driver running while pumping messages until asked to quit.
fn control_thread(
    clsid: GUID,
    name: String,
    config: &StreamConfig,
    status: Arc<StreamStatus>,
    ready: &mpsc::Sender<Result<StreamInfo, IoError>>,
    callback: &mpsc::Receiver<Box<dyn AudioCallback>>,
    started: &mpsc::Sender<Result<u32, IoError>>,
) {
    // SAFETY: COM initialization and message queue calls on the current thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    match Driver::open(clsid, &name, config) {
        Ok(mut driver) => {
            let _ = ready.send(Ok(driver.info.clone()));
            match callback.recv().map(|cb| driver.start(config, cb, status)) {
                Ok(Ok(())) => {
                    // SAFETY: plain Win32 call.
                    let _ = started.send(Ok(unsafe { GetCurrentThreadId() }));
                    let mut msg = MSG::default();
                    // SAFETY: standard message loop on this thread's queue, ended by WM_QUIT.
                    unsafe {
                        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                    driver.close();
                }
                Ok(Err(e)) => {
                    let _ = started.send(Err(e));
                    driver.release();
                }
                Err(_) => driver.release(),
            }
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
    // SAFETY: balances CoInitializeEx above.
    unsafe { CoUninitialize() };
}

/// Format agreed with the driver, before buffers are created.
struct Negotiated {
    infos: Vec<BufferInfo>,
    encodings: Vec<Encoding>,
    inputs: usize,
    frames: i32,
}

struct Driver {
    this: This,
    info: StreamInfo,
    negotiated: Option<Negotiated>,
}

impl Driver {
    fn call<R>(&self, f: impl FnOnce(&IAsioVtbl, This) -> R) -> R {
        // SAFETY: `this` is a live driver instance until `close`.
        unsafe { f(&*(*self.this).vtbl, self.this) }
    }

    fn error_message(&self) -> String {
        let mut buf = [0 as c_char; 128];
        self.call(|v, t| unsafe { (v.get_error_message)(t, buf.as_mut_ptr()) });
        // SAFETY: the driver writes a NUL terminated string into the zeroed buffer.
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    fn check(&self, what: &str, result: i32) -> Result<(), IoError> {
        if result == ASE_OK {
            Ok(())
        } else {
            Err(backend_error(format!(
                "{what} failed ({result}): {}",
                self.error_message()
            )))
        }
    }

    /// Loads and initializes the driver, then negotiates rate, buffer size and sample types.
    fn open(clsid: GUID, name: &str, config: &StreamConfig) -> Result<Self, IoError> {
        let mut object = ptr::null_mut();
        // SAFETY: ASIO drivers use their CLSID as interface id.
        let hr = unsafe {
            CoCreateInstance(
                &clsid,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER.0,
                &clsid,
                &mut object,
            )
        };
        if hr < 0 || object.is_null() {
            return Err(backend_error(format!(
                "cannot load driver {name} (0x{hr:08x})"
            )));
        }
        let mut driver = Self {
            this: object.cast(),
            info: StreamInfo {
                backend: BackendKind::Asio,
                device: name.to_owned(),
                sample_rate: 0,
                buffer_size: 0,
                inputs: config.inputs,
                outputs: config.outputs,
            },
            negotiated: None,
        };
        // SAFETY: the desktop window is a valid system handle for drivers that want one.
        let hwnd = unsafe { GetDesktopWindow() };
        if driver.call(|v, t| unsafe { (v.init)(t, hwnd.0) }) != ASIO_TRUE {
            let message = driver.error_message();
            driver.release();
            return Err(backend_error(format!("{name} init failed: {message}")));
        }
        match driver.negotiate(config) {
            Ok(()) => Ok(driver),
            Err(e) => {
                driver.release();
                Err(e)
            }
        }
    }

    fn negotiate(&mut self, config: &StreamConfig) -> Result<(), IoError> {
        let (mut hw_in, mut hw_out) = (0, 0);
        self.check(
            "getChannels",
            self.call(|v, t| unsafe { (v.get_channels)(t, &mut hw_in, &mut hw_out) }),
        )?;
        // A driver whose device is absent or disabled (Realtek ASIO without its jacks) loads
        // fine but has nothing to stream.
        if hw_in <= 0 && hw_out <= 0 {
            return Err(backend_error(format!(
                "{} has no input or output channel",
                self.info.device
            )));
        }
        if let Some(rate) = config.sample_rate {
            let rate = f64::from(rate);
            self.check(
                "canSampleRate",
                self.call(|v, t| unsafe { (v.can_sample_rate)(t, rate) }),
            )?;
            self.check(
                "setSampleRate",
                self.call(|v, t| unsafe { (v.set_sample_rate)(t, rate) }),
            )?;
        }
        let mut rate = 0.0;
        self.check(
            "getSampleRate",
            self.call(|v, t| unsafe { (v.get_sample_rate)(t, &mut rate) }),
        )?;
        // Some drivers have no rate until one is set: a common one, else no stream. A stream
        // at 0 Hz would have the media resampled to nothing.
        for candidate in [48_000.0, 44_100.0] {
            if rate > 0.0 {
                break;
            }
            if self.call(|v, t| unsafe { (v.can_sample_rate)(t, candidate) }) == ASE_OK
                && self.call(|v, t| unsafe { (v.set_sample_rate)(t, candidate) }) == ASE_OK
            {
                let _ = self.call(|v, t| unsafe { (v.get_sample_rate)(t, &mut rate) });
            }
        }
        if rate <= 0.0 {
            return Err(backend_error(format!(
                "{} reports no sample rate",
                self.info.device
            )));
        }
        let (mut min, mut max, mut preferred, mut granularity) = (0, 0, 0, 0);
        self.check(
            "getBufferSize",
            self.call(|v, t| unsafe {
                (v.get_buffer_size)(t, &mut min, &mut max, &mut preferred, &mut granularity)
            }),
        )?;
        let frames = asio_buffer_size(min, max, preferred, granularity, config.buffer_size as i32);

        let used_in = (config.inputs as i32).min(hw_in.max(0));
        let used_out = (config.outputs as i32).min(hw_out.max(0));
        let infos: Vec<BufferInfo> = (0..used_in)
            .map(|c| (ASIO_TRUE, c))
            .chain((0..used_out).map(|c| (0, c)))
            .map(|(is_input, channel_num)| BufferInfo {
                is_input,
                channel_num,
                buffers: [ptr::null_mut(); 2],
            })
            .collect();
        let mut encodings = Vec::with_capacity(infos.len());
        for info in &infos {
            let mut channel = ChannelInfo {
                channel: info.channel_num,
                is_input: info.is_input,
                is_active: 0,
                channel_group: 0,
                sample_type: 0,
                name: [0; 32],
            };
            self.check(
                "getChannelInfo",
                self.call(|v, t| unsafe { (v.get_channel_info)(t, &mut channel) }),
            )?;
            encodings.push(Encoding::from_asio(channel.sample_type).ok_or_else(|| {
                backend_error(format!("unsupported sample type {}", channel.sample_type))
            })?);
        }
        self.info.sample_rate = rate as u32;
        self.info.buffer_size = frames as u32;
        self.negotiated = Some(Negotiated {
            infos,
            encodings,
            inputs: used_in as usize,
            frames,
        });
        Ok(())
    }

    /// Creates the driver buffers, publishes the callback state and starts streaming.
    fn start(
        &mut self,
        config: &StreamConfig,
        callback: Box<dyn AudioCallback>,
        status: Arc<StreamStatus>,
    ) -> Result<(), IoError> {
        let Some(Negotiated {
            mut infos,
            encodings,
            inputs,
            frames,
        }) = self.negotiated.take()
        else {
            return Err(backend_error("driver not negotiated"));
        };
        self.check(
            "createBuffers",
            self.call(|v, t| unsafe {
                (v.create_buffers)(
                    t,
                    infos.as_mut_ptr(),
                    infos.len() as i32,
                    frames,
                    &CALLBACKS,
                )
            }),
        )?;
        let output_ready = self.call(|v, t| unsafe { (v.output_ready)(t) }) == ASE_OK;
        let paired: Vec<_> = infos.into_iter().zip(encodings).collect();
        let state = Box::new(State {
            driver: self.this,
            inputs: paired[..inputs].to_vec(),
            outputs: paired[inputs..].to_vec(),
            frames: frames as usize,
            storage: ChannelBuffers::new(
                config.inputs as usize,
                config.outputs as usize,
                frames as usize,
            ),
            runner: CallbackRunner::new(callback, status, self.info.sample_rate),
            output_ready,
        });
        STATE.store(Box::into_raw(state), Ordering::Release);
        if let Err(e) = self.check("start", self.call(|v, t| unsafe { (v.start)(t) })) {
            self.unpublish();
            let _ = self.call(|v, t| unsafe { (v.dispose_buffers)(t) });
            return Err(e);
        }
        Ok(())
    }

    fn unpublish(&self) {
        let state = STATE.swap(ptr::null_mut(), Ordering::AcqRel);
        if !state.is_null() {
            // SAFETY: the pointer came from Box::into_raw and the driver no longer calls back.
            drop(unsafe { Box::from_raw(state) });
        }
    }

    fn release(&self) {
        self.call(|v, t| unsafe { (v.release)(t) });
    }

    fn close(self) {
        let _ = self.call(|v, t| unsafe { (v.stop)(t) });
        let _ = self.call(|v, t| unsafe { (v.dispose_buffers)(t) });
        self.unpublish();
        self.release();
    }
}

struct AsioStream {
    info: StreamInfo,
    status: Arc<StreamStatus>,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for AsioStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsioStream")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl AudioStream for AsioStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn status(&self) -> &StreamStatus {
        &self.status
    }
}

impl Drop for AsioStream {
    fn drop(&mut self) {
        // SAFETY: posting WM_QUIT to the control thread's queue, created before setup completed.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
