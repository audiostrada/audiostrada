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
use crate::i18n::tr;
use crate::transport::EditSettings;
use crate::trf;

use music::{ChordKind, Scale, Voicing};
use ops::{MIN_LENGTH, Timing};
use view::View;

const KEYS_WIDTH: f32 = 74.0;
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
        }
    }

    fn key(self) -> egui::Key {
        match self {
            Tool::Pointer => egui::Key::Num1,
            Tool::Draw => egui::Key::Num2,
            Tool::Paint => egui::Key::Num3,
            Tool::Slice => egui::Key::Num4,
            Tool::Erase => egui::Key::Num5,
        }
    }
}

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
}

impl Drag {
    pub fn new(kind: DragKind, origin: egui::Pos2, original: Vec<Note>) -> Self {
        Self {
            kind,
            origin,
            original,
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
    /// The notes before, and those it applies to.
    pub base: Vec<Note>,
    pub chosen: Vec<usize>,
}

#[derive(Debug)]
pub struct PianoRoll {
    doc: Rc<RefCell<Document>>,
    clip: Option<ClipId>,
    view: View,
    selected: Vec<usize>,
    drag: Option<Drag>,
    preview: Option<Vec<Note>>,
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
}

/// Everything a frame works with, once borrowed.
pub(super) struct Scene {
    pub project: Project,
    pub clip: audiostrada_core::Clip,
    /// Project time of the clip's content start.
    pub origin: u64,
    pub notes: Vec<Note>,
    pub color: egui::Color32,
    pub track: Option<audiostrada_core::TrackId>,
}

impl Scene {
    pub fn timing(&self) -> Timing<'_> {
        Timing {
            tempo: &self.project.tempo,
            rate: self.project.sample_rate,
            origin: self.origin,
        }
    }
}

impl PianoRoll {
    pub fn new(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            doc,
            clip: None,
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
        }
    }

    /// Takes back the clipboard of `other`, the view this one replaces.
    pub fn keep_clipboard(&mut self, other: &mut Self) {
        std::mem::swap(&mut self.clipboard, &mut other.clipboard);
    }

    pub fn reset(&mut self) {
        self.clip = None;
        self.selected.clear();
        self.drag = None;
        self.preview = None;
        self.pending = None;
        self.focused = false;
    }

    fn notes(&self, project: &Project) -> Option<(u64, Vec<Note>)> {
        let clip = edit::clip(project, self.clip?)?;
        let origin = clip.start.saturating_sub(clip.offset);
        let notes = self
            .preview
            .clone()
            .or_else(|| clip.notes.as_ref().map(|n| (**n).clone()))?;
        Some((origin, notes))
    }

    fn scene(&self) -> Option<Scene> {
        let project = self.doc.borrow().project().clone();
        let (origin, notes) = self.notes(&project)?;
        let clip = edit::clip(&project, self.clip?)?.clone();
        let track = edit::track_of_clip(&project, clip.id);
        let color = track
            .and_then(|t| project.track(t))
            .map_or(egui::Color32::from_rgb(82, 160, 255), |t| {
                crate::theme::track_color(t.color)
            });
        Some(Scene {
            project,
            clip,
            origin,
            notes,
            color,
            track,
        })
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

    /// Saves `notes` as one undo step; the selection stays on the same notes once sorted.
    fn commit(&mut self, label: &str, notes: Vec<Note>) {
        let Some(id) = self.clip else { return };
        let chosen: Vec<Note> = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i).copied())
            .collect();
        let mut sorted = notes.clone();
        sorted.sort_by_key(|n| (n.start, n.key));
        let mut taken = vec![false; sorted.len()];
        self.selected = chosen
            .iter()
            .filter_map(|c| {
                let i = (0..sorted.len()).find(|&i| !taken[i] && sorted[i] == *c)?;
                taken[i] = true;
                Some(i)
            })
            .collect();
        self.preview = None;
        self.doc.borrow_mut().history_mut().edit(label, |p| {
            edit::set_notes(p, id, notes);
            edit::fit_clip_to_notes(p, id);
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
        let project = self.doc.borrow().project().clone();
        let Some((_, notes)) = self.notes(&project) else {
            return false;
        };
        let mut copied: Vec<Note> = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i).copied())
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

    /// Pastes the copied notes at `at` (project samples, snapped), selected; false if nothing
    /// was copied.
    pub fn paste(&mut self, _settings: &EditSettings, at: u64) -> bool {
        let project = self.doc.borrow().project().clone();
        let Some((origin, mut notes)) = self.notes(&project) else {
            return false;
        };
        if self.clipboard.is_empty() {
            return false;
        }
        let at = self.snap(&project, at, false).max(origin) - origin;
        let first = notes.len();
        notes.extend(self.clipboard.iter().map(|n| Note {
            start: n.start + at,
            ..*n
        }));
        self.selected = (first..notes.len()).collect();
        self.commit(tr("Paste notes"), notes);
        true
    }

