//! The piano roll's toolbar, its context menu, and the transform panel that previews a
//! transformation live on the notes before it is applied.

use audiostrada_core::Note;
use eframe::egui::{self, Color32, Painter, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};

use super::music::{ChordKind, NOTE_NAMES, SCALES, Scale, Voicing};
use super::ops::{self, Arpeggio, Strum, Timing};
use super::{Grid, Pending, PianoRoll, Scene, Stamp, Tool};
use crate::i18n::tr;
use crate::theme;
use crate::trf;

/// A transformation and its settings.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Transform {
    Quantize(ops::Quantize),
    Humanize {
        timing: f64,
        velocity: f64,
        length: f64,
        seed: u32,
    },
    Strum {
        ms: f64,
        direction: Strum,
    },
    Arpeggiate(ops::Arpeggiate),
    Harmonize {
        steps: i32,
    },
    Chordify {
        size: usize,
        voicing: Voicing,
    },
    Vary(ops::Vary),
    Randomize {
        velocity: f64,
        probability: u8,
        seed: u32,
    },
    Velocity {
        from: u8,
        to: u8,
        curve: f64,
        contrast: f64,
    },
    Stretch(f64),
}

impl Transform {
    pub fn name(&self) -> &'static str {
        match self {
            Transform::Quantize(_) => tr("Quantize"),
            Transform::Humanize { .. } => tr("Humanize"),
            Transform::Strum { .. } => tr("Strum"),
            Transform::Arpeggiate(_) => tr("Arpeggiate"),
            Transform::Harmonize { .. } => tr("Harmonize"),
            Transform::Chordify { .. } => tr("Chords from notes"),
            Transform::Vary(_) => tr("Variation"),
            Transform::Randomize { .. } => tr("Randomize"),
            Transform::Velocity { .. } => tr("Velocity shape"),
            Transform::Stretch(_) => tr("Stretch"),
        }
    }

    pub fn help(&self) -> &'static str {
        match self {
            Transform::Quantize(_) => tr("To the grid, with a strength and a swing"),
            Transform::Humanize { .. } => tr("Small variations of timing, velocity and length"),
            Transform::Strum { .. } => tr("Spreads the chords like a guitar strum"),
            Transform::Arpeggiate(_) => tr("Plays the chords one note after another"),
            Transform::Harmonize { .. } => {
                tr("Adds a second voice, in the scale when there is one")
            }
            Transform::Chordify { .. } => {
                tr("Turns each note into the chord the scale builds on it")
            }
            Transform::Vary(_) => tr("A new take on the rhythm and the melody"),
            Transform::Randomize { .. } => tr("Random velocities and chances of playing"),
            Transform::Velocity { .. } => tr("A ramp of velocities, more or less contrasted"),
            Transform::Stretch(_) => tr("Longer or shorter, from the first note"),
        }
    }

    /// Each transformation as offered, from the current grid.
    pub fn all(grid: f64) -> Vec<Transform> {
        vec![
            Transform::Quantize(ops::Quantize {
                step: grid,
                strength: 1.0,
                swing: 0.0,
                ends: false,
            }),
            Transform::Humanize {
                timing: 10.0,
                velocity: 10.0,
                length: 0.1,
                seed: 1,
            },
            Transform::Strum {
                ms: 25.0,
                direction: Strum::Up,
            },
            Transform::Arpeggiate(ops::Arpeggiate {
                pattern: Arpeggio::Up,
                step: grid,
                gate: 0.8,
                octaves: 1,
                seed: 1,
            }),
            Transform::Harmonize { steps: 2 },
            Transform::Chordify {
                size: 3,
                voicing: Voicing::Close,
            },
            Transform::Vary(ops::Vary {
                rhythm: 0.3,
                melody: 0.3,
                step: grid,
                seed: 1,
            }),
            Transform::Randomize {
                velocity: 20.0,
                probability: 100,
                seed: 1,
            },
            Transform::Velocity {
                from: 50,
                to: 120,
                curve: 1.0,
                contrast: 1.0,
            },
            Transform::Stretch(2.0),
        ]
    }
}

/// The notes `transform` makes from `base`, and the selection after it.
/// [`apply`] on each part alone, so that every note stays in its clip.
pub(super) fn apply_parts(
    pending: &Pending,
    timing: Timing,
    scale: Option<Scale>,
) -> super::Changed {
    super::per_part(
        &pending.base,
        &pending.owner,
        &pending.chosen,
        |notes, chosen| {
            let (out, selection) = apply(&pending.transform, notes, chosen, timing, scale);
            *notes = out;
            Some(selection)
        },
    )
}

