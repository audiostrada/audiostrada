//! An open project: undo history, file location, autosave, media and playback.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use audiostrada_core::automation::{
    AutomationLane, AutomationMode, AutomationPoint, AutomationTarget, CurveShape, value_at,
};
use audiostrada_core::autosave::{self, Autosaver};
use audiostrada_core::file::{self, ProjectError};
use audiostrada_core::routing::Channel;
use audiostrada_core::{
    Clip, ClipId, History, Note, Project, ProjectPaths, Source, SourceId, TrackId, TrackKind,
    base64, edit,
};
use audiostrada_engine::EngineConfig;
use audiostrada_engine::EngineProcessor;
use audiostrada_engine::media::{MediaPool, MediaState};
use audiostrada_engine::project::{ProjectPlayer, media_paths};
use audiostrada_engine::record::{Recorder, TakeTarget};
use audiostrada_midi::live::LiveReader;
use audiostrada_plugins::scan::Database;

use crate::i18n::tr;
use crate::trf;

mod archive;
mod export;
mod groove;
mod processing;
mod reference;
pub use groove::{Band, GrooveMoves, GrooveScan, GrooveSettings, Grooved};
pub use processing::{Found, HitScan, StripSettings};

pub use export::{ExportRange, ExportSettings, Normalize, Process};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

/// Standard sound controllers (GM2) and the synth parameter each one turns.
const SYNTH_CONTROLLERS: &[(u8, &str)] = &[
    (74, "cutoff"),
    (71, "resonance"),
    (73, "attack"),
    (75, "decay"),
    (72, "release"),
    (7, "volume"),
];

/// Folders used outside of any project.
#[derive(Clone, Debug, Default)]
pub struct AppDirs {
    /// Autosaves of untitled projects.
    pub recovery: Option<PathBuf>,
    /// Peak caches of media used by untitled projects.
    pub peaks: Option<PathBuf>,
    /// Recordings made before the project was saved.
    pub recordings: Option<PathBuf>,
}

/// A recording in progress, or finishing after the stop.
#[derive(Debug)]
struct Recording {
    recorder: Option<Recorder>,
    midi: Option<MidiTake>,
    /// Project position where the visible take starts.
    start: u64,
    tracks: Vec<TrackId>,
    offset: i64,
    seen_playing: bool,
    stopping: bool,
}

/// How a click changes the track selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Only,
    Toggle,
    Extend,
}

impl SelectMode {
    /// From the modifiers of a click: Ctrl (Cmd) toggles, Shift extends.
    pub fn from_modifiers(modifiers: eframe::egui::Modifiers) -> Self {
        if modifiers.command {
            Self::Toggle
        } else if modifiers.shift {
            Self::Extend
        } else {
            Self::Only
        }
    }
}

/// MIDI being recorded on instrument tracks.
#[derive(Debug)]
struct MidiTake {
    reader: LiveReader,
    /// Messages not turned into notes yet, and their arrival position (project samples).
    events: Vec<([u8; 3], u64)>,
    tracks: Vec<TrackId>,
    /// Loop recording: passes merged so far, the clip they went into on each track, the
    /// notes recorded and the playhead at the last poll (to notice the jump back).
    passes: u32,
    clips: Vec<(TrackId, ClipId)>,
    notes: usize,
    last: Option<u64>,
}

/// Notes and controller points of `events` (project positions) between `from` and `to`,
/// relative to `from`. Note-ons more than `early` before `from` are dropped (count-in) and
/// notes still held end at `to`.
fn midi_from_events(
    events: &[([u8; 3], u64)],
    from: u64,
    to: u64,
    early: u64,
) -> (Vec<Note>, Vec<audiostrada_core::ControlPoint>) {
    let to = to.max(from + 1);
    let mut open: Vec<(u8, u8, u64, u8)> = Vec::new();
    let mut notes = Vec::new();
    let mut controls = Vec::new();
    for &(data, position) in events {
        let at = position.clamp(from, to) - from;
        let event = audiostrada_midi::MidiEvent { time: 0, data };
        if event.as_note_on().is_some() && position + early < from {
            continue;
        }
        if let Some(point) = audiostrada_core::ControlPoint::from_message(at, data) {
            if position + early >= from {
                controls.push(point);
            }
            continue;
        }
        if let Some((key, velocity)) = event.as_note_on() {
            open.push((event.channel(), key, at, velocity));
        } else if let Some(key) = event.as_note_off()
            && let Some(i) = open
                .iter()
                .position(|o| o.0 == event.channel() && o.1 == key)
        {
            let (channel, key, on, velocity) = open.remove(i);
            notes.push(Note {
                start: on,
                length: (at - on).max(1),
                key,
                velocity,
                channel,
                ..Default::default()
            });
        }
    }
    for (channel, key, on, velocity) in open {
        notes.push(Note {
            start: on,
            length: (to - from - on).max(1),
            key,
            velocity,
            channel,
            ..Default::default()
        });
    }
    notes.sort_by_key(|n| (n.start, n.key));
    (notes, controls)
}

fn midi_clip(
    name: String,
    start: u64,
    length: u64,
    notes: Vec<Note>,
    controls: Vec<audiostrada_core::ControlPoint>,
) -> Clip {
    Clip {
        id: ClipId(0),
        source: SourceId(0),
        name,
        start,
        offset: 0,
        length,
        gain_db: 0.0,
        fade_in: 0,
        fade_out: 0,
        fade_shape: Default::default(),
        fade_out_shape: None,
        muted: false,
        stretch: 1.0,
        pitch: 0.0,
        controls: (!controls.is_empty()).then(|| std::sync::Arc::new(controls)),
        notes: Some(std::sync::Arc::new(notes)),
    }
}

/// A file being imported: added to the project once decoded.
#[derive(Debug)]
struct Import {
    path: PathBuf,
    /// Path as stored in the project.
    stored: PathBuf,
    /// Existing track and position to place it at; `None` adds a track at the start.
    target: Option<(Option<TrackId>, u64)>,
}

#[derive(Debug)]
pub struct Document {
    history: History,
    paths: Option<ProjectPaths>,
    dirs: AppDirs,
    autosaver: Autosaver,
    untitled_autosave: Option<PathBuf>,
    media: MediaPool,
    player: ProjectPlayer,
    imports: Vec<Import>,
    synced: Option<(u64, u64)>,
    messages: Vec<String>,
    /// A clip to select and open in its editor, asked for by a window that cannot do it.
    revealed_clip: Option<ClipId>,
    /// File being auditioned, and whether it still waits for decoding.
    preview: Option<(PathBuf, bool)>,
    recording: Option<Recording>,
    writers: HashMap<(Channel, AutomationTarget), Writer>,
    /// Automated controls held in Read mode (overriding their lane).
    held: HashMap<(Channel, AutomationTarget), f32>,
    was_playing: bool,
    exporting: Option<export::Exporting>,
    /// Inputs of the running audio stream, offered to tracks.
    stream_inputs: usize,
    /// Selected tracks, shared by the views; the last one has the focus.
    track_selection: Vec<TrackId>,
    render_queue: Vec<export::QueuedExport>,
    /// Control waiting for a MIDI controller (MIDI learn): target and value range.
    learning: Option<(Channel, AutomationTarget, f32, f32)>,
    controllers: LiveReader,
    /// Archive being written, to its file.
    archiving: Option<std::thread::JoinHandle<Result<PathBuf, String>>>,
    /// The reference file measured, and its integrated loudness.
    reference_lufs: Option<(PathBuf, Option<f64>)>,
    /// Track meters, shared by the views (reading a meter takes its peaks), and when they were
    /// last read.
    levels: HashMap<TrackId, crate::widgets::MeterState>,
    levels_read: std::time::Instant,
}

/// Automation being written for one control.
#[derive(Debug)]
struct Writer {
    mode: AutomationMode,
    value: f32,
    held: bool,
    /// Project time of the last written point.
    last: u64,
    start: u64,
    /// The lane before writing (return point).
    original: Vec<AutomationPoint>,
    /// Control value when the touch started (Trim).
    base: f32,
}

/// Delay after a Touch release before returning to the existing automation.
const RETURN_SECONDS: f64 = 0.05;

/// Range used to clamp trimmed values.
fn target_range(target: &AutomationTarget) -> (f32, f32) {
    match target {
        AutomationTarget::Gain | AutomationTarget::Send(_) => (-120.0, 24.0),
        AutomationTarget::Pan => (-1.0, 1.0),
        AutomationTarget::Mute => (0.0, 1.0),
        AutomationTarget::Param { .. } => (f32::MIN, f32::MAX),
    }
}

/// The automation lanes of a channel.
pub(crate) fn lanes_of(project: &Project, channel: Channel) -> &[AutomationLane] {
    match channel {
        Channel::Master => &project.master.automation,
        Channel::Track(id) => project.track(id).map_or(&[], |t| t.automation.as_slice()),
    }
}

/// The lane of `target` on a channel, created if missing.
pub(crate) fn lane_in<'a>(
    project: &'a mut Project,
    channel: Channel,
    target: &AutomationTarget,
) -> Option<&'a mut AutomationLane> {
    let lanes = match channel {
        Channel::Master => &mut project.master.automation,
        Channel::Track(id) => &mut project.track_mut(id)?.automation,
    };
    if let Some(i) = lanes.iter().position(|l| &l.target == target) {
        return lanes.get_mut(i);
    }
    lanes.push(AutomationLane::new(target.clone()));
    lanes.last_mut()
}

impl Document {
    fn with(project: Project, paths: Option<ProjectPaths>, dirs: AppDirs, dirty: bool) -> Self {
        let mut history = History::new(Project::default());
        history.replace(project, dirty);
        let untitled_autosave = dirs.recovery.as_ref().map(|dir| {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            dir.join(format!(
                "untitled-{stamp}-{}.{}",
                std::process::id(),
                file::EXTENSION
            ))
        });
        let peaks = paths
            .as_ref()
            .map(ProjectPaths::peaks_dir)
            .or_else(|| dirs.peaks.clone());
        let mut doc = Self {
            autosaver: Autosaver::new(
                AUTOSAVE_INTERVAL,
                if dirty { u64::MAX } else { history.revision() },
            ),
            history,
            paths,
            dirs,
            untitled_autosave,
            media: MediaPool::new(48_000, peaks),
            player: ProjectPlayer::new(EngineConfig::default()),
            imports: Vec::new(),
            synced: None,
            messages: Vec::new(),
            preview: None,
            revealed_clip: None,
            recording: None,
            writers: HashMap::new(),
            held: HashMap::new(),
            was_playing: false,
            exporting: None,
            stream_inputs: 0,
            track_selection: Vec::new(),
            render_queue: Vec::new(),
            learning: None,
            controllers: audiostrada_midi::live::LiveMidi::default().reader(),
            archiving: None,
            reference_lufs: None,
            levels: HashMap::new(),
            levels_read: std::time::Instant::now(),
        };
        doc.controllers = doc.player.live_midi().reader();
        doc.request_media();
        doc
    }

    /// Asks the main window to select a clip and open it in the piano roll.
    pub fn reveal_clip(&mut self, id: ClipId) {
        self.revealed_clip = Some(id);
    }

    pub fn take_revealed_clip(&mut self) -> Option<ClipId> {
        self.revealed_clip.take()
    }

    pub fn new(dirs: AppDirs) -> Self {
        Self::with(Project::default(), None, dirs, false)
    }

    pub fn open(path: &Path, dirs: AppDirs) -> Result<Self, ProjectError> {
        let project = file::load(path)?;
        Ok(Self::with(
            project,
            Some(ProjectPaths::new(path)),
            dirs,
            false,
        ))
    }

