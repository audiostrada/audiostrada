//! CoreAudio backend (macOS) built on the HAL output unit (AUHAL).
//!
//! Input and output on the same device run in a single AUHAL, sample synchronous. Separate
//! devices use one unit each, joined by a lock-free FIFO.

use std::ffi::c_void;
use std::fmt;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use audiostrada_rt::{Receiver, Sender, channel};
use coreaudio::audio_unit::macos_helpers::{
    get_audio_device_ids, get_default_device_id, get_device_name, set_device_sample_rate,
};
use objc2_audio_toolbox::{
    AURenderCallbackStruct, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioOutputUnitStart,
    AudioOutputUnitStop, AudioUnit, AudioUnitInitialize, AudioUnitRender,
    AudioUnitRenderActionFlags, AudioUnitSetProperty, AudioUnitUninitialize,
    kAudioOutputUnitProperty_CurrentDevice, kAudioOutputUnitProperty_EnableIO,
    kAudioOutputUnitProperty_SetInputCallback, kAudioUnitManufacturer_Apple,
    kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Input,
    kAudioUnitScope_Output, kAudioUnitSubType_HALOutput, kAudioUnitType_Output,
};
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData, kAudioDevicePropertyBufferFrameSize,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, kAudioObjectUnknown,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM,
};

use crate::stream::CallbackRunner;
use crate::{
    AudioBackend, AudioBuffers, AudioStream, BackendKind, CallbackFactory, ChannelBuffers,
    DeviceInfo, IoError, StreamConfig, StreamInfo, StreamStatus,
};

const MAX_FRAMES: usize = 4096;
const INPUT_BUS: u32 = 1;
const OUTPUT_BUS: u32 = 0;
const FIFO_BUFFERS: usize = 8;

fn backend_error(message: impl ToString) -> IoError {
    IoError::backend(BackendKind::CoreAudio, message)
}

fn check(what: &str, status: i32) -> Result<(), IoError> {
    if status == 0 {
        Ok(())
    } else {
        Err(backend_error(format!("{what} failed (OSStatus {status})")))
    }
}

fn address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Total channels a device exposes in one direction.
fn device_channels(device: u32, input: bool) -> u32 {
    let scope = if input {
        kAudioObjectPropertyScopeInput
    } else {
        kAudioObjectPropertyScopeOutput
    };
    let addr = address(kAudioDevicePropertyStreamConfiguration, scope);
    let mut size = 0u32;
    // SAFETY: valid address and size pointer.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            device,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
        )
    };
    if status != 0 || size == 0 {
        return 0;
    }
    let mut storage = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `storage` holds `size` bytes, 8-byte aligned as AudioBufferList requires.
    unsafe {
        if AudioObjectGetPropertyData(
            device,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut storage[0]).cast(),
        ) != 0
        {
            return 0;
        }
        let list = storage.as_ptr().cast::<AudioBufferList>();
        let buffers =
            std::slice::from_raw_parts((*list).mBuffers.as_ptr(), (*list).mNumberBuffers as usize);
        buffers.iter().map(|b| b.mNumberChannels).sum()
    }
}

fn set_device_property<T>(device: u32, selector: u32, value: &T) -> Result<(), IoError> {
    let addr = address(selector, kAudioObjectPropertyScopeGlobal);
    // SAFETY: `value` is a valid `T` for the duration of the call.
    check("AudioObjectSetPropertyData", unsafe {
        AudioObjectSetPropertyData(
            device,
            NonNull::from(&addr),
            0,
            ptr::null(),
            size_of::<T>() as u32,
            NonNull::from(value).cast(),
        )
    })
}

