//! Editable keyboard shortcuts, with presets modelled on other DAWs.

use eframe::egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};

use crate::actions::Action;

/// X keysym of a key (lowercase for letters), for the keys taken from plugin windows.
pub fn keysym(key: Key) -> Option<u32> {
    let name = key.name();
    if name.len() == 1 {
        let c = name.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(u32::from(c.to_ascii_lowercase()));
        }
    }
    let named = match key {
        Key::Space => 0x20,
        Key::Enter => 0xff0d,
        Key::Escape => 0xff1b,
        Key::Tab => 0xff09,
        Key::Backspace => 0xff08,
        Key::Delete => 0xffff,
        Key::Insert => 0xff63,
        Key::Home => 0xff50,
        Key::End => 0xff57,
        Key::PageUp => 0xff55,
        Key::PageDown => 0xff56,
        Key::ArrowLeft => 0xff51,
        Key::ArrowUp => 0xff52,
        Key::ArrowRight => 0xff53,
        Key::ArrowDown => 0xff54,
        Key::Comma => 0x2c,
        Key::Period => 0x2e,
        Key::Minus => 0x2d,
        Key::Plus => 0x2b,
        Key::Equals => 0x3d,
        Key::Slash => 0x2f,
        Key::Semicolon => 0x3b,
        Key::OpenBracket => 0x5b,
        Key::CloseBracket => 0x5d,
        Key::F1 => 0xffbe,
        Key::F2 => 0xffbf,
        Key::F3 => 0xffc0,
        Key::F4 => 0xffc1,
        Key::F5 => 0xffc2,
        Key::F6 => 0xffc3,
        Key::F7 => 0xffc4,
        Key::F8 => 0xffc5,
        Key::F9 => 0xffc6,
        Key::F10 => 0xffc7,
        Key::F11 => 0xffc8,
        Key::F12 => 0xffc9,
        _ => return None,
    };
    Some(named)
}

/// A key with modifiers. `command` is Ctrl on Linux and Windows, Cmd on macOS.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shortcut {
    #[serde(default)]
    pub command: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    pub key: String,
}

impl Shortcut {
    pub fn new(command: bool, shift: bool, alt: bool, key: Key) -> Self {
        Self {
            command,
            shift,
            alt,
            key: key.name().to_owned(),
        }
    }

    pub fn key(&self) -> Option<Key> {
        Key::from_name(&self.key)
    }

    pub fn matches(&self, key: Key, modifiers: Modifiers) -> bool {
        self.key() == Some(key)
            && modifiers.command == self.command
            && modifiers.shift == self.shift
            && modifiers.alt == self.alt
    }

