//! Piano roll: edits the notes of the selected MIDI clip.
//!
//! The pointer is smart (move, resize from either edge, Alt-drag copies, rubber band, double
//! click adds); the pencil adds notes and drags their length, the brush paints them along the
//! grid, the scissors slice, the eraser removes. Chords can be stamped, a scale shades or folds
//! the keys, the chord track names the harmony, ghost notes show the other clips. Transforms
//! (quantize with swing, humanize, strum, arpeggiate, harmonize, chordify, variations,
//! randomize, velocity shapes) preview live before they are applied. Lanes edit velocity,
//! probability, controllers and per-note expression (MPE). Every edit is one undo step.

mod canvas;
mod lanes;
mod music;
mod ops;
mod panel;
mod view;

use std::cell::RefCell;
use std::rc::Rc;

use audiostrada_core::model::{CHANNEL_PRESSURE, PITCH_BEND};
use audiostrada_core::{ClipId, ControlPoint, Note, Project, edit};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};

use crate::document::Document;
use crate::editor_frame;
use crate::i18n::tr;
use crate::transport::EditSettings;
use crate::trf;

use music::{ChordKind, Scale, Voicing};
use ops::{MIN_LENGTH, Timing};
use view::View;

const KEYS_WIDTH: f32 = 74.0;
/// Width of the note editor's list of tracks, as the automation editor's names.
const PARTS_WIDTH: f32 = 176.0;
const RULER_HEIGHT: f32 = 22.0;
const CHORDS_HEIGHT: f32 = 18.0;
const TOOLBAR_HEIGHT: f32 = 34.0;
const SPLITTER: f32 = 6.0;
/// How long a note added with a click sounds, in seconds.
const AUDITION: f64 = 0.3;

/// What a drag on the notes does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    /// Selects, moves, resizes; double-click adds.
    Pointer,
    /// Adds a note per click, its length dragged.
    Draw,
    /// Paints a note per grid step along the pointer.
    Paint,
    /// Cuts notes where they are clicked.
    Slice,
    /// Removes the notes it passes over.
    Erase,
    /// Fills the view with a span dragged across, zooms in at a click (the note editor's).
    Zoom,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Pointer,
        Tool::Draw,
        Tool::Paint,
        Tool::Slice,
        Tool::Erase,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Pointer => tr("Pointer"),
            Tool::Draw => tr("Pencil"),
            Tool::Paint => tr("Brush"),
            Tool::Slice => tr("Scissors"),
            Tool::Erase => tr("Eraser"),
            Tool::Zoom => tr("Zoom"),
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Tool::Pointer => tr(
                "Click to select, drag to move (Alt: copy), drag an edge to resize, drag the \
                 empty space to select, double-click to add",
            ),
            Tool::Draw => tr("Click to add a note, drag right to set its length"),
            Tool::Paint => tr("Drag to paint a note on each grid step, up and down too"),
            Tool::Slice => tr("Click a note to cut it there (all the selected ones under it)"),
            Tool::Erase => tr("Click or drag over notes to remove them"),
            Tool::Zoom => tr("Drag across a span to fill the view with it, click to zoom in"),
        }
    }

    fn key(self) -> egui::Key {
        match self {
            Tool::Pointer => egui::Key::Num1,
            Tool::Draw => egui::Key::Num2,
            Tool::Paint => egui::Key::Num3,
            Tool::Slice => egui::Key::Num4,
            Tool::Erase => egui::Key::Num5,
            Tool::Zoom => egui::Key::Num6,
        }
    }

    /// The editors' tool it is, for the note editor's bar and keys.
    pub(super) fn shared(self) -> editor_frame::Tool {
        match self {
            Tool::Pointer => editor_frame::Tool::Select,
            Tool::Draw => editor_frame::Tool::Pencil,
            Tool::Paint => editor_frame::Tool::Brush,
            Tool::Slice => editor_frame::Tool::Scissors,
            Tool::Erase => editor_frame::Tool::Erase,
            Tool::Zoom => editor_frame::Tool::Zoom,
        }
    }

    pub(super) fn of(tool: editor_frame::Tool) -> Self {
        match tool {
            editor_frame::Tool::Select => Tool::Pointer,
            editor_frame::Tool::Pencil => Tool::Draw,
            editor_frame::Tool::Brush => Tool::Paint,
            editor_frame::Tool::Scissors => Tool::Slice,
            editor_frame::Tool::Erase => Tool::Erase,
            editor_frame::Tool::Zoom => Tool::Zoom,
        }
    }
}

/// The note editor's tools, in the bar's order (the zoom with the view's buttons).
pub(crate) const EDITOR_TOOLS: [editor_frame::Tool; 5] = [
    editor_frame::Tool::Pencil,
    editor_frame::Tool::Select,
    editor_frame::Tool::Brush,
    editor_frame::Tool::Scissors,
    editor_frame::Tool::Erase,
];

/// The piano roll's own grid (finer than the timeline's, triplets too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Grid {
    Bar,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    QuarterTriplet,
    EighthTriplet,
    SixteenthTriplet,
}

impl Grid {
    pub const ALL: [Grid; 9] = [
        Grid::Bar,
        Grid::Half,
        Grid::Quarter,
        Grid::Eighth,
        Grid::Sixteenth,
        Grid::ThirtySecond,
        Grid::QuarterTriplet,
        Grid::EighthTriplet,
        Grid::SixteenthTriplet,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Grid::Bar => tr("1 bar"),
            Grid::Half => "1/2",
            Grid::Quarter => "1/4",
            Grid::Eighth => "1/8",
            Grid::Sixteenth => "1/16",
            Grid::ThirtySecond => "1/32",
            Grid::QuarterTriplet => "1/4 T",
            Grid::EighthTriplet => "1/8 T",
            Grid::SixteenthTriplet => "1/16 T",
        }
    }

    /// Step in quarter-note beats; `None` for whole bars.
    pub fn beats(self) -> Option<f64> {
        match self {
            Grid::Bar => None,
            Grid::Half => Some(2.0),
            Grid::Quarter => Some(1.0),
            Grid::Eighth => Some(0.5),
            Grid::Sixteenth => Some(0.25),
            Grid::ThirtySecond => Some(0.125),
            Grid::QuarterTriplet => Some(2.0 / 3.0),
            Grid::EighthTriplet => Some(1.0 / 3.0),
            Grid::SixteenthTriplet => Some(1.0 / 6.0),
        }
    }
}