fn get_device_property<T: Default>(device: u32, selector: u32) -> Result<T, IoError> {
    let addr = address(selector, kAudioObjectPropertyScopeGlobal);
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` provides `size` writable bytes.
    check("AudioObjectGetPropertyData", unsafe {
        AudioObjectGetPropertyData(
            device,
            NonNull::from(&addr),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    })?;
    Ok(value)
}

/// Resolves a device name (our device id) or the system default for a direction.
fn resolve_device(name: Option<&str>, input: bool) -> Result<u32, IoError> {
    match name {
        Some(name) => get_audio_device_ids()
            .map_err(backend_error)?
            .into_iter()
            .find(|&id| get_device_name(id).is_ok_and(|n| n == name))
            .ok_or_else(|| IoError::DeviceNotFound(name.into())),
        None => default_device(input).ok_or_else(|| backend_error("no default device")),
    }
}

/// The system default device for a direction; `None` when unset, as the input of a Mac
/// without a microphone, where CoreAudio answers `kAudioObjectUnknown`.
fn default_device(input: bool) -> Option<u32> {
    get_default_device_id(input).filter(|&id| id != kAudioObjectUnknown)
}

/// An AUHAL instance, disposed on drop.
struct Unit(AudioUnit);

// SAFETY: AudioUnit handles may be used from any thread; callbacks are serialized by CoreAudio.
unsafe impl Send for Unit {}

impl Unit {
    fn new_hal(device: u32, input: bool, output: bool) -> Result<Self, IoError> {
        let desc = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: kAudioUnitSubType_HALOutput,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        // SAFETY: plain component lookup and instantiation.
        let unit = unsafe {
            let component = AudioComponentFindNext(ptr::null_mut(), NonNull::from(&desc));
            if component.is_null() {
                return Err(backend_error("HAL output unit not found"));
            }
            let mut unit: AudioUnit = ptr::null_mut();
            check(
                "AudioComponentInstanceNew",
                AudioComponentInstanceNew(component, NonNull::from(&mut unit)),
            )?;
            Self(unit)
        };
        unit.set(
            kAudioOutputUnitProperty_EnableIO,
            kAudioUnitScope_Input,
            INPUT_BUS,
            &u32::from(input),
        )?;
        unit.set(
            kAudioOutputUnitProperty_EnableIO,
            kAudioUnitScope_Output,
            OUTPUT_BUS,
            &u32::from(output),
        )?;
        unit.set(
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &device,
        )?;
        unit.set(
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            0,
            &(MAX_FRAMES as u32),
        )?;
        Ok(unit)
    }

    fn set<T>(&self, id: u32, scope: u32, element: u32, value: &T) -> Result<(), IoError> {
        // SAFETY: `value` is a valid `T` for the duration of the call.
        check("AudioUnitSetProperty", unsafe {
            AudioUnitSetProperty(
                self.0,
                id,
                scope,
                element,
                ptr::from_ref(value).cast(),
                size_of::<T>() as u32,
            )
        })
    }

    /// Sets the planar f32 format seen by our callbacks on one side of the unit.
    fn set_client_format(&self, input_side: bool, channels: u32, rate: f64) -> Result<(), IoError> {
        let asbd = AudioStreamBasicDescription {
            mSampleRate: rate,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat
                | kAudioFormatFlagIsPacked
                | kAudioFormatFlagIsNonInterleaved,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: channels,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let (scope, bus) = if input_side {
            (kAudioUnitScope_Output, INPUT_BUS)
        } else {
            (kAudioUnitScope_Input, OUTPUT_BUS)
        };
        self.set(kAudioUnitProperty_StreamFormat, scope, bus, &asbd)
    }

    fn set_callback(
        &self,
        input: bool,
        function: Callback,
        refcon: *mut c_void,
    ) -> Result<(), IoError> {
        let callback = AURenderCallbackStruct {
            inputProc: Some(function),
            inputProcRefCon: refcon,
        };
        if input {
            self.set(
                kAudioOutputUnitProperty_SetInputCallback,
                kAudioUnitScope_Global,
                0,
                &callback,
            )
        } else {
            self.set(
                kAudioUnitProperty_SetRenderCallback,
                kAudioUnitScope_Input,
                OUTPUT_BUS,
                &callback,
            )
        }
    }

    fn start(&self) -> Result<(), IoError> {
        // SAFETY: the unit is configured; callbacks reference state that outlives the unit.
        unsafe {
            check("AudioUnitInitialize", AudioUnitInitialize(self.0))?;
            check("AudioOutputUnitStart", AudioOutputUnitStart(self.0))
        }
    }
}

impl Drop for Unit {
    fn drop(&mut self) {
        // SAFETY: stopping before disposal guarantees no callback runs afterwards.
        unsafe {
            AudioOutputUnitStop(self.0);
            AudioUnitUninitialize(self.0);
            AudioComponentInstanceDispose(self.0);
        }
    }
}

type Callback = unsafe extern "C-unwind" fn(
    NonNull<c_void>,
    NonNull<AudioUnitRenderActionFlags>,
    NonNull<AudioTimeStamp>,
    u32,
    u32,
    *mut AudioBufferList,
) -> i32;

/// Preallocated AudioBufferList pointing at planar channel buffers, for AudioUnitRender.
struct InputList {
    storage: Vec<u64>,
    channels: Vec<*mut f32>,
}

impl InputList {
    fn new(channels: Vec<*mut f32>) -> Self {
        let bytes = size_of::<AudioBufferList>()
            + channels.len().saturating_sub(1) * size_of::<AudioBuffer>();
        Self {
            storage: vec![0; bytes.div_ceil(8)],
            channels,
        }
    }

    /// Pulls `frames` input frames from the unit into the channel buffers.
    ///
    /// # Safety
    /// Must be called from a render or input callback of `unit`, with its arguments.
    unsafe fn render(
        &mut self,
        unit: AudioUnit,
        flags: NonNull<AudioUnitRenderActionFlags>,
        time: NonNull<AudioTimeStamp>,
        frames: u32,
    ) -> i32 {
        let list = self.storage.as_mut_ptr().cast::<AudioBufferList>();
        // SAFETY: storage is sized for `channels.len()` buffers.
        unsafe {
            (*list).mNumberBuffers = self.channels.len() as u32;
            let buffers =
                std::slice::from_raw_parts_mut((*list).mBuffers.as_mut_ptr(), self.channels.len());
            for (buffer, &data) in buffers.iter_mut().zip(&self.channels) {
                *buffer = AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: frames * 4,
                    mData: data.cast(),
                };
            }
            AudioUnitRender(
                unit,
                flags.as_ptr(),
                time,
                INPUT_BUS,
                frames,
                NonNull::new_unchecked(list),
            )
        }
    }
}

/// Collects the output channel pointers of a render callback, using scratch for missing ones.
///
/// # Safety
/// `data` must be the AudioBufferList passed to a render callback.
unsafe fn output_pointers(
    data: *mut AudioBufferList,
    scratch: &mut ChannelBuffers,
    out: &mut [*mut f32],
) {
    let provided = if data.is_null() {
        &mut [][..]
    } else {
        // SAFETY: CoreAudio passes a list with `mNumberBuffers` planar buffers.
        unsafe {
            std::slice::from_raw_parts_mut(
                (*data).mBuffers.as_mut_ptr(),
                (*data).mNumberBuffers as usize,
            )
        }
    };
    for (ch, ptr) in out.iter_mut().enumerate() {
        *ptr = match provided.get(ch) {
            Some(buffer) if !buffer.mData.is_null() => buffer.mData.cast(),
            _ => scratch.output_ptr(ch),
        };
    }
}

/// Callback state when one unit does both input and output.
struct Duplex {
    unit: AudioUnit,
    input: Option<InputList>,
    storage: ChannelBuffers,
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
    runner: CallbackRunner,
}

impl Duplex {
    fn process(
        &mut self,
        flags: NonNull<AudioUnitRenderActionFlags>,
        time: NonNull<AudioTimeStamp>,
        frames: u32,
        data: *mut AudioBufferList,
    ) -> i32 {
        let n = frames as usize;
        if n > MAX_FRAMES {
            return -1;
        }
        if let Some(input) = &mut self.input {
            // SAFETY: called from this unit's callback with its own arguments.
            if unsafe { input.render(self.unit, flags, time, frames) } != 0 {
                for ch in 0..self.in_ptrs.len() {
                    self.storage.input_mut(ch)[..n].fill(0.0);
                }
            }
        }
        // SAFETY: `data` comes straight from CoreAudio (null for input-only units).
        unsafe { output_pointers(data, &mut self.storage, &mut self.out_ptrs) };
        // SAFETY: input pointers target storage slices, outputs are distinct device or scratch buffers.
        let mut buffers = unsafe { AudioBuffers::from_raw(&self.in_ptrs, &self.out_ptrs, n) };
        self.runner.run(&mut buffers);
        0
    }
}

unsafe extern "C-unwind" fn duplex_proc(
    refcon: NonNull<c_void>,
    flags: NonNull<AudioUnitRenderActionFlags>,
    time: NonNull<AudioTimeStamp>,
    _bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> i32 {
    // SAFETY: refcon is the Duplex registered with this unit, alive until the unit is disposed.
    unsafe { refcon.cast::<Duplex>().as_mut() }.process(flags, time, frames, data)
}

/// Capture side of a two-device stream: renders input and queues it interleaved.
struct CaptureSide {
    unit: AudioUnit,
    input: InputList,
    storage: ChannelBuffers,
    channels: usize,
    fifo: Sender<f32>,
    status: Arc<StreamStatus>,
}

unsafe extern "C-unwind" fn capture_proc(
    refcon: NonNull<c_void>,
    flags: NonNull<AudioUnitRenderActionFlags>,
    time: NonNull<AudioTimeStamp>,
    _bus: u32,
    frames: u32,
    _data: *mut AudioBufferList,
) -> i32 {
    // SAFETY: refcon is the CaptureSide registered with this unit.
    let side = unsafe { refcon.cast::<CaptureSide>().as_mut() };
    let n = frames as usize;
    if n > MAX_FRAMES {
        return -1;
    }
    // SAFETY: called from this unit's input callback with its arguments.
    let status = unsafe { side.input.render(side.unit, flags, time, frames) };
    if status != 0 {
        return status;
    }
    let mut dropped = false;
    for f in 0..n {
        for ch in 0..side.channels {
            dropped |= side.fifo.try_send(side.storage.input_mut(ch)[f]).is_err();
        }
    }
    if dropped {
        side.status.add_xruns(1);
    }
    0
}

/// Playback side of a two-device stream: consumes queued input and runs the callback.
struct PlaybackSide {
    fifo: Receiver<f32>,
    capture_channels: usize,
    storage: ChannelBuffers,
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
    runner: CallbackRunner,
}

unsafe extern "C-unwind" fn playback_proc(
    refcon: NonNull<c_void>,
    _flags: NonNull<AudioUnitRenderActionFlags>,
    _time: NonNull<AudioTimeStamp>,
    _bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> i32 {
    // SAFETY: refcon is the PlaybackSide registered with this unit.
    let side = unsafe { refcon.cast::<PlaybackSide>().as_mut() };
    let n = frames as usize;
    if n > MAX_FRAMES {
        return -1;
    }
    let mut underflow = false;
    for f in 0..n {
        for ch in 0..side.capture_channels {
            let sample = side.fifo.try_recv().unwrap_or_else(|| {
                underflow = true;
                0.0
            });
            if ch < side.in_ptrs.len() {
                side.storage.input_mut(ch)[f] = sample;
            }
        }
    }
    if underflow {
        side.runner.status().add_xruns(1);
    }
    // SAFETY: `data` comes straight from CoreAudio.
    unsafe { output_pointers(data, &mut side.storage, &mut side.out_ptrs) };
    // SAFETY: as in Duplex::process.
    let mut buffers = unsafe { AudioBuffers::from_raw(&side.in_ptrs, &side.out_ptrs, n) };
    side.runner.run(&mut buffers);
    0
}

#[derive(Debug)]
pub struct CoreAudioBackend {
    watcher: crate::watch::DeviceWatcher,
}

impl CoreAudioBackend {
    pub fn connect() -> Result<Self, IoError> {
        get_audio_device_ids().map_err(|_| IoError::BackendUnavailable(BackendKind::CoreAudio))?;
        Ok(Self {
            watcher: crate::watch::DeviceWatcher::start(|| {
                let mut ids: Vec<String> = get_audio_device_ids()
                    .unwrap_or_default()
                    .iter()
                    .map(u32::to_string)
                    .collect();
                ids.extend(
                    [true, false]
                        .map(|input| format!("default:{:?}", get_default_device_id(input))),
                );
                ids
            }),
        })
    }
}

impl AudioBackend for CoreAudioBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::CoreAudio
    }

    fn device_generation(&self) -> u64 {
        self.watcher.generation()
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        let default_in = get_default_device_id(true);
        let default_out = get_default_device_id(false);
        Ok(get_audio_device_ids()
            .map_err(backend_error)?
            .into_iter()
            .filter_map(|id| {
                let name = get_device_name(id).ok()?;
                Some(DeviceInfo {
                    id: name.clone(),
                    name,
                    inputs: device_channels(id, true),
                    outputs: device_channels(id, false),
                    is_default_input: default_in == Some(id),
                    is_default_output: default_out == Some(id),
                })
            })
            .collect())
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        if config.inputs == 0 && config.outputs == 0 || config.buffer_size == 0 {
            return Err(IoError::UnsupportedConfig(format!("{config:?}")));
        }
        // Without a default input (no microphone) the stream runs output only, inputs silent.
        let input = match config.input_device.as_deref() {
            _ if config.inputs == 0 => None,
            None if config.outputs > 0 => default_device(true),
            name => Some(resolve_device(name, true)?),
        };
        let output = (config.outputs > 0)
            .then(|| resolve_device(config.output_device.as_deref(), false))
            .transpose()?;
        let main = output.or(input).ok_or_else(|| backend_error("no device"))?;
        for device in [input, output].into_iter().flatten() {
            if let Some(rate) = config.sample_rate {
                set_device_sample_rate(device, f64::from(rate)).map_err(backend_error)?;
            }
            set_device_property(
                device,
                kAudioDevicePropertyBufferFrameSize,
                &config.buffer_size.min(MAX_FRAMES as u32),
            )?;
        }
        let rate: f64 = get_device_property(main, kAudioDevicePropertyNominalSampleRate)?;
        let frames: u32 = get_device_property(main, kAudioDevicePropertyBufferFrameSize)?;
        let status = Arc::new(StreamStatus::default());
        let name = |d: Option<u32>| {
            d.and_then(|d| get_device_name(d).ok())
                .unwrap_or_else(|| "none".into())
        };
        let info = StreamInfo {
            backend: BackendKind::CoreAudio,
            device: format!("in: {}, out: {}", name(input), name(output)),
            sample_rate: rate as u32,
            buffer_size: frames,
            inputs: config.inputs,
            outputs: config.outputs,
        };
        let runner = CallbackRunner::new(factory(&info), Arc::clone(&status), rate as u32);
        let inputs = config.inputs as usize;
        let outputs = config.outputs as usize;
        let state = match (input, output) {
            (Some(i), Some(o)) if i != o => Self::open_split(
                i,
                o,
                inputs,
                outputs,
                rate,
                frames as usize,
                runner,
                Arc::clone(&status),
            )?,
            _ => Self::open_duplex(main, inputs, outputs, rate, runner)?,
        };
        Ok(Box::new(CoreAudioStream {
            info,
            status,
            _state: state,
        }))
    }
}

/// Units and the callback state they point to; units are declared first so they drop first.
#[allow(dead_code, reason = "held only for their Drop")]
enum Running {
    Duplex(Unit, Box<Duplex>),
    Split(Unit, Unit, Box<CaptureSide>, Box<PlaybackSide>),
}

// SAFETY: the raw pointers inside are only used by CoreAudio callbacks or during setup.
unsafe impl Send for Running {}

fn planar_inputs(storage: &mut ChannelBuffers, channels: usize) -> Vec<*mut f32> {
    (0..channels)
        .map(|ch| storage.input_mut(ch).as_mut_ptr())
        .collect()
}

impl CoreAudioBackend {
    fn open_duplex(
        device: u32,
        inputs: usize,
        outputs: usize,
        rate: f64,
        runner: CallbackRunner,
    ) -> Result<Running, IoError> {
        let hw_in = (device_channels(device, true) as usize).min(inputs);
        let hw_out = (device_channels(device, false) as usize).min(outputs);
        let unit = Unit::new_hal(device, hw_in > 0, hw_out > 0)?;
        if hw_in > 0 {
            unit.set_client_format(true, hw_in as u32, rate)?;
        }
        if hw_out > 0 {
            unit.set_client_format(false, hw_out as u32, rate)?;
        }
        let mut storage = ChannelBuffers::new(inputs, outputs, MAX_FRAMES);
        let input = (hw_in > 0).then(|| InputList::new(planar_inputs(&mut storage, hw_in)));
        let in_ptrs = planar_inputs(&mut storage, inputs)
            .into_iter()
            .map(|p| p.cast_const())
            .collect();
        let mut duplex = Box::new(Duplex {
            unit: unit.0,
            input,
            storage,
            in_ptrs,
            out_ptrs: vec![ptr::null_mut(); outputs],
            runner,
        });
        let refcon = ptr::from_mut(duplex.as_mut()).cast();
        unit.set_callback(hw_out == 0, duplex_proc, refcon)?;
        unit.start()?;
        Ok(Running::Duplex(unit, duplex))
    }

    #[allow(clippy::too_many_arguments)]
    fn open_split(
        input: u32,
        output: u32,
        inputs: usize,
        outputs: usize,
        rate: f64,
        frames: usize,
        runner: CallbackRunner,
        status: Arc<StreamStatus>,
    ) -> Result<Running, IoError> {
        let hw_in = (device_channels(input, true) as usize).min(inputs);
        let hw_out = (device_channels(output, false) as usize).min(outputs);
        let (mut tx, rx) = channel::<f32>(hw_in.max(1) * frames.max(1) * FIFO_BUFFERS);
        for _ in 0..hw_in * frames {
            let _ = tx.try_send(0.0);
        }

        let capture_unit = Unit::new_hal(input, true, false)?;
        capture_unit.set_client_format(true, hw_in as u32, rate)?;
        let mut capture_storage = ChannelBuffers::new(hw_in, 0, MAX_FRAMES);
        let mut capture = Box::new(CaptureSide {
            unit: capture_unit.0,
            input: InputList::new(planar_inputs(&mut capture_storage, hw_in)),
            storage: capture_storage,
            channels: hw_in,
            fifo: tx,
            status,
        });
        capture_unit.set_callback(true, capture_proc, ptr::from_mut(capture.as_mut()).cast())?;

        let playback_unit = Unit::new_hal(output, false, true)?;
        playback_unit.set_client_format(false, hw_out as u32, rate)?;
        let mut storage = ChannelBuffers::new(inputs, outputs, MAX_FRAMES);
        let in_ptrs = planar_inputs(&mut storage, inputs)
            .into_iter()
            .map(|p| p.cast_const())
            .collect();
        let mut playback = Box::new(PlaybackSide {
            fifo: rx,
            capture_channels: hw_in,
            storage,
            in_ptrs,
            out_ptrs: vec![ptr::null_mut(); outputs],
            runner,
        });
        playback_unit.set_callback(
            false,
            playback_proc,
            ptr::from_mut(playback.as_mut()).cast(),
        )?;
        playback_unit.start()?;
        capture_unit.start()?;
        Ok(Running::Split(
            capture_unit,
            playback_unit,
            capture,
            playback,
        ))
    }
}

struct CoreAudioStream {
    info: StreamInfo,
    status: Arc<StreamStatus>,
    _state: Running,
}

impl fmt::Debug for CoreAudioStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreAudioStream")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl AudioStream for CoreAudioStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn status(&self) -> &StreamStatus {
        &self.status
    }
}