    /// Opens the autosave of a project in place of the saved file, keeping the file location.
    pub fn recover(
        autosave: &Path,
        project_file: Option<&Path>,
        dirs: AppDirs,
    ) -> Result<Self, ProjectError> {
        let project = file::load(autosave)?;
        let mut doc = Self::with(project, project_file.map(ProjectPaths::new), dirs, true);
        if project_file.is_none() {
            doc.untitled_autosave = Some(autosave.to_path_buf());
        }
        Ok(doc)
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    pub fn project(&self) -> &Project {
        self.history.project()
    }

    pub fn paths(&self) -> Option<&ProjectPaths> {
        self.paths.as_ref()
    }

    pub fn media(&self) -> &MediaPool {
        &self.media
    }

    pub fn player(&self) -> &ProjectPlayer {
        &self.player
    }

    pub fn player_mut(&mut self) -> &mut ProjectPlayer {
        &mut self.player
    }

    /// A new project nothing was done in yet: opening another one can take its place.
    pub fn is_blank(&self) -> bool {
        self.paths.is_none() && !self.history.is_dirty() && self.project().tracks.is_empty()
    }

    /// Sends the project to the background, another tab taking the stream: stops playback,
    /// auditions and the plugins' own windows, returning those to open again on return.
    pub fn park(&mut self) -> Vec<(Channel, audiostrada_core::InsertId)> {
        self.preview(None);
        self.player.engine_mut().stop();
        self.player.close_plugin_windows()
    }

    /// Window title: project name, with a mark when there are unsaved changes.
    pub fn title(&self) -> String {
        let name = self
            .paths
            .as_ref()
            .and_then(|p| p.file.file_stem())
            .map_or_else(
                || self.project().name.clone(),
                |s| s.to_string_lossy().into_owned(),
            );
        format!("{name}{}", if self.history.is_dirty() { " *" } else { "" })
    }

    pub fn selected_tracks(&self) -> &[TrackId] {
        &self.track_selection
    }

    /// The track last selected, which single-track commands act on.
    pub fn focused_track(&self) -> Option<TrackId> {
        self.track_selection.last().copied()
    }

    pub fn is_track_selected(&self, id: TrackId) -> bool {
        self.track_selection.contains(&id)
    }

    /// Selects a track: alone, toggled in the selection (Ctrl) or extended up to it from the
    /// focused track in the shown order (Shift).
    pub fn select_track(&mut self, id: TrackId, mode: SelectMode) {
        match mode {
            SelectMode::Only => self.track_selection = vec![id],
            SelectMode::Toggle => {
                if let Some(i) = self.track_selection.iter().position(|&t| t == id) {
                    self.track_selection.remove(i);
                } else {
                    self.track_selection.push(id);
                }
            }
            SelectMode::Extend => {
                let order: Vec<TrackId> = audiostrada_core::routing::visible_tracks(self.project())
                    .into_iter()
                    .map(|(i, _)| self.project().tracks[i].id)
                    .collect();
                let from = self
                    .focused_track()
                    .and_then(|f| order.iter().position(|&t| t == f));
                let to = order.iter().position(|&t| t == id);
                match (from, to) {
                    (Some(a), Some(b)) => {
                        let range = if a <= b { a..=b } else { b..=a };
                        for t in &order[range] {
                            if !self.track_selection.contains(t) {
                                self.track_selection.push(*t);
                            }
                        }
                        self.track_selection.retain(|&t| t != id);
                        self.track_selection.push(id);
                    }
                    _ => self.track_selection = vec![id],
                }
            }
        }
    }

    pub fn set_track_selection(&mut self, tracks: Vec<TrackId>) {
        self.track_selection = tracks;
    }

    /// Reads the track meters, once for every view drawn in the same frame.
    pub fn update_levels(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.levels_read).as_secs_f32();
        if elapsed < 0.004 {
            return;
        }
        self.levels_read = now;
        let project = self.history.project();
        self.levels.retain(|id, _| project.track(*id).is_some());
        for track in &project.tracks {
            let Some(output) = self.player.track_meter(track.id) else {
                continue;
            };
            let level = self.levels.entry(track.id).or_default();
            // An armed audio track shows its input, heard or not, to set its level.
            match self.player.track_input_meter(track.id) {
                Some(input) if track.armed && track.kind == TrackKind::Audio => {
                    level.take(input, elapsed);
                    for ch in 0..output.channels() {
                        output.take_peak(ch);
                    }
                }
                _ => level.take(output, elapsed),
            }
        }
    }

    pub fn track_level(&self, id: TrackId) -> crate::widgets::MeterState {
        self.levels.get(&id).copied().unwrap_or_default()
    }

    /// Keeps what a view changed in a meter (its clip light cleared).
    pub fn set_track_level(&mut self, id: TrackId, level: crate::widgets::MeterState) {
        if let Some(l) = self.levels.get_mut(&id) {
            l.clipped = level.clipped;
        }
    }

    pub fn set_stream_inputs(&mut self, inputs: usize) {
        self.stream_inputs = inputs;
    }

    pub fn stream_inputs(&self) -> usize {
        self.stream_inputs
    }

    /// Shows a short message to the user.
    pub fn notify(&mut self, message: String) {
        self.messages.push(message);
    }

    /// Takes the messages produced since the last call (import results, autosave errors).
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// Absolute path of a project media path.
    pub fn resolve(&self, media: &Path) -> PathBuf {
        self.paths
            .as_ref()
            .map_or_else(|| media.to_path_buf(), |p| p.resolve(media))
    }

    fn request_media(&mut self) {
        let mut wanted = media_paths(self.project(), self.paths.as_ref());
        wanted.extend(self.imports.iter().map(|i| i.path.clone()));
        wanted.extend(self.preview.iter().map(|p| p.0.clone()));
        self.media.retain(&wanted);
    }

    /// Auditions a file from its start (decoded in the background), or stops with `None`.
    pub fn preview(&mut self, file: Option<&Path>) {
        self.player.preview(None);
        self.preview = file.map(|f| (f.to_path_buf(), true));
        if let Some(f) = file {
            self.media.request(f);
        }
    }

    /// The file being auditioned.
    pub fn previewing(&self) -> Option<&Path> {
        match &self.preview {
            Some((path, true)) => Some(path),
            Some((path, false)) if self.player.previewing() => Some(path),
            _ => None,
        }
    }

    fn poll_preview(&mut self) {
        let Some((path, true)) = self.preview.clone() else {
            return;
        };
        match self.media.state(&path) {
            Some(audiostrada_engine::media::MediaState::Ready) => {
                let data = self.media.audio(&path).cloned();
                self.player.preview(data);
                self.preview = Some((path, false));
            }
            Some(audiostrada_engine::media::MediaState::Failed(e)) => {
                self.messages.push(trf!(
                    "Cannot play {path}: {error}",
                    path = path.display(),
                    error = e
                ));
                self.preview = None;
            }
            _ => {}
        }
    }

    /// Starts importing audio files; each becomes a new track once decoded.
    pub fn import(&mut self, files: &[PathBuf]) {
        self.import_to(files, None);
    }

    /// Imports files at `position`: the first on `track` (or a new track), the others on new
    /// tracks.
    pub fn import_at(&mut self, files: &[PathBuf], track: Option<TrackId>, position: u64) {
        self.import_to(files, Some((track, position)));
    }

    fn import_to(&mut self, files: &[PathBuf], target: Option<(Option<TrackId>, u64)>) {
        for (i, file) in files.iter().enumerate() {
            let target = target.map(|(track, position)| (track.filter(|_| i == 0), position));
            let (path, stored) = match &self.paths {
                Some(paths) => match file::import_media(paths, file) {
                    Ok(stored) => (paths.resolve(&stored), stored),
                    Err(e) => {
                        self.messages.push(trf!(
                            "Cannot copy {path}: {error}",
                            path = file.display(),
                            error = e
                        ));
                        continue;
                    }
                },
                None => (file.clone(), file.clone()),
            };
            self.media.request(&path);
            self.imports.push(Import {
                path,
                stored,
                target,
            });
        }
    }

    pub fn importing(&self) -> usize {
        self.imports.len()
    }

    /// Adds decoded imports to the project, one undo step each, in the order requested.
    fn finish_imports(&mut self) {
        let mut pending = Vec::new();
        for import in std::mem::take(&mut self.imports) {
            if !pending.is_empty() {
                pending.push(import);
                continue;
            }
            match self.media.state(&import.path) {
                Some(MediaState::Ready) => {
                    let Some(data) = self.media.original(&import.path) else {
                        continue;
                    };
                    let source = Source {
                        path: import.stored.clone(),
                        channels: data.channels() as u16,
                        sample_rate: data.sample_rate(),
                        frames: data.frames() as u64,
                    };
                    let name = import
                        .path
                        .file_stem()
                        .map_or_else(|| "Audio".to_owned(), |s| s.to_string_lossy().into_owned());
                    let target = import.target;
                    let source_channels = source.channels;
                    self.history.edit(&trf!("Import {name}", name = name), |p| {
                        let source = p.add_source(source);
                        let (track, position) = match target {
                            Some((Some(track), position)) if p.track(track).is_some() => {
                                (track, position)
                            }
                            Some((_, position)) => (p.add_track(&name), position),
                            None => (p.add_track(&name), 0),
                        };
                        // A stereo file makes a new (or still empty) track stereo.
                        if source_channels > 1
                            && let Some(t) = p.track_mut(track)
                            && t.clips.is_empty()
                        {
                            t.stereo = true;
                        }
                        if let Some(clip) = p.add_clip(track, source, position) {
                            edit::resolve_overlaps(p, track, &[clip]);
                        }
                    });
                    self.messages
                        .push(trf!("Imported {path}", path = import.path.display()));
                }
                Some(MediaState::Failed(e)) => {
                    self.messages.push(trf!(
                        "Cannot import {path}: {error}",
                        path = import.path.display(),
                        error = e
                    ));
                }
                _ => pending.push(import),
            }
        }
        self.imports = pending;
    }

    /// Sources whose media could not be loaded, with the absolute path and reason.
    pub fn missing(&self) -> Vec<(SourceId, PathBuf, String)> {
        self.project()
            .sources
            .iter()
            .filter_map(|(id, s)| {
                let path = self.resolve(&s.path);
                match self.media.state(&path) {
                    Some(MediaState::Failed(reason)) => Some((*id, path, reason.clone())),
                    _ => None,
                }
            })
            .collect()
    }

    /// Points a source at another file (undoable).
    pub fn relink(&mut self, source: SourceId, file: &Path) {
        let stored = match &self.paths {
            Some(paths) => match file::import_media(paths, file) {
                Ok(stored) => stored,
                Err(e) => {
                    self.messages.push(trf!(
                        "Cannot copy {path}: {error}",
                        path = file.display(),
                        error = e
                    ));
                    return;
                }
            },
            None => file.to_path_buf(),
        };
        self.media.forget(&self.resolve(&stored));
        self.history.edit(tr("Relink media"), |p| {
            if let Some(s) = p.sources.get_mut(&source) {
                s.path = stored;
            }
        });
    }