    /// Copies the selected notes right after themselves, a whole number of beats later.
    pub fn duplicate(&mut self) -> bool {
        let project = self.doc.borrow().project().clone();
        let Some((origin, mut notes)) = self.notes(&project) else {
            return false;
        };
        let chosen: Vec<Note> = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i).copied())
            .collect();
        let (Some(start), Some(end)) = (
            chosen.iter().map(|n| n.start).min(),
            chosen.iter().map(|n| n.start + n.length).max(),
        ) else {
            return false;
        };
        let (rate, tempo) = (project.sample_rate, &project.tempo);
        let from = tempo.sample_to_beat(origin + start, rate);
        let span = (tempo.sample_to_beat(origin + end, rate) - from)
            .ceil()
            .max(1.0);
        let shift = tempo.beat_to_sample(from + span, rate) - (origin + start);
        let first = notes.len();
        notes.extend(chosen.iter().map(|n| Note {
            start: n.start + shift,
            ..*n
        }));
        self.selected = (first..notes.len()).collect();
        self.commit(tr("Duplicate notes"), notes);
        true
    }

    /// Moves the selected notes by a grid step; false if none is selected.
    pub fn nudge(&mut self, _settings: &EditSettings, later: bool) -> bool {
        let project = self.doc.borrow().project().clone();
        let Some((origin, mut notes)) = self.notes(&project) else {
            return false;
        };
        if self.selected.is_empty() {
            return false;
        }
        let step = self
            .selected
            .first()
            .and_then(|&i| notes.get(i))
            .map_or(MIN_LENGTH, |n| self.step(&project, origin + n.start));
        for &i in &self.selected {
            if let Some(n) = notes.get_mut(i) {
                n.start = if later {
                    n.start + step
                } else {
                    n.start.saturating_sub(step)
                };
            }
        }
        self.commit(tr("Nudge notes"), notes);
        true
    }

    /// Moves the selected notes up or down by semitones (by scale steps when notes are kept in
    /// the scale).
    pub fn transpose(&mut self, semitones: i32) {
        let project = self.doc.borrow().project().clone();
        let Some((_, mut notes)) = self.notes(&project) else {
            return;
        };
        if self.selected.is_empty() {
            return;
        }
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
        self.commit(tr("Transpose notes"), notes);
    }

    /// Applies `change` to the selected notes, or all, as one undo step.
    fn instant(&mut self, label: &str, change: impl FnOnce(&mut Vec<Note>, &[usize])) {
        let project = self.doc.borrow().project().clone();
        let Some((_, mut notes)) = self.notes(&project) else {
            return;
        };
        let chosen = self.chosen(notes.len());
        change(&mut notes, &chosen);
        self.commit(label, notes);
    }

    /// Deletes the selected notes; false if nothing was selected.
    pub fn delete_selected(&mut self) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        let project = self.doc.borrow().project().clone();
        let Some((_, notes)) = self.notes(&project) else {
            return false;
        };
        let mask = self.mask(notes.len());
        let kept: Vec<Note> = notes
            .iter()
            .enumerate()
            .filter(|(i, _)| !mask[*i])
            .map(|(_, n)| *n)
            .collect();
        self.selected.clear();
        self.commit(tr("Delete notes"), kept);
        true
    }

    pub fn select_all(&mut self) {
        let project = self.doc.borrow().project().clone();
        if let Some((_, notes)) = self.notes(&project) {
            self.selected = (0..notes.len()).collect();
        }
    }

    /// Quantizes the selected notes, or all of them, to the piano roll's grid.
    pub fn quantize(&mut self, _settings: &EditSettings) {
        let project = self.doc.borrow().project().clone();
        let Some((origin, _)) = self.notes(&project) else {
            return;
        };
        let step = self.grid.beats().unwrap_or(4.0);
        let timing = Timing {
            tempo: &project.tempo,
            rate: project.sample_rate,
            origin,
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

    pub fn show(&mut self, ui: &mut Ui, _settings: &EditSettings, clip: Option<ClipId>) {
        self.drawn = ui.ctx().cumulative_frame_nr();
        if clip != self.clip && self.drag.is_none() {
            self.clip = clip;
            self.selected.clear();
            self.preview = None;
            self.pending = None;
            self.fit = true;
        }
        let Some(mut scene) = self.scene() else {
            ui.centered_and_justified(|ui| {
                ui.weak(tr("Select a MIDI clip to edit its notes.\nCreate one with Track › New MIDI clip, or by double-clicking an empty part of a track."));
            });
            return;
        };
        let p = crate::theme::current(ui);

        let (bar, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), TOOLBAR_HEIGHT), Sense::hover());
        self.toolbar(ui, bar, &scene);
        if let Some(pending) = &self.pending {
            let (notes, selection) = panel::apply(
                &pending.transform,
                &pending.base,
                &pending.chosen,
                scene.timing(),
                self.scale,
            );
            let _ = selection;
            scene.notes = notes;
        }

        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
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
        if self.ghosts {
            self.draw_ghosts(ui, &scene, area);
        }
        self.draw_notes(ui, &scene, area);
        self.draw_ruler(ui, &scene, ruler);
        self.draw_chords(ui, &scene, chords);
        self.draw_corner(ui, corner);
        self.keyboard(ui, &scene, keys, area);
        self.splitter(ui, splitter, full);
        self.lane(ui, &mut scene, lane_head, lane, area);
        if self.pending.is_none() {
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
            scene.clip.start,
            scene.clip.end(),
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

    /// Tools by number keys; up and down transpose the selection, by an octave with Shift.
    fn shortcuts(&mut self, ui: &Ui) {
        if !self.focused || ui.ctx().text_edit_focused() {
            return;
        }
        let tool = ui.input_mut(|i| {
            Tool::ALL
                .into_iter()
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
                    .and_then(|&i| self.preview.as_ref()?.get(i))
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
            if let (Some(k), Some(t)) = (wanted, scene.track) {
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
