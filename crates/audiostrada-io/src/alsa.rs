//! Direct ALSA backend, for systems without PipeWire or JACK.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use crate::convert::{SampleFormat, deinterleave, interleave};
use crate::stream::{CallbackRunner, ThreadStream};
use crate::{
    AudioBackend, AudioStream, BackendKind, CallbackFactory, ChannelBuffers, DeviceInfo, IoError,
    StreamConfig, StreamInfo, StreamStatus,
};

const DEFAULT_DEVICE: &str = "default";
const DEFAULT_RATE: u32 = 48_000;
const PERIODS: u32 = 2;
const MAX_CHANNELS: u32 = 64;
const FORMATS: [(Format, SampleFormat); 4] = [
    (Format::FloatLE, SampleFormat::F32),
    (Format::S32LE, SampleFormat::S32),
    (Format::S243LE, SampleFormat::S24Packed),
    (Format::S16LE, SampleFormat::S16),
];

fn backend_error(message: impl ToString) -> IoError {
    IoError::backend(BackendKind::Alsa, message)
}

#[derive(Debug)]
pub struct AlsaBackend {
    watcher: crate::watch::DeviceWatcher,
}

impl AlsaBackend {
    pub fn connect() -> Result<Self, IoError> {
        Ok(Self {
            watcher: crate::watch::DeviceWatcher::start(|| {
                alsa::card::Iter::new()
                    .flatten()
                    .filter_map(|c| c.get_longname().ok())
                    .collect()
            }),
        })
    }
}

/// Maximum channel count of a PCM in one direction, or `None` if it cannot be opened.
fn max_channels(name: &str, direction: Direction) -> Option<u32> {
    let pcm = PCM::new(name, direction, true).ok()?;
    let params = HwParams::any(&pcm).ok()?;
    params.get_channels_max().ok().map(|c| c.min(MAX_CHANNELS))
}

impl AudioBackend for AlsaBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Alsa
    }

    fn device_generation(&self) -> u64 {
        self.watcher.generation()
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        let hints = alsa::device_name::HintIter::new_str(None, "pcm").map_err(backend_error)?;
        let mut devices = Vec::new();
        for hint in hints {
            let Some(name) = hint.name else { continue };
            if name != DEFAULT_DEVICE && !name.starts_with("hw:") {
                continue;
            }
            let capture = hint.direction != Some(Direction::Playback);
            let playback = hint.direction != Some(Direction::Capture);
            let inputs = capture.then(|| max_channels(&name, Direction::Capture));
            let outputs = playback.then(|| max_channels(&name, Direction::Playback));
            let busy = matches!(inputs, Some(None)) || matches!(outputs, Some(None));
            let description = hint
                .desc
                .as_deref()
                .map(|d| d.replace('\n', ", "))
                .unwrap_or_else(|| name.clone());
            devices.push(DeviceInfo {
                is_default_input: name == DEFAULT_DEVICE,
                is_default_output: name == DEFAULT_DEVICE,
                id: name,
                name: if busy {
                    format!("{description} (busy)")
                } else {
                    description
                },
                inputs: inputs.flatten().unwrap_or(0),
                outputs: outputs.flatten().unwrap_or(0),
            });
        }
        Ok(devices)
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        if config.inputs > MAX_CHANNELS || config.outputs > MAX_CHANNELS || config.buffer_size == 0
        {
            return Err(IoError::UnsupportedConfig(format!("{config:?}")));
        }
        let in_name = config.input_device.as_deref().unwrap_or(DEFAULT_DEVICE);
        let out_name = config.output_device.as_deref().unwrap_or(DEFAULT_DEVICE);
        let rate = config.sample_rate.unwrap_or(DEFAULT_RATE);
        let playback = (config.outputs > 0)
            .then(|| {
                Side::open(
                    out_name,
                    Direction::Playback,
                    config.outputs,
                    rate,
                    config.buffer_size,
                )
            })
            .transpose()?;
        let rate = playback.as_ref().map_or(rate, |p| p.rate);
        let capture = (config.inputs > 0)
            .then(|| {
                Side::open(
                    in_name,
                    Direction::Capture,
                    config.inputs,
                    rate,
                    config.buffer_size,
                )
            })
            .transpose()?;
        if let (Some(p), Some(c)) = (&playback, &capture) {
            if p.rate != c.rate || p.period != c.period {
                return Err(IoError::UnsupportedConfig(format!(
                    "capture runs at {} Hz / {} frames, playback at {} Hz / {} frames",
                    c.rate, c.period, p.rate, p.period
                )));
            }
            if let Err(e) = c.pcm.link(&p.pcm) {
                tracing::debug!("ALSA capture and playback not linked: {e}");
            }
        }
        let period = playback
            .as_ref()
            .or(capture.as_ref())
            .map_or(0, |s| s.period);
        let info = StreamInfo {
            backend: BackendKind::Alsa,
            device: format!(
                "in: {}, out: {}",
                if capture.is_some() { in_name } else { "none" },
                if playback.is_some() { out_name } else { "none" }
            ),
            sample_rate: rate,
            buffer_size: period as u32,
            inputs: config.inputs,
            outputs: config.outputs,
        };
        let status = Arc::new(StreamStatus::default());
        let running = Arc::new(AtomicBool::new(true));
        let worker = Worker {
            storage: ChannelBuffers::new(config.inputs as usize, config.outputs as usize, period),
            runner: CallbackRunner::new(factory(&info), Arc::clone(&status), rate),
            inputs: config.inputs as usize,
            outputs: config.outputs as usize,
            period,
            capture,
            playback,
        };
        let thread = {
            let running = Arc::clone(&running);
            thread::Builder::new()
                .name("audiostrada-alsa-audio".into())
                .spawn(move || worker.run(&running))
                .map_err(backend_error)?
        };
        Ok(Box::new(ThreadStream {
            info,
            status,
            running,
            thread: Some(thread),
        }))
    }
}

