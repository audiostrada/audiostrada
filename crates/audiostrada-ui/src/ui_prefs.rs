//! Interface preferences: theme, scale, shortcuts and panel layout.

use eframe::egui::{self, Visuals};
use serde::{Deserialize, Serialize};

use crate::keymap::Keymap;

/// A list read from saved preferences, leaving out entries this version no longer knows
/// (a removed panel or command) instead of failing to load them all.
pub(crate) fn known<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entry<T> {
        Known(T),
        Unknown(serde::de::IgnoredAny),
    }
    Ok(Vec::<Entry<T>>::deserialize(deserializer)?
        .into_iter()
        .filter_map(|e| match e {
            Entry::Known(t) => Some(t),
            Entry::Unknown(_) => None,
        })
        .collect())
}

/// The themes: three drawn ones and the user's own. Themes of older versions open as the
/// closest one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    #[serde(alias = "Midnight", alias = "Console", alias = "Neon")]
    Dark,
    #[serde(alias = "Vintage")]
    Studio,
    #[serde(alias = "Paper")]
    Light,
    Custom,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 4] = [
        ThemeChoice::Dark,
        ThemeChoice::Studio,
        ThemeChoice::Light,
        ThemeChoice::Custom,
    ];

    /// The name shown in the preferences, in the interface's language.
    pub fn name(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            ThemeChoice::Dark => tr("Dark"),
            ThemeChoice::Studio => tr("Studio gray"),
            ThemeChoice::Light => tr("Light"),
            ThemeChoice::Custom => tr("Custom"),
        }
    }

    /// Its palette; `custom` sets the user's own.
    pub fn palette(self, custom: &CustomTheme) -> crate::theme::Palette {
        use crate::theme::{DARK, LIGHT, STUDIO};
        match self {
            ThemeChoice::Dark => DARK,
            ThemeChoice::Studio => STUDIO,
            ThemeChoice::Light => LIGHT,
            ThemeChoice::Custom => custom.palette(),
        }
    }

    /// Makes this theme active and returns its visuals.
    pub fn visuals(self, custom: &CustomTheme) -> Visuals {
        let palette = self.palette(custom);
        crate::theme::set_palette(palette);
        crate::theme::visuals(palette)
    }
}

/// The user's own theme: a background, an accent and a contrast, from which every color is
/// made readable, and the look.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    pub background: [u8; 3],
    pub accent: [u8; 3],
    /// 0 soft, 1 strong.
    pub contrast: f32,
    /// Rounding of buttons and fields, in points.
    pub radius: u8,
    pub meter: crate::theme::MeterStyle,
    pub cap: crate::theme::CapStyle,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self::from_palette(crate::theme::DARK)
    }
}

impl CustomTheme {
    /// Starts from a drawn theme.
    pub fn from_palette(p: crate::theme::Palette) -> Self {
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        Self {
            background: rgb(p.panel),
            accent: rgb(p.accent),
            contrast: 0.5,
            radius: p.look.radius,
            meter: p.look.meter,
            cap: p.look.cap,
        }
    }

    pub fn palette(&self) -> crate::theme::Palette {
        let color = |[r, g, b]: [u8; 3]| egui::Color32::from_rgb(r, g, b);
        crate::theme::custom(
            color(self.background),
            color(self.accent),
            self.contrast,
            crate::theme::look(self.radius.min(10), self.meter, self.cap),
        )
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
    /// The user's own theme, used when `theme` is `Custom`.
    pub custom: CustomTheme,
    /// Interface zoom factor.
    pub scale: f32,
    pub keymap: Keymap,
    /// Panel layout as JSON text, since TOML has no null.
    pub layout: Option<String>,
    /// Panels shown in their own windows.
    #[serde(deserialize_with = "known")]
    pub detached: Vec<crate::layout::Pane>,
    /// The main window as it was left.
    pub window: Option<WindowState>,
    /// The detached panels' windows as they were left.
    #[serde(deserialize_with = "known")]
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
    /// The master strip at the left of the tracks instead of their right.
    pub master_left: bool,
    /// The master's band at the top of the timeline, with its automation.
    pub timeline_master: bool,
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
            custom: CustomTheme::default(),
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
            master_left: false,
            timeline_master: false,
            timeline_tool: crate::timeline::Tool::Smart,
            counter: crate::transport::Counter::default(),
            transport_bottom: false,
            transport_large: false,
            waveforms: crate::timeline::WaveView::default(),
            assistants: false,
            assistants_port: crate::integrations::DEFAULT_PORT,
        }
    }
}

impl UiPrefs {
    pub fn apply(&self, ctx: &egui::Context) {
        crate::i18n::set_language(self.language);
        crate::theme::install_fonts(ctx);
        ctx.set_visuals(self.theme.visuals(&self.custom));
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
            theme: ThemeChoice::Custom,
            ..UiPrefs::default()
        };
        changed.detached.push(crate::layout::Pane::Mixer);
        let text = serde_json::to_string(&changed).unwrap();
        assert_eq!(serde_json::from_str::<UiPrefs>(&text).unwrap(), changed);
    }

    #[test]
    fn themes_of_older_versions_open_as_the_closest_one() {
        let theme =
            |name: &str| serde_json::from_str::<ThemeChoice>(&format!("\"{name}\"")).unwrap();
        assert_eq!(theme("Neon"), ThemeChoice::Dark);
        assert_eq!(theme("Vintage"), ThemeChoice::Studio);
        assert_eq!(theme("Paper"), ThemeChoice::Light);
        let custom = CustomTheme::default();
        assert!(ThemeChoice::Dark.palette(&custom).dark);
        assert!(!ThemeChoice::Light.palette(&custom).dark);
    }

    #[test]
    fn a_custom_theme_stays_readable_whatever_its_colors() {
        use crate::theme::contrast_ratio;
        let backgrounds = [
            [0, 0, 0],
            [128, 128, 128],
            [255, 0, 0],
            [250, 250, 250],
            [90, 60, 200],
        ];
        let accents = [
            [232, 72, 72],
            [255, 255, 0],
            [10, 10, 10],
            [255, 255, 255],
            [0, 120, 255],
        ];
        for background in backgrounds {
            for accent in accents {
                for contrast in [0.0, 1.0] {
                    let theme = CustomTheme {
                        background,
                        accent,
                        contrast,
                        ..CustomTheme::default()
                    };
                    let p = theme.palette();
                    let what = format!("{background:?} {accent:?} {contrast}");
                    assert!(contrast_ratio(p.text, p.panel) >= 7.0, "text {what}");
                    assert!(contrast_ratio(p.text, p.surface) >= 4.5, "buttons {what}");
                    assert!(contrast_ratio(p.weak, p.panel) >= 4.5, "weak {what}");
                    assert!(contrast_ratio(p.accent, p.panel) >= 3.0, "accent {what}");
                    assert_ne!(p.accent, crate::theme::RECORD, "{what}");
                }
            }
        }
    }
}