pub(super) fn apply(
    transform: &Transform,
    base: &[Note],
    chosen: &[usize],
    timing: Timing,
    scale: Option<Scale>,
) -> (Vec<Note>, Vec<usize>) {
    let mut notes = base.to_vec();
    let mut selection = chosen.to_vec();
    let ms = |m: f64| (f64::from(timing.rate) * m / 1000.0) as u64;
    match transform {
        Transform::Quantize(q) => ops::quantize(&mut notes, chosen, timing, q),
        Transform::Humanize {
            timing: t,
            velocity,
            length,
            seed,
        } => ops::humanize(&mut notes, chosen, ms(*t), *velocity, *length, *seed),
        Transform::Strum { ms: m, direction } => {
            ops::strum(&mut notes, chosen, ms(*m), *direction, ms(30.0))
        }
        Transform::Arpeggiate(a) => {
            let (out, created) = ops::arpeggiate(&notes, chosen, timing, a);
            notes = out;
            selection = created;
        }
        Transform::Harmonize { steps } => {
            let added = ops::harmonize(&mut notes, chosen, scale, *steps);
            selection.extend(added);
        }
        Transform::Chordify { size, voicing } => {
            let added = ops::chordify(&mut notes, chosen, scale, *size, *voicing);
            selection.extend(added);
        }
        Transform::Vary(v) => ops::vary(&mut notes, chosen, timing, scale, v),
        Transform::Randomize {
            velocity,
            probability,
            seed,
        } => ops::randomize(&mut notes, chosen, *velocity, *probability, *seed),
        Transform::Velocity {
            from,
            to,
            curve,
            contrast,
        } => {
            ops::velocity(&mut notes, chosen, ops::Velocity::Ramp(*from, *to, *curve));
            ops::velocity(&mut notes, chosen, ops::Velocity::Contrast(*contrast));
        }
        Transform::Stretch(f) => ops::stretch(&mut notes, chosen, *f),
    }
    (notes, selection)
}

/// The glyph of a tool.
pub(super) fn paint_tool(painter: &Painter, rect: Rect, tool: Tool, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5;
    let stroke = Stroke::new(1.5, color);
    match tool {
        Tool::Pointer => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - s * 0.55, c.y - s * 0.9),
                    pos2(c.x + s * 0.6, c.y + s * 0.15),
                    pos2(c.x + s * 0.05, c.y + s * 0.2),
                    pos2(c.x - s * 0.25, c.y + s * 0.85),
                    pos2(c.x - s * 0.55, c.y + s * 0.1),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Tool::Draw => {
            let a = pos2(c.x + s * 0.7, c.y - s * 0.7);
            let b = pos2(c.x - s * 0.55, c.y + s * 0.55);
            painter.line_segment([a, b], Stroke::new(3.0, color));
            painter.add(Shape::convex_polygon(
                vec![
                    b + vec2(-0.25 * s, 0.25 * s) * 1.4,
                    b + vec2(0.25 * s, 0.05 * s),
                    b + vec2(-0.05 * s, -0.25 * s),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Tool::Paint => {
            painter.line_segment(
                [
                    pos2(c.x + s * 0.8, c.y - s * 0.8),
                    pos2(c.x - s * 0.05, c.y + s * 0.05),
                ],
                Stroke::new(2.0, color),
            );
            painter.circle_filled(pos2(c.x - s * 0.35, c.y + s * 0.4), s * 0.42, color);
        }
        Tool::Slice => {
            for dy in [-0.45, 0.45] {
                painter.circle_stroke(pos2(c.x - s * 0.55, c.y + dy * s), s * 0.28, stroke);
            }
            painter.line_segment(
                [
                    pos2(c.x - s * 0.3, c.y - s * 0.3),
                    pos2(c.x + s * 0.85, c.y + s * 0.55),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(c.x - s * 0.3, c.y + s * 0.3),
                    pos2(c.x + s * 0.85, c.y - s * 0.55),
                ],
                stroke,
            );
        }
        Tool::Zoom => {
            painter.circle_stroke(pos2(c.x - s * 0.2, c.y - s * 0.2), s * 0.55, stroke);
            painter.line_segment(
                [
                    pos2(c.x + s * 0.2, c.y + s * 0.2),
                    pos2(c.x + s * 0.8, c.y + s * 0.8),
                ],
                Stroke::new(2.0, color),
            );
        }
        Tool::Erase => {
            let body = vec![
                pos2(c.x - s * 0.9, c.y + s * 0.2),
                pos2(c.x - s * 0.1, c.y - s * 0.6),
                pos2(c.x + s * 0.8, c.y + s * 0.2),
                pos2(c.x, c.y + s * 0.95),
            ];
            painter.add(Shape::closed_line(body, stroke));
            painter.line_segment(
                [
                    pos2(c.x - s * 0.5, c.y - s * 0.2),
                    pos2(c.x + s * 0.4, c.y + s * 0.6),
                ],
                stroke,
            );
        }
    }
}

/// A flat icon button of the toolbar; lit when `on`.
fn glyph_button(
    ui: &mut Ui,
    on: bool,
    hint: &str,
    paint: impl FnOnce(&Painter, Rect, Color32),
) -> egui::Response {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 24.0), Sense::click());
    let fill = if on {
        p.accent.gamma_multiply(0.28)
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    if on {
        ui.painter().rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0, p.accent.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
    }
    let color = if on || response.hovered() {
        p.text
    } else {
        p.text.gamma_multiply(0.7)
    };
    paint(ui.painter(), rect.shrink2(vec2(7.0, 5.0)), color);
    response.on_hover_text(hint)
}

/// A chip opening a popup, with a label.
fn chip(ui: &mut Ui, text: &str, on: bool, hint: &str, add: impl FnOnce(&mut Ui)) {
    let p = theme::current(ui);
    let label = RichText::new(format!("{text}  ⏷")).size(11.5);
    let button = egui::Button::new(if on {
        label.color(p.text)
    } else {
        label.color(p.text.gamma_multiply(0.8))
    })
    .fill(if on {
        p.accent.gamma_multiply(0.18)
    } else {
        p.surface
    })
    .corner_radius(6.0);
    let response = ui.add(button).on_hover_text(hint);
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(200.0);
        add(ui);
    });
}