/// What the lane under the notes shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lane {
    Velocity,
    Probability,
    /// A controller of the selection's channel.
    Controller(u16),
    /// Per-note expression (MPE): pitch bend, pressure or timbre of the selected note alone.
    Expression(u16),
}

impl Lane {
    pub fn all() -> Vec<(Lane, &'static str)> {
        vec![
            (Lane::Velocity, tr("Velocity")),
            (Lane::Probability, tr("Probability")),
            (Lane::Expression(PITCH_BEND), tr("Note pitch (MPE)")),
            (
                Lane::Expression(CHANNEL_PRESSURE),
                tr("Note pressure (MPE)"),
            ),
            (Lane::Expression(74), tr("Note timbre (MPE)")),
            (Lane::Controller(1), tr("Modulation (CC 1)")),
            (Lane::Controller(2), tr("Breath (CC 2)")),
            (Lane::Controller(7), tr("Volume (CC 7)")),
            (Lane::Controller(10), tr("Pan (CC 10)")),
            (Lane::Controller(11), tr("Expression (CC 11)")),
            (Lane::Controller(64), tr("Sustain (CC 64)")),
            (Lane::Controller(74), tr("Slide (CC 74)")),
            (Lane::Controller(PITCH_BEND), tr("Pitch bend")),
            (Lane::Controller(CHANNEL_PRESSURE), tr("Channel pressure")),
        ]
    }

    pub fn name(self) -> &'static str {
        Self::all()
            .into_iter()
            .find(|(l, _)| *l == self)
            .map_or(tr("Velocity"), |(_, n)| n)
    }
}

/// Chords stamped by the pencil and the brush.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Stamp {
    pub on: bool,
    /// `None`: the chord the scale builds on the key (diatonic).
    pub kind: Option<ChordKind>,
    /// Notes of a diatonic chord: 3 (triad) or 4 (seventh).
    pub size: usize,
    pub inversion: usize,
    pub voicing: Voicing,
}

impl Stamp {
    /// The keys of the chord on `key`.
    pub fn keys(&self, key: u8, scale: Option<Scale>) -> Vec<u8> {
        let intervals = match (self.kind, scale) {
            (Some(kind), _) => kind.intervals().to_vec(),
            (None, Some(scale)) => music::diatonic_intervals(scale, key, self.size),
            (None, None) => ChordKind::Major.intervals().to_vec(),
        };
        let root = match (self.kind, scale) {
            (None, Some(scale)) => scale.snap(key),
            _ => key,
        };
        music::build_chord(root, &intervals, self.inversion, self.voicing)
    }

