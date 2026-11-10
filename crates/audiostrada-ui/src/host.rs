//! Owns the audio backend and stream, and adds the configuration panel tools (test tone,
//! meters) around the processing callback supplied by the application.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use audiostrada_rt::channel::{Receiver, Sender, channel};

use audiostrada_io::tools::{Meters, TestTone};
use audiostrada_io::{
    AudioBackend, AudioBuffers, AudioCallback, AudioStream, BackendKind, CallbackTime, DeviceInfo,
    StreamConfig, StreamInfo, StreamStatus, auto_backend, backend,
};

const TONE_FREQUENCY: f32 = 440.0;
const TONE_GAIN_DB: f32 = -18.0;
/// Longest wait for the audio thread to take a new processor before the stream is reopened.
const HANDOVER: Duration = Duration::from_millis(250);

type Callback = Box<dyn AudioCallback>;

/// Builds the processing callback once the format of a new stream is known.
pub type ProcessorFactory = Box<dyn FnMut(&StreamInfo) -> Box<dyn AudioCallback>>;

/// Wraps the application callback with the test tone and level meters. A new callback can
/// replace it while the stream runs, the old one going back to be dropped off the audio thread.
struct Monitor {
    inner: Box<dyn AudioCallback>,
    incoming: Receiver<Callback>,
    retired: Sender<Callback>,
    tone: TestTone,
    tone_rate: u32,
    tone_on: Arc<AtomicBool>,
    inputs: Meters,
    outputs: Meters,
}

impl AudioCallback for Monitor {
    fn process(&mut self, buffers: &mut AudioBuffers<'_>, time: &CallbackTime) {
        if let Some(next) = self.incoming.try_recv() {
            let old = std::mem::replace(&mut self.inner, next);
            if let Err(old) = self.retired.try_send(old) {
                // The host always drains it first: never freed here, even if full.
                std::mem::forget(old);
            }
        }
        self.inner.process(buffers, time);
        if self.tone_on.load(Ordering::Relaxed) && buffers.output_channels() > 0 {
            if time.sample_rate != self.tone_rate {
                self.tone_rate = time.sample_rate;
                self.tone.set_sample_rate(TONE_FREQUENCY, time.sample_rate);
            }
            self.tone.fill(buffers.output(0));
            for ch in 1..buffers.output_channels() {
                buffers.copy_output(0, ch);
            }
        }
        for ch in 0..buffers.input_channels().min(self.inputs.channels()) {
            self.inputs.update(ch, buffers.input(ch));
        }
        for ch in 0..buffers.output_channels().min(self.outputs.channels()) {
            self.outputs.update(ch, buffers.output(ch));
        }
    }
}

/// Devices that appeared and disappeared since the last poll.
#[derive(Debug, Default)]
pub struct DeviceChanges {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// The stream was restarted on devices that came back.
    pub restarted: bool,
}

pub struct AudioHost {
    backend: Option<Box<dyn AudioBackend>>,
    stream: Option<Box<dyn AudioStream>>,
    config: StreamConfig,
    factory: ProcessorFactory,
    /// Hands the running stream a new processor and takes the old one back.
    handover: Option<(Sender<Callback>, Receiver<Callback>)>,
    tone_on: Arc<AtomicBool>,
    input_meters: Meters,
    output_meters: Meters,
    devices: Vec<DeviceInfo>,
    device_generation: u64,
    error: Option<String>,
}

