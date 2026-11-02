use std::fmt;

use crate::{
    AudioBuffers, AudioCallback, AudioStream, BackendKind, CallbackTime, DeviceInfo, IoError,
    StreamConfig, StreamInfo,
};

/// Builds the audio callback once the stream format is known; called once, off the audio
/// thread, before the stream starts.
pub type CallbackFactory<'a> = dyn FnMut(&StreamInfo) -> Box<dyn AudioCallback> + 'a;

pub trait AudioBackend: Send + Sync + fmt::Debug {
    fn kind(&self) -> BackendKind;

    fn devices(&self) -> Result<Vec<DeviceInfo>, IoError>;

    /// Incremented whenever the device list may have changed; poll it to refresh device menus.
    fn device_generation(&self) -> u64 {
        0
    }

    /// Starts a duplex stream running the callback built by `factory` on the audio thread.
    fn open(
        &self,
        config: &StreamConfig,
        factory: &mut CallbackFactory<'_>,
    ) -> Result<Box<dyn AudioStream>, IoError>;
}

/// Opens a stream with a callback that does not depend on the stream format.
pub fn open_stream(
    backend: &dyn AudioBackend,
    config: &StreamConfig,
    callback: Box<dyn AudioCallback>,
) -> Result<Box<dyn AudioStream>, IoError> {
    let mut callback = Some(callback);
    backend.open(config, &mut |_| {
        callback.take().unwrap_or_else(|| {
            Box::new(|b: &mut AudioBuffers<'_>, _: &CallbackTime| b.clear_outputs())
        })
    })
}

/// Backends built for this platform, in auto-detection order.
pub fn compiled_backends() -> &'static [BackendKind] {
    &[
        #[cfg(all(target_os = "linux", feature = "pipewire"))]
        BackendKind::PipeWire,
        #[cfg(all(target_os = "linux", feature = "jack"))]
        BackendKind::Jack,
        #[cfg(all(target_os = "linux", feature = "alsa"))]
        BackendKind::Alsa,
        #[cfg(target_os = "macos")]
        BackendKind::CoreAudio,
        #[cfg(windows)]
        BackendKind::Wasapi,
        #[cfg(all(windows, not(target_arch = "x86")))]
        BackendKind::Asio,
        BackendKind::Dummy,
    ]
}

/// Connects to a specific backend.
pub fn backend(kind: BackendKind) -> Result<Box<dyn AudioBackend>, IoError> {
    match kind {
        #[cfg(all(target_os = "linux", feature = "pipewire"))]
        BackendKind::PipeWire => Ok(Box::new(crate::pipewire::PipeWireBackend::connect()?)),
        #[cfg(all(target_os = "linux", feature = "jack"))]
        BackendKind::Jack => Ok(Box::new(crate::jack::JackBackend::connect()?)),
        #[cfg(all(target_os = "linux", feature = "alsa"))]
        BackendKind::Alsa => Ok(Box::new(crate::alsa::AlsaBackend::connect()?)),
        #[cfg(target_os = "macos")]
        BackendKind::CoreAudio => Ok(Box::new(crate::coreaudio::CoreAudioBackend::connect()?)),
        #[cfg(windows)]
        BackendKind::Wasapi => Ok(Box::new(crate::wasapi::WasapiBackend::connect()?)),
        #[cfg(all(windows, not(target_arch = "x86")))]
        BackendKind::Asio => Ok(Box::new(crate::asio::AsioBackend::connect()?)),
        BackendKind::Dummy => Ok(Box::new(crate::dummy::DummyBackend)),
        _ => Err(IoError::BackendUnavailable(kind)),
    }
}

/// Picks the first hardware backend that works on this system.
pub fn auto_backend() -> Result<Box<dyn AudioBackend>, IoError> {
    for &kind in compiled_backends() {
        if kind == BackendKind::Dummy {
            continue;
        }
        match backend(kind) {
            Ok(backend) => return Ok(backend),
            Err(e) => tracing::info!("{kind} backend unavailable: {e}"),
        }
    }
    Err(IoError::NoBackend)
}
