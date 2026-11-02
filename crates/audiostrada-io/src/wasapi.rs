//! WASAPI shared-mode backend (Windows), event driven, with the render endpoint as clock master.
//!
//! When the engine runs at the requested rate, IAudioClient3 low-latency periods are used;
//! otherwise the stream falls back to the standard 10 ms engine period with format conversion.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use wasapi::{
    AudioCaptureClient, AudioClient, AudioRenderClient, DeviceEnumerator, Direction, Handle,
    SampleType, StreamMode, TimingMode, WaveFormat,
};

use crate::convert::{Fifo, SampleFormat, deinterleave, interleave};
use crate::stream::{CallbackRunner, ThreadStream};
use crate::timing::engine_period;
use crate::{
    AudioBackend, AudioBuffers, AudioCallback, AudioStream, BackendKind, CallbackFactory,
    CallbackTime, ChannelBuffers, DeviceInfo, IoError, StreamConfig, StreamInfo, StreamStatus,
};

/// Placeholder until the real callback arrives from the opening thread.
fn silence(buffers: &mut AudioBuffers<'_>, _: &CallbackTime) {
    buffers.clear_outputs();
}

const EVENT_TIMEOUT_MS: u32 = 1000;
const FIFO_PERIODS: usize = 8;

fn backend_error(message: impl ToString) -> IoError {
    IoError::backend(BackendKind::Wasapi, message)
}

#[derive(Debug)]
pub struct WasapiBackend {
    watcher: crate::watch::DeviceWatcher,
}

/// Ids of the present endpoints and of the defaults, to notice changes.
fn endpoint_ids() -> Vec<String> {
    let _ = wasapi::initialize_mta();
    let Ok(enumerator) = DeviceEnumerator::new() else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for direction in [Direction::Capture, Direction::Render] {
        if let Ok(id) = enumerator
            .get_default_device(&direction)
            .and_then(|d| d.get_id())
        {
            ids.push(format!("default:{id}"));
        }
        if let Ok(collection) = enumerator.get_device_collection(&direction) {
            ids.extend(
                (&collection)
                    .into_iter()
                    .flatten()
                    .filter_map(|d| d.get_id().ok()),
            );
        }
    }
    ids
}

/// COM for enumeration on the caller's thread, often the UI thread: an STA there, as an MTA
/// would make the window's `OleInitialize` (drag and drop) fail. Already initialized either
/// way is fine, the enumerator works in both apartments.
fn initialize_caller_com() {
    let _ = wasapi::initialize_sta();
}

impl WasapiBackend {
    pub fn connect() -> Result<Self, IoError> {
        initialize_caller_com();
        DeviceEnumerator::new().map_err(|_| IoError::BackendUnavailable(BackendKind::Wasapi))?;
        Ok(Self {
            watcher: crate::watch::DeviceWatcher::start(endpoint_ids),
        })
    }
}

impl AudioBackend for WasapiBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Wasapi
    }

    fn device_generation(&self) -> u64 {
        self.watcher.generation()
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        initialize_caller_com();
        let enumerator = DeviceEnumerator::new().map_err(backend_error)?;
        let mut devices = Vec::new();
        for direction in [Direction::Capture, Direction::Render] {
            let default_id = enumerator
                .get_default_device(&direction)
                .and_then(|d| d.get_id())
                .ok();
            let collection = enumerator
                .get_device_collection(&direction)
                .map_err(backend_error)?;
            for device in (&collection).into_iter().flatten() {
                let Ok(id) = device.get_id() else { continue };
                let channels = device
                    .get_iaudioclient()
                    .and_then(|c| c.get_mixformat())
                    .map(|f| u32::from(f.get_nchannels()))
                    .unwrap_or(0);
                let capture = direction == Direction::Capture;
                devices.push(DeviceInfo {
                    is_default_input: capture && default_id.as_deref() == Some(id.as_str()),
                    is_default_output: !capture && default_id.as_deref() == Some(id.as_str()),
                    name: device.get_friendlyname().unwrap_or_else(|_| id.clone()),
                    id,
                    inputs: if capture { channels } else { 0 },
                    outputs: if capture { 0 } else { channels },
                });
            }
        }
        Ok(devices)
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        if config.inputs == 0 && config.outputs == 0 || config.buffer_size == 0 {
            return Err(IoError::UnsupportedConfig(format!("{config:?}")));
        }
        let status = Arc::new(StreamStatus::default());
        let running = Arc::new(AtomicBool::new(true));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (callback_tx, callback_rx) = mpsc::channel();
        let thread = {
            let config = config.clone();
            let status = Arc::clone(&status);
            let running = Arc::clone(&running);
            thread::Builder::new()
                .name("audiostrada-wasapi-audio".into())
                .spawn(move || audio_thread(&config, status, &running, &ready_tx, &callback_rx))
                .map_err(backend_error)?
        };
        let info = match ready_rx.recv() {
            Ok(Ok(info)) => {
                let _ = callback_tx.send(factory(&info));
                info
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(_) => {
                let _ = thread.join();
                return Err(backend_error("audio thread exited during setup"));
            }
        };
        Ok(Box::new(ThreadStream {
            info,
            status,
            running,
            thread: Some(thread),
        }))
    }
}

