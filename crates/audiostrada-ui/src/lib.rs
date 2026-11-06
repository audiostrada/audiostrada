//! AudioStrada user interface (egui + wgpu).

mod actions;
mod add_tracks;
mod assistants_window;
mod browser;
mod captures;
mod chains;
mod channel_editor;
mod console_ui;
pub mod control;
mod dialogs;
pub mod document;
mod drum_sequencer;
mod drums_editor;
mod effect_browser;
mod effect_editors;
mod export_window;
mod gear;
mod host;
pub mod i18n;
mod inspector;
mod integrations;
mod keymap;
mod layout;
mod license_window;
mod listening;
mod markers;
mod mb_editor;
mod metronome_window;
mod mixer;
mod mod_editor;
mod nam_library;
mod palette;
mod piano_roll;
mod pitch_editor;
mod plots;
mod plugin_categories;
mod preferences;
mod presets;
mod reference_window;
mod rig_editor;
mod screens;
mod settings;
mod synth_editor;
mod tabs;
mod tape_deck;
mod tape_editor;
mod theme;
mod timeline;
mod tone3000;
mod track_icons;
mod track_menu;
mod transport;
mod trigger_editor;
mod ui_prefs;
mod updates;
mod versions_window;
mod widgets;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use audiostrada_core::ProjectPaths;
use audiostrada_core::autosave;
use audiostrada_plugins::scan::Database;
use eframe::egui::{self, Event};

pub use crate::actions::Action;
use crate::browser::Browser;
use crate::document::{AppDirs, Document};
pub use crate::host::{AudioHost, ProcessorFactory};
use crate::i18n::tr;
pub use crate::keymap::{Keymap, Preset};
pub use crate::layout::Pane;
use crate::layout::{PaneContent, Workspace};
use crate::mixer::MixerView;
use crate::palette::CommandPalette;
use crate::preferences::PreferencesWindow;
pub use crate::settings::{AudioSettingsPanel, SettingsChange};
use crate::timeline::TimelineView;
use crate::transport::{EditMode, EditSettings};
pub use crate::ui_prefs::{ThemeChoice, UiPrefs, WindowState};

const REFRESH: Duration = Duration::from_millis(33);
/// Repaint rate at rest (stopped, silent): input still repaints at once.
const IDLE_REFRESH: Duration = Duration::from_millis(250);
/// At rest with a plugin's window open.
const PLUGIN_REFRESH: Duration = Duration::from_millis(50);
const SCREENSHOT_FRAME: u32 = 60;
const MAX_RECENT: usize = 10;
const LAYOUT_SAVE_INTERVAL: Duration = Duration::from_secs(2);
const MIDI_PORTS_INTERVAL: Duration = Duration::from_secs(2);

/// Development aid: with `AUDIOSTRADA_SCREENSHOT=<file.ppm>`, saves the window after a few frames
/// and closes it.
#[derive(Debug)]
struct Screenshot {
    path: PathBuf,
    frames: u32,
    done: Arc<AtomicBool>,
}

impl Screenshot {
    fn from_env() -> Option<Self> {
        std::env::var_os("AUDIOSTRADA_SCREENSHOT").map(|path| Self {
            path: path.into(),
            frames: 0,
            done: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Returns true once the screenshot is saved and the window should close.
    fn tick(&mut self, ctx: &egui::Context) -> bool {
        self.frames += 1;
        if self.frames == SCREENSHOT_FRAME {
            let (path, done) = (self.path.clone(), Arc::clone(&self.done));
            ctx.request_screenshot(move |image| {
                let mut ppm =
                    format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
                ppm.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                if let Err(e) = std::fs::write(&path, ppm) {
                    tracing::error!("cannot write screenshot {}: {e}", path.display());
                }
                done.store(true, Ordering::Release);
            });
        }
        self.done.load(Ordering::Acquire)
    }
}

pub type AudioHook = Box<dyn FnMut(&AudioHost)>;
pub type RecentHook = Box<dyn FnMut(&[PathBuf])>;
pub type UiHook = Box<dyn FnMut(&UiPrefs)>;
/// Starts a plugin scan with extra folders; `true` rescans everything.
pub type PluginScanHook = Box<dyn FnMut(Vec<PathBuf>, bool) -> std::thread::JoinHandle<Database>>;

/// Callbacks into the application, to persist preferences.
pub struct Hooks {
    pub audio_changed: AudioHook,
    pub recent_changed: RecentHook,
    pub ui_changed: UiHook,
    pub plugin_scan: PluginScanHook,
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks").finish_non_exhaustive()
    }
}

/// What to do once a save, waiting for its file dialog, succeeded.
#[derive(Debug)]
enum AfterSave {
    Perform(Pending),
    Archive,
}

/// What to do once unsaved changes have been dealt with.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    New,
    /// A project file, and whether it opens in a new tab.
    Open(PathBuf, bool),
    CloseTab,
    Quit,
}

/// An autosave that can be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOffer {
    pub autosave: PathBuf,
    /// Project file the autosave belongs to; `None` for an untitled project.
    pub project: Option<PathBuf>,
}

impl RecoveryOffer {
    /// Recovery available for `file`, or for an untitled project in `dirs`.
    pub fn find(file: Option<&Path>, dirs: &AppDirs) -> Option<Self> {
        match file {
            Some(file) => autosave::recovery_for(&ProjectPaths::new(file)).map(|autosave| Self {
                autosave,
                project: Some(file.to_path_buf()),
            }),
            None => dirs
                .recovery
                .as_deref()
                .and_then(|dir| autosave::untitled_recoveries(dir).into_iter().next())
                .map(|autosave| Self {
                    autosave,
                    project: None,
                }),
        }
    }
}

/// The panel views, drawn inside the dock tree.
struct Views {
    doc: Rc<RefCell<Document>>,
    browser: Browser,
    mixer: MixerView,
    timeline: TimelineView,
    piano: piano_roll::PianoRoll,
    edit: EditSettings,
}

impl Views {
    /// True when a panel's menus asked the application for something.
    fn has_requests(&self) -> bool {
        !self.timeline.requests.is_empty()
            || !self.mixer.requests.is_empty()
            || self.timeline.stem_request.is_some()
            || self.mixer.stem_request.is_some()
    }
}

impl PaneContent for Views {
    fn pane_ui(&mut self, ui: &mut egui::Ui, pane: Pane) {
        match pane {
            // The mixer scrolls its strips itself; in its own window, or tall enough, it shows
            // its lists.
            Pane::Mixer => {
                self.mixer.detached = ui.ctx().viewport_id() != egui::ViewportId::ROOT;
                self.mixer.show(ui);
            }
            Pane::Timeline => self.timeline.show(ui, &mut self.edit),
            Pane::Browser => self.browser.show(ui, &mut self.doc.borrow_mut()),
            Pane::Inspector => {
                inspector::show(ui, &mut self.doc.borrow_mut(), &self.timeline.selection)
            }
            Pane::Markers => markers::show(ui, &mut self.doc.borrow_mut()),
            Pane::PianoRoll => {
                let clip = self.timeline.midi_clip();
                self.piano.show(ui, &self.edit, clip);
            }
            Pane::Rack => self.mixer.rack(ui),
        }
    }
}

pub struct AudioStradaApp {
    host: AudioHost,
    doc: Rc<RefCell<Document>>,
    dirs: AppDirs,
    /// Shared with the detached panels' windows, which draw on their own.
    views: Rc<RefCell<Views>>,
    /// What the detached windows ask of the application.
    inbox: Rc<RefCell<DetachedInbox>>,
    tree: egui_tiles::Tree<Pane>,
    prefs: UiPrefs,
    edit: EditSettings,
    palette: CommandPalette,
    export: export_window::ExportWindow,
    /// Panels shown in their own windows.
    detached: Vec<Pane>,
    midi_inputs: Option<audiostrada_midi::devices::MidiInputs>,
    /// Where playback last started, to go back there on stop.
    play_start: u64,
    /// Repaint smoothly until then: something moved (playback, sound, armed tracks).
    lively_until: Instant,
    /// The main or a panel's window moved, resized or (un)maximized: saved on exit.
    windows_changed: bool,
    /// Frames left before maximizing the main window again: asked while it is still hidden
    /// (as eframe creates it), X11 window managers ignore it.
    maximize_in: u8,
    /// How each detached panel's window opened, kept while it is open: a builder that
    /// changed would resize the window under the user's hand.
    pane_opened: HashMap<Pane, ui_prefs::WindowState>,
    /// Track selected last frame, to follow a new selection with the panels.
    last_selected: Option<audiostrada_core::TrackId>,
    /// Messages of the document shown in a corner, with when they came.
    toasts: Vec<(String, Instant)>,
    midi_outputs: Option<audiostrada_midi::devices::MidiOutputs>,
    midi_routes: Vec<(String, audiostrada_midi::live::LiveMidi)>,
    /// MIDI ports present at the last check, to notice devices plugged in or out.
    midi_ports: (Instant, Vec<String>, Vec<String>),
    preferences: PreferencesWindow,
    audio_settings: AudioSettingsPanel,
    hooks: Hooks,
    plugin_scan: Option<std::thread::JoinHandle<Database>>,
    recent: Vec<PathBuf>,
    pending: Option<Pending>,
    recovery: Option<RecoveryOffer>,
    /// The project to recover opens in a new tab.
    recovery_tab: bool,
    /// Projects of the tabs in the background; `None` at the current tab, shown by `doc`.
    tabs: Vec<Option<tabs::Parked>>,
    current: usize,
    /// The tabs open at the start of quitting are noted, and those of the last exit reopened.
    tabs_noted: bool,
    tabs_restored: bool,
    error: Option<String>,
    title: String,
    quitting: bool,
    prefs_applied: bool,
    saved_layout: Option<String>,
    layout_checked: Instant,
    mac: bool,
    screenshot: Option<Screenshot>,
    /// The About window is open.
    about: bool,
    after_save: Option<AfterSave>,
    license: audiostrada_license::Status,
    license_window: license_window::LicenseWindow,
    updates: updates::Updates,
    /// Tool calls from `audiostrada mcp --connect`, when started with `--control`.
    control: Option<control::Server>,
    /// The last tools assistants called: (tool, when).
    control_log: std::collections::VecDeque<(String, std::time::SystemTime)>,
    assistants_window: assistants_window::AssistantsWindow,
}

impl std::fmt::Debug for AudioStradaApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioStradaApp")
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

impl AudioStradaApp {
    pub fn new(
        host: AudioHost,
        doc: Rc<RefCell<Document>>,
        dirs: AppDirs,
        prefs: UiPrefs,
        recent: Vec<PathBuf>,
        recovery: Option<RecoveryOffer>,
        hooks: Hooks,
    ) -> Self {
        let mut prefs = prefs;
        if prefs.layout_version < layout::VERSION {
            prefs.layout = None;
            prefs.layout_version = layout::VERSION;
        }
        let mut tree = layout::load(prefs.layout.as_deref());
        let detached = prefs.detached.clone();
        for pane in &detached {
            layout::set_visible(&mut tree, *pane, false);
        }
        let mut hooks = hooks;
        let plugin_scan = audiostrada_plugins::enabled()
            .then(|| (hooks.plugin_scan)(prefs.plugin_dirs.clone(), false));
        // AI assistants switched on in a previous session listen again.
        let control = if prefs.assistants {
            control::listen(prefs.assistants_port)
                .inspect_err(|e| tracing::warn!("AI assistants: {e}"))
                .ok()
        } else {
            None
        };
        Self {
            plugin_scan,
            host,
            views: Rc::new(RefCell::new(Views {
                doc: Rc::clone(&doc),
                browser: Browser::new(),
                mixer: {
                    let mut mixer = MixerView::new(Rc::clone(&doc));
                    mixer.compact = prefs.mixer_compact;
                    mixer
                },
                timeline: {
                    let mut timeline = TimelineView::new(Rc::clone(&doc));
                    timeline.header_width = prefs.header_width;
                    timeline.tool = prefs.timeline_tool;
                    timeline.wave_view = prefs.waveforms;
                    timeline
                },
                piano: piano_roll::PianoRoll::new(Rc::clone(&doc)),
                edit: EditSettings::default(),
            })),
            inbox: Rc::default(),
            doc,
            dirs,
            saved_layout: prefs.layout.clone(),
            tree,
            prefs,
            edit: EditSettings::default(),
            palette: CommandPalette::default(),
            export: export_window::ExportWindow::default(),
            detached,
            midi_inputs: None,
            play_start: 0,
            lively_until: Instant::now(),
            windows_changed: false,
            maximize_in: 0,
            pane_opened: HashMap::new(),
            last_selected: None,
            toasts: Vec::new(),
            midi_outputs: None,
            midi_routes: Vec::new(),
            midi_ports: (
                Instant::now(),
                audiostrada_midi::devices::input_names(),
                audiostrada_midi::devices::output_names(),
            ),
            preferences: PreferencesWindow::default(),
            audio_settings: AudioSettingsPanel::default(),
            hooks,
            recent,
            pending: None,
            recovery,
            recovery_tab: false,
            tabs: vec![None],
            current: 0,
            tabs_noted: false,
            tabs_restored: false,
            error: None,
            title: String::new(),
            quitting: false,
            prefs_applied: false,
            layout_checked: Instant::now(),
            mac: cfg!(target_os = "macos"),
            screenshot: Screenshot::from_env(),
            about: false,
            after_save: None,
            license: {
                let status = audiostrada_license::status(audiostrada_license::load());
                audiostrada_engine::evaluation::set_active(status.evaluation());
                status
            },
            license_window: license_window::LicenseWindow::default(),
            updates: updates::Updates::default(),
            control,
            control_log: Default::default(),
            assistants_window: Default::default(),
        }
    }