    /// Human readable form, e.g. "Ctrl+Shift+S" (or "⌘⇧S" on macOS).
    pub fn label(&self, mac: bool) -> String {
        let key = self
            .key()
            .map_or(self.key.clone(), |k| k.symbol_or_name().to_owned());
        let mut parts: Vec<&str> = Vec::new();
        if self.command {
            parts.push(if mac { "⌘" } else { "Ctrl" });
        }
        if self.alt {
            parts.push(if mac { "⌥" } else { "Alt" });
        }
        if self.shift {
            parts.push(if mac { "⇧" } else { "Shift" });
        }
        if mac {
            format!("{}{key}", parts.concat())
        } else {
            parts.push(&key);
            parts.join("+")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    AudioStrada,
    ProTools,
    Logic,
    Reaper,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::AudioStrada,
        Preset::ProTools,
        Preset::Logic,
        Preset::Reaper,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::AudioStrada => "AudioStrada",
            Preset::ProTools => "Pro Tools",
            Preset::Logic => "Logic Pro",
            Preset::Reaper => "Reaper",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    pub preset: Preset,
    pub bindings: Vec<(Shortcut, Action)>,
    /// Defaults version the bindings were made from (0 when saved before versions existed).
    #[serde(default)]
    pub version: u32,
}

/// Bumped when default bindings are added, so saved keymaps get them.
const VERSION: u32 = 11;

impl Default for Keymap {
    fn default() -> Self {
        Self::preset(Preset::AudioStrada)
    }
}

const fn k(command: bool, shift: bool, alt: bool, key: Key) -> (bool, bool, bool, Key) {
    (command, shift, alt, key)
}

/// (command, shift, alt, key) and the bound action.
type Binding = ((bool, bool, bool, Key), Action);

const PLAIN: bool = false;
const CMD: bool = true;

/// Bindings shared by every preset (file, undo, view, palette).
const COMMON: &[Binding] = &[
    (k(CMD, false, false, Key::N), Action::NewProject),
    (k(CMD, false, false, Key::O), Action::OpenProject),
    (k(CMD, false, false, Key::S), Action::Save),
    (k(CMD, true, false, Key::S), Action::SaveAs),
    (k(CMD, false, false, Key::I), Action::ImportAudio),
    (k(CMD, true, false, Key::E), Action::Export),
    (k(CMD, false, false, Key::Q), Action::Quit),
    (k(CMD, false, false, Key::Z), Action::Undo),
    (k(CMD, true, false, Key::Z), Action::Redo),
    (k(CMD, false, false, Key::A), Action::SelectAll),
    (k(CMD, false, false, Key::C), Action::Copy),
    (k(CMD, false, false, Key::X), Action::Cut),
    (k(CMD, false, false, Key::V), Action::Paste),
    (k(PLAIN, false, false, Key::Escape), Action::Deselect),
    (k(PLAIN, false, false, Key::Delete), Action::Delete),
    (k(PLAIN, false, false, Key::Backspace), Action::Delete),
    (k(PLAIN, false, false, Key::Space), Action::PlayPause),
    (k(PLAIN, false, false, Key::ArrowLeft), Action::NudgeLeft),
    (k(PLAIN, false, false, Key::ArrowRight), Action::NudgeRight),
    (k(CMD, true, false, Key::P), Action::CommandPalette),
    (k(CMD, false, false, Key::Comma), Action::Preferences),
    (k(CMD, true, false, Key::T), Action::AddTrack),
    (k(CMD, true, false, Key::N), Action::AddTracks),
    (k(CMD, true, false, Key::G), Action::ToggleGroups),
    (k(CMD, true, false, Key::D), Action::DuplicateTracks),
    (k(CMD, false, false, Key::Delete), Action::DeleteTracks),
    (k(CMD, false, false, Key::Backspace), Action::DeleteTracks),
    (
        k(CMD, false, false, Key::ArrowUp),
        Action::SelectPreviousTrack,
    ),
    (
        k(CMD, false, false, Key::ArrowDown),
        Action::SelectNextTrack,
    ),
    (k(PLAIN, false, true, Key::ArrowUp), Action::MoveTrackUp),
    (k(PLAIN, false, true, Key::ArrowDown), Action::MoveTrackDown),
    (k(PLAIN, true, true, Key::M), Action::MuteTracks),
    (k(PLAIN, true, true, Key::S), Action::SoloTracks),
    (k(PLAIN, true, true, Key::R), Action::ArmTracks),
    (k(CMD, false, false, Key::M), Action::DetachMixer),
    (k(CMD, false, false, Key::P), Action::DetachPianoRoll),
    (k(PLAIN, false, false, Key::F11), Action::ToggleFullscreen),
];

/// Panels: Alt and a number shows or hides them (added in version 1). Vertical zoom (added
/// in version 2): Ctrl+Shift arrows for the track height, Alt+Shift arrows for the waveforms.
const PANELS: &[Binding] = &[
    (k(CMD, false, true, Key::N), Action::NewProjectTab),
    (k(CMD, false, true, Key::O), Action::OpenProjectInTab),
    (k(CMD, false, false, Key::W), Action::CloseTab),
    (k(CMD, false, false, Key::Tab), Action::NextTab),
    (k(CMD, true, false, Key::Tab), Action::PreviousTab),
    (k(PLAIN, false, true, Key::Num1), Action::ShowMixer),
    (k(PLAIN, false, true, Key::Num2), Action::ShowPianoRoll),
    (k(PLAIN, false, true, Key::Num3), Action::ShowBrowser),
    (k(PLAIN, false, true, Key::Num4), Action::ShowInspector),
    (k(PLAIN, false, true, Key::Num5), Action::ShowMarkers),
    // Compact mixer (added in version 11).
    (k(PLAIN, true, true, Key::C), Action::CompactMixer),
    (k(CMD, true, false, Key::ArrowUp), Action::TallerTracks),
    (k(CMD, true, false, Key::ArrowDown), Action::ShorterTracks),
    (k(PLAIN, true, true, Key::ArrowUp), Action::WaveformZoomIn),
    (
        k(PLAIN, true, true, Key::ArrowDown),
        Action::WaveformZoomOut,
    ),
    // Clip processing (added in version 3).
    (k(CMD, false, false, Key::H), Action::Heal),
    (k(CMD, false, false, Key::U), Action::StripSilence),
    (k(CMD, true, false, Key::C), Action::Consolidate),
    // Editing, transport and windows (added in version 7).
    (k(PLAIN, false, false, Key::P), Action::TogglePunch),
    (k(PLAIN, true, false, Key::K), Action::MetronomeSettings),
    (k(PLAIN, false, true, Key::R), Action::ListenToReference),
    (k(PLAIN, false, false, Key::Q), Action::Quantize),
    (k(PLAIN, false, true, Key::G), Action::Groove),
    (k(PLAIN, false, true, Key::B), Action::BeatDetective),
    (k(PLAIN, false, true, Key::P), Action::ProcessClips),
    (k(PLAIN, true, false, Key::R), Action::RangeFromClips),
    (k(CMD, true, false, Key::R), Action::CropToSelection),
    (k(CMD, true, false, Key::Delete), Action::RemoveSection),
    (k(PLAIN, true, false, Key::Delete), Action::ClearSection),
    (k(CMD, true, false, Key::Equals), Action::GridFiner),
    (k(CMD, true, false, Key::Minus), Action::GridCoarser),
    (k(CMD, false, true, Key::T), Action::AddStereoTrack),
    (k(CMD, true, false, Key::I), Action::AddInstrumentTrack),
    (k(CMD, true, false, Key::M), Action::NewMidiClip),
    (k(CMD, false, true, Key::I), Action::ImportMidi),
    (k(PLAIN, false, true, Key::Num0), Action::ShowTimeline),
    (k(PLAIN, true, true, Key::Num0), Action::WaveformZoomReset),
    (k(CMD, false, false, Key::Slash), Action::KeyboardShortcuts),
];

const AUDIOSTRADA: &[Binding] = &[
    (k(CMD, false, false, Key::G), Action::GroupTracks),
    (k(CMD, false, false, Key::Y), Action::Redo),
    (k(PLAIN, false, false, Key::S), Action::Split),
    (k(CMD, false, false, Key::D), Action::Duplicate),
    (k(PLAIN, false, false, Key::R), Action::Record),
    (k(PLAIN, false, false, Key::Home), Action::GoToStart),
    (k(PLAIN, false, false, Key::End), Action::GoToEnd),
    (k(PLAIN, false, false, Key::L), Action::ToggleLoop),
    (k(PLAIN, true, false, Key::L), Action::LoopSelection),
    (k(PLAIN, false, false, Key::K), Action::ToggleMetronome),
    (k(PLAIN, false, false, Key::M), Action::AddMarker),
    (k(PLAIN, false, false, Key::Period), Action::NextMarker),
    (k(PLAIN, false, false, Key::Comma), Action::PreviousMarker),
    (k(PLAIN, false, false, Key::Plus), Action::ZoomIn),
    (k(PLAIN, false, false, Key::Equals), Action::ZoomIn),
    (k(PLAIN, false, false, Key::Minus), Action::ZoomOut),
    (k(PLAIN, false, false, Key::F), Action::ZoomToFit),
    (k(PLAIN, false, false, Key::G), Action::ToggleSnap),
    (k(PLAIN, false, false, Key::F1), Action::ModeShuffle),
    (k(PLAIN, false, false, Key::F2), Action::ModeSlip),
    (k(PLAIN, false, false, Key::F4), Action::ModeGrid),
];

/// Pro Tools: Cmd+E separate, Cmd+D duplicate, Return to start, Cmd+Space record,
/// Cmd+[ / Cmd+] zoom, Enter adds a marker, F1/F2/F4 edit modes.
const PRO_TOOLS: &[Binding] = &[
    (k(CMD, false, false, Key::E), Action::Split),
    (k(CMD, false, false, Key::D), Action::Duplicate),
    (k(CMD, false, false, Key::Space), Action::Record),
    (k(PLAIN, false, false, Key::F12), Action::Record),
    (k(PLAIN, false, false, Key::Enter), Action::GoToStart),
    (k(PLAIN, false, false, Key::End), Action::GoToEnd),
    (k(CMD, true, false, Key::L), Action::ToggleLoop),
    (k(PLAIN, false, false, Key::Insert), Action::AddMarker),
    (k(CMD, false, false, Key::CloseBracket), Action::ZoomIn),
    (k(CMD, false, false, Key::OpenBracket), Action::ZoomOut),
    (k(CMD, false, true, Key::OpenBracket), Action::ZoomToFit),
    (k(PLAIN, false, false, Key::F1), Action::ModeShuffle),
    (k(PLAIN, false, false, Key::F2), Action::ModeSlip),
    (k(PLAIN, false, false, Key::F4), Action::ModeGrid),
];

/// Logic Pro: Cmd+T split at playhead, Cmd+D duplicate, R record, C cycle, Return to start,
/// Alt+' create marker, Cmd+arrows zoom, Z zoom to fit.
const LOGIC: &[Binding] = &[
    (k(CMD, false, false, Key::T), Action::Split),
    (k(CMD, false, false, Key::D), Action::Duplicate),
    (k(PLAIN, false, false, Key::R), Action::Record),
    (k(PLAIN, false, false, Key::Enter), Action::GoToStart),
    (k(PLAIN, false, false, Key::C), Action::ToggleLoop),
    (k(PLAIN, false, false, Key::K), Action::ToggleMetronome),
    (k(PLAIN, false, true, Key::Quote), Action::AddMarker),
    (k(PLAIN, false, true, Key::Period), Action::NextMarker),
    (k(PLAIN, false, true, Key::Comma), Action::PreviousMarker),
    (k(CMD, false, false, Key::ArrowRight), Action::ZoomIn),
    (k(CMD, false, false, Key::ArrowLeft), Action::ZoomOut),
    (k(PLAIN, false, false, Key::Z), Action::ZoomToFit),
    (k(CMD, false, false, Key::G), Action::ToggleSnap),
];

/// Reaper: S split, Ctrl+D duplicate, Ctrl+R record, R loop, M marker, Home/End,
/// +/- zoom, Alt+S snap.
const REAPER: &[Binding] = &[
    (k(CMD, false, false, Key::G), Action::GroupTracks),
    (k(PLAIN, false, false, Key::S), Action::Split),
    (k(CMD, false, false, Key::D), Action::Duplicate),
    (k(CMD, false, false, Key::R), Action::Record),
    (k(PLAIN, false, false, Key::Home), Action::GoToStart),
    (k(PLAIN, false, false, Key::End), Action::GoToEnd),
    (k(PLAIN, false, false, Key::R), Action::ToggleLoop),
    (k(PLAIN, false, false, Key::M), Action::AddMarker),
    (
        k(PLAIN, false, false, Key::CloseBracket),
        Action::NextMarker,
    ),
    (
        k(PLAIN, false, false, Key::OpenBracket),
        Action::PreviousMarker,
    ),
    (k(PLAIN, false, false, Key::Plus), Action::ZoomIn),
    (k(PLAIN, false, false, Key::Equals), Action::ZoomIn),
    (k(PLAIN, false, false, Key::Minus), Action::ZoomOut),
    (k(PLAIN, false, false, Key::Backtick), Action::ZoomToFit),
    (k(PLAIN, false, true, Key::S), Action::ToggleSnap),
];

impl Keymap {
    pub fn preset(preset: Preset) -> Self {
        let specific = match preset {
            Preset::AudioStrada => AUDIOSTRADA,
            Preset::ProTools => PRO_TOOLS,
            Preset::Logic => LOGIC,
            Preset::Reaper => REAPER,
        };
        let bindings = COMMON
            .iter()
            .chain(PANELS)
            .chain(specific)
            .map(|&((command, shift, alt, key), action)| {
                (Shortcut::new(command, shift, alt, key), action)
            })
            .collect();
        Self {
            preset,
            bindings,
            version: VERSION,
        }
    }

    /// Adds the default bindings that appeared since this keymap was saved, where their keys
    /// are still free.
    pub fn upgrade(&mut self) {
        if self.version < 5 {
            // Ctrl+M showed the mixer panel; it now puts the mixer in its window, Ctrl+P the
            // piano roll, wherever these keys are free.
            let ctrl_m = Shortcut::new(true, false, false, Key::M);
            for (shortcut, action) in &mut self.bindings {
                if *shortcut == ctrl_m && *action == Action::ShowMixer {
                    *action = Action::DetachMixer;
                }
            }
            for (key, action) in [
                (Key::M, Action::DetachMixer),
                (Key::P, Action::DetachPianoRoll),
            ] {
                let shortcut = Shortcut::new(true, false, false, key);
                if !self.bindings.iter().any(|(s, _)| *s == shortcut) {
                    self.bindings.push((shortcut, action));
                }
            }
        }
        if self.version < 8 {
            let f11 = Shortcut::new(false, false, false, Key::F11);
            if !self.bindings.iter().any(|(s, _)| *s == f11) {
                self.bindings.push((f11, Action::ToggleFullscreen));
            }
        }
        for (version, key, action) in [
            (9, Key::N, Action::AddTracks),
            (10, Key::G, Action::ToggleGroups),
        ] {
            let shortcut = Shortcut::new(true, true, false, key);
            if self.version < version && !self.bindings.iter().any(|(s, _)| *s == shortcut) {
                self.bindings.push((shortcut, action));
            }
        }
        if self.version < VERSION {
            for &((command, shift, alt, key), action) in PANELS {
                let shortcut = Shortcut::new(command, shift, alt, key);
                if !self.bindings.iter().any(|(s, _)| *s == shortcut) {
                    self.bindings.push((shortcut, action));
                }
            }
        }
        self.version = VERSION;
    }

    /// The transport's shortcuts as X keysyms and modifier masks (Shift 1, Ctrl 4, Alt 8),
    /// for plugin windows to hand them back.
    pub fn transport_keys(&self) -> Vec<(u32, u16)> {
        self.bindings
            .iter()
            .filter(|(_, a)| a.category() == "Transport")
            .filter_map(|(s, _)| {
                let sym = keysym(s.key()?)?;
                let mods = u16::from(s.shift) | u16::from(s.command) << 2 | u16::from(s.alt) << 3;
                Some((sym, mods))
            })
            .collect()
    }

    /// Action bound to a key pressed in a plugin window (X keysym and modifier mask).
    pub fn action_for_keysym(&self, sym: u32, mods: u16) -> Option<Action> {
        let key = Key::ALL.iter().copied().find(|&k| keysym(k) == Some(sym))?;
        let modifiers = Modifiers {
            shift: mods & 1 != 0,
            ctrl: mods & 4 != 0,
            command: mods & 4 != 0,
            alt: mods & 8 != 0,
            ..Modifiers::NONE
        };
        self.action_for(key, modifiers)
    }

    /// Action bound to a key press, if any.
    pub fn action_for(&self, key: Key, modifiers: Modifiers) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(s, _)| s.matches(key, modifiers))
            .map(|(_, a)| *a)
    }