impl std::fmt::Debug for AudioHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioHost")
            .field("backend", &self.kind())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl AudioHost {
    pub fn new(factory: ProcessorFactory) -> Self {
        Self {
            backend: None,
            stream: None,
            config: StreamConfig::default(),
            factory,
            handover: None,
            tone_on: Arc::new(AtomicBool::new(false)),
            input_meters: Meters::new(0),
            output_meters: Meters::new(0),
            devices: Vec::new(),
            device_generation: 0,
            error: None,
        }
    }

    /// Connects to `kind` (or the best available backend) and starts a stream with `config`.
    pub fn start(&mut self, kind: Option<BackendKind>, config: StreamConfig) {
        self.stop();
        let connected = match kind {
            Some(kind) => backend(kind).or_else(|e| {
                tracing::warn!("{kind} unavailable ({e}), falling back to auto-detection");
                auto_backend()
            }),
            None => auto_backend(),
        };
        match connected {
            Ok(backend) => {
                self.backend = Some(backend);
                self.refresh_devices();
                self.apply(config);
            }
            Err(e) => {
                self.backend = None;
                self.devices.clear();
                self.error = Some(e.to_string());
            }
        }
    }

    /// Switches backend, keeping channel counts and buffer size but resetting devices.
    pub fn select_backend(&mut self, kind: BackendKind) {
        let config = StreamConfig {
            input_device: None,
            output_device: None,
            ..self.config.clone()
        };
        self.start(Some(kind), config);
    }

    /// Reopens the stream with a new configuration.
    pub fn apply(&mut self, config: StreamConfig) {
        self.stop();
        tracing::info!(?config, "opening the audio stream");
        self.config = config;
        let Some(backend) = &self.backend else {
            return;
        };
        let factory = &mut self.factory;
        let tone_on = &self.tone_on;
        let mut meters = None;
        let (to_stream, incoming) = channel(2);
        let (retired, from_stream) = channel(2);
        let mut ends = Some((incoming, retired));
        let opened = backend.open(&self.config, &mut |info| {
            let (incoming, retired) = ends.take().unwrap_or_else(|| (channel(1).1, channel(1).0));
            let monitor = Monitor {
                inner: factory(info),
                incoming,
                retired,
                tone: TestTone::new(TONE_FREQUENCY, TONE_GAIN_DB, info.sample_rate),
                tone_rate: info.sample_rate,
                tone_on: Arc::clone(tone_on),
                inputs: Meters::new(info.inputs as usize),
                outputs: Meters::new(info.outputs as usize),
            };
            meters = Some((monitor.inputs.clone(), monitor.outputs.clone()));
            Box::new(monitor)
        });
        if let Some((inputs, outputs)) = meters {
            self.input_meters = inputs;
            self.output_meters = outputs;
        }
        match opened {
            Ok(stream) => {
                tracing::info!(info = ?stream.info(), "audio stream started");
                self.stream = Some(stream);
                self.handover = Some((to_stream, from_stream));
                self.error = None;
            }
            Err(e) => {
                tracing::warn!("cannot open audio stream: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    /// Re-reads the device list when the backend reports a change and says what changed. A
    /// stream that stopped (its device unplugged) restarts once its devices are back.
    pub fn poll_devices(&mut self) -> Option<DeviceChanges> {
        let generation = self.backend.as_ref().map_or(0, |b| b.device_generation());
        if generation == self.device_generation {
            return None;
        }
        let before = self.devices.clone();
        self.refresh_devices();
        let missing = |list: &[DeviceInfo], other: &[DeviceInfo]| -> Vec<String> {
            list.iter()
                .filter(|d| !other.iter().any(|o| o.id == d.id))
                .map(|d| d.name.clone())
                .collect()
        };
        let mut changes = DeviceChanges {
            added: missing(&self.devices, &before),
            removed: missing(&before, &self.devices),
            restarted: false,
        };
        let down = self.stream.as_ref().is_none_or(|s| s.status().failed());
        let present = [&self.config.input_device, &self.config.output_device]
            .into_iter()
            .flatten()
            .all(|id| self.devices.iter().any(|d| &d.id == id));
        if down && present && !changes.added.is_empty() {
            self.apply(self.config.clone());
            changes.restarted = self.stream.is_some();
        }
        Some(changes)
    }

    fn refresh_devices(&mut self) {
        if let Some(backend) = &self.backend {
            self.device_generation = backend.device_generation();
            match backend.devices() {
                Ok(devices) => self.devices = devices,
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }

    pub fn stop(&mut self) {
        self.handover = None;
        if self.stream.take().is_some() {
            tracing::info!("audio stream stopped");
        }
    }

    /// Gives the running stream a new processor from the factory (another project shown),
    /// keeping the device open: some interfaces misbehave when restarted at once. Reopens the
    /// stream when none runs or the audio thread does not take it.
    pub fn reattach(&mut self) {
        let info = self
            .stream
            .as_ref()
            .filter(|s| !s.status().failed())
            .map(|s| s.info().clone());
        let (Some(info), Some((to_stream, from_stream))) = (info, self.handover.as_mut()) else {
            self.apply(self.config.clone());
            return;
        };
        let processor = (self.factory)(&info);
        if to_stream.try_send(processor).is_ok() {
            let start = Instant::now();
            while start.elapsed() < HANDOVER {
                if let Some(old) = from_stream.try_recv() {
                    drop(old);
                    tracing::info!("audio processor replaced, the stream kept open");
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        tracing::warn!("the audio thread did not take the new processor: reopening the stream");
        self.apply(self.config.clone());
    }

    pub fn kind(&self) -> Option<BackendKind> {
        self.backend.as_ref().map(|b| b.kind())
    }

    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    pub fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    pub fn stream(&self) -> Option<&dyn AudioStream> {
        self.stream.as_deref()
    }

    pub fn status(&self) -> Option<&StreamStatus> {
        self.stream.as_ref().map(|s| s.status())
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn tone(&self) -> bool {
        self.tone_on.load(Ordering::Relaxed)
    }

    pub fn set_tone(&self, on: bool) {
        self.tone_on.store(on, Ordering::Relaxed);
    }

    pub fn input_meters(&self) -> &Meters {
        &self.input_meters
    }

    pub fn output_meters(&self) -> &Meters {
        &self.output_meters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn another_processor_takes_over_without_reopening_the_stream() {
        let made = Arc::new(AtomicUsize::new(0));
        let playing = Arc::new(AtomicUsize::new(0));
        let mut host = AudioHost::new({
            let (made, playing) = (Arc::clone(&made), Arc::clone(&playing));
            Box::new(move |_| {
                let id = made.fetch_add(1, Ordering::Relaxed) + 1;
                let playing = Arc::clone(&playing);
                Box::new(move |_: &mut AudioBuffers<'_>, _: &CallbackTime| {
                    playing.store(id, Ordering::Relaxed);
                })
            })
        });
        host.start(Some(BackendKind::Dummy), StreamConfig::default());
        let stream = |host: &AudioHost| {
            host.stream()
                .map(|s| std::ptr::from_ref(s).cast::<u8>() as usize)
        };
        let before = stream(&host);
        assert!(before.is_some());
        host.reattach();
        assert_eq!(made.load(Ordering::Relaxed), 2);
        assert_eq!(stream(&host), before, "the same stream");
        let start = Instant::now();
        while playing.load(Ordering::Relaxed) != 2 && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(playing.load(Ordering::Relaxed), 2);
        // Without a stream, it opens one.
        host.stop();
        host.reattach();
        assert!(host.stream().is_some());
        assert_eq!(made.load(Ordering::Relaxed), 3);
    }
}