fn separator(ui: &mut Ui) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 20.0), Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1.0, p.border),
    );
}

impl PianoRoll {
    pub(super) fn toolbar(&mut self, ui: &mut Ui, bar: Rect, scene: &Scene) {
        let p = theme::current(ui);
        ui.painter().rect_filled(bar, 0.0, p.panel);
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            Stroke::new(1.0, p.border),
        );
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(8.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut ui;
        ui.spacing_mut().item_spacing.x = 4.0;
        // The clip.
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.5, scene.color);
        ui.label(RichText::new(&scene.clip.name).strong().size(12.5));
        ui.label(
            RichText::new(self.summary(&scene.notes))
                .size(11.0)
                .color(p.weak),
        );
        separator(ui);
        // Tools.
        for (k, tool) in Tool::ALL.into_iter().enumerate() {
            let hint = format!("{} ({})\n{}", tool.name(), k + 1, tool.help());
            if glyph_button(ui, self.tool == tool, &hint, |painter, r, c| {
                paint_tool(painter, r, tool, c)
            })
            .clicked()
            {
                self.tool = tool;
            }
        }
        separator(ui);
        self.stamp_controls(ui);
        separator(ui);
        let p = theme::current(ui);
        if theme::labeled_toggle(
            ui,
            theme::Icon::Snap,
            tr("Snap"),
            self.snap,
            p.accent,
            tr("Notes snap to the grid; hold Ctrl to place freely"),
        )
        .clicked()
        {
            self.snap = !self.snap;
        }
        self.grid_chip(ui);
        self.scale_chip(ui, scene);

