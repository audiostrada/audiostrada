//! JACK backend (also works with PipeWire's JACK implementation).
//!
//! JACK exposes physical ports grouped by client; each group is presented as a device.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use jack::{
    AsyncClient, AudioIn, AudioOut, Client, ClientOptions, Control, NotificationHandler, Port,
    PortFlags, PortId, ProcessHandler, ProcessScope,
};

use crate::routing::pair_ports;
use crate::stream::CallbackRunner;
use crate::{
    AudioBackend, AudioBuffers, AudioStream, BackendKind, CallbackFactory, DeviceInfo, IoError,
    StreamConfig, StreamInfo, StreamStatus,
};

const CLIENT_NAME: &str = "AudioStrada";
const AUDIO_TYPE: &str = "32 bit float mono audio";
const DEFAULT_DEVICE: &str = "system";
const MAX_CHANNELS: u32 = 64;

fn backend_error(message: impl ToString) -> IoError {
    IoError::backend(BackendKind::Jack, message)
}

/// Physical ports by device name: (capture ports, playback ports), in registration order.
fn physical_ports(client: &Client) -> BTreeMap<String, (Vec<String>, Vec<String>)> {
    let mut devices: BTreeMap<String, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for (flag, capture) in [(PortFlags::IS_OUTPUT, true), (PortFlags::IS_INPUT, false)] {
        for port in client.ports(None, Some(AUDIO_TYPE), PortFlags::IS_PHYSICAL | flag) {
            let Some((device, _)) = port.split_once(':') else {
                continue;
            };
            let entry = devices.entry(device.to_owned()).or_default();
            if capture {
                entry.0.push(port);
            } else {
                entry.1.push(port);
            }
        }
    }
    devices
}

/// Default device: `system` when present, otherwise the first device with ports in that direction.
fn default_device(
    devices: &BTreeMap<String, (Vec<String>, Vec<String>)>,
    capture: bool,
) -> Option<String> {
    let has_ports = |ports: &(Vec<String>, Vec<String>)| {
        if capture {
            !ports.0.is_empty()
        } else {
            !ports.1.is_empty()
        }
    };
    devices
        .get(DEFAULT_DEVICE)
        .filter(|p| has_ports(p))
        .map(|_| DEFAULT_DEVICE.to_owned())
        .or_else(|| {
            devices
                .iter()
                .find(|(_, p)| has_ports(p))
                .map(|(name, _)| name.clone())
        })
}

pub struct JackBackend {
    client: Client,
    device_generation: Arc<AtomicU64>,
}

impl fmt::Debug for JackBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JackBackend").finish_non_exhaustive()
    }
}

impl JackBackend {
    /// Connects to a running JACK server; never starts one.
    pub fn connect() -> Result<Self, IoError> {
        let (client, _) = Client::new(
            &format!("{CLIENT_NAME}-control"),
            ClientOptions::NO_START_SERVER,
        )
        .map_err(|_| IoError::BackendUnavailable(BackendKind::Jack))?;
        Ok(Self {
            client,
            device_generation: Arc::new(AtomicU64::new(0)),
        })
    }
}