    /// Records the armed tracks from the playhead (inside the punch range if set) and starts
    /// playback. `inputs` is the stream's input count; `offset` moves takes earlier.
    pub fn start_recording(&mut self, inputs: usize, offset: i64) -> Result<(), String> {
        if self.recording.is_some() {
            return Ok(());
        }
        let project = self.history.project().clone();
        let armed_all: Vec<_> = project.tracks.iter().filter(|t| t.armed).collect();
        if armed_all.is_empty() {
            return Err(tr("Arm a track with its R button to record on it.").into());
        }
        let (midi_tracks, armed): (Vec<_>, Vec<_>) = armed_all
            .into_iter()
            .partition(|t| self.is_instrument_track(t.id));
        if inputs == 0 && !armed.is_empty() {
            return Err(
                tr("The audio stream has no inputs. Enable some in Preferences › Audio.").into(),
            );
        }
        let mut targets = Vec::new();
        for track in &armed {
            let first = usize::from(track.input);
            let channels: Vec<usize> = if track.stereo {
                vec![first, first + 1]
            } else {
                vec![first]
            };
            if let Some(missing) = channels.iter().find(|&&c| c >= inputs) {
                return Err(trf!(
                    "Track \"{name}\" records input {input}, but the stream has {inputs} input(s).",
                    name = track.name,
                    input = missing + 1,
                    inputs = inputs
                ));
            }
            targets.push(TakeTarget {
                track: track.id,
                name: track.name.clone(),
                inputs: channels,
            });
        }
        let dir = match (&self.paths, &self.dirs.recordings) {
            (Some(paths), _) => paths.audio_dir(),
            (None, Some(dir)) => dir.clone(),
            (None, None) => std::env::temp_dir().join("audiostrada-recordings"),
        };
        let mut punch = project.punch_range;
        let tracks = armed.iter().chain(&midi_tracks).map(|t| t.id).collect();
        let position = self.player.position();
        let start = punch.map_or(position, |(a, _)| position.max(a));
        let count_in = project.metronome.count_in;
        if count_in > 0 && !self.player.engine().playing() {
            // Start earlier and only record from where the playhead was.
            let tempo = &project.tempo;
            let rate = project.sample_rate;
            let beat = tempo.sample_to_beat(position, rate);
            let bars = f64::from(count_in) * tempo.signature_at_beat(beat).beats_per_bar();
            let from = tempo.beat_to_sample((beat - bars).max(0.0), rate);
            punch = Some(punch.map_or((start, u64::MAX), |(_, b)| (start, b)));
            self.player.locate(from);
        }
        let recorder = if targets.is_empty() {
            None
        } else {
            Some(self.player.start_recording(targets, dir, punch)?)
        };
        let midi = (!midi_tracks.is_empty()).then(|| MidiTake {
            reader: self.player.live_midi().reader(),
            events: Vec::new(),
            tracks: midi_tracks.iter().map(|t| t.id).collect(),
            passes: 0,
            clips: Vec::new(),
            notes: 0,
            last: None,
        });
        self.player.set_recording_click(true);
        self.player.engine_mut().play();
        self.recording = Some(Recording {
            recorder,
            midi,
            start,
            tracks,
            offset,
            seen_playing: false,
            stopping: false,
        });
        Ok(())
    }

    /// Stops capturing; takes are added to the project once their files are closed.
    pub fn stop_recording(&mut self) {
        if let Some(recording) = &mut self.recording
            && !recording.stopping
        {
            recording.stopping = true;
            self.player.set_recording_click(false);
            if recording.recorder.is_some() {
                self.player.stop_recording();
            }
            if let Some(take) = recording.midi.take() {
                let start = recording.start;
                match self.project().looping() {
                    Some((_, b)) if take.passes > 0 => {
                        let end = self.player.position().min(b);
                        if let Some(r) = &mut self.recording {
                            r.midi = Some(take);
                        }
                        self.merge_midi_pass(end, true);
                    }
                    _ => self.finish_midi_take(take, start),
                }
            }
        }
    }

    /// True when the first insert of a track is an instrument: it records MIDI.
    pub fn is_instrument_track(&self, id: TrackId) -> bool {
        let Some(first) = self.project().track(id).and_then(|t| t.inserts.first()) else {
            return false;
        };
        audiostrada_dsp::info(&first.effect).is_some_and(|i| i.instrument)
            || self
                .player
                .plugins()
                .find(&first.effect)
                .is_some_and(|d| d.instrument)
    }

    /// Turns recorded MIDI into a clip on each recorded instrument track.
    fn finish_midi_take(&mut self, mut take: MidiTake, start: u64) {
        let player = &self.player;
        take.reader
            .drain(|data, position| take.events.push((data, player.to_project(position))));
        let end = self.player.position().max(start + 1);
        // Notes played during a count-in are dropped, except a slightly early first note.
        let early = u64::from(self.project().sample_rate) / 10;
        let (notes, controls) = midi_from_events(&take.events, start, end, early);
        if notes.is_empty() && controls.is_empty() {
            self.messages
                .push(tr("No MIDI was received while recording").into());
            return;
        }
        let tracks = take.tracks;
        let count = notes.len();
        self.history.edit(tr("Record MIDI"), |p| {
            for track in tracks {
                let name = p.track(track).map_or_else(String::new, |t| t.name.clone());
                let clip = midi_clip(name, start, end - start, notes.clone(), controls.clone());
                edit::place_take(p, track, clip);
            }
        });
        self.messages
            .push(trf!("Recorded {count} MIDI note(s)", count = count));
    }

    /// Loop recording: when the playhead jumps back to the loop start, merges the pass just
    /// played into the take, so it plays along on the next pass.
    fn poll_midi_loop(&mut self) {
        let Some((a, b)) = self.project().looping() else {
            return;
        };
        let position = self.player.position();
        let Some(take) = self
            .recording
            .as_mut()
            .filter(|r| !r.stopping)
            .and_then(|r| r.midi.as_mut())
        else {
            return;
        };
        let wrapped = take.last.is_some_and(|last| position + (b - a) / 2 < last);
        take.last = Some(position);
        if wrapped {
            self.merge_midi_pass(b, false);
        }
    }

    /// Adds one loop pass, ending at `end`, to the MIDI clip of each recorded track (created at
    /// the first pass with notes, over the loop). The last pass also ends the recording.
    fn merge_midi_pass(&mut self, end: u64, last: bool) {
        let Some((a, b)) = self.project().looping() else {
            return;
        };
        let rate = self.project().sample_rate;
        let Some(recording) = &mut self.recording else {
            return;
        };
        let start = recording.start;
        let Some(take) = &mut recording.midi else {
            return;
        };
        let player = &self.player;
        take.reader
            .drain(|data, position| take.events.push((data, player.to_project(position))));
        // Messages after the jump back belong to the next pass.
        let split = if last {
            take.events.len()
        } else {
            take.events
                .windows(2)
                .position(|w| w[1].1 + (b - a) / 2 < w[0].1)
                .map_or(take.events.len(), |i| i + 1)
        };
        let pass: Vec<_> = take.events.drain(..split).collect();
        let first = take.passes == 0;
        take.passes += 1;
        let from = if first { start } else { a };
        let early = if first { u64::from(rate) / 10 } else { 0 };
        let (notes, controls) = midi_from_events(&pass, from, end, early);
        take.notes += notes.len();
        let clip_start = start.min(a);
        let shift = from - clip_start;
        let notes: Vec<Note> = notes
            .into_iter()
            .map(|n| Note {
                start: n.start + shift,
                ..n
            })
            .collect();
        let controls: Vec<audiostrada_core::ControlPoint> = controls
            .into_iter()
            .map(|c| audiostrada_core::ControlPoint {
                time: c.time + shift,
                ..c
            })
            .collect();
        let tracks = take.tracks.clone();
        let mut clips = take.clips.clone();
        let total = take.notes;
        if !notes.is_empty() || !controls.is_empty() {
            self.history
                .edit_merged(tr("Record MIDI"), "midi-loop-record", |p| {
                    for track in tracks {
                        let existing = clips.iter().find(|c| c.0 == track).map(|c| c.1);
                        let clip = existing.and_then(|id| {
                            p.track_mut(track)?.clips.iter_mut().find(|c| c.id == id)
                        });
                        match clip {
                            Some(clip) => {
                                let all =
                                    std::sync::Arc::make_mut(clip.notes.get_or_insert_default());
                                all.extend(notes.iter().cloned());
                                all.sort_by_key(|n| (n.start, n.key));
                                if !controls.is_empty() {
                                    let all = std::sync::Arc::make_mut(
                                        clip.controls.get_or_insert_default(),
                                    );
                                    all.extend(controls.iter().cloned());
                                    all.sort_by_key(|c| c.time);
                                }
                            }
                            None => {
                                let name =
                                    p.track(track).map_or_else(String::new, |t| t.name.clone());
                                let clip = midi_clip(
                                    name,
                                    clip_start,
                                    b - clip_start,
                                    notes.clone(),
                                    controls.clone(),
                                );
                                let id = edit::place_take(p, track, clip);
                                clips.retain(|c| c.0 != track);
                                clips.push((track, id));
                            }
                        }
                    }
                });
        }
        if let Some(take) = self.recording.as_mut().and_then(|r| r.midi.as_mut()) {
            take.clips = clips;
        }
        if last {
            if let Some(r) = &mut self.recording {
                r.midi = None;
            }
            self.messages.push(if total == 0 {
                tr("No MIDI was received while recording").into()
            } else {
                trf!("Recorded {count} MIDI note(s) in loop", count = total)
            });
        }
    }

    /// Waveforms of the takes being recorded: track, start and length of a peak (project
    /// samples), and the peaks.
    pub fn live_recording(&self, mut each: impl FnMut(TrackId, u64, f64, &[f32])) {
        let Some(recorder) = self.recording.as_ref().and_then(|r| r.recorder.as_ref()) else {
            return;
        };
        let rate = self.player.engine().sample_rate().max(1);
        let block = f64::from(audiostrada_engine::record::LIVE_BLOCK)
            * f64::from(self.project().sample_rate)
            / f64::from(rate);
        recorder
            .live(|track, start, peaks| each(track, self.player.to_project(start), block, peaks));
    }

    /// Start position and tracks of the recording in progress.
    pub fn recording(&self) -> Option<(u64, &[TrackId])> {
        self.recording
            .as_ref()
            .filter(|r| !r.stopping)
            .map(|r| (r.start, r.tracks.as_slice()))
    }

    fn poll_recording(&mut self) {
        let playing = self.player.engine().playing();
        let Some(recording) = &mut self.recording else {
            return;
        };
        recording.seen_playing |= playing;
        if !recording.stopping && recording.seen_playing && !playing {
            self.stop_recording();
            return;
        }
        self.poll_midi_loop();
        let Some(recording) = &mut self.recording else {
            return;
        };
        if !recording.stopping
            || !recording
                .recorder
                .as_ref()
                .is_none_or(Recorder::is_finished)
        {
            return;
        }
        let Some(recording) = self.recording.take() else {
            return;
        };
        let offset = recording.offset;
        let Some(recorder) = recording.recorder else {
            return;
        };
        let result = recorder.finish();
        self.messages.extend(
            result
                .errors
                .iter()
                .map(|e| trf!("Recording error: {error}", error = e)),
        );
        if result.dropped > 0 {
            self.messages.push(trf!(
                "{count} frames were lost while recording: the disk did not keep up.",
                count = result.dropped
            ));
        }
        if result.takes.is_empty() {
            return;
        }
        let rate = self.player.engine().sample_rate();
        let takes: Vec<(TrackId, Source, Clip)> = result
            .takes
            .iter()
            .map(|take| {
                let stored = self
                    .paths
                    .as_ref()
                    .map_or_else(|| take.path.clone(), |p| p.relativize(&take.path));
                let name = take
                    .path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let source = Source {
                    path: stored,
                    channels: take.channels,
                    sample_rate: rate,
                    frames: take.frames,
                };
                let start = self.player.to_project(take.start) as i64 - offset;
                let trim = (-start).max(0) as u64;
                let clip = Clip {
                    id: ClipId(0),
                    source: SourceId(0),
                    name,
                    start: start.max(0) as u64,
                    offset: trim,
                    length: self.player.to_project(take.frames).saturating_sub(trim),
                    gain_db: 0.0,
                    fade_in: 0,
                    fade_out: 0,
                    fade_shape: Default::default(),
                    fade_out_shape: None,
                    muted: false,
                    stretch: 1.0,
                    pitch: 0.0,
                    controls: None,
                    notes: None,
                };
                (take.track, source, clip)
            })
            .collect();
        let count = takes.len();
        self.history.edit(tr("Record"), |p| {
            for (track, source, mut clip) in takes {
                if clip.length > 0 {
                    clip.source = p.add_source(source);
                    edit::place_take(p, track, clip);
                }
            }
        });
        self.messages
            .push(trf!("Recorded {count} take(s)", count = count));
    }

