//! Piano roll: edits the notes of the selected MIDI clip. Click to add a note, drag to move or
//! (from its right edge) resize, Alt-drag to copy, double-click to delete, drag in the velocity
//! lane, rubber band selection; a draw tool paints notes along the grid. Copy, cut, paste,
//! duplicate, nudge, transpose (arrows), quantize, legato and humanize; a scale shades the keys
//! outside it; notes sound while they are played, added or moved. A drag is one undo step.

use std::cell::RefCell;
use std::rc::Rc;

use audiostrada_core::model::{CHANNEL_PRESSURE, PITCH_BEND};
use audiostrada_core::{ClipId, ControlPoint, Note, Project, edit};
use audiostrada_dsp::pitch_correct::{NOTE_NAMES, SCALES};
use audiostrada_midi::key_name;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};

use crate::document::Document;
use crate::i18n::tr;
use crate::transport::EditSettings;
use crate::trf;

const KEYS_WIDTH: f32 = 56.0;
const RULER_HEIGHT: f32 = 20.0;
const VELOCITY_HEIGHT: f32 = 90.0;

/// What the lane under the notes edits: velocity, or a controller.
fn lanes() -> [(&'static str, Option<u16>); 10] {
    [
        (tr("Velocity"), None),
        (tr("Modulation (CC 1)"), Some(1)),
        (tr("Breath (CC 2)"), Some(2)),
        (tr("Volume (CC 7)"), Some(7)),
        (tr("Pan (CC 10)"), Some(10)),
        (tr("Expression (CC 11)"), Some(11)),
        (tr("Sustain (CC 64)"), Some(64)),
        (tr("Slide (CC 74)"), Some(74)),
        (tr("Pitch bend"), Some(PITCH_BEND)),
        (tr("Channel pressure"), Some(CHANNEL_PRESSURE)),
    ]
}
const TOOLBAR_HEIGHT: f32 = 28.0;
const EDGE: f32 = 6.0;
const MIN_LENGTH: u64 = 16;

/// How long a note added with a click sounds, in seconds.
const AUDITION: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragKind {
    Move,
    Resize,
    Select {
        anchor: Pos2,
    },
    Velocity,
    /// The draw tool: a note per grid step along the pointer.
    Paint,
}

#[derive(Debug)]
struct Drag {
    kind: DragKind,
    origin: Pos2,
    original: Vec<Note>,
    /// Notes painted so far.
    painted: Vec<Note>,
    /// Copies are moved, the originals stay (Alt-drag).
    copy: bool,
}

#[derive(Debug)]
pub struct PianoRoll {
    doc: Rc<RefCell<Document>>,
    clip: Option<ClipId>,
    /// Project samples per pixel.
    zoom: f64,
    /// Project time at the left edge.
    scroll: f64,
    key_height: f32,
    scroll_y: f32,
    selected: Vec<usize>,
    drag: Option<Drag>,
    preview: Option<Vec<Note>>,
    last_length: Option<u64>,
    /// Keyboard commands go to the piano roll after a click in it.
    pub focused: bool,
    /// Controller shown in the lane under the notes; `None` for velocity.
    lane: Option<u16>,
    /// Controller points being drawn, and the time of the last one.
    drawing: Option<(Vec<ControlPoint>, u64)>,
    /// Notes copied, from the earliest one's start.
    clipboard: Vec<Note>,
    /// The draw tool is on: dragging paints notes.
    draw: bool,
    /// Root of the scale shown (0 = C) and its index in [`SCALES`], if any.
    scale: (u8, Option<usize>),
    /// The note sounding for the user, and on which track.
    sounding: Option<(audiostrada_core::TrackId, u8)>,
    /// A note just added with a click, sounding until the given time.
    added: Option<(u8, f64)>,
}

/// Tint of the scale's root rows.
fn color_of_root(visuals: &egui::Visuals) -> Color32 {
    visuals.selection.bg_fill
}

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// Whether a key is in a scale from `root`.
fn in_scale(key: u8, root: u8, scale: usize) -> bool {
    SCALES[scale.min(SCALES.len() - 1)].1[usize::from((key + 12 - root % 12) % 12)]
}

fn transpose(notes: &mut [Note], chosen: &[usize], semitones: i32) {
    for &i in chosen {
        if let Some(n) = notes.get_mut(i) {
            n.key = (i32::from(n.key) + semitones).clamp(0, 127) as u8;
        }
    }
}

/// Lengthens each chosen note up to the start of the next note (of any key), so the line has
/// no gaps; the last one keeps its length.
fn legato(notes: &mut [Note], chosen: &[usize]) {
    let starts: Vec<u64> = notes.iter().map(|n| n.start).collect();
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else {
            continue;
        };
        if let Some(next) = starts.iter().filter(|&&s| s > n.start).min() {
            n.length = next - n.start;
        }
    }
}

/// Moves each chosen note by up to `spread` samples either way and changes its velocity by up
/// to 10, from a seeded generator so the result is repeatable.
fn humanize(notes: &mut [Note], chosen: &[usize], spread: u64, mut seed: u32) {
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    for &i in chosen {
        let Some(n) = notes.get_mut(i) else {
            continue;
        };
        let shift = (random() * spread as f64) as i64;
        n.start = (n.start as i64 + shift).max(0) as u64;
        n.velocity = (f64::from(n.velocity) + random() * 10.0)
            .round()
            .clamp(1.0, 127.0) as u8;
    }
}

