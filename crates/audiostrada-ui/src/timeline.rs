//! Arrangement timeline: ruler, track headers, clips with waveforms, editing with a Smart Tool.
//!
//! Dragging previews the edit on a copy of the project; releasing commits it as one undo step.
//! Smart Tool zones on a clip: edges trim, top corners set fades, Ctrl-drag changes the clip
//! gain, the body moves (Alt copies).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use audiostrada_core::automation::{AutomationTarget, CurveShape};
use audiostrada_core::edit;
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{ClipId, MarkerId, Project, Track, TrackId, TrackKind};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use crate::actions::Action;
use crate::browser::DraggedFile;
use crate::document::{Document, SelectMode};
use crate::i18n::tr;
use crate::transport::{EditMode, EditSettings};
use crate::trf;

mod menus;
mod processing;
pub use menus::TRACK_COLORS;

const HEADER_WIDTH: f32 = 180.0;
const RULER_HEIGHT: f32 = 28.0;
const EDGE_ZONE: f32 = 6.0;
const FADE_ZONE_HEIGHT: f32 = 12.0;
const MIN_TRACK_HEIGHT: f32 = 40.0;
const MAX_TRACK_HEIGHT: f32 = 600.0;
/// Most a waveform is magnified vertically.
const MAX_WAVE_ZOOM: f32 = 64.0;
const MIN_ZOOM: f64 = 0.05;
const SCROLLBAR_HEIGHT: f32 = 12.0;
const MAX_ZOOM: f64 = 200_000.0;
const LANE_HEIGHT: f32 = 46.0;
const TAKE_HEIGHT: f32 = 36.0;
/// Distance in pixels within which an automation point can be grabbed.
const POINT_GRAB: f32 = 9.0;
const PUNCH: Color32 = Color32::from_rgb(220, 60, 60);
/// The time range, told apart from the loop and the clip selection.
const RANGE: Color32 = Color32::from_rgb(64, 196, 176);
/// How near a range edge the pointer grabs it, in pixels.
const RANGE_GRAB: f32 = 5.0;
/// Height of the loop band at the bottom of the ruler, where drags set the loop.
const LOOP_BAND: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragKind {
    Move {
        copy: bool,
    },
    TrimStart,
    TrimEnd,
    /// Alt on the right edge: time stretch.
    Stretch,
    FadeIn,
    FadeOut,
    Gain,
    Loop {
        anchor: u64,
        punch: bool,
    },
    Marker(MarkerId),
    Select {
        anchor: Pos2,
    },
    /// Shift-drag: a time range across tracks (razor).
    Range {
        anchor: Pos2,
    },
    /// An edge of the range, the other one (`anchor`) staying.
    RangeEdge {
        anchor: u64,
    },
    /// A drag in the ruler: a time selection on every track.
    RulerRange {
        anchor: u64,
    },
    /// An automation point: track, lane index, point index, top of the lane on screen.
    Point(TrackId, usize, usize, f32),
    /// Swipe comping on a take lane: track, playlist, where the drag started.
    Comp(TrackId, usize, u64),
}

#[derive(Debug)]
struct Drag {
    kind: DragKind,
    clip: Option<ClipId>,
    origin: Pos2,
    original: Project,
}

#[derive(Debug)]
pub struct TimelineView {
    doc: Rc<RefCell<Document>>,
    /// Project samples per pixel.
    zoom: f64,
    /// Project sample at the left edge of the lanes.
    scroll: f64,
    scroll_y: f32,
    track_height: f32,
    /// Vertical magnification of the waveforms.
    wave_zoom: f32,
    pub selection: Vec<ClipId>,
    drag: Option<Drag>,
    preview: Option<Project>,
    fitted: bool,
    lanes_width: f32,
    /// Lanes area of the last frame, to place files dropped from the system.
    lanes_rect: Rect,
    /// Shown tracks (index in the project, folder depth), top to bottom.
    rows: Vec<(usize, usize)>,
    /// Top of each row (track and its automation lanes), from the top of the content.
    tops: Vec<f32>,
    content_height: f32,
    /// Range and name of automation targets whose range comes from a plugin.
    target_info: HashMap<(TrackId, AutomationTarget), (String, f32, f32)>,
    /// Automation point under the last right click.
    point_menu: Option<(TrackId, usize, usize)>,
    /// Clip or lane under the last right click.
    menu: Option<menus::Menu>,
    /// Position and marker under the last right click on the ruler.
    ruler_target: Option<(u64, Option<MarkerId>)>,
    /// Commands chosen in menus, run by the application.
    pub requests: Vec<Action>,
    /// Track to export, chosen in its header menu.
    pub stem_request: Option<TrackId>,
    /// Time range across tracks, drawn with Shift-drag.
    pub range: Option<Range>,
    clipboard: menus::Clipboard,
    /// Name and range of every shown lane, by track and lane index.
    lane_info: HashMap<(TrackId, usize), (String, f32, f32)>,
    /// MIDI clip to create at the next frame (track, position).
    pending_midi: Option<(TrackId, u64)>,
    /// Range being comped: track, playlist, start and end.
    comping: Option<(TrackId, usize, u64, u64)>,
    /// Strip Silence and Process clips windows, when open.
    strip: Option<processing::StripWindow>,
    process: Option<processing::ProcessWindow>,
    beat: Option<processing::BeatWindow>,
    groove: Option<processing::GrooveWindow>,
    /// The span the range and the loop last agreed on, while they are linked.
    linked: Option<(u64, u64)>,
    /// Pointer over the lanes in the last frame, where keyboard zooms are centered.
    hover: Option<Pos2>,
    /// Track whose name is being edited in its header, and the text typed so far.
    renaming: Option<(TrackId, String)>,
    /// Clip last clicked, from which Shift+click extends the selection.
    clip_anchor: Option<ClipId>,
}

/// A time range on some tracks.
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    pub start: u64,
    pub end: u64,
    pub tracks: Vec<TrackId>,
}

/// Maps between project samples and screen x for the lanes.
#[derive(Clone, Copy)]
struct Scale {
    left: f32,
    scroll: f64,
    zoom: f64,
}

impl Scale {
    fn x(&self, sample: u64) -> f32 {
        self.left + ((sample as f64 - self.scroll) / self.zoom) as f32
    }

    fn sample(&self, x: f32) -> u64 {
        (self.scroll + f64::from(x - self.left) * self.zoom).max(0.0) as u64
    }

    fn samples(&self, dx: f32) -> i64 {
        (f64::from(dx) * self.zoom) as i64
    }
}