        // On the right: transforms, ghosts, framing.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            self.fit_button(ui);
            self.ghosts_button(ui);
            self.transform_menu(ui, scene);
        });
    }

    /// The note editor's bar, laid out as the automation editor's: the tools, what applies to
    /// the selected notes, the grids of time and pitch, the view. The chord the pencil adds
    /// sits by the tools.
    pub(super) fn editor_toolbar(&mut self, ui: &mut Ui, scene: &Scene) {
        use crate::editor_frame::{self, Glyph, ViewButton, button};
        use editor_frame::Tool as Shared;
        let tools = [
            (
                Shared::Pencil,
                tr("Pencil: click adds a note, drag right sets its length; drag a note to move it"),
            ),
            (
                Shared::Select,
                tr(
                    "Selection: click a note or drag a rectangle; drag what is selected to move \
                    it (Alt: copy), an edge to resize it",
                ),
            ),
            (
                Shared::Brush,
                tr("Brush: drag to paint a note on each grid step"),
            ),
            (
                Shared::Scissors,
                tr("Scissors: click a note to cut it there"),
            ),
            (
                Shared::Erase,
                tr("Eraser: click a note or drag over notes to remove them"),
            ),
        ];
        let current = self.tool.shared();
        let mut view = None;
        let picked = editor_frame::tool_bar(ui, editor_frame::NOTES, &tools, current, |ui| {
            self.chord_chip(ui);
            editor_frame::separator(ui);
            self.transform_menu(ui, scene);
            let some = !self.selected.is_empty();
            if ui
                .add_enabled_ui(some, |ui| {
                    button(ui, Glyph::Delete, false, tr("Delete the selected notes"))
                })
                .inner
                .clicked()
            {
                self.delete_selected();
            }
            editor_frame::separator(ui);
            if editor_frame::snap_toggle(
                ui,
                self.snap,
                tr("Notes snap to the grid; hold Ctrl to place freely"),
            ) {
                self.snap = !self.snap;
            }
            self.grid_chip(ui);
            self.scale_chip(ui, scene);
            editor_frame::separator(ui);
            view = editor_frame::view_buttons(ui, current, tr("Frame the notes"));
        });
        if let Some(tool) = picked {
            self.tool = Tool::of(tool);
        }
        match view {
            Some(ViewButton::ZoomTool) => self.tool = Tool::Zoom,
            Some(ViewButton::ZoomOut) => self.view.zoom_time(0.5, self.area.width() / 2.0),
            Some(ViewButton::Fit) => self.fit = true,
            None => {}
        }
    }

    /// The note editor's tracks: the clips edited, the active one marked (a click makes a
    /// clip active), a cross to take one away, and the other MIDI tracks to add.
    pub(super) fn parts_list(&mut self, ui: &mut Ui, scene: &Scene, rect: Rect) {
        let p = theme::current(ui);
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, p.panel);
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(8.0, 8.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let ui = &mut ui;
        // A click here gives the editor the keyboard, as one on the notes does.
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(rect) {
            self.focused = true;
        }
        ui.label(RichText::new(tr("Tracks")).size(10.5).color(p.weak));
        ui.add_space(2.0);
        let project = &scene.project;
        let several = |track| scene.parts.iter().filter(|q| q.track == track).count() > 1;
        let mut removed = None;
        for (k, part) in scene.parts.iter().enumerate() {
            let name = part
                .track
                .and_then(|t| project.track(t))
                .map_or_else(String::new, |t| t.name.clone());
            let name = if several(part.track) {
                format!("{name}: {}", part.clip.name)
            } else {
                name
            };
            let active = k == scene.active;
            let (row, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
            let fill = if active {
                p.accent.gamma_multiply(0.22)
            } else if response.hovered() {
                p.hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(row, 5.0, fill);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    row.min + vec2(2.0, 5.0),
                    pos2(row.left() + 6.0, row.bottom() - 5.0),
                ),
                2.0,
                part.color,
            );
            let mut job = egui::text::LayoutJob::simple_singleline(
                name,
                if active {
                    theme::strong(12.0)
                } else {
                    egui::FontId::proportional(12.0)
                },
                p.text,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(row.width() - 40.0);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(
                pos2(row.left() + 12.0, row.center().y - galley.size().y / 2.0),
                galley,
                p.text,
            );
            let cross =
                Rect::from_center_size(pos2(row.right() - 12.0, row.center().y), vec2(18.0, 18.0));
            let close = ui
                .interact(cross, ui.scope_id().with(("part-close", k)), Sense::click())
                .on_hover_text(tr("Take this track out of the editor"));
            ui.painter().text(
                cross.center(),
                egui::Align2::CENTER_CENTER,
                "✕",
                egui::FontId::proportional(11.0),
                if close.hovered() { p.text } else { p.weak },
            );
            if close.clicked() {
                removed = Some(part.clip.id);
            } else if response
                .on_hover_text(tr(
                    "New notes go to the active track; click to make it active",
                ))
                .clicked()
            {
                self.active = Some(part.clip.id);
            }
        }
        if let Some(clip) = removed {
            self.clip_request = Some(self.clips.iter().copied().filter(|&c| c != clip).collect());
        }
        // The other tracks with MIDI clips, to add.
        let others: Vec<(&str, Vec<audiostrada_core::ClipId>)> = project
            .tracks
            .iter()
            .filter_map(|t| {
                let clips: Vec<_> = t
                    .clips
                    .iter()
                    .filter(|c| c.is_midi() && !self.clips.contains(&c.id))
                    .map(|c| c.id)
                    .collect();
                (!clips.is_empty()).then_some((t.name.as_str(), clips))
            })
            .collect();
        ui.add_space(4.0);
        ui.add_enabled_ui(!others.is_empty(), |ui| {
            ui.menu_button(RichText::new(tr("+ Add a track")).size(11.5), |ui| {
                for (name, clips) in &others {
                    if ui.button(*name).clicked() {
                        let mut all = self.clips.clone();
                        all.extend(clips);
                        self.clip_request = Some(all);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(tr("Edit the notes of another track here too"))
        });
    }

    /// The chords the pencil and the brush stamp: on or off, and which.
    pub(super) fn stamp_controls(&mut self, ui: &mut Ui) {
        let stamp_hint = tr("Chords: the pencil and the brush add a whole chord instead of a note");
        if glyph_button(ui, self.stamp.on, stamp_hint, |painter, r, c| {
            for (i, dy) in [-0.6f32, 0.0, 0.6].into_iter().enumerate() {
                let w = r.width() * (0.55 + 0.15 * i as f32);
                let y = r.center().y + dy * r.height() * 0.5;
                painter.rect_filled(
                    Rect::from_center_size(pos2(r.left() + w / 2.0, y), vec2(w, r.height() * 0.22)),
                    1.5,
                    c,
                );
            }
        })
        .clicked()
        {
            self.stamp.on = !self.stamp.on;
        }
        if self.stamp.on {
            let scale = self.scale;
            let label = self.stamp.label(scale);
            let stamp = &mut self.stamp;
            chip(
                ui,
                &label,
                true,
                tr("Chord kind, inversion and voicing"),
                |ui| stamp_menu(ui, stamp, scale),
            );
        }
    }

    /// The note editor's chord chip, next to the tools it changes: off, or the chord the pencil
    /// and the brush add.
    pub(super) fn chord_chip(&mut self, ui: &mut Ui) {
        let scale = self.scale;
        let label = if self.stamp.on {
            trf!("Chord: {chord}", chord = self.stamp.label(scale))
        } else {
            tr("Chord: off").to_owned()
        };
        let adds = !matches!(self.tool, Tool::Slice | Tool::Erase | Tool::Zoom);
        let stamp = &mut self.stamp;
        ui.add_enabled_ui(adds, |ui| {
            chip(
                ui,
                &label,
                stamp.on,
                tr("Chords: the pencil and the brush add a whole chord instead of a note"),
                |ui| {
                    if ui.selectable_label(!stamp.on, tr("Off")).clicked() {
                        stamp.on = false;
                        ui.close();
                    }
                    ui.separator();
                    let before = (stamp.kind, stamp.size);
                    stamp_menu(ui, stamp, scale);
                    if (stamp.kind, stamp.size) != before {
                        stamp.on = true;
                    }
                },
            );
        });
    }

    /// The grid of the notes, and the snap to it.
    pub(super) fn grid_chip(&mut self, ui: &mut Ui) {
        let grid = self.grid;
        chip(
            ui,
            grid.name(),
            self.snap,
            tr("Grid of the piano roll; hold Ctrl to place freely"),
            |ui| {
                for g in Grid::ALL {
                    if ui.selectable_label(self.grid == g, g.name()).clicked() {
                        self.grid = g;
                        ui.close();
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.snap, tr("Snap to the grid"));
            },
        );
    }

    /// The scale: shades the keys outside it, can fold the keyboard and keep the notes in it.
    pub(super) fn scale_chip(&mut self, ui: &mut Ui, scene: &Scene) {
        let scale_label = self
            .scale
            .map_or_else(|| tr("No scale").to_owned(), Scale::name);
        let mut scale = self.scale;
        let (mut fold, mut keep) = (self.fold, self.scale_snap);
        chip(
            ui,
            &scale_label,
            self.scale.is_some(),
            tr("Scale: shades the keys outside it, can fold the keyboard and keep the notes in it"),
            |ui| {
                ui.label(RichText::new(tr("Root")).weak().size(10.5));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (k, name) in NOTE_NAMES.iter().enumerate() {
                        let on = scale.is_some_and(|s| usize::from(s.root) == k);
                        if ui.selectable_label(on, *name).clicked() {
                            scale = Some(Scale {
                                root: k as u8,
                                mode: scale.map_or(0, |s| s.mode),
                            });
                        }
                    }
                });
                ui.separator();
                if ui
                    .selectable_label(scale.is_none(), tr("No scale"))
                    .clicked()
                {
                    scale = None;
                }
                for (i, (name, _)) in SCALES.iter().enumerate() {
                    if ui
                        .selectable_label(scale.is_some_and(|s| s.mode == i), tr(name))
                        .clicked()
                    {
                        scale = Some(Scale {
                            root: scale.map_or(0, |s| s.root),
                            mode: i,
                        });
                    }
                }
                ui.separator();
                ui.add_enabled_ui(scale.is_some(), |ui| {
                    ui.checkbox(
                        &mut fold,
                        tr("Fold: only the scale's keys and those in use"),
                    );
                    ui.checkbox(&mut keep, tr("Keep the notes in the scale"));
                });
                if ui.button(tr("Detect from the notes")).clicked() {
                    scale = detect_scale(&scene.notes);
                }
            },
        );
        self.scale = scale;
        self.fold = fold && scale.is_some();
        self.scale_snap = keep && scale.is_some();
    }

    /// Frames the notes.
    pub(super) fn fit_button(&mut self, ui: &mut Ui) {
        if glyph_button(ui, false, tr("Frame the notes"), |painter, r, c| {
            let s = Stroke::new(1.5, c);
            for (a, b) in [
                (r.left_top(), vec2(1.0, 1.0)),
                (r.right_top(), vec2(-1.0, 1.0)),
                (r.left_bottom(), vec2(1.0, -1.0)),
                (r.right_bottom(), vec2(-1.0, -1.0)),
            ] {
                painter.line_segment([a, a + vec2(b.x * 5.0, 0.0)], s);
                painter.line_segment([a, a + vec2(0.0, b.y * 5.0)], s);
            }
        })
        .clicked()
        {
            self.fit = true;
        }
    }

    /// The other MIDI clips, faint.
    pub(super) fn ghosts_button(&mut self, ui: &mut Ui) {
        if glyph_button(
            ui,
            self.ghosts,
            tr("Ghost notes: the other MIDI clips, faint (double-click one to edit its clip)"),
            |painter, r, c| {
                painter.rect_stroke(
                    Rect::from_min_size(r.min, vec2(r.width() * 0.7, r.height() * 0.45)),
                    2.0,
                    Stroke::new(1.2, c.gamma_multiply(0.6)),
                    egui::StrokeKind::Inside,
                );
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(r.left() + r.width() * 0.3, r.top() + r.height() * 0.55),
                        r.max,
                    ),
                    2.0,
                    c,
                );
            },
        )
        .clicked()
        {
            self.ghosts = !self.ghosts;
        }
    }

    /// The transformations of the selected notes (or all), previewed before they apply.
    pub(super) fn transform_menu(&mut self, ui: &mut Ui, scene: &Scene) {
        let p = theme::current(ui);
        let step = self.grid.beats().unwrap_or(1.0);
        let label = RichText::new(format!("{}  ⏷", tr("Transform"))).size(11.5);
        let response = ui
            .add(egui::Button::new(label).fill(p.surface).corner_radius(6.0))
            .on_hover_text(tr(
                "Transformations of the selected notes (or all), previewed before they are applied",
            ));
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(240.0);
            for transform in Transform::all(step) {
                if ui
                    .button(transform.name())
                    .on_hover_text(transform.help())
                    .clicked()
                {
                    self.open_transform(transform, scene);
                    ui.close();
                }
            }
            ui.separator();
            self.instant_items(ui);
        });
    }

    pub(super) fn open_transform(&mut self, transform: Transform, scene: &Scene) {
        self.pending = Some(Pending {
            transform,
            base: scene.notes.clone(),
            owner: scene.owner.clone(),
            chosen: self.chosen(scene.notes.len()),
        });
    }

    /// Transformations without settings.
    fn instant_items(&mut self, ui: &mut Ui) {
        let scale = self.scale;
        type Change<'a> = &'a dyn Fn(&mut Vec<Note>, &[usize]);
        let items: [(&str, &str, Change); 6] = [
            (tr("Legato"), tr("Each note up to the next one"), &|n, c| {
                ops::legato(n, c, 0)
            }),
            (tr("Reverse"), tr("Played backwards"), &|n, c| {
                ops::reverse(n, c)
            }),
            (tr("Mirror"), tr("Upside down"), &|n, c| {
                ops::mirror(n, c, scale)
            }),
            (
                tr("Glue"),
                tr("Joins the touching notes of the same key"),
                &|n, c| ops::glue(n, c),
            ),
            (
                tr("Mute or unmute"),
                tr("Muted notes stay but do not play"),
                &|n, c| {
                    let mute = c.iter().any(|&i| n.get(i).is_some_and(|n| !n.muted));
                    c.iter().for_each(|&i| n[i].muted = mute)
                },
            ),
            (
                tr("Double length"),
                tr("Twice as long, from the first note"),
                &|n, c| ops::stretch(n, c, 2.0),
            ),
        ];
        for (name, help, change) in items {
            if ui.button(name).on_hover_text(help).clicked() {
                self.instant(name, change);
                ui.close();
            }
        }
    }

    /// Right-click on the notes.
    pub(super) fn context_menu(&mut self, ui: &mut Ui, scene: &Scene) {
        let any = !self.selected.is_empty();
        ui.label(RichText::new(self.summary(&scene.notes)).weak().size(10.5));
        if ui.add_enabled(any, egui::Button::new(tr("Copy"))).clicked() {
            self.copy();
            ui.close();
        }
        if ui.add_enabled(any, egui::Button::new(tr("Cut"))).clicked() {
            self.cut();
            ui.close();
        }
        if ui
            .add_enabled(
                !self.clipboard.is_empty(),
                egui::Button::new(tr("Paste at the playhead")),
            )
            .clicked()
        {
            let at = self.doc.borrow().player().position();
            self.paste(&Default::default(), at);
            ui.close();
        }
        if ui
            .add_enabled(any, egui::Button::new(tr("Duplicate")))
            .clicked()
        {
            self.duplicate();
            ui.close();
        }
        if ui
            .add_enabled(any, egui::Button::new(tr("Delete")))
            .clicked()
        {
            self.delete_selected();
            ui.close();
        }
        if ui.button(tr("Select all")).clicked() {
            self.select_all();
            ui.close();
        }
        ui.separator();
        let step = self.grid.beats().unwrap_or(1.0);
        ui.menu_button(tr("Transform"), |ui| {
            for transform in Transform::all(step) {
                if ui
                    .button(transform.name())
                    .on_hover_text(transform.help())
                    .clicked()
                {
                    self.open_transform(transform, scene);
                    ui.close();
                }
            }
        });
        self.instant_items(ui);
        ui.separator();
        ui.menu_button(tr("Channel"), |ui| {
            for c in 0..16u8 {
                if ui
                    .button(trf!("Channel {channel}", channel = c + 1))
                    .clicked()
                {
                    let mut notes = scene.notes.clone();
                    for &i in &self.chosen(notes.len()) {
                        notes[i].channel = c;
                    }
                    self.commit(tr("Note channel"), notes, scene.owner.clone());
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button(tr("Own channels (MPE)"))
                .on_hover_text(tr(
                    "Overlapping notes on their own channel (2 to 16), each shaped alone",
                ))
                .clicked()
            {
                self.selected.clear();
                self.spread_channels(scene);
                ui.close();
            }
        });
        if ui.button(tr("Select the same key")).clicked() {
            let keys: Vec<u8> = self
                .selected
                .iter()
                .filter_map(|&i| scene.notes.get(i))
                .map(|n| n.key)
                .collect();
            self.selected = (0..scene.notes.len())
                .filter(|&i| keys.contains(&scene.notes[i].key))
                .collect();
            ui.close();
        }
    }

    /// The panel of the transformation being previewed: its settings, Apply and Cancel.
    pub(super) fn transform_panel(&mut self, ui: &mut Ui, scene: &Scene, area: Rect) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        let p = theme::current(ui);
        let mut close = false;
        let mut apply_now = false;
        let at = pos2(area.right() - 300.0, area.top() + 10.0);
        egui::Area::new(egui::Id::unique("piano-transform"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_width(270.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(pending.transform.name()).strong().size(13.5));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(trf!(
                                            "{count} notes",
                                            count = pending.chosen.len()
                                        ))
                                        .weak()
                                        .size(10.5),
                                    );
                                },
                            );
                        });
                        ui.label(RichText::new(pending.transform.help()).weak().size(10.5));
                        ui.add_space(6.0);
                        settings(ui, &mut pending.transform, self.scale.is_some());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if theme::primary_button(ui, tr("Apply")).clicked() {
                                apply_now = true;
                            }
                            if ui.button(tr("Cancel")).clicked() {
                                close = true;
                            }
                            let _ = p;
                        });
                        ui.label(
                            RichText::new(tr(
                                "Previewed on the notes. Enter applies, Escape cancels.",
                            ))
                            .weak()
                            .size(9.5),
                        );
                    });
            });
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            apply_now = true;
        }
        if apply_now {
            let changed = apply_parts(&pending, scene.timing(), self.scale);
            self.selected = changed.picked.unwrap_or_default();
            self.commit(pending.transform.name(), changed.notes, changed.owner);
        } else if !close {
            self.pending = Some(pending);
        }
    }
}

