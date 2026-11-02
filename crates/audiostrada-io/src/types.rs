use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    PipeWire,
    Jack,
    Alsa,
    Wasapi,
    Asio,
    CoreAudio,
    Dummy,
}

impl BackendKind {
    pub const ALL: [BackendKind; 7] = [
        BackendKind::PipeWire,
        BackendKind::Jack,
        BackendKind::Alsa,
        BackendKind::Wasapi,
        BackendKind::Asio,
        BackendKind::CoreAudio,
        BackendKind::Dummy,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BackendKind::PipeWire => "PipeWire",
            BackendKind::Jack => "JACK",
            BackendKind::Alsa => "ALSA",
            BackendKind::Wasapi => "WASAPI",
            BackendKind::Asio => "ASIO",
            BackendKind::CoreAudio => "CoreAudio",
            BackendKind::Dummy => "Dummy",
        }
    }

    /// Parses a backend name, case-insensitively.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(name))
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier, to be passed in [`StreamConfig`].
    pub id: String,
    pub name: String,
    pub inputs: u32,
    pub outputs: u32,
    pub is_default_input: bool,
    pub is_default_output: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamConfig {
    /// Capture device identifier; `None` uses the system default.
    pub input_device: Option<String>,
    /// Playback device identifier; `None` uses the system default.
    pub output_device: Option<String>,
    /// `None` keeps the device or graph rate.
    pub sample_rate: Option<u32>,
    /// Requested buffer size in frames.
    pub buffer_size: u32,
    pub inputs: u32,
    pub outputs: u32,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            sample_rate: None,
            buffer_size: 256,
            inputs: 2,
            outputs: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    pub backend: BackendKind,
    /// Human readable description of the devices in use.
    pub device: String,
    pub sample_rate: u32,
    /// Nominal buffer size in frames.
    pub buffer_size: u32,
    pub inputs: u32,
    pub outputs: u32,
}

impl StreamInfo {
    pub fn buffer_latency_ms(&self) -> f64 {
        f64::from(self.buffer_size) * 1000.0 / f64::from(self.sample_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_names_round_trip() {
        for kind in BackendKind::ALL {
            assert_eq!(
                BackendKind::from_name(&kind.name().to_lowercase()),
                Some(kind)
            );
        }
        assert_eq!(BackendKind::from_name("nope"), None);
    }

    #[test]
    fn latency_in_ms() {
        let info = StreamInfo {
            backend: BackendKind::Dummy,
            device: String::new(),
            sample_rate: 48_000,
            buffer_size: 240,
            inputs: 0,
            outputs: 2,
        };
        assert_eq!(info.buffer_latency_ms(), 5.0);
    }
}