impl TimelineView {
    pub fn new(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            doc,
            zoom: 400.0,
            scroll: 0.0,
            scroll_y: 0.0,
            track_height: 72.0,
            wave_zoom: 1.0,
            selection: Vec::new(),
            drag: None,
            preview: None,
            fitted: false,
            lanes_width: 800.0,
            lanes_rect: Rect::NOTHING,
            rows: Vec::new(),
            tops: Vec::new(),
            content_height: 0.0,
            target_info: HashMap::new(),
            point_menu: None,
            menu: None,
            ruler_target: None,
            requests: Vec::new(),
            stem_request: None,
            range: None,
            clipboard: menus::Clipboard::default(),
            lane_info: HashMap::new(),
            pending_midi: None,
            comping: None,
            strip: None,
            process: None,
            beat: None,
            groove: None,
            linked: None,
            hover: None,
            renaming: None,
            clip_anchor: None,
        }
    }

    /// Takes back the clipboard of `other`, the view this one replaces.
    pub fn keep_clipboard(&mut self, other: &mut Self) {
        std::mem::swap(&mut self.clipboard, &mut other.clipboard);
    }

    /// While they are linked, the loop follows the time selection, and the selection a loop
    /// set otherwise (a command, an undo). Waits for the end of a drag.
    fn link_loop(&mut self, doc: &mut Document) {
        if doc.project().loop_fixed || self.drag.is_some() {
            return;
        }
        let range = self
            .range
            .as_ref()
            .map(|r| (r.start, r.end))
            .filter(|(a, b)| b > a);
        let looped = doc.project().loop_range;
        if range.is_some() && range != self.linked {
            if looped != range {
                doc.history_mut()
                    .modify_unrecorded(|p| p.loop_range = range);
            }
            self.linked = range;
        } else if looped != self.linked {
            if let (Some(r), Some((a, b))) = (&mut self.range, looped) {
                (r.start, r.end) = (a, b);
            }
            self.linked = looped;
        }
    }

    /// Grows the range to `at` (or moves its nearer edge there when `at` is inside), and to
    /// the tracks down or up to `row`.
    fn extend_range(&mut self, project: &Project, at: u64, row: Option<usize>) {
        let rows: Vec<TrackId> = self
            .rows
            .iter()
            .map(|&(index, _)| project.tracks[index].id)
            .collect();
        let Some(r) = &mut self.range else {
            return;
        };
        if at < r.start {
            r.start = at;
        } else if at > r.end {
            r.end = at;
        } else if at - r.start < r.end - at {
            r.start = at;
        } else {
            r.end = at;
        }
        let held = rows
            .iter()
            .enumerate()
            .filter(|(_, t)| r.tracks.contains(t));
        let (mut first, mut last) =
            held.fold((usize::MAX, 0), |(a, b), (i, _)| (a.min(i), b.max(i)));
        if let Some(row) = row {
            (first, last) = (first.min(row), last.max(row));
        }
        if first <= last {
            r.tracks = rows[first..=last].to_vec();
        }
    }

    /// The range over the selected clips, the loop, or every track.
    fn set_range(&mut self, doc: &mut Document, action: Action) {
        let project = doc.project();
        let all: Vec<TrackId> = self
            .rows
            .iter()
            .map(|&(index, _)| project.tracks[index].id)
            .collect();
        let range = match action {
            Action::RangeFromClips => {
                let clips: Vec<(TrackId, u64, u64)> = project
                    .tracks
                    .iter()
                    .flat_map(|t| {
                        t.clips
                            .iter()
                            .filter(|c| self.selection.contains(&c.id))
                            .map(move |c| (t.id, c.start, c.end()))
                    })
                    .collect();
                let start = clips.iter().map(|c| c.1).min();
                let end = clips.iter().map(|c| c.2).max();
                start.zip(end).map(|(start, end)| Range {
                    start,
                    end,
                    tracks: all
                        .iter()
                        .filter(|t| clips.iter().any(|c| c.0 == **t))
                        .copied()
                        .collect(),
                })
            }
            Action::RangeFromLoop => project.loop_range.map(|(start, end)| Range {
                start,
                end,
                tracks: self
                    .range
                    .as_ref()
                    .map_or_else(|| all.clone(), |r| r.tracks.clone()),
            }),
            _ => self.range.clone().map(|r| Range { tracks: all, ..r }),
        };
        match range.filter(|r| r.end > r.start) {
            Some(range) => {
                self.range = Some(range);
                self.selection.clear();
            }
            None => doc.notify(
                match action {
                    Action::RangeFromClips => tr("Select clips first"),
                    Action::RangeFromLoop => tr("Set a loop first"),
                    _ => tr("Select a passage first: Shift+drag across the tracks"),
                }
                .into(),
            ),
        }
    }

    pub fn reset(&mut self) {
        self.selection.clear();
        self.drag = None;
        self.preview = None;
        self.fitted = false;
        self.scroll = 0.0;
        self.scroll_y = 0.0;
    }

    /// Row (track with its lanes) under content height `y` + scroll, if any.
    fn row_at(&self, y: f32) -> Option<usize> {
        let y = y + self.scroll_y;
        if y < 0.0 {
            return None;
        }
        let i = self.tops.partition_point(|&t| t <= y);
        (i > 0 && y < self.content_height).then(|| i - 1)
    }

    /// Track whose clip area is under `y` (relative to the top of the lanes).
    fn track_at<'a>(&self, project: &'a Project, y: f32) -> Option<&'a Track> {
        let r = self.row_at(y)?;
        (y + self.scroll_y - self.tops[r] < self.track_height)
            .then(|| project.tracks[self.rows[r].0].as_ref())
    }

    /// Automation lane under `y`: row, index of the lane in its track, and the lane's top.
    fn lane_at(&self, project: &Project, y: f32) -> Option<(usize, usize, f32)> {
        let r = self.row_at(y)?;
        let offset = y + self.scroll_y - self.tops[r] - self.track_height;
        if offset < 0.0 {
            return None;
        }
        let track = &project.tracks[self.rows[r].0];
        let offset = offset - takes_height(track);
        if offset < 0.0 {
            return None;
        }
        let k = (offset / LANE_HEIGHT) as usize;
        let lane = *shown_lanes(track).get(k)?;
        Some((
            r,
            lane,
            self.tops[r] + self.track_height + takes_height(track) + k as f32 * LANE_HEIGHT
                - self.scroll_y,
        ))
    }

    /// Take lane under `y`: track and index of the playlist in its alternates.
    fn take_at(&self, project: &Project, y: f32) -> Option<(TrackId, usize)> {
        let r = self.row_at(y)?;
        let track = &project.tracks[self.rows[r].0];
        let offset = y + self.scroll_y - self.tops[r] - self.track_height;
        (offset >= 0.0 && offset < takes_height(track))
            .then(|| (track.id, (offset / TAKE_HEIGHT) as usize))
    }

    /// Track index offset that moves the dragged clip to the row under the pointer.
    fn track_offset(&self, drag: &Drag, y: f32) -> isize {
        let Some((index, _)) = drag.clip.and_then(|c| edit::locate(&drag.original, c)) else {
            return 0;
        };
        let target = match self.row_at(y) {
            Some(r) => r,
            None if y + self.scroll_y < 0.0 => 0,
            None => self.rows.len().saturating_sub(1),
        };
        self.rows
            .get(target)
            .map_or(0, |r| r.0 as isize - index as isize)
    }

    fn zoom_around(&mut self, factor: f64, anchor_sample: f64, anchor_offset: f32) {
        self.zoom = (self.zoom / factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = (anchor_sample - f64::from(anchor_offset) * self.zoom).max(0.0);
    }

    /// Changes the track height keeping the content under `pointer_y` (from the top of the
    /// lanes) in place.
    fn set_track_height(&mut self, height: f32, pointer_y: Option<f32>) {
        let height = height.clamp(MIN_TRACK_HEIGHT, MAX_TRACK_HEIGHT);
        if let Some(y) = pointer_y
            && let Some(i) = self.tops.iter().rposition(|&t| t <= self.scroll_y + y)
        {
            let within = self.scroll_y + y - self.tops[i];
            let delta = height - self.track_height;
            // Lanes below the track keep their height.
            let moved = if within <= self.track_height {
                within * height / self.track_height
            } else {
                within + delta
            };
            self.scroll_y = (self.tops[i] + i as f32 * delta + moved - y).max(0.0);
        }
        self.track_height = height;
    }

    /// The pointer's distance from the left and top of the lanes, when it is over them.
    fn hover_offset(&self) -> Option<Vec2> {
        self.hover.map(|p| p - self.lanes_rect.min)
    }

    fn zoom_to_fit(&mut self, length: u64) {
        let length = length.max(1) as f64;
        self.zoom =
            (length * 1.05 / f64::from(self.lanes_width.max(100.0))).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = 0.0;
    }

    /// The selected MIDI clip shown in the piano roll (the last one selected).
    pub fn midi_clip(&self) -> Option<ClipId> {
        let doc = self.doc.borrow();
        self.selection
            .iter()
            .rev()
            .copied()
            .find(|&c| edit::clip(doc.project(), c).is_some_and(|c| c.is_midi()))
    }

    /// Creates an empty MIDI clip of one bar on the selected track (or the first one), at
    /// `start` or the bar of the playhead, and selects it.
    pub fn new_midi_clip(&mut self, at: Option<(TrackId, u64)>) {
        let mut doc = self.doc.borrow_mut();
        let focused = doc.focused_track();
        let project = doc.project();
        let audio = |id: TrackId| {
            project
                .track(id)
                .is_some_and(|t| t.kind == TrackKind::Audio)
        };
        let (track, position) = match at {
            Some(at) => at,
            None => {
                let Some(track) = focused.filter(|t| audio(*t)).or_else(|| {
                    project
                        .tracks
                        .iter()
                        .find(|t| t.kind == TrackKind::Audio)
                        .map(|t| t.id)
                }) else {
                    return;
                };
                (track, doc.player().position())
            }
        };
        let (rate, tempo) = (project.sample_rate, project.tempo.clone());
        let beat = tempo.sample_to_beat(position, rate);
        let bar = tempo.beat_to_bbt(beat);
        let per_bar = tempo.signature_at_beat(beat).beats_per_bar();
        let bar_start = tempo.bbt_to_beat(audiostrada_core::tempo::BarBeatTick {
            bar: bar.bar,
            beat: 1,
            tick: 0,
        });
        let start = tempo.beat_to_sample(bar_start, rate);
        let length = tempo
            .beat_to_sample(bar_start + per_bar, rate)
            .saturating_sub(start);
        let mut created = None;
        doc.history_mut().edit(Action::NewMidiClip.label(), |p| {
            created = p.add_midi_clip(track, start, length, Vec::new());
            if let Some(id) = created {
                edit::resolve_overlaps(p, track, &[id]);
            }
        });
        if let Some(id) = created {
            self.selection = vec![id];
            doc.select_track(track, SelectMode::Only);
        }
    }

    /// Track and position under `pos` for files dropped there; the track is `None` below the
    /// last one or on a track that cannot hold audio.
    pub fn drop_target(
        &self,
        pos: Pos2,
        settings: &EditSettings,
    ) -> Option<(Option<TrackId>, u64)> {
        if !self.lanes_rect.contains(pos) {
            return None;
        }
        let doc = self.doc.borrow();
        let project = doc.project();
        let scale = Scale {
            left: self.lanes_rect.left(),
            scroll: self.scroll,
            zoom: self.zoom,
        };
        let track = self
            .track_at(project, pos.y - self.lanes_rect.top())
            .filter(|t| t.kind == TrackKind::Audio)
            .map(|t| t.id);
        Some((track, settings.snap(project, scale.sample(pos.x))))
    }

    /// Runs the timeline actions; returns true if `action` was handled.
    pub fn run(&mut self, action: Action, settings: &EditSettings) -> bool {
        let doc_rc = Rc::clone(&self.doc);
        let mut doc = doc_rc.borrow_mut();
        let position = doc.player().position();
        let selection = self.selection.clone();
        let track = doc.focused_track();
        match action {
            Action::Copy => self.copy(&doc),
            Action::Cut => self.cut(&mut doc, settings),
            Action::Paste => self.paste(&mut doc),
            Action::Delete if self.range.is_some() => {
                if let Some(r) = self.range.take() {
                    let shuffle = settings.mode == EditMode::Shuffle;
                    doc.history_mut().edit(tr("Delete range"), |p| {
                        edit::delete_range(p, &r.tracks, r.start, r.end, shuffle)
                    });
                }
            }
            Action::Split if self.range.is_some() => {
                if let Some(r) = &self.range {
                    doc.history_mut().edit(tr("Split range"), |p| {
                        edit::split_range(p, &r.tracks, r.start, r.end);
                    });
                }
            }
            Action::LoopSelection if self.range.is_some() => {
                if let Some(r) = &self.range {
                    let (a, b) = (r.start, r.end);
                    doc.history_mut().edit(tr("Loop range"), |p| {
                        p.loop_range = Some((a, b));
                        p.loop_off = false;
                    });
                }
            }
            Action::Deselect if self.range.is_some() => self.range = None,
            Action::SelectAll => {
                self.selection = doc
                    .project()
                    .tracks
                    .iter()
                    .flat_map(|t| t.clips.iter().map(|c| c.id))
                    .collect();
            }
            Action::Deselect => self.selection.clear(),
            Action::Delete if !selection.is_empty() => {
                let shuffle = settings.mode == EditMode::Shuffle;
                doc.history_mut().edit(tr("Delete clips"), |p| {
                    edit::delete_clips(p, &selection, shuffle)
                });
                self.selection.clear();
            }
            Action::Split => {
                let mut created = Vec::new();
                doc.history_mut().edit(tr("Split clips"), |p| {
                    created = edit::split(p, position, &selection)
                });
                self.selection.extend(created);
            }
            Action::Duplicate if !selection.is_empty() => {
                let mut created = Vec::new();
                doc.history_mut().edit(Action::Duplicate.label(), |p| {
                    created = edit::duplicate(p, &selection)
                });
                self.selection = created;
            }
            Action::NudgeLeft | Action::NudgeRight if !selection.is_empty() => {
                let step = settings.grid_step(doc.project(), position).max(1) as i64;
                let delta = if action == Action::NudgeLeft {
                    -step
                } else {
                    step
                };
                doc.history_mut().edit_merged(tr("Nudge"), "nudge", |p| {
                    edit::move_clips(p, &selection, delta, 0)
                });
            }
            Action::LoopSelection if !selection.is_empty() => {
                let project = doc.project();
                let clips: Vec<_> = selection
                    .iter()
                    .filter_map(|id| edit::clip(project, *id))
                    .collect();
                if let (Some(start), Some(end)) = (
                    clips.iter().map(|c| c.start).min(),
                    clips.iter().map(|c| c.end()).max(),
                ) {
                    doc.history_mut().edit(tr("Loop selection"), |p| {
                        p.loop_range = Some((start, end));
                        p.loop_off = false;
                    });
                }
            }
            Action::AddMarker => {
                doc.history_mut().edit(tr("Add marker"), |p| {
                    edit::add_marker(p, position);
                });
            }
            Action::NextMarker => {
                if let Some(target) = edit::next_marker(doc.project(), position) {
                    doc.player_mut().locate(target);
                }
            }
            Action::PreviousMarker => {
                if let Some(target) = edit::previous_marker(doc.project(), position) {
                    doc.player_mut().locate(target);
                }
            }
            Action::NewPlaylist => {
                if let Some(track) = track {
                    doc.history_mut()
                        .edit(tr("New playlist"), |p| edit::new_playlist(p, track));
                }
            }
            Action::NextPlaylist => {
                if let Some(track) = track {
                    doc.history_mut()
                        .edit(tr("Switch playlist"), |p| edit::next_playlist(p, track));
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                let factor = if action == Action::ZoomIn {
                    1.5
                } else {
                    1.0 / 1.5
                };
                // Around the pointer when it is over the lanes, else around the playhead.
                let (anchor, offset) = match self.hover_offset() {
                    Some(o) => (self.scroll + f64::from(o.x) * self.zoom, o.x),
                    None => {
                        let offset = ((position as f64 - self.scroll) / self.zoom) as f32;
                        (position as f64, offset.clamp(0.0, self.lanes_width))
                    }
                };
                self.zoom_around(factor, anchor, offset);
            }
            Action::ZoomToFit => {
                let length = doc.project().length();
                self.zoom_to_fit(length);
            }
            Action::TallerTracks => {
                let y = self.hover_offset().map(|o| o.y);
                self.set_track_height(self.track_height * 1.25, y);
            }
            Action::ShorterTracks => {
                let y = self.hover_offset().map(|o| o.y);
                self.set_track_height(self.track_height / 1.25, y);
            }
            Action::Heal => self.heal(&mut doc),
            Action::Consolidate => self.consolidate(&mut doc),
            Action::StripSilence => self.open_strip(&mut doc),
            Action::ProcessClips => self.open_process(&mut doc),
            Action::BeatDetective => self.open_beat(&mut doc),
            Action::Groove => self.open_groove(&mut doc),
            Action::RangeFromClips | Action::RangeFromLoop | Action::RangeAllTracks => {
                self.set_range(&mut doc, action)
            }
            Action::CropToSelection | Action::RemoveSection | Action::ClearSection => {
                self.edit_section(&mut doc, action)
            }
            Action::WaveformZoomIn => self.wave_zoom = (self.wave_zoom * 1.5).min(MAX_WAVE_ZOOM),
            Action::WaveformZoomOut => self.wave_zoom = (self.wave_zoom / 1.5).max(1.0),
            Action::WaveformZoomReset => self.wave_zoom = 1.0,
            _ => return false,
        }
        true
    }

    pub fn show(&mut self, ui: &mut Ui, settings: &EditSettings) {
        if let Some(at) = self.pending_midi.take() {
            self.new_midi_clip(Some(at));
        }
        let doc_rc = Rc::clone(&self.doc);
        let mut doc = doc_rc.borrow_mut();
        let project = self
            .preview
            .clone()
            .unwrap_or_else(|| doc.project().clone());
        let length = project.length();
        self.rows = routing::visible_tracks(&project);
        self.tops.clear();
        let mut y = 0.0;
        for &(index, _) in &self.rows {
            self.tops.push(y);
            let track = &project.tracks[index];
            y += self.track_height
                + takes_height(track)
                + shown_lanes(track).len() as f32 * LANE_HEIGHT;
        }
        self.content_height = y;
        self.lane_info.clear();
        for &(index, _) in &self.rows.clone() {
            let track = &project.tracks[index];
            for li in shown_lanes(track) {
                let info = self.target_info(&mut doc, track, &track.automation[li].target);
                self.lane_info.insert((track.id, li), info);
            }
        }

        let all = ui.available_rect_before_wrap();
        let (_, background) = ui.allocate_exact_size(all.size(), Sense::hover());
        // The horizontal scrollbar takes the bottom.
        let full = Rect::from_min_max(all.min, pos2(all.right(), all.bottom() - SCROLLBAR_HEIGHT));
        let header = Rect::from_min_max(
            pos2(full.left(), full.top() + RULER_HEIGHT),
            pos2(full.left() + HEADER_WIDTH, full.bottom()),
        );
        let ruler = Rect::from_min_max(
            pos2(full.left() + HEADER_WIDTH, full.top()),
            pos2(full.right(), full.top() + RULER_HEIGHT),
        );
        let lanes = Rect::from_min_max(
            pos2(full.left() + HEADER_WIDTH, full.top() + RULER_HEIGHT),
            full.max,
        );
        self.lanes_width = lanes.width();
        self.lanes_rect = lanes;
        if !self.fitted && length > 0 && !doc.busy() {
            self.fitted = true;
            self.zoom_to_fit(length);
        }
        let visuals = ui.visuals().clone();
        let painter = ui.painter().with_clip_rect(full);
        painter.rect_filled(all, 0.0, visuals.extreme_bg_color);
        self.hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| lanes.contains(*p));

        // Scrolling and zooming.
        if background.hovered() || ui.rect_contains_pointer(full) {
            let (scroll, zoom, modifiers, pointer) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.zoom_delta(),
                    i.modifiers,
                    i.pointer.hover_pos(),
                )
            });
            if modifiers.alt {
                // Alt: taller or shorter tracks; Alt+Shift: louder-looking waveforms.
                let delta = scroll.x + scroll.y;
                if delta != 0.0 {
                    let factor = (delta * 0.01).exp();
                    if modifiers.shift {
                        self.wave_zoom = (self.wave_zoom * factor).clamp(1.0, MAX_WAVE_ZOOM);
                    } else {
                        let y = pointer.map(|p| p.y - lanes.top());
                        self.set_track_height(self.track_height * factor, y);
                    }
                }
            } else if zoom != 1.0 {
                let x = pointer.map_or(lanes.center().x, |p| p.x) - lanes.left();
                let anchor = self.scroll + f64::from(x) * self.zoom;
                self.zoom_around(f64::from(zoom), anchor, x);
            } else if modifiers.shift {
                self.scroll = (self.scroll - f64::from(scroll.y + scroll.x) * self.zoom).max(0.0);
            } else {
                self.scroll = (self.scroll - f64::from(scroll.x) * self.zoom).max(0.0);
                let content = self.content_height;
                self.scroll_y = (self.scroll_y - scroll.y)
                    .clamp(0.0, (content - lanes.height() + 40.0).max(0.0));
            }
        }

        // Follow the playhead while playing.
        let position = doc.player().position();
        let playing = doc.player().engine().playing();
        if playing && self.drag.is_none() {
            let visible = f64::from(lanes.width()) * self.zoom;
            if (position as f64) > self.scroll + visible * 0.95 || (position as f64) < self.scroll {
                self.scroll = (position as f64 - visible * 0.05).max(0.0);
            }
        }
        self.scrollbar(
            ui,
            Rect::from_min_max(pos2(lanes.left(), full.bottom()), all.max),
            length,
        );
        let scale = Scale {
            left: lanes.left(),
            scroll: self.scroll,
            zoom: self.zoom,
        };

        self.draw_ruler(&painter, ruler, &project, scale, &visuals);
        let lane_painter = painter.with_clip_rect(lanes);
        self.draw_grid(&lane_painter, lanes, &project, scale, &visuals);
        if let Some((start, end)) = project.looping() {
            let r = Rect::from_x_y_ranges(scale.x(start)..=scale.x(end), lanes.y_range());
            lane_painter.rect_filled(r, 0.0, visuals.selection.bg_fill.gamma_multiply(0.08));
        }
        if let Some((start, end)) = project.punch_range {
            let r = Rect::from_x_y_ranges(scale.x(start)..=scale.x(end), lanes.y_range());
            lane_painter.rect_filled(r, 0.0, PUNCH.gamma_multiply(0.06));
        }
        let recording = doc
            .recording()
            .map(|(start, tracks)| (start, tracks.to_vec()));

        // Lanes and clips.
        let mut hovered_clip = None;
        // While a button is held, what was under the press decides what a drag grabs.
        let pointer = ui.input(|i| {
            if i.pointer.any_down() {
                i.pointer.press_origin().or(i.pointer.hover_pos())
            } else {
                i.pointer.hover_pos()
            }
        });
        for (i, &(index, _)) in self.rows.iter().enumerate() {
            let track = &project.tracks[index];
            let top = lanes.top() + self.tops[i] - self.scroll_y;
            let row = Rect::from_min_max(
                pos2(lanes.left(), top),
                pos2(lanes.right(), top + self.track_height),
            );
            if row.bottom() < lanes.top() || row.top() > lanes.bottom() {
                continue;
            }
            let selected_track = doc.is_track_selected(track.id);
            let lane_fill = if selected_track {
                visuals.faint_bg_color.gamma_multiply(1.6)
            } else {
                visuals.faint_bg_color
            };
            lane_painter.rect_filled(row.shrink2(vec2(0.0, 0.5)), 0.0, lane_fill);
            if track.kind == TrackKind::Vca {
                let members = project
                    .tracks
                    .iter()
                    .filter(|t| t.vca == Some(track.id))
                    .count();
                lane_painter.text(
                    pos2(lanes.left() + 8.0, row.center().y),
                    Align2::LEFT_CENTER,
                    trf!("VCA controlling {members} track(s)", members = members),
                    FontId::proportional(11.0),
                    visuals.weak_text_color(),
                );
            }
            for clip in &track.clips {
                let rect = Rect::from_min_max(
                    pos2(scale.x(clip.start), row.top() + 2.0),
                    pos2(scale.x(clip.end()), row.bottom() - 2.0),
                );
                if rect.right() < lanes.left() || rect.left() > lanes.right() {
                    continue;
                }
                let selected = self.selection.contains(&clip.id);
                self.draw_clip(
                    &lane_painter,
                    rect,
                    &doc,
                    &project,
                    track,
                    clip,
                    selected,
                    &visuals,
                );
                if pointer.is_some_and(|p| rect.contains(p) && lanes.contains(p)) {
                    hovered_clip = Some((clip.id, track.id, rect));
                }
            }
            if let Some((start, tracks)) = &recording
                && tracks.contains(&track.id)
            {
                let end = project
                    .punch_range
                    .map_or(position, |(_, b)| position.min(b));
                if end > *start {
                    let r = Rect::from_min_max(
                        pos2(scale.x(*start), row.top() + 2.0),
                        pos2(scale.x(end), row.bottom() - 2.0),
                    );
                    lane_painter.rect_filled(r, 3.0, PUNCH.gamma_multiply(0.35));
                    lane_painter.rect_stroke(r, 3.0, Stroke::new(1.0, PUNCH), StrokeKind::Inside);
                    let wave = Color32::from_rgb(255, 205, 200);
                    let (mid, half) = (r.center().y, r.height() / 2.0 - 3.0);
                    doc.live_recording(|t, take_start, block, peaks| {
                        if t != track.id || peaks.is_empty() {
                            return;
                        }
                        let left = r.left().max(lanes.left()).max(scale.x(take_start));
                        let right = r.right().min(lanes.right());
                        let mut x = left.floor();
                        while x < right {
                            let a = scale.sample(x).saturating_sub(take_start) as f64 / block;
                            let b = scale.sample(x + 1.0).saturating_sub(take_start) as f64 / block;
                            let (a, b) = (a as usize, (b.ceil() as usize).max(a as usize + 1));
                            if a >= peaks.len() {
                                break;
                            }
                            let peak = peaks[a..b.min(peaks.len())]
                                .iter()
                                .fold(0.0f32, |m, &p| m.max(p))
                                .min(1.0);
                            let h = (peak * half).max(0.5);
                            lane_painter.line_segment(
                                [pos2(x + 0.5, mid - h), pos2(x + 0.5, mid + h)],
                                Stroke::new(1.0, wave),
                            );
                            x += 1.0;
                        }
                    });
                }
            }
            if track.show_takes {
                self.draw_takes(
                    &lane_painter,
                    row,
                    lanes,
                    &doc,
                    &project,
                    track,
                    scale,
                    &visuals,
                );
            }
            let reading =
                track.automation_mode != audiostrada_core::automation::AutomationMode::Off;
            for (k, li) in shown_lanes(track).into_iter().enumerate() {
                let top = row.bottom() + takes_height(track) + k as f32 * LANE_HEIGHT;
                let rect = Rect::from_min_max(
                    pos2(lanes.left(), top),
                    pos2(lanes.right(), top + LANE_HEIGHT),
                );
                let Some((_, min, max)) = self.lane_info.get(&(track.id, li)).cloned() else {
                    continue;
                };
                let lane = &track.automation[li];
                let color = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);
                let color = if reading && lane.enabled {
                    color
                } else {
                    color.gamma_multiply(0.35)
                };
                draw_lane(&lane_painter, rect, lane, min, max, scale, color, &visuals);
            }
        }

        // Playhead.
        let x = scale.x(position);
        if x >= lanes.left() {
            painter.line_segment(
                [pos2(x, ruler.top()), pos2(x, full.bottom())],
                Stroke::new(1.5, Color32::from_rgb(240, 80, 70)),
            );
        }

        if let Some(range) = &self.range {
            let (x0, x1) = (scale.x(range.start), scale.x(range.end));
            for (i, &(index, _)) in self.rows.iter().enumerate() {
                if !range.tracks.contains(&project.tracks[index].id) {
                    continue;
                }
                let top = lanes.top() + self.tops[i] - self.scroll_y;
                let r = Rect::from_x_y_ranges(x0..=x1, top..=top + self.track_height);
                lane_painter.rect_filled(r, 0.0, RANGE.gamma_multiply(0.28));
                lane_painter.rect_stroke(r, 0.0, Stroke::new(1.0, RANGE), StrokeKind::Inside);
            }
            // The edges across every track, where they can be grabbed on the range's.
            for x in [x0, x1] {
                lane_painter.vline(x, lanes.y_range(), Stroke::new(1.5, RANGE));
            }
            if x0 < lanes.right() && x1 > lanes.left() {
                let galley = lane_painter.layout_no_wrap(
                    range_label(&project, range.start, range.end),
                    FontId::proportional(11.5),
                    Color32::from_rgb(12, 32, 28),
                );
                let at = pos2(x0.max(lanes.left()) + 6.0, lanes.top() + 5.0);
                let tag = Rect::from_min_size(at, galley.size()).expand2(vec2(5.0, 2.0));
                lane_painter.rect_filled(tag, 3.0, RANGE);
                lane_painter.galley(at, galley, Color32::from_rgb(12, 32, 28));
            }
        }
        self.draw_headers(ui, header, &mut doc, &project, &visuals);
        self.processing_windows(ui.ctx(), &mut doc, settings.grid);
        self.interact(
            ui,
            &mut doc,
            &project,
            settings,
            scale,
            lanes,
            ruler,
            hovered_clip,
        );
        self.link_loop(&mut doc);

        if project.tracks.is_empty() {
            painter.text(
                lanes.center(),
                Align2::CENTER_CENTER,
                tr("Drop audio files here, or use File › Import audio"),
                FontId::proportional(15.0),
                visuals.weak_text_color(),
            );
        }
    }

    /// The horizontal scrollbar: its thumb shows the visible part of the session, dragged to
    /// scroll; a click beside it centers the view there.
    fn scrollbar(&mut self, ui: &mut Ui, rect: Rect, length: u64) {
        let visible = f64::from(rect.width()) * self.zoom;
        let extent = (length as f64 * 1.05).max(self.scroll + visible).max(1.0);
        let width = f64::from(rect.width());
        let thumb_width = (visible / extent * width).max(24.0).min(width) as f32;
        let travel = (rect.width() - thumb_width).max(1.0);
        let samples_per_px = (extent - visible).max(0.0) / f64::from(travel);
        let thumb_left = rect.left() + (self.scroll / samples_per_px.max(1e-9)) as f32;
        let thumb = Rect::from_min_size(
            pos2(thumb_left.min(rect.right() - thumb_width), rect.top() + 2.0),
            vec2(thumb_width, rect.height() - 4.0),
        );
        let response = ui.interact(
            rect,
            ui.scope_id().with("h-scrollbar"),
            Sense::click_and_drag(),
        );
        if response.dragged() {
            self.scroll = (self.scroll + f64::from(response.drag_delta().x) * samples_per_px)
                .clamp(0.0, (extent - visible).max(0.0));
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && !thumb.contains(pos)
        {
            let center = f64::from(pos.x - rect.left()) / width * extent;
            self.scroll = (center - visible / 2.0).clamp(0.0, (extent - visible).max(0.0));
        }
        let visuals = ui.visuals();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
        let fill = if response.dragged() || response.hovered() {
            visuals.weak_text_color().gamma_multiply(0.8)
        } else {
            visuals.weak_text_color().gamma_multiply(0.45)
        };
        painter.rect_filled(thumb, 4.0, fill);
    }

    fn draw_ruler(
        &self,
        painter: &egui::Painter,
        ruler: Rect,
        project: &Project,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        painter.rect_filled(ruler, 0.0, visuals.panel_fill);
        let rate = project.sample_rate;
        let tempo = &project.tempo;
        let first_beat = tempo
            .sample_to_beat(scale.sample(ruler.left()), rate)
            .floor()
            .max(0.0);
        let last_beat = tempo
            .sample_to_beat(scale.sample(ruler.right()), rate)
            .ceil();
        let beat_px = (tempo.beat_to_sample(first_beat + 1.0, rate)
            - tempo.beat_to_sample(first_beat, rate)) as f64
            / scale.zoom;
        let per_bar = tempo.signature_at_beat(first_beat).beats_per_bar();
        let label_every = if beat_px * per_bar < 40.0 {
            (40.0 / (beat_px * per_bar)).ceil() * per_bar
        } else {
            per_bar
        };
        let mut beat = first_beat;
        let mut count = 0;
        while beat <= last_beat && count < 4000 {
            count += 1;
            let x = scale.x(tempo.beat_to_sample(beat, rate));
            let bbt = tempo.beat_to_bbt(beat);
            let on_bar = bbt.beat == 1 && bbt.tick == 0;
            if on_bar || beat_px > 6.0 {
                let h = if on_bar { 12.0 } else { 6.0 };
                painter.line_segment(
                    [pos2(x, ruler.bottom() - h), pos2(x, ruler.bottom())],
                    Stroke::new(1.0, visuals.weak_text_color()),
                );
            }
            let bar_index = f64::from(bbt.bar - 1) * per_bar;
            if on_bar && (bar_index % label_every).abs() < 1e-6 {
                painter.text(
                    pos2(x + 3.0, ruler.top() + 2.0),
                    Align2::LEFT_TOP,
                    bbt.bar.to_string(),
                    FontId::monospace(11.0),
                    visuals.text_color(),
                );
            }
            beat += 1.0;
        }
        if let Some((start, end)) = project.loop_range {
            let r = Rect::from_x_y_ranges(
                scale.x(start)..=scale.x(end),
                (ruler.bottom() - 6.0)..=ruler.bottom(),
            );
            // Kept but off: an outline only.
            if project.loop_off {
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, visuals.weak_text_color()),
                    StrokeKind::Inside,
                );
            } else {
                painter.rect_filled(r, 2.0, visuals.selection.bg_fill.gamma_multiply(0.8));
            }
        }
        if let Some((start, end)) = project.punch_range {
            let r = Rect::from_x_y_ranges(
                scale.x(start)..=scale.x(end),
                ruler.top()..=(ruler.top() + 4.0),
            );
            painter.rect_filled(r, 2.0, PUNCH);
        }
        if let Some(range) = &self.range {
            let r =
                Rect::from_x_y_ranges(scale.x(range.start)..=scale.x(range.end), ruler.y_range());
            painter.rect_filled(r, 0.0, RANGE.gamma_multiply(0.22));
            for x in [r.left(), r.right()] {
                painter.vline(x, ruler.y_range(), Stroke::new(2.0, RANGE));
            }
        }
        for marker in &project.markers {
            let x = scale.x(marker.position);
            if x < ruler.left() || x > ruler.right() {
                continue;
            }
            let color = Color32::from_rgb(230, 170, 40);
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(x, ruler.top() + 14.0),
                    pos2(x + 8.0, ruler.top() + 14.0),
                    pos2(x, ruler.bottom() - 6.0),
                ],
                color,
                Stroke::NONE,
            ));
            painter.text(
                pos2(x + 10.0, ruler.top() + 13.0),
                Align2::LEFT_TOP,
                &marker.name,
                FontId::proportional(10.0),
                color,
            );
        }
    }

    fn draw_grid(
        &self,
        painter: &egui::Painter,
        lanes: Rect,
        project: &Project,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        let rate = project.sample_rate;
        let tempo = &project.tempo;
        let first = tempo
            .sample_to_beat(scale.sample(lanes.left()), rate)
            .floor()
            .max(0.0);
        let last = tempo
            .sample_to_beat(scale.sample(lanes.right()), rate)
            .ceil();
        let beat_px = (tempo.beat_to_sample(first + 1.0, rate) - tempo.beat_to_sample(first, rate))
            as f64
            / scale.zoom;
        let mut beat = first;
        let mut count = 0;
        while beat <= last && count < 4000 {
            count += 1;
            let bbt = tempo.beat_to_bbt(beat);
            let on_bar = bbt.beat == 1 && bbt.tick == 0;
            if on_bar || beat_px > 12.0 {
                let x = scale.x(tempo.beat_to_sample(beat, rate));
                let alpha = if on_bar { 0.12 } else { 0.05 };
                painter.line_segment(
                    [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
                    Stroke::new(1.0, visuals.text_color().gamma_multiply(alpha)),
                );
            }
            beat += 1.0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_clip(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        doc: &Document,
        project: &Project,
        track: &Track,
        clip: &audiostrada_core::Clip,
        selected: bool,
        visuals: &egui::Visuals,
    ) {
        let mut base = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);
        if clip.muted {
            base = visuals.weak_text_color();
        }
        let fill = if selected {
            base.gamma_multiply(0.55)
        } else {
            base.gamma_multiply(0.3)
        };
        painter.rect_filled(rect, 3.0, fill);
        // Name bar in the track color.
        let bar = Rect::from_min_max(
            rect.min,
            pos2(rect.right(), rect.top() + 14.0_f32.min(rect.height() / 3.0)),
        );
        painter.rect_filled(
            bar,
            egui::CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            base.gamma_multiply(if selected { 0.95 } else { 0.7 }),
        );
        let outline = if selected {
            Stroke::new(1.5, visuals.strong_text_color())
        } else {
            Stroke::new(1.0, base.gamma_multiply(0.8))
        };
        painter.rect_stroke(rect, 3.0, outline, StrokeKind::Inside);
        let wave_rect = rect.shrink2(vec2(1.0, 14.0_f32.min(rect.height() / 4.0)));
        if let Some(notes) = &clip.notes {
            draw_notes(painter, wave_rect, clip, notes, self.zoom, base);
        } else {
            self.draw_waveform(painter, wave_rect, doc, project, clip, base);
            self.strip_preview(painter, wave_rect, clip);
            self.beat_preview(painter, wave_rect, track.id, clip);
            self.groove_preview(painter, wave_rect, track.id, clip);
        }

        // Fades.
        let fade_color = visuals.strong_text_color().gamma_multiply(0.7);
        let px = |samples: u64| (samples as f64 / self.zoom) as f32;
        let fade = |x0: f32, width: f32, rising: bool, shape: audiostrada_core::FadeShape| {
            let steps = (width / 3.0).clamp(2.0, 48.0) as usize;
            let points: Vec<Pos2> = (0..=steps)
                .map(|i| {
                    let t = i as f32 / steps as f32;
                    let g = shape.gain(if rising { t } else { 1.0 - t });
                    pos2(x0 + t * width, rect.bottom() - g * rect.height())
                })
                .collect();
            // Shade the attenuated part, above the curve, with one quad per step.
            let mut mesh = egui::Mesh::default();
            let shade = visuals.extreme_bg_color.gamma_multiply(0.45);
            for pair in points.windows(2) {
                let i = mesh.vertices.len() as u32;
                for p in [
                    pair[0],
                    pair[1],
                    pos2(pair[1].x, rect.top()),
                    pos2(pair[0].x, rect.top()),
                ] {
                    mesh.colored_vertex(p, shade);
                }
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::line(points, Stroke::new(1.0, fade_color)));
        };
        if clip.fade_in > 0 {
            fade(rect.left(), px(clip.fade_in), true, clip.fade_shape);
        }
        if clip.fade_out > 0 {
            let width = px(clip.fade_out);
            fade(rect.right() - width, width, false, clip.out_shape());
        }
        let handle = |x: f32| Rect::from_center_size(pos2(x, rect.top() + 4.0), vec2(6.0, 6.0));
        painter.rect_filled(
            handle(rect.left() + px(clip.fade_in) + 3.0),
            1.0,
            fade_color,
        );
        painter.rect_filled(
            handle(rect.right() - px(clip.fade_out) - 3.0),
            1.0,
            fade_color,
        );

        let mut label = clip.name.clone();
        if (clip.stretch - 1.0).abs() > 1e-6 {
            label.push_str(&format!("  {:.0} %", clip.stretch * 100.0));
        }
        if clip.pitch != 0.0 {
            label.push_str(&format!("  {:+.1} st", clip.pitch));
        }
        if clip.variant().is_some()
            && audiostrada_engine::project::clip_media(project, doc.paths(), clip)
                .is_some_and(|p| doc.media().audio(&p).is_none())
        {
            label.push_str(&format!("  {}", tr("(rendering…)")));
        }
        if clip.muted {
            label.push_str(&format!("  {}", tr("(muted)")));
        }
        if clip.gain_db != 0.0 {
            label.push_str(&format!("  {:+.1} dB", clip.gain_db));
        }
        painter
            .with_clip_rect(rect.intersect(painter.clip_rect()))
            .text(
                pos2(rect.left() + 10.0, rect.top() + 1.0),
                Align2::LEFT_TOP,
                label,
                crate::theme::strong(10.5),
                Color32::from_rgb(20, 20, 24),
            );
    }

    /// Waveform from the peak caches, or from the samples when zoomed in closely.
    /// The alternate playlists of a track as dimmed lanes under it, with the range being comped.
    #[allow(clippy::too_many_arguments)]
    fn draw_takes(
        &self,
        painter: &egui::Painter,
        row: Rect,
        lanes: Rect,
        doc: &Document,
        project: &Project,
        track: &Track,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        for (k, playlist) in track.alternates.iter().enumerate() {
            let top = row.bottom() + k as f32 * TAKE_HEIGHT;
            let lane = Rect::from_min_max(
                pos2(lanes.left(), top),
                pos2(lanes.right(), top + TAKE_HEIGHT),
            );
            if lane.bottom() < lanes.top() || lane.top() > lanes.bottom() {
                continue;
            }
            painter.rect_filled(lane.shrink2(vec2(0.0, 0.5)), 0.0, visuals.extreme_bg_color);
            for clip in &playlist.clips {
                let rect = Rect::from_min_max(
                    pos2(scale.x(clip.start), lane.top() + 2.0),
                    pos2(scale.x(clip.end()), lane.bottom() - 2.0),
                );
                if rect.right() < lanes.left() || rect.left() > lanes.right() {
                    continue;
                }
                self.draw_clip(painter, rect, doc, project, track, clip, false, visuals);
                painter.rect_filled(rect, 3.0, visuals.extreme_bg_color.gamma_multiply(0.45));
            }
            if let Some((t, p, start, end)) = self.comping
                && t == track.id
                && p == k
            {
                let r = Rect::from_x_y_ranges(scale.x(start)..=scale.x(end), lane.y_range());
                painter.rect_filled(r, 0.0, visuals.selection.bg_fill.gamma_multiply(0.3));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, visuals.selection.bg_fill),
                    StrokeKind::Inside,
                );
            }
        }
    }

    fn draw_waveform(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        doc: &Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        color: Color32,
    ) {
        let Some(source) = project.sources.get(&clip.source) else {
            return;
        };
        let Some(path) = audiostrada_engine::project::clip_media(project, doc.paths(), clip) else {
            return;
        };
        let ratio = f64::from(source.sample_rate) / f64::from(project.sample_rate);
        let source_per_px = self.zoom * ratio;
        let visible = rect.intersect(painter.clip_rect());
        if visible.width() <= 0.0 {
            return;
        }
        let first_px = (visible.left() - rect.left()).floor().max(0.0) as usize;
        let pixels = visible.width().ceil() as usize + 1;
        let start = clip.offset as f64 * ratio + first_px as f64 * source_per_px;
        let peaks = doc.media().peaks(&path);
        let original = doc.media().original(&path);
        let channels = match (&peaks, &original) {
            (Some(p), _) => p.channels(),
            (None, Some(d)) => d.channels(),
            _ => return,
        };
        // Each channel in its own band (left above right) unless the track is too low.
        let bands = if rect.height() / channels as f32 >= 10.0 {
            channels
        } else {
            1
        };
        let band_height = rect.height() / bands as f32;
        let band = |ch: usize| {
            let b = ch.min(bands - 1) as f32;
            let mid = rect.top() + band_height * (b + 0.5);
            (mid, band_height / 2.0 - 1.0)
        };
        let zoom = self.wave_zoom;
        let clip_px = (clip.length as f64 / self.zoom) as f32;
        let gain_at_x = |x: f32| {
            let rel = ((x - rect.left()) / clip_px.max(1.0)) as f64 * clip.length as f64;
            envelope(clip, rel as u64)
        };
        let y = |mid: f32, half: f32, v: f32| mid - (v * zoom).clamp(-1.0, 1.0) * half;
        let zero = Stroke::new(1.0, color.gamma_multiply(0.25));
        for ch in 0..bands {
            let (mid, _) = band(ch);
            painter.line_segment(
                [pos2(visible.left(), mid), pos2(visible.right(), mid)],
                zero,
            );
            if ch > 0 {
                let edge = rect.top() + band_height * ch as f32;
                painter.line_segment(
                    [pos2(visible.left(), edge), pos2(visible.right(), edge)],
                    Stroke::new(1.0, color.gamma_multiply(0.45)),
                );
            }
        }
        // Zoomed in to the samples: a line through them, and dots when they are far apart.
        if source_per_px < 1.0
            && let Some(data) = &original
        {
            let first = (start.floor() as i64 - 1).max(0) as usize;
            let last =
                ((start + pixels as f64 * source_per_px).ceil() as usize + 1).min(data.frames());
            let x_of = |i: usize| {
                rect.left() + first_px as f32 + ((i as f64 - start) / source_per_px) as f32
            };
            for ch in 0..channels {
                let (mid, half) = band(ch);
                let samples = data.channel(ch);
                let points: Vec<Pos2> = (first..last)
                    .map(|i| {
                        let x = x_of(i);
                        pos2(x, y(mid, half, samples[i] * gain_at_x(x)))
                    })
                    .collect();
                if source_per_px < 0.15 {
                    for p in &points {
                        painter.circle_filled(*p, 1.8, color);
                    }
                }
                painter.add(egui::Shape::line(points, Stroke::new(1.2, color)));
            }
            return;
        }
        for ch in 0..channels {
            // Peaks of this channel (with one band, the channels are merged).
            let mut columns = vec![(f32::MAX, f32::MIN); pixels];
            match (&peaks, &original) {
                (Some(peaks), _) if source_per_px >= f64::from(peaks.finest()) => {
                    peaks.query(ch, start, source_per_px, &mut columns);
                }
                (_, Some(data)) => {
                    let samples = data.channel(ch);
                    for (i, column) in columns.iter_mut().enumerate() {
                        let from = (start + i as f64 * source_per_px) as usize;
                        let to = ((start + (i + 1) as f64 * source_per_px) as usize)
                            .max(from + 1)
                            .min(data.frames());
                        for &s in samples.get(from..to).unwrap_or(&[]) {
                            column.0 = column.0.min(s);
                            column.1 = column.1.max(s);
                        }
                    }
                }
                _ => return,
            }
            let (mid, half) = band(ch);
            let mut previous: Option<(f32, f32)> = None;
            for (i, &(lo, hi)) in columns.iter().enumerate() {
                if lo > hi {
                    previous = None;
                    continue;
                }
                // Each column reaches the previous one, so the waveform has no gaps.
                let (lo, hi) = match previous.replace((lo, hi)) {
                    Some((plo, phi)) => (lo.min(phi), hi.max(plo)),
                    None => (lo, hi),
                };
                let x = rect.left() + (first_px + i) as f32 + 0.5;
                if x > rect.right() {
                    break;
                }
                let gain = gain_at_x(x);
                let top = y(mid, half, hi * gain);
                let bottom = y(mid, half, lo * gain);
                painter.line_segment(
                    [pos2(x, top), pos2(x, bottom.max(top + 0.5))],
                    Stroke::new(1.0, color),
                );
            }
        }
    }

    /// Name and value range of an automation target.
    fn target_info(
        &mut self,
        doc: &mut Document,
        track: &Track,
        target: &AutomationTarget,
    ) -> (String, f32, f32) {
        match target {
            AutomationTarget::Gain => (tr("Volume").into(), -60.0, 12.0),
            AutomationTarget::Pan => (tr("Pan").into(), -1.0, 1.0),
            AutomationTarget::Mute => (tr("Mute").into(), 0.0, 1.0),
            AutomationTarget::Send(to) => {
                let name = doc.project().track(*to).map_or("?", |t| t.name.as_str());
                (trf!("Send → {name}", name = name), -60.0, 12.0)
            }
            AutomationTarget::Param { insert, param } => {
                let Some(ins) = track.inserts.iter().find(|i| i.id == *insert) else {
                    return ("?".into(), 0.0, 1.0);
                };
                if let Some(info) = audiostrada_dsp::info(&ins.effect) {
                    return info
                        .params
                        .iter()
                        .find(|p| p.id == param)
                        .map_or(("?".into(), 0.0, 1.0), |p| {
                            (format!("{}: {}", tr(info.name), tr(p.name)), p.min, p.max)
                        });
                }
                let key = (track.id, target.clone());
                if let Some(cached) = self.target_info.get(&key) {
                    return cached.clone();
                }
                let name = doc
                    .player()
                    .plugins()
                    .find(&ins.effect)
                    .map_or_else(|| ins.effect.clone(), |p| p.name.clone());
                let info = param
                    .parse::<u32>()
                    .ok()
                    .and_then(|id| {
                        let plugin = doc.player_mut().plugin(Channel::Track(track.id), *insert)?;
                        plugin.params().into_iter().find(|p| p.id == id)
                    })
                    .map_or((format!("{name}: {param}"), 0.0, 1.0), |p| {
                        (format!("{name}: {}", p.name), p.min as f32, p.max as f32)
                    });
                self.target_info.insert(key, info.clone());
                info
            }
        }
    }

    fn draw_headers(
        &mut self,
        ui: &mut Ui,
        header: Rect,
        doc: &mut Document,
        project: &Project,
        visuals: &egui::Visuals,
    ) {
        let painter = ui.painter().with_clip_rect(header);
        painter.rect_filled(header, 0.0, visuals.panel_fill);
        let rows = self.rows.clone();
        for (i, &(index, depth)) in rows.iter().enumerate() {
            let track = &project.tracks[index];
            let top = header.top() + self.tops[i] - self.scroll_y;
            let row = Rect::from_min_max(
                pos2(header.left(), top),
                pos2(header.right(), top + self.track_height),
            );
            if row.bottom() < header.top() || row.top() > header.bottom() {
                continue;
            }
            // The whole row selects, drags and opens the menu; its buttons come above it.
            let response = ui.interact(
                row,
                ui.scope_id().with(("header", track.id.0)),
                Sense::click_and_drag(),
            );
            let color = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);
            painter.rect_filled(
                Rect::from_min_size(row.min, vec2(4.0, row.height() - 1.0)),
                0.0,
                color,
            );
            if doc.is_track_selected(track.id) {
                painter.rect_filled(
                    row.shrink(1.0),
                    2.0,
                    visuals.selection.bg_fill.gamma_multiply(0.15),
                );
            }
            // An armed track shows it across its header.
            if track.armed {
                painter.rect_filled(
                    row.shrink(1.0),
                    2.0,
                    crate::theme::RECORD.gamma_multiply(0.12),
                );
                painter.rect_stroke(
                    row.shrink(1.0),
                    2.0,
                    Stroke::new(1.0, crate::theme::RECORD.gamma_multiply(0.6)),
                    StrokeKind::Inside,
                );
            }
            painter.line_segment(
                [
                    pos2(row.left(), row.bottom()),
                    pos2(row.right(), row.bottom()),
                ],
                Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            );
            let id = track.id;
            let vca = track.kind == TrackKind::Vca;
            let mut x = row.left() + 10.0 + depth as f32 * 12.0;
            if routing::has_children(project, id) {
                let arrow = Rect::from_min_size(pos2(x - 2.0, row.top() + 4.0), vec2(16.0, 18.0));
                let text = if track.collapsed { "▸" } else { "▾" };
                if ui
                    .put(arrow, egui::Button::new(text).frame(false))
                    .on_hover_text(tr("Show or hide the folder contents"))
                    .clicked()
                {
                    let collapsed = !track.collapsed;
                    doc.history_mut().modify_unrecorded(|p| {
                        if let Some(t) = p.track_mut(id) {
                            t.collapsed = collapsed;
                        }
                    });
                }
                x += 16.0;
            }
            let group = audiostrada_core::groups::group_of(project, id).map(|g| &project.groups[g]);
            // Up to the automation buttons (and the group tag), cut with "…" beyond.
            let buttons = if vca {
                6.0
            } else if track.show_automation {
                56.0
            } else {
                32.0
            };
            let max_width =
                (row.right() - buttons - x - if group.is_some() { 22.0 } else { 0.0 }).max(20.0);
            let mut job = egui::text::LayoutJob::simple_singleline(
                track.name.clone(),
                crate::theme::strong(13.0),
                visuals.strong_text_color(),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let elided = galley.elided;
            let name = Rect::from_min_size(pos2(x, row.top() + 6.0), galley.size());
            let renaming = self.renaming.as_ref().is_some_and(|(t, _)| *t == id);
            if !renaming {
                painter.galley(name.min, galley, visuals.strong_text_color());
            }
            if let Some(g) = group {
                let c = Color32::from_rgb(g.color[0], g.color[1], g.color[2]);
                let c = if g.active { c } else { c.gamma_multiply(0.4) };
                let tag = Rect::from_min_size(
                    pos2(name.right() + 6.0, name.top() + 1.0),
                    vec2(14.0, 13.0),
                );
                painter.rect_filled(tag, 3.0, c);
                painter.text(
                    tag.center(),
                    Align2::CENTER_CENTER,
                    g.name.chars().next().unwrap_or('G').to_string(),
                    FontId::proportional(9.5),
                    Color32::BLACK,
                );
            }
            if row.height() > 54.0 {
                painter.text(
                    pos2(x, row.top() + 24.0),
                    Align2::LEFT_TOP,
                    if vca {
                        "VCA"
                    } else if track.frozen.is_some() {
                        tr("Frozen")
                    } else {
                        track.playlist.as_str()
                    },
                    FontId::proportional(10.0),
                    visuals.weak_text_color(),
                );
            }
            let y = row.bottom() - 26.0;
            let button = |ui: &mut Ui, x: f32, text: &str, on: bool, on_color: Color32| {
                let rect = Rect::from_min_size(pos2(row.left() + x, y), vec2(24.0, 20.0));
                let response = ui.interact(rect, ui.scope_id().with((text, id.0)), Sense::click());
                crate::theme::paint_letter_toggle(ui, rect, &response, text, on, on_color);
                let hint = if text == "M" { tr("Mute") } else { tr("Solo") };
                response.on_hover_text(hint).clicked()
            };
            if row.bottom() - 26.0 > header.top() {
                if button(ui, 10.0, "M", track.mute, crate::theme::MUTE) {
                    let mute = !track.mute;
                    doc.history_mut().edit(tr("Mute"), |p| {
                        audiostrada_core::groups::set_switch(
                            p,
                            id,
                            audiostrada_core::groups::Link::Mute,
                            mute,
                        )
                    });
                }
                if button(ui, 38.0, "S", track.solo, crate::theme::SOLO) {
                    let solo = !track.solo;
                    doc.history_mut().edit(tr("Solo"), |p| {
                        audiostrada_core::groups::set_switch(
                            p,
                            id,
                            audiostrada_core::groups::Link::Solo,
                            solo,
                        )
                    });
                }
                let arm = Rect::from_min_size(pos2(row.left() + 66.0, y), vec2(24.0, 20.0));
                let arm_response = (!vca).then(|| {
                    let response =
                        ui.interact(arm, ui.scope_id().with(("arm", id.0)), Sense::click());
                    crate::theme::paint_arm_toggle(ui, arm, &response, track.armed);
                    response.on_hover_text(tr("Record arm"))
                });
                if arm_response.is_some_and(|r| r.clicked()) {
                    let armed = !track.armed;
                    doc.history_mut().modify_unrecorded(|p| {
                        audiostrada_core::groups::set_switch(
                            p,
                            id,
                            audiostrada_core::groups::Link::Arm,
                            armed,
                        )
                    });
                }
                let input_label = if track.stereo {
                    trf!(
                        "In {first}-{second}",
                        first = track.input + 1,
                        second = track.input + 2
                    )
                } else {
                    trf!("In {input}", input = track.input + 1)
                };
                if !vca {
                    let combo_rect =
                        Rect::from_min_size(pos2(row.left() + 96.0, y), vec2(76.0, 20.0));
                    let mut ui_child = ui.new_child(egui::UiBuilder::new().max_rect(combo_rect));
                    egui::ComboBox::from_id_salt(("input", id.0))
                        .width(70.0)
                        .selected_text(input_label)
                        .show_ui(&mut ui_child, |ui| {
                            crate::mixer::input_choices(ui, doc, track)
                        });
                }
            }
            if renaming {
                self.rename_field(ui, doc, id, name.with_max_x(name.left() + max_width));
            } else if response.double_clicked()
                && response
                    .interact_pointer_pos()
                    .is_some_and(|p| name.expand(4.0).contains(p))
            {
                self.renaming = Some((id, track.name.clone()));
            }
            if response.clicked() {
                let mode = SelectMode::from_modifiers(ui.input(|i| i.modifiers));
                doc.select_track(id, mode);
            }
            if response.secondary_clicked() && !doc.is_track_selected(id) {
                doc.select_track(id, SelectMode::Only);
            }
            if response.drag_started() {
                let dragged = if doc.is_track_selected(id) {
                    doc.selected_tracks().to_vec()
                } else {
                    vec![id]
                };
                response.dnd_set_drag_payload(crate::mixer::DraggedTracks(dragged));
            }
            // Tracks dropped on a header move above or below it.
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                let after = pointer.y > row.center().y;
                if response
                    .dnd_hover_payload::<crate::mixer::DraggedTracks>()
                    .is_some()
                {
                    let line = if after { row.bottom() } else { row.top() };
                    painter.line_segment(
                        [pos2(row.left(), line), pos2(row.right(), line)],
                        Stroke::new(3.0, visuals.selection.bg_fill),
                    );
                }
                if let Some(dragged) = response.dnd_release_payload::<crate::mixer::DraggedTracks>()
                    && !dragged.0.contains(&id)
                {
                    doc.history_mut().edit(tr("Move tracks"), |p| {
                        routing::move_tracks(p, &dragged.0, id, after)
                    });
                }
            }
            let hint = tr(
                "Click to select (Ctrl adds, Shift extends), double-click the name to rename, \
                 drag to reorder, right-click for options",
            );
            // The whole name when it is cut.
            let over_name = elided && ui.rect_contains_pointer(name);
            response
                .on_hover_text(if over_name {
                    format!("{}\n\n{hint}", track.name)
                } else {
                    hint.to_owned()
                })
                .context_menu(|ui| self.header_menu(ui, doc, id));
            if vca {
                continue;
            }
            // Automation: show/hide lanes, add a lane, lane headers.
            let a_rect =
                Rect::from_min_size(pos2(row.right() - 26.0, row.top() + 4.0), vec2(22.0, 18.0));
            if row.top() + 4.0 > header.top() {
                let shown = track.show_automation;
                let button = egui::Button::new(egui::RichText::new("A").size(11.0)).selected(shown);
                if ui
                    .put(a_rect, button)
                    .on_hover_text(tr("Show automation lanes"))
                    .clicked()
                {
                    doc.history_mut().modify_unrecorded(|p| {
                        if let Some(t) = p.track_mut(id) {
                            t.show_automation = !shown;
                        }
                    });
                }
                if shown {
                    let plus = Rect::from_min_size(
                        pos2(row.right() - 50.0, row.top() + 4.0),
                        vec2(22.0, 18.0),
                    );
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(plus));
                    child
                        .menu_button("+", |ui| self.add_lane_menu(ui, doc, project, track))
                        .response
                        .on_hover_text(tr("Add an automation lane"));
                }
            }
            if track.show_takes {
                for (k, playlist) in track.alternates.iter().enumerate() {
                    let top = row.bottom() + k as f32 * TAKE_HEIGHT;
                    let take_row = Rect::from_min_max(
                        pos2(header.left(), top),
                        pos2(header.right(), top + TAKE_HEIGHT),
                    );
                    if take_row.bottom() < header.top() || take_row.top() > header.bottom() {
                        continue;
                    }
                    painter.rect_filled(
                        take_row.shrink2(vec2(0.0, 0.5)),
                        0.0,
                        visuals.extreme_bg_color,
                    );
                    painter.rect_filled(
                        Rect::from_min_size(take_row.min, vec2(4.0, TAKE_HEIGHT)),
                        0.0,
                        color.gamma_multiply(0.3),
                    );
                    painter.text(
                        pos2(
                            take_row.left() + 12.0 + depth as f32 * 12.0,
                            take_row.center().y,
                        ),
                        Align2::LEFT_CENTER,
                        &playlist.name,
                        FontId::proportional(11.0),
                        visuals.weak_text_color(),
                    );
                    let use_rect = Rect::from_min_size(
                        pos2(take_row.right() - 44.0, take_row.center().y - 9.0),
                        vec2(40.0, 18.0),
                    );
                    if ui
                        .put(
                            use_rect,
                            egui::Button::new(egui::RichText::new(tr("Use")).size(10.0)),
                        )
                        .on_hover_text(tr("Play this playlist instead of the current one"))
                        .clicked()
                    {
                        doc.history_mut()
                            .edit(tr("Select playlist"), |p| edit::select_playlist(p, id, k));
                    }
                }
            }
            for (k, li) in shown_lanes(track).into_iter().enumerate() {
                let top = row.bottom() + takes_height(track) + k as f32 * LANE_HEIGHT;
                let lane_row = Rect::from_min_max(
                    pos2(header.left(), top),
                    pos2(header.right(), top + LANE_HEIGHT),
                );
                if lane_row.bottom() < header.top() || lane_row.top() > header.bottom() {
                    continue;
                }
                painter.rect_filled(
                    lane_row.shrink2(vec2(0.0, 0.5)),
                    0.0,
                    visuals.extreme_bg_color,
                );
                painter.rect_filled(
                    Rect::from_min_size(lane_row.min, vec2(4.0, LANE_HEIGHT)),
                    0.0,
                    color.gamma_multiply(0.5),
                );
                let name = self
                    .lane_info
                    .get(&(id, li))
                    .map_or("?".to_owned(), |i| i.0.clone());
                painter.text(
                    pos2(
                        lane_row.left() + 12.0 + depth as f32 * 12.0,
                        lane_row.top() + 5.0,
                    ),
                    Align2::LEFT_TOP,
                    name,
                    FontId::proportional(11.0),
                    visuals.text_color(),
                );
                let lane = &track.automation[li];
                let y = lane_row.bottom() - 22.0;
                let on = Rect::from_min_size(pos2(lane_row.left() + 12.0, y), vec2(40.0, 18.0));
                let toggle = egui::Button::new(
                    egui::RichText::new(if lane.enabled { "On" } else { "Off" }).size(10.0),
                )
                .selected(lane.enabled);
                if ui
                    .put(on, toggle)
                    .on_hover_text(tr("Play this lane"))
                    .clicked()
                {
                    let enabled = !lane.enabled;
                    doc.history_mut().edit(tr("Automation lane"), |p| {
                        if let Some(l) = p.track_mut(id).and_then(|t| t.automation.get_mut(li)) {
                            l.enabled = enabled;
                        }
                    });
                }
                let hide = Rect::from_min_size(pos2(lane_row.left() + 56.0, y), vec2(58.0, 18.0));
                if ui
                    .put(
                        hide,
                        egui::Button::new(egui::RichText::new(tr("Hide")).size(10.0)),
                    )
                    .clicked()
                {
                    doc.history_mut().modify_unrecorded(|p| {
                        if let Some(l) = p.track_mut(id).and_then(|t| t.automation.get_mut(li)) {
                            l.visible = false;
                        }
                    });
                }
                let remove =
                    Rect::from_min_size(pos2(lane_row.right() - 26.0, y), vec2(22.0, 18.0));
                if ui
                    .put(remove, egui::Button::new("✕"))
                    .on_hover_text(tr("Delete this lane"))
                    .clicked()
                {
                    doc.history_mut().edit(tr("Delete automation lane"), |p| {
                        if let Some(t) = p.track_mut(id)
                            && li < t.automation.len()
                        {
                            t.automation.remove(li);
                        }
                    });
                }
            }
        }
        // Below the tracks: a click clears the selection, a right-click adds tracks.
        let end = (header.top() + self.content_height - self.scroll_y).max(header.top());
        let empty = Rect::from_min_max(pos2(header.left(), end), header.max);
        if empty.height() > 4.0 {
            let response = ui.interact(empty, ui.scope_id().with("headers-empty"), Sense::click());
            if response.clicked() {
                doc.set_track_selection(Vec::new());
            }
            response.context_menu(|ui| {
                for action in [
                    Action::AddTrack,
                    Action::AddStereoTrack,
                    Action::AddTracks,
                    Action::AddInstrumentTrack,
                    Action::AddDrumTrack,
                    Action::AddVca,
                ] {
                    if ui.button(action.label()).clicked() {
                        self.requests.push(action);
                        ui.close();
                    }
                }
            });
        }
    }

    /// Lanes that can be added to a track (shown again if hidden).
    /// Clips from the last clicked one to `clip`: those overlapping the time between them, on the
    /// shown tracks between theirs.
    fn clips_between(&self, project: &Project, clip: ClipId) -> Vec<ClipId> {
        let anchor = self
            .clip_anchor
            .or(self.selection.first().copied())
            .unwrap_or(clip);
        let row = |id: ClipId| {
            let (t, _) = edit::locate(project, id)?;
            self.rows.iter().position(|&(index, _)| index == t)
        };
        let (Some(a), Some(b), Some(ca), Some(cb)) = (
            row(anchor),
            row(clip),
            edit::clip(project, anchor),
            edit::clip(project, clip),
        ) else {
            return vec![clip];
        };
        let (start, end) = (ca.start.min(cb.start), ca.end().max(cb.end()));
        self.rows[a.min(b)..=a.max(b)]
            .iter()
            .flat_map(|&(index, _)| &project.tracks[index].clips)
            .filter(|c| c.start < end && c.end() > start)
            .map(|c| c.id)
            .collect()
    }

    /// The text field over a track's name while renaming it: Enter or a click elsewhere keeps
    /// the name, Escape leaves it as it was.
    fn rename_field(&mut self, ui: &mut Ui, doc: &mut Document, id: TrackId, rect: Rect) {
        let Some((_, text)) = self.renaming.as_mut() else {
            return;
        };
        let edit_id = ui.scope_id().with(("rename", id.0));
        let response = ui.put(
            rect.expand2(vec2(2.0, 2.0)).with_min_y(rect.top() - 3.0),
            egui::TextEdit::singleline(text)
                .id(edit_id)
                .font(crate::theme::strong(13.0))
                .margin(vec2(2.0, 1.0)),
        );
        if !response.has_focus() && !response.lost_focus() {
            response.request_focus();
            // The whole name selected, to type a new one.
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
                )));
            state.store(ui.ctx(), edit_id);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.renaming = None;
        } else if response.lost_focus() {
            let name = text.trim().to_owned();
            self.renaming = None;
            if !name.is_empty() {
                doc.history_mut().edit(tr("Rename track"), |p| {
                    if let Some(t) = p.track_mut(id) {
                        t.name = name;
                    }
                });
            }
        }
    }

    fn add_lane_menu(&mut self, ui: &mut Ui, doc: &mut Document, project: &Project, track: &Track) {
        let mut targets = vec![
            AutomationTarget::Gain,
            AutomationTarget::Pan,
            AutomationTarget::Mute,
        ];
        targets.extend(track.sends.iter().map(|s| AutomationTarget::Send(s.target)));
        for insert in &track.inserts {
            if let Some(info) = audiostrada_dsp::info(&insert.effect) {
                targets.extend(
                    info.params
                        .iter()
                        .filter(|p| {
                            crate::effect_editors::automatable(&insert.effect, &insert.params, p.id)
                        })
                        .map(|p| AutomationTarget::Param {
                            insert: insert.id,
                            param: p.id.to_owned(),
                        }),
                );
            } else if let Some(plugin) =
                doc.player_mut().plugin(Channel::Track(track.id), insert.id)
            {
                targets.extend(
                    plugin
                        .params()
                        .into_iter()
                        .filter(|p| !p.hidden && !p.read_only)
                        .take(200)
                        .map(|p| AutomationTarget::Param {
                            insert: insert.id,
                            param: p.id.to_string(),
                        }),
                );
            }
        }
        let _ = project;
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .show(ui, |ui| {
                for target in targets {
                    let existing = track.automation.iter().position(|l| l.target == target);
                    if existing.is_some_and(|i| track.automation[i].visible) {
                        continue;
                    }
                    let (name, ..) = self.target_info(doc, track, &target);
                    if ui.button(name).clicked() {
                        let id = track.id;
                        doc.history_mut().edit(tr("Add automation lane"), |p| {
                            if let Some(t) = p.track_mut(id) {
                                audiostrada_core::automation::lane_mut(t, &target).visible = true;
                                t.show_automation = true;
                            }
                        });
                        ui.close();
                    }
                }
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn interact(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        settings: &EditSettings,
        scale: Scale,
        lanes: Rect,
        ruler: Rect,
        hovered: Option<(ClipId, TrackId, Rect)>,
    ) {
        let ruler_response =
            ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        let lanes_response =
            ui.interact(lanes, ui.scope_id().with("lanes"), Sense::click_and_drag());
        let modifiers = ui.input(|i| i.modifiers);

        // Smart Tool zone under the pointer.
        let zone = |pos: Pos2| -> Option<(DragKind, ClipId)> {
            let (clip_id, _, rect) = hovered?;
            let clip = edit::clip(project, clip_id)?;
            let px = |s: u64| (s as f64 / scale.zoom) as f32;
            let kind = if pos.y < rect.top() + FADE_ZONE_HEIGHT
                && pos.x < rect.left() + px(clip.fade_in) + 10.0
            {
                DragKind::FadeIn
            } else if pos.y < rect.top() + FADE_ZONE_HEIGHT
                && pos.x > rect.right() - px(clip.fade_out) - 10.0
            {
                DragKind::FadeOut
            } else if pos.x < rect.left() + EDGE_ZONE {
                DragKind::TrimStart
            } else if pos.x > rect.right() - EDGE_ZONE && modifiers.alt && !clip.is_midi() {
                DragKind::Stretch
            } else if pos.x > rect.right() - EDGE_ZONE {
                DragKind::TrimEnd
            } else if modifiers.command {
                DragKind::Gain
            } else {
                DragKind::Move {
                    copy: modifiers.alt,
                }
            };
            Some((kind, clip_id))
        };

        // An edge of the range, on its tracks or in the ruler: the edge that stays.
        let range_view = self.range.as_ref().map(|r| {
            let spans: Vec<(f32, f32)> = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, (index, _))| r.tracks.contains(&project.tracks[*index].id))
                .map(|(i, _)| {
                    let top = lanes.top() + self.tops[i] - self.scroll_y;
                    (top, top + self.track_height)
                })
                .collect();
            (r.start, r.end, spans)
        });
        let range_edge = |pos: Pos2, in_ruler: bool| -> Option<u64> {
            let (start, end, spans) = range_view.as_ref()?;
            let on_track = in_ruler || spans.iter().any(|&(a, b)| pos.y >= a && pos.y < b);
            let (d0, d1) = (
                (pos.x - scale.x(*start)).abs(),
                (pos.x - scale.x(*end)).abs(),
            );
            match on_track && d0.min(d1) <= RANGE_GRAB {
                true if d0 <= d1 => Some(*end),
                true => Some(*start),
                false => None,
            }
        };
        if self.drag.is_none()
            && let Some(pos) = ruler_response.hover_pos()
            && range_edge(pos, true).is_some()
        {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if self.drag.is_none()
            && let Some(pos) = lanes_response.hover_pos()
        {
            let cursor = match zone(pos).map(|z| z.0) {
                _ if range_edge(pos, false).is_some() => CursorIcon::ResizeHorizontal,
                Some(DragKind::TrimStart | DragKind::TrimEnd) => CursorIcon::ResizeHorizontal,
                Some(DragKind::Stretch) => CursorIcon::ResizeColumn,
                Some(DragKind::FadeIn | DragKind::FadeOut) => CursorIcon::ResizeNwSe,
                Some(DragKind::Gain) => CursorIcon::ResizeVertical,
                Some(DragKind::Move { copy: true }) => CursorIcon::Copy,
                Some(DragKind::Move { .. }) => CursorIcon::Grab,
                _ => CursorIcon::Text,
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        // Files dragged from the browser.
        if let Some(pos) = lanes_response.dnd_hover_payload::<DraggedFile>().and(
            lanes_response
                .hover_pos()
                .or(ui.input(|i| i.pointer.hover_pos())),
        ) {
            let x = scale.x(settings.snap(project, scale.sample(pos.x)));
            ui.painter().line_segment(
                [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
        }
        if let Some(file) = lanes_response.dnd_release_payload::<DraggedFile>()
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            let track = self
                .track_at(project, pos.y - lanes.top())
                .filter(|t| t.kind == TrackKind::Audio)
                .map(|t| t.id);
            let position = settings.snap(project, scale.sample(pos.x));
            doc.import_at(std::slice::from_ref(&file.0), track, position);
        }

        // Starting a drag.
        if ruler_response.drag_started()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let sample = settings.snap(project, scale.sample(pos.x));
            let marker = project
                .markers
                .iter()
                .find(|m| (scale.x(m.position) - pos.x).abs() < 6.0 && pos.y > ruler.top() + 10.0);
            // The loop band (or Alt) sets the loop, from the edge not grabbed; Shift the punch.
            let in_band = pos.y > ruler.bottom() - LOOP_BAND || modifiers.alt;
            let loop_edge = project.loop_range.and_then(|(a, b)| {
                let (d0, d1) = ((pos.x - scale.x(a)).abs(), (pos.x - scale.x(b)).abs());
                (d0.min(d1) <= RANGE_GRAB).then_some(if d0 <= d1 { b } else { a })
            });
            let kind = if modifiers.shift {
                DragKind::Loop {
                    anchor: sample,
                    punch: true,
                }
            } else if in_band {
                DragKind::Loop {
                    anchor: loop_edge.unwrap_or(sample),
                    punch: false,
                }
            } else if let Some(anchor) = range_edge(pos, true) {
                DragKind::RangeEdge { anchor }
            } else if let Some(m) = marker {
                DragKind::Marker(m.id)
            } else {
                DragKind::RulerRange { anchor: sample }
            };
            self.drag = Some(Drag {
                kind,
                clip: None,
                origin: pos,
                original: doc.project().clone(),
            });
        }
        // Automation lanes: hit test of the pointer.
        let lane_hit = |pos: Pos2| -> Option<(TrackId, usize, Rect, Option<usize>)> {
            let (r, li, top) = self.lane_at(project, pos.y - lanes.top())?;
            let track = &project.tracks[self.rows[r].0];
            let lane = track.automation.get(li)?;
            let (_, min, max) = *self
                .lane_info
                .get(&(track.id, li))
                .map(|i| (&i.0, i.1, i.2))
                .as_ref()?;
            let rect = Rect::from_min_max(
                pos2(lanes.left(), lanes.top() + top),
                pos2(lanes.right(), lanes.top() + top + LANE_HEIGHT),
            );
            let point = lane
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let d = pos2(scale.x(p.time), value_y(rect, p.value, min, max)) - pos;
                    (i, d.length())
                })
                .filter(|&(_, d)| d < POINT_GRAB)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            Some((track.id, li, rect, point))
        };
        // A drag starts a few pixels away from the press: hit test where the button went down.
        let press = ui.input(|i| i.pointer.press_origin());
        let hit = if lanes_response.drag_started() {
            press.or(lanes_response.interact_pointer_pos())
        } else {
            lanes_response
                .interact_pointer_pos()
                .or(lanes_response.hover_pos())
        }
        .and_then(lane_hit);
        let start_pos = press.or(lanes_response.interact_pointer_pos());
        if self.drag.is_none() && hit.is_some() && lanes_response.hovered() {
            let point = hit.as_ref().and_then(|h| h.3);
            ui.ctx().set_cursor_icon(if point.is_some() {
                CursorIcon::Grab
            } else {
                CursorIcon::Crosshair
            });
        }
        if lanes_response.drag_started()
            && let Some((track, li, rect, Some(point))) = hit
        {
            self.drag = Some(Drag {
                kind: DragKind::Point(track, li, point, rect.top()),
                clip: None,
                origin: start_pos.unwrap_or(rect.center()),
                original: doc.project().clone(),
            });
        } else if lanes_response.drag_started()
            && let Some(pos) = start_pos
            && let Some((track, playlist)) = self.take_at(project, pos.y - lanes.top())
        {
            self.drag = Some(Drag {
                kind: DragKind::Comp(track, playlist, settings.snap(project, scale.sample(pos.x))),
                clip: None,
                origin: pos,
                original: doc.project().clone(),
            });
        } else if lanes_response.drag_started()
            && hit.is_none()
            && let Some(pos) = start_pos
        {
            let edge = range_edge(pos, false);
            let (kind, clip) = match zone(pos) {
                _ if let Some(anchor) = edge => (DragKind::RangeEdge { anchor }, None),
                _ if modifiers.shift && !modifiers.command => {
                    (DragKind::Range { anchor: pos }, None)
                }
                Some((kind, clip)) => {
                    if !self.selection.contains(&clip) {
                        if !modifiers.shift {
                            self.selection.clear();
                        }
                        self.selection.push(clip);
                    }
                    (kind, Some(clip))
                }
                None => (DragKind::Select { anchor: pos }, None),
            };
            if let Some((_, track, _)) = hovered
                && !doc.is_track_selected(track)
            {
                doc.select_track(track, SelectMode::Only);
            }
            self.drag = Some(Drag {
                kind,
                clip,
                origin: pos,
                original: doc.project().clone(),
            });
        }

        // Updating the preview.
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let (Some(drag), Some(pos)) = (&self.drag, pointer) {
            let delta = pos - drag.origin;
            let mut preview = drag.original.clone();
            let track_offset = self.track_offset(drag, pos.y - lanes.top());
            match drag.kind {
                DragKind::Move { copy } => {
                    let mut ids = self.selection.clone();
                    if copy {
                        ids = edit::duplicate(&mut preview, &ids);
                        // Duplicates land after the selection; bring them back under the pointer.
                        let shift = ids
                            .iter()
                            .zip(&self.selection)
                            .filter_map(|(c, o)| {
                                Some(
                                    edit::clip(&preview, *c)?.start as i64
                                        - edit::clip(&drag.original, *o)?.start as i64,
                                )
                            })
                            .next()
                            .unwrap_or(0);
                        edit::move_clips(&mut preview, &ids, -shift, 0);
                    }
                    let mut samples = scale.samples(delta.x);
                    if let Some(c) = drag.clip.and_then(|c| edit::clip(&drag.original, c)) {
                        let target = (c.start as i64 + samples).max(0) as u64;
                        samples = settings.snap(project, target) as i64 - c.start as i64;
                    }
                    edit::move_clips(&mut preview, &ids, samples, track_offset);
                }
                DragKind::TrimStart
                | DragKind::TrimEnd
                | DragKind::Stretch
                | DragKind::FadeIn
                | DragKind::FadeOut
                | DragKind::Gain => {
                    if let Some(c) = drag
                        .clip
                        .and_then(|c| edit::clip(&drag.original, c))
                        .cloned()
                    {
                        let at = |x: u64| settings.snap(project, x);
                        // Trims, fades and gain act on every selected clip by the same amount.
                        let others: Vec<audiostrada_core::Clip> = if self.selection.contains(&c.id)
                        {
                            self.selection
                                .iter()
                                .filter(|&&id| id != c.id)
                                .filter_map(|&id| edit::clip(&drag.original, id).cloned())
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let shifted = |v: u64, d: i64| (v as i64 + d).max(0) as u64;
                        match drag.kind {
                            DragKind::TrimStart => {
                                let target =
                                    at((c.start as i64 + scale.samples(delta.x)).max(0) as u64);
                                edit::trim_start(&mut preview, c.id, target);
                                let d = target as i64 - c.start as i64;
                                for o in &others {
                                    edit::trim_start(&mut preview, o.id, shifted(o.start, d));
                                }
                            }
                            DragKind::TrimEnd => {
                                let target =
                                    at((c.end() as i64 + scale.samples(delta.x)).max(0) as u64);
                                edit::trim_end(&mut preview, c.id, target);
                                let d = target as i64 - c.end() as i64;
                                for o in &others {
                                    edit::trim_end(&mut preview, o.id, shifted(o.end(), d));
                                }
                                if settings.mode == EditMode::Shuffle && others.is_empty() {
                                    ripple_after(&mut preview, &drag.original, c.id);
                                }
                            }
                            DragKind::Stretch => {
                                let target =
                                    at((c.end() as i64 + scale.samples(delta.x)).max(0) as u64);
                                edit::stretch_clip(
                                    &mut preview,
                                    c.id,
                                    target.saturating_sub(c.start),
                                );
                            }
                            DragKind::FadeIn => {
                                let d = scale.samples(delta.x);
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::set_fades(
                                        &mut preview,
                                        o.id,
                                        shifted(o.fade_in, d),
                                        o.fade_out,
                                    );
                                }
                            }
                            DragKind::FadeOut => {
                                let d = -scale.samples(delta.x);
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::set_fades(
                                        &mut preview,
                                        o.id,
                                        o.fade_in,
                                        shifted(o.fade_out, d),
                                    );
                                }
                            }
                            DragKind::Gain => {
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::set_clip_gain(
                                        &mut preview,
                                        o.id,
                                        o.gain_db - delta.y * 0.1,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                DragKind::Loop { anchor, punch } => {
                    let other = settings.snap(project, scale.sample(pos.x));
                    let (a, b) = (anchor.min(other), anchor.max(other));
                    let range = (b > a).then_some((a, b));
                    if punch {
                        preview.punch_range = range;
                    } else {
                        preview.loop_range = range;
                        preview.loop_off = false;
                        // A linked selection moves with the loop.
                        if !project.loop_fixed
                            && let (Some(r), Some((a, b))) = (&mut self.range, range)
                        {
                            (r.start, r.end) = (a, b);
                            self.linked = Some((a, b));
                        }
                    }
                }
                DragKind::Marker(id) => {
                    let target = settings.snap(project, scale.sample(pos.x));
                    if let Some(m) = preview.markers.iter_mut().find(|m| m.id == id) {
                        m.position = target;
                    }
                    preview.markers.sort_by_key(|m| (m.position, m.id));
                }
                DragKind::Point(track, li, point, top) => {
                    if let Some((_, min, max)) = self.lane_info.get(&(track, li)).cloned() {
                        let rect = Rect::from_min_max(
                            pos2(lanes.left(), top),
                            pos2(lanes.right(), top + LANE_HEIGHT),
                        );
                        let time = settings.snap(project, scale.sample(pos.x));
                        let value = y_value(rect, pos.y, min, max);
                        if let Some(lane) = preview
                            .track_mut(track)
                            .and_then(|t| t.automation.get_mut(li))
                        {
                            lane.move_point(point, time, value);
                        }
                    }
                }
                DragKind::Comp(track, playlist, anchor) => {
                    let other = settings.snap(project, scale.sample(pos.x));
                    let (a, b) = (anchor.min(other), anchor.max(other));
                    edit::comp_range(&mut preview, track, playlist, a, b);
                    self.comping = (b > a).then_some((track, playlist, a, b));
                }
                DragKind::Range { anchor } => {
                    let (a, b) = (
                        settings.snap(project, scale.sample(anchor.x.min(pos.x))),
                        settings.snap(project, scale.sample(anchor.x.max(pos.x))),
                    );
                    let first = self.row_at(anchor.y.min(pos.y) - lanes.top()).unwrap_or(0);
                    let last = self
                        .row_at(anchor.y.max(pos.y) - lanes.top())
                        .unwrap_or(self.rows.len().saturating_sub(1));
                    let tracks = self
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i >= first && *i <= last)
                        .map(|(_, &(index, _))| project.tracks[index].id)
                        .collect();
                    self.range = (b > a).then_some(Range {
                        start: a,
                        end: b,
                        tracks,
                    });
                    self.selection.clear();
                }
                DragKind::RulerRange { anchor } => {
                    let at = settings.snap(project, scale.sample(pos.x));
                    let tracks = self
                        .rows
                        .iter()
                        .map(|&(index, _)| project.tracks[index].id)
                        .collect();
                    self.range = (at != anchor).then_some(Range {
                        start: anchor.min(at),
                        end: anchor.max(at),
                        tracks,
                    });
                    self.selection.clear();
                }
                DragKind::RangeEdge { anchor } => {
                    let at = settings.snap(project, scale.sample(pos.x));
                    if let Some(r) = &mut self.range
                        && at != anchor
                    {
                        (r.start, r.end) = (anchor.min(at), anchor.max(at));
                    }
                }
                DragKind::Select { anchor } => {
                    let r = Rect::from_two_pos(anchor, pos);
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.0, ui.visuals().selection.bg_fill),
                        StrokeKind::Inside,
                    );
                    let first = self.row_at(r.top() - lanes.top()).unwrap_or(0);
                    let last = self
                        .row_at(r.bottom() - lanes.top())
                        .unwrap_or(self.rows.len());
                    let (s0, s1) = (scale.sample(r.left()), scale.sample(r.right()));
                    self.selection = self
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i >= first && *i <= last)
                        .map(|(_, &(index, _))| &project.tracks[index])
                        .flat_map(|t| {
                            t.clips
                                .iter()
                                .filter(|c| c.start < s1 && c.end() > s0)
                                .map(|c| c.id)
                        })
                        .collect();
                }
            }
            if !matches!(
                drag.kind,
                DragKind::Select { .. }
                    | DragKind::Range { .. }
                    | DragKind::RangeEdge { .. }
                    | DragKind::RulerRange { .. }
            ) {
                self.preview = Some(preview);
            }
        }

        // Committing.
        if (lanes_response.drag_stopped() || ruler_response.drag_stopped())
            && let Some(drag) = self.drag.take()
            && let Some(preview) = self.preview.take()
        {
            let label = match drag.kind {
                DragKind::Move { copy: true } => tr("Copy clips"),
                DragKind::Move { .. } => tr("Move clips"),
                DragKind::TrimStart | DragKind::TrimEnd => tr("Trim clip"),
                DragKind::Stretch => tr("Stretch clip"),
                DragKind::FadeIn | DragKind::FadeOut => tr("Fade"),
                DragKind::Gain => tr("Clip gain"),
                DragKind::Loop { punch: true, .. } => tr("Set punch range"),
                DragKind::Loop { .. } => tr("Set loop"),
                DragKind::Marker(_) => tr("Move marker"),
                DragKind::Select { .. }
                | DragKind::Range { .. }
                | DragKind::RangeEdge { .. }
                | DragKind::RulerRange { .. } => tr("Edit"),
                DragKind::Point(..) => tr("Move automation point"),
                DragKind::Comp(..) => tr("Comp takes"),
            };
            doc.history_mut().edit(label, |p| *p = preview);
        }
        if self.drag.is_none() {
            self.comping = None;
        }

        // A click on a take promotes the clip under the pointer.
        if lanes_response.clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
            && let Some((track, playlist)) = self.take_at(project, pos.y - lanes.top())
        {
            let at = scale.sample(pos.x);
            let clip = project
                .track(track)
                .and_then(|t| t.alternates.get(playlist))
                .and_then(|a| a.clips.iter().find(|c| c.start <= at && at < c.end()))
                .map(|c| c.id);
            if let Some(clip) = clip {
                doc.history_mut().edit(tr("Comp take"), |p| {
                    edit::promote_take(p, track, playlist, clip);
                });
            }
        }

        // Automation clicks: add a point, double-click removes, right-click edits the shape.
        if let Some((track, li, rect, point)) = hit {
            let pos = lanes_response
                .interact_pointer_pos()
                .unwrap_or(rect.center());
            if lanes_response.double_clicked()
                && let Some(point) = point
            {
                doc.history_mut().edit(tr("Delete automation point"), |p| {
                    if let Some(lane) = p.track_mut(track).and_then(|t| t.automation.get_mut(li))
                        && point < lane.points.len()
                    {
                        lane.points.remove(point);
                    }
                });
            } else if lanes_response.clicked()
                && point.is_none()
                && let Some((_, min, max)) = self.lane_info.get(&(track, li)).cloned()
            {
                let time = settings.snap(project, scale.sample(pos.x));
                let value = y_value(rect, pos.y, min, max);
                doc.history_mut().edit(tr("Add automation point"), |p| {
                    if let Some(lane) = p.track_mut(track).and_then(|t| t.automation.get_mut(li)) {
                        lane.add(time, value, CurveShape::Linear);
                    }
                });
            }
            if lanes_response.secondary_clicked() {
                self.point_menu = point.map(|p| (track, li, p));
            }
        }
        if lanes_response.secondary_clicked()
            && hit.is_none()
            && let Some(pos) = lanes_response.interact_pointer_pos()
        {
            let at = settings.snap(project, scale.sample(pos.x));
            self.point_menu = None;
            let row_track = self.track_at(project, pos.y - lanes.top()).map(|t| t.id);
            let in_range = self.range.as_ref().is_some_and(|r| {
                let s = scale.sample(pos.x);
                s >= r.start && s < r.end && row_track.is_some_and(|t| r.tracks.contains(&t))
            });
            self.menu = Some(match hovered {
                _ if in_range => menus::Menu::Range,
                Some((clip, track, _)) => {
                    if !doc.is_track_selected(track) {
                        doc.select_track(track, SelectMode::Only);
                    }
                    menus::Menu::Clip { clip, at }
                }
                None => menus::Menu::Lane {
                    track: self.track_at(project, pos.y - lanes.top()).map(|t| t.id),
                    at,
                },
            });
        } else if lanes_response.secondary_clicked() {
            self.menu = None;
        }
        let point_menu = self.point_menu;
        let menu = self.menu;
        lanes_response.context_menu(|ui| {
            let Some((track, li, point)) = point_menu else {
                match menu {
                    Some(menus::Menu::Clip { clip, at }) => self.clip_menu(ui, doc, clip, at),
                    Some(menus::Menu::Lane { track, at }) => self.lane_menu(ui, doc, track, at),
                    Some(menus::Menu::Range) => self.range_menu(ui, doc, settings),
                    None => ui.close(),
                }
                return;
            };
            ui.label(egui::RichText::new(tr("Curve to the next point")).weak());
            for (shape, name) in [
                (CurveShape::Linear, tr("Linear")),
                (CurveShape::Smooth, tr("Smooth")),
                (CurveShape::Step, tr("Step")),
            ] {
                if ui.button(name).clicked() {
                    doc.history_mut().edit(tr("Automation curve"), |p| {
                        if let Some(pt) = p
                            .track_mut(track)
                            .and_then(|t| t.automation.get_mut(li))
                            .and_then(|l| l.points.get_mut(point))
                        {
                            pt.shape = shape;
                        }
                    });
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("Delete point")).clicked() {
                doc.history_mut().edit(tr("Delete automation point"), |p| {
                    if let Some(lane) = p.track_mut(track).and_then(|t| t.automation.get_mut(li))
                        && point < lane.points.len()
                    {
                        lane.points.remove(point);
                    }
                });
                ui.close();
            }
        });

        // Double-click on an empty part of a track creates a MIDI clip.
        if hit.is_none()
            && hovered.is_none()
            && lanes_response.double_clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
            && let Some(track) = self.track_at(project, pos.y - lanes.top())
            && track.kind == TrackKind::Audio
        {
            self.pending_midi = Some((track.id, scale.sample(pos.x)));
        }

        // Clicks.
        if hit.is_none()
            && lanes_response.clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
        {
            if !modifiers.shift {
                self.range = None;
            }
            let extend = modifiers.shift && self.range.is_some();
            if extend {
                let at = settings.snap(project, scale.sample(pos.x));
                self.extend_range(project, at, self.row_at(pos.y - lanes.top()));
            }
            match hovered {
                _ if extend => {}
                Some((clip, track, _)) => {
                    // Ctrl toggles the clip, Shift selects from the last clicked one to it.
                    if modifiers.command {
                        if let Some(i) = self.selection.iter().position(|c| *c == clip) {
                            self.selection.remove(i);
                        } else {
                            self.selection.push(clip);
                        }
                        self.clip_anchor = Some(clip);
                    } else if modifiers.shift {
                        self.selection = self.clips_between(project, clip);
                    } else {
                        self.selection = vec![clip];
                        self.clip_anchor = Some(clip);
                    }
                    if modifiers.command || modifiers.shift {
                        for t in edit_tracks(project, &self.selection) {
                            if !doc.is_track_selected(t) {
                                doc.select_track(t, SelectMode::Toggle);
                            }
                        }
                    } else {
                        doc.select_track(track, SelectMode::Only);
                    }
                }
                None => {
                    self.selection.clear();
                    match self.track_at(project, pos.y - lanes.top()) {
                        Some(t) => doc.select_track(t.id, SelectMode::Only),
                        None => doc.set_track_selection(Vec::new()),
                    }
                }
            }
            let target = settings.snap(project, scale.sample(pos.x));
            doc.player_mut().locate(target);
        }
        if ruler_response.clicked()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let target = settings.snap(project, scale.sample(pos.x));
            doc.player_mut().locate(target);
        }
        if ruler_response.secondary_clicked()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let marker = project
                .markers
                .iter()
                .find(|m| (scale.x(m.position) - pos.x).abs() < 6.0)
                .map(|m| m.id);
            let at = settings.snap(project, scale.sample(pos.x));
            self.ruler_target = Some((at, marker));
        }
        if let Some((at, marker)) = self.ruler_target {
            ruler_response.context_menu(|ui| self.ruler_menu(ui, doc, at, marker));
        }
        if ruler_response.double_clicked() {
            if modifiers.shift {
                doc.history_mut()
                    .edit(tr("Punch off"), |p| p.punch_range = None);
            } else if project.loop_range.is_some() {
                let off = !project.loop_off;
                let label = if off { tr("Loop off") } else { tr("Loop on") };
                doc.history_mut().edit(label, |p| p.loop_off = off);
            }
        }
    }
}

