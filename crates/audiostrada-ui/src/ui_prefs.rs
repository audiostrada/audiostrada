//! Interface preferences: theme, scale, shortcuts and panel layout.

use eframe::egui::{self, Visuals};
use serde::{Deserialize, Serialize};

use crate::keymap::Keymap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Midnight,
    Vintage,
    Studio,
    Light,
    Console,
    Neon,
    Paper,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 8] = [
        ThemeChoice::Dark,
        ThemeChoice::Midnight,
        ThemeChoice::Console,
        ThemeChoice::Neon,
        ThemeChoice::Vintage,
        ThemeChoice::Studio,
        ThemeChoice::Light,
        ThemeChoice::Paper,
    ];

    /// The name shown in the preferences, in the interface's language.
    pub fn name(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            ThemeChoice::Dark => tr("Modern"),
            ThemeChoice::Midnight => tr("Midnight"),
            ThemeChoice::Vintage => tr("Vintage"),
            ThemeChoice::Studio => tr("Studio gray"),
            ThemeChoice::Light => tr("Light"),
            ThemeChoice::Console => tr("Console"),
            ThemeChoice::Neon => tr("Neon"),
            ThemeChoice::Paper => tr("Paper"),
        }
    }

    pub fn palette(self) -> crate::theme::Palette {
        use crate::theme::*;
        match self {
            ThemeChoice::Dark => DARK,
            ThemeChoice::Midnight => MIDNIGHT,
            ThemeChoice::Vintage => VINTAGE,
            ThemeChoice::Studio => STUDIO,
            ThemeChoice::Light => LIGHT,
            ThemeChoice::Console => CONSOLE,
            ThemeChoice::Neon => NEON,
            ThemeChoice::Paper => PAPER,
        }
    }

    /// The palette with the user's accent color, if any.
    pub fn palette_with(self, accent: Option<[u8; 3]>) -> crate::theme::Palette {
        let mut palette = self.palette();
        if let Some([r, g, b]) = accent {
            palette.accent = egui::Color32::from_rgb(r, g, b);
        }
        palette
    }

    /// Makes this theme active and returns its visuals.
    pub fn visuals(self, accent: Option<[u8; 3]>) -> Visuals {
        let palette = self.palette_with(accent);
        crate::theme::set_palette(palette);
        crate::theme::visuals(palette)
    }
}

/// Size, place and state of a window, to open it as it was left. The place is unknown on
/// Wayland, where the compositor places windows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    /// Inner size when not maximized, in points.
    pub size: [f32; 2],
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
}

impl WindowState {
    /// What a viewport reports this frame, keeping the size it had before being maximized.
    pub fn read(ctx: &egui::Context, previous: Option<WindowState>) -> Option<WindowState> {
        // In window units: the interface scale is taken out of egui's points.
        let zoom = ctx.zoom_factor();
        ctx.input(|i| {
            let v = i.viewport();
            let maximized = v.maximized.unwrap_or(false);
            let area = i.viewport_rect().size() * zoom;
            let size = match (maximized, previous) {
                (true, Some(p)) => p.size,
                _ if area.x > 0.0 && area.y > 0.0 => [area.x.round(), area.y.round()],
                (_, Some(p)) => p.size,
                _ => return None,
            };
            // Maximized, the corner tells the screen it fills.
            let position = v
                .outer_rect
                .map(|r| [r.min.x, r.min.y])
                .or(previous.and_then(|p| p.position));
            Some(WindowState {
                size,
                position,
                maximized,
            })
        })
    }

