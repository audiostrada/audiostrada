//! Backend without hardware: runs the callback from a timer thread with silent inputs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::buffers::ChannelBuffers;
use crate::stream::{CallbackRunner, ThreadStream};
use crate::{
    AudioBackend, AudioStream, BackendKind, CallbackFactory, DeviceInfo, IoError, StreamConfig,
    StreamInfo, StreamStatus,
};

const DEVICE_ID: &str = "dummy";
const DEFAULT_RATE: u32 = 48_000;
const MAX_CHANNELS: u32 = 64;

#[derive(Debug, Default)]
pub struct DummyBackend;

impl AudioBackend for DummyBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Dummy
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        Ok(vec![DeviceInfo {
            id: DEVICE_ID.into(),
            name: "Dummy device".into(),
            inputs: MAX_CHANNELS,
            outputs: MAX_CHANNELS,
            is_default_input: true,
            is_default_output: true,
        }])
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        for device in [&config.input_device, &config.output_device] {
            if let Some(device) = device.as_deref().filter(|d| *d != DEVICE_ID) {
                return Err(IoError::DeviceNotFound(device.into()));
            }
        }
        if config.buffer_size == 0
            || config.sample_rate == Some(0)
            || config.inputs > MAX_CHANNELS
            || config.outputs > MAX_CHANNELS
        {
            return Err(IoError::UnsupportedConfig(format!("{config:?}")));
        }
        let info = StreamInfo {
            backend: BackendKind::Dummy,
            device: DEVICE_ID.into(),
            sample_rate: config.sample_rate.unwrap_or(DEFAULT_RATE),
            buffer_size: config.buffer_size,
            inputs: config.inputs,
            outputs: config.outputs,
        };
        let status = Arc::new(StreamStatus::default());
        let runner = CallbackRunner::new(factory(&info), Arc::clone(&status), info.sample_rate);
        let running = Arc::new(AtomicBool::new(true));
        let thread = {
            let info = info.clone();
            let running = Arc::clone(&running);
            thread::Builder::new()
                .name("audiostrada-dummy-audio".into())
                .spawn(move || run(info, runner, &running))
                .map_err(|e| IoError::backend(BackendKind::Dummy, e))?
        };
        Ok(Box::new(ThreadStream {
            info,
            status,
            running,
            thread: Some(thread),
        }))
    }
}

fn run(info: StreamInfo, mut runner: CallbackRunner, running: &AtomicBool) {
    let frames = info.buffer_size as usize;
    let mut storage = ChannelBuffers::new(info.inputs as usize, info.outputs as usize, frames);
    let period = Duration::from_secs_f64(frames as f64 / f64::from(info.sample_rate));
    let mut deadline = Instant::now();
    while running.load(Ordering::Acquire) {
        runner.run(&mut storage.buffers(frames));
        deadline += period;
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        } else if now - deadline > period {
            runner.status().add_xruns(1);
            deadline = now;
        }
    }
}