const LANE_PAD: f32 = 5.0;

fn value_y(rect: Rect, value: f32, min: f32, max: f32) -> f32 {
    let f = ((value - min) / (max - min).max(1e-9)).clamp(0.0, 1.0);
    rect.bottom() - LANE_PAD - f * (rect.height() - 2.0 * LANE_PAD)
}

fn y_value(rect: Rect, y: f32, min: f32, max: f32) -> f32 {
    let f = ((rect.bottom() - LANE_PAD - y) / (rect.height() - 2.0 * LANE_PAD)).clamp(0.0, 1.0);
    min + f * (max - min)
}

/// Curve and points of an automation lane.
#[allow(clippy::too_many_arguments)]
fn draw_lane(
    painter: &egui::Painter,
    rect: Rect,
    lane: &audiostrada_core::automation::AutomationLane,
    min: f32,
    max: f32,
    scale: Scale,
    color: Color32,
    visuals: &egui::Visuals,
) {
    painter.rect_filled(rect.shrink2(vec2(0.0, 0.5)), 0.0, visuals.extreme_bg_color);
    if lane.points.is_empty() {
        painter.text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            tr("Click to add points, or write them during playback"),
            FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
        return;
    }
    let visible = rect.intersect(painter.clip_rect());
    let mut line = Vec::new();
    let mut x = visible.left();
    while x <= visible.right() + 2.0 {
        if let Some(v) = audiostrada_core::automation::value_at(&lane.points, scale.sample(x)) {
            line.push(pos2(x, value_y(rect, v, min, max)));
        }
        x += 2.0;
    }
    painter.add(Shape::line(line, Stroke::new(1.5, color)));
    for p in &lane.points {
        let px = scale.x(p.time);
        if px >= visible.left() - 4.0 && px <= visible.right() + 4.0 {
            let center = pos2(px, value_y(rect, p.value, min, max));
            painter.rect_filled(Rect::from_center_size(center, vec2(7.0, 7.0)), 1.0, color);
        }
    }
}

