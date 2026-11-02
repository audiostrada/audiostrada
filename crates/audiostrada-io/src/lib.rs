//! AudioStrada audio backends (PipeWire, JACK, ALSA, WASAPI, ASIO, CoreAudio).
// Builds without any hardware backend leave the shared backend helpers unused.
#![cfg_attr(
    all(
        target_os = "linux",
        not(any(feature = "pipewire", feature = "jack", feature = "alsa"))
    ),
    allow(dead_code)
)]

#[cfg(all(target_os = "linux", feature = "alsa"))]
pub mod alsa;
#[cfg(all(windows, not(target_arch = "x86")))]
pub mod asio;
mod backend;
mod buffers;
#[cfg(any(all(target_os = "linux", feature = "alsa"), windows))]
mod convert;
#[cfg(target_os = "macos")]
pub mod coreaudio;
pub mod diagnostics;
pub mod dummy;
mod error;
#[cfg(all(target_os = "linux", feature = "jack"))]
pub mod jack;
#[cfg(all(target_os = "linux", feature = "pipewire"))]
pub mod pipewire;
#[cfg(all(target_os = "linux", any(feature = "pipewire", feature = "jack")))]
mod routing;
mod stream;
mod timing;
pub mod tools;
mod types;
#[cfg(windows)]
pub mod wasapi;
#[cfg(any(
    all(target_os = "linux", feature = "alsa"),
    windows,
    target_os = "macos"
))]
mod watch;

pub use backend::{
    AudioBackend, CallbackFactory, auto_backend, backend, compiled_backends, open_stream,
};
pub use buffers::{AudioBuffers, ChannelBuffers};
pub use error::IoError;
pub use stream::{AudioCallback, AudioStream, CallbackTime, StreamStatus};
pub use types::{BackendKind, DeviceInfo, StreamConfig, StreamInfo};