    pub fn automation_mode(&self, channel: Channel) -> AutomationMode {
        match channel {
            Channel::Master => AutomationMode::Read,
            Channel::Track(id) => self
                .project()
                .track(id)
                .map_or(AutomationMode::Off, |t| t.automation_mode),
        }
    }

    /// True when a lane drives this control.
    pub fn automation_active(&self, channel: Channel, target: &AutomationTarget) -> bool {
        self.automation_mode(channel) != AutomationMode::Off
            && lanes_of(self.project(), channel)
                .iter()
                .any(|l| &l.target == target && l.enabled && !l.points.is_empty())
    }

    /// Value to show for an automated control: held or written value, else the lane at the
    /// playhead. `None` when the control is not automated.
    pub fn automation_value(&self, channel: Channel, target: &AutomationTarget) -> Option<f32> {
        let key = (channel, target.clone());
        if let Some(w) = self.writers.get(&key) {
            return Some(w.value);
        }
        if let Some(&v) = self.held.get(&key) {
            return Some(v);
        }
        if !self.automation_active(channel, target) {
            return None;
        }
        let position = self.player.position();
        lanes_of(self.project(), channel)
            .iter()
            .find(|l| &l.target == target)
            .and_then(|l| l.value_at(position))
    }

    /// A control moved (`Some`) or was released (`None`). Returns true when automation took
    /// the change (written or overriding the lane); false means it is a static edit.
    pub fn automation_gesture(
        &mut self,
        channel: Channel,
        target: &AutomationTarget,
        value: Option<f32>,
    ) -> bool {
        let key = (channel, target.clone());
        let project = self.history.project().clone();
        let Some(value) = value else {
            if let Some(w) = self.writers.get_mut(&key) {
                w.held = false;
                if matches!(w.mode, AutomationMode::Touch | AutomationMode::Trim) {
                    self.finish_writer(&key, true);
                }
                return true;
            }
            if self.held.remove(&key).is_some() {
                self.player.set_override(&project, channel, target, None);
                return true;
            }
            return false;
        };
        if let Some(w) = self.writers.get_mut(&key) {
            w.value = value;
            w.held = true;
            self.player
                .set_override(&project, channel, target, Some(value));
            return true;
        }
        let mode = self.automation_mode(channel);
        let playing = self.player.engine().playing();
        let active = self.automation_active(channel, target);
        if playing && mode.writes() && (mode != AutomationMode::Trim || active) {
            let now = self.player.position();
            let original = lanes_of(&project, channel)
                .iter()
                .find(|l| &l.target == target)
                .map(|l| l.points.clone())
                .unwrap_or_default();
            if mode != AutomationMode::Trim {
                let before = value_at(&original, now).unwrap_or(value);
                self.history
                    .edit_merged(tr("Write automation"), "automation-write", |p| {
                        if let Some(lane) = lane_in(p, channel, target) {
                            lane.add(now, before, CurveShape::Linear);
                        }
                    });
            }
            self.writers.insert(
                key,
                Writer {
                    mode,
                    value,
                    held: true,
                    last: now,
                    start: now,
                    original,
                    base: value,
                },
            );
            self.player
                .set_override(&project, channel, target, Some(value));
            return true;
        }
        if active {
            self.held.insert(key, value);
            self.player
                .set_override(&project, channel, target, Some(value));
            return true;
        }
        false
    }

    /// Stops writing a control, adding a return to the previous automation if asked.
    fn finish_writer(&mut self, key: &(Channel, AutomationTarget), return_point: bool) {
        let Some(w) = self.writers.remove(key) else {
            return;
        };
        let (channel, target) = key;
        let rate = f64::from(self.project().sample_rate);
        let back = w.last + (RETURN_SECONDS * rate) as u64;
        let returned = (return_point && w.mode != AutomationMode::Trim)
            .then(|| value_at(&w.original, back))
            .flatten();
        let (min, max) = target_range(target);
        let tolerance = if max - min < 1e6 {
            (max - min) * 0.002
        } else {
            0.001
        };
        self.history
            .edit_merged(tr("Write automation"), "automation-write", |p| {
                if let Some(lane) = lane_in(p, *channel, target) {
                    if let Some(v) = returned {
                        lane.add(back, v, CurveShape::Linear);
                    }
                    lane.thin(w.start + 1, w.last.saturating_sub(1), tolerance);
                }
            });
        let project = self.history.project().clone();
        self.player.set_override(&project, *channel, target, None);
        if self.writers.is_empty() {
            self.history.end_gesture();
        }
    }

    /// Advances automation writing with the transport.
    fn poll_automation(&mut self) {
        let playing = self.player.engine().playing();
        let now = self.player.position();
        if playing && !self.was_playing {
            // Write mode: every lane of the track is written from the start of playback.
            let project = self.history.project().clone();
            for track in project
                .tracks
                .iter()
                .filter(|t| t.automation_mode == AutomationMode::Write)
            {
                for lane in track.automation.iter().filter(|l| l.enabled) {
                    let key = (Channel::Track(track.id), lane.target.clone());
                    if self.writers.contains_key(&key) {
                        continue;
                    }
                    let value = lane.value_at(now).unwrap_or(0.0);
                    self.writers.insert(
                        key,
                        Writer {
                            mode: AutomationMode::Write,
                            value,
                            held: false,
                            last: now,
                            start: now,
                            original: lane.points.clone(),
                            base: value,
                        },
                    );
                    self.player.set_override(
                        &project,
                        Channel::Track(track.id),
                        &lane.target,
                        Some(value),
                    );
                }
            }
        }
        if playing {
            let mut edits = Vec::new();
            for ((channel, target), w) in &mut self.writers {
                if now < w.last {
                    // Loop wrap: continue from the loop start.
                    w.last = now;
                    w.start = now;
                    continue;
                }
                if now > w.last {
                    edits.push((
                        *channel,
                        target.clone(),
                        w.mode,
                        w.last,
                        now,
                        w.value,
                        w.value - w.base,
                    ));
                    w.last = now;
                }
            }
            if !edits.is_empty() {
                self.history
                    .edit_merged(tr("Write automation"), "automation-write", |p| {
                        for (channel, target, mode, from, to, value, delta) in edits {
                            let (min, max) = target_range(&target);
                            if let Some(lane) = lane_in(p, channel, &target) {
                                if mode == AutomationMode::Trim {
                                    lane.trim(from + 1, to, delta, min, max);
                                } else {
                                    lane.write(from, to, value);
                                }
                            }
                        }
                    });
            }
        } else if self.was_playing {
            let keys: Vec<_> = self.writers.keys().cloned().collect();
            for key in keys {
                self.finish_writer(&key, false);
            }
            self.history.end_gesture();
        }
        self.was_playing = playing;
    }

    /// Sets the gain of audio clips so their peak reaches `target_db`.
    pub fn normalize_clips(&mut self, ids: &[ClipId], target_db: f32) {
        let project = self.project();
        let ratio = f64::from(self.media.rate()) / f64::from(project.sample_rate.max(1));
        let mut gains = Vec::new();
        for &id in ids {
            let Some(clip) = audiostrada_core::edit::clip(project, id).filter(|c| !c.is_midi())
            else {
                continue;
            };
            let Some(source) = project.sources.get(&clip.source) else {
                continue;
            };
            let Some(data) = self.media.audio(&self.resolve(&source.path)) else {
                continue;
            };
            let frame = |s: u64| ((s as f64 * ratio) as usize).min(data.frames());
            let (a, b) = (frame(clip.offset), frame(clip.offset + clip.length));
            let peak = (0..data.channels())
                .flat_map(|ch| data.channel(ch)[a..b].iter())
                .fold(0.0f32, |m, s| m.max(s.abs()));
            if peak > 1e-6 {
                gains.push((id, target_db - 20.0 * peak.log10()));
            }
        }
        if !gains.is_empty() {
            self.history.edit(tr("Normalize"), |p| {
                for (id, gain) in gains {
                    audiostrada_core::edit::set_clip_gain(p, id, gain);
                }
            });
        }
    }