impl AudioBackend for JackBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Jack
    }

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError> {
        let devices = physical_ports(&self.client);
        let default_in = default_device(&devices, true);
        let default_out = default_device(&devices, false);
        Ok(devices
            .iter()
            .map(|(name, (capture, playback))| DeviceInfo {
                id: name.clone(),
                name: name.clone(),
                inputs: capture.len() as u32,
                outputs: playback.len() as u32,
                is_default_input: default_in.as_deref() == Some(name),
                is_default_output: default_out.as_deref() == Some(name),
            })
            .collect())
    }

    fn device_generation(&self) -> u64 {
        self.device_generation.load(Ordering::Relaxed)
    }

    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError> {
        if config.inputs > MAX_CHANNELS || config.outputs > MAX_CHANNELS {
            return Err(IoError::UnsupportedConfig(format!("{config:?}")));
        }
        let (client, _) =
            Client::new(CLIENT_NAME, ClientOptions::NO_START_SERVER).map_err(backend_error)?;
        if config.buffer_size != client.buffer_size()
            && let Err(e) = client.set_buffer_size(config.buffer_size)
        {
            tracing::warn!("JACK refused buffer size {}: {e}", config.buffer_size);
        }
        let rate = client.sample_rate();
        if config.sample_rate.is_some_and(|r| r != rate) {
            tracing::warn!("JACK runs at {rate} Hz; the sample rate is set by the JACK server");
        }

        let devices = physical_ports(&client);
        let resolve = |wanted: &Option<String>, capture: bool| match wanted {
            Some(name) if devices.contains_key(name) => Ok(Some(name.clone())),
            Some(name) => Err(IoError::DeviceNotFound(name.clone())),
            None => Ok(default_device(&devices, capture)),
        };
        let input = match config.inputs {
            0 => None,
            _ => resolve(&config.input_device, true)?,
        };
        let output = match config.outputs {
            0 => None,
            _ => resolve(&config.output_device, false)?,
        };

        let ins = (0..config.inputs)
            .map(|i| client.register_port(&format!("in_{}", i + 1), AudioIn::default()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend_error)?;
        let outs = (0..config.outputs)
            .map(|i| client.register_port(&format!("out_{}", i + 1), AudioOut::default()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(backend_error)?;
        let our_in: Vec<String> = ins.iter().filter_map(|p| p.name().ok()).collect();
        let our_out: Vec<String> = outs.iter().filter_map(|p| p.name().ok()).collect();

        let info = StreamInfo {
            backend: BackendKind::Jack,
            device: format!(
                "in: {}, out: {}",
                input.as_deref().unwrap_or("none"),
                output.as_deref().unwrap_or("none")
            ),
            sample_rate: rate,
            buffer_size: client.buffer_size(),
            inputs: config.inputs,
            outputs: config.outputs,
        };
        let status = Arc::new(StreamStatus::default());
        let process = Process {
            in_ptrs: vec![std::ptr::null(); ins.len()],
            out_ptrs: vec![std::ptr::null_mut(); outs.len()],
            ins,
            outs,
            runner: CallbackRunner::new(factory(&info), Arc::clone(&status), rate),
            rate,
        };
        let notifications = Notifications {
            status: Arc::clone(&status),
            device_generation: Arc::clone(&self.device_generation),
        };
        let active = client
            .activate_async(notifications, process)
            .map_err(backend_error)?;

        let connect = |from: &str, to: &str| {
            if let Err(e) = active.as_client().connect_ports_by_name(from, to) {
                tracing::warn!("cannot connect {from} to {to}: {e}");
            }
        };
        if let Some((_, playback)) = output.as_ref().and_then(|d| devices.get(d)) {
            for (theirs, ours) in pair_ports(playback, &our_out) {
                connect(&ours, &theirs);
            }
        }
        if let Some((capture, _)) = input.as_ref().and_then(|d| devices.get(d)) {
            for (theirs, ours) in pair_ports(capture, &our_in) {
                connect(&theirs, &ours);
            }
        }

        Ok(Box::new(JackStream {
            info,
            status,
            client: Some(active),
        }))
    }
}

struct Process {
    ins: Vec<Port<AudioIn>>,
    outs: Vec<Port<AudioOut>>,
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
    runner: CallbackRunner,
    rate: u32,
}

// SAFETY: the raw pointers are scratch storage rewritten at the start of every cycle.
unsafe impl Send for Process {}

impl ProcessHandler for Process {
    fn process(&mut self, client: &Client, scope: &ProcessScope) -> Control {
        let rate = client.sample_rate();
        if rate != self.rate {
            self.rate = rate;
            self.runner.set_sample_rate(rate);
        }
        for (ptr, port) in self.in_ptrs.iter_mut().zip(&self.ins) {
            *ptr = port.as_slice(scope).as_ptr();
        }
        for (ptr, port) in self.out_ptrs.iter_mut().zip(&mut self.outs) {
            *ptr = port.as_mut_slice(scope).as_mut_ptr();
        }
        // SAFETY: JACK port buffers hold `n_frames` samples and output ports are distinct.
        let mut buffers = unsafe {
            AudioBuffers::from_raw(&self.in_ptrs, &self.out_ptrs, scope.n_frames() as usize)
        };
        self.runner.run(&mut buffers);
        Control::Continue
    }
}

struct Notifications {
    status: Arc<StreamStatus>,
    device_generation: Arc<AtomicU64>,
}

impl NotificationHandler for Notifications {
    fn xrun(&mut self, _: &Client) -> Control {
        self.status.add_xruns(1);
        Control::Continue
    }

    fn port_registration(&mut self, _: &Client, _port: PortId, _registered: bool) {
        self.device_generation.fetch_add(1, Ordering::Relaxed);
    }
}

struct JackStream {
    info: StreamInfo,
    status: Arc<StreamStatus>,
    client: Option<AsyncClient<Notifications, Process>>,
}

impl fmt::Debug for JackStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JackStream")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl AudioStream for JackStream {
    fn info(&self) -> &StreamInfo {
        &self.info
    }

    fn status(&self) -> &StreamStatus {
        &self.status
    }
}

impl Drop for JackStream {
    fn drop(&mut self) {
        if let Some(client) = self.client.take()
            && let Err(e) = client.deactivate()
        {
            tracing::warn!("JACK deactivate failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices(list: &[(&str, usize, usize)]) -> BTreeMap<String, (Vec<String>, Vec<String>)> {
        list.iter()
            .map(|&(name, c, p)| {
                let ports = |kind: &str, n| (0..n).map(|i| format!("{name}:{kind}_{i}")).collect();
                (name.to_owned(), (ports("capture", c), ports("playback", p)))
            })
            .collect()
    }

    #[test]
    fn system_is_preferred_as_default() {
        let d = devices(&[("a_usb", 2, 2), ("system", 2, 2)]);
        assert_eq!(default_device(&d, true).as_deref(), Some("system"));
        let d = devices(&[("a_usb", 0, 2), ("b_mic", 1, 0)]);
        assert_eq!(default_device(&d, true).as_deref(), Some("b_mic"));
        assert_eq!(default_device(&d, false).as_deref(), Some("a_usb"));
        assert_eq!(default_device(&BTreeMap::new(), false), None);
    }
}