    pub fn shortcuts_for(&self, action: Action) -> impl Iterator<Item = &Shortcut> {
        self.bindings
            .iter()
            .filter(move |(_, a)| *a == action)
            .map(|(s, _)| s)
    }

    /// First shortcut label of an action, for menus.
    pub fn label(&self, action: Action, mac: bool) -> Option<String> {
        self.shortcuts_for(action).next().map(|s| s.label(mac))
    }

    /// Binds `shortcut` to `action`, removing any other action it was bound to. Returns the
    /// action that lost the shortcut.
    pub fn bind(&mut self, shortcut: Shortcut, action: Action) -> Option<Action> {
        let previous = self
            .bindings
            .iter()
            .find(|(s, a)| *s == shortcut && *a != action)
            .map(|(_, a)| *a);
        self.bindings.retain(|(s, _)| *s != shortcut);
        self.bindings.push((shortcut, action));
        previous
    }

    pub fn clear(&mut self, action: Action) {
        self.bindings.retain(|(_, a)| *a != action);
    }

    /// Restores the preset bindings of one action.
    pub fn reset(&mut self, action: Action) {
        self.clear(action);
        for (shortcut, a) in Keymap::preset(self.preset).bindings {
            if a == action {
                self.bind(shortcut, a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(command: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers {
            alt,
            ctrl: command,
            shift,
            mac_cmd: false,
            command,
        }
    }

    #[test]
    fn transport_keys_come_back_from_plugin_windows() {
        let keymap = Keymap::default();
        let keys = keymap.transport_keys();
        assert!(keys.contains(&(0x20, 0)), "Space plays");
        for (sym, mods) in keys {
            let action = keymap.action_for_keysym(sym, mods).unwrap();
            assert_eq!(action.category(), "Transport", "{sym:#x} {mods}");
        }
        assert_eq!(keymap.action_for_keysym(0x20, 0), Some(Action::PlayPause));
    }

    #[test]
    fn ctrl_m_and_ctrl_p_open_the_mixer_and_piano_roll_windows() {
        let ctrl = mods(true, false, false);
        let map = Keymap::preset(Preset::AudioStrada);
        assert_eq!(map.action_for(Key::M, ctrl), Some(Action::DetachMixer));
        assert_eq!(map.action_for(Key::P, ctrl), Some(Action::DetachPianoRoll));
        let mut saved = Keymap::preset(Preset::Reaper);
        saved.bindings.retain(|(s, _)| !s.matches(Key::P, ctrl));
        for (shortcut, action) in &mut saved.bindings {
            if shortcut.matches(Key::M, ctrl) {
                *action = Action::ShowMixer;
            }
        }
        saved.version = 3;
        saved.upgrade();
        assert_eq!(saved.action_for(Key::M, ctrl), Some(Action::DetachMixer));
        // A keymap saved without Ctrl+M gets it too.
        let mut without = Keymap::preset(Preset::AudioStrada);
        without.bindings.retain(|(s, _)| !s.matches(Key::M, ctrl));
        without.version = 4;
        without.upgrade();
        assert_eq!(without.action_for(Key::M, ctrl), Some(Action::DetachMixer));
        assert_eq!(
            saved.action_for(Key::P, ctrl),
            Some(Action::DetachPianoRoll)
        );
    }

    #[test]
    fn saved_keymaps_get_new_defaults_on_free_keys() {
        let mut keymap = Keymap::preset(Preset::AudioStrada);
        keymap
            .bindings
            .retain(|(s, _)| !s.matches(Key::Num1, mods(false, false, true)));
        keymap.bindings.retain(|(_, a)| *a != Action::ShowPianoRoll);
        keymap.bind(Shortcut::new(false, false, true, Key::Num2), Action::Undo);
        keymap.version = 0;
        keymap.upgrade();
        let alt = mods(false, false, true);
        assert_eq!(keymap.action_for(Key::Num1, alt), Some(Action::ShowMixer));
        assert_eq!(keymap.action_for(Key::Num2, alt), Some(Action::Undo));
        assert_eq!(keymap.version, VERSION);
    }

    #[test]
    fn saved_keymaps_get_full_screen_on_f11() {
        let mut keymap = Keymap::preset(Preset::AudioStrada);
        keymap
            .bindings
            .retain(|(_, a)| *a != Action::ToggleFullscreen);
        keymap.version = 7;
        keymap.upgrade();
        let none = mods(false, false, false);
        assert_eq!(
            keymap.action_for(Key::F11, none),
            Some(Action::ToggleFullscreen)
        );
    }

    #[test]
    fn presets_have_no_conflicts() {
        for preset in Preset::ALL {
            let map = Keymap::preset(preset);
            let mut seen = std::collections::HashSet::new();
            for (shortcut, action) in &map.bindings {
                assert!(
                    shortcut.key().is_some(),
                    "{preset:?}: unknown key {}",
                    shortcut.key
                );
                assert!(
                    seen.insert(shortcut.clone()),
                    "{preset:?}: {} bound twice ({action:?})",
                    shortcut.label(false)
                );
            }
        }
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let map = Keymap::preset(Preset::AudioStrada);
        assert_eq!(
            map.action_for(Key::Z, mods(true, false, false)),
            Some(Action::Undo)
        );
        assert_eq!(
            map.action_for(Key::Z, mods(true, true, false)),
            Some(Action::Redo)
        );
        assert_eq!(
            map.action_for(Key::S, mods(false, false, false)),
            Some(Action::Split)
        );
        assert_eq!(map.action_for(Key::S, mods(false, false, true)), None);
    }

    #[test]
    fn presets_differ_where_daws_differ() {
        let split = |p| Keymap::preset(p).label(Action::Split, false);
        assert_eq!(split(Preset::ProTools).as_deref(), Some("Ctrl+E"));
        assert_eq!(split(Preset::Logic).as_deref(), Some("Ctrl+T"));
        assert_eq!(split(Preset::Reaper).as_deref(), Some("S"));
    }

    #[test]
    fn rebinding_steals_and_reset_restores() {
        let mut map = Keymap::preset(Preset::AudioStrada);
        let shortcut = Shortcut::new(false, false, false, Key::S);
        assert_eq!(
            map.bind(shortcut.clone(), Action::AddMarker),
            Some(Action::Split)
        );
        assert_eq!(
            map.action_for(Key::S, mods(false, false, false)),
            Some(Action::AddMarker)
        );
        assert_eq!(map.label(Action::Split, false), None);
        map.reset(Action::Split);
        assert_eq!(
            map.action_for(Key::S, mods(false, false, false)),
            Some(Action::Split)
        );
        assert_eq!(Shortcut::new(true, true, false, Key::S).label(true), "⌘⇧S");
    }

    #[test]
    fn keymap_round_trips_through_json() {
        let map = Keymap::preset(Preset::Reaper);
        let back: Keymap = serde_json::from_str(&serde_json::to_string(&map).unwrap()).unwrap();
        assert_eq!(back, map);
    }
}