/// Miniature of a MIDI clip's notes.
fn draw_notes(
    painter: &egui::Painter,
    rect: Rect,
    clip: &audiostrada_core::Clip,
    notes: &[audiostrada_core::Note],
    zoom: f64,
    color: Color32,
) {
    let visible: Vec<&audiostrada_core::Note> = notes
        .iter()
        .filter(|n| n.start + n.length > clip.offset && n.start < clip.offset + clip.length)
        .collect();
    let (Some(low), Some(high)) = (
        visible.iter().map(|n| n.key).min(),
        visible.iter().map(|n| n.key).max(),
    ) else {
        return;
    };
    let span = f32::from(high - low).max(11.0) + 1.0;
    let row = rect.height() / span;
    let clip_rect = painter.clip_rect().intersect(rect);
    let painter = painter.with_clip_rect(clip_rect);
    for n in visible {
        let x0 = rect.left() + ((n.start as f64 - clip.offset as f64) / zoom) as f32;
        let x1 = x0 + ((n.length as f64 / zoom) as f32).max(1.5);
        let y = rect.bottom() - (f32::from(n.key - low) + 1.0) * row;
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, y), pos2(x1, y + row.max(1.5))),
            1.0,
            color,
        );
    }
}

/// Height of the take lanes shown under a track.
fn takes_height(track: &Track) -> f32 {
    if track.show_takes {
        track.alternates.len() as f32 * TAKE_HEIGHT
    } else {
        0.0
    }
}