/// One initialized endpoint (render or capture).
struct Endpoint {
    client: AudioClient,
    event: Handle,
    channels: usize,
    block_align: usize,
    buffer_frames: usize,
    name: String,
}

impl Endpoint {
    fn open(
        enumerator: &DeviceEnumerator,
        id: Option<&str>,
        direction: Direction,
        rate: Option<u32>,
        buffer: u32,
    ) -> Result<(Self, u32), IoError> {
        let device = match id {
            Some(id) => enumerator
                .get_device(id)
                .map_err(|_| IoError::DeviceNotFound(id.into()))?,
            None => enumerator
                .get_default_device(&direction)
                .map_err(backend_error)?,
        };
        let name = device.get_friendlyname().unwrap_or_default();
        let mut client = device.get_iaudioclient().map_err(backend_error)?;
        let mix = client.get_mixformat().map_err(backend_error)?;
        let channels = usize::from(mix.get_nchannels());
        let mix_rate = mix.get_samplespersec();
        let rate = rate.unwrap_or(mix_rate);
        let format = WaveFormat::new(32, 32, &SampleType::Float, rate as usize, channels, None);

        let low_latency = (rate == mix_rate)
            .then(|| client.get_shared_mode_engine_period(&format).ok())
            .flatten()
            .map(|p| engine_period(p.fundamental, p.min, p.max, buffer));
        let initialized = match low_latency {
            Some(period) => client
                .initialize_shared_audio_stream(&format, TimingMode::Events, period)
                .is_ok(),
            None => false,
        };
        if !initialized {
            let mode = StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: wasapi::calculate_period_100ns(
                    i64::from(buffer),
                    i64::from(rate),
                ),
            };
            client
                .initialize_client(&format, &direction, &mode)
                .map_err(backend_error)?;
        }
        let event = client.set_get_eventhandle().map_err(backend_error)?;
        let buffer_frames = client.get_buffer_size().map_err(backend_error)? as usize;
        Ok((
            Self {
                client,
                event,
                channels,
                block_align: format.get_blockalign() as usize,
                buffer_frames,
                name,
            },
            rate,
        ))
    }
}

