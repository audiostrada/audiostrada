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

mod draw;
mod fades;
mod gestures;
mod headers;
mod magnet;
mod menus;
mod processing;
mod toolbar;
mod view;
use draw::*;
pub use draw::{WaveScale, WaveView};
use gestures::*;
pub use menus::TRACK_COLORS;
pub use toolbar::Tool;

const HEADER_WIDTH: f32 = 210.0;
const MIN_HEADER_WIDTH: f32 = 150.0;
const MAX_HEADER_WIDTH: f32 = 420.0;
/// Width the track meter takes at the right of a header.
const HEADER_METER: f32 = 12.0;
const RULER_HEIGHT: f32 = 28.0;
const EDGE_ZONE: f32 = 6.0;
const FADE_ZONE_HEIGHT: f32 = 12.0;
/// Height of the name bar at the top of a clip.
const NAME_BAR: f32 = 14.0;
/// Clips lower than this move from anywhere with the Smart Tool, no range in their upper half.
const SMART_HEIGHT: f32 = 56.0;
const MIN_TRACK_HEIGHT: f32 = 40.0;
const MAX_TRACK_HEIGHT: f32 = 600.0;
/// Most a waveform is magnified vertically.
const MAX_WAVE_ZOOM: f32 = 64.0;
const MIN_ZOOM: f64 = 0.05;
/// The overview of the whole project under the lanes.
const OVERVIEW_HEIGHT: f32 = 26.0;
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
    /// The eraser (no `mute`) or the mute tool, over every clip the pointer crosses.
    Sweep {
        mute: Option<bool>,
    },
    /// The middle of a fade, up or down for its shape; the clip's top and height on screen.
    FadeCurve {
        fade_in: bool,
        top: f32,
        height: f32,
    },
    /// The handle of the crossfade between two clips, their overlap when grabbed.
    Crossfade {
        left: ClipId,
        right: ClipId,
        overlap: u64,
    },
    /// Ctrl+Alt: the audio slides inside the clips.
    Slip,
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
    /// Height of each shown track's clips, by row.
    heights: Vec<f32>,
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
    /// Width of the track headers, set by dragging their edge.
    pub header_width: f32,
    /// The headers were resized: the application keeps their width.
    pub header_resized: bool,
    /// What the mouse does on the clips.
    pub tool: Tool,
    /// How waveforms are drawn.
    pub wave_view: WaveView,
    /// Zoom and scroll before a zoom to the selection, to go back to.
    zoom_back: Option<(f64, f64)>,
    /// Clips swept by the eraser or the mute tool during a drag.
    swept: Vec<ClipId>,
    /// Attacks found in each media file, in its frames.
    onsets: HashMap<std::path::PathBuf, Rc<Vec<usize>>>,
    /// What a drag in the overview holds: the view's left edge, its right edge or all of it.
    overview_grab: Option<Grab>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Left,
    Right,
    Move,
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
            heights: Vec::new(),
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
            header_width: HEADER_WIDTH,
            header_resized: false,
            tool: Tool::Smart,
            wave_view: WaveView::default(),
            zoom_back: None,
            swept: Vec::new(),
            onsets: HashMap::new(),
            overview_grab: None,
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

    /// Height of the clips of the track on row `r`.
    fn row_height(&self, r: usize) -> f32 {
        self.heights.get(r).copied().unwrap_or(self.track_height)
    }

    /// Track whose clip area is under `y` (relative to the top of the lanes).
    fn track_at<'a>(&self, project: &'a Project, y: f32) -> Option<&'a Track> {
        let r = self.row_at(y)?;
        (y + self.scroll_y - self.tops[r] < self.row_height(r))
            .then(|| project.tracks[self.rows[r].0].as_ref())
    }

    /// Automation lane under `y`: row, index of the lane in its track, and the lane's top.
    fn lane_at(&self, project: &Project, y: f32) -> Option<(usize, usize, f32)> {
        let r = self.row_at(y)?;
        let offset = y + self.scroll_y - self.tops[r] - self.row_height(r);
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
            self.tops[r] + self.row_height(r) + takes_height(track) + k as f32 * LANE_HEIGHT
                - self.scroll_y,
        ))
    }

    /// Take lane under `y`: track and index of the playlist in its alternates.
    fn take_at(&self, project: &Project, y: f32) -> Option<(TrackId, usize)> {
        let r = self.row_at(y)?;
        let track = &project.tracks[self.rows[r].0];
        let offset = y + self.scroll_y - self.tops[r] - self.row_height(r);
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
        if action == Action::Deselect {
            self.tool = Tool::Smart;
        }
        match action {
            Action::ToolSmart => self.tool = Tool::Smart,
            Action::ToolRange => self.tool = Tool::Range,
            Action::ToolSplit => self.tool = Tool::Split,
            Action::ToolErase => self.tool = Tool::Erase,
            Action::ToolMute => self.tool = Tool::Mute,
            Action::ZoomToSelection => self.zoom_to_selection(&doc),
            Action::Fades => self.fades(&mut doc),
            Action::NextTransient | Action::PreviousTransient => {
                let tracks: Vec<TrackId> = if selection.is_empty() {
                    track.into_iter().collect()
                } else {
                    edit_tracks(doc.project(), &selection)
                };
                let all: Vec<u64> = tracks
                    .iter()
                    .flat_map(|t| self.transients(&doc, *t))
                    .collect();
                let target = if action == Action::NextTransient {
                    all.iter().filter(|&&t| t > position + 1).min()
                } else {
                    all.iter().filter(|&&t| t + 1 < position).max()
                };
                if let Some(&target) = target {
                    doc.player_mut().locate(target);
                }
            }
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

    pub fn show(&mut self, ui: &mut Ui, settings: &mut EditSettings) {
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
        self.heights.clear();
        let mut y = 0.0;
        for &(index, _) in &self.rows {
            self.tops.push(y);
            let track = &project.tracks[index];
            let height =
                (self.track_height * track.height).clamp(MIN_TRACK_HEIGHT, MAX_TRACK_HEIGHT);
            self.heights.push(height);
            y += height + takes_height(track) + shown_lanes(track).len() as f32 * LANE_HEIGHT;
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

        doc.update_levels();
        self.toolbar(ui, settings, doc.player().position());
        let settings = &*settings;
        let all = ui.available_rect_before_wrap();
        let (_, background) = ui.allocate_exact_size(all.size(), Sense::hover());
        // The horizontal scrollbar takes the bottom.
        let full = Rect::from_min_max(all.min, pos2(all.right(), all.bottom() - OVERVIEW_HEIGHT));
        let header_width = self.header_width.clamp(
            MIN_HEADER_WIDTH,
            MAX_HEADER_WIDTH
                .min(full.width() * 0.6)
                .max(MIN_HEADER_WIDTH),
        );
        let header = Rect::from_min_max(
            pos2(full.left(), full.top() + RULER_HEIGHT),
            pos2(full.left() + header_width, full.bottom()),
        );
        let ruler = Rect::from_min_max(
            pos2(full.left() + header_width, full.top()),
            pos2(full.right(), full.top() + RULER_HEIGHT),
        );
        let lanes = Rect::from_min_max(
            pos2(full.left() + header_width, full.top() + RULER_HEIGHT),
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
        self.overview(
            ui,
            Rect::from_min_max(pos2(lanes.left(), full.bottom()), all.max),
            &project,
            position,
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
                pos2(lanes.right(), top + self.heights[i]),
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
            // A folder shows its tracks' clips in small, one thin band each.
            let members = routing::subtree(&project, track.id);
            if track.clips.is_empty() && members.len() > 1 {
                draw_folder(&lane_painter, row, &project, &members[1..], scale);
            }
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
                if selected {
                    draw_handles(&lane_painter, rect, clip, self.zoom, false);
                }
                if pointer.is_some_and(|p| rect.contains(p) && lanes.contains(p)) {
                    hovered_clip = Some((clip.id, track.id, rect));
                }
            }
            draw_crossfades(&lane_painter, row, track, scale);
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

        if let Some((id, track, rect)) = hovered_clip
            && let Some(clip) = project
                .track(track)
                .and_then(|t| t.clips.iter().find(|c| c.id == id))
        {
            draw_handles(&lane_painter, rect, clip, self.zoom, true);
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
                let r = Rect::from_x_y_ranges(x0..=x1, top..=top + self.heights[i]);
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
        // Rows scrolled past the header's edges must not catch the tool bar's or ruler's clicks.
        let clip = ui.clip_rect();
        ui.set_clip_rect(header.intersect(clip));
        self.draw_headers(ui, header, &mut doc, &project, &visuals);
        ui.set_clip_rect(clip);
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
        self.header_edge(ui, full, header);

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

    /// Lanes that can be added to a track (shown again if hidden).
    /// Adds to the selection the clips and tracks edited along with it (groups), the tracks
    /// chosen staying last (focused).
    fn follow_groups(&mut self, doc: &mut Document) {
        let project = doc.project();
        self.selection = audiostrada_core::groups::edit_clips(project, &self.selection);
        let chosen = doc.selected_tracks().to_vec();
        let mut tracks: Vec<TrackId> = audiostrada_core::groups::edit_tracks(project, &chosen)
            .into_iter()
            .filter(|t| !chosen.contains(t))
            .collect();
        if !tracks.is_empty() {
            tracks.extend(chosen);
            doc.set_track_selection(tracks);
        }
    }

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
}

const LANE_PAD: f32 = 5.0;

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
                let mut settings = EditSettings::default();
                let view = &mut self.view;
                self.ctx
                    .run_ui(input, |ui| view.show(ui, &mut settings))
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
            // Past the track's icon.
            let name = pos2(
                h.view.lanes_rect.left() - HEADER_WIDTH + 44.0,
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
        fn the_handles_set_the_volume_and_the_fade_shape() {
            let (mut h, doc, tracks) = harness();
            let lanes = h.view.lanes_rect;
            let x = |s: u64, h: &Harness| {
                lanes.left() + ((s as f64 - h.view.scroll) / h.view.zoom) as f32
            };
            let top = lanes.top() + h.view.tops[1] + 2.0;
            let clip = |doc: &Rc<RefCell<Document>>| {
                doc.borrow().project().track(tracks[1]).unwrap().clips[0].clone()
            };
            // The volume handle, in the middle of the top edge, dragged up.
            let middle = pos2(x(500, &h), top + 4.0);
            h.drag(middle, middle - vec2(0.0, 30.0));
            assert!(
                (clip(&doc).gain_db - 3.0).abs() < 0.2,
                "{}",
                clip(&doc).gain_db
            );
            // A double-click on it goes back to 0 dB.
            h.click(middle, Modifiers::NONE);
            h.click(middle, Modifiers::NONE);
            assert_eq!(clip(&doc).gain_db, 0.0);
            // The middle of a fade, pulled down, slows it.
            let id = clip(&doc).id;
            doc.borrow_mut()
                .history_mut()
                .edit("Fade", |p| edit::set_fades(p, id, 400, 0));
            h.frame(vec![]);
            let height = h.view.track_height - 4.0;
            let shape = clip(&doc).fade_shape;
            let at = pos2(x(200, &h), top + height - shape.gain(0.5) * height);
            h.drag(at, pos2(at.x, top + height - 4.0));
            assert_eq!(
                clip(&doc).fade_shape,
                audiostrada_core::model::FadeShape::Slow
            );
        }

        #[test]
        fn a_track_gets_its_own_height_from_its_header_edge() {
            let (mut h, doc, tracks) = harness();
            let lanes = h.view.lanes_rect;
            let edge = pos2(
                lanes.left() - HEADER_WIDTH + 60.0,
                lanes.top() + h.view.tops[0] + h.view.track_height - 1.0,
            );
            h.drag(edge, edge + vec2(0.0, 36.0));
            h.frame(vec![]);
            assert!(
                (h.view.heights[0] - 108.0).abs() < 1.0,
                "{:?}",
                h.view.heights
            );
            assert_eq!(h.view.heights[1], 72.0, "the other track stays");
            let track = |doc: &Rc<RefCell<Document>>| {
                doc.borrow().project().track(tracks[0]).unwrap().height
            };
            assert!((track(&doc) - 1.5).abs() < 0.02);
            // The global height scales it too.
            h.view.set_track_height(144.0, None);
            h.frame(vec![]);
            assert!((h.view.heights[0] - 216.0).abs() < 2.0);
        }

        #[test]
        fn the_overview_moves_and_fits_the_view() {
            let (mut h, _, _) = harness();
            let fitted = h.view.zoom;
            h.view.zoom = fitted / 4.0;
            h.view.scroll = 0.0;
            h.frame(vec![]);
            let lanes = h.view.lanes_rect;
            let y = 600.0 - OVERVIEW_HEIGHT / 2.0;
            h.click(pos2(lanes.right() - 20.0, y), Modifiers::NONE);
            assert!(h.view.scroll > 0.0, "a click goes there");
            // Long enough after that click for the next two to be a double-click.
            for _ in 0..10 {
                h.frame(vec![]);
            }
            h.click(pos2(lanes.left() + 200.0, y), Modifiers::NONE);
            h.click(pos2(lanes.left() + 200.0, y), Modifiers::NONE);
            assert!(
                (h.view.zoom - fitted).abs() < fitted * 0.01,
                "a double-click shows all"
            );
        }

        #[test]
        fn tools_over_the_headers_stay_clickable_when_scrolled() {
            let (mut h, _, _) = harness();
            h.view.scroll_y = 50.0;
            h.frame(vec![]);
            // The third tool, the scissors, sits above the track headers.
            let scissors = pos2(6.0 + 2.0 * 30.0 + 14.0, toolbar::HEIGHT / 2.0);
            h.click(scissors, Modifiers::NONE);
            assert_eq!(h.view.tool, Tool::Split);
        }

        #[test]
        fn the_smart_tool_draws_a_range_above_and_moves_below() {
            let (mut h, doc, tracks) = harness();
            let lanes = h.view.lanes_rect;
            let x = |s: u64, h: &Harness| {
                lanes.left() + ((s as f64 - h.view.scroll) / h.view.zoom) as f32
            };
            let top = |i: usize, h: &Harness| lanes.top() + h.view.tops[i];
            let start = |doc: &Rc<RefCell<Document>>| {
                doc.borrow().project().track(tracks[1]).unwrap().clips[0].start
            };
            // Upper half: a range, the clip stays.
            let y = top(1, &h) + 25.0;
            h.drag(pos2(x(200, &h), y), pos2(x(800, &h), y));
            assert!(
                h.view
                    .range
                    .as_ref()
                    .is_some_and(|r| r.tracks == [tracks[1]])
            );
            assert_eq!(start(&doc), 0);
            // Lower half: the clip moves.
            let y = top(1, &h) + 55.0;
            h.drag(pos2(x(500, &h), y), pos2(x(500, &h) + 60.0, y));
            assert!(start(&doc) > 0);
            // The eraser deletes the clip clicked.
            h.view.tool = Tool::Erase;
            h.click(pos2(x(3500, &h), top(0, &h) + 50.0), Modifiers::NONE);
            assert_eq!(
                doc.borrow().project().track(tracks[0]).unwrap().clips.len(),
                2
            );
            // The mute tool mutes it.
            h.view.tool = Tool::Mute;
            h.click(pos2(x(500, &h), top(0, &h) + 50.0), Modifiers::NONE);
            assert!(doc.borrow().project().track(tracks[0]).unwrap().clips[0].muted);
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

        #[test]
        fn a_group_selects_and_edits_its_members_clips_unless_ctrl() {
            let (mut h, doc, tracks) = harness();
            doc.borrow_mut().history_mut().edit("Group", |p| {
                p.groups
                    .push(audiostrada_core::groups::TrackGroup::new("G", [0; 3]));
                for &t in &tracks {
                    audiostrada_core::groups::set_group(p, t, Some(0));
                }
            });
            let lanes = h.view.lanes_rect;
            let a = lanes.top() + h.view.tops[0] + 25.0;
            h.click(pos2(lanes.left() + 20.0, a), Modifiers::NONE);
            assert_eq!(h.view.selection.len(), 2, "the member's clip too");
            assert_eq!(doc.borrow().selected_tracks(), [tracks[1], tracks[0]]);
            h.view.selection.clear();
            h.click(pos2(lanes.left() + 20.0, a), Modifiers::COMMAND);
            assert_eq!(h.view.selection.len(), 1, "Ctrl: the clip alone");
            doc.borrow_mut()
                .history_mut()
                .modify_unrecorded(|p| p.groups_off = true);
            h.view.selection.clear();
            h.click(pos2(lanes.left() + 20.0, a), Modifiers::NONE);
            assert_eq!(h.view.selection.len(), 1, "groups off");
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