/// The settings of a transformation.
fn settings(ui: &mut Ui, transform: &mut Transform, scale: bool) {
    let percent = |ui: &mut Ui, label: &str, value: &mut f64| {
        ui.add(
            egui::Slider::new(value, 0.0..=1.0)
                .text(label)
                .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
        );
    };
    let reseed = |ui: &mut Ui, seed: &mut u32| {
        if ui.button(tr("Another draw")).clicked() {
            *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345).max(1);
        }
    };
    let grid = |ui: &mut Ui, step: &mut f64| {
        ui.horizontal(|ui| {
            ui.label(tr("Step"));
            for g in [
                Grid::Quarter,
                Grid::Eighth,
                Grid::Sixteenth,
                Grid::ThirtySecond,
                Grid::EighthTriplet,
                Grid::SixteenthTriplet,
            ] {
                let beats = g.beats().unwrap_or(1.0);
                if ui
                    .selectable_label((*step - beats).abs() < 1e-6, g.name())
                    .clicked()
                {
                    *step = beats;
                }
            }
        });
    };
    match transform {
        Transform::Quantize(q) => {
            grid(ui, &mut q.step);
            percent(ui, tr("Strength"), &mut q.strength);
            percent(ui, tr("Swing"), &mut q.swing);
            ui.checkbox(&mut q.ends, tr("The ends too"));
        }
        Transform::Humanize {
            timing,
            velocity,
            length,
            seed,
        } => {
            ui.add(
                egui::Slider::new(timing, 0.0..=40.0)
                    .text(tr("Timing"))
                    .suffix(" ms"),
            );
            ui.add(egui::Slider::new(velocity, 0.0..=40.0).text(tr("Velocity")));
            percent(ui, tr("Length"), length);
            reseed(ui, seed);
        }
        Transform::Strum { ms, direction } => {
            ui.add(
                egui::Slider::new(ms, 2.0..=120.0)
                    .text(tr("Between notes"))
                    .suffix(" ms"),
            );
            ui.horizontal(|ui| {
                ui.selectable_value(direction, Strum::Up, tr("Up"));
                ui.selectable_value(direction, Strum::Down, tr("Down"));
                ui.selectable_value(direction, Strum::Alternate, tr("Alternate"));
            });
        }
        Transform::Arpeggiate(a) => {
            ui.horizontal_wrapped(|ui| {
                for (pattern, name) in [
                    (Arpeggio::Up, tr("Up")),
                    (Arpeggio::Down, tr("Down")),
                    (Arpeggio::UpDown, tr("Up and down")),
                    (Arpeggio::AsPlayed, tr("As played")),
                    (Arpeggio::Random, tr("Random")),
                ] {
                    ui.selectable_value(&mut a.pattern, pattern, name);
                }
            });
            grid(ui, &mut a.step);
            percent(ui, tr("Gate"), &mut a.gate);
            ui.add(egui::Slider::new(&mut a.octaves, 1..=4).text(tr("Octaves")));
            if a.pattern == Arpeggio::Random {
                reseed(ui, &mut a.seed);
            }
        }
        Transform::Harmonize { steps } => {
            ui.label(if scale {
                tr("Interval, in the scale")
            } else {
                tr("Interval, in semitones")
            });
            ui.horizontal_wrapped(|ui| {
                let choices: &[(i32, &str)] = if scale {
                    &[
                        (-7, "−8ve"),
                        (-5, "−6th"),
                        (-2, "−3rd"),
                        (2, "3rd"),
                        (4, "5th"),
                        (5, "6th"),
                        (7, "8ve"),
                    ]
                } else {
                    &[
                        (-12, "−8ve"),
                        (-3, "−m3"),
                        (3, "m3"),
                        (4, "M3"),
                        (7, "5th"),
                        (12, "8ve"),
                    ]
                };
                for &(value, name) in choices {
                    ui.selectable_value(steps, value, name);
                }
            });
        }
        Transform::Chordify { size, voicing } => {
            if !scale {
                ui.label(
                    RichText::new(tr(
                        "Without a scale: major chords. Choose a scale for chords that fit.",
                    ))
                    .weak()
                    .size(10.5),
                );
            }
            ui.horizontal(|ui| {
                ui.selectable_value(size, 3, tr("Triads"));
                ui.selectable_value(size, 4, tr("Sevenths"));
            });
            ui.horizontal_wrapped(|ui| {
                for v in Voicing::ALL {
                    ui.selectable_value(voicing, v, v.name());
                }
            });
        }
        Transform::Vary(v) => {
            percent(ui, tr("Rhythm"), &mut v.rhythm);
            percent(ui, tr("Melody"), &mut v.melody);
            grid(ui, &mut v.step);
            reseed(ui, &mut v.seed);
        }
        Transform::Randomize {
            velocity,
            probability,
            seed,
        } => {
            ui.add(egui::Slider::new(velocity, 0.0..=64.0).text(tr("Velocity")));
            ui.add(
                egui::Slider::new(probability, 1..=100)
                    .text(tr("Lowest chance"))
                    .suffix(" %"),
            );
            reseed(ui, seed);
        }
        Transform::Velocity {
            from,
            to,
            curve,
            contrast,
        } => {
            ui.add(egui::Slider::new(from, 1..=127).text(tr("From")));
            ui.add(egui::Slider::new(to, 1..=127).text(tr("To")));
            ui.add(
                egui::Slider::new(curve, 0.25..=4.0)
                    .logarithmic(true)
                    .text(tr("Curve")),
            );
            ui.add(egui::Slider::new(contrast, 0.0..=2.0).text(tr("Contrast")));
        }
        Transform::Stretch(f) => {
            ui.add(
                egui::Slider::new(f, 0.25..=4.0)
                    .logarithmic(true)
                    .text(tr("Factor"))
                    .suffix(" ×"),
            );
            ui.horizontal(|ui| {
                for (v, name) in [(0.5, "½"), (2.0 / 3.0, "⅔"), (1.5, "1½"), (2.0, "2")] {
                    if ui.selectable_label((*f - v).abs() < 1e-6, name).clicked() {
                        *f = v;
                    }
                }
            });
        }
    }
}