    pub fn apply(self, builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
        let builder = builder
            .with_inner_size(self.size)
            .with_maximized(self.maximized);
        match self.position {
            Some(p) => builder.with_position(p),
            None => builder,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    pub language: crate::i18n::Language,
    pub theme: ThemeChoice,
    /// Accent color replacing the theme's.
    pub accent: Option<[u8; 3]>,
    /// Interface zoom factor.
    pub scale: f32,
    pub keymap: Keymap,
    /// Panel layout as JSON text, since TOML has no null.
    pub layout: Option<String>,
    /// Panels shown in their own windows.
    pub detached: Vec<crate::layout::Pane>,
    /// The main window as it was left.
    pub window: Option<WindowState>,
    /// The detached panels' windows as they were left.
    pub pane_windows: Vec<(crate::layout::Pane, WindowState)>,
    /// Version of the default layout `layout` was saved against; 0 when missing.
    #[serde(default)]
    pub layout_version: u32,
    /// Shift applied to recorded clips to compensate the input latency, in samples.
    pub record_offset: i64,
    /// Plugin folders searched besides the standard ones.
    pub plugin_dirs: Vec<std::path::PathBuf>,
    /// Last export settings.
    pub export: crate::document::ExportSettings,
    /// MIDI inputs left closed.
    pub midi_disabled: Vec<String>,
    /// Each plugin runs in its own process, so a crash cannot take AudioStrada down.
    pub plugin_sandbox: bool,
    /// Third-party plugins are neither scanned nor loaded, from the next start.
    pub disable_plugins: bool,
    /// Armed tracks hear their input only while stopped or recording.
    pub auto_monitoring: bool,
    /// Stopping with the play key goes back to where playback started (as in Reaper).
    pub return_on_stop: bool,
    /// Linux under Wayland: run through X11 (XWayland), where AudioStrada can put each window back
    /// on its screen and keep effect and plugin windows above it; the compositor places windows
    /// itself under Wayland. On by default, off for a sharp fractional display scale.
    pub x11_windows: bool,
    /// Projects open in tabs when AudioStrada was closed, reopened at the next start.
    pub tabs: Vec<std::path::PathBuf>,
    /// Index in `tabs` of the tab that was shown.
    pub tab: usize,
    /// Official builds look for new versions once a day; `None` until the user was asked.
    pub check_updates: Option<bool>,
    /// Day of the last check (YYYY-MM-DD).
    pub updates_checked: Option<String>,
    /// Width of the timeline's track headers.
    pub header_width: f32,
    /// Narrow mixer strips, but for the one opened by a click.
    pub mixer_compact: bool,
    /// The timeline's tool.
    #[serde(default)]
    pub timeline_tool: crate::timeline::Tool,
    /// How the transport's counter shows the position.
    pub counter: crate::transport::Counter,
    /// The transport sits under the workspace instead of above it.
    pub transport_bottom: bool,
    /// The transport drawn larger, to read it from afar.
    pub transport_large: bool,
    /// How the timeline draws waveforms.
    pub waveforms: crate::timeline::WaveView,
    /// The recording mode's panel layout, as `layout`.
    pub recording_layout: Option<String>,
    /// Audio buffer of the recording mode, in frames.
    pub recording_buffer: u32,
    /// In the recording mode, effects on the way of what is played that add more than this
    /// (in ms) are bypassed.
    pub recording_bypass_ms: f32,
    /// Arming a track offers the recording mode.
    pub recording_offer: bool,
    /// Hardware input of the talkback microphone, from 0.
    pub talkback_input: Option<u16>,
    /// What the rest of the headphone mixes is lowered by while talking.
    pub talkback_duck_db: f32,
    /// The talkback key switches it on and off, instead of talking while it is held.
    pub talkback_latch: bool,
    /// AI assistants may drive AudioStrada through the control channel (MCP).
    pub assistants: bool,
    /// Local port of the control channel.
    pub assistants_port: u16,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            language: crate::i18n::Language::default(),
            theme: ThemeChoice::default(),
            accent: None,
            scale: 1.0,
            keymap: Keymap::default(),
            layout: None,
            detached: Vec::new(),
            window: None,
            pane_windows: Vec::new(),
            layout_version: crate::layout::VERSION,
            record_offset: 0,
            plugin_dirs: Vec::new(),
            export: crate::document::ExportSettings::default(),
            midi_disabled: Vec::new(),
            plugin_sandbox: true,
            disable_plugins: false,
            auto_monitoring: false,
            return_on_stop: true,
            x11_windows: true,
            tabs: Vec::new(),
            tab: 0,
            check_updates: None,
            updates_checked: None,
            header_width: 210.0,
            mixer_compact: false,
            timeline_tool: crate::timeline::Tool::Smart,
            counter: crate::transport::Counter::default(),
            transport_bottom: false,
            transport_large: false,
            waveforms: crate::timeline::WaveView::default(),
            recording_layout: None,
            recording_buffer: 64,
            recording_bypass_ms: 2.0,
            recording_offer: true,
            talkback_input: None,
            talkback_duck_db: -15.0,
            talkback_latch: false,
            assistants: false,
            assistants_port: crate::integrations::DEFAULT_PORT,
        }
    }
}

impl UiPrefs {
    pub fn apply(&self, ctx: &egui::Context) {
        crate::i18n::set_language(self.language);
        crate::theme::install_fonts(ctx);
        ctx.set_visuals(self.theme.visuals(self.accent));
        ctx.all_styles_mut(crate::theme::style);
        ctx.set_zoom_factor(self.scale.clamp(0.5, 3.0));
    }

    /// Applies the plugin sandbox setting to plugins loaded from now on.
    pub fn apply_sandbox(&self) {
        let host = if self.plugin_sandbox {
            audiostrada_plugins::own_executable()
        } else {
            None
        };
        audiostrada_plugins::set_sandbox(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_take_their_defaults() {
        let prefs: UiPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(prefs.layout_version, 0, "an old layout is upgraded");
        assert!(prefs.plugin_sandbox && prefs.return_on_stop);
        assert_eq!(prefs.scale, 1.0);
        let mut changed = UiPrefs {
            theme: ThemeChoice::Neon,
            accent: Some([1, 2, 3]),
            ..UiPrefs::default()
        };
        changed.detached.push(crate::layout::Pane::Mixer);
        let text = serde_json::to_string(&changed).unwrap();
        assert_eq!(serde_json::from_str::<UiPrefs>(&text).unwrap(), changed);
    }

    #[test]
    fn themes_and_accents() {
        let mut all = ThemeChoice::ALL.to_vec();
        all.sort_by_key(|t| *t as u8);
        all.dedup();
        assert_eq!(all.len(), 8);
        let palette = ThemeChoice::Light.palette_with(Some([1, 2, 3]));
        assert_eq!(palette.accent, egui::Color32::from_rgb(1, 2, 3));
        assert!(!palette.dark);
        assert!(ThemeChoice::Dark.palette().dark);
    }
}