    pub fn label(&self, scale: Option<Scale>) -> String {
        match (self.kind, scale) {
            (Some(kind), _) if kind.symbol().is_empty() => tr("Major").to_owned(),
            (Some(kind), _) => kind.symbol().to_owned(),
            (None, Some(_)) if self.size == 4 => tr("Diatonic 7th").to_owned(),
            (None, Some(_)) => tr("Diatonic").to_owned(),
            (None, None) => tr("Major").to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DragKind {
    Move,
    /// From the right edge.
    Resize,
    /// From the left edge.
    ResizeStart,
    Select {
        anchor: egui::Pos2,
    },
    /// Notes being painted by the brush.
    Paint,
    /// A note just added by the pencil, its length following the pointer.
    Draw,
    Erase,
    /// A value lane: notes passed over take the pointer's value, or a straight line with Shift.
    Values {
        line: bool,
    },
    /// One note's value (and the other selected ones by as much).
    Value {
        index: usize,
    },
}

#[derive(Debug)]
pub(super) struct Drag {
    pub kind: DragKind,
    pub origin: egui::Pos2,
    pub original: Vec<Note>,
    /// Notes painted (brush) or added (pencil) so far, as indices into the edited notes.
    pub painted: Vec<Note>,
    /// Copies are moved, the originals stay (Alt-drag).
    pub copy: bool,
    /// The note grabbed, as it was.
    pub grabbed: Option<Note>,
    /// Notes removed by the eraser (indices into the original notes).
    pub erased: Vec<usize>,
    /// Pointer at the last frame.
    pub last: egui::Pos2,
    /// Selection when the rubber band started (Shift adds to it).
    pub base: Vec<usize>,
    /// The part of each original note, and the one the painted notes go to.
    pub owner: Vec<usize>,
    pub target: usize,
}

impl Drag {
    pub fn new(kind: DragKind, origin: egui::Pos2, original: Vec<Note>, owner: Vec<usize>) -> Self {
        Self {
            kind,
            origin,
            original,
            owner,
            target: 0,
            painted: Vec::new(),
            copy: false,
            grabbed: None,
            erased: Vec::new(),
            last: origin,
            base: Vec::new(),
        }
    }
}

/// A transform being previewed in its panel.
#[derive(Clone, Debug)]
pub(super) struct Pending {
    pub transform: panel::Transform,
    /// The notes before, their parts, and those it applies to.
    pub base: Vec<Note>,
    pub owner: Vec<usize>,
    pub chosen: Vec<usize>,
}

#[derive(Debug)]
pub struct PianoRoll {
    doc: Rc<RefCell<Document>>,
    /// The clips edited together, one part each.
    clips: Vec<ClipId>,
    /// The clip new notes go to (and whose controllers the lane shows).
    active: Option<ClipId>,
    view: View,
    selected: Vec<usize>,
    drag: Option<Drag>,
    /// Notes being edited, with their parts, shown in place of the clips' until committed.
    preview: Option<(Vec<Note>, Vec<usize>)>,
    /// Length of the last note drawn or resized, for the next ones.
    last_length: Option<u64>,
    /// Keyboard commands go to the piano roll after a click in it.
    pub focused: bool,
    lane: Lane,
    lane_height: f32,
    /// Controller points being drawn, and the time of the last one.
    drawing: Option<(Vec<ControlPoint>, u64)>,
    /// Notes copied, from the earliest one's start.
    clipboard: Vec<Note>,
    tool: Tool,
    grid: Grid,
    snap: bool,
    scale: Option<Scale>,
    /// Only the scale's keys and those in use are shown.
    fold: bool,
    /// Notes drawn or moved stay in the scale.
    scale_snap: bool,
    stamp: Stamp,
    /// Notes of the other MIDI clips shown faintly.
    ghosts: bool,
    pending: Option<Pending>,
    /// The note sounding for the user, and on which track.
    sounding: Option<(audiostrada_core::TrackId, u8)>,
    /// The frame the piano roll was last drawn in.
    drawn: u64,
    /// A note just added with a click, sounding until the given time.
    added: Option<(u8, f64)>,
    /// Note under the pointer at the last frame.
    hovered: Option<usize>,
    /// Fit the view to the notes at the next frame.
    fit: bool,
    /// Where the notes were drawn at the last frame.
    area: Rect,
    /// In the note editor: its bar of tools, its keys and its zoom tool.
    editor: bool,
    /// Where the zoom tool's drag began.
    zoom_from: Option<f32>,
    /// The clips the note editor's list asked to edit, for the timeline's selection.
    clip_request: Option<Vec<ClipId>>,
}

/// One of the clips edited together.
#[derive(Clone, Debug)]
pub(super) struct Part {
    pub clip: audiostrada_core::Clip,
    /// Project time of the clip's content start.
    pub origin: u64,
    pub color: egui::Color32,
    pub track: Option<audiostrada_core::TrackId>,
}

/// Everything a frame works with, once borrowed.
pub(super) struct Scene {
    pub project: Project,
    pub parts: Vec<Part>,
    /// The part new notes go to.
    pub active: usize,
    /// The active part's clip, its content start, color and track.
    pub clip: audiostrada_core::Clip,
    pub clip_origin: u64,
    pub color: egui::Color32,
    pub track: Option<audiostrada_core::TrackId>,
    /// Where the notes' times count from: the project's start, for every part alike.
    pub origin: u64,
    /// The notes of every part, in project time, and the part of each.
    pub notes: Vec<Note>,
    pub owner: Vec<usize>,
}

/// The notes of the parts (content start, notes in clip time) as one list in project time,
/// each with its part.
pub(super) fn merge(parts: &[(u64, &[Note])]) -> (Vec<Note>, Vec<usize>) {
    let mut notes = Vec::new();
    let mut owner = Vec::new();
    for (k, (origin, part)) in parts.iter().enumerate() {
        notes.extend(part.iter().map(|n| Note {
            start: origin + n.start,
            ..*n
        }));
        owner.extend(std::iter::repeat_n(k, part.len()));
    }
    (notes, owner)
}

/// The notes of each part back in its clip's time, sorted as clips keep them; a note moved
/// before its clip's content start stays at it.
pub(super) fn split(notes: &[Note], owner: &[usize], origins: &[u64]) -> Vec<Vec<Note>> {
    let mut parts = vec![Vec::new(); origins.len()];
    for (n, &k) in notes.iter().zip(owner) {
        if let (Some(part), Some(&origin)) = (parts.get_mut(k), origins.get(k)) {
            part.push(Note {
                start: n.start.saturating_sub(origin),
                ..*n
            });
        }
    }
    for part in &mut parts {
        part.sort_by_key(|n| (n.start, n.key));
    }
    parts
}

/// What [`per_part`] gives back.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Changed {
    /// The notes merged again, and the part of each.
    pub notes: Vec<Note>,
    pub owner: Vec<usize>,
    /// The selection `change` gave back, if it did, as indices into the merged notes.
    pub picked: Option<Vec<usize>>,
    /// Where each note given now is, if its part kept as many notes before it.
    pub moved: Vec<Option<usize>>,
}

/// Runs `change` on each part's notes alone, with its chosen ones, so that every note stays
/// in its clip whatever `change` adds, removes or reorders.
pub(super) fn per_part(
    notes: &[Note],
    owner: &[usize],
    chosen: &[usize],
    mut change: impl FnMut(&mut Vec<Note>, &[usize]) -> Option<Vec<usize>>,
) -> Changed {
    let parts = owner.iter().max().map_or(0, |m| m + 1);
    let mut out = Changed {
        moved: vec![None; notes.len()],
        ..Changed::default()
    };
    for k in 0..parts {
        let index: Vec<usize> = (0..notes.len()).filter(|&i| owner[i] == k).collect();
        let mut part: Vec<Note> = index.iter().map(|&i| notes[i]).collect();
        let local: Vec<usize> = chosen
            .iter()
            .filter_map(|c| index.iter().position(|i| i == c))
            .collect();
        let first = out.notes.len();
        if let Some(picked) = change(&mut part, &local) {
            out.picked
                .get_or_insert_with(Vec::new)
                .extend(picked.into_iter().map(|i| first + i));
        }
        for (j, &i) in index.iter().enumerate() {
            out.moved[i] = (j < part.len()).then_some(first + j);
        }
        out.owner.extend(std::iter::repeat_n(k, part.len()));
        out.notes.extend(part);
    }
    out
}

impl Scene {
    pub fn timing(&self) -> Timing<'_> {
        Timing {
            tempo: &self.project.tempo,
            rate: self.project.sample_rate,
            origin: self.origin,
        }
    }

    /// Whether note `i` belongs to another track than the active one, drawn faint.
    pub fn dimmed(&self, i: usize) -> bool {
        self.parts.len() > 1 && self.owner.get(i).is_some_and(|&k| k != self.active)
    }
}

impl PianoRoll {
    pub fn new(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            doc,
            clips: Vec::new(),
            active: None,
            view: View::default(),
            selected: Vec::new(),
            drag: None,
            preview: None,
            last_length: None,
            focused: false,
            lane: Lane::Velocity,
            lane_height: 120.0,
            drawing: None,
            clipboard: Vec::new(),
            tool: Tool::Pointer,
            grid: Grid::Sixteenth,
            snap: true,
            scale: None,
            fold: false,
            scale_snap: false,
            stamp: Stamp {
                on: false,
                kind: None,
                size: 3,
                inversion: 0,
                voicing: Voicing::Close,
            },
            ghosts: true,
            pending: None,
            sounding: None,
            drawn: 0,
            added: None,
            hovered: None,
            fit: false,
            area: Rect::NOTHING,
            editor: false,
            zoom_from: None,
            clip_request: None,
        }
    }