/// The scale the notes fit best: most notes inside, the root on the most played key on a tie.
pub(super) fn detect_scale(notes: &[Note]) -> Option<Scale> {
    if notes.is_empty() {
        return None;
    }
    let mut weight = [0u64; 12];
    for n in notes {
        weight[usize::from(n.key % 12)] += n.length.max(1);
    }
    let mut best: Option<(u64, u64, Scale)> = None;
    for mode in [0usize, 1] {
        for root in 0..12u8 {
            let scale = Scale { root, mode };
            let inside: u64 = (0..12u8)
                .filter(|&k| scale.contains(k))
                .map(|k| weight[usize::from(k)])
                .sum();
            let tonic = weight[usize::from(root)];
            if best.is_none_or(|(i, t, _)| inside > i || inside == i && tonic > t) {
                best = Some((inside, tonic, scale));
            }
        }
    }
    best.map(|b| b.2)
}

/// The chord's kind, inversion and voicing.
fn stamp_menu(ui: &mut Ui, stamp: &mut Stamp, scale: Option<Scale>) {
    if scale.is_some() {
        ui.label(RichText::new(tr("From the scale")).weak().size(10.5));
        ui.horizontal(|ui| {
            if ui
                .selectable_label(stamp.kind.is_none() && stamp.size == 3, tr("Triads"))
                .clicked()
            {
                stamp.kind = None;
                stamp.size = 3;
            }
            if ui
                .selectable_label(stamp.kind.is_none() && stamp.size == 4, tr("Sevenths"))
                .clicked()
            {
                stamp.kind = None;
                stamp.size = 4;
            }
        });
        ui.separator();
    }
    ui.label(RichText::new(tr("Chord")).weak().size(10.5));
    egui::Grid::new("stamp-kinds")
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            for (i, kind) in ChordKind::ALL.into_iter().enumerate() {
                let name = if kind.symbol().is_empty() {
                    "maj"
                } else {
                    kind.symbol()
                };
                if ui
                    .selectable_label(stamp.kind == Some(kind), name)
                    .clicked()
                {
                    stamp.kind = Some(kind);
                }
                if i % 6 == 5 {
                    ui.end_row();
                }
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Inversion")).weak().size(10.5));
        for i in 0..4 {
            ui.selectable_value(&mut stamp.inversion, i, i.to_string());
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Voicing")).weak().size(10.5));
        for v in Voicing::ALL {
            ui.selectable_value(&mut stamp.voicing, v, v.name());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_detected_from_the_notes() {
        let note = |key, length| Note {
            key,
            length,
            ..Default::default()
        };
        let a_minor = [
            note(57, 400),
            note(60, 100),
            note(64, 200),
            note(67, 100),
            note(69, 300),
            note(62, 50),
        ];
        assert_eq!(detect_scale(&a_minor), Some(Scale { root: 9, mode: 1 }));
    }
}