    fn remember(&mut self, file: &Path) {
        self.recent.retain(|p| p != file);
        self.recent.insert(0, file.to_path_buf());
        self.recent.truncate(MAX_RECENT);
        (self.hooks.recent_changed)(&self.recent);
    }

    /// Swaps in another document and restarts the stream on it.
    /// (Re)opens the enabled MIDI inputs, feeding the current document's engine.
    fn open_midi_inputs(&mut self) {
        self.midi_inputs = None;
        let doc = self.doc.borrow();
        let shared = std::sync::Arc::clone(doc.player().engine().shared());
        let disabled = self.prefs.midi_disabled.clone();
        let (inputs, errors) = audiostrada_midi::devices::MidiInputs::open(
            move |name| !disabled.iter().any(|d| d == name),
            &doc.player().live_midi(),
            move || shared.position(),
        );
        drop(doc);
        for e in errors {
            tracing::warn!("MIDI input: {e}");
        }
        self.preferences.set_midi_open(inputs.names());
        self.midi_inputs = Some(inputs);
    }

    /// Reports audio and MIDI devices plugged in or out, reopening MIDI inputs and retrying
    /// MIDI outputs when they change.
    fn watch_devices(&mut self) {
        if let Some(changes) = self.host.poll_devices() {
            let mut doc = self.doc.borrow_mut();
            for name in changes.added {
                doc.notify(trf!("Audio device connected: {name}", name = name));
            }
            for name in changes.removed {
                doc.notify(trf!("Audio device disconnected: {name}", name = name));
            }
            if changes.restarted {
                doc.notify(tr("Audio restarted").into());
            }
        }
        if self.midi_ports.0.elapsed() < MIDI_PORTS_INTERVAL {
            return;
        }
        let inputs = audiostrada_midi::devices::input_names();
        let outputs = audiostrada_midi::devices::output_names();
        let (_, old_inputs, old_outputs) = &self.midi_ports;
        let mut messages = Vec::new();
        for (now, before) in [(&inputs, old_inputs), (&outputs, old_outputs)] {
            messages.extend(
                now.iter()
                    .filter(|p| !before.contains(p))
                    .map(|p| trf!("MIDI device connected: {name}", name = p)),
            );
            messages.extend(
                before
                    .iter()
                    .filter(|p| !now.contains(p))
                    .map(|p| trf!("MIDI device disconnected: {name}", name = p)),
            );
        }
        let inputs_changed = inputs != *old_inputs;
        let outputs_changed = outputs != *old_outputs;
        self.midi_ports = (Instant::now(), inputs, outputs);
        if !messages.is_empty() {
            let mut doc = self.doc.borrow_mut();
            // Inputs and outputs of one device usually share a name.
            let mut seen = std::collections::HashSet::new();
            messages.retain(|m| seen.insert(m.clone()));
            for m in messages {
                doc.notify(m);
            }
        }
        if inputs_changed {
            self.open_midi_inputs();
        }
        if outputs_changed {
            self.midi_routes.clear();
        }
    }

    /// Points the MIDI output thread at the tracks playing external instruments.
    fn route_midi_outputs(&mut self) {
        let routes: Vec<(String, audiostrada_midi::live::LiveMidi)> = self
            .doc
            .borrow()
            .player()
            .midi_outputs()
            .into_iter()
            .map(|(_, port, out)| (port, out))
            .collect();
        let unchanged = routes.len() == self.midi_routes.len()
            && routes
                .iter()
                .zip(&self.midi_routes)
                .all(|(a, b)| a.0 == b.0 && a.1.same(&b.1));
        if unchanged {
            return;
        }
        let outputs = self
            .midi_outputs
            .get_or_insert_with(audiostrada_midi::devices::MidiOutputs::start);
        outputs.set_routes(
            routes
                .iter()
                .map(|(port, out)| (port.clone(), out.reader()))
                .collect(),
        );
        self.midi_routes = routes;
    }

    fn replace(&mut self, doc: Document) {
        self.host.stop();
        let mut doc = doc;
        doc.set_plugins(self.doc.borrow().player().plugins().clone());
        *self.doc.borrow_mut() = doc;
        {
            let mut views = self.views.borrow_mut();
            views.mixer.reset();
            views.timeline.reset();
            views.piano.reset();
        }
        self.restart();
    }

    /// Restarts the stream on the current document, after it changed.
    fn restart(&mut self) {
        let config = self.host.config().clone();
        self.host.apply(config);
        if self.prefs_applied {
            self.open_midi_inputs();
        }
        let auto = self.prefs.auto_monitoring;
        self.doc.borrow_mut().player_mut().set_auto_monitoring(auto);
        let file = self.doc.borrow().paths().map(|p| p.file.clone());
        if let Some(file) = file {
            self.remember(&file);
        }
    }