    /// Gives a built-in effect a file (amp model, impulse response), copied into the project's
    /// Models folder when the project is saved; `None` removes it.
    pub fn set_effect_file(
        &mut self,
        channel: Channel,
        insert: audiostrada_core::InsertId,
        slot: &str,
        file: Option<&Path>,
    ) -> Result<(), String> {
        let stored = match (file, &self.paths) {
            (None, _) => None,
            (Some(file), Some(paths)) => {
                let dir = paths.models_dir();
                let target = dir.join(file.file_name().ok_or(tr("not a file"))?);
                if !file.starts_with(&dir) {
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    if file.is_dir() {
                        // A pack of captures: its files, one level deep.
                        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                        for entry in std::fs::read_dir(file)
                            .map_err(|e| e.to_string())?
                            .flatten()
                        {
                            let path = entry.path();
                            if path.is_file()
                                && let Some(name) = path.file_name()
                            {
                                std::fs::copy(&path, target.join(name))
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                    } else {
                        std::fs::copy(file, &target).map_err(|e| e.to_string())?;
                    }
                }
                Some(paths.relativize(&target))
            }
            (Some(file), None) => Some(file.to_path_buf()),
        };
        let label = if stored.is_some() {
            tr("Load file")
        } else {
            tr("Remove file")
        };
        self.history.edit(label, |p| {
            let inserts = match channel {
                Channel::Master => Some(&mut p.master.inserts),
                Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
            };
            if let Some(i) = inserts.and_then(|i| i.iter_mut().find(|i| i.id == insert)) {
                match stored {
                    Some(path) => {
                        i.files.insert(slot.to_owned(), path);
                    }
                    None => {
                        i.files.remove(slot);
                    }
                }
            }
        });
        Ok(())
    }

    /// Tempo of the source part a clip plays, before any stretch.
    pub fn detect_clip_tempo(&self, clip: ClipId) -> Option<f64> {
        let project = self.project();
        let c = audiostrada_core::edit::clip(project, clip).filter(|c| !c.is_midi())?;
        let source = project.sources.get(&c.source)?;
        let data = self.media.original(&self.resolve(&source.path))?;
        let ratio = f64::from(data.sample_rate()) / f64::from(project.sample_rate) / c.stretch;
        let frame = |s: u64| ((s as f64 * ratio) as usize).min(data.frames());
        let (a, b) = (frame(c.offset), frame(c.offset + c.length));
        let mono: Vec<f32> = (a..b)
            .map(|i| {
                (0..data.channels())
                    .map(|ch| data.channel(ch)[i])
                    .sum::<f32>()
            })
            .collect();
        audiostrada_dsp::tempo::detect_bpm(&mono, data.sample_rate())
    }

    /// Changes the metronome; not an undo step.
    pub fn set_metronome(&mut self, f: impl FnOnce(&mut audiostrada_core::MetronomeSettings)) {
        self.history.modify_unrecorded(|p| f(&mut p.metronome));
    }

    /// Imports a `.mid` file at the playhead: one instrument track (built-in Synth) per part.
    pub fn import_midi(&mut self, path: &Path) -> Result<usize, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let tracks = audiostrada_midi::file::import(&bytes)?;
        if tracks.is_empty() {
            return Err(tr("the file has no notes").into());
        }
        let position = self.player.position();
        let rate = f64::from(self.project().sample_rate);
        let count = tracks.len();
        self.history.edit(tr("Import MIDI"), |p| {
            for track in tracks {
                let notes: Vec<Note> = track
                    .notes
                    .iter()
                    .map(|n| Note {
                        start: (n.start * rate) as u64,
                        length: ((n.length * rate) as u64).max(1),
                        key: n.key,
                        velocity: n.velocity,
                        channel: n.channel,
                        ..Default::default()
                    })
                    .collect();
                let controls: Vec<audiostrada_core::ControlPoint> = track
                    .controls
                    .iter()
                    .filter_map(|&(time, data)| {
                        audiostrada_core::ControlPoint::from_message((time * rate) as u64, data)
                    })
                    .collect();
                let length = notes
                    .iter()
                    .map(|n| n.start + n.length)
                    .chain(controls.iter().map(|c| c.time + 1))
                    .max()
                    .unwrap_or(1);
                let t = p.add_track(&track.name);
                audiostrada_core::routing::add_insert(
                    p,
                    Channel::Track(t),
                    "synth",
                    Default::default(),
                );
                if let Some(clip) = p.add_midi_clip(t, position, length, notes) {
                    edit::set_controls(p, clip, controls);
                }
            }
        });
        Ok(count)
    }

    /// Writes the MIDI clips of `track` (or of every track) to a `.mid` file.
    pub fn export_midi(&self, path: &Path, track: Option<TrackId>) -> Result<(), String> {
        let project = self.project();
        let (tempo, rate) = (&project.tempo, project.sample_rate);
        let mut parts = Vec::new();
        for t in project
            .tracks
            .iter()
            .filter(|t| track.is_none_or(|id| id == t.id))
        {
            let mut notes = Vec::new();
            let mut controls = Vec::new();
            for clip in t.clips.iter().filter(|c| c.is_midi()) {
                let origin = clip.start.saturating_sub(clip.offset);
                for c in clip.controls.iter().flat_map(|c| c.iter()) {
                    let at = origin + c.time;
                    if at >= clip.start && at < clip.end() {
                        controls.push((tempo.sample_to_beat(at, rate), c.message()));
                    }
                }
                for n in clip.notes.iter().flat_map(|n| n.iter()) {
                    let on = origin + n.start;
                    if on < clip.start || on >= clip.end() {
                        continue;
                    }
                    let off = (on + n.length).min(clip.end());
                    let start = tempo.sample_to_beat(on, rate);
                    notes.push(audiostrada_midi::file::FileNote {
                        start,
                        length: tempo.sample_to_beat(off, rate) - start,
                        key: n.key,
                        velocity: n.velocity,
                        channel: n.channel,
                    });
                }
            }
            if !notes.is_empty() || !controls.is_empty() {
                parts.push(audiostrada_midi::file::FileTrack {
                    name: t.name.clone(),
                    notes,
                    controls,
                });
            }
        }
        if parts.is_empty() {
            return Err(tr("there are no MIDI notes to export").into());
        }
        let bytes = audiostrada_midi::file::export(&parts, tempo.bpm_at_beat(0.0))?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    /// Waits for the next MIDI controller moved to map it to a control.
    pub fn learn(&mut self, channel: Channel, target: AutomationTarget, min: f32, max: f32) {
        self.learning = Some((channel, target, min, max));
        self.messages
            .push(tr("MIDI learn: move a controller").into());
    }

    /// Controller mapped to a control: (channel, controller).
    pub fn mapping(&self, channel: Channel, target: &AutomationTarget) -> Option<(u8, u8)> {
        let track = match channel {
            Channel::Master => None,
            Channel::Track(t) => Some(t),
        };
        self.project()
            .midi_map
            .iter()
            .find(|m| m.track == track && &m.target == target)
            .map(|m| (m.channel, m.controller))
    }

    pub fn forget_mapping(&mut self, channel: Channel, target: &AutomationTarget) {
        let track = match channel {
            Channel::Master => None,
            Channel::Track(t) => Some(t),
        };
        self.history.edit(tr("Forget MIDI mapping"), |p| {
            p.midi_map
                .retain(|m| !(m.track == track && &m.target == target));
        });
    }

    /// Sets a control's own value (not automation), merging continuous changes.
    pub fn set_static(&mut self, channel: Channel, target: &AutomationTarget, value: f32) {
        let key = format!("control:{channel:?}:{target:?}");
        self.history
            .edit_merged(tr("Change control"), &key, |p| match (channel, target) {
                (Channel::Master, AutomationTarget::Gain) => p.master.gain_db = value,
                (Channel::Track(t), AutomationTarget::Gain) => {
                    if let Some(t) = p.track_mut(t) {
                        t.gain_db = value;
                    }
                }
                (Channel::Track(t), AutomationTarget::Pan) => {
                    if let Some(t) = p.track_mut(t) {
                        t.pan = value.clamp(-1.0, 1.0);
                    }
                }
                (Channel::Track(t), AutomationTarget::Mute) => {
                    if let Some(t) = p.track_mut(t) {
                        t.mute = value >= 0.5;
                    }
                }
                (Channel::Track(t), AutomationTarget::Send(to)) => {
                    if let Some(s) = p
                        .track_mut(t)
                        .and_then(|t| t.sends.iter_mut().find(|s| s.target == *to))
                    {
                        s.gain_db = value;
                    }
                }
                (_, AutomationTarget::Param { insert, param }) => {
                    audiostrada_core::routing::set_insert_param(p, channel, *insert, param, value);
                }
                _ => {}
            });
    }

    /// Applies MIDI controllers to their mapped controls, or maps one when learning.
    fn poll_controllers(&mut self) {
        let mut moves = Vec::new();
        self.controllers.drain(|data, _| {
            let event = audiostrada_midi::MidiEvent { time: 0, data };
            if let Some((controller, value)) = event.as_control() {
                moves.push((event.channel(), controller, value));
            }
        });
        for (midi_channel, controller, value) in moves {
            if let Some((channel, target, min, max)) = self.learning.take() {
                let track = match channel {
                    Channel::Master => None,
                    Channel::Track(t) => Some(t),
                };
                let mapping = audiostrada_core::MidiMapping {
                    channel: midi_channel,
                    controller,
                    track,
                    target: target.clone(),
                    min,
                    max,
                };
                self.history.edit(tr("MIDI learn"), |p| {
                    p.midi_map
                        .retain(|m| !(m.channel == midi_channel && m.controller == controller));
                    p.midi_map
                        .retain(|m| !(m.track == track && m.target == target));
                    p.midi_map.push(mapping);
                });
                self.messages.push(trf!(
                    "Mapped controller {controller} (channel {channel})",
                    controller = controller,
                    channel = midi_channel + 1
                ));
                continue;
            }
            let mapped: Vec<_> = self
                .project()
                .midi_map
                .iter()
                .filter(|m| m.channel == midi_channel && m.controller == controller)
                .cloned()
                .collect();
            if mapped.is_empty() {
                self.play_synth_controller(controller, value);
            }
            for m in mapped {
                let channel = m.track.map_or(Channel::Master, Channel::Track);
                let v = m.min + (m.max - m.min) * f32::from(value) / 127.0;
                if !self.automation_gesture(channel, &m.target, Some(v)) {
                    self.set_static(channel, &m.target, v);
                }
            }
        }
    }

    /// A keyboard's standard sound controllers on the built-in synth of the armed tracks, when
    /// no control was mapped to them.
    fn play_synth_controller(&mut self, controller: u8, value: u8) {
        let Some(&(_, id)) = SYNTH_CONTROLLERS.iter().find(|(cc, _)| *cc == controller) else {
            return;
        };
        let Some(param) =
            audiostrada_dsp::info("synth").and_then(|i| i.params.iter().find(|p| p.id == id))
        else {
            return;
        };
        let t = f32::from(value) / 127.0;
        // As the knob turns: times and frequencies on a logarithmic scale.
        let v = if matches!(param.unit, "Hz" | "ms") && param.min > 0.0 {
            param.min * (param.max / param.min).powf(t)
        } else {
            param.min + t * (param.max - param.min)
        };
        let synths: Vec<(TrackId, audiostrada_core::InsertId)> = self
            .project()
            .tracks
            .iter()
            .filter(|t| t.armed)
            .filter_map(|t| {
                let first = t.inserts.first()?;
                let mpe = first.params.get("mpe").is_some_and(|&m| m >= 0.5);
                // In MPE mode CC 74 shapes each note instead.
                (first.effect == "synth" && !first.bypass && !(mpe && controller == 74))
                    .then_some((t.id, first.id))
            })
            .collect();
        for (track, insert) in synths {
            let channel = Channel::Track(track);
            let target = AutomationTarget::Param {
                insert,
                param: id.to_owned(),
            };
            if !self.automation_gesture(channel, &target, Some(v)) {
                self.set_static(channel, &target, v);
            }
        }
    }

    /// Periodic work: media loading, imports, engine sync, autosave. Call every frame.
    pub fn update(&mut self) {
        let project = self.history.project();
        self.track_selection.retain(|&t| project.track(t).is_some());
        self.poll_controllers();
        self.poll_preview();
        self.poll_automation();
        self.poll_export();
        self.poll_archive();
        self.update_reference();
        self.poll_recording();
        let media_changed = self.media.poll();
        if media_changed {
            self.finish_imports();
        }
        let state = (self.history.revision(), self.media.generation());
        if self.synced != Some(state) {
            self.request_media();
            self.player
                .sync(self.history.project(), self.paths.as_ref(), &self.media);
            self.synced = Some((self.history.revision(), self.media.generation()));
        }
        self.player.update();
        self.apply_plugin_feedback();
        if self.player.take_resync() {
            self.synced = None;
        }
        let target = match &self.paths {
            Some(paths) => Some(paths.autosave()),
            None => self.untitled_autosave.clone(),
        };
        if let Some(target) = target
            && let Some(Err(e)) = self.autosaver.tick(&self.history, &target)
        {
            self.messages
                .push(trf!("Autosave failed: {error}", error = e));
        }
    }

    /// Creates the engine processor for a stream with the given format.
    pub fn processor(&mut self, sample_rate: u32, max_frames: usize) -> EngineProcessor {
        self.media.set_rate(sample_rate);
        let processor = self.player.processor(sample_rate, max_frames);
        self.synced = None;
        processor
    }

    /// Saves to the current file.
    /// Stores the current state of every plugin in the project.
    fn capture_plugin_states(&mut self) {
        let states = self.player.plugin_states();
        if states.is_empty() {
            return;
        }
        self.history.modify_unrecorded(|p| {
            for (channel, id, state) in states {
                let inserts = match channel {
                    Channel::Master => Some(&mut p.master.inserts),
                    Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
                };
                if let Some(insert) = inserts.and_then(|l| l.iter_mut().find(|i| i.id == id)) {
                    insert.state = Some(base64::encode(&state));
                }
            }
        });
    }

    /// Plugin parameter changes made in plugin windows go into the project.
    fn apply_plugin_feedback(&mut self) {
        let feedback = self.player.take_param_feedback();
        if feedback.is_empty() {
            return;
        }
        self.history.modify_unrecorded(|p| {
            for (channel, id, param, value) in feedback {
                let inserts = match channel {
                    Channel::Master => Some(&mut p.master.inserts),
                    Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
                };
                if let Some(insert) = inserts.and_then(|l| l.iter_mut().find(|i| i.id == id)) {
                    insert.params.insert(param.to_string(), value as f32);
                }
            }
        });
    }

    pub fn set_plugins(&mut self, database: Database) {
        self.player.set_plugins(database);
    }

    pub fn save(&mut self) -> Result<(), ProjectError> {
        let paths = self
            .paths
            .clone()
            .ok_or_else(|| ProjectError::Format("project has no file yet".into()))?;
        self.capture_plugin_states();
        file::save(self.project(), &paths)?;
        self.saved(&paths);
        Ok(())
    }

    /// Saves to a new location, copying media from outside the new folder into it.
    pub fn save_as(&mut self, paths: ProjectPaths) -> Result<(), ProjectError> {
        self.capture_plugin_states();
        let moved = file::collect_media(self.project(), &paths, self.paths.as_ref())?;
        let name = paths.file.file_stem().map_or_else(
            || self.project().name.clone(),
            |s| s.to_string_lossy().into_owned(),
        );
        self.history.modify_unrecorded(|p| {
            p.name = name;
            for (id, path) in moved {
                if let Some(s) = p.sources.get_mut(&id) {
                    s.path = path;
                }
            }
        });
        file::save(self.project(), &paths)?;
        if let Some(untitled) = self.untitled_autosave.take() {
            autosave::discard(&untitled);
        }
        self.media.set_peaks_dir(Some(paths.peaks_dir()));
        self.paths = Some(paths.clone());
        self.saved(&paths);
        self.synced = None;
        Ok(())
    }

    fn saved(&mut self, paths: &ProjectPaths) {
        self.history.mark_saved();
        self.autosaver.saved(self.history.revision());
        autosave::discard(&paths.autosave());
    }

    /// Removes the autosave when closing without saving changes the user discarded.
    pub fn discard_autosave(&mut self) {
        match &self.paths {
            Some(paths) => autosave::discard(&paths.autosave()),
            None => {
                if let Some(path) = &self.untitled_autosave {
                    autosave::discard(path);
                }
            }
        }
    }

    /// Imports still loading, media still decoding.
    pub fn busy(&self) -> bool {
        !self.imports.is_empty() || self.media.loading() > 0
    }

    /// Blocks until media and imports are loaded (command line use).
    pub fn wait_loaded(&mut self, timeout: Duration) -> bool {
        let end = std::time::Instant::now() + timeout;
        loop {
            self.update();
            if !self.busy() {
                return true;
            }
            if std::time::Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn dirs(&self) -> &AppDirs {
        &self.dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_media::{AudioData, write_wav};

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-doc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn import_save_reopen() {
        let dir = temp("cycle");
        let take = dir.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.1; 4800]], 48_000)).unwrap();
        let dirs = AppDirs {
            recovery: Some(dir.join("recovery")),
            peaks: Some(dir.join("peaks")),
            recordings: Some(dir.join("recordings")),
        };
        let mut doc = Document::new(dirs.clone());
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        assert_eq!(doc.project().tracks.len(), 1);
        assert_eq!(doc.history().undo_label(), Some("Import take"));
        assert!(doc.title().ends_with('*'));

        let paths = ProjectPaths::in_new_folder(&dir, "Song");
        doc.save_as(paths.clone()).unwrap();
        assert_eq!(doc.title(), "Song");
        assert!(
            paths.audio_dir().join("take.wav").exists(),
            "media collected into the project"
        );
        let source = doc.project().sources.values().next().unwrap();
        assert_eq!(source.path, PathBuf::from("Audio Files/take.wav"));

        let mut reopened = Document::open(&paths.file, dirs).unwrap();
        assert!(reopened.wait_loaded(Duration::from_secs(10)));
        assert!(reopened.missing().is_empty());
        assert_eq!(reopened.project().tracks.len(), 1);
        assert!(!reopened.history().is_dirty());
        assert!(reopened.dirs().recovery.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recording_adds_takes_with_the_latency_offset() {
        let dir = temp("record");
        let dirs = AppDirs {
            recordings: Some(dir.clone()),
            ..AppDirs::default()
        };
        let mut doc = Document::new(dirs);
        assert!(doc.start_recording(2, 0).is_err(), "nothing armed");
        doc.history_mut().edit("Track", |p| {
            let t = p.add_track("Vox");
            p.track_mut(t).unwrap().armed = true;
        });
        let mut processor = doc.processor(48_000, 64);
        assert!(doc.start_recording(0, 0).is_err(), "no inputs");
        doc.start_recording(2, 100).unwrap();
        let mut storage = audiostrada_io::ChannelBuffers::new(2, 2, 64);
        storage.input_mut(0).fill(0.5);
        for _ in 0..20 {
            processor.render(&mut storage.buffers(64));
            doc.update();
        }
        assert!(doc.recording().is_some());
        doc.stop_recording();
        processor.render(&mut storage.buffers(64));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while doc.recording.is_some() && std::time::Instant::now() < deadline {
            doc.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        let track = &doc.project().tracks[0];
        assert_eq!(track.clips.len(), 1);
        let clip = &track.clips[0];
        assert_eq!((clip.start, clip.offset), (0, 100));
        assert_eq!(clip.length, 20 * 64 - 100);
        assert_eq!(doc.history().undo_label(), Some("Record"));
        assert!(dir.join("Vox-01.wav").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn touch_writes_automation_in_one_undo_step() {
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().edit("Track", |p| {
            let t = p.add_track("Vox");
            p.track_mut(t).unwrap().automation_mode = AutomationMode::Touch;
        });
        let t = doc.project().tracks[0].id;
        let channel = Channel::Track(t);
        let mut processor = doc.processor(48_000, 64);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 64);
        assert!(
            !doc.automation_gesture(channel, &AutomationTarget::Gain, Some(-3.0)),
            "stopped: static edit"
        );
        doc.player_mut().engine_mut().play();
        let mut run = |doc: &mut Document, blocks: usize| {
            for _ in 0..blocks {
                processor.render(&mut storage.buffers(64));
                doc.update();
            }
        };
        run(&mut doc, 4);
        assert!(doc.automation_gesture(channel, &AutomationTarget::Gain, Some(-6.0)));
        run(&mut doc, 10);
        assert!(doc.automation_gesture(channel, &AutomationTarget::Gain, Some(-12.0)));
        run(&mut doc, 10);
        assert!(doc.automation_gesture(channel, &AutomationTarget::Gain, None));
        let lane = &doc.project().tracks[0].automation[0];
        assert_eq!(lane.target, AutomationTarget::Gain);
        assert!(lane.points.len() >= 2);
        assert_eq!(lane.points.last().unwrap().value, -12.0);
        doc.player_mut().engine_mut().stop();
        run(&mut doc, 2);
        assert_eq!(doc.history().undo_label(), Some("Write automation"));
        doc.history_mut().undo();
        assert!(
            doc.project().tracks[0]
                .automation
                .iter()
                .all(|l| l.points.is_empty())
        );
    }

    #[test]
    fn exports_mix_and_stems_then_freezes() {
        let dir = temp("export");
        let take = dir.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.25; 9600]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs {
            recordings: Some(dir.join("recordings")),
            ..AppDirs::default()
        });
        doc.import(&[take.clone(), take.clone()]);
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let settings = ExportSettings {
            format: audiostrada_media::FileFormat::Flac24,
            normalize: Normalize::Peak(-6.0206),
            tail: 0.1,
            stems: true,
            ..ExportSettings::default()
        };
        doc.start_export(settings, &dir.join("Song.wav")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
            doc.update();
        }
        let mix = audiostrada_media::load(&dir.join("Song.flac")).unwrap();
        assert_eq!(mix.frames(), 9600 + 4800);
        let peak = mix.channel(0).iter().fold(0.0f32, |p, s| p.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.001, "normalized to -6 dB: {peak}");
        assert!(dir.join("Song - take.flac").exists(), "stems");
        assert!(
            doc.take_messages()
                .iter()
                .any(|m| m.starts_with("Exported 3"))
        );

        let t = doc.project().tracks[0].id;
        doc.freeze(t).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
            doc.update();
        }
        let frozen = doc.project().tracks[0].frozen.expect("frozen");
        assert!(doc.project().sources.contains_key(&frozen.source));
        assert_eq!(doc.history().undo_label(), Some("Freeze track"));
        doc.unfreeze(t);
        assert!(doc.project().tracks[0].frozen.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn render_queue_keeps_the_project_as_queued() {
        let dir = temp("queue");
        let take = dir.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.25; 4800]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let wav = ExportSettings {
            format: audiostrada_media::FileFormat::Wav24,
            tail: 0.0,
            ..ExportSettings::default()
        };
        doc.queue_export(wav.clone(), &dir.join("Loud")).unwrap();
        let t = doc.project().tracks[0].id;
        doc.history_mut()
            .edit("Quieter", |p| p.track_mut(t).unwrap().gain_db = -6.0206);
        let flac = ExportSettings {
            format: audiostrada_media::FileFormat::Flac16,
            ..wav
        };
        doc.queue_export(flac, &dir.join("Quiet")).unwrap();
        assert_eq!(doc.queued_exports().len(), 2);
        doc.render_queue().unwrap();
        assert!(doc.queued_exports().is_empty());
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
            doc.update();
        }
        let peak = |name: &str| {
            let audio = audiostrada_media::load(&dir.join(name)).unwrap();
            audio.channel(0).iter().fold(0.0f32, |p, s| p.max(s.abs()))
        };
        assert!((peak("Loud.wav") - 0.25).abs() < 0.01);
        assert!((peak("Quiet.flac") - 0.125).abs() < 0.01);
        assert!(doc.render_queue().is_err(), "empty queue");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn records_midi_and_learns_controllers() {
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().edit("Keys", |p| {
            let t = p.add_track("Keys");
            audiostrada_core::routing::add_insert(
                p,
                Channel::Track(t),
                "synth",
                Default::default(),
            );
            p.track_mut(t).unwrap().armed = true;
        });
        let t = doc.project().tracks[0].id;
        assert!(doc.is_instrument_track(t));
        let mut processor = doc.processor(48_000, 64);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 64);
        doc.start_recording(0, 0).unwrap();
        let live = doc.player().live_midi();
        let mut run = |doc: &mut Document, blocks: usize| {
            for _ in 0..blocks {
                processor.render(&mut storage.buffers(64));
                doc.update();
            }
        };
        run(&mut doc, 10);
        live.push([0x90, 60, 100], 1000);
        live.push([0x80, 60, 0], 3000);
        live.push([0x90, 64, 90], 3200);
        run(&mut doc, 60);
        doc.stop_recording();
        run(&mut doc, 2);
        let clip = doc.project().tracks[0]
            .clips
            .first()
            .expect("MIDI clip")
            .clone();
        let notes = clip.notes.unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(
            (notes[0].key, notes[0].start, notes[0].length),
            (60, 1000, 2000)
        );
        assert_eq!(notes[1].key, 64, "held note closed at the stop");

        doc.learn(Channel::Track(t), AutomationTarget::Gain, -60.0, 12.0);
        live.push([0xB0, 7, 10], 0);
        run(&mut doc, 1);
        assert_eq!(
            doc.mapping(Channel::Track(t), &AutomationTarget::Gain),
            Some((0, 7))
        );
        live.push([0xB0, 7, 127], 0);
        run(&mut doc, 1);
        assert_eq!(doc.project().tracks[0].gain_db, 12.0);
    }

    #[test]
    fn notes_played_on_the_instruments_keyboard_are_recorded() {
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().edit("Keys", |p| {
            let t = p.add_track("Keys");
            audiostrada_core::routing::add_insert(
                p,
                Channel::Track(t),
                "synth",
                Default::default(),
            );
            p.track_mut(t).unwrap().armed = true;
        });
        let t = doc.project().tracks[0].id;
        let mut processor = doc.processor(48_000, 64);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 64);
        doc.start_recording(0, 0).unwrap();
        let mut run = |doc: &mut Document, blocks: usize| {
            for _ in 0..blocks {
                processor.render(&mut storage.buffers(64));
                doc.update();
            }
        };
        run(&mut doc, 10);
        assert!(doc.player().play_key(t, [0x90, 67, 100]));
        run(&mut doc, 20);
        assert!(doc.player().play_key(t, [0x80, 67, 0]));
        run(&mut doc, 10);
        doc.stop_recording();
        run(&mut doc, 2);
        let clip = doc.project().tracks[0]
            .clips
            .first()
            .expect("MIDI clip")
            .clone();
        let notes = clip.notes.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].key, 67);
        assert!(notes[0].length > 1000, "{}", notes[0].length);
    }

    #[test]
    fn keyboard_knobs_turn_the_synth_of_armed_tracks() {
        let mut doc = Document::new(AppDirs::default());
        let mut insert = None;
        doc.history_mut().edit("Keys", |p| {
            let t = p.add_track("Keys");
            insert = audiostrada_core::routing::add_insert(
                p,
                Channel::Track(t),
                "synth",
                Default::default(),
            );
        });
        let (t, insert) = (doc.project().tracks[0].id, insert.unwrap());
        let mut processor = doc.processor(48_000, 64);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 64);
        let live = doc.player().live_midi();
        let mut turn = |doc: &mut Document, cc: u8, value: u8| {
            live.push([0xB0, cc, value], 0);
            processor.render(&mut storage.buffers(64));
            doc.update();
        };
        let param =
            |doc: &Document, id: &str| doc.project().tracks[0].inserts[0].params.get(id).copied();
        turn(&mut doc, 74, 127);
        assert_eq!(param(&doc, "cutoff"), None, "not armed");
        doc.history_mut()
            .edit("Arm", |p| p.track_mut(t).unwrap().armed = true);
        turn(&mut doc, 74, 127);
        assert_eq!(param(&doc, "cutoff"), Some(18_000.0));
        turn(&mut doc, 74, 0);
        assert_eq!(param(&doc, "cutoff"), Some(40.0));
        turn(&mut doc, 71, 127);
        assert_eq!(param(&doc, "resonance"), Some(95.0));
        // A controller mapped by MIDI learn does only what it was mapped to.
        let target = AutomationTarget::Param {
            insert,
            param: "detune".into(),
        };
        doc.learn(Channel::Track(t), target, 0.0, 50.0);
        turn(&mut doc, 73, 0);
        turn(&mut doc, 73, 127);
        assert_eq!(param(&doc, "detune"), Some(50.0));
        assert_eq!(param(&doc, "attack"), None);
    }

    /// Times importing the file in AUDIOSTRADA_BENCH_WAV.
    #[test]
    fn import_speed() {
        let Some(file) = std::env::var_os("AUDIOSTRADA_BENCH_WAV") else {
            return;
        };
        let mut doc = Document::new(AppDirs::default());
        let rate = std::env::var("AUDIOSTRADA_BENCH_RATE").map_or(48_000, |r| r.parse().unwrap());
        let _processor = doc.processor(rate, 256);
        let started = std::time::Instant::now();
        doc.import(&[PathBuf::from(file)]);
        assert!(doc.wait_loaded(Duration::from_secs(120)));
        eprintln!("imported in {:?}", started.elapsed());
    }

    #[test]
    fn loop_recording_overdubs_each_pass() {
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().edit("Keys", |p| {
            let t = p.add_track("Keys");
            audiostrada_core::routing::add_insert(
                p,
                Channel::Track(t),
                "synth",
                Default::default(),
            );
            p.track_mut(t).unwrap().armed = true;
            p.loop_range = Some((0, 9600));
        });
        let mut processor = doc.processor(48_000, 64);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 64);
        doc.update();
        doc.start_recording(0, 0).unwrap();
        let live = doc.player().live_midi();
        let mut run = |doc: &mut Document, blocks: usize| {
            for _ in 0..blocks {
                processor.render(&mut storage.buffers(64));
                doc.update();
            }
        };
        run(&mut doc, 10);
        live.push([0x90, 60, 100], 1000);
        live.push([0x80, 60, 0], 2000);
        run(&mut doc, 160);
        let notes = |doc: &Document| {
            let clips = &doc.project().tracks[0].clips;
            assert_eq!(clips.len(), 1, "one clip over the loop");
            assert_eq!((clips[0].start, clips[0].length), (0, 9600));
            clips[0]
                .notes
                .as_ref()
                .unwrap()
                .iter()
                .map(|n| (n.key, n.start))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            notes(&doc),
            vec![(60, 1000)],
            "first pass plays on the second"
        );
        live.push([0x90, 64, 90], 5000);
        live.push([0x80, 64, 0], 6000);
        run(&mut doc, 150);
        assert_eq!(notes(&doc), vec![(60, 1000), (64, 5000)]);
        live.push([0x90, 67, 90], 300);
        run(&mut doc, 20);
        doc.stop_recording();
        run(&mut doc, 2);
        assert_eq!(notes(&doc), vec![(67, 300), (60, 1000), (64, 5000)]);
        assert!(
            doc.take_messages()
                .iter()
                .any(|m| m.contains("3 MIDI note"))
        );
        doc.history_mut().undo();
        assert!(doc.project().tracks[0].clips.is_empty(), "one undo step");
    }

    #[test]
    fn normalizes_bounces_and_exports_one_track() {
        let dir = temp("bounce");
        let take = dir.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.25; 9600]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs {
            recordings: Some(dir.join("recordings")),
            ..AppDirs::default()
        });
        doc.import(&[take.clone(), take]);
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let clip = doc.project().tracks[0].clips[0].id;
        doc.normalize_clips(&[clip], -6.0206);
        let gain = doc.project().tracks[0].clips[0].gain_db;
        assert!((gain - 6.0206).abs() < 0.01, "0.25 peak up to 0.5: {gain}");
        let run = |doc: &mut Document| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
                doc.update();
            }
        };
        let t = doc.project().tracks[0].id;
        let settings = ExportSettings {
            tail: 0.0,
            ..ExportSettings::default()
        };
        doc.export_track(t, settings, &dir.join("one.wav")).unwrap();
        run(&mut doc);
        let one = audiostrada_media::load(&dir.join("one.wav")).unwrap();
        let peak = one.channel(0).iter().fold(0.0f32, |p, s| p.max(s.abs()));
        assert!(
            (peak - 0.5).abs() < 0.01,
            "only the normalized track: {peak}"
        );

        doc.bounce(t).unwrap();
        run(&mut doc);
        let tracks = &doc.project().tracks;
        assert_eq!(tracks.len(), 3);
        assert!(tracks[0].mute, "original muted");
        assert_eq!(tracks[1].name, "take (bounce)");
        assert_eq!(tracks[1].clips.len(), 1);
    }

    #[test]
    fn strips_heals_consolidates_and_reverses() {
        let dir = temp("clip-ops");
        let take = dir.join("take.wav");
        // Loud, silent, quiet: 0.1 s each at 48 kHz, then the silence twice as long.
        let mut samples = vec![0.0f32; 19_200];
        samples[..4800].fill(0.5);
        samples[14_400..].fill(0.2);
        write_wav(&take, &AudioData::new(vec![samples], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs {
            recordings: Some(dir.join("recordings")),
            ..AppDirs::default()
        });
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let t = doc.project().tracks[0].id;
        let clip = doc.project().tracks[0].clips[0].id;
        let settings = StripSettings {
            pad_before_ms: 0.0,
            pad_after_ms: 0.0,
            ..StripSettings::default()
        };
        let found = doc.find_sound(&[clip], &settings);
        assert_eq!(found[0].1, [(0, 4800), (14_400, 19_200)]);
        doc.strip_silence(&found, audiostrada_core::edit::StripMode::Strip);
        assert_eq!(doc.project().tracks[0].clips.len(), 2);
        assert_eq!(doc.heal(&[t], 0, 19_200), 1);
        assert_eq!(doc.project().tracks[0].clips[0].length, 19_200);

        let run = |doc: &mut Document| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
                doc.update();
            }
        };
        doc.history_mut().edit("Split", |p| {
            audiostrada_core::edit::split(p, 9600, &[]);
        });
        doc.consolidate(&[(t, 0, 19_200)]).unwrap();
        run(&mut doc);
        let clips = &doc.project().tracks[0].clips;
        assert_eq!(clips.len(), 1, "one clip in place of two");
        assert_eq!((clips[0].start, clips[0].length), (0, 19_200));
        let source = clips[0].source;
        let id = clips[0].id;
        assert!(
            doc.project().sources[&source]
                .path
                .to_string_lossy()
                .contains("consolidated")
        );

        doc.process_clips(&[id], &Process::Reverse).unwrap();
        run(&mut doc);
        let c = &doc.project().tracks[0].clips[0];
        assert_ne!(c.source, source);
        let path = doc.project().sources[&c.source].path.clone();
        let reversed = audiostrada_media::load(&path).unwrap();
        let level = |i: usize| reversed.channel(0)[i].abs();
        assert!(
            (level(100) - 0.2).abs() < 0.01,
            "quiet part first: {}",
            level(100)
        );
        assert!((level(19_100) - 0.5).abs() < 0.01, "loud part last");
    }

    #[test]
    fn two_mono_tracks_become_one_stereo_track() {
        let dir = temp("make-stereo");
        let (left, right) = (dir.join("OH L.wav"), dir.join("OH R.wav"));
        write_wav(&left, &AudioData::new(vec![vec![0.5; 9600]], 48_000)).unwrap();
        write_wav(&right, &AudioData::new(vec![vec![0.25; 9600]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs {
            recordings: Some(dir.join("recordings")),
            ..AppDirs::default()
        });
        // The right one first: the names decide, not the order.
        doc.import(&[right, left]);
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let ids: Vec<TrackId> = doc.project().tracks.iter().map(|t| t.id).collect();
        doc.make_stereo(ids[0], ids[1]).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
            doc.update();
        }
        let tracks = &doc.project().tracks;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "OH");
        assert!(tracks[0].stereo);
        let clip = &tracks[0].clips[0];
        let source = &doc.project().sources[&clip.source];
        let audio = audiostrada_media::load(&doc.resolve(&source.path)).unwrap();
        let (l, r) = (audio.channel(0)[4800], audio.channel(1)[4800]);
        assert!((l - 0.5).abs() < 1e-3 && (r - 0.25).abs() < 1e-3, "{l} {r}");
        assert_eq!(
            doc.history_mut().undo().as_deref(),
            Some("Convert to a stereo track")
        );
        assert_eq!(doc.project().tracks.len(), 2);
        assert!(!doc.project().tracks[0].stereo, "mono files, mono tracks");
        // A stereo file makes a stereo track.
        let both = dir.join("pair.wav");
        write_wav(&both, &AudioData::new(vec![vec![0.1; 4800]; 2], 48_000)).unwrap();
        doc.import(std::slice::from_ref(&both));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        assert!(doc.project().tracks.last().unwrap().stereo);
    }

    #[test]
    fn track_names_tell_the_side() {
        use super::export::name_side as side;
        assert_eq!(side("Overhead Left"), Some((true, "Overhead".into())));
        assert_eq!(side("OH_R"), Some((false, "OH".into())));
        assert_eq!(side("Room (G)"), Some((true, "Room".into())));
        assert_eq!(side("Snare"), None);
        assert_eq!(side("Bell"), None);
    }

    #[test]
    fn beat_detective_puts_late_and_early_hits_on_the_grid() {
        let dir = temp("beat-detective");
        let take = dir.join("drums.wav");
        // At 120 BPM a beat is 24000 samples: hits a little late or early around beats 2 to 5.
        let at = [24_600usize, 47_400, 72_500, 95_700];
        let mut seed = 3u32;
        let mut samples = vec![0.0f32; 120_000];
        for &start in &at {
            for i in 0..4800 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
                samples[start + i] += noise * 0.8 * (-(i as f32) / 600.0).exp();
            }
        }
        write_wav(&take, &AudioData::new(vec![samples], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let t = doc.project().tracks[0].id;
        let hits = doc.find_hits(&[t], 0, 120_000, 0.5);
        assert_eq!(hits.len(), 4, "{hits:?}");
        doc.beat_detective(&[t], 0, 120_000, &hits, Some((Some(1.0), 1.0)));
        let starts: Vec<u64> = doc.project().tracks[0]
            .clips
            .iter()
            .map(|c| c.start)
            .collect();
        assert_eq!(starts, [0, 24_000, 48_000, 72_000, 96_000]);
        assert_eq!(
            doc.history_mut().undo().as_deref(),
            Some("Conform to the grid")
        );
        assert_eq!(doc.project().tracks[0].clips.len(), 1, "one undo step");
    }

    #[test]
    fn stretched_clips_play_their_rendered_variant() {
        let dir = temp("stretch");
        let take = dir.join("tone.wav");
        let tone: Vec<f32> = (0..24_000)
            .map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        write_wav(&take, &AudioData::new(vec![tone], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let clip = doc.project().tracks[0].clips[0].id;
        doc.history_mut().edit("Stretch", |p| {
            audiostrada_core::edit::stretch_clip(p, clip, 48_000)
        });
        let mut processor = doc.processor(48_000, 480);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 480);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let c = doc.project().tracks[0].clips[0].clone();
        let path = audiostrada_engine::project::clip_media(doc.project(), None, &c).unwrap();
        while doc.media().audio(&path).is_none() && std::time::Instant::now() < deadline {
            doc.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(doc.media().audio(&path).unwrap().frames(), 48_000);
        doc.update();
        doc.player_mut().locate(36_000);
        doc.player_mut().engine_mut().play();
        let mut energy = 0.0;
        for _ in 0..10 {
            processor.render(&mut storage.buffers(480));
            doc.update();
            energy += storage.output(0).iter().map(|s| s * s).sum::<f32>();
        }
        assert!(energy > 1.0, "plays past the original end: {energy}");
    }

    #[test]
    fn folders_freeze_with_their_content() {
        let dir = temp("folder-freeze");
        let take = dir.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.25; 4800]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs {
            recordings: Some(dir.join("recordings")),
            ..AppDirs::default()
        });
        doc.import(&[take.clone(), take]);
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let (a, b) = (doc.project().tracks[0].id, doc.project().tracks[1].id);
        let mut folder = None;
        doc.history_mut().edit("Folder", |p| {
            let f = p.add_track("Drums");
            audiostrada_core::routing::set_parent(p, a, Some(f)).unwrap();
            audiostrada_core::routing::set_parent(p, b, Some(f)).unwrap();
            folder = Some(f);
        });
        let folder = folder.unwrap();
        doc.freeze(folder).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while doc.export_progress().is_some() && std::time::Instant::now() < deadline {
            doc.update();
        }
        let frozen = doc
            .project()
            .track(folder)
            .unwrap()
            .frozen
            .expect("frozen folder");
        let source = &doc.project().sources[&frozen.source];
        let audio = audiostrada_media::load(&doc.resolve(&source.path)).unwrap();
        let peak = audio.channel(0).iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.01, "both children rendered: {peak}");
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let mut processor = doc.processor(48_000, 480);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 480);
        doc.update();
        doc.player_mut().engine_mut().play();
        for _ in 0..4 {
            processor.render(&mut storage.buffers(480));
            doc.update();
        }
        let level = storage.output(0).iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            (level - 0.5).abs() < 0.01,
            "children silent while frozen: {level}"
        );
    }

    #[test]
    fn previews_files_without_the_transport() {
        let dir = temp("preview");
        let file = dir.join("hit.wav");
        write_wav(&file, &AudioData::new(vec![vec![0.4; 4800]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        let mut processor = doc.processor(48_000, 480);
        let mut storage = audiostrada_io::ChannelBuffers::new(0, 2, 480);
        doc.preview(Some(&file));
        assert_eq!(doc.previewing(), Some(file.as_path()));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut heard = 0.0f32;
        while std::time::Instant::now() < deadline && heard == 0.0 {
            doc.update();
            processor.render(&mut storage.buffers(480));
            heard = storage.output(1).iter().fold(0.0, |m, s| m.max(s.abs()));
        }
        assert!((heard - 0.4).abs() < 1e-3, "mono to both sides: {heard}");
        assert!(!doc.player().engine().playing(), "transport untouched");
        for _ in 0..12 {
            processor.render(&mut storage.buffers(480));
        }
        assert!(
            storage.output(0).iter().all(|&s| s == 0.0),
            "stops at the end"
        );
        assert_eq!(doc.previewing(), None);
    }

    #[test]
    fn effect_files_are_copied_and_loaded() {
        let dir = temp("nam");
        let model = dir.join("amp.nam");
        std::fs::write(
            &model,
            r#"{"version": "0.5.4", "architecture": "WaveNet",
                "config": {"layers": [{"input_size": 1, "condition_size": 1, "channels": 1,
                "head_size": 1, "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                "gated": false, "head_bias": false}], "head": null, "head_scale": 1.0},
                "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]}"#,
        )
        .unwrap();
        let mut doc = Document::new(AppDirs::default());
        let paths = ProjectPaths::in_new_folder(&dir, "Amp");
        doc.save_as(paths.clone()).unwrap();
        let mut insert = None;
        doc.history_mut().edit("Guitar", |p| {
            let t = p.add_track("Guitar");
            insert = audiostrada_core::routing::add_insert(
                p,
                Channel::Track(t),
                "amp_rig",
                Default::default(),
            );
        });
        let (t, insert) = (doc.project().tracks[0].id, insert.unwrap());
        doc.set_effect_file(Channel::Track(t), insert, "amp", Some(&model))
            .unwrap();
        assert!(paths.models_dir().join("amp.nam").exists());
        let stored = &doc.project().tracks[0].inserts[0].files["amp"];
        assert_eq!(stored, std::path::Path::new("Models/amp.nam"));
        let _processor = doc.processor(48_000, 256);
        doc.update();
        assert!(matches!(
            doc.player().effect_file(insert, "amp"),
            Some(Ok(_))
        ));
        doc.set_effect_file(Channel::Track(t), insert, "amp", None)
            .unwrap();
        assert!(doc.project().tracks[0].inserts[0].files.is_empty());
    }

    #[test]
    fn exports_and_imports_midi_files() {
        let mut doc = Document::new(AppDirs::default());
        let note = Note {
            start: 24_000,
            length: 12_000,
            key: 62,
            velocity: 99,
            channel: 0,
            ..Default::default()
        };
        doc.history_mut().edit("Keys", |p| {
            let t = p.add_track("Keys");
            p.add_midi_clip(t, 48_000, 96_000, vec![note]);
        });
        let path = temp("midi").join("keys.mid");
        assert!(doc.export_midi(&path, Some(TrackId(9999))).is_err());
        doc.export_midi(&path, None).unwrap();
        assert_eq!(doc.import_midi(&path), Ok(1));
        let track = &doc.project().tracks[1];
        assert_eq!(track.name, "Keys");
        assert_eq!(track.inserts[0].effect, "synth");
        let notes = track.clips[0].notes.clone().unwrap();
        assert_eq!(
            (
                notes[0].start,
                notes[0].length,
                notes[0].key,
                notes[0].velocity
            ),
            (72_000, 12_000, 62, 99)
        );
    }

    #[test]
    fn missing_media_can_be_relinked() {
        let dir = temp("relink");
        let take = dir.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.1; 480]], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let paths = ProjectPaths::in_new_folder(&dir, "Song");
        doc.save_as(paths.clone()).unwrap();
        std::fs::remove_file(paths.audio_dir().join("take.wav")).unwrap();

        let mut doc = Document::open(&paths.file, AppDirs::default()).unwrap();
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        let missing = doc.missing();
        assert_eq!(missing.len(), 1);
        doc.relink(missing[0].0, &take);
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        assert!(doc.missing().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_reference_is_measured_and_matched_to_the_mix() {
        let dir = temp("reference");
        let song = dir.join("song.wav");
        let sine: Vec<f32> = (0..96_000)
            .map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        write_wav(&song, &AudioData::new(vec![sine], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        assert_eq!(doc.reference_gain(), 0.0);
        doc.set_reference(Some(&song));
        assert_eq!(doc.history().undo_label(), Some("Reference track"));
        assert!(doc.project().reference.as_ref().unwrap().matched);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while doc.reference_loudness().is_none() && std::time::Instant::now() < deadline {
            doc.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        let lufs = doc.reference_loudness().expect("measured");
        assert!((-12.0..-3.0).contains(&lufs), "{lufs}");
        assert_eq!(doc.reference_gain(), 0.0, "no mix loudness yet");
        doc.set_reference_matched(false);
        assert!(!doc.project().reference.as_ref().unwrap().matched);
        doc.set_reference(Some(&song));
        assert!(
            !doc.project().reference.as_ref().unwrap().matched,
            "a new file keeps the choice"
        );
        doc.player_mut().set_reference_listening(true);
        doc.set_reference(None);
        assert!(doc.project().reference.is_none());
        assert!(!doc.player().reference_listening());
        doc.update();
        assert_eq!(doc.reference_loudness(), None);
        assert_eq!(
            doc.history_mut().undo().as_deref(),
            Some("Remove reference track")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tracks_are_selected_alone_toggled_or_extended() {
        let mut doc = Document::new(AppDirs::default());
        let mut ids = Vec::new();
        doc.history_mut().edit("Tracks", |p| {
            ids = ["A", "B", "C", "D"].map(|n| p.add_track(n)).to_vec();
        });
        assert_eq!(doc.focused_track(), None);
        doc.select_track(ids[1], SelectMode::Only);
        doc.select_track(ids[3], SelectMode::Extend);
        assert_eq!(doc.selected_tracks(), [ids[1], ids[2], ids[3]]);
        assert_eq!(doc.focused_track(), Some(ids[3]));
        doc.select_track(ids[2], SelectMode::Toggle);
        assert!(!doc.is_track_selected(ids[2]));
        doc.select_track(ids[0], SelectMode::Toggle);
        assert_eq!(doc.focused_track(), Some(ids[0]));
        doc.select_track(ids[2], SelectMode::Extend);
        assert_eq!(doc.selected_tracks(), [ids[1], ids[3], ids[0], ids[2]]);
        doc.set_track_selection(Vec::new());
        doc.select_track(ids[2], SelectMode::Extend);
        assert_eq!(doc.selected_tracks(), [ids[2]], "nothing to extend from");
        use eframe::egui::Modifiers;
        assert_eq!(
            SelectMode::from_modifiers(Modifiers::COMMAND),
            SelectMode::Toggle
        );
        assert_eq!(
            SelectMode::from_modifiers(Modifiers::SHIFT),
            SelectMode::Extend
        );
        assert_eq!(
            SelectMode::from_modifiers(Modifiers::NONE),
            SelectMode::Only
        );
        assert_eq!(doc.title(), "Untitled *");
        doc.notify("hello".into());
        assert_eq!(doc.take_messages(), ["hello"]);
        assert!(doc.take_messages().is_empty());
    }
}
