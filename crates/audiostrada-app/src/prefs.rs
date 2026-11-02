//! User preferences, stored as TOML in the platform configuration directory.

use std::fs;
use std::path::PathBuf;

use audiostrada_io::{BackendKind, StreamConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioPrefs {
    pub backend: Option<String>,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_size: Option<u32>,
    pub inputs: Option<u32>,
    pub outputs: Option<u32>,
}

impl AudioPrefs {
    pub fn backend(&self) -> Option<BackendKind> {
        self.backend.as_deref().and_then(BackendKind::from_name)
    }

    pub fn config(&self) -> StreamConfig {
        let defaults = StreamConfig::default();
        StreamConfig {
            input_device: self.input_device.clone(),
            output_device: self.output_device.clone(),
            sample_rate: self.sample_rate,
            buffer_size: self.buffer_size.unwrap_or(defaults.buffer_size),
            inputs: self.inputs.unwrap_or(defaults.inputs),
            outputs: self.outputs.unwrap_or(defaults.outputs),
        }
    }

    pub fn from_config(backend: Option<BackendKind>, config: &StreamConfig) -> Self {
        Self {
            backend: backend.map(|b| b.name().to_owned()),
            input_device: config.input_device.clone(),
            output_device: config.output_device.clone(),
            sample_rate: config.sample_rate,
            buffer_size: Some(config.buffer_size),
            inputs: Some(config.inputs),
            outputs: Some(config.outputs),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub audio: AudioPrefs,
    /// Recently opened projects, most recent first.
    pub recent: Vec<PathBuf>,
    pub ui: audiostrada_ui::UiPrefs,
}

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("preferences.toml"))
}

impl Prefs {
    /// Loads preferences, falling back to defaults when missing or unreadable.
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("ignoring invalid preferences {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let result = toml::to_string_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                if let Some(dir) = path.parent() {
                    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                fs::write(&path, text).map_err(|e| e.to_string())
            });
        if let Err(e) = result {
            tracing::warn!("cannot save preferences to {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_through_toml() {
        let config = StreamConfig {
            output_device: Some("hw:1".into()),
            sample_rate: Some(96_000),
            buffer_size: 64,
            ..StreamConfig::default()
        };
        let prefs = Prefs {
            audio: AudioPrefs::from_config(Some(BackendKind::Alsa), &config),
            recent: vec![PathBuf::from("/music/Song/Song.strada")],
            ui: audiostrada_ui::UiPrefs {
                layout: Some(r#"{"root":null}"#.into()),
                window: Some(audiostrada_ui::WindowState {
                    size: [1400.0, 900.0],
                    position: None,
                    maximized: true,
                }),
                pane_windows: vec![(
                    audiostrada_ui::Pane::Mixer,
                    audiostrada_ui::WindowState {
                        size: [1100.0, 560.0],
                        position: Some([40.0, 60.0]),
                        maximized: false,
                    },
                )],
                ..audiostrada_ui::UiPrefs::default()
            },
        };
        let back: Prefs = toml::from_str(&toml::to_string_pretty(&prefs).unwrap()).unwrap();
        assert_eq!(back, prefs);
        assert_eq!(back.audio.backend(), Some(BackendKind::Alsa));
        assert_eq!(back.audio.config(), config);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let prefs: Prefs = toml::from_str("[audio]\nbuffer_size = 128\n").unwrap();
        assert_eq!(prefs.audio.config().buffer_size, 128);
        assert_eq!(prefs.audio.config().inputs, StreamConfig::default().inputs);
        assert_eq!(prefs.audio.backend(), None);
    }
}