    /// Runs `pending`, asking first if the current project has unsaved changes.
    fn request(&mut self, ctx: &egui::Context, pending: Pending) {
        if self.doc.borrow().history().is_dirty() {
            self.pending = Some(pending);
        } else {
            self.perform(ctx, pending);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, pending: Pending) {
        match pending {
            Pending::New => {
                self.doc.borrow_mut().discard_autosave();
                self.replace(Document::new(self.dirs.clone()));
            }
            Pending::Open(file, tab) => {
                if let Some(offer) = RecoveryOffer::find(Some(&file), &self.dirs) {
                    self.recovery = Some(offer);
                    self.recovery_tab = tab;
                    return;
                }
                match Document::open(&file, self.dirs.clone()) {
                    Ok(doc) => self.show(doc, tab),
                    Err(e) => {
                        self.error = Some(trf!(
                            "Cannot open {file}: {error}",
                            file = file.display(),
                            error = e
                        ))
                    }
                }
            }
            Pending::CloseTab => self.close_current_tab(),
            Pending::Quit => {
                // Each project with changes asks in its turn.
                if self.quit_next() {
                    self.request(ctx, Pending::Quit);
                    return;
                }
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Shows an opened document in a new tab or in place of the current one.
    fn show(&mut self, doc: Document, tab: bool) {
        if tab {
            self.add_tab(doc);
        } else {
            self.doc.borrow_mut().discard_autosave();
            self.replace(doc);
        }
    }

    /// Opens a project file, showing its tab when it is already open.
    fn open(&mut self, ctx: &egui::Context, file: PathBuf, tab: bool) {
        if let Some(i) = self.tab_of(&file) {
            self.switch_tab(i);
        } else if tab {
            self.perform(ctx, Pending::Open(file, true));
        } else {
            self.request(ctx, Pending::Open(file, false));
        }
    }

    /// Starts quitting, noting the open tabs first.
    fn quit(&mut self, ctx: &egui::Context) {
        if !self.tabs_noted {
            self.remember_tabs();
            self.tabs_noted = true;
        }
        self.request(ctx, Pending::Quit);
    }

    /// Saves, asking for a location if the project has none: then the save happens once the
    /// dialog is answered, and `None` is returned. Otherwise true on success.
    fn save(&mut self, ctx: &egui::Context, choose: bool) -> Option<bool> {
        if self.doc.borrow().paths().is_none() || choose {
            let name = self.doc.borrow().project().name.clone();
            dialogs::start(ctx, "save-project", move || dialogs::save_project(&name));
            return None;
        }
        let result = self.doc.borrow_mut().save();
        Some(self.saved(result))
    }

    fn saved(&mut self, result: Result<(), audiostrada_core::ProjectError>) -> bool {
        match result {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(trf!("Cannot save: {error}", error = e));
                false
            }
        }
    }

    /// Saves, then does `then` if the save succeeded.
    fn save_then(&mut self, ctx: &egui::Context, then: AfterSave) {
        self.after_save = Some(then);
        if let Some(saved) = self.save(ctx, false) {
            self.finish_save(ctx, saved);
        }
    }

    fn finish_save(&mut self, ctx: &egui::Context, saved: bool) {
        match self.after_save.take() {
            Some(AfterSave::Perform(pending)) if saved => self.perform(ctx, pending),
            Some(AfterSave::Archive) if saved => self.run(ctx, Action::ArchiveProject),
            _ => self.tabs_noted = false,
        }
    }

    /// Acts on the file dialogs opened by commands, once they are answered.
    fn file_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(paths) = dialogs::answer::<ProjectPaths>(ctx, "save-project") {
            let saved = paths.is_some_and(|paths| {
                let file = paths.file.clone();
                let result = self.doc.borrow_mut().save_as(paths);
                let saved = self.saved(result);
                if saved {
                    self.remember(&file);
                }
                saved
            });
            self.finish_save(ctx, saved);
        }
        if let Some(Some(file)) = dialogs::answer(ctx, "open-project") {
            self.open(ctx, file, false);
        }
        if let Some(Some(file)) = dialogs::answer(ctx, "open-project-tab") {
            self.open(ctx, file, true);
        }
        if let Some(Some(mut target)) = dialogs::answer::<PathBuf>(ctx, "archive") {
            if !target.to_string_lossy().ends_with(".tar.gz") {
                target.set_extension("tar.gz");
            }
            let mut doc = self.doc.borrow_mut();
            if let Err(e) = doc.archive(target) {
                doc.notify(trf!("Cannot archive the project: {error}", error = e));
            }
        }
        if let Some(Some(files)) = dialogs::answer::<Vec<PathBuf>>(ctx, "import-audio") {
            // As in Reaper: onto the selected track at the playhead, else new tracks.
            let mut doc = self.doc.borrow_mut();
            let track = doc.focused_track().filter(|&t| {
                doc.project()
                    .track(t)
                    .is_some_and(|t| t.kind == audiostrada_core::TrackKind::Audio)
            });
            match track {
                Some(track) => {
                    let at = doc.player().position();
                    doc.import_at(&files, Some(track), at);
                }
                None => doc.import(&files),
            }
        }
        if let Some(Some(path)) = dialogs::answer::<PathBuf>(ctx, "import-midi") {
            let result = self.doc.borrow_mut().import_midi(&path);
            if let Err(e) = result {
                self.error = Some(trf!(
                    "Cannot import {file}: {error}",
                    file = path.display(),
                    error = e
                ));
            }
        }
        if let Some(Some(path)) = dialogs::answer::<PathBuf>(ctx, "export-midi") {
            let track = self.doc.borrow().focused_track();
            let result = self.doc.borrow().export_midi(&path, track);
            if let Err(e) = result {
                self.error = Some(trf!("Cannot export MIDI: {error}", error = e));
            }
        }
        if let Some(Some((track, path))) =
            dialogs::answer::<(audiostrada_core::TrackId, PathBuf)>(ctx, "export-track")
        {
            let settings = self.prefs.export.clone();
            let result = self.doc.borrow_mut().export_track(track, settings, &path);
            if let Err(e) = result {
                self.error = Some(trf!("Cannot export: {error}", error = e));
            }
        }
    }

    /// Official builds ask once whether to look for new versions.
    fn ask_for_updates(&mut self, ctx: &egui::Context) {
        if !audiostrada_license::OFFICIAL || self.prefs.check_updates.is_some() {
            return;
        }
        let mut answer = None;
        egui::Modal::new(egui::Id::unique("ask-updates")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(tr("New versions"));
            ui.add_space(4.0);
            ui.label(tr("AudioStrada can check once a day whether a new version is out. It only downloads a short version file from the web site: nothing about you or your projects is sent."));
            theme::footer(ui, |ui| {
                if theme::primary_button(ui, tr("Check once a day")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Never")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if let Some(answer) = answer {
            self.prefs.check_updates = Some(answer);
            self.ui_changed();
        }
    }

    fn ui_changed(&mut self) {
        self.prefs.layout = layout::save(&self.tree);
        self.saved_layout = self.prefs.layout.clone();
        self.prefs_applied = false;
        (self.hooks.ui_changed)(&self.prefs);
    }

    /// Opens the control channel on `127.0.0.1:port` for `audiostrada mcp --connect`.
    pub fn listen(&mut self, port: u16) -> Result<(), String> {
        if self.control.as_ref().is_some_and(|c| c.port == port) {
            return Ok(());
        }
        self.control = None;
        self.control = Some(control::listen(port)?);
        Ok(())
    }

    /// Closes the control channel: assistants can no longer reach this window.
    pub fn stop_listening(&mut self) {
        self.control = None;
    }

    /// Answers the tool calls of the control channel, here on the UI thread.
    fn answer_control(&mut self, ctx: &egui::Context) {
        let Some(server) = &self.control else {
            return;
        };
        let requests: Vec<control::Request> = server.requests.try_iter().collect();
        for request in requests {
            self.control_log
                .push_front((request.tool.clone(), std::time::SystemTime::now()));
            self.control_log.truncate(8);
            let answer = match request.tool.as_str() {
                "list_actions" => Ok(control::actions()),
                "run_action" => {
                    let name = request
                        .args
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    match Action::ALL.iter().find(|a| {
                        a.name().eq_ignore_ascii_case(name.trim_end_matches('…'))
                            || a.name().eq_ignore_ascii_case(name)
                    }) {
                        Some(&action) => {
                            self.run(ctx, action);
                            Ok(serde_json::json!("ok"))
                        }
                        None => Err(format!("no action {name} (see list_actions)")),
                    }
                }
                tool => control::call_tool(&mut self.doc.borrow_mut(), true, tool, &request.args),
            };
            let _ = request.reply.send(answer);
        }
    }

    /// Runs a user command, whatever triggered it (menu, shortcut, palette, button).
    fn run(&mut self, ctx: &egui::Context, action: Action) {
        // Something on the system clipboard, so that Ctrl+V reaches the application.
        if matches!(action, Action::Copy | Action::Cut) {
            ctx.copy_text(tr("AudioStrada clips").to_owned());
        }
        match action {
            Action::NewProject => self.request(ctx, Pending::New),
            Action::OpenProject => dialogs::start(ctx, "open-project", dialogs::pick_project),
            Action::NewProjectTab => self.add_tab(Document::new(self.dirs.clone())),
            Action::OpenProjectInTab => {
                dialogs::start(ctx, "open-project-tab", dialogs::pick_project);
            }
            Action::CloseTab => self.close_tab(ctx, self.current),
            Action::NextTab => self.cycle_tab(1),
            Action::PreviousTab => self.cycle_tab(-1),
            Action::Save => {
                self.save(ctx, false);
            }
            Action::SaveAs => {
                self.save(ctx, true);
            }
            Action::ProjectVersions => versions_window::open(ctx),
            Action::CollectMedia => {
                let mut doc = self.doc.borrow_mut();
                let message = match doc.collect_media() {
                    Ok(0) => tr("Everything the project uses is already in its folder").to_owned(),
                    Ok(n) => trf!("{count} file(s) copied into the project folder", count = n),
                    Err(e) => trf!("Cannot collect the media: {error}", error = e),
                };
                doc.notify(message);
            }
            Action::ArchiveProject => {
                // A project never saved is saved first: the archive is made from its folder.
                if self.doc.borrow().paths().is_none() {
                    self.save_then(ctx, AfterSave::Archive);
                    return;
                }
                let name = self.doc.borrow().title().trim_end_matches(" *").to_owned();
                dialogs::start(ctx, "archive", move || dialogs::save_archive(&name));
            }
            Action::ImportAudio => dialogs::start(ctx, "import-audio", dialogs::pick_audio_files),
            Action::Export => self.export.open(),
            Action::ImportMidi => dialogs::start(ctx, "import-midi", dialogs::pick_midi_file),
            Action::ExportMidi => {
                let name = self.doc.borrow().project().name.clone();
                dialogs::start(ctx, "export-midi", move || dialogs::save_midi_file(&name));
            }
            Action::Quit => self.quit(ctx),
            Action::Undo => {
                self.doc.borrow_mut().history_mut().undo();
            }
            Action::Redo => {
                self.doc.borrow_mut().history_mut().redo();
            }
            Action::PlayPause => {
                let mut doc = self.doc.borrow_mut();
                if doc.player().engine().playing() {
                    doc.player_mut().engine_mut().stop();
                    if self.prefs.return_on_stop && doc.recording().is_none() {
                        doc.player_mut().locate(self.play_start);
                    }
                } else {
                    self.play_start = doc.player().position();
                    doc.player_mut().engine_mut().play();
                }
            }
            Action::Stop => {
                let mut doc = self.doc.borrow_mut();
                // Effects ring out after a stop; stopping again silences them.
                if doc.player().engine().playing() {
                    doc.player_mut().engine_mut().stop();
                } else {
                    doc.player_mut().engine_mut().flush();
                }
                doc.player_mut().locate(0);
            }
            Action::Panic => {
                self.doc.borrow_mut().player_mut().panic();
                self.views.borrow_mut().piano.release_preview();
            }
            Action::GoToStart => self.doc.borrow_mut().player_mut().locate(0),
            Action::GoToEnd => {
                let mut doc = self.doc.borrow_mut();
                let end = doc.project().length();
                doc.player_mut().locate(end);
            }
            Action::ToggleMetronome => {
                self.doc
                    .borrow_mut()
                    .set_metronome(|m| m.enabled = !m.enabled);
            }
            Action::MetronomeSettings => crate::metronome_window::open(ctx),
            Action::ListenToReference => reference_window::toggle(ctx, &mut self.doc.borrow_mut()),
            Action::LowLatency => {
                let mut doc = self.doc.borrow_mut();
                let on = !doc.player().low_latency();
                doc.player_mut().set_low_latency(on);
                doc.notify(if on {
                    tr(
                        "Low latency mode: effects adding more than 1 ms are bypassed, except on \
                        the tracks being played",
                    )
                    .into()
                } else {
                    tr("Low latency mode off").into()
                });
            }
            Action::ReferenceTrack => reference_window::open(ctx),
            Action::ToggleLoop => {
                // On over the loop kept, else the selection, else the whole project.
                let selection = self
                    .views
                    .borrow()
                    .timeline
                    .range
                    .as_ref()
                    .map(|r| (r.start, r.end));
                let mut doc = self.doc.borrow_mut();
                let project = doc.project();
                let on = project.looping().is_none();
                let range = project
                    .loop_range
                    .or(selection)
                    .unwrap_or((0, project.length().max(1)));
                doc.history_mut()
                    .edit(if on { tr("Loop on") } else { tr("Loop off") }, |p| {
                        p.loop_range = Some(range);
                        p.loop_off = !on;
                    });
            }
            Action::KeyboardShortcuts => self.preferences.open_keyboard(),
            Action::About => self.about = true,
            Action::License => self.license_window.open(),
            Action::Assistants => self.assistants_window.open(),
            Action::LoopFollowsSelection => {
                let mut doc = self.doc.borrow_mut();
                let fixed = !doc.project().loop_fixed;
                doc.history_mut()
                    .edit(Action::LoopFollowsSelection.label(), |p| {
                        p.loop_fixed = fixed
                    });
            }
            Action::AddVca => mixer::add_vca(&mut self.doc.borrow_mut()),
            Action::GroupTracks => mixer::group_selection(&mut self.doc.borrow_mut()),
            Action::ToggleGroups => {
                let mut doc = self.doc.borrow_mut();
                let off = !doc.project().groups_off;
                doc.history_mut().modify_unrecorded(|p| p.groups_off = off);
                doc.notify(if off {
                    tr("Groups off: every track behaves on its own").into()
                } else {
                    tr("Groups on").into()
                });
            }
            Action::DuplicateTracks => {
                let mut doc = self.doc.borrow_mut();
                let tracks = doc.selected_tracks().to_vec();
                let mut copies = Vec::new();
                doc.history_mut().edit(tr("Duplicate track"), |p| {
                    for &t in &tracks {
                        copies.extend(audiostrada_core::edit::duplicate_track(p, t));
                    }
                });
                if !copies.is_empty() {
                    doc.set_track_selection(copies);
                }
            }
            Action::DeleteTracks => {
                let mut doc = self.doc.borrow_mut();
                let tracks = doc.selected_tracks().to_vec();
                if !tracks.is_empty() {
                    doc.history_mut().edit(tr("Delete track"), |p| {
                        for &t in &tracks {
                            p.remove_track(t);
                        }
                        p.prune_sources();
                    });
                }
            }
            Action::SelectPreviousTrack | Action::SelectNextTrack => {
                let mut doc = self.doc.borrow_mut();
                let order: Vec<audiostrada_core::TrackId> =
                    audiostrada_core::routing::visible_tracks(doc.project())
                        .into_iter()
                        .map(|(i, _)| doc.project().tracks[i].id)
                        .collect();
                let current = doc
                    .focused_track()
                    .and_then(|f| order.iter().position(|&t| t == f));
                let next = match (current, action == Action::SelectNextTrack) {
                    (Some(i), true) => (i + 1).min(order.len().saturating_sub(1)),
                    (Some(i), false) => i.saturating_sub(1),
                    (None, _) => 0,
                };
                if let Some(&t) = order.get(next) {
                    doc.select_track(t, document::SelectMode::Only);
                }
            }
            Action::MuteTracks | Action::SoloTracks | Action::ArmTracks => {
                let mut doc = self.doc.borrow_mut();
                let tracks = doc.selected_tracks().to_vec();
                let Some(&first) = tracks.first() else {
                    return;
                };
                let Some(track) = doc.project().track(first) else {
                    return;
                };
                let (link, on, label) = match action {
                    Action::MuteTracks => (
                        audiostrada_core::groups::Link::Mute,
                        !track.mute,
                        tr("Mute"),
                    ),
                    Action::SoloTracks => (
                        audiostrada_core::groups::Link::Solo,
                        !track.solo,
                        tr("Solo"),
                    ),
                    _ => (audiostrada_core::groups::Link::Arm, !track.armed, "Arm"),
                };
                let apply = |p: &mut audiostrada_core::Project| {
                    for &t in &tracks {
                        audiostrada_core::groups::set_switch(p, t, link, on);
                    }
                };
                // Arming is not an edit of the song, as with the R buttons.
                if link == audiostrada_core::groups::Link::Arm {
                    doc.history_mut().modify_unrecorded(apply);
                } else {
                    doc.history_mut().edit(label, apply);
                }
            }
            Action::MoveTrackUp | Action::MoveTrackDown => {
                let focused = self.doc.borrow().focused_track();
                if let Some(track) = focused {
                    let up = action == Action::MoveTrackUp;
                    self.doc
                        .borrow_mut()
                        .history_mut()
                        .edit(tr("Move track"), |p| {
                            audiostrada_core::routing::move_track(p, track, up)
                        });
                }
            }
            Action::AddInstrumentTrack | Action::AddDrumTrack => {
                let mut doc = self.doc.borrow_mut();
                let n = doc.project().tracks.len() + 1;
                let (effect, label, name) = if action == Action::AddDrumTrack {
                    (
                        "drums",
                        tr("Add drum machine track"),
                        trf!("Drums {n}", n = n),
                    )
                } else {
                    (
                        "synth",
                        tr("Add instrument track"),
                        trf!("Instrument {n}", n = n),
                    )
                };
                let mut created = None;
                let at = add_tracks::below_selection(&doc);
                doc.history_mut().edit(label, |p| {
                    let t = p.add_track(&name);
                    audiostrada_core::routing::add_insert(
                        p,
                        audiostrada_core::routing::Channel::Track(t),
                        effect,
                        Default::default(),
                    );
                    add_tracks::place(p, &[t], at);
                    created = Some(t);
                });
                if let Some(t) = created {
                    doc.select_track(t, document::SelectMode::Only);
                }
            }
            Action::NewMidiClip => {
                self.views.borrow_mut().timeline.new_midi_clip(None);
            }
            Action::Quantize => self.views.borrow_mut().piano.quantize(&self.edit),
            Action::AddTrack | Action::AddStereoTrack => {
                let mut doc = self.doc.borrow_mut();
                let at = add_tracks::below_selection(&doc);
                add_tracks::add_one(&mut doc, action == Action::AddStereoTrack, at);
            }
            Action::AddTracks => {
                let at = add_tracks::below_selection(&self.doc.borrow());
                add_tracks::open(ctx, at);
            }
            Action::ModeSlip => self.edit.mode = EditMode::Slip,
            Action::ModeShuffle => self.edit.mode = EditMode::Shuffle,
            Action::ModeGrid => self.edit.mode = EditMode::Grid,
            Action::ToggleSnap => self.edit.snap = !self.edit.snap,
            Action::GridFiner => self.edit.grid = self.edit.grid.finer(),
            Action::GridCoarser => self.edit.grid = self.edit.grid.coarser(),
            Action::ShowTimeline
            | Action::ShowMixer
            | Action::ShowBrowser
            | Action::ShowInspector
            | Action::ShowMarkers
            | Action::ShowPianoRoll
            | Action::ShowRack => {
                if let Some(pane) = Pane::from_action(action) {
                    if self.detached.contains(&pane) {
                        // Brings its window forward; Detach or closing the window docks it back.
                        let window = pane_window(pane);
                        ctx.send_viewport_cmd_to(window, egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd_to(window, egui::ViewportCommand::Focus);
                    } else {
                        layout::toggle(&mut self.tree, pane);
                        self.ui_changed();
                    }
                }
            }
            Action::RevealRack => self.reveal(ctx, Pane::Rack),
            Action::CompactMixer => {
                let compact = {
                    let mut views = self.views.borrow_mut();
                    views.mixer.compact = !views.mixer.compact;
                    views.mixer.compact
                };
                self.prefs.mixer_compact = compact;
                self.ui_changed();
            }
            Action::DetachMixer | Action::DetachPianoRoll => {
                let pane = if action == Action::DetachMixer {
                    Pane::Mixer
                } else {
                    Pane::PianoRoll
                };
                if self.detached.contains(&pane) {
                    self.reattach(pane);
                } else {
                    self.detach(pane);
                }
            }
            Action::ResetLayout => {
                self.tree = layout::default_tree();
                self.detached.clear();
                self.prefs.detached.clear();
                self.ui_changed();
            }
            Action::ToggleFullscreen => {
                // Follows the main window; detached panels (mixer, piano roll) go with it, each
                // on its own screen.
                let on = !fullscreen(ctx);
                let windows = std::iter::once(egui::ViewportId::ROOT)
                    .chain(self.detached.iter().map(|&pane| pane_window(pane)));
                for window in windows {
                    ctx.send_viewport_cmd_to(window, screens::fullscreen(ctx, window, on));
                }
            }
            Action::CommandPalette => self.palette.open(),
            Action::Preferences => self.preferences.open(),
            Action::Record => {
                let inputs = self.host.stream().map_or(0, |s| s.info().inputs as usize);
                let mut doc = self.doc.borrow_mut();
                if doc.recording().is_some() {
                    doc.stop_recording();
                } else if let Err(e) = doc.start_recording(inputs, self.prefs.record_offset) {
                    self.error = Some(e);
                }
            }
            Action::TogglePunch => {
                let mut doc = self.doc.borrow_mut();
                let project = doc.project();
                let range = match (project.punch_range, project.loop_range) {
                    (Some(_), _) => None,
                    (None, Some(range)) => Some(range),
                    (None, None) => {
                        let (tempo, rate) = (&project.tempo, project.sample_rate);
                        let from = doc.player().position();
                        let beat = tempo.sample_to_beat(from, rate);
                        let bars = 4.0 * tempo.signature_at_beat(beat).beats_per_bar();
                        Some((from, tempo.beat_to_sample(beat + bars, rate)))
                    }
                };
                let label = if range.is_some() {
                    tr("Punch on")
                } else {
                    tr("Punch off")
                };
                doc.history_mut().edit(label, |p| p.punch_range = range);
            }
            Action::Delete
                if self.views.borrow().piano.focused
                    && self.views.borrow_mut().piano.delete_selected() => {}
            Action::SelectAll if self.views.borrow().piano.focused => {
                self.views.borrow_mut().piano.select_all()
            }
            Action::Copy
                if self.views.borrow().piano.focused && self.views.borrow_mut().piano.copy() => {}
            Action::Cut
                if self.views.borrow().piano.focused && self.views.borrow_mut().piano.cut() => {}
            Action::Paste if self.views.borrow().piano.focused => {
                let at = self.doc.borrow().player().position();
                self.views.borrow_mut().piano.paste(&self.edit, at);
            }
            Action::Duplicate
                if self.views.borrow().piano.focused
                    && self.views.borrow_mut().piano.duplicate() => {}
            Action::NudgeLeft | Action::NudgeRight
                if self.views.borrow().piano.focused
                    && self
                        .views
                        .borrow_mut()
                        .piano
                        .nudge(&self.edit, action == Action::NudgeRight) => {}
            Action::SelectAll
            | Action::Deselect
            | Action::Copy
            | Action::Cut
            | Action::Paste
            | Action::Delete
            | Action::Split
            | Action::Duplicate
            | Action::NudgeLeft
            | Action::NudgeRight
            | Action::LoopSelection
            | Action::AddMarker
            | Action::NextMarker
            | Action::PreviousMarker
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::ZoomToFit
            | Action::ZoomToSelection
            | Action::Fades
            | Action::NextTransient
            | Action::PreviousTransient
            | Action::ToolSmart
            | Action::ToolRange
            | Action::ToolSplit
            | Action::ToolErase
            | Action::ToolMute
            | Action::TallerTracks
            | Action::ShorterTracks
            | Action::Heal
            | Action::Consolidate
            | Action::StripSilence
            | Action::ProcessClips
            | Action::BeatDetective
            | Action::Groove
            | Action::CropToSelection
            | Action::RangeFromClips
            | Action::RangeFromLoop
            | Action::RangeAllTracks
            | Action::RemoveSection
            | Action::ClearSection
            | Action::WaveformZoomIn
            | Action::WaveformZoomOut
            | Action::WaveformZoomReset
            | Action::NewPlaylist
            | Action::NextPlaylist => {
                self.views.borrow_mut().timeline.run(action, &self.edit);
            }
        }
    }

    /// Actions bound to the keys pressed this frame.
    fn pressed_actions(&self, ctx: &egui::Context) -> Vec<Action> {
        if self.palette.is_open() || self.preferences.capturing_keys() {
            return Vec::new();
        }
        pressed_actions(ctx, &self.prefs.keymap)
    }

    /// Asks where to write one track and exports it with the last export settings.
    fn export_track(&mut self, ctx: &egui::Context, track: audiostrada_core::TrackId) {
        let name = {
            let doc = self.doc.borrow();
            let project = doc.project();
            let track = project.track(track).map_or("Track", |t| t.name.as_str());
            format!("{} - {track}", project.name)
        };
        let extension = self.prefs.export.format.extension();
        dialogs::start(ctx, "export-track", move || {
            dialogs::save_audio_file(&name, extension).map(|path| (track, path))
        });
    }

    /// Moves a panel into its own window, e.g. the console on a second screen.
    fn detach(&mut self, pane: Pane) {
        if !self.detached.contains(&pane) {
            layout::set_visible(&mut self.tree, pane, false);
            self.detached.push(pane);
            self.prefs.detached = self.detached.clone();
            self.ui_changed();
        }
    }

    /// Puts a detached panel back in the main window.
    /// Selects a clip a window asked for and shows it in the piano roll.
    fn reveal_clip(&mut self, ctx: &egui::Context) {
        let Some(id) = self.doc.borrow_mut().take_revealed_clip() else {
            return;
        };
        self.views.borrow_mut().timeline.selection = vec![id];
        self.reveal(ctx, Pane::PianoRoll);
    }

    /// With the mixer in its own window, a newly selected track brings its panel forward among
    /// the docked tabs: the rack for an audio track, the piano roll for an instrument.
    fn follow_selection(&mut self) {
        let selected = self.doc.borrow().selected_tracks().last().copied();
        if selected == self.last_selected {
            return;
        }
        self.last_selected = selected;
        let Some(track) = selected else {
            return;
        };
        if !self.detached.contains(&Pane::Mixer) {
            return;
        }
        let doc = self.doc.borrow();
        let Some(kind) = doc.project().track(track).map(|t| t.kind) else {
            return;
        };
        if kind != audiostrada_core::TrackKind::Audio {
            return;
        }
        let pane = if doc.is_instrument_track(track) {
            Pane::PianoRoll
        } else {
            Pane::Rack
        };
        drop(doc);
        if !self.detached.contains(&pane) && layout::is_visible(&self.tree, pane) {
            layout::activate(&mut self.tree, pane);
        }
    }

    /// Shows a panel and brings it forward, docked or in its window.
    fn reveal(&mut self, ctx: &egui::Context, pane: Pane) {
        if self.detached.contains(&pane) {
            let window = pane_window(pane);
            ctx.send_viewport_cmd_to(window, egui::ViewportCommand::Focus);
        } else {
            if !layout::is_visible(&self.tree, pane) {
                layout::toggle(&mut self.tree, pane);
            }
            layout::activate(&mut self.tree, pane);
            self.ui_changed();
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    fn reattach(&mut self, pane: Pane) {
        if self.detached.contains(&pane) {
            self.detached.retain(|p| *p != pane);
            layout::set_visible(&mut self.tree, pane, true);
            layout::activate(&mut self.tree, pane);
            self.pane_opened.remove(&pane);
            self.prefs.detached = self.detached.clone();
            self.ui_changed();
        }
    }

    /// What the document has to say, in the bottom right corner of the main window for a few
    /// seconds whatever panels are shown, and an archive being written while it is.
    fn toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let (new, archiving) = {
            let mut doc = self.doc.borrow_mut();
            (doc.take_messages(), doc.archiving())
        };
        self.toasts.extend(new.into_iter().map(|m| (m, now)));
        self.toasts
            .retain(|(_, at)| now.duration_since(*at) < Duration::from_secs(8));
        if self.toasts.is_empty() && !archiving {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let p = theme::current_for(ctx);
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                // A dot of the accent (red for errors), fading out in its last second.
                let toast = |ui: &mut egui::Ui,
                             color: egui::Color32,
                             fade: f32,
                             add: &mut dyn FnMut(&mut egui::Ui)| {
                    ui.scope(|ui| {
                        ui.multiply_opacity(fade);
                        egui::Frame::popup(ui.style())
                            .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.5)))
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    let (dot, _) = ui.allocate_exact_size(
                                        egui::vec2(8.0, 14.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(dot.center(), 3.5, color);
                                    add(ui);
                                });
                            });
                    });
                };
                if archiving {
                    toast(ui, p.accent, 1.0, &mut |ui| {
                        ui.spinner();
                        ui.label(tr("Archiving the project…"));
                    });
                }
                for (message, at) in &self.toasts {
                    let error = i18n::is_error(message);
                    let color = if error { settings::ERROR } else { p.accent };
                    let left = 8.0 - now.duration_since(*at).as_secs_f32();
                    toast(ui, color, left.clamp(0.0, 1.0), &mut |ui| {
                        ui.colored_label(if error { settings::ERROR } else { p.text }, message);
                    });
                }
            });
    }

    /// How soon to repaint: smoothly while something moves (playback, recording, sound
    /// reaching the master, armed tracks, and a moment after for the meters to fall), slowly
    /// at rest. Input repaints at once either way.
    fn refresh(&mut self) -> Duration {
        let now = Instant::now();
        {
            let doc = self.doc.borrow();
            let player = doc.player();
            let master = player.master_meter();
            let sounding = (0..master.channels()).any(|c| master.rms(c) > 1e-5);
            if player.engine().playing()
                || doc.recording().is_some()
                || sounding
                || doc.project().tracks.iter().any(|t| t.armed)
            {
                self.lively_until = now + Duration::from_millis(1500);
            }
        }
        // Screenshots count frames: they run at full rate.
        let refresh = if now < self.lively_until || self.screenshot.is_some() {
            REFRESH
        } else if self.doc.borrow().player().plugin_windows_open() {
            // Shortcuts typed in a plugin's window come through the main loop.
            PLUGIN_REFRESH
        } else {
            IDLE_REFRESH
        };
        self.inbox.borrow_mut().refresh = refresh;
        refresh
    }

    /// Opens the detached panels in their own windows. They draw independently of the main
    /// window (so they keep working when it is covered) and leave their requests in the inbox.
    fn detached_windows(&mut self, ctx: &egui::Context) {
        INBOX.with(|i| {
            i.borrow_mut().get_or_insert_with(|| Rc::clone(&self.inbox));
        });
        {
            let mut inbox = self.inbox.borrow_mut();
            inbox.blocked = self.palette.is_open() || self.preferences.capturing_keys();
            inbox.keymap.clone_from(&self.prefs.keymap);
        }
        // Plugin windows opened from now on hand the transport's shortcuts back.
        let keys = self.prefs.keymap.transport_keys();
        if keys != audiostrada_plugins::window::grabbed_keys() {
            audiostrada_plugins::window::set_grabbed_keys(keys);
        }
        let saved: Vec<(Pane, ui_prefs::WindowState)> =
            std::mem::take(&mut self.inbox.borrow_mut().windows);
        for (pane, state) in saved {
            match self.prefs.pane_windows.iter_mut().find(|(p, _)| *p == pane) {
                Some((_, s)) if *s == state => continue,
                Some((_, s)) => *s = state,
                None => self.prefs.pane_windows.push((pane, state)),
            }
            self.windows_changed = true;
        }
        for pane in self.detached.clone() {
            let opened = *self.pane_opened.entry(pane).or_insert_with(|| {
                self.prefs
                    .pane_windows
                    .iter()
                    .find(|(p, _)| *p == pane)
                    .map(|(_, s)| *s)
                    .unwrap_or(ui_prefs::WindowState {
                        size: match pane {
                            Pane::Mixer => [1100.0, 560.0],
                            Pane::PianoRoll => [1000.0, 520.0],
                            Pane::Rack => [1000.0, 300.0],
                            _ => [420.0, 600.0],
                        },
                        position: None,
                        maximized: false,
                    })
            });
            let builder = opened.apply(
                egui::ViewportBuilder::default()
                    .with_title(format!("{} — AudioStrada", pane.title()))
                    .with_min_inner_size([240.0, 160.0])
                    .with_window_level(floating_level(ctx, Floating::Panel)),
            );
            let shared = MainThread((Rc::clone(&self.views), Rc::clone(&self.inbox)));
            ctx.show_viewport_deferred(pane_window(pane), builder, move |ui, _class| {
                let (views, inbox) = shared.get();
                let ctx = ui.ctx().clone();
                let mut asked = false;
                // Shortcuts first, so the panel's widgets do not take their keys.
                {
                    let mut inbox = inbox.borrow_mut();
                    if !inbox.blocked {
                        let keys = pressed_actions(&ctx, &inbox.keymap);
                        asked |= !keys.is_empty();
                        inbox.actions.extend(keys);
                    }
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    if let Ok(mut views) = views.try_borrow_mut() {
                        views.pane_ui(ui, pane);
                        if pane == Pane::Mixer {
                            views.mixer.windows(&ctx);
                        }
                        asked |= views.has_requests();
                        if let Ok(mut doc) = views.doc.try_borrow_mut() {
                            mixer::save_chain_dialog(&ctx, &mut doc);
                            add_tracks::dialog(&ctx, &mut doc);
                        }
                    }
                });
                let mut inbox = inbox.borrow_mut();
                let previous = inbox
                    .windows
                    .iter()
                    .find(|(p, _)| *p == pane)
                    .map(|(_, s)| *s);
                if let Some(state) = ui_prefs::WindowState::read(&ctx, previous.or(Some(opened))) {
                    inbox.windows.retain(|(p, _)| *p != pane);
                    inbox.windows.push((pane, state));
                }
                if ctx.input(|i| i.viewport().close_requested()) {
                    inbox.closed.push(pane);
                    asked = true;
                }
                // The main window's logic runs the requests, even while it is hidden.
                if asked {
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
                // Meters and playheads move on their own: nothing else repaints this window.
                if matches!(pane, Pane::Mixer | Pane::PianoRoll | Pane::Rack) {
                    ctx.request_repaint_after(match inbox.refresh {
                        Duration::ZERO => REFRESH,
                        refresh => refresh,
                    });
                }
            });
        }
    }

    /// Runs what the panels asked for: menu commands, exports, detached windows' shortcuts.
    fn run_requests(&mut self, ctx: &egui::Context) {
        let typed = self.doc.borrow_mut().player_mut().take_plugin_keys();
        if !self.palette.is_open() && !self.preferences.capturing_keys() {
            let mut inbox = self.inbox.borrow_mut();
            for (sym, mods) in typed {
                inbox
                    .actions
                    .extend(self.prefs.keymap.action_for_keysym(sym, mods));
            }
        }
        let (mut actions, closed) = {
            let mut inbox = self.inbox.borrow_mut();
            (
                std::mem::take(&mut inbox.actions),
                std::mem::take(&mut inbox.closed),
            )
        };
        for pane in closed {
            self.reattach(pane);
        }
        let resized = {
            let mut views = self.views.borrow_mut();
            std::mem::take(&mut views.timeline.header_resized)
                .then_some(views.timeline.header_width)
        };
        if let Some(width) = resized {
            self.prefs.header_width = width;
            (self.hooks.ui_changed)(&self.prefs);
        }
        let (tool, waves) = {
            let views = self.views.borrow();
            (views.timeline.tool, views.timeline.wave_view)
        };
        if tool != self.prefs.timeline_tool || waves != self.prefs.waveforms {
            self.prefs.timeline_tool = tool;
            self.prefs.waveforms = waves;
            (self.hooks.ui_changed)(&self.prefs);
        }
        let stem = {
            let mut views = self.views.borrow_mut();
            actions.append(&mut views.timeline.requests);
            actions.append(&mut views.mixer.requests);
            views
                .timeline
                .stem_request
                .take()
                .or(views.mixer.stem_request.take())
        };
        if let Some(track) = stem {
            self.export_track(ctx, track);
        }
        for action in actions {
            self.run(ctx, action);
        }
    }
}

/// Values used only on the main thread. eframe calls the deferred viewports' callbacks on
/// the event loop thread, where the application lives, so sharing them there is sound.
pub(crate) struct MainThread<T>(pub(crate) T);

impl<T> MainThread<T> {
    /// A method rather than the field, so closures capture the whole wrapper.
    pub(crate) fn get(&self) -> &T {
        &self.0
    }
}

// SAFETY: see above; native eframe never runs viewport callbacks on another thread.
unsafe impl<T> Send for MainThread<T> {}
// SAFETY: as above.
unsafe impl<T> Sync for MainThread<T> {}

/// Requests left by the detached windows for the main window's logic.
#[derive(Debug, Default)]
struct DetachedInbox {
    actions: Vec<Action>,
    closed: Vec<Pane>,
    blocked: bool,
    keymap: Keymap,
    /// Repaint rate the main window chose (zero before it did).
    refresh: Duration,
    /// How the detached windows are, as they report it.
    windows: Vec<(Pane, ui_prefs::WindowState)>,
}

thread_local! {
    /// The inbox, for the effect windows the panels open.
    static INBOX: RefCell<Option<Rc<RefCell<DetachedInbox>>>> = const { RefCell::new(None) };
}

/// The keys of `action` in the current keymap, for tooltips.
pub(crate) fn shortcut_labels(action: Action) -> Vec<String> {
    INBOX
        .with(|i| {
            let inbox = i.borrow().clone()?;
            let inbox = inbox.try_borrow().ok()?;
            let mac = cfg!(target_os = "macos");
            Some(
                inbox
                    .keymap
                    .shortcuts_for(action)
                    .map(|s| s.label(mac))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

/// Shortcuts typed in an effect window, run as in the main window.
pub(crate) fn window_shortcuts(ctx: &egui::Context) {
    let Some(inbox) = INBOX.with(|i| i.borrow().clone()) else {
        return;
    };
    let Ok(mut inbox) = inbox.try_borrow_mut() else {
        return;
    };
    if inbox.blocked {
        return;
    }
    take_piano_keys(ctx);
    let keys = pressed_actions(ctx, &inbox.keymap);
    if !keys.is_empty() {
        inbox.actions.extend(keys);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// While a synth editor plays from the computer keyboard, its letters go to it instead of the
/// shortcuts.
/// The window of a panel detached from the main one.
fn pane_window(pane: Pane) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("audiostrada-pane", pane))
}

/// Whether the main window is in full screen, whichever window asks.
fn fullscreen(ctx: &egui::Context) -> bool {
    ctx.input_for(egui::ViewportId::ROOT, |i| {
        i.viewport().fullscreen.unwrap_or(false)
    })
}

fn take_piano_keys(ctx: &egui::Context) {
    let viewport = ctx.viewport_id();
    let claimed = ctx.data(|d| d.get_temp::<bool>(synth_editor::claim_id(viewport)));
    if claimed != Some(true) || ctx.text_edit_focused() {
        return;
    }
    let taken = ctx.input_mut(|i| {
        let mut taken = Vec::new();
        i.events.retain(|e| match e {
            Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                modifiers,
            } if !modifiers.any() => {
                let key = physical_key.unwrap_or(*key);
                if synth_editor::computer_key(key).is_none() {
                    return true;
                }
                if !repeat {
                    taken.push((key, *pressed));
                }
                false
            }
            _ => true,
        });
        taken
    });
    if !taken.is_empty() {
        ctx.data_mut(|d| d.insert_temp(synth_editor::keys_id(viewport), taken));
    }
}

/// Actions bound to the keys pressed this frame in the viewport of `ctx`. Space is consumed
/// when it runs a command, so a focused button does not also take it as a click.
fn pressed_actions(ctx: &egui::Context, keymap: &Keymap) -> Vec<Action> {
    // Only typing in a text field holds plain keys back; a clicked button keeps focus too.
    let typing = ctx.text_edit_focused();
    ctx.input_mut(|i| {
        let mut actions = Vec::new();
        i.events.retain(|e| {
            // The system turns Ctrl+C, X and V into clipboard events: outside text fields they
            // are the bound edits.
            let clipboard = match e {
                Event::Copy => Some(egui::Key::C),
                Event::Cut => Some(egui::Key::X),
                Event::Paste(_) | Event::PasteImage(_) => Some(egui::Key::V),
                _ => None,
            };
            if let Some(key) = clipboard {
                if typing {
                    return true;
                }
                if let Some(action) = keymap.action_for(key, egui::Modifiers::COMMAND) {
                    actions.push(action);
                }
                return false;
            }
            let Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = e
            else {
                return true;
            };
            let Some(action) = keymap.action_for(*key, *modifiers) else {
                return true;
            };
            let repeatable = matches!(
                action,
                Action::NudgeLeft
                    | Action::NudgeRight
                    | Action::ZoomIn
                    | Action::ZoomOut
                    | Action::Undo
                    | Action::Redo
            );
            // While typing in a text field, only shortcuts with Ctrl/Cmd apply.
            if *pressed && (!*repeat || repeatable) && (!typing || modifiers.command) {
                actions.push(action);
            }
            typing || *key != egui::Key::Space
        });
        actions
    })
}

/// AudioStrada's mark (packaging/icons/audiostrada-mark.svg) drawn as shapes, sharp at any
/// size: five slanted rounded bars, centered in `rect` at their own proportions.
pub(crate) fn paint_logo(painter: &egui::Painter, rect: egui::Rect) {
    use egui::{Color32, Pos2, pos2};
    let blue = |alpha: f32| Color32::from_rgb(90, 162, 255).gamma_multiply(alpha);
    let bars = [
        (5.0, 25.0, 14.0, blue(0.28)),
        (16.0, 20.0, 24.0, blue(0.5)),
        (27.0, 14.0, 36.0, blue(0.75)),
        (38.0, 7.0, 50.0, Color32::from_rgb(140, 196, 255)),
        (49.0, 17.0, 30.0, Color32::from_rgb(255, 181, 71)),
    ];
    let (width, radius) = (7.0f32, 2.0f32);
    // skewX(-12 degrees) around y = 32, as in the SVG.
    let skew = |p: Pos2| pos2(p.x - (12f32).to_radians().tan() * (p.y - 32.0), p.y);
    let (mut min, mut max) = (pos2(f32::MAX, f32::MAX), pos2(f32::MIN, f32::MIN));
    for &(x, y, h, _) in &bars {
        for corner in [
            pos2(x, y),
            pos2(x + width, y),
            pos2(x, y + h),
            pos2(x + width, y + h),
        ] {
            let q = skew(corner);
            min = min.min(q);
            max = max.max(q);
        }
    }
    let size = max - min;
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let origin = rect.center() - size * scale / 2.0;
    let place = |p: Pos2| origin + (skew(p) - min) * scale;
    for (x, y, h, color) in bars {
        // The rounded rectangle's outline, corner by corner, clockwise from the top left.
        let mut points = Vec::with_capacity(24);
        let corners = [
            (pos2(x + radius, y + radius), 180.0f32),
            (pos2(x + width - radius, y + radius), 270.0),
            (pos2(x + width - radius, y + h - radius), 0.0),
            (pos2(x + radius, y + h - radius), 90.0),
        ];
        for (center, start) in corners {
            for k in 0..=5 {
                let a = (start + 18.0 * k as f32).to_radians();
                points.push(place(center + radius * egui::vec2(a.cos(), a.sin())));
            }
        }
        painter.add(egui::Shape::convex_polygon(
            points,
            color,
            egui::Stroke::NONE,
        ));
    }
}

impl AudioStradaApp {
    fn menu_bar(&mut self, ui: &mut egui::Ui) -> Vec<Action> {
        let mut chosen = Vec::new();
        let keymap = self.prefs.keymap.clone();
        let mac = self.mac;
        let loop_fixed = self.doc.borrow().project().loop_fixed;
        let (undo, redo) = {
            let doc = self.doc.borrow();
            let h = doc.history();
            (
                h.undo_label().map(str::to_owned),
                h.redo_label().map(str::to_owned),
            )
        };
        let item = |ui: &mut egui::Ui,
                    chosen: &mut Vec<Action>,
                    action: Action,
                    text: Option<String>,
                    enabled: bool| {
            let mut button = egui::Button::new(text.unwrap_or_else(|| action.label().to_owned()));
            if let Some(label) = keymap.label(action, mac) {
                button = button.shortcut_text(label);
            }
            if ui.add_enabled(enabled, button).clicked() {
                chosen.push(action);
                ui.close();
            }
        };
        let check = |ui: &mut egui::Ui, chosen: &mut Vec<Action>, action: Action, on: bool| {
            let mut button = egui::Button::new(action.label()).selected(on);
            if let Some(label) = keymap.label(action, mac) {
                button = button.shortcut_text(label);
            }
            if ui.add(button).clicked() {
                chosen.push(action);
                ui.close();
            }
        };
        let mut open_recent = None;
        let mut open_audio = false;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(tr("File"), |ui| {
                item(ui, &mut chosen, Action::NewProject, None, true);
                item(ui, &mut chosen, Action::OpenProject, None, true);
                ui.menu_button(tr("Open recent"), |ui| {
                    if self.recent.is_empty() {
                        ui.weak(tr("No recent projects"));
                    }
                    for file in &self.recent {
                        if ui.button(file.display().to_string()).clicked() {
                            open_recent = Some(file.clone());
                            ui.close();
                        }
                    }
                });
                ui.menu_button(tr("Tabs"), |ui| {
                    for action in [
                        Action::NewProjectTab,
                        Action::OpenProjectInTab,
                        Action::NextTab,
                        Action::PreviousTab,
                        Action::CloseTab,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.separator();
                item(ui, &mut chosen, Action::Save, None, true);
                item(ui, &mut chosen, Action::SaveAs, None, true);
                item(ui, &mut chosen, Action::ProjectVersions, None, true);
                ui.separator();
                ui.menu_button(tr("Import"), |ui| {
                    item(ui, &mut chosen, Action::ImportAudio, None, true);
                    item(ui, &mut chosen, Action::ImportMidi, None, true);
                });
                ui.menu_button(tr("Export"), |ui| {
                    item(ui, &mut chosen, Action::Export, None, true);
                    item(ui, &mut chosen, Action::ExportMidi, None, true);
                });
                ui.menu_button(tr("Project files"), |ui| {
                    item(ui, &mut chosen, Action::CollectMedia, None, true);
                    item(ui, &mut chosen, Action::ArchiveProject, None, true);
                });
                ui.separator();
                item(ui, &mut chosen, Action::Quit, None, true);
            });
            ui.menu_button(tr("Edit"), |ui| {
                let text = undo.as_ref().map(|l| trf!("Undo {action}", action = l));
                item(ui, &mut chosen, Action::Undo, text, undo.is_some());
                let text = redo.as_ref().map(|l| trf!("Redo {action}", action = l));
                item(ui, &mut chosen, Action::Redo, text, redo.is_some());
                ui.separator();
                for action in [
                    Action::Cut,
                    Action::Copy,
                    Action::Paste,
                    Action::Duplicate,
                    Action::Delete,
                ] {
                    item(ui, &mut chosen, action, None, true);
                }
                ui.separator();
                item(ui, &mut chosen, Action::SelectAll, None, true);
                item(ui, &mut chosen, Action::Deselect, None, true);
                ui.menu_button(tr("Time range"), |ui| {
                    for action in [
                        Action::RangeFromClips,
                        Action::RangeFromLoop,
                        Action::RangeAllTracks,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.separator();
                item(ui, &mut chosen, Action::Split, None, true);
                ui.menu_button(tr("Nudge"), |ui| {
                    item(ui, &mut chosen, Action::NudgeLeft, None, true);
                    item(ui, &mut chosen, Action::NudgeRight, None, true);
                });
                ui.menu_button(tr("Whole project"), |ui| {
                    for action in [
                        Action::CropToSelection,
                        Action::RemoveSection,
                        Action::ClearSection,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.separator();
                ui.menu_button(tr("Tools"), |ui| {
                    let tool = self.views.borrow().timeline.tool;
                    for t in crate::timeline::Tool::ALL {
                        check(ui, &mut chosen, t.action(), tool == t);
                    }
                });
                ui.menu_button(tr("Edit mode and grid"), |ui| {
                    for (action, mode) in [
                        (Action::ModeSlip, EditMode::Slip),
                        (Action::ModeShuffle, EditMode::Shuffle),
                        (Action::ModeGrid, EditMode::Grid),
                    ] {
                        check(ui, &mut chosen, action, self.edit.mode == mode);
                    }
                    ui.separator();
                    check(ui, &mut chosen, Action::ToggleSnap, self.edit.snap);
                    item(ui, &mut chosen, Action::GridFiner, None, true);
                    item(ui, &mut chosen, Action::GridCoarser, None, true);
                });
                ui.separator();
                item(ui, &mut chosen, Action::Preferences, None, true);
            });
            ui.menu_button(tr("Clip"), |ui| {
                item(ui, &mut chosen, Action::NewMidiClip, None, true);
                ui.separator();
                item(ui, &mut chosen, Action::Fades, None, true);
                for action in [Action::Heal, Action::Consolidate, Action::StripSilence] {
                    item(ui, &mut chosen, action, None, true);
                }
                ui.separator();
                for action in [Action::ProcessClips, Action::BeatDetective, Action::Groove] {
                    item(ui, &mut chosen, action, None, true);
                }
                ui.separator();
                item(ui, &mut chosen, Action::Quantize, None, true);
            });
            ui.menu_button(tr("Track"), |ui| {
                ui.menu_button(tr("Add"), |ui| {
                    for action in [
                        Action::AddTrack,
                        Action::AddStereoTrack,
                        Action::AddInstrumentTrack,
                        Action::AddDrumTrack,
                        Action::AddVca,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                    ui.separator();
                    item(ui, &mut chosen, Action::AddTracks, None, true);
                });
                ui.separator();
                ui.label(egui::RichText::new(tr("Selected tracks")).small().weak());
                for action in [
                    Action::DuplicateTracks,
                    Action::DeleteTracks,
                    Action::GroupTracks,
                ] {
                    item(ui, &mut chosen, action, None, true);
                }
                for action in [Action::MuteTracks, Action::SoloTracks, Action::ArmTracks] {
                    item(ui, &mut chosen, action, None, true);
                }
                ui.menu_button(tr("Move and select"), |ui| {
                    for action in [
                        Action::MoveTrackUp,
                        Action::MoveTrackDown,
                        Action::SelectPreviousTrack,
                        Action::SelectNextTrack,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.menu_button(tr("Playlists"), |ui| {
                    for action in [Action::NewPlaylist, Action::NextPlaylist] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.separator();
                item(ui, &mut chosen, Action::ToggleGroups, None, true);
            });
            ui.menu_button(tr("Transport"), |ui| {
                for action in [
                    Action::PlayPause,
                    Action::Stop,
                    Action::Panic,
                    Action::Record,
                    Action::GoToStart,
                    Action::GoToEnd,
                ] {
                    item(ui, &mut chosen, action, None, true);
                }
                ui.separator();
                ui.menu_button(tr("Loop and punch"), |ui| {
                    item(ui, &mut chosen, Action::ToggleLoop, None, true);
                    item(ui, &mut chosen, Action::LoopSelection, None, true);
                    check(ui, &mut chosen, Action::LoopFollowsSelection, !loop_fixed);
                    ui.separator();
                    item(ui, &mut chosen, Action::TogglePunch, None, true);
                });
                ui.menu_button(tr("Markers"), |ui| {
                    for action in [
                        Action::AddMarker,
                        Action::PreviousMarker,
                        Action::NextMarker,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.menu_button(tr("Transients"), |ui| {
                    item(ui, &mut chosen, Action::PreviousTransient, None, true);
                    item(ui, &mut chosen, Action::NextTransient, None, true);
                });
                ui.menu_button(tr("Metronome"), |ui| {
                    item(ui, &mut chosen, Action::ToggleMetronome, None, true);
                    item(ui, &mut chosen, Action::MetronomeSettings, None, true);
                });
                ui.menu_button(tr("Reference track"), |ui| {
                    item(ui, &mut chosen, Action::ListenToReference, None, true);
                    item(ui, &mut chosen, Action::ReferenceTrack, None, true);
                });
                ui.separator();
                let low = self.doc.borrow().player().low_latency();
                check(ui, &mut chosen, Action::LowLatency, low);
            });
            ui.menu_button(tr("View"), |ui| {
                ui.menu_button(tr("Panels"), |ui| {
                    for pane in Pane::ALL {
                        let action = match pane {
                            Pane::Timeline => Action::ShowTimeline,
                            Pane::Mixer => Action::ShowMixer,
                            Pane::Browser => Action::ShowBrowser,
                            Pane::Inspector => Action::ShowInspector,
                            Pane::Markers => Action::ShowMarkers,
                            Pane::PianoRoll => Action::ShowPianoRoll,
                            Pane::Rack => Action::ShowRack,
                        };
                        check(
                            ui,
                            &mut chosen,
                            action,
                            layout::is_visible(&self.tree, pane) || self.detached.contains(&pane),
                        );
                    }
                    ui.separator();
                    item(ui, &mut chosen, Action::ResetLayout, None, true);
                });
                ui.menu_button(tr("Windows"), |ui| {
                    check(
                        ui,
                        &mut chosen,
                        Action::DetachMixer,
                        self.detached.contains(&Pane::Mixer),
                    );
                    check(
                        ui,
                        &mut chosen,
                        Action::DetachPianoRoll,
                        self.detached.contains(&Pane::PianoRoll),
                    );
                });
                let compact = self.views.borrow().mixer.compact;
                check(ui, &mut chosen, Action::CompactMixer, compact);
                check(
                    ui,
                    &mut chosen,
                    Action::ToggleFullscreen,
                    fullscreen(ui.ctx()),
                );
                ui.separator();
                ui.menu_button(tr("Zoom"), |ui| {
                    for action in [
                        Action::ZoomIn,
                        Action::ZoomOut,
                        Action::ZoomToSelection,
                        Action::ZoomToFit,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.menu_button(tr("Track height"), |ui| {
                    item(ui, &mut chosen, Action::TallerTracks, None, true);
                    item(ui, &mut chosen, Action::ShorterTracks, None, true);
                });
                ui.menu_button(tr("Waveforms"), |ui| {
                    for action in [
                        Action::WaveformZoomIn,
                        Action::WaveformZoomOut,
                        Action::WaveformZoomReset,
                    ] {
                        item(ui, &mut chosen, action, None, true);
                    }
                });
                ui.separator();
                item(ui, &mut chosen, Action::CommandPalette, None, true);
            });
            ui.menu_button(tr("Help"), |ui| {
                item(ui, &mut chosen, Action::KeyboardShortcuts, None, true);
                item(ui, &mut chosen, Action::CommandPalette, None, true);
                ui.separator();
                item(ui, &mut chosen, Action::Assistants, None, true);
                item(ui, &mut chosen, Action::License, None, true);
                item(ui, &mut chosen, Action::About, None, true);
            });
            ui.add_space(16.0);
            if let Some(stream) = self.host.stream() {
                let info = stream.info();
                let text = format!(
                    "{} · {} Hz · {:.1} ms",
                    info.backend,
                    info.sample_rate,
                    info.buffer_latency_ms()
                );
                let label =
                    egui::Label::new(egui::RichText::new(text).weak()).sense(egui::Sense::click());
                open_audio |= ui.add(label).on_hover_text(tr("Audio settings")).clicked();
            } else if let Some(error) = self.host.error() {
                open_audio |= ui.colored_label(settings::ERROR, error).clicked();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let version = concat!("AudioStrada ", env!("CARGO_PKG_VERSION"));
                ui.label(egui::RichText::new("AudioStrada").strong().size(15.0))
                    .on_hover_text(version);
                // Right to left: the logo goes before the name.
                let (mark, response) =
                    ui.allocate_exact_size(egui::vec2(18.0, 16.5), egui::Sense::hover());
                paint_logo(ui.painter(), mark);
                response.on_hover_text(version);
                if let Some(release) = self.updates.found.clone() {
                    let covered = match self.license.license() {
                        Some(license) if license.covers(&release.date) => {
                            tr("Included in your license")
                        }
                        Some(_) => tr("Released after your updates ended: renew to use it"),
                        None => tr("Download it from the web site"),
                    };
                    let text = trf!("Version {version} available", version = release.version);
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new(text).small().strong(),
                        ))
                        .on_hover_text(covered)
                        .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&release.url));
                        self.updates.found = None;
                    }
                }
                if self.license.evaluation() {
                    let badge = egui::Button::new(
                        egui::RichText::new(tr("Evaluation"))
                            .small()
                            .strong()
                            .color(theme::RECORD),
                    )
                    .stroke(egui::Stroke::new(1.0, theme::RECORD))
                    .corner_radius(8.0);
                    if ui
                        .add(badge)
                        .on_hover_text(tr("This copy has no license: see the License window"))
                        .clicked()
                    {
                        self.license_window.open();
                    }
                }
            });
        });
        if open_audio {
            self.preferences.open_audio();
        }
        if let Some(file) = open_recent {
            self.open(ui.ctx(), file, false);
        }
        chosen
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Ok(mut doc) = self.doc.try_borrow_mut() {
            mixer::save_chain_dialog(ctx, &mut doc);
            add_tracks::dialog(ctx, &mut doc);
        }
        if let Some(pending) = self.pending.clone() {
            let title = self.doc.borrow().title();
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("unsaved")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading(tr("Unsaved changes"));
                ui.add_space(4.0);
                ui.label(trf!(
                    "Save the changes to \"{project}\" first?",
                    project = title.trim_end_matches(" *")
                ));
                theme::footer(ui, |ui| {
                    if theme::primary_button(ui, tr("Save")).clicked() {
                        choice = Some(0);
                    }
                    if ui.button(tr("Cancel")).clicked() {
                        choice = Some(2);
                    }
                    if ui.button(tr("Don't save")).clicked() {
                        choice = Some(1);
                    }
                });
            });
            match choice {
                Some(0) => {
                    self.pending = None;
                    self.save_then(ctx, AfterSave::Perform(pending));
                }
                Some(1) => {
                    self.pending = None;
                    self.perform(ctx, pending);
                }
                Some(_) => {
                    self.pending = None;
                    self.tabs_noted = false;
                }
                None => {}
            }
        }

        if let Some(offer) = self.recovery.clone() {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("recovery")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.heading(tr("Recover unsaved work?"));
                ui.add_space(4.0);
                let what = offer.project.as_ref().map_or_else(
                    || tr("an untitled project").to_owned(),
                    |p| {
                        format!(
                            "\"{}\"",
                            p.file_stem().unwrap_or_default().to_string_lossy()
                        )
                    },
                );
                ui.label(trf!(
                    "AudioStrada has an autosave of {what} with changes that were never saved, \
                     probably because it closed unexpectedly.",
                    what = what
                ));
                theme::footer(ui, |ui| {
                    if theme::primary_button(ui, tr("Recover")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr("Discard")).clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(recover) = choice {
                self.recovery = None;
                if recover {
                    match Document::recover(
                        &offer.autosave,
                        offer.project.as_deref(),
                        self.dirs.clone(),
                    ) {
                        Ok(doc) => self.show(doc, self.recovery_tab),
                        Err(e) => self.error = Some(trf!("Cannot recover: {error}", error = e)),
                    }
                } else {
                    autosave::discard(&offer.autosave);
                    if let Some(file) = offer.project {
                        self.perform(ctx, Pending::Open(file, self.recovery_tab));
                    }
                }
            }
        }

        if self.about {
            egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.vertical_centered(|ui| {
                    let font = theme::strong(26.0);
                    let name = ui
                        .painter()
                        .layout_no_wrap("AudioStrada".into(), font.clone(), egui::Color32::WHITE)
                        .size();
                    let mark = egui::vec2(name.y * 1.1, name.y);
                    let gap = 10.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.add_space(
                            ((ui.available_width() - mark.x - gap - name.x) / 2.0).max(0.0),
                        );
                        let (rect, _) = ui.allocate_exact_size(mark, egui::Sense::hover());
                        paint_logo(ui.painter(), rect);
                        ui.label(egui::RichText::new("AudioStrada").font(font));
                    });
                    ui.weak(concat!("Version ", env!("CARGO_PKG_VERSION")));
                    ui.add_space(6.0);
                    ui.label(tr("A free digital audio workstation, written in Rust."));
                    ui.weak(tr("GNU General Public License, version 3"));
                    ui.add_space(6.0);
                    match &self.license {
                        audiostrada_license::Status::Unofficial => {
                            ui.weak(tr("Unofficial build, compiled from source"));
                        }
                        audiostrada_license::Status::Evaluation => {
                            ui.colored_label(theme::RECORD, tr("Evaluation"));
                        }
                        audiostrada_license::Status::Licensed(license)
                        | audiostrada_license::Status::Lapsed(license) => {
                            ui.label(trf!(
                                "{plan} of {name}",
                                plan = license_window::plan_name(license.plan),
                                name = license.name
                            ));
                        }
                    }
                    if ui.link(tr("Source code")).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(audiostrada_license::link("source")));
                    }
                    ui.add_space(6.0);
                    ui.collapsing(tr("Credits"), |ui| {
                        ui.label(egui::RichText::new(tr("Strada-Trigger drum samples")).strong());
                        for line in [
                            "Karoryfer Samples: Unruly, Big Rusty and Swirly Drums (CC0 1.0)",
                            "SM Drums: Scott McLean, Tod Stillwell, Suleiman Ali",
                            "DrumGizmo team: MuldjordKit and DRSKit (CC BY 4.0)",
                        ] {
                            ui.weak(line);
                        }
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(tr("Strada-Amp captures and cabinets")).strong());
                        for line in [
                            "Neural Amp Modeler community captures (GPL v3): Tim R, Phillipe P, George B, Sascha S, Roman A",
                            "Guitarix cabinet impulse responses (GPL v2 or later)",
                        ] {
                            ui.weak(line);
                        }
                    });
                });
                theme::footer(ui, |ui| {
                    if theme::primary_button(ui, tr("OK")).clicked() {
                        self.about = false;
                    }
                });
            });
        }

        if let Some(error) = self.error.clone() {
            egui::Modal::new(egui::Id::unique("error")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.heading(egui::RichText::new(tr("Error")).color(theme::RECORD));
                ui.add_space(4.0);
                ui.label(&error);
                theme::footer(ui, |ui| {
                    if theme::primary_button(ui, tr("OK")).clicked() {
                        self.error = None;
                    }
                });
            });
        }
    }

    /// Persists the panel layout when the user rearranged it.
    fn watch_layout(&mut self) {
        if self.layout_checked.elapsed() < LAYOUT_SAVE_INTERVAL {
            return;
        }
        self.layout_checked = Instant::now();
        if layout::save(&self.tree) != self.saved_layout {
            self.ui_changed();
        }
    }
}

/// Plugin windows stay above AudioStrada's main window where the system allows it (X11, Windows);
/// a Wayland window cannot parent the plugins' X11 windows.
fn set_plugin_window_parent(frame: &eframe::Frame) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[allow(clippy::useless_conversion, reason = "c_ulong is 32 bits on Windows")]
    let parent = match frame.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => u64::from(h.window),
        Ok(RawWindowHandle::Xcb(h)) => u64::from(h.window.get()),
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as u64,
        _ => 0,
    };
    audiostrada_plugins::window::set_parent(parent);
}

/// On macOS and Windows the detached panels and editors float above the main window while
/// AudioStrada is in use, so that they stay in front when it takes the focus.
#[cfg(any(target_os = "macos", windows))]
#[cfg_attr(target_os = "macos", allow(unused_variables))]
fn keep_windows_above(ctx: &egui::Context, frame: &eframe::Frame) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = frame.window_handle().map(|h| h.as_raw());
    #[cfg(target_os = "macos")]
    if let Ok(RawWindowHandle::AppKit(h)) = handle {
        audiostrada_plugins::window::keep_windows_above(h.ns_view.as_ptr());
    }
    // Seen again soon: another application coming in front sends us nothing.
    #[cfg(windows)]
    if let Ok(RawWindowHandle::Win32(h)) = handle
        && audiostrada_plugins::window::keep_windows_above(h.hwnd.get() as u64)
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// What a window besides the main one is, for its level.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Floating {
    /// The effect browser.
    Browser,
    /// An effect's editor, shown from the viewport drawing it.
    Editor(egui::ViewportId),
    /// A detached panel.
    Panel,
}

/// Whether the main window shows a menu, a popup or a dialog that floating windows must not
/// cover, as of its last frame.
static MAIN_COVERED: AtomicBool = AtomicBool::new(false);

/// Lowers the floating windows under the main one while it shows a menu or a dialog: they
/// drop to the normal level, then the main window comes to the front once (Linux only).
fn uncover_main(ctx: &egui::Context) {
    if cfg!(any(windows, target_os = "macos")) {
        return;
    }
    let covered = ctx.any_popup_open()
        || ctx.memory(|m| {
            m.top_modal_layer().is_some()
                || m.areas()
                    .visible_layer_ids()
                    .iter()
                    .any(|l| l.order == egui::Order::Middle)
        });
    MAIN_COVERED.store(covered, Ordering::Relaxed);
    let id = egui::Id::unique("main-covered-frames");
    let frames = ctx.data_mut(|d| {
        let f = d.get_temp_mut_or_default::<u8>(id);
        *f = if covered { f.saturating_add(1) } else { 0 };
        *f
    });
    // The levels change on this frame, the main window comes up on a later one.
    #[cfg(all(unix, not(target_os = "macos")))]
    if frames == 3 {
        audiostrada_plugins::window::raise_parent();
    }
    if (1..3).contains(&frames) {
        ctx.request_repaint();
    }
}

/// Level of AudioStrada's windows besides the main one. macOS and Windows keep them above it
/// themselves while AudioStrada is in use (see [`keep_windows_above`]). Elsewhere the browser
/// floats, an editor floats while the main window, its own or the one drawing it has the
/// focus, and panels stay normal; none floats over a menu, a dialog or a file chooser.
pub(crate) fn floating_level(ctx: &egui::Context, kind: Floating) -> egui::WindowLevel {
    if cfg!(windows) {
        return egui::WindowLevel::Normal;
    }
    if cfg!(target_os = "macos") {
        return egui::WindowLevel::AlwaysOnTop;
    }
    if MAIN_COVERED.load(Ordering::Relaxed) || dialogs::any_open() {
        return egui::WindowLevel::Normal;
    }
    let focused = |v| ctx.input_for(v, |i| i.viewport().focused.unwrap_or(false));
    let floats = match kind {
        Floating::Browser => true,
        Floating::Editor(id) => {
            focused(egui::ViewportId::ROOT) || focused(ctx.viewport_id()) || focused(id)
        }
        Floating::Panel => false,
    };
    if floats {
        egui::WindowLevel::AlwaysOnTop
    } else {
        egui::WindowLevel::Normal
    }
}

impl eframe::App for AudioStradaApp {
    /// Everything that must go on while the main window is hidden (covered, minimized) and
    /// a detached panel is used: the engine, devices, and what the panels ask for.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.prefs_applied {
            set_plugin_window_parent(frame);
            self.prefs.keymap.upgrade();
            self.prefs.apply(ctx);
            self.prefs_applied = true;
            if let Some(name) = audiostrada_plugins::guard::take_report() {
                self.doc.borrow_mut().notify(trf!(
                    "{name} crashed AudioStrada while loading: it is ignored while the plugin \
                     sandbox is off",
                    name = name
                ));
            }
            let auto = self.prefs.auto_monitoring;
            self.doc.borrow_mut().player_mut().set_auto_monitoring(auto);
            self.open_midi_inputs();
        }
        if !self.tabs_restored {
            self.tabs_restored = true;
            self.restore_tabs();
        }
        self.watch_devices();
        #[cfg(any(target_os = "macos", windows))]
        keep_windows_above(ctx, frame);
        if self.plugin_scan.as_ref().is_some_and(|h| h.is_finished())
            && let Some(Ok(database)) = self.plugin_scan.take().map(|h| h.join())
        {
            let mut doc = self.doc.borrow_mut();
            // Files that failed before were already told about.
            let known = doc
                .player()
                .plugins()
                .failures()
                .map(|(p, _)| p.to_path_buf());
            let known: Vec<_> = known.collect();
            let failed = database
                .failures()
                .filter(|(p, _)| !known.iter().any(|k| k == p))
                .count();
            doc.set_plugins(database);
            if failed > 0 {
                doc.notify(trf!(
                    "{count} plugin file(s) could not be loaded and are ignored: see Preferences, \
                     Plugins",
                    count = failed
                ));
            }
        }
        let inputs = self.host.stream().map_or(0, |s| s.info().inputs as usize);
        self.doc.borrow_mut().set_stream_inputs(inputs);
        self.run_requests(ctx);
        self.reveal_clip(ctx);
        self.follow_selection();
        self.answer_control(ctx);
        self.doc.borrow_mut().update();
        self.route_midi_outputs();
        ctx.request_repaint_after(self.refresh());
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        let mut media = Vec::new();
        for path in dropped {
            match license_file(&path) {
                Some(text) => self.license_window.offer(text, &mut self.license),
                None => media.push(path),
            }
        }
        let dropped = media;
        if !dropped.is_empty() {
            let target = ctx
                .input(|i| i.pointer.latest_pos())
                .and_then(|pos| self.views.borrow().timeline.drop_target(pos, &self.edit));
            match target {
                Some((track, at)) => self.doc.borrow_mut().import_at(&dropped, track, at),
                None => self.doc.borrow_mut().import(&dropped),
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if self.any_unsaved() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.pending.is_none() {
                    self.quit(&ctx);
                }
            } else {
                self.remember_tabs();
                self.tabs_noted = true;
                self.quit_next();
            }
        }
        let title = format!("{} — AudioStrada", self.doc.borrow().title());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        // A press anywhere takes the shortcuts from the piano roll, unless it lands in it.
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.views.borrow_mut().piano.focused = false;
        }
        let frame = ctx.cumulative_frame_nr();
        self.views.borrow_mut().piano.release_if_hidden(frame);
        let mut actions = self.pressed_actions(&ctx);
        let menu_frame = egui::Frame::side_top_panel(&ctx.global_style())
            .inner_margin(egui::Margin::symmetric(10, 6));
        egui::Panel::top("menu-bar")
            .frame(menu_frame)
            .show(ui, |ui| actions.extend(self.menu_bar(ui)));
        let mut counter = self.prefs.counter;
        let mut audio = transport::Audio {
            status: self.host.stream().map(|s| s.status()),
            open_settings: false,
        };
        egui::Panel::top("transport").show(ui, |ui| {
            let mut doc = self.doc.borrow_mut();
            let recording = doc.recording().is_some();
            actions.extend(transport::show(
                ui,
                &mut doc,
                &self.prefs.keymap,
                self.mac,
                (recording, &mut audio),
                &mut counter,
            ));
            crate::metronome_window::show(ui.ctx(), &mut doc);
            versions_window::show(ui.ctx(), &mut doc);
            reference_window::show(ui.ctx(), &mut doc);
        });
        if audio.open_settings {
            self.preferences.open_audio();
        }
        if counter != self.prefs.counter {
            self.prefs.counter = counter;
            (self.hooks.ui_changed)(&self.prefs);
        }
        if self.tabs.len() > 1 {
            egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        }
        let views = Rc::clone(&self.views);
        let detach = {
            let mut views = views.borrow_mut();
            views.edit = self.edit;
            let detach = egui::CentralPanel::default()
                .show(ui, |ui| {
                    let mut workspace = Workspace {
                        content: &mut *views,
                        detach: None,
                    };
                    self.tree.ui(&mut workspace, ui);
                    workspace.detach
                })
                .inner;
            self.edit = views.edit;
            detach
        };
        if let Some(pane) = detach {
            self.detach(pane);
        }
        // Docked, the mixer's windows belong to the main window (detached, to the mixer's).
        if !self.detached.contains(&Pane::Mixer) {
            self.views.borrow_mut().mixer.windows(&ctx);
        }
        self.detached_windows(&ctx);

        if let Some(action) = self.palette.show(&ctx, &self.prefs.keymap, self.mac) {
            actions.push(action);
        }
        let export_changed = {
            let mut doc = self.doc.borrow_mut();
            self.export.show(&ctx, &mut doc, &mut self.prefs.export)
        };
        if export_changed {
            self.ui_changed();
        }
        let database = self.doc.borrow().player().plugins().clone();
        let outcome = self.preferences.show(
            &ctx,
            &mut self.prefs,
            &mut self.audio_settings,
            &self.host,
            self.mac,
            (&database, self.plugin_scan.is_some()),
        );
        if outcome.midi_changed {
            self.open_midi_inputs();
        }
        if outcome.ui_changed {
            self.ui_changed();
        }
        if let Some(full) = outcome.rescan
            && self.plugin_scan.is_none()
            && audiostrada_plugins::enabled()
        {
            self.plugin_scan = Some((self.hooks.plugin_scan)(
                self.prefs.plugin_dirs.clone(),
                full,
            ));
        }
        match outcome.audio {
            SettingsChange::None => {}
            SettingsChange::Backend(kind) => {
                self.host.select_backend(kind);
                (self.hooks.audio_changed)(&self.host);
            }
            SettingsChange::Config(config) => {
                self.host.apply(config);
                (self.hooks.audio_changed)(&self.host);
            }
        }
        for action in actions {
            self.run(&ctx, action);
        }
        self.run_requests(&ctx);
        self.dialogs(&ctx);
        self.file_dialogs(&ctx);
        self.license_window.show(&ctx, &mut self.license);
        let listening = self.control.as_ref().map(|c| c.port);
        if let Some(change) = self.assistants_window.show(
            &ctx,
            listening,
            self.prefs.assistants_port,
            &self.control_log,
        ) {
            match change {
                assistants_window::Change::Start(port) => match self.listen(port) {
                    Ok(()) => {
                        self.prefs.assistants = true;
                        self.prefs.assistants_port = port;
                    }
                    Err(e) => self.error = Some(e),
                },
                assistants_window::Change::Stop => {
                    self.stop_listening();
                    self.prefs.assistants = false;
                }
            }
            (self.hooks.ui_changed)(&self.prefs);
        }
        self.ask_for_updates(&ctx);
        self.updates.poll();
        if self.updates.start(&mut self.prefs) {
            self.ui_changed();
        }
        self.toasts(&ctx);
        if self.maximize_in > 0 {
            self.maximize_in -= 1;
            if self.maximize_in == 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            ctx.request_repaint();
        } else if let Some(state) = ui_prefs::WindowState::read(&ctx, self.prefs.window)
            && Some(state) != self.prefs.window
        {
            self.prefs.window = Some(state);
            self.windows_changed = true;
        }
        self.watch_layout();
        uncover_main(&ctx);
        if let Some(screenshot) = &mut self.screenshot
            && screenshot.tick(&ctx)
        {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(self.refresh());
    }

    fn on_exit(&mut self) {
        if !self.tabs_noted {
            self.remember_tabs();
        }
        self.ui_changed();
    }
}

/// Opens the main window; returns when it is closed.
pub fn run(mut app: AudioStradaApp) -> eframe::Result {
    // Development aid for screenshots: AUDIOSTRADA_WINDOW_SIZE=1600x1000.
    let forced = std::env::var("AUDIOSTRADA_WINDOW_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some([w.parse().ok()?, h.parse().ok()?])
    });
    let viewport = egui::ViewportBuilder::default()
        .with_title("AudioStrada")
        // Matches the desktop file, so Wayland shows the right icon.
        .with_app_id("audiostrada");
    if forced.is_none() && app.prefs.window.is_some_and(|w| w.maximized) {
        app.maximize_in = 3;
    }
    // As the window was left, else a default size.
    let mut viewport = match (forced, app.prefs.window) {
        (Some(size), _) => viewport.with_inner_size(size),
        (None, Some(state)) => state.apply(viewport),
        (None, None) => viewport.with_inner_size([1280.0, 800.0]),
    };
    match eframe::icon_data::from_png_bytes(include_bytes!(
        "../../../packaging/icons/audiostrada-256.png"
    )) {
        Ok(icon) => viewport = viewport.with_icon(std::sync::Arc::new(icon)),
        Err(e) => tracing::warn!("cannot decode the window icon: {e}"),
    }
    let mut options = eframe::NativeOptions {
        viewport: viewport.with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    // Waiting for the vertical sync stalls every window while one is covered by another (the
    // compositor stops its frames), so that closing a window over another took seconds. The
    // windows only draw when asked, at the meters' pace.
    options.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    #[cfg(target_os = "linux")]
    {
        let display = std::env::var_os("DISPLAY").is_some();
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        if app.prefs.x11_windows && display {
            use winit::platform::x11::EventLoopBuilderExtX11;
            options.event_loop_builder = Some(Box::new(|builder| {
                builder.with_x11();
            }));
        }
        screens::set_x11(display && (app.prefs.x11_windows || !wayland));
    }
    eframe::run_native("AudioStrada", options, Box::new(|_cc| Ok(Box::new(app))))
}

/// The text of a license file dropped on the window.
fn license_file(path: &Path) -> Option<String> {
    let small = std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() < 16_384);
    let text = small
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()?;
    text.contains("-----BEGIN AUDIOSTRADA LICENSE-----")
        .then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_windows_run_the_shortcuts() {
        let inbox = Rc::new(RefCell::new(DetachedInbox::default()));
        INBOX.with(|i| *i.borrow_mut() = Some(Rc::clone(&inbox)));
        let ctx = egui::Context::default();
        let space = Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let input = egui::RawInput {
            events: vec![space],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| window_shortcuts(ui.ctx()))
            .drop_without_applying_deltas();
        assert_eq!(inbox.borrow().actions, [Action::PlayPause]);
        INBOX.with(|i| *i.borrow_mut() = None);
    }

    #[test]
    fn the_system_clipboard_keys_run_copy_cut_and_paste() {
        let ctx = egui::Context::default();
        let keymap = Keymap::default();
        let input = egui::RawInput {
            events: vec![Event::Copy, Event::Cut, Event::Paste("x".into())],
            ..Default::default()
        };
        let mut actions = Vec::new();
        ctx.run_ui(input, |ui| actions = pressed_actions(ui.ctx(), &keymap))
            .drop_without_applying_deltas();
        assert_eq!(actions, [Action::Copy, Action::Cut, Action::Paste]);
    }
}