/// One direction of the stream: the PCM and its interleaved byte buffer.
struct Side {
    pcm: PCM,
    format: SampleFormat,
    channels: usize,
    rate: u32,
    period: usize,
    bytes: Vec<u8>,
}

impl Side {
    fn open(
        name: &str,
        direction: Direction,
        channels: u32,
        rate: u32,
        period: u32,
    ) -> Result<Self, IoError> {
        let error = |e: alsa::Error| backend_error(format!("{name}: {e}"));
        let pcm = PCM::new(name, direction, false).map_err(error)?;
        let (format, channels, rate, period) = {
            let params = HwParams::any(&pcm).map_err(error)?;
            params.set_access(Access::RWInterleaved).map_err(error)?;
            let (alsa_format, format) = FORMATS
                .into_iter()
                .find(|(f, _)| params.test_format(*f).is_ok())
                .ok_or_else(|| backend_error(format!("{name}: no supported sample format")))?;
            params.set_format(alsa_format).map_err(error)?;
            let channels = params.set_channels_near(channels).map_err(error)?;
            let rate = params
                .set_rate_near(rate, ValueOr::Nearest)
                .map_err(error)?;
            let period = params
                .set_period_size_near(period as alsa::pcm::Frames, ValueOr::Nearest)
                .map_err(error)?;
            params
                .set_periods_near(PERIODS, ValueOr::Nearest)
                .map_err(error)?;
            pcm.hw_params(&params).map_err(error)?;
            (format, channels as usize, rate, period as usize)
        };
        {
            let sw = pcm.sw_params_current().map_err(error)?;
            sw.set_avail_min(period as alsa::pcm::Frames)
                .map_err(error)?;
            sw.set_start_threshold(alsa::pcm::Frames::MAX)
                .map_err(error)?;
            pcm.sw_params(&sw).map_err(error)?;
        }
        Ok(Self {
            bytes: vec![0; period * channels * format.bytes()],
            pcm,
            format,
            channels,
            rate,
            period,
        })
    }
}

struct Worker {
    storage: ChannelBuffers,
    runner: CallbackRunner,
    inputs: usize,
    outputs: usize,
    period: usize,
    capture: Option<Side>,
    playback: Option<Side>,
}

// SAFETY: the PCM handles are only used from the worker thread once moved there.
unsafe impl Send for Worker {}

impl Worker {
    fn run(mut self, running: &AtomicBool) {
        if let Err(e) =
            audiostrada_rt::thread::promote_current_thread(audiostrada_rt::thread::AUDIO_PRIORITY)
        {
            tracing::warn!("ALSA audio thread runs without real-time priority: {e}");
        }
        if let Err(e) = self.restart() {
            tracing::error!("cannot start ALSA stream: {e}");
            self.runner.status().set_failed();
            return;
        }
        while running.load(Ordering::Acquire) {
            if let Err(e) = self.cycle() {
                self.runner.status().add_xruns(1);
                tracing::debug!("ALSA xrun: {e}");
                if let Err(e) = self.restart() {
                    tracing::error!("cannot restart ALSA stream: {e}");
                    self.runner.status().set_failed();
                    return;
                }
            }
        }
        for side in [&self.capture, &self.playback].into_iter().flatten() {
            let _ = side.pcm.drop();
        }
    }

    /// Prepares both PCMs, fills the playback buffer with silence and starts.
    fn restart(&mut self) -> alsa::Result<()> {
        for side in [&self.capture, &self.playback].into_iter().flatten() {
            let _ = side.pcm.drop();
            side.pcm.prepare()?;
        }
        if let Some(p) = &mut self.playback {
            p.bytes.fill(0);
            for _ in 0..PERIODS {
                write_all(p)?;
            }
            p.pcm.start()?;
        }
        if let Some(c) = &self.capture
            && c.pcm.state() != alsa::pcm::State::Running
        {
            c.pcm.start()?;
        }
        Ok(())
    }

    fn cycle(&mut self) -> alsa::Result<()> {
        if let Some(c) = &mut self.capture {
            read_all(c)?;
            deinterleave(
                c.format,
                &c.bytes,
                c.channels,
                self.period,
                &mut self.storage,
                self.inputs,
            );
        }
        self.runner.run(&mut self.storage.buffers(self.period));
        if let Some(p) = &mut self.playback {
            interleave(
                p.format,
                &self.storage,
                self.outputs,
                &mut p.bytes,
                p.channels,
                self.period,
            );
            write_all(p)?;
        }
        Ok(())
    }
}

fn read_all(side: &mut Side) -> alsa::Result<()> {
    let frame_bytes = side.channels * side.format.bytes();
    let mut done = 0;
    while done < side.period {
        let io = side.pcm.io_bytes();
        done += io.readi(&mut side.bytes[done * frame_bytes..])?;
    }
    Ok(())
}

fn write_all(side: &mut Side) -> alsa::Result<()> {
    let frame_bytes = side.channels * side.format.bytes();
    let mut done = 0;
    while done < side.period {
        let io = side.pcm.io_bytes();
        done += io.writei(&side.bytes[done * frame_bytes..])?;
    }
    Ok(())
}