/// Indices of the automation lanes shown under a track.
fn shown_lanes(track: &Track) -> Vec<usize> {
    if !track.show_automation {
        return Vec::new();
    }
    track
        .automation
        .iter()
        .enumerate()
        .filter(|(_, l)| l.visible)
        .map(|(i, _)| i)
        .collect()
}

/// Visual gain of a clip at `pos` samples into it.
fn envelope(clip: &audiostrada_core::Clip, pos: u64) -> f32 {
    let mut gain = 10f32.powf(clip.gain_db / 20.0);
    if pos < clip.fade_in {
        gain *= clip.fade_shape.gain(pos as f32 / clip.fade_in as f32);
    }
    let to_end = clip.length.saturating_sub(pos);
    if to_end < clip.fade_out {
        gain *= clip.out_shape().gain(to_end as f32 / clip.fade_out as f32);
    }
    gain
}

/// Tracks holding the given clips, each once.
fn edit_tracks(project: &Project, clips: &[ClipId]) -> Vec<TrackId> {
    let mut tracks = Vec::new();
    for &c in clips {
        if let Some((t, _)) = edit::locate(project, c)
            && !tracks.contains(&project.tracks[t].id)
        {
            tracks.push(project.tracks[t].id);
        }
    }
    tracks
}

/// Shuffle mode: clips after `trimmed` on its track follow its new end.
fn ripple_after(preview: &mut Project, original: &Project, trimmed: ClipId) {
    let (Some(before), Some(after)) = (
        edit::clip(original, trimmed).cloned(),
        edit::clip(preview, trimmed).cloned(),
    ) else {
        return;
    };
    let delta = after.end() as i64 - before.end() as i64;
    let Some((t, _)) = edit::locate(preview, trimmed) else {
        return;
    };
    let track = preview.tracks[t].id;
    let later: Vec<ClipId> = original
        .track(track)
        .map(|tr| {
            tr.clips
                .iter()
                .filter(|c| c.start >= before.end() && c.id != trimmed)
                .map(|c| c.id)
                .collect()
        })
        .unwrap_or_default();
    if let Some(tr) = preview.track_mut(track) {
        for c in tr.clips.iter_mut().filter(|c| later.contains(&c.id)) {
            c.start = (c.start as i64 + delta).max(0) as u64;
        }
    }
}