impl PianoRoll {
    pub fn new(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            doc,
            clip: None,
            zoom: 100.0,
            scroll: 0.0,
            key_height: 12.0,
            scroll_y: 40.0 * 12.0,
            selected: Vec::new(),
            drag: None,
            preview: None,
            last_length: None,
            focused: false,
            lane: None,
            drawing: None,
            clipboard: Vec::new(),
            draw: false,
            scale: (0, None),
            sounding: None,
            added: None,
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

    fn commit(&mut self, label: &str, notes: Vec<Note>) {
        let Some(id) = self.clip else { return };
        // Keep the selection on the same notes once they are sorted.
        let chosen: Vec<Note> = self
            .selected
            .iter()
            .filter_map(|&i| notes.get(i).copied())
            .collect();
        let mut sorted = notes.clone();
        sorted.sort_by_key(|n| (n.start, n.key));
        self.selected = chosen
            .iter()
            .filter_map(|c| sorted.iter().position(|n| n == c))
            .collect();
        self.doc.borrow_mut().history_mut().edit(label, |p| {
            edit::set_notes(p, id, notes);
            edit::fit_clip_to_notes(p, id);
        });
    }

    /// The selected notes, or all of them when none is.
    fn chosen(&self, count: usize) -> Vec<usize> {
        if self.selected.is_empty() {
            (0..count).collect()
        } else {
            self.selected.clone()
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
    pub fn paste(&mut self, settings: &EditSettings, at: u64) -> bool {
        let project = self.doc.borrow().project().clone();
        let Some((origin, mut notes)) = self.notes(&project) else {
            return false;
        };
        if self.clipboard.is_empty() {
            return false;
        }
        let at = settings.snap(&project, at).max(origin) - origin;
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
    pub fn nudge(&mut self, settings: &EditSettings, later: bool) -> bool {
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
            .map_or(MIN_LENGTH, |n| {
                settings.grid_step(&project, origin + n.start)
            })
            .max(1);
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

    /// Moves the selected notes up or down by semitones.
    pub fn transpose(&mut self, semitones: i32) {
        let project = self.doc.borrow().project().clone();
        let Some((_, mut notes)) = self.notes(&project) else {
            return;
        };
        if self.selected.is_empty() {
            return;
        }
        transpose(&mut notes, &self.selected, semitones);
        self.commit(tr("Transpose notes"), notes);
    }

    /// Lengthens the selected notes, or all, up to the next note.
    pub fn legato(&mut self) {
        let project = self.doc.borrow().project().clone();
        let Some((_, mut notes)) = self.notes(&project) else {
            return;
        };
        let chosen = self.chosen(notes.len());
        legato(&mut notes, &chosen);
        self.commit(tr("Legato"), notes);
    }

    /// Varies the timing (a few ms) and velocity of the selected notes, or all, a little.
    pub fn humanize(&mut self) {
        let project = self.doc.borrow().project().clone();
        let Some((_, mut notes)) = self.notes(&project) else {
            return;
        };
        let chosen = self.chosen(notes.len());
        let spread = u64::from(project.sample_rate) / 100;
        let seed = notes.len() as u32 ^ notes.iter().map(|n| n.start as u32).sum::<u32>();
        humanize(&mut notes, &chosen, spread, seed.max(1));
        self.commit(tr("Humanize notes"), notes);
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
        let kept: Vec<Note> = notes
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.selected.contains(i))
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

    /// Quantizes the selected notes, or all of them, to the grid.
    pub fn quantize(&mut self, settings: &EditSettings) {
        let project = self.doc.borrow().project().clone();
        let Some((origin, mut notes)) = self.notes(&project) else {
            return;
        };
        let all = self.selected.is_empty();
        let mut chosen: Vec<Note> = notes
            .iter()
            .enumerate()
            .filter(|(i, _)| all || self.selected.contains(i))
            .map(|(_, n)| *n)
            .collect();
        edit::quantize(
            &mut chosen,
            origin,
            settings.grid.beats(),
            &project.tempo,
            project.sample_rate,
            1.0,
        );
        let mut k = 0;
        for (i, n) in notes.iter_mut().enumerate() {
            if all || self.selected.contains(&i) {
                *n = chosen[k];
                k += 1;
            }
        }
        self.commit(tr("Quantize notes"), notes);
    }

    pub fn show(&mut self, ui: &mut Ui, settings: &EditSettings, clip: Option<ClipId>) {
        if clip != self.clip && self.drag.is_none() {
            self.clip = clip;
            self.selected.clear();
            self.preview = None;
            let project = self.doc.borrow().project().clone();
            if let Some(c) = clip.and_then(|c| edit::clip(&project, c)) {
                self.zoom = (c.length as f64 * 1.1
                    / f64::from((ui.available_width() - KEYS_WIDTH).max(200.0)))
                .max(1.0);
                self.scroll = c.start as f64 - c.length as f64 * 0.05;
                if let Some(notes) = &c.notes
                    && let Some(mid) = notes
                        .iter()
                        .map(|n| u32::from(n.key))
                        .sum::<u32>()
                        .checked_div(notes.len() as u32)
                {
                    self.scroll_y =
                        (127.0 - mid as f32) * self.key_height - ui.available_height() / 2.0;
                }
            }
        }
        let project = self.doc.borrow().project().clone();
        let Some((origin, notes)) = self.notes(&project) else {
            ui.centered_and_justified(|ui| {
                ui.weak(tr("Select a MIDI clip to edit its notes.\nCreate one with Track › New MIDI clip, or by double-clicking an empty part of a track."));
            });
            return;
        };
        let Some(clip) = self.clip.and_then(|c| edit::clip(&project, c)).cloned() else {
            return;
        };

        // Toolbar.
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&clip.name).strong());
            ui.weak(trf!("{count} notes", count = notes.len()));
            ui.separator();
            // The tool, then the selection, then what reshapes the notes.
            ui.toggle_value(&mut self.draw, tr("✏ Draw"))
                .on_hover_text(tr(
                    "Drag to paint a note per grid step, click a note to remove it",
                ));
            ui.separator();
            if ui.button(tr("Select all")).clicked() {
                self.select_all();
            }
            if ui
                .add_enabled(!self.selected.is_empty(), egui::Button::new(tr("Delete")))
                .clicked()
            {
                self.delete_selected();
            }
            ui.separator();
            if ui
                .button(tr("Quantize"))
                .on_hover_text(tr("Selected notes, or all, to the grid"))
                .clicked()
            {
                self.quantize(settings);
            }
            if ui
                .button(tr("Legato"))
                .on_hover_text(tr(
                    "Lengthen the selected notes, or all, up to the next note",
                ))
                .clicked()
            {
                self.legato();
            }
            if ui
                .button(tr("Humanize"))
                .on_hover_text(tr(
                    "Vary the timing and velocity of the selected notes, or all, a little",
                ))
                .clicked()
            {
                self.humanize();
            }
            ui.separator();
            let (root, scale) = self.scale;
            egui::ComboBox::from_id_salt("piano-scale")
                .width(110.0)
                .selected_text(match scale {
                    Some(i) => format!("{} {}", NOTE_NAMES[usize::from(root)], tr(SCALES[i].0)),
                    None => tr("No scale").to_owned(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.scale.1, None, tr("No scale"));
                    for (i, (name, _)) in SCALES.iter().enumerate() {
                        ui.selectable_value(&mut self.scale.1, Some(i), tr(name));
                    }
                })
                .response
                .on_hover_text(tr("Shades the keys outside the scale"));
            if self.scale.1.is_some() {
                egui::ComboBox::from_id_salt("piano-root")
                    .width(44.0)
                    .selected_text(NOTE_NAMES[usize::from(root)])
                    .show_ui(ui, |ui| {
                        for (k, name) in NOTE_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.scale.0, k as u8, *name);
                        }
                    });
            }
            ui.separator();
            let current = lanes()
                .iter()
                .find(|l| l.1 == self.lane)
                .map_or(tr("Velocity"), |l| l.0);
            egui::ComboBox::from_id_salt("piano-lane")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (name, lane) in lanes() {
                        ui.selectable_value(&mut self.lane, lane, name);
                    }
                })
                .response
                .on_hover_text(tr(
                    "What the lane under the notes edits; with notes of one channel selected, \
                     controllers of that channel (MPE per-note expression)",
                ));
            ui.separator();
            let channels: Vec<u8> = self
                .selected
                .iter()
                .filter_map(|&i| notes.get(i).map(|n| n.channel))
                .collect();
            let shown = match channels.first() {
                Some(&c) if channels.iter().all(|&o| o == c) => {
                    trf!("Ch {channel}", channel = c + 1)
                }
                Some(_) => tr("Ch mixed").to_owned(),
                None => tr("Ch").to_owned(),
            };
            let mut chosen = None;
            ui.add_enabled_ui(!channels.is_empty(), |ui| {
                egui::ComboBox::from_id_salt("piano-channel")
                    .selected_text(shown)
                    .width(64.0)
                    .show_ui(ui, |ui| {
                        for c in 0..16u8 {
                            if ui.selectable_label(false, format!("{}", c + 1)).clicked() {
                                chosen = Some(c);
                            }
                        }
                    })
                    .response
                    .on_hover_text(tr("MIDI channel of the selected notes"));
            });
            if let Some(c) = chosen {
                let mut edited = notes.clone();
                for &i in &self.selected {
                    if let Some(n) = edited.get_mut(i) {
                        n.channel = c;
                    }
                }
                self.commit(tr("Note channel"), edited);
            }
            if ui
                .button("MPE")
                .on_hover_text(tr(
                    "Give overlapping notes their own channel (2 to 16) so each can be bent and \
                     shaped alone",
                ))
                .clicked()
            {
                let mut edited = notes.clone();
                edited.sort_by_key(|n| (n.start, n.key));
                edit::spread_mpe_channels(&mut edited);
                self.selected.clear();
                self.commit(tr("Spread notes over MPE channels"), edited);
            }
        });
        let lane_channel = self.lane_channel(&notes);
        let full = ui.available_rect_before_wrap();
        let (_, background) = ui.allocate_exact_size(full.size(), Sense::hover());
        let keys = Rect::from_min_max(
            pos2(full.left(), full.top() + RULER_HEIGHT),
            pos2(full.left() + KEYS_WIDTH, full.bottom() - VELOCITY_HEIGHT),
        );
        let ruler = Rect::from_min_max(
            pos2(full.left() + KEYS_WIDTH, full.top()),
            pos2(full.right(), full.top() + RULER_HEIGHT),
        );
        let area = Rect::from_min_max(
            pos2(full.left() + KEYS_WIDTH, full.top() + RULER_HEIGHT),
            pos2(full.right(), full.bottom() - VELOCITY_HEIGHT),
        );
        let velocity = Rect::from_min_max(
            pos2(full.left() + KEYS_WIDTH, full.bottom() - VELOCITY_HEIGHT),
            full.max,
        );
        let _ = TOOLBAR_HEIGHT;

        // Scrolling and zooming.
        if ui.rect_contains_pointer(full) {
            let (scroll, zoom, modifiers, pointer) = ui.input(|i| {
                (
                    i.smooth_scroll_delta,
                    i.zoom_delta(),
                    i.modifiers,
                    i.pointer.hover_pos(),
                )
            });
            if zoom != 1.0 {
                let x = pointer.map_or(area.center().x, |p| p.x) - area.left();
                let anchor = self.scroll + f64::from(x) * self.zoom;
                self.zoom = (self.zoom / f64::from(zoom)).clamp(0.5, 50_000.0);
                self.scroll = anchor - f64::from(x) * self.zoom;
            } else if modifiers.shift {
                self.scroll -= f64::from(scroll.y + scroll.x) * self.zoom;
            } else {
                self.scroll -= f64::from(scroll.x) * self.zoom;
                self.scroll_y = (self.scroll_y - scroll.y)
                    .clamp(0.0, (128.0 * self.key_height - area.height()).max(0.0));
            }
        }
        let (zoom, scroll, key_height, scroll_y) =
            (self.zoom, self.scroll, self.key_height, self.scroll_y);
        let x_of = |t: u64| area.left() + ((t as f64 - scroll) / zoom) as f32;
        let t_of = |x: f32| (scroll + f64::from(x - area.left()) * zoom).max(0.0) as u64;
        let y_of = |key: u8| area.top() + f32::from(127 - key) * key_height - scroll_y;
        let key_of = |y: f32| {
            (127.0 - ((y - area.top() + scroll_y) / key_height).floor()).clamp(0.0, 127.0) as u8
        };

        let visuals = ui.visuals().clone();
        let painter = ui.painter().with_clip_rect(full);
        painter.rect_filled(full, 0.0, visuals.extreme_bg_color);
        let area_painter = painter.with_clip_rect(area);

        // Rows, beat lines, clip boundaries.
        for key in 0..=127u8 {
            let y = y_of(key);
            if y + self.key_height < area.top() || y > area.bottom() {
                continue;
            }
            let row =
                Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), self.key_height));
            let fill = match self.scale {
                (root, Some(scale)) if !in_scale(key, root, scale) => {
                    visuals.faint_bg_color.gamma_multiply(0.35)
                }
                (root, Some(_)) if key % 12 == root => visuals
                    .faint_bg_color
                    .lerp_to_gamma(color_of_root(&visuals), 0.25),
                _ if is_black(key) => visuals.faint_bg_color.gamma_multiply(0.6),
                _ => visuals.faint_bg_color,
            };
            area_painter.rect_filled(row, 0.0, fill);
            if key % 12 == 0 {
                area_painter.line_segment(
                    [row.left_bottom(), row.right_bottom()],
                    Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.4)),
                );
            }
        }
        let rate = project.sample_rate;
        let tempo = &project.tempo;
        let first = tempo
            .sample_to_beat(t_of(area.left()), rate)
            .floor()
            .max(0.0);
        let last = tempo.sample_to_beat(t_of(area.right()), rate).ceil();
        let step = settings.grid.beats().unwrap_or(4.0).max(0.0625);
        let mut beat = first;
        let mut guard = 0;
        while beat <= last && guard < 4000 {
            guard += 1;
            let x = x_of(tempo.beat_to_sample(beat, rate));
            let bbt = tempo.beat_to_bbt(beat);
            let bar = bbt.beat == 1 && bbt.tick == 0;
            let alpha = if bar {
                0.35
            } else if beat.fract() == 0.0 {
                0.15
            } else {
                0.06
            };
            area_painter.line_segment(
                [pos2(x, area.top()), pos2(x, area.bottom())],
                Stroke::new(1.0, visuals.text_color().gamma_multiply(alpha)),
            );
            if bar {
                painter.text(
                    pos2(x + 3.0, ruler.top() + 3.0),
                    Align2::LEFT_TOP,
                    bbt.bar.to_string(),
                    FontId::monospace(10.0),
                    visuals.text_color(),
                );
            }
            beat += step;
        }
        let shade = Color32::from_black_alpha(90);
        area_painter.rect_filled(
            Rect::from_min_max(area.min, pos2(x_of(clip.start), area.bottom())),
            0.0,
            shade,
        );
        area_painter.rect_filled(
            Rect::from_min_max(pos2(x_of(clip.end()), area.top()), area.max),
            0.0,
            shade,
        );

        // Keyboard.
        let key_painter = painter.with_clip_rect(keys);
        for key in 0..=127u8 {
            let y = y_of(key) - area.top() + keys.top();
            let r = Rect::from_min_size(
                pos2(keys.left(), y),
                vec2(KEYS_WIDTH - 2.0, self.key_height - 1.0),
            );
            key_painter.rect_filled(
                r,
                1.0,
                if is_black(key) {
                    Color32::from_gray(30)
                } else {
                    Color32::from_gray(210)
                },
            );
            if key % 12 == 0 {
                key_painter.text(
                    pos2(r.right() - 3.0, r.center().y),
                    Align2::RIGHT_CENTER,
                    key_name(key),
                    FontId::proportional(9.0),
                    Color32::from_gray(40),
                );
            }
        }

        // Notes and velocity stems.
        let color = visuals.selection.bg_fill;
        let vel_painter = painter.with_clip_rect(velocity);
        vel_painter.rect_filled(velocity, 0.0, visuals.panel_fill);
        let note_rect = |n: &Note| {
            let x0 = x_of(origin + n.start);
            let x1 = x_of(origin + n.start + n.length).max(x0 + 3.0);
            Rect::from_min_max(
                pos2(x0, y_of(n.key)),
                pos2(x1, y_of(n.key) + key_height - 1.0),
            )
        };
        for (i, n) in notes.iter().enumerate() {
            let r = note_rect(n);
            let selected = self.selected.contains(&i);
            let fill = color.gamma_multiply(0.45 + 0.55 * f32::from(n.velocity) / 127.0);
            area_painter.rect_filled(r, 2.0, fill);
            area_painter.rect_stroke(
                r,
                2.0,
                Stroke::new(
                    if selected { 2.0 } else { 1.0 },
                    if selected {
                        visuals.strong_text_color()
                    } else {
                        color
                    },
                ),
                StrokeKind::Inside,
            );
            if self.lane.is_some() {
                continue;
            }
            let x = r.left();
            let h = (velocity.height() - 8.0) * f32::from(n.velocity) / 127.0;
            vel_painter.line_segment(
                [
                    pos2(x, velocity.bottom() - 4.0),
                    pos2(x, velocity.bottom() - 4.0 - h),
                ],
                Stroke::new(if selected { 3.0 } else { 2.0 }, color),
            );
            vel_painter.circle_filled(pos2(x, velocity.bottom() - 4.0 - h), 3.0, color);
        }
        let value_y =
            |v: f32| velocity.bottom() - 4.0 - (velocity.height() - 8.0) * v.clamp(0.0, 1.0);
        let value_of =
            |y: f32| ((velocity.bottom() - 4.0 - y) / (velocity.height() - 8.0)).clamp(0.0, 1.0);
        let points: Vec<ControlPoint> = match &self.drawing {
            Some((points, _)) => points.clone(),
            None => clip
                .controls
                .as_ref()
                .map(|c| (**c).clone())
                .unwrap_or_default(),
        };
        if let Some(controller) = self.lane {
            if controller == PITCH_BEND {
                let y = value_y(0.5);
                vel_painter.line_segment(
                    [pos2(velocity.left(), y), pos2(velocity.right(), y)],
                    Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.4)),
                );
            }
            let mine: Vec<&ControlPoint> = points
                .iter()
                .filter(|p| p.controller == controller && p.channel == lane_channel)
                .collect();
            for (i, p) in mine.iter().enumerate() {
                let x = x_of(origin + p.time);
                let next = mine
                    .get(i + 1)
                    .map_or(velocity.right(), |n| x_of(origin + n.time));
                let y = value_y(p.value);
                vel_painter.line_segment([pos2(x, y), pos2(next, y)], Stroke::new(2.0, color));
                if let Some(n) = mine.get(i + 1) {
                    vel_painter.line_segment(
                        [pos2(next, y), pos2(next, value_y(n.value))],
                        Stroke::new(1.0, color.gamma_multiply(0.6)),
                    );
                }
                vel_painter.circle_filled(pos2(x, y), 3.0, color);
            }
        }
        let lane_name = lanes()
            .iter()
            .find(|l| l.1 == self.lane)
            .map_or(tr("Velocity"), |l| l.0);
        painter.text(
            pos2(velocity.left() + 4.0, velocity.top() + 2.0),
            Align2::LEFT_TOP,
            if self.lane.is_some() && lane_channel != 0 {
                trf!(
                    "{lane}, channel {channel}",
                    lane = lane_name,
                    channel = lane_channel + 1
                )
            } else {
                lane_name.to_owned()
            },
            FontId::proportional(10.0),
            visuals.weak_text_color(),
        );

        // Interaction.
        let response = ui.interact(
            area,
            ui.scope_id().with("piano-notes"),
            Sense::click_and_drag(),
        );
        let vel_response = ui.interact(
            velocity,
            ui.scope_id().with("piano-velocity"),
            Sense::click_and_drag(),
        );
        let _ = background;
        if ui.input(|i| i.pointer.any_pressed()) {
            self.focused = ui.rect_contains_pointer(full);
        }
        let modifiers = ui.input(|i| i.modifiers);
        let hit = |pos: Pos2| -> Option<(usize, bool)> {
            notes.iter().enumerate().rev().find_map(|(i, n)| {
                let r = note_rect(n);
                r.contains(pos).then_some((i, pos.x > r.right() - EDGE))
            })
        };
        if self.drag.is_none()
            && let Some(pos) = response.hover_pos()
        {
            ui.ctx().set_cursor_icon(match hit(pos) {
                Some((_, true)) => CursorIcon::ResizeHorizontal,
                Some(_) => CursorIcon::Grab,
                None => CursorIcon::Crosshair,
            });
        }
        // A drag starts a few pixels away from the press: what it does depends on where the
        // button went down (else pulling a note's edge would start a rubber band).
        let press = ui.input(|i| i.pointer.press_origin());
        if response.drag_started()
            && let Some(pos) = press.or(response.interact_pointer_pos())
        {
            let mut original = notes.clone();
            let mut copy = false;
            let kind = match hit(pos) {
                Some((i, edge)) => {
                    if !self.selected.contains(&i) {
                        if !modifiers.shift {
                            self.selected.clear();
                        }
                        self.selected.push(i);
                    }
                    if edge {
                        DragKind::Resize
                    } else {
                        if modifiers.alt {
                            // Alt-drag moves copies and leaves the notes where they are.
                            let first = original.len();
                            original.extend(self.selected.iter().filter_map(|&i| notes.get(i)));
                            self.selected = (first..original.len()).collect();
                            copy = true;
                        }
                        DragKind::Move
                    }
                }
                None if self.draw => DragKind::Paint,
                None => DragKind::Select { anchor: pos },
            };
            self.drag = Some(Drag {
                kind,
                origin: pos,
                original,
                painted: Vec::new(),
                copy,
            });
        }
        // Controller lane: click adds a point, drag draws, double-click removes.
        if let Some(controller) = self.lane {
            let nearest = |pos: Pos2| {
                points.iter().position(|p| {
                    p.controller == controller
                        && p.channel == lane_channel
                        && (x_of(origin + p.time) - pos.x).abs() < 6.0
                        && (value_y(p.value) - pos.y).abs() < 8.0
                })
            };
            let pointer = vel_response.interact_pointer_pos();
            if vel_response.drag_started()
                && let Some(pos) = pointer
            {
                self.drawing = Some((points.clone(), t_of(pos.x).saturating_sub(origin)));
            }
            if let (Some((drawn, last)), Some(pos)) = (&mut self.drawing, pointer)
                && vel_response.dragged()
            {
                let t = t_of(pos.x).saturating_sub(origin);
                let (a, b) = ((*last).min(t), (*last).max(t));
                drawn.retain(|p| {
                    p.controller != controller
                        || p.channel != lane_channel
                        || p.time < a
                        || p.time > b
                });
                drawn.push(ControlPoint {
                    time: t,
                    controller,
                    value: value_of(pos.y),
                    channel: lane_channel,
                });
                drawn.sort_by_key(|p| (p.time, p.controller));
                *last = t;
            }
            if vel_response.drag_stopped()
                && let Some((drawn, _)) = self.drawing.take()
                && let Some(id) = self.clip
            {
                self.doc
                    .borrow_mut()
                    .history_mut()
                    .edit(tr("Draw controller"), |p| edit::set_controls(p, id, drawn));
            }
            if vel_response.double_clicked()
                && let Some(i) = pointer.and_then(nearest)
                && let Some(id) = self.clip
            {
                let mut edited = points.clone();
                edited.remove(i);
                self.doc
                    .borrow_mut()
                    .history_mut()
                    .edit(tr("Delete controller point"), |p| {
                        edit::set_controls(p, id, edited)
                    });
            } else if vel_response.clicked()
                && let Some(pos) = pointer
                && let Some(id) = self.clip
            {
                let t = settings.snap(&project, t_of(pos.x)).saturating_sub(origin);
                let mut edited = points.clone();
                edited.retain(|p| {
                    p.controller != controller || p.channel != lane_channel || p.time != t
                });
                edited.push(ControlPoint {
                    time: t,
                    controller,
                    value: value_of(pos.y),
                    channel: lane_channel,
                });
                self.doc
                    .borrow_mut()
                    .history_mut()
                    .edit(tr("Add controller point"), |p| {
                        edit::set_controls(p, id, edited)
                    });
            }
        } else if vel_response.drag_started()
            && let Some(pos) = vel_response.interact_pointer_pos()
        {
            if self.selected.is_empty()
                && let Some(i) = notes
                    .iter()
                    .position(|n| (x_of(origin + n.start) - pos.x).abs() < 6.0)
            {
                self.selected = vec![i];
            }
            self.drag = Some(Drag {
                kind: DragKind::Velocity,
                origin: pos,
                original: notes.clone(),
                painted: Vec::new(),
                copy: false,
            });
        }
        if let (Some(drag), Some(pos)) = (&mut self.drag, ui.input(|i| i.pointer.interact_pos())) {
            let mut edited = drag.original.clone();
            let delta = pos - drag.origin;
            match drag.kind {
                DragKind::Move => {
                    let samples = (f64::from(delta.x) * self.zoom) as i64;
                    let keys = -(delta.y / self.key_height).round() as i32;
                    let anchor = self
                        .selected
                        .first()
                        .and_then(|&i| drag.original.get(i))
                        .map_or(0, |n| origin + n.start);
                    let snapped = settings.snap(&project, (anchor as i64 + samples).max(0) as u64)
                        as i64
                        - anchor as i64;
                    for &i in &self.selected {
                        if let Some(n) = edited.get_mut(i) {
                            n.start = (n.start as i64 + snapped).max(0) as u64;
                            n.key = (i32::from(n.key) + keys).clamp(0, 127) as u8;
                        }
                    }
                }
                DragKind::Resize => {
                    let samples = (f64::from(delta.x) * self.zoom) as i64;
                    for &i in &self.selected {
                        if let Some(n) = edited.get_mut(i) {
                            let end = settings.snap(
                                &project,
                                (origin as i64 + (n.start + n.length) as i64 + samples).max(0)
                                    as u64,
                            );
                            n.length = end.saturating_sub(origin + n.start).max(MIN_LENGTH);
                        }
                    }
                }
                DragKind::Velocity => {
                    let value = ((velocity.bottom() - 4.0 - pos.y) / (velocity.height() - 8.0)
                        * 127.0)
                        .clamp(1.0, 127.0) as u8;
                    for &i in &self.selected {
                        if let Some(n) = edited.get_mut(i) {
                            n.velocity = value;
                        }
                    }
                }
                DragKind::Paint => {
                    let at = settings.snap(&project, t_of(pos.x)).max(origin);
                    let length = settings.grid_step(&project, at).max(MIN_LENGTH);
                    let key = key_of(pos.y);
                    let start = at - origin;
                    let taken = drag.painted.iter().any(|n| n.start == start)
                        || drag
                            .original
                            .iter()
                            .any(|n| n.start == start && n.key == key);
                    if !taken {
                        drag.painted.push(Note {
                            start,
                            length,
                            key,
                            velocity: 100,
                            channel: 0,
                        });
                    }
                    edited.extend(drag.painted.iter().copied());
                }
                DragKind::Select { anchor } => {
                    let r = Rect::from_two_pos(anchor, pos);
                    ui.painter()
                        .rect_stroke(r, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
                    self.selected = drag
                        .original
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| note_rect(n).intersects(r))
                        .map(|(i, _)| i)
                        .collect();
                }
            }
            if !matches!(drag.kind, DragKind::Select { .. }) {
                self.preview = Some(edited);
            }
        }
        if (response.drag_stopped() || vel_response.drag_stopped())
            && let Some(drag) = self.drag.take()
            && let Some(edited) = self.preview.take()
        {
            if drag.kind == DragKind::Resize
                && let Some(n) = self.selected.first().and_then(|&i| edited.get(i))
            {
                self.last_length = Some(n.length);
            }
            let label = match drag.kind {
                DragKind::Move if drag.copy => tr("Copy notes"),
                DragKind::Move => tr("Move notes"),
                DragKind::Resize => tr("Resize notes"),
                DragKind::Paint => tr("Draw notes"),
                _ => tr("Note velocity"),
            };
            if drag.kind == DragKind::Paint {
                let first = edited.len() - drag.painted.len();
                self.selected = (first..edited.len()).collect();
            }
            self.commit(label, edited);
        }
        if (response.double_clicked() || (self.draw && response.clicked()))
            && let Some((i, _)) = response.interact_pointer_pos().and_then(hit)
        {
            self.selected = vec![i];
            self.delete_selected();
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            match hit(pos) {
                Some((i, _)) => {
                    if modifiers.shift {
                        if let Some(k) = self.selected.iter().position(|&s| s == i) {
                            self.selected.remove(k);
                        } else {
                            self.selected.push(i);
                        }
                    } else {
                        self.selected = vec![i];
                    }
                }
                None => {
                    let at = settings.snap(&project, t_of(pos.x)).max(origin);
                    let length = self
                        .last_length
                        .unwrap_or_else(|| settings.grid_step(&project, at).max(MIN_LENGTH));
                    let mut edited = notes.clone();
                    edited.push(Note {
                        start: at - origin,
                        length,
                        key: key_of(pos.y),
                        velocity: 100,
                        channel: 0,
                    });
                    self.selected = vec![edited.len() - 1];
                    self.added = Some((key_of(pos.y), ui.input(|i| i.time) + AUDITION));
                    self.commit(tr("Add note"), edited);
                }
            }
        }

        // Up and down transpose the selection, by an octave with Shift.
        if self.focused && !self.selected.is_empty() && !ui.ctx().text_edit_focused() {
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

        // Sounding: the key pressed on the keyboard, the note dragged or painted, a note just
        // added.
        let keys_response = ui.interact(
            keys,
            ui.scope_id().with("piano-keys"),
            Sense::click_and_drag(),
        );
        let now = ui.input(|i| i.time);
        let wanted = if keys_response.is_pointer_button_down_on() {
            keys_response.interact_pointer_pos().map(|p| key_of(p.y))
        } else {
            match self.drag.as_ref().map(|d| d.kind) {
                Some(DragKind::Move) => self
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
        let track = edit::track_of_clip(&project, clip.id);
        if wanted != self.sounding.map(|(_, k)| k) {
            let doc = self.doc.borrow();
            if let Some((t, k)) = self.sounding.take() {
                doc.player().play_key(t, [0x80, k, 0]);
            }
            if let (Some(k), Some(t)) = (wanted, track) {
                doc.player().play_key(t, [0x90, k, 100]);
                self.sounding = Some((t, k));
            }
        }
        if self.sounding.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;

    fn note(start: u64, key: u8, channel: u8) -> Note {
        Note {
            start,
            length: 1000,
            key,
            velocity: 100,
            channel,
        }
    }

    fn roll(notes: Vec<Note>) -> (PianoRoll, Rc<RefCell<Document>>, ClipId) {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Synth");
        let clip = p.add_midi_clip(t, 0, 96_000, notes).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().replace(p, false);
        let doc = Rc::new(RefCell::new(doc));
        let mut roll = PianoRoll::new(Rc::clone(&doc));
        roll.clip = Some(clip);
        (roll, doc, clip)
    }

    fn keys(doc: &Rc<RefCell<Document>>, clip: ClipId) -> Vec<(u64, u8)> {
        let doc = doc.borrow();
        let notes = edit::clip(doc.project(), clip)
            .unwrap()
            .notes
            .clone()
            .unwrap();
        notes.iter().map(|n| (n.start, n.key)).collect()
    }

    #[test]
    fn selected_notes_are_deleted_and_quantized() {
        let (mut roll, doc, clip) = roll(vec![note(1000, 60, 0), note(23_000, 64, 1)]);
        assert!(!roll.delete_selected(), "nothing selected");
        roll.select_all();
        assert_eq!(roll.selected, [0, 1]);
        let pair = [note(0, 60, 0), note(0, 64, 1)];
        assert_eq!(roll.lane_channel(&pair), 0, "mixed channels");
        roll.selected = vec![1];
        assert_eq!(roll.lane_channel(&pair), 1);

        roll.quantize(&EditSettings::default());
        assert_eq!(
            keys(&doc, clip),
            [(1000, 60), (24_000, 64)],
            "only the selection"
        );
        assert_eq!(roll.selected, [1], "the selection follows the note");
        roll.selected.clear();
        roll.quantize(&EditSettings::default());
        assert_eq!(keys(&doc, clip), [(0, 60), (24_000, 64)]);
        assert_eq!(
            doc.borrow().history().undo_label(),
            Some(tr("Quantize notes"))
        );

        roll.selected = vec![0];
        assert!(roll.delete_selected());
        assert_eq!(keys(&doc, clip), [(24_000, 64)]);
        assert!(roll.selected.is_empty());
        roll.reset();
        assert!(roll.notes(doc.borrow().project()).is_none());
    }

    /// Draws the roll for one frame with input events, in the root Ui (the keys start at x = 0).
    fn frame(roll: &mut PianoRoll, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let clip = roll.clip;
        ctx.run_ui(input, |ui| roll.show(ui, &EditSettings::default(), clip))
            .drop_without_applying_deltas();
    }

    #[test]
    fn dragging_a_notes_right_edge_resizes_it() {
        use egui::{Event, Modifiers, PointerButton};
        let (mut roll, doc, clip) = roll(vec![Note {
            length: 24_000,
            ..note(0, 60, 0)
        }]);
        let ctx = egui::Context::default();
        frame(&mut roll, &ctx, vec![]);
        // 100 samples a pixel from the clip start; key 60 a tall row around y = 100.
        (roll.zoom, roll.scroll, roll.key_height) = (100.0, 0.0, 200.0);
        roll.scroll_y = 67.0 * 200.0 + 100.0;
        let edge = pos2(KEYS_WIDTH + 240.0 - 3.0, 100.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(&mut roll, &ctx, vec![Event::PointerMoved(edge)]);
        frame(&mut roll, &ctx, vec![button(edge, true)]);
        for x in [10.0, 40.0, 107.0] {
            frame(
                &mut roll,
                &ctx,
                vec![Event::PointerMoved(edge + vec2(x, 0.0))],
            );
        }
        frame(
            &mut roll,
            &ctx,
            vec![button(edge + vec2(107.0, 0.0), false)],
        );
        frame(&mut roll, &ctx, vec![]);
        let doc = doc.borrow();
        let notes = edit::clip(doc.project(), clip)
            .unwrap()
            .notes
            .clone()
            .unwrap();
        assert_eq!(notes.len(), 1, "no rubber band, no new note");
        assert!(notes[0].length > 30_000, "resized to {}", notes[0].length);
        assert_eq!(doc.history().undo_label(), Some(tr("Resize notes")));
    }

    #[test]
    fn copy_paste_duplicate_nudge_and_transpose() {
        let (mut roll, doc, clip) = roll(vec![note(0, 60, 0), note(12_000, 64, 0)]);
        assert!(!roll.copy(), "nothing selected");
        roll.selected = vec![0, 1];
        assert!(roll.copy());
        // 120 BPM: a beat is 24 000 samples; pasted at the second bar, the clip grows.
        assert!(roll.paste(&EditSettings::default(), 96_000));
        assert_eq!(
            keys(&doc, clip),
            [(0, 60), (12_000, 64), (96_000, 60), (108_000, 64)]
        );
        assert_eq!(roll.selected, [2, 3], "the pasted notes are selected");
        assert_eq!(
            edit::clip(doc.borrow().project(), clip).unwrap().length,
            192_000
        );
        roll.selected = vec![0, 1];
        assert!(roll.duplicate());
        assert!(keys(&doc, clip).contains(&(24_000, 60)), "a beat later");
        roll.selected = vec![0];
        assert!(roll.nudge(&EditSettings::default(), true));
        assert!(!keys(&doc, clip).contains(&(0, 60)));
        roll.transpose(12);
        assert!(keys(&doc, clip).iter().any(|&(_, k)| k == 72));
        roll.selected = vec![0];
        assert!(roll.cut());
        assert_eq!(roll.clipboard.len(), 1);
    }

    #[test]
    fn legato_fills_the_gaps_and_humanize_stays_close() {
        let mut notes = vec![note(0, 60, 0), note(5000, 62, 0), note(9000, 64, 0)];
        legato(&mut notes, &[0, 1, 2]);
        let lengths: Vec<u64> = notes.iter().map(|n| n.length).collect();
        assert_eq!(lengths, [5000, 4000, 1000], "the last keeps its length");
        let before = notes.clone();
        humanize(&mut notes, &[0, 1, 2], 480, 7);
        for (a, b) in before.iter().zip(&notes) {
            assert!(a.start.abs_diff(b.start) <= 480);
            assert!(a.velocity.abs_diff(b.velocity) <= 10);
        }
        let mut again = before.clone();
        humanize(&mut again, &[0, 1, 2], 480, 7);
        assert_eq!(again, notes, "repeatable");
        transpose(&mut notes, &[0], -100);
        assert_eq!(notes[0].key, 0);
    }

    #[test]
    fn scales_follow_their_root() {
        let c_major: Vec<u8> = (60..72).filter(|&k| in_scale(k, 0, 0)).collect();
        assert_eq!(c_major, [60, 62, 64, 65, 67, 69, 71]);
        let a_minor: Vec<u8> = (57..69).filter(|&k| in_scale(k, 9, 1)).collect();
        assert_eq!(a_minor, [57, 59, 60, 62, 64, 65, 67]);
    }

    #[test]
    fn black_keys() {
        let black: Vec<u8> = (60..72).filter(|&k| is_black(k)).collect();
        assert_eq!(black, [61, 63, 66, 68, 70]);
    }
}