    /// The piano roll of the note editor: the editors' bar, keys and zoom.
    pub fn editor(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            editor: true,
            tool: Tool::Draw,
            ..Self::new(doc)
        }
    }

    /// Escape may close the editor: nothing dragged, no transform waiting.
    pub fn lets_escape(&self) -> bool {
        self.drag.is_none() && self.pending.is_none()
    }

    /// Drops the transform waiting to be applied; true if there was one.
    pub fn cancel_transform(&mut self) -> bool {
        self.pending.take().is_some()
    }

    /// The clips edited together.
    #[cfg(test)]
    pub fn edited_clips(&self) -> &[ClipId] {
        &self.clips
    }

    /// The clips to edit the note editor's list asked for, once.
    pub fn take_clip_request(&mut self) -> Option<Vec<ClipId>> {
        self.clip_request.take()
    }

    /// Frames the notes at the next frame.
    pub fn frame_notes(&mut self) {
        self.fit = true;
    }

    /// The tool of the note editor, from its key.
    pub fn set_tool(&mut self, tool: editor_frame::Tool) {
        self.tool = Tool::of(tool);
    }

    /// Takes back the clipboard of `other`, the view this one replaces.
    pub fn keep_clipboard(&mut self, other: &mut Self) {
        std::mem::swap(&mut self.clipboard, &mut other.clipboard);
    }

    pub fn reset(&mut self) {
        self.clips.clear();
        self.active = None;
        self.selected.clear();
        self.drag = None;
        self.preview = None;
        self.pending = None;
        self.focused = false;
    }

    /// The clips edited, the notes of all of them (the preview's while a gesture goes on),
    /// in project time; `None` without a MIDI clip.
    fn scene(&self) -> Option<Scene> {
        let project = self.doc.borrow().project().clone();
        let parts: Vec<Part> = self
            .clips
            .iter()
            .filter_map(|&id| {
                let clip = edit::clip(&project, id)?;
                clip.notes.as_ref()?;
                let track = edit::track_of_clip(&project, id);
                let color = track
                    .and_then(|t| project.track(t))
                    .map_or(egui::Color32::from_rgb(82, 160, 255), |t| {
                        crate::theme::track_color(t.color)
                    });
                Some(Part {
                    clip: clip.clone(),
                    origin: clip.start.saturating_sub(clip.offset),
                    color,
                    track,
                })
            })
            .collect();
        let active = self
            .active
            .and_then(|a| parts.iter().position(|p| p.clip.id == a))
            .unwrap_or(0);
        let first = parts.get(active)?.clone();
        let (notes, owner) = match &self.preview {
            Some((notes, owner)) if owner.iter().all(|&k| k < parts.len()) => {
                (notes.clone(), owner.clone())
            }
            _ => {
                let clips: Vec<(u64, &[Note])> = parts
                    .iter()
                    .map(|p| (p.origin, p.clip.notes.as_deref().map_or(&[][..], |n| n)))
                    .collect();
                merge(&clips)
            }
        };
        Some(Scene {
            active,
            clip: first.clip,
            clip_origin: first.origin,
            color: first.color,
            track: first.track,
            origin: 0,
            notes,
            owner,
            parts,
            project,
        })
    }

    /// The part a note added at `t` goes to: the edited clip of the active track that holds
    /// `t`, else the active part.
    pub(super) fn target(&self, scene: &Scene, t: u64) -> usize {
        let track = scene.track;
        scene
            .parts
            .iter()
            .position(|p| p.track == track && p.clip.start <= t && t < p.clip.end())
            .unwrap_or(scene.active)
    }

    /// Makes the part of note `i` the active one.
    pub(super) fn activate(&mut self, scene: &Scene, i: usize) {
        if let Some(part) = scene.owner.get(i).and_then(|&k| scene.parts.get(k)) {
            self.active = Some(part.clip.id);
        }
    }

    /// Runs `change` on the chosen notes of each part alone (see [`per_part`]) and saves the
    /// result as one undo step.
    fn change_parts(
        &mut self,
        label: &str,
        scene: &Scene,
        chosen: &[usize],
        change: impl FnMut(&mut Vec<Note>, &[usize]) -> Option<Vec<usize>>,
    ) {
        let changed = per_part(&scene.notes, &scene.owner, chosen, change);
        self.selected = match changed.picked {
            Some(picked) => picked,
            None => self
                .selected
                .iter()
                .filter_map(|&i| changed.moved.get(i).copied().flatten())
                .collect(),
        };
        self.commit(label, changed.notes, changed.owner);
    }

    /// Channel the controller lane shows: that of the selected notes when they share one.
    fn lane_channel(&self, notes: &[Note]) -> u8 {
        let mut channels = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i))
            .map(|n| n.channel);
        match channels.next() {
            Some(c) if channels.all(|o| o == c) => c,
            _ => 0,
        }
    }

    /// Saves `notes` (in project time, `owner` giving the part of each) to their clips as one
    /// undo step; the selection stays on the same notes once sorted.
    pub(super) fn commit(&mut self, label: &str, notes: Vec<Note>, owner: Vec<usize>) {
        let project = self.doc.borrow().project().clone();
        let parts: Vec<(ClipId, u64)> = self
            .clips
            .iter()
            .filter_map(|&id| {
                let clip = edit::clip(&project, id)?;
                clip.notes.as_ref()?;
                Some((id, clip.start.saturating_sub(clip.offset)))
            })
            .collect();
        if parts.is_empty() {
            return;
        }
        let origins: Vec<u64> = parts.iter().map(|p| p.1).collect();
        let chosen: Vec<(Note, usize)> = self
            .selected
            .iter()
            .filter_map(|&i| Some((*notes.get(i)?, *owner.get(i)?)))
            .collect();
        let split = split(&notes, &owner, &origins);
        // The notes as the next frame merges them, to find the selection there.
        let views: Vec<(u64, &[Note])> = origins
            .iter()
            .zip(&split)
            .map(|(&o, n)| (o, n.as_slice()))
            .collect();
        let (next, next_owner) = merge(&views);
        let mut taken = vec![false; next.len()];
        self.selected = chosen
            .iter()
            .filter_map(|(c, k)| {
                // Where it lands once kept in its clip.
                let start = c.start.max(origins[*k]);
                let i = (0..next.len()).find(|&i| {
                    !taken[i] && next_owner[i] == *k && next[i] == Note { start, ..*c }
                })?;
                taken[i] = true;
                Some(i)
            })
            .collect();
        self.preview = None;
        self.doc.borrow_mut().history_mut().edit(label, |p| {
            for ((id, _), notes) in parts.iter().zip(split) {
                edit::set_notes(p, *id, notes);
                edit::fit_clip_to_notes(p, *id);
            }
        });
    }

    /// Whether each of `count` notes is selected (constant time lookups).
    fn mask(&self, count: usize) -> Vec<bool> {
        let mut mask = vec![false; count];
        for &i in &self.selected {
            if let Some(m) = mask.get_mut(i) {
                *m = true;
            }
        }
        mask
    }

    /// The selected notes, or all of them when none is.
    fn chosen(&self, count: usize) -> Vec<usize> {
        if self.selected.is_empty() {
            (0..count).collect()
        } else {
            self.selected.clone()
        }
    }

    /// `t` (project samples) on the grid when snapping, unless `free`.
    fn snap(&self, project: &Project, t: u64, free: bool) -> u64 {
        if self.snap && !free {
            edit::snap(t, self.grid.beats(), &project.tempo, project.sample_rate)
        } else {
            t
        }
    }

    /// One grid step from `t` (project samples).
    fn step(&self, project: &Project, t: u64) -> u64 {
        crate::transport::step_length(project, t, self.grid.beats()).max(MIN_LENGTH)
    }

    /// A key in the scale when notes are kept in it.
    fn in_scale(&self, key: u8) -> u8 {
        match self.scale {
            Some(scale) if self.scale_snap => scale.snap(key),
            _ => key,
        }
    }

    /// Copies the selected notes; false if none is.
    pub fn copy(&mut self) -> bool {
        let Some(scene) = self.scene() else {
            return false;
        };
        let mut copied: Vec<Note> = self
            .selected
            .iter()
            .filter_map(|&i| scene.notes.get(i).copied())
            .collect();
        let Some(first) = copied.iter().map(|n| n.start).min() else {
            return false;
        };
        for n in &mut copied {
            n.start -= first;
        }
        self.clipboard = copied;
        true
    }

    pub fn cut(&mut self) -> bool {
        self.copy() && self.delete_selected()
    }

    /// Pastes the copied notes at `at` (project samples, snapped), selected, in the part that
    /// takes new notes there; false if nothing was copied.
    pub fn paste(&mut self, _settings: &EditSettings, at: u64) -> bool {
        let Some(scene) = self.scene() else {
            return false;
        };
        if self.clipboard.is_empty() {
            return false;
        }
        let at = self.snap(&scene.project, at, false);
        let target = self.target(&scene, at);
        let (mut notes, mut owner) = (scene.notes, scene.owner);
        let first = notes.len();
        notes.extend(self.clipboard.iter().map(|n| Note {
            start: n.start + at,
            ..*n
        }));
        owner.resize(notes.len(), target);
        self.selected = (first..notes.len()).collect();
        self.commit(tr("Paste notes"), notes, owner);
        true
    }

    /// Copies the selected notes right after themselves, a whole number of beats later, each
    /// in its own clip.
    pub fn duplicate(&mut self) -> bool {
        let Some(scene) = self.scene() else {
            return false;
        };
        let chosen: Vec<(Note, usize)> = self
            .selected
            .iter()
            .filter_map(|&i| Some((*scene.notes.get(i)?, *scene.owner.get(i)?)))
            .collect();
        let (Some(start), Some(end)) = (
            chosen.iter().map(|(n, _)| n.start).min(),
            chosen.iter().map(|(n, _)| n.start + n.length).max(),
        ) else {
            return false;
        };
        let (rate, tempo) = (scene.project.sample_rate, &scene.project.tempo);
        let from = tempo.sample_to_beat(start, rate);
        let span = (tempo.sample_to_beat(end, rate) - from).ceil().max(1.0);
        let shift = tempo.beat_to_sample(from + span, rate) - start;
        let (mut notes, mut owner) = (scene.notes.clone(), scene.owner.clone());
        let first = notes.len();
        for (n, k) in &chosen {
            notes.push(Note {
                start: n.start + shift,
                ..*n
            });
            owner.push(*k);
        }
        self.selected = (first..notes.len()).collect();
        self.commit(tr("Duplicate notes"), notes, owner);
        true
    }

    /// Moves the selected notes by a grid step; false if none is selected.
    pub fn nudge(&mut self, _settings: &EditSettings, later: bool) -> bool {
        let Some(scene) = self.scene() else {
            return false;
        };
        if self.selected.is_empty() {
            return false;
        }
        let (mut notes, owner) = (scene.notes, scene.owner);
        let step = self
            .selected
            .first()
            .and_then(|&i| notes.get(i))
            .map_or(MIN_LENGTH, |n| self.step(&scene.project, n.start));
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.start = if later {
                    n.start + step
                } else {
                    n.start.saturating_sub(step)
                };
            }
        }
        self.commit(tr("Nudge notes"), notes, owner);
        true
    }

    /// Moves the selected notes up or down by semitones (by scale steps when notes are kept in
    /// the scale).
    pub fn transpose(&mut self, semitones: i32) {
        let Some(scene) = self.scene() else {
            return;
        };
        if self.selected.is_empty() {
            return;
        }
        let (mut notes, owner) = (scene.notes, scene.owner);
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.key = match self.scale {
                    Some(scale) if self.scale_snap && semitones.abs() < 12 => {
                        scale.step(n.key, semitones)
                    }
                    _ => (i32::from(n.key) + semitones).clamp(0, 127) as u8,
                };
            }
        }
        self.commit(tr("Transpose notes"), notes, owner);
    }

    /// Applies `change` to the selected notes, or all, of each part alone, as one undo step.
    fn instant(&mut self, label: &str, mut change: impl FnMut(&mut Vec<Note>, &[usize])) {
        let Some(scene) = self.scene() else {
            return;
        };
        let chosen = self.chosen(scene.notes.len());
        self.change_parts(label, &scene, &chosen, |notes, chosen| {
            change(notes, chosen);
            None
        });
    }

    /// Gives the overlapping notes of each part their own MIDI channel (MPE).
    pub(super) fn spread_channels(&mut self, scene: &Scene) {
        let all: Vec<usize> = (0..scene.notes.len()).collect();
        self.change_parts(
            tr("Spread notes over MPE channels"),
            scene,
            &all,
            |notes, _| {
                notes.sort_by_key(|n| (n.start, n.key));
                edit::spread_mpe_channels(notes);
                None
            },
        );
    }

    /// Deletes the selected notes; false if nothing was selected.
    pub fn delete_selected(&mut self) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let Some(scene) = self.scene() else {
            return false;
        };
        let mask = self.mask(scene.notes.len());
        let (notes, owner): (Vec<Note>, Vec<usize>) = scene
            .notes
            .iter()
            .zip(&scene.owner)
            .enumerate()
            .filter(|(i, _)| !mask[*i])
            .map(|(_, (n, k))| (*n, *k))
            .unzip();
        self.selected.clear();
        self.commit(tr("Delete notes"), notes, owner);
        true
    }

    pub fn select_all(&mut self) {
        if let Some(scene) = self.scene() {
            self.selected = (0..scene.notes.len()).collect();
        }
    }

    /// Quantizes the selected notes, or all of them, to the piano roll's grid.
    pub fn quantize(&mut self, _settings: &EditSettings) {
        let project = self.doc.borrow().project().clone();
        let step = self.grid.beats().unwrap_or(4.0);
        let timing = Timing {
            tempo: &project.tempo,
            rate: project.sample_rate,
            origin: 0,
        };
        let q = ops::Quantize {
            step,
            strength: 1.0,
            swing: 0.0,
            ends: false,
        };
        self.instant(tr("Quantize notes"), |notes, chosen| {
            ops::quantize(notes, chosen, timing, &q)
        });
    }

    /// Lets go of the note sounding for the user.
    pub fn release_preview(&mut self) {
        if let Some((t, k)) = self.sounding.take() {
            self.doc.borrow().player().play_key(t, [0x80, k, 0]);
        }
        self.added = None;
    }

    /// Lets go of the sounding note when the piano roll was not drawn at the last frame
    /// (its tab hidden, its window closed), so that no note keeps sounding.
    pub fn release_if_hidden(&mut self, frame: u64) {
        if self.sounding.is_some() && self.drawn + 1 < frame {
            self.release_preview();
        }
    }

    /// Draws the notes of `clips` (the MIDI ones), edited together.
    pub fn show(&mut self, ui: &mut Ui, _settings: &EditSettings, clips: &[ClipId]) {
        self.drawn = ui.ctx().cumulative_frame_nr();
        if clips != self.clips.as_slice() && self.drag.is_none() {
            // A clip added or taken away keeps the view; another set frames its notes.
            let kept = clips.iter().any(|c| self.clips.contains(c));
            self.clips = clips.to_vec();
            self.selected.clear();
            self.preview = None;
            self.pending = None;
            self.fit |= !kept;
            if self.active.is_none_or(|a| !clips.contains(&a)) {
                self.active = clips.last().copied();
            }
        }
        let Some(mut scene) = self.scene() else {
            ui.centered_and_justified(|ui| {
                ui.weak(tr("Select a MIDI clip to edit its notes.\nCreate one with Track › New MIDI clip, or by double-clicking an empty part of a track."));
            });
            return;
        };
        let p = crate::theme::current(ui);

        if self.editor {
            self.editor_toolbar(ui, &scene);
        } else {
            let (bar, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), TOOLBAR_HEIGHT), Sense::hover());
            self.toolbar(ui, bar, &scene);
        }
        if let Some(pending) = &self.pending {
            let changed = panel::apply_parts(pending, scene.timing(), self.scale);
            scene.notes = changed.notes;
            scene.owner = changed.owner;
        }

        let mut full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        // The note editor's tracks at the left, as the automation editor's names.
        if self.editor {
            let list = Rect::from_min_max(full.min, pos2(full.left() + PARTS_WIDTH, full.bottom()));
            self.parts_list(ui, &scene, list);
            full.min.x = list.right() + 1.0;
        }
        let lane_height = self.lane_height.min(full.height() * 0.3).max(48.0);
        let lanes_top = full.bottom() - lane_height;
        let chords = Rect::from_min_max(
            pos2(full.left() + KEYS_WIDTH, full.top()),
            pos2(full.right(), full.top() + CHORDS_HEIGHT),
        );
        let ruler = Rect::from_min_max(
            pos2(full.left() + KEYS_WIDTH, chords.bottom()),
            pos2(full.right(), chords.bottom() + RULER_HEIGHT),
        );
        let area = Rect::from_min_max(
            pos2(full.left() + KEYS_WIDTH, ruler.bottom()),
            pos2(full.right(), lanes_top - SPLITTER),
        );
        let keys = Rect::from_min_max(
            pos2(full.left(), area.top()),
            pos2(area.left(), area.bottom()),
        );
        let corner = Rect::from_min_max(full.left_top(), pos2(area.left(), area.top()));
        let lane = Rect::from_min_max(pos2(area.left(), lanes_top), full.right_bottom());
        let lane_head = Rect::from_min_max(
            pos2(full.left(), lanes_top),
            pos2(area.left(), full.bottom()),
        );
        let splitter = Rect::from_min_max(
            pos2(full.left(), area.bottom()),
            pos2(full.right(), lanes_top),
        );

        self.area = area;
        self.update_rows(&scene);
        if std::mem::take(&mut self.fit) {
            self.fit_view(&scene, area);
        }
        self.navigate(ui, full, area);
        // Pages along with the playhead while playing.
        let (position, playing) = {
            let doc = self.doc.borrow();
            (doc.player().position(), doc.player().engine().playing())
        };
        if playing && self.drag.is_none() {
            let x = self.frame(area).x(position);
            if x > area.right() - 8.0 || x < area.left() {
                self.view.scroll =
                    position as f64 - f64::from(area.width()) * self.view.zoom * 0.05;
            }
            ui.ctx().request_repaint();
        }
        self.view.clamp(area.height());

        let painter = ui.painter().with_clip_rect(full);
        painter.rect_filled(full, 0.0, p.base);
        self.draw_grid(ui, &scene, area);
        if self.ghosts && !self.editor {
            self.draw_ghosts(ui, &scene, area);
        }
        self.draw_notes(ui, &scene, area);
        self.draw_ruler(ui, &scene, ruler);
        self.draw_chords(ui, &scene, chords);
        self.draw_corner(ui, corner);
        self.keyboard(ui, &scene, keys, area);
        self.splitter(ui, splitter, full);
        self.lane(ui, &mut scene, lane_head, lane, area);
        if self.tool == Tool::Zoom {
            self.zoom_tool(ui, area);
        } else if self.pending.is_none() {
            self.interact(ui, &scene, area);
        } else {
            ui.interact(
                area,
                ui.scope_id().with("piano-blocked"),
                Sense::click_and_drag(),
            );
        }
        self.draw_playhead(
            ui,
            &scene,
            Rect::from_min_max(ruler.min, pos2(area.right(), full.bottom())),
        );
        if self.editor && self.pending.is_none() {
            let painter = ui.painter().with_clip_rect(area);
            editor_frame::help(ui, &painter, area, self.editor_help());
        }
        self.transform_panel(ui, &scene, area);
        self.shortcuts(ui);
        self.sound(ui, &scene, keys, area);
    }

    /// Keys shown: all of them, or folded to the scale and the keys in use.
    fn update_rows(&mut self, scene: &Scene) {
        let rows: Vec<u8> = if self.fold {
            (0..=127u8)
                .rev()
                .filter(|&k| {
                    self.scale.is_some_and(|s| s.contains(k)) && (24..=108).contains(&k)
                        || scene.notes.iter().any(|n| n.key == k)
                })
                .collect()
        } else {
            (0..=127u8).rev().collect()
        };
        if rows != self.view.rows {
            let middle = scene
                .notes
                .iter()
                .map(|n| u32::from(n.key))
                .sum::<u32>()
                .checked_div(scene.notes.len() as u32)
                .unwrap_or(60) as u8;
            self.view.set_rows(rows, middle);
        }
    }

    /// Frames the clip and its notes.
    fn fit_view(&mut self, scene: &Scene, area: Rect) {
        let mut keys: Vec<u8> = scene.notes.iter().map(|n| n.key).collect();
        keys.sort_unstable();
        let fits = (area.height() / 11.0) as usize;
        let (low, high) = match (keys.first(), keys.last()) {
            (Some(&l), Some(&h)) if usize::from(h - l) + 4 <= fits => (l, h),
            (Some(_), Some(_)) => {
                let middle = keys[keys.len() / 2];
                let half = (fits / 2).saturating_sub(2) as u8;
                (
                    middle.saturating_sub(half),
                    middle.saturating_add(half).min(127),
                )
            }
            _ => (55, 72),
        };
        let (low, high) = if high - low < 12 {
            (low.saturating_sub(6), high.saturating_add(6).min(127))
        } else {
            (low, high)
        };
        self.view.fit(
            scene.parts.iter().map(|p| p.clip.start).min().unwrap_or(0),
            scene.parts.iter().map(|p| p.clip.end()).max().unwrap_or(0),
            low,
            high,
            area.width(),
            area.height(),
        );
    }

    /// Wheel: through the keys (Shift: in time); Ctrl+wheel or pinch zooms time and Alt+wheel
    /// the keys, around the pointer; the middle button drags the view.
    fn navigate(&mut self, ui: &Ui, full: Rect, area: Rect) {
        if !ui.rect_contains_pointer(full) {
            return;
        }
        let (scroll, zoom, modifiers, pointer, middle, delta) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.modifiers,
                i.pointer.hover_pos(),
                i.pointer.middle_down(),
                i.pointer.delta(),
            )
        });
        let at = pointer.unwrap_or(area.center());
        if zoom != 1.0 {
            self.view.zoom_time(f64::from(zoom), at.x - area.left());
        } else if modifiers.alt && scroll.y != 0.0 {
            self.view
                .zoom_keys((scroll.y * 0.01).exp(), at.y - area.top());
        } else if modifiers.shift {
            self.view.scroll -= f64::from(scroll.y + scroll.x) * self.view.zoom;
        } else {
            self.view.scroll -= f64::from(scroll.x) * self.view.zoom;
            self.view.scroll_y -= scroll.y;
        }
        if middle {
            self.view.scroll -= f64::from(delta.x) * self.view.zoom;
            self.view.scroll_y -= delta.y;
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }

    /// The edge between the notes and the lane, dragged to share the height.
    fn splitter(&mut self, ui: &Ui, rect: Rect, full: Rect) {
        let p = crate::theme::current(ui);
        let response = ui.interact(rect, ui.scope_id().with("piano-splitter"), Sense::drag());
        let hot = response.hovered() || response.dragged();
        ui.painter().rect_filled(rect, 0.0, p.panel);
        ui.painter().line_segment(
            [
                rect.left_center() + vec2(KEYS_WIDTH, 0.0),
                rect.right_center(),
            ],
            egui::Stroke::new(
                if hot { 2.0 } else { 1.0 },
                if hot { p.accent } else { p.border },
            ),
        );
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if response.dragged() {
            self.lane_height = (self.lane_height - response.drag_delta().y)
                .clamp(48.0, (full.height() * 0.6).max(48.0));
        }
    }

    /// The zoom tool over the notes: a span dragged across fills the view, a click zooms in
    /// (Alt or right-click: out).
    fn zoom_tool(&mut self, ui: &mut Ui, area: Rect) {
        let painter = ui.painter().with_clip_rect(area);
        let asked = editor_frame::zoom_tool(ui, area, &painter, &mut self.zoom_from);
        match asked {
            Some(editor_frame::Zoomed::Span(a, b)) => {
                let frame = self.frame(area);
                let (from, to) = (frame.t(a), frame.t(b));
                self.view.zoom = (to.saturating_sub(from) as f64 / f64::from(area.width()))
                    .clamp(view::MIN_ZOOM, view::MAX_ZOOM);
                self.view.scroll = from as f64;
            }
            Some(editor_frame::Zoomed::In(x)) => self.view.zoom_time(2.0, x - area.left()),
            Some(editor_frame::Zoomed::Out(x)) => self.view.zoom_time(0.5, x - area.left()),
            None => {}
        }
    }

    /// What the tool in hand does, on the note editor's pill.
    fn editor_help(&self) -> &'static str {
        match self.tool {
            Tool::Draw => tr(
                "Click: add a note · Drag right: set its length · Drag a note: move it · Drag an edge: resize it · Ctrl: place freely",
            ),
            Tool::Pointer => tr(
                "Click a note: select it · Drag: select in a rectangle · Drag a note: move it (Alt: copy) · Drag an edge: resize it · Double-click: add a note",
            ),
            Tool::Paint => tr("Drag: paint a note on each grid step, up and down too"),
            Tool::Slice => tr("Click a note: cut it there (the selected ones under it too)"),
            Tool::Erase => tr("Click a note: remove it · Drag: remove the notes passed over"),
            Tool::Zoom => tr(
                "Drag across a span: fill the view with it · Click: zoom in · Alt+click or right-click: zoom out · Shift+wheel: move in time",
            ),
        }
    }

    /// Tools by number keys; up and down transpose the selection, by an octave with Shift.
    fn shortcuts(&mut self, ui: &Ui) {
        if !self.focused || ui.ctx().text_edit_focused() {
            return;
        }
        // The note editor's tools answer to the editors' letters, taken before.
        let tool = ui.input_mut(|i| {
            Tool::ALL
                .into_iter()
                .filter(|_| !self.editor)
                .find(|t| i.consume_key(egui::Modifiers::NONE, t.key()))
        });
        if let Some(tool) = tool {
            self.tool = tool;
        }
        if self.pending.is_some() {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.pending = None;
            }
            return;
        }
        if self.selected.is_empty() {
            return;
        }
        let moves = ui.input_mut(|i| {
            [
                (egui::Modifiers::NONE, egui::Key::ArrowUp, 1),
                (egui::Modifiers::NONE, egui::Key::ArrowDown, -1),
                (egui::Modifiers::SHIFT, egui::Key::ArrowUp, 12),
                (egui::Modifiers::SHIFT, egui::Key::ArrowDown, -12),
            ]
            .into_iter()
            .filter(|(m, k, _)| i.consume_key(*m, *k))
            .map(|(_, _, by)| by)
            .sum::<i32>()
        });
        if moves != 0 {
            self.transpose(moves);
        }
    }

    /// Sounding: the key pressed on the keyboard, the note dragged or painted, a note just
    /// added.
    fn sound(&mut self, ui: &Ui, scene: &Scene, keys: Rect, area: Rect) {
        let frame = self.frame(area);
        let keys_response = ui.interact(
            keys,
            ui.scope_id().with("piano-keys"),
            Sense::click_and_drag(),
        );
        if keys_response.double_clicked()
            && let Some(pos) = keys_response.interact_pointer_pos()
        {
            let key = frame.key(pos.y);
            self.selected = (0..scene.notes.len())
                .filter(|&i| scene.notes[i].key == key)
                .collect();
        }
        let keys_response = keys_response
            .on_hover_text(tr("Click to hear a key, double-click to select its notes"));
        let now = ui.input(|i| i.time);
        let wanted = if keys_response.is_pointer_button_down_on() {
            keys_response.interact_pointer_pos().map(|p| frame.key(p.y))
        } else {
            match self.drag.as_ref().map(|d| d.kind) {
                Some(DragKind::Move | DragKind::Draw) => self
                    .selected
                    .first()
                    .and_then(|&i| self.preview.as_ref()?.0.get(i))
                    .map(|n| n.key),
                Some(DragKind::Paint) => self
                    .drag
                    .as_ref()
                    .and_then(|d| d.painted.last())
                    .map(|n| n.key),
                _ => self.added.filter(|(_, until)| now < *until).map(|(k, _)| k),
            }
        };
        if wanted != self.sounding.map(|(_, k)| k) {
            let doc = self.doc.borrow();
            if let Some((t, k)) = self.sounding.take() {
                doc.player().play_key(t, [0x80, k, 0]);
            }
            // A note of another part plays on its own track.
            let track = self
                .selected
                .first()
                .filter(|_| self.drag.is_some() || self.added.is_some())
                .and_then(|&i| scene.parts.get(*scene.owner.get(i)?))
                .map_or(scene.track, |p| p.track);
            if let (Some(k), Some(t)) = (wanted, track) {
                doc.player().play_key(t, [0x90, k, 100]);
                self.sounding = Some((t, k));
            }
        }
        if self.sounding.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// Label of the notes count and selection, for the toolbar.
    fn summary(&self, notes: &[Note]) -> String {
        if self.selected.is_empty() {
            trf!("{count} notes", count = notes.len())
        } else {
            trf!(
                "{selected} of {count} notes",
                selected = self.selected.len(),
                count = notes.len()
            )
        }
    }
}

#[cfg(test)]
mod tests;