fn audio_thread(
    config: &StreamConfig,
    status: Arc<StreamStatus>,
    running: &AtomicBool,
    ready: &mpsc::Sender<Result<StreamInfo, IoError>>,
    callback: &mpsc::Receiver<Box<dyn AudioCallback>>,
) {
    let _ = wasapi::initialize_mta();
    if let Err(e) =
        audiostrada_rt::thread::promote_current_thread(audiostrada_rt::thread::AUDIO_PRIORITY)
    {
        tracing::warn!("WASAPI audio thread runs without MMCSS: {e}");
    }
    match Session::open(config, Arc::clone(&status)) {
        Ok(mut session) => {
            let _ = ready.send(Ok(session.info.clone()));
            let Ok(callback) = callback.recv() else {
                wasapi::deinitialize();
                return;
            };
            session.runner.set_callback(callback);
            if let Err(e) = session.run(running) {
                tracing::error!("WASAPI stream stopped: {e}");
                status.set_failed();
            }
            session.stop();
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
    wasapi::deinitialize();
}

struct Session {
    info: StreamInfo,
    runner: CallbackRunner,
    storage: ChannelBuffers,
    inputs: usize,
    outputs: usize,
    render: Option<(Endpoint, AudioRenderClient, Vec<u8>)>,
    capture: Option<(Endpoint, AudioCaptureClient, Vec<u8>, Fifo)>,
}

impl Session {
    fn open(config: &StreamConfig, status: Arc<StreamStatus>) -> Result<Self, IoError> {
        let enumerator = DeviceEnumerator::new().map_err(backend_error)?;
        let mut rate = config.sample_rate;
        let render = match config.outputs {
            0 => None,
            _ => {
                let (endpoint, r) = Endpoint::open(
                    &enumerator,
                    config.output_device.as_deref(),
                    Direction::Render,
                    rate,
                    config.buffer_size,
                )?;
                rate = Some(r);
                let client = endpoint
                    .client
                    .get_audiorenderclient()
                    .map_err(backend_error)?;
                let bytes = vec![0; endpoint.buffer_frames * endpoint.block_align];
                Some((endpoint, client, bytes))
            }
        };
        let capture = match config.inputs {
            0 => None,
            _ => {
                let (endpoint, r) = Endpoint::open(
                    &enumerator,
                    config.input_device.as_deref(),
                    Direction::Capture,
                    rate,
                    config.buffer_size,
                )?;
                rate = Some(r);
                let client = endpoint
                    .client
                    .get_audiocaptureclient()
                    .map_err(backend_error)?;
                let bytes = vec![0; endpoint.buffer_frames * endpoint.block_align];
                let fifo = Fifo::new(endpoint.channels, endpoint.buffer_frames * FIFO_PERIODS);
                Some((endpoint, client, bytes, fifo))
            }
        };
        let rate = rate.unwrap_or(48_000);
        let max_frames = render
            .as_ref()
            .map(|r| r.0.buffer_frames)
            .or(capture.as_ref().map(|c| c.0.buffer_frames))
            .unwrap_or(0);
        let info = StreamInfo {
            backend: BackendKind::Wasapi,
            device: format!(
                "in: {}, out: {}",
                capture.as_ref().map_or("none", |c| c.0.name.as_str()),
                render.as_ref().map_or("none", |r| r.0.name.as_str())
            ),
            sample_rate: rate,
            buffer_size: max_frames as u32,
            inputs: config.inputs,
            outputs: config.outputs,
        };
        Ok(Self {
            info,
            runner: CallbackRunner::new(Box::new(silence), status, rate),
            storage: ChannelBuffers::new(
                config.inputs as usize,
                config.outputs as usize,
                max_frames,
            ),
            inputs: config.inputs as usize,
            outputs: config.outputs as usize,
            render,
            capture,
        })
    }

    fn run(&mut self, running: &AtomicBool) -> Result<(), IoError> {
        if let Some((endpoint, client, bytes)) = &mut self.render {
            let frames = endpoint
                .client
                .get_available_space_in_frames()
                .map_err(backend_error)? as usize;
            bytes.fill(0);
            client
                .write_to_device(frames, &bytes[..frames * endpoint.block_align], None)
                .map_err(backend_error)?;
        }
        if let Some((endpoint, ..)) = &self.capture {
            endpoint.client.start_stream().map_err(backend_error)?;
        }
        if let Some((endpoint, ..)) = &self.render {
            endpoint.client.start_stream().map_err(backend_error)?;
        }
        while running.load(Ordering::Acquire) {
            if self.render.is_some() {
                self.render_cycle()?;
            } else {
                self.capture_cycle()?;
            }
        }
        Ok(())
    }

    /// Reads every pending capture packet into the FIFO.
    fn drain_capture(&mut self) -> Result<(), IoError> {
        let Some((endpoint, client, bytes, fifo)) = &mut self.capture else {
            return Ok(());
        };
        while client
            .get_next_packet_size()
            .map_err(backend_error)?
            .unwrap_or(0)
            > 0
        {
            let (frames, info) = client.read_from_device(bytes).map_err(backend_error)?;
            let overflow = fifo.push(&bytes[..frames as usize * endpoint.block_align]);
            if info.flags.data_discontinuity || overflow {
                self.runner.status().add_xruns(1);
            }
        }
        Ok(())
    }

    fn render_cycle(&mut self) -> Result<(), IoError> {
        if let Some((endpoint, ..)) = &self.render {
            endpoint
                .event
                .wait_for_event(EVENT_TIMEOUT_MS)
                .map_err(backend_error)?;
        }
        self.drain_capture()?;
        let Some((endpoint, client, bytes)) = &mut self.render else {
            return Ok(());
        };
        let available = endpoint
            .client
            .get_available_space_in_frames()
            .map_err(backend_error)? as usize;
        let mut done = 0;
        while done < available {
            let frames = (available - done).min(self.storage.max_frames());
            if let Some((.., fifo)) = &mut self.capture
                && !fifo.pop(&mut self.storage, self.inputs, frames)
            {
                self.runner.status().add_xruns(1);
            }
            self.runner.run(&mut self.storage.buffers(frames));
            let len = frames * endpoint.block_align;
            interleave(
                SampleFormat::F32,
                &self.storage,
                self.outputs,
                &mut bytes[..len],
                endpoint.channels,
                frames,
            );
            client
                .write_to_device(frames, &bytes[..len], None)
                .map_err(backend_error)?;
            done += frames;
        }
        Ok(())
    }

    fn capture_cycle(&mut self) -> Result<(), IoError> {
        let Some((endpoint, client, bytes, _)) = &mut self.capture else {
            return Ok(());
        };
        endpoint
            .event
            .wait_for_event(EVENT_TIMEOUT_MS)
            .map_err(backend_error)?;
        while client
            .get_next_packet_size()
            .map_err(backend_error)?
            .unwrap_or(0)
            > 0
        {
            let (frames, info) = client.read_from_device(bytes).map_err(backend_error)?;
            if info.flags.data_discontinuity {
                self.runner.status().add_xruns(1);
            }
            let mut done = 0;
            let frames = frames as usize;
            while done < frames {
                let n = (frames - done).min(self.storage.max_frames());
                let start = done * endpoint.block_align;
                deinterleave(
                    SampleFormat::F32,
                    &bytes[start..start + n * endpoint.block_align],
                    endpoint.channels,
                    n,
                    &mut self.storage,
                    self.inputs,
                );
                self.runner.run(&mut self.storage.buffers(n));
                done += n;
            }
        }
        Ok(())
    }

    fn stop(&self) {
        if let Some((endpoint, ..)) = &self.render {
            let _ = endpoint.client.stop_stream();
        }
        if let Some((endpoint, ..)) = &self.capture {
            let _ = endpoint.client.stop_stream();
        }
    }
}