/// Where a range starts and ends, in bars, beats and ticks, and how long it lasts.
fn range_label(project: &Project, start: u64, end: u64) -> String {
    let (tempo, rate) = (&project.tempo, project.sample_rate.max(1));
    let at = |sample: u64| {
        let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(sample, rate));
        format!("{}.{}.{:03}", bbt.bar, bbt.beat, bbt.tick)
    };
    let beats = tempo.sample_to_beat(end, rate) - tempo.sample_to_beat(start, rate);
    trf!(
        "{start} → {end}   {seconds} s, {beats} beats",
        start = at(start),
        end = at(end),
        seconds = format!("{:.2}", (end - start) as f64 / f64::from(rate)),
        beats = format!("{beats:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_core::Source;
    use std::path::PathBuf;

    fn project() -> (Project, TrackId, Vec<ClipId>) {
        let mut p = Project::new("t", 48_000);
        let s = p.add_source(Source {
            path: PathBuf::from("a.wav"),
            channels: 1,
            sample_rate: 48_000,
            frames: 1000,
        });
        let t = p.add_track("A");
        let ids = [0, 1000, 3000]
            .iter()
            .filter_map(|&start| p.add_clip(t, s, start))
            .collect();
        (p, t, ids)
    }

    #[test]
    fn shift_click_selects_the_clips_between() {
        let (mut p, _, ids) = project();
        let s = *p.sources.keys().next().unwrap();
        let b = p.add_track("B");
        let under = p.add_clip(b, s, 1500).unwrap();
        let far = p.add_clip(b, s, 9000).unwrap();
        let mut view = TimelineView::new(Rc::new(RefCell::new(Document::new(
            crate::document::AppDirs::default(),
        ))));
        view.rows = routing::visible_tracks(&p);
        view.clip_anchor = Some(ids[0]);
        let mut between = view.clips_between(&p, under);
        between.sort();
        let mut expected = vec![ids[0], ids[1], under];
        expected.sort();
        assert_eq!(between, expected);
        assert!(!view.clips_between(&p, ids[2]).contains(&far));
    }

    #[test]
    fn ranges_come_from_clips_the_loop_or_every_track() {
        let doc = Rc::new(RefCell::new(Document::new(
            crate::document::AppDirs::default(),
        )));
        let (mut p, t, ids) = project();
        let b = p.add_track("B");
        p.loop_range = Some((500, 900));
        doc.borrow_mut().history_mut().edit("Setup", |q| *q = p);
        let mut view = TimelineView::new(Rc::clone(&doc));
        view.rows = routing::visible_tracks(doc.borrow().project());
        let settings = EditSettings::default();
        view.selection = vec![ids[1], ids[2]];
        view.run(Action::RangeFromClips, &settings);
        let r = view.range.clone().unwrap();
        assert_eq!((r.start, r.end, r.tracks), (1000, 4000, vec![t]));
        assert!(view.selection.is_empty());
        view.run(Action::RangeAllTracks, &settings);
        assert_eq!(view.range.as_ref().unwrap().tracks, [t, b]);
        view.run(Action::RangeFromLoop, &settings);
        let r = view.range.clone().unwrap();
        assert_eq!((r.start, r.end, r.tracks.len()), (500, 900, 2));
    }

    #[test]
    fn a_range_tells_where_it_goes_and_how_long() {
        let (p, _, _) = project();
        assert_eq!(
            range_label(&p, 0, 48_000),
            "1.1.000 → 1.3.000   1.00 s, 2 beats"
        );
    }

    #[test]
    fn passages_go_from_the_range_or_else_the_loop() {
        let doc = Rc::new(RefCell::new(Document::new(
            crate::document::AppDirs::default(),
        )));
        let (p, t, _) = project();
        doc.borrow_mut().history_mut().edit("Setup", |q| *q = p);
        let mut view = TimelineView::new(Rc::clone(&doc));
        let settings = EditSettings::default();
        view.run(Action::RemoveSection, &settings);
        assert!(
            !doc.borrow_mut().take_messages().is_empty(),
            "nothing selected"
        );
        assert_eq!(doc.borrow().project().length(), 4000);
        view.range = Some(Range {
            start: 500,
            end: 1500,
            tracks: vec![t],
        });
        view.run(Action::RemoveSection, &settings);
        assert_eq!(doc.borrow().project().length(), 3000);
        assert!(view.range.is_none());
        doc.borrow_mut()
            .history_mut()
            .edit("Loop", |q| q.loop_range = Some((2000, 3000)));
        view.run(Action::CropToSelection, &settings);
        assert_eq!(doc.borrow().project().length(), 1000);
        assert_eq!(doc.borrow().project().loop_range, Some((0, 1000)));
    }

    #[test]
    fn clips_pasted_into_another_project_bring_their_file() {
        let doc = Rc::new(RefCell::new(Document::new(
            crate::document::AppDirs::default(),
        )));
        let (mut a, _, ids) = project();
        a.sources
            .values_mut()
            .for_each(|s| s.path = PathBuf::from("/music/a.wav"));
        doc.borrow_mut().history_mut().edit("Setup", |p| *p = a);
        let mut view = TimelineView::new(Rc::clone(&doc));
        let settings = EditSettings::default();
        view.selection = vec![ids[1]];
        view.run(Action::Copy, &settings);
        view.run(Action::Paste, &settings);
        assert_eq!(
            doc.borrow().project().sources.len(),
            1,
            "same file, same source"
        );

        let mut other = Document::new(crate::document::AppDirs::default());
        other.history_mut().edit("Setup", |p| {
            *p = Project::new("b", 44_100);
            let s = p.add_source(Source {
                path: PathBuf::from("/music/b.wav"),
                channels: 1,
                sample_rate: 44_100,
                frames: 10,
            });
            let t = p.add_track("B");
            p.add_clip(t, s, 0);
        });
        *doc.borrow_mut() = other;
        view.run(Action::Paste, &settings);
        let doc = doc.borrow();
        let b = doc.project();
        assert_eq!(b.sources.len(), 2);
        let pasted = b.tracks[0]
            .clips
            .iter()
            .find(|c| view.selection.contains(&c.id));
        let pasted = pasted.unwrap();
        assert_eq!(
            b.sources[&pasted.source].path,
            PathBuf::from("/music/a.wav")
        );
        assert_eq!(pasted.length, 919, "1000 samples at 48 kHz, at 44.1 kHz");
    }

    mod input {
        use super::*;
        use eframe::egui::{Event, Key, Modifiers, PointerButton, RawInput};

        struct Harness {
            ctx: egui::Context,
            view: TimelineView,
            time: f64,
        }

        impl Harness {
            fn frame(&mut self, events: Vec<Event>) {
                self.time += 0.05;
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 600.0))),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                };
                let settings = EditSettings::default();
                let view = &mut self.view;
                self.ctx
                    .run_ui(input, |ui| view.show(ui, &settings))
                    .drop_without_applying_deltas();
            }

            fn button(&mut self, pos: Pos2, pressed: bool, modifiers: Modifiers) {
                self.frame(vec![
                    Event::ModifiersChanged(modifiers),
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers,
                    },
                ]);
            }

            fn click(&mut self, pos: Pos2, modifiers: Modifiers) {
                self.frame(vec![Event::PointerMoved(pos)]);
                self.button(pos, true, modifiers);
                self.button(pos, false, modifiers);
            }

            fn drag(&mut self, from: Pos2, to: Pos2) {
                self.frame(vec![Event::PointerMoved(from)]);
                self.button(from, true, Modifiers::NONE);
                for t in [0.2, 0.6, 1.0] {
                    self.frame(vec![Event::PointerMoved(from.lerp(to, t))]);
                }
                self.button(to, false, Modifiers::NONE);
                self.frame(vec![]);
            }
        }

        fn harness() -> (Harness, Rc<RefCell<Document>>, Vec<TrackId>) {
            let (mut p, a, _) = project();
            let s = *p.sources.keys().next().unwrap();
            let b = p.add_track("B");
            p.add_clip(b, s, 0);
            let mut doc = Document::new(crate::document::AppDirs::default());
            doc.history_mut().replace(p, false);
            let doc = Rc::new(RefCell::new(doc));
            let mut h = Harness {
                ctx: egui::Context::default(),
                view: TimelineView::new(Rc::clone(&doc)),
                time: 0.0,
            };
            h.frame(vec![]);
            h.frame(vec![]);
            (h, doc, vec![a, b])
        }

        #[test]
        fn double_click_on_a_name_renames_the_track() {
            let (mut h, doc, tracks) = harness();
            let name = pos2(
                h.view.lanes_rect.left() - HEADER_WIDTH + 20.0,
                h.view.lanes_rect.top() + 12.0,
            );
            h.click(name, Modifiers::NONE);
            h.click(name, Modifiers::NONE);
            h.frame(vec![]);
            assert!(h.view.renaming.is_some());
            h.frame(vec![Event::Text("Lead".into())]);
            h.frame(vec![Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
            h.frame(vec![]);
            assert_eq!(
                doc.borrow().project().track(tracks[0]).unwrap().name,
                "Lead"
            );
            assert!(h.view.renaming.is_none());
        }

        #[test]
        fn range_edges_drag_and_shift_click_extends_the_range() {
            let (mut h, doc, tracks) = harness();
            let before = doc.borrow().project().clone();
            let lanes = h.view.lanes_rect;
            let x = |s: u64, h: &Harness| {
                lanes.left() + ((s as f64 - h.view.scroll) / h.view.zoom) as f32
            };
            let sample = |x: f32, h: &Harness| {
                (h.view.scroll + f64::from(x - lanes.left()) * h.view.zoom) as u64
            };
            let row = |i: usize, h: &Harness| lanes.top() + h.view.tops[i] + 30.0;
            h.view.range = Some(Range {
                start: 1000,
                end: 2000,
                tracks: vec![tracks[0]],
            });
            h.frame(vec![]);
            let from = x(2000, &h);
            h.drag(pos2(from, row(0, &h)), pos2(from + 40.0, row(0, &h)));
            let r = h.view.range.clone().unwrap();
            assert_eq!(r.start, 1000);
            assert_eq!(r.end, sample(from + 40.0, &h));
            let after = doc.borrow().project().clone();
            assert_eq!(after.tracks, before.tracks, "the clips stay");
            assert_eq!(after.loop_range, Some((r.start, r.end)), "the loop follows");
            // Dragged past the other edge, the range turns over.
            let from = x(r.end, &h);
            let to = x(500, &h);
            h.drag(pos2(from, row(0, &h)), pos2(to, row(0, &h)));
            let r = h.view.range.clone().unwrap();
            assert_eq!((r.start, r.end), (sample(to, &h), 1000));
            // Shift+click further on the second track: later end, both tracks.
            let at = x(r.end + 2000, &h);
            h.click(pos2(at, row(1, &h)), Modifiers::SHIFT);
            let r = h.view.range.clone().unwrap();
            assert_eq!(r.end, sample(at, &h));
            assert_eq!(r.tracks, tracks);
            // A plain click elsewhere lets it go.
            h.click(pos2(x(3500, &h), row(1, &h)), Modifiers::NONE);
            assert!(h.view.range.is_none());
        }

        #[test]
        fn the_ruler_selects_and_the_loop_follows_unless_fixed() {
            let (mut h, doc, tracks) = harness();
            let lanes = h.view.lanes_rect;
            let sample = |x: f32, h: &Harness| {
                (h.view.scroll + f64::from(x - lanes.left()) * h.view.zoom) as u64
            };
            let mid = lanes.top() - RULER_HEIGHT / 2.0;
            let band = lanes.top() - 3.0;
            let (x0, x1) = (lanes.left() + 40.0, lanes.left() + 120.0);
            // A drag in the ruler: a selection on every track, and the loop on it.
            h.drag(pos2(x0, mid), pos2(x1, mid));
            let r = h.view.range.clone().unwrap();
            assert_eq!((r.start, r.end), (sample(x0, &h), sample(x1, &h)));
            assert_eq!(r.tracks, tracks);
            assert_eq!(doc.borrow().project().loop_range, Some((r.start, r.end)));
            assert!(doc.borrow().project().looping().is_some());
            // Double-click in the ruler: looping off, the range kept.
            h.click(pos2(lanes.left() + 300.0, mid), Modifiers::NONE);
            h.click(pos2(lanes.left() + 300.0, mid), Modifiers::NONE);
            assert!(doc.borrow().project().looping().is_none());
            assert_eq!(doc.borrow().project().loop_range, Some((r.start, r.end)));
            // The loop band sets the loop, and the linked selection moves with it.
            let x2 = lanes.left() + 200.0;
            h.drag(pos2(x1, band), pos2(x2, band));
            assert_eq!(
                doc.borrow().project().loop_range,
                Some((r.start, sample(x2, &h)))
            );
            assert!(
                doc.borrow().project().looping().is_some(),
                "setting it turns it on"
            );
            assert_eq!(h.view.range.as_ref().unwrap().end, sample(x2, &h));
            // Fixed, the loop stays while another passage is selected.
            doc.borrow_mut()
                .history_mut()
                .edit("Fix", |p| p.loop_fixed = true);
            let fixed = doc.borrow().project().loop_range;
            h.drag(
                pos2(lanes.left() + 300.0, mid),
                pos2(lanes.left() + 400.0, mid),
            );
            assert_eq!(
                h.view.range.as_ref().unwrap().start,
                sample(lanes.left() + 300.0, &h)
            );
            assert_eq!(doc.borrow().project().loop_range, fixed);
        }

        #[test]
        fn a_fade_drawn_on_one_selected_clip_applies_to_all() {
            let (mut h, doc, tracks) = harness();
            let lanes = h.view.lanes_rect;
            let x = lanes.left() + 3.0;
            let row = |i: usize, h: &Harness| lanes.top() + h.view.tops[i] + 5.0;
            let (a, b) = (row(0, &h), row(1, &h));
            h.click(pos2(lanes.left() + 20.0, a + 20.0), Modifiers::NONE);
            h.click(pos2(lanes.left() + 20.0, b + 20.0), Modifiers::COMMAND);
            assert_eq!(h.view.selection.len(), 2);
            h.drag(pos2(x, a), pos2(x + 30.0, a));
            let fades: Vec<u64> = tracks
                .iter()
                .map(|&t| doc.borrow().project().track(t).unwrap().clips[0].fade_in)
                .collect();
            assert!(fades[0] > 0 && fades[0] == fades[1], "{fades:?}");
        }
    }

    #[test]
    fn a_new_midi_clip_is_created_and_selected() {
        let (p, t, _) = project();
        let mut doc = Document::new(crate::document::AppDirs::default());
        doc.history_mut().replace(p, false);
        let doc = Rc::new(RefCell::new(doc));
        let mut view = TimelineView::new(Rc::clone(&doc));
        view.new_midi_clip(Some((t, 10_000)));
        let clip = view.selection[0];
        assert!(edit::clip(doc.borrow().project(), clip).is_some_and(|c| c.is_midi()));
        assert_eq!(doc.borrow().selected_tracks(), [t]);
    }

    #[test]
    fn track_height_changes_around_the_pointer() {
        let mut view = TimelineView::new(Rc::new(RefCell::new(Document::new(
            crate::document::AppDirs::default(),
        ))));
        view.track_height = 100.0;
        view.tops = vec![0.0, 100.0, 200.0];
        view.scroll_y = 50.0;
        // The pointer is halfway into the third track (content 250).
        view.set_track_height(200.0, Some(200.0));
        assert_eq!(view.scroll_y, 500.0 - 200.0);
        assert_eq!(view.track_height, 200.0);
    }

    #[test]
    fn copies_ranges_and_clips_then_pastes_at_the_playhead() {
        let (p, t, ids) = project();
        let mut doc = Document::new(crate::document::AppDirs::default());
        doc.history_mut().replace(p, false);
        let other = {
            let mut id = None;
            doc.history_mut().edit("B", |p| id = Some(p.add_track("B")));
            id.unwrap()
        };
        let doc = Rc::new(RefCell::new(doc));
        let mut view = TimelineView::new(Rc::clone(&doc));
        let settings = EditSettings::default();

        view.selection = vec![ids[1]];
        view.run(Action::Copy, &settings);
        doc.borrow_mut().player_mut().locate(10_000);
        doc.borrow_mut().select_track(other, SelectMode::Only);
        view.run(Action::Paste, &settings);
        let b = doc.borrow().project().track(other).unwrap().clips.clone();
        assert_eq!((b.len(), b[0].start, b[0].length), (1, 10_000, 1000));

        // A range across the first track: a piece of the first clip and all of the second.
        view.range = Some(Range {
            start: 500,
            end: 2000,
            tracks: vec![t],
        });
        view.run(Action::Cut, &settings);
        let a: Vec<_> = doc
            .borrow()
            .project()
            .track(t)
            .unwrap()
            .clips
            .iter()
            .map(|c| (c.start, c.length))
            .collect();
        assert_eq!(a, vec![(0, 500), (3000, 1000)], "cut leaves the rest");
        doc.borrow_mut().player_mut().locate(20_000);
        doc.borrow_mut().select_track(t, SelectMode::Only);
        view.run(Action::Paste, &settings);
        let pasted: Vec<_> = doc
            .borrow()
            .project()
            .track(t)
            .unwrap()
            .clips
            .iter()
            .filter(|c| c.start >= 20_000)
            .map(|c| (c.start, c.length, c.offset))
            .collect();
        assert_eq!(pasted, vec![(20_000, 500, 500), (20_500, 1000, 0)]);
    }

    #[test]
    fn rows_lanes_and_takes_are_found_under_the_pointer() {
        let (mut p, a, _) = project();
        let b = p.add_track("B");
        {
            let t = p.track_mut(a).unwrap();
            t.show_automation = true;
            t.automation = vec![
                audiostrada_core::automation::AutomationLane::new(AutomationTarget::Pan),
                audiostrada_core::automation::AutomationLane::new(AutomationTarget::Gain),
            ];
            t.automation[0].visible = false;
        }
        edit::new_playlist(&mut p, b);
        p.track_mut(b).unwrap().show_takes = true;
        let mut view = TimelineView::new(Rc::new(RefCell::new(Document::new(
            crate::document::AppDirs::default(),
        ))));
        view.rows = routing::visible_tracks(&p);
        view.track_height = 72.0;
        view.tops = vec![0.0, 72.0 + LANE_HEIGHT];
        view.content_height = view.tops[1] + 72.0 + TAKE_HEIGHT;
        assert_eq!(shown_lanes(p.track(a).unwrap()), [1]);
        assert_eq!(takes_height(p.track(b).unwrap()), TAKE_HEIGHT);

        assert_eq!(view.row_at(-1.0), None);
        assert_eq!(view.row_at(10.0), Some(0));
        assert_eq!(view.row_at(130.0), Some(1));
        assert_eq!(view.row_at(view.content_height), None);
        assert_eq!(view.track_at(&p, 10.0).map(|t| t.id), Some(a));
        assert!(view.track_at(&p, 80.0).is_none(), "on the lane");
        assert_eq!(view.lane_at(&p, 80.0), Some((0, 1, 72.0)));
        assert_eq!(view.lane_at(&p, 10.0), None);
        assert_eq!(view.take_at(&p, 200.0), Some((b, 0)));
        assert_eq!(view.take_at(&p, 130.0), None);
        view.scroll_y = 50.0;
        assert_eq!(view.lane_at(&p, 30.0), Some((0, 1, 22.0)));
    }

    #[test]
    fn view_actions_zoom_and_select() {
        let (mut p, t, ids) = project();
        edit::add_marker(&mut p, 500);
        edit::add_marker(&mut p, 2000);
        let mut doc = Document::new(crate::document::AppDirs::default());
        doc.history_mut().replace(p, false);
        doc.select_track(t, SelectMode::Only);
        let doc = Rc::new(RefCell::new(doc));
        let mut view = TimelineView::new(Rc::clone(&doc));
        let settings = EditSettings::default();
        let run = |view: &mut TimelineView, action| assert!(view.run(action, &settings));

        run(&mut view, Action::ZoomIn);
        assert!((view.zoom - 400.0 / 1.5).abs() < 1e-9);
        assert_eq!(view.scroll, 0.0, "around the playhead at 0");
        run(&mut view, Action::ZoomOut);
        assert!((view.zoom - 400.0).abs() < 1e-9);
        view.lanes_width = 1000.0;
        run(&mut view, Action::ZoomToFit);
        assert!((view.zoom - 4000.0 * 1.05 / 1000.0).abs() < 1e-9);
        view.zoom_around(1e9, 0.0, 0.0);
        assert_eq!(view.zoom, MIN_ZOOM);
        run(&mut view, Action::WaveformZoomOut);
        assert_eq!(view.wave_zoom, 1.0);
        for _ in 0..20 {
            run(&mut view, Action::WaveformZoomIn);
        }
        assert_eq!(view.wave_zoom, MAX_WAVE_ZOOM);
        run(&mut view, Action::WaveformZoomReset);
        assert_eq!(view.wave_zoom, 1.0);
        run(&mut view, Action::TallerTracks);
        assert_eq!(view.track_height, 90.0);
        run(&mut view, Action::ShorterTracks);
        assert_eq!(view.track_height, 72.0);

        run(&mut view, Action::NextMarker);
        assert_eq!(doc.borrow().player().position(), 500);
        run(&mut view, Action::NextMarker);
        run(&mut view, Action::PreviousMarker);
        assert_eq!(doc.borrow().player().position(), 500);
        run(&mut view, Action::AddMarker);
        assert_eq!(doc.borrow().project().markers.len(), 3);

        run(&mut view, Action::SelectAll);
        assert_eq!(view.selection, ids);
        run(&mut view, Action::LoopSelection);
        assert_eq!(doc.borrow().project().loop_range, Some((0, 4000)));
        run(&mut view, Action::Deselect);
        assert!(view.selection.is_empty());

        view.selection = vec![ids[2]];
        run(&mut view, Action::NudgeRight);
        assert_eq!(
            edit::clip(doc.borrow().project(), ids[2]).unwrap().start,
            27_000
        );
        run(&mut view, Action::NudgeLeft);
        assert_eq!(
            edit::clip(doc.borrow().project(), ids[2]).unwrap().start,
            3000
        );
        run(&mut view, Action::Duplicate);
        assert_ne!(view.selection, [ids[2]], "the copy is selected");
        run(&mut view, Action::Delete);
        assert!(view.selection.is_empty());
        assert_eq!(doc.borrow().project().track(t).unwrap().clips.len(), 3);

        view.range = Some(Range {
            start: 100,
            end: 200,
            tracks: vec![t],
        });
        run(&mut view, Action::LoopSelection);
        assert_eq!(doc.borrow().project().loop_range, Some((100, 200)));
        run(&mut view, Action::Split);
        assert_eq!(doc.borrow().project().track(t).unwrap().clips.len(), 5);
        run(&mut view, Action::Deselect);
        assert!(view.range.is_none());
        run(&mut view, Action::NewPlaylist);
        assert_eq!(doc.borrow().project().track(t).unwrap().alternates.len(), 1);
        assert!(!view.run(Action::Quit, &settings));
    }

    #[test]
    fn scale_maps_both_ways() {
        let scale = Scale {
            left: 100.0,
            scroll: 1000.0,
            zoom: 10.0,
        };
        assert_eq!(scale.x(1000), 100.0);
        assert_eq!(scale.sample(150.0), 1500);
        assert_eq!(scale.sample(0.0), 0);
        assert_eq!(scale.samples(-3.0), -30);
    }

    #[test]
    fn shuffle_trim_moves_later_clips() {
        let (original, _, ids) = project();
        let mut preview = original.clone();
        edit::trim_end(&mut preview, ids[0], 600);
        ripple_after(&mut preview, &original, ids[0]);
        assert_eq!(edit::clip(&preview, ids[1]).unwrap().start, 600);
        assert_eq!(edit::clip(&preview, ids[2]).unwrap().start, 2600);
    }

    #[test]
    fn envelope_follows_fades_and_gain() {
        let (mut p, _, ids) = project();
        edit::set_fades(&mut p, ids[0], 100, 100);
        edit::set_clip_gain(&mut p, ids[0], -6.0);
        let clip = edit::clip(&p, ids[0]).unwrap();
        assert_eq!(envelope(clip, 0), 0.0);
        assert!((envelope(clip, 500) - 0.501).abs() < 0.01);
        assert!(envelope(clip, 999) < 0.1);
    }
}
