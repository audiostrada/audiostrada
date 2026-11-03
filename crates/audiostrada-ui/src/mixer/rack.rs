//! The rack: the chain of the selected track (or of a pinned channel) as modules side by side,
//! each with its main controls, A/B and its editor a click away; parallel chains split the
//! signal into branches of their own and mix them back.

use std::collections::BTreeMap;

use super::*;

/// Width of an effect's module.
const MODULE: f32 = 244.0;
/// Height of the modules' faceplates.
const HEIGHT: f32 = 180.0;
/// Height of the rails above and below the modules.
const RAIL: f32 = 14.0;
/// Width of the gaps between modules, with their add button.
const GAP: f32 = 34.0;
/// Width of a parallel chain's module.
const PARALLEL: f32 = 470.0;
/// Width of the empty slot at the end of the rack.
const EMPTY: f32 = 132.0;

/// Parameter values of an insert, as an A or B slot keeps them.
type Settings = BTreeMap<String, f32>;

/// What the rack remembers between frames.
#[derive(Debug, Default)]
pub(super) struct RackState {
    /// Channel shown whatever the selection.
    pub pin: Option<Channel>,
    /// Where the effect chosen in the browser goes.
    pub place: Option<(Channel, routing::Place, usize)>,
    /// Insert being dragged.
    drag: Option<InsertId>,
    /// Where a dragged insert can land, drawn this frame.
    gaps: Vec<(Rect, routing::Place, usize)>,
    /// Settings kept in the A and B slots of each insert, and the one in use.
    ab: HashMap<InsertId, (usize, [Option<Settings>; 2])>,
}

/// The parameters shown on a built-in effect's face: the main ones of the common effects,
/// else its first values that are neither switches nor lists.
fn face(info: &audiostrada_dsp::EffectInfo) -> Vec<&'static str> {
    let chosen: &[&str] = match info.id {
        "reverb" | "plate" | "hall" => &["decay", "predelay", "size", "mix"],
        "delay" => &["time", "feedback", "high_cut", "mix"],
        "tape" => &["drive", "bump", "wow", "mix"],
        "compressor" => &["threshold", "ratio", "attack", "makeup"],
        _ => &[],
    };
    let known: Vec<&'static str> = info
        .params
        .iter()
        .filter(|p| chosen.contains(&p.id))
        .map(|p| p.id)
        .collect();
    if !known.is_empty() {
        return known;
    }
    info.params
        .iter()
        .filter(|p| !p.toggle && p.choices.is_empty())
        .take(4)
        .map(|p| p.id)
        .collect()
}

/// The response of an EQ module's bands, drawn in its face.
fn eq_curve(ui: &mut Ui, values: &[f32], tint: Color32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, p.panel);
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        egui::Stroke::new(1.0, p.weak.gamma_multiply(0.25)),
    );
    let offsets = [0.0; audiostrada_dsp::eq::BANDS];
    let points: Vec<egui::Pos2> = (0..=60)
        .map(|i| {
            let t = i as f32 / 60.0;
            let freq = 20.0 * 1000f64.powf(f64::from(t));
            let db = audiostrada_dsp::eq::response_db(values, &offsets, freq, 48_000.0)
                .clamp(-18.0, 18.0) as f32;
            pos2(
                rect.left() + rect.width() * t,
                rect.center().y - db / 18.0 * rect.height() * 0.45,
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5, tint)));
}

/// An effect and its settings, as a template puts it in a branch.
type Effect = (&'static str, &'static [(&'static str, f32)]);

/// What a new parallel chain starts with.
#[derive(Clone, Copy, PartialEq)]
enum Template {
    Empty,
    Compression,
    Saturation,
    Blend,
}

impl Template {
    const ALL: [Template; 4] = [
        Template::Compression,
        Template::Saturation,
        Template::Blend,
        Template::Empty,
    ];

    fn label(self) -> &'static str {
        match self {
            Template::Compression => tr("Parallel compression"),
            Template::Saturation => tr("Parallel saturation"),
            Template::Blend => tr("Reverb blended with the dry sound"),
            Template::Empty => tr("Empty parallel chain"),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Template::Compression => tr(
                "The dry sound and a heavily compressed copy, mixed: density and sustain \
                 without losing the attacks (New York compression)",
            ),
            Template::Saturation => {
                tr("The dry sound and a driven copy, mixed: grain and harmonics, the low end kept")
            }
            Template::Blend => tr(
                "The dry sound and a fully wet reverb, each with its level: the space without \
                 washing out the source",
            ),
            Template::Empty => tr("Two branches to fill: the dry sound and one for effects"),
        }
    }

    /// The second branch's name, effect, settings and level.
    fn wet(self) -> (&'static str, Option<Effect>, f32) {
        match self {
            Template::Compression => (
                tr("Crushed"),
                Some((
                    "compressor",
                    &[
                        ("threshold", -35.0),
                        ("ratio", 10.0),
                        ("attack", 1.0),
                        ("release", 100.0),
                        ("makeup", 8.0),
                    ],
                )),
                -6.0,
            ),
            Template::Saturation => (tr("Driven"), Some(("drive", &[("drive", 30.0)])), -10.0),
            Template::Blend => (tr("Wet"), Some(("reverb", &[("mix", 100.0)])), -12.0),
            Template::Empty => ("B", None, 0.0),
        }
    }
}

/// Adds a parallel chain made from a template at `index` of a channel's list.
fn add_template(p: &mut Project, channel: Channel, index: usize, template: Template) {
    let Some(par) = routing::add_parallel(p, channel, index) else {
        return;
    };
    let (name, effect, level) = template.wet();
    if let Some(dry) = routing::branch_mut(p, channel, par, 0) {
        dry.name = tr("Dry").to_owned();
    }
    if let Some(wet) = routing::branch_mut(p, channel, par, 1) {
        wet.name = name.to_owned();
        wet.gain_db = level;
    }
    if let Some((effect, settings)) = effect
        && let Some(id) = routing::add_insert_at(p, channel, (Some((par, 1)), 0), effect)
    {
        for &(key, value) in settings {
            routing::set_insert_param(p, channel, id, key, value);
        }
    }
}

/// Rails above and below a column of the rack, with their holes, and the inside between
/// them: in the theme's tones.
fn rails(ui: &Ui, rect: Rect) {
    let p = theme::current(ui);
    let painter = ui.painter_at(rect);
    for rail in [
        Rect::from_min_size(rect.min, vec2(rect.width(), RAIL)),
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - RAIL),
            vec2(rect.width(), RAIL),
        ),
    ] {
        theme::gradient_rect(&painter, rail, p.hover, p.surface);
        // Holes on a fixed pitch, whatever the column.
        let pitch = 22.0;
        let mut x = (rail.left() / pitch).ceil() * pitch;
        while x < rail.right() {
            painter.circle_filled(pos2(x, rail.center().y), 2.0, p.base);
            x += pitch;
        }
    }
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.left(), rect.top() + RAIL),
            pos2(rect.right(), rect.bottom() - RAIL),
        ),
        0.0,
        p.base,
    );
}

/// A screw head, discreet.
fn screw(painter: &egui::Painter, center: egui::Pos2, p: &theme::Palette) {
    painter.circle_filled(center, 2.6, p.hover);
    painter.line_segment(
        [center - vec2(1.6, 0.8), center + vec2(1.6, 0.8)],
        egui::Stroke::new(1.0, p.base),
    );
}

/// A module's faceplate, as the mixer's strips: their background and border, its family's
/// color on top, a screw at each corner.
fn faceplate(ui: &Ui, rect: Rect, tint: Color32, lit: bool) {
    let p = theme::current(ui);
    let painter = ui.painter_at(rect.expand(1.0));
    let fill = if p.dark {
        theme::mix(p.panel, p.surface, 0.3)
    } else {
        theme::mix(p.panel, p.surface, 0.45)
    };
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 4.0)),
        egui::CornerRadius {
            nw: 6,
            ne: 6,
            sw: 0,
            se: 0,
        },
        tint.gamma_multiply(if lit { 1.0 } else { 0.35 }),
    );
    for corner in [
        rect.left_top() + vec2(7.0, 11.0),
        rect.right_top() + vec2(-7.0, 11.0),
        rect.left_bottom() + vec2(7.0, -8.0),
        rect.right_bottom() + vec2(-7.0, -8.0),
    ] {
        screw(&painter, corner, &p);
    }
}

/// A module's power switch: a round button with a green light while it plays.
fn power_led(ui: &mut Ui, on: bool) -> Response {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    painter.circle_filled(
        center,
        8.0,
        if response.hovered() { p.hover } else { p.base },
    );
    if on {
        painter.circle_filled(center, 9.5, theme::PLAY.gamma_multiply(0.18));
    }
    theme::paint_power(painter, rect.shrink(3.0), theme::power_ink(on, &p));
    response
}

impl MixerView {
    /// The rack's panel.
    pub fn rack(&mut self, ui: &mut Ui) {
        let doc_rc = Rc::clone(&self.doc);
        let Ok(mut doc) = doc_rc.try_borrow_mut() else {
            return;
        };
        let project = doc.project().clone();
        let selected = doc
            .selected_tracks()
            .last()
            .copied()
            .filter(|&t| project.track(t).is_some_and(|t| t.kind == TrackKind::Audio));
        let channel = self
            .rack
            .pin
            .or(selected.map(Channel::Track))
            .filter(|c| match c {
                Channel::Track(t) => project.track(*t).is_some(),
                Channel::Master => true,
            });
        self.rack_header(ui, &doc, &project, channel);
        ui.add_space(4.0);
        let Some(channel) = channel else {
            ui.weak(tr("Select a track to see its effects here."));
            return;
        };
        self.rack.gaps.clear();
        egui::ScrollArea::horizontal()
            .id_salt("rack")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.rack_strip(ui, &mut doc, channel);
                    let shown: Vec<audiostrada_core::Insert> = routing::inserts(&project, channel)
                        .iter()
                        .filter(|i| !i.monitor && i.effect != routing::STRIP)
                        .cloned()
                        .collect();
                    for (k, insert) in shown.iter().enumerate() {
                        let at = listed_index(&project, channel, false, k);
                        self.rack_gap(ui, &mut doc, channel, (None, at));
                        if insert.effect == routing::PARALLEL {
                            self.parallel_module(ui, &mut doc, channel, insert);
                        } else {
                            self.module(ui, &mut doc, channel, insert);
                        }
                    }
                    let end = listed_index(&project, channel, false, shown.len());
                    self.rack_gap(ui, &mut doc, channel, (None, end));
                    self.empty_slot(ui, &mut doc, channel, end);
                });
            });
        self.rack_drop(ui, &mut doc, channel);
    }

    /// A column of the rack `width` wide: rails, a faceplate and what `add` puts on it.
    fn slot(
        &mut self,
        ui: &mut Ui,
        width: f32,
        (tint, lit): (Color32, bool),
        add: impl FnOnce(&mut Self, &mut Ui),
    ) -> Rect {
        let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT + 2.0 * RAIL), Sense::hover());
        rails(ui, rect);
        let plate = Rect::from_min_max(
            pos2(rect.left() + 2.0, rect.top() + RAIL - 3.0),
            pos2(rect.right() - 2.0, rect.bottom() - RAIL + 3.0),
        );
        faceplate(ui, plate, tint, lit);
        let inner = Rect::from_min_max(plate.min + vec2(16.0, 8.0), plate.max - vec2(16.0, 6.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        add(self, &mut child);
        plate
    }

    /// The channel strip at the head of the rack.
    fn rack_strip(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel) {
        let tint = theme::family_color("EQ & Filter");
        self.slot(ui, 172.0, (tint, true), |view, ui| {
            ui.add_space(6.0);
            view.channel_section(ui, doc, channel, 140.0);
        });
    }

    /// A gap between modules: a round button adds an effect there; it takes a dragged one.
    fn rack_gap(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        (place, index): (routing::Place, usize),
    ) {
        let p = theme::current(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(GAP, HEIGHT + 2.0 * RAIL), Sense::hover());
        rails(ui, rect);
        self.rack.gaps.push((rect, place, index));
        let dropping = self.rack.drag.is_some()
            && ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| rect.contains(pos));
        if dropping {
            ui.painter().line_segment(
                [
                    rect.center_top() + vec2(0.0, RAIL),
                    rect.center_bottom() - vec2(0.0, RAIL),
                ],
                egui::Stroke::new(3.0, p.accent),
            );
        }
        let button = Rect::from_center_size(rect.center(), vec2(24.0, 24.0));
        let response = ui
            .interact(
                button,
                ui.scope_id().with(("rack-add", index, place)),
                Sense::click(),
            )
            .on_hover_text(tr("Add an effect here"));
        let (fill, ink) = if response.hovered() {
            (p.accent, theme::text_on(p.accent))
        } else {
            (p.surface, p.text)
        };
        ui.painter().circle_filled(button.center(), 12.0, fill);
        ui.painter().circle_stroke(
            button.center(),
            12.0,
            egui::Stroke::new(1.0, p.accent.gamma_multiply(0.7)),
        );
        ui.painter().text(
            button.center(),
            egui::Align2::CENTER_CENTER,
            "+",
            egui::FontId::proportional(17.0),
            ink,
        );
        egui::Popup::menu(&response).show(|ui| {
            self.add_menu(ui, doc, channel, (place, index), place.is_none());
        });
    }

    /// The free slot at the end: a large add button.
    fn empty_slot(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel, index: usize) {
        let p = theme::current(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(EMPTY, HEIGHT + 2.0 * RAIL), Sense::hover());
        rails(ui, rect);
        self.rack.gaps.push((rect, None, index));
        let hole = Rect::from_min_max(
            pos2(rect.left() + 6.0, rect.top() + RAIL + 4.0),
            pos2(rect.right() - 6.0, rect.bottom() - RAIL - 4.0),
        );
        let response = ui
            .interact(hole, ui.scope_id().with("rack-empty"), Sense::click())
            .on_hover_text(tr("Add an effect or a parallel chain"));
        let ink = if response.hovered() {
            p.accent
        } else {
            p.weak.gamma_multiply(0.8)
        };
        // A dashed outline: a slot waiting for a module.
        let dashes = egui::Shape::dashed_line(
            &[
                hole.left_top(),
                hole.right_top(),
                hole.right_bottom(),
                hole.left_bottom(),
                hole.left_top(),
            ],
            egui::Stroke::new(1.2, ink),
            6.0,
            4.0,
        );
        ui.painter().extend(dashes);
        ui.painter().text(
            hole.center() - vec2(0.0, 12.0),
            egui::Align2::CENTER_CENTER,
            "+",
            egui::FontId::proportional(30.0),
            ink,
        );
        ui.painter().text(
            hole.center() + vec2(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            tr("Add an effect"),
            egui::FontId::proportional(11.0),
            ink,
        );
        egui::Popup::menu(&response).show(|ui| {
            self.add_menu(ui, doc, channel, (None, index), true);
        });
    }

    /// What can be added at a place: an effect or a plugin, or a parallel chain from a model.
    fn add_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        (place, index): (routing::Place, usize),
        parallel: bool,
    ) {
        if ui.button(tr("Effect or plugin…")).clicked() {
            self.rack.place = Some((channel, place, index));
            self.browser.borrow_mut().open((channel, false));
            ui.close();
        }
        if !parallel {
            return;
        }
        ui.separator();
        ui.label(RichText::new(tr("Parallel chain")).small().weak());
        for template in Template::ALL {
            if ui
                .button(template.label())
                .on_hover_text(template.hint())
                .clicked()
            {
                doc.history_mut().edit(template.label(), |p| {
                    add_template(p, channel, index, template)
                });
                ui.close();
            }
        }
    }

    /// An effect's module: its switch and name, its main controls, A/B and its editor.
    fn module(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let name = effect_name(doc, &insert.effect);
        let error = doc.player().plugin_error(id).is_some();
        let latency = latency_label(doc, channel, id);
        let tint = theme::family_color(family(doc, &insert.effect));
        self.slot(ui, MODULE, (tint, !insert.bypass), |view, ui| {
            let p = theme::current(ui);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if power_led(ui, !insert.bypass)
                    .on_hover_text(tr("Bypass or play"))
                    .clicked()
                {
                    let bypass = !insert.bypass;
                    doc.history_mut().edit(tr("Bypass insert"), |p| {
                        routing::set_bypass(p, channel, id, bypass)
                    });
                }
                let ink = match (error, insert.bypass) {
                    (true, _) => ERROR,
                    (false, true) => p.weak,
                    (false, false) => p.text,
                };
                let width = ui.available_width() - if latency.is_some() { 44.0 } else { 0.0 };
                let head = ui
                    .add_sized(
                        [width, 18.0],
                        egui::Label::new(RichText::new(&name).strong().size(12.5).color(ink))
                            .truncate()
                            .sense(Sense::click_and_drag()),
                    )
                    .on_hover_text(tr("Click to edit, drag to move"));
                #[cfg(test)]
                super::tests::seen(format!("module {}", id.0), head.rect);
                if head.drag_started() {
                    view.rack.drag = Some(id);
                }
                if head.clicked() {
                    view.open_insert(channel, insert);
                }
                if let Some(latency) = &latency {
                    ui.label(RichText::new(latency).size(9.0).color(p.weak));
                }
            });
            ui.add_space(6.0);
            ui.allocate_ui(vec2(ui.available_width(), HEIGHT - 66.0), |ui| {
                ui.set_min_height(HEIGHT - 66.0);
                match audiostrada_dsp::info(&insert.effect) {
                    Some(info) => {
                        let mut params = effect_editors::Params {
                            values: info
                                .params
                                .iter()
                                .map(|p| insert.params.get(p.id).copied().unwrap_or(p.default))
                                .collect(),
                            doc: &mut *doc,
                            channel,
                            insert: id,
                            info,
                        };
                        if info.id == "eq" {
                            eq_curve(ui, &params.values, tint);
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                                for param in face(info) {
                                    effect_editors::param_knob(ui, &mut params, param, 34.0, tint);
                                }
                            });
                        }
                    }
                    None => view.macros(ui, doc, channel, insert, tint),
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                view.ab_buttons(ui, doc, channel, insert);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if remove_button(ui).clicked() {
                        view.remove_insert(doc, channel, id);
                    }
                    if ui
                        .small_button(tr("Edit"))
                        .on_hover_text(tr("Every setting, in the effect's window"))
                        .clicked()
                    {
                        view.open_insert(channel, insert);
                    }
                });
            });
        });
    }

    /// A parallel chain, drawn as the signal goes: split at the left, a lane per branch with
    /// its effects, its switches and its level, mixed back at the right.
    fn parallel_module(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let tint = theme::family_color("Utility");
        self.slot(ui, PARALLEL, (tint, !insert.bypass), |view, ui| {
            let p = theme::current(ui);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if power_led(ui, !insert.bypass)
                    .on_hover_text(tr("Bypass or play"))
                    .clicked()
                {
                    let bypass = !insert.bypass;
                    doc.history_mut().edit(tr("Bypass insert"), |p| {
                        routing::set_bypass(p, channel, id, bypass)
                    });
                }
                let head = ui
                    .add(
                        egui::Label::new(
                            RichText::new(tr("Parallel chain"))
                                .strong()
                                .size(12.5)
                                .color(if insert.bypass { p.weak } else { p.text }),
                        )
                        .sense(Sense::click_and_drag()),
                    )
                    .on_hover_text(tr("Drag to move"));
                if head.drag_started() {
                    view.rack.drag = Some(id);
                }
                ui.label(
                    RichText::new(tr(
                        "the signal goes through every branch, then they are mixed",
                    ))
                    .size(9.5)
                    .color(p.weak),
                );
            });
            // The lanes: as many as fit, closer when there are more.
            let area = Rect::from_min_size(
                ui.cursor().min + vec2(0.0, 4.0),
                vec2(ui.available_width(), HEIGHT - 70.0),
            );
            ui.allocate_rect(area, Sense::hover());
            let count = insert.branches.len().max(1);
            let pitch = (area.height() / count as f32).clamp(22.0, 36.0);
            let top = area.center().y - pitch * (count as f32 - 1.0) / 2.0;
            let input = pos2(area.left() + 4.0, area.center().y);
            let output = pos2(area.right() - 4.0, area.center().y);
            let solo = insert.branches.iter().any(|b| b.solo);
            for (b, branch) in insert.branches.iter().enumerate() {
                let y = top + pitch * b as f32;
                let heard = !insert.bypass && !branch.mute && (!solo || branch.solo);
                let wire = egui::Stroke::new(
                    if heard { 2.0 } else { 1.0 },
                    if heard {
                        tint
                    } else {
                        p.weak.gamma_multiply(0.4)
                    },
                );
                let start = pos2(area.left() + 26.0, y);
                let end = pos2(area.right() - 26.0, y);
                let painter = ui.painter();
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        input,
                        pos2(input.x + 14.0, input.y),
                        pos2(start.x - 12.0, y),
                        start,
                    ],
                    false,
                    Color32::TRANSPARENT,
                    wire,
                ));
                painter.line_segment([start, end], wire);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        end,
                        pos2(end.x + 12.0, y),
                        pos2(output.x - 14.0, output.y),
                        output,
                    ],
                    false,
                    Color32::TRANSPARENT,
                    wire,
                ));
                let row = Rect::from_min_max(pos2(start.x + 4.0, y - 10.0), pos2(end.x, y + 10.0));
                view.lane(ui, doc, channel, insert, b, branch, row);
            }
            for node in [input, output] {
                ui.painter().circle_filled(node, 4.0, tint);
            }
            ui.horizontal(|ui| {
                if ui
                    .small_button(tr("+ Branch"))
                    .on_hover_text(tr("Another copy of the signal, with effects of its own"))
                    .clicked()
                {
                    doc.history_mut().edit(tr("Add a branch"), |p| {
                        routing::add_branch(p, channel, id);
                    });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if remove_button(ui).clicked() {
                        view.remove_insert(doc, channel, id);
                    }
                });
            });
        });
    }

    /// A branch's lane: its name, its effects on the wire, an add button, mute, solo and level.
    #[allow(clippy::too_many_arguments)]
    fn lane(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        parallel: &audiostrada_core::Insert,
        b: usize,
        branch: &audiostrada_core::Branch,
        row: Rect,
    ) {
        let p = theme::current(ui);
        let id = parallel.id;
        #[cfg(test)]
        super::tests::seen(format!("branch {} {b}", id.0), row);
        // A dragged effect dropped on the lane goes at its end.
        self.rack
            .gaps
            .push((row, Some((id, b)), branch.inserts.len()));
        let mut lane = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut lane;
        ui.spacing_mut().item_spacing.x = 3.0;
        // The name on a plate, over the wire.
        let name = ui
            .add(
                egui::Button::new(RichText::new(&branch.name).strong().size(10.5))
                    .min_size(vec2(44.0, 18.0))
                    .fill(p.panel),
            )
            .on_hover_text(tr("Right-click to rename or remove"));
        name.context_menu(|ui| {
            let mut text = branch.name.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(120.0))
                .changed()
            {
                doc.history_mut().edit_merged(
                    tr("Rename branch"),
                    &format!("branch:{}:{b}", id.0),
                    |p| {
                        if let Some(branch) = routing::branch_mut(p, channel, id, b) {
                            branch.name = text.clone();
                        }
                    },
                );
            }
            if parallel.branches.len() > 1 && ui.button(tr("Remove the branch")).clicked() {
                doc.history_mut().edit(tr("Remove branch"), |p| {
                    routing::remove_branch(p, channel, id, b)
                });
                ui.close();
            }
        });
        for inner in &branch.inserts {
            self.chip(ui, doc, channel, inner);
        }
        if branch.inserts.is_empty() {
            ui.label(
                RichText::new(tr("dry signal"))
                    .italics()
                    .size(10.0)
                    .color(p.weak)
                    .background_color(p.panel),
            );
        }
        let add = ui
            .add(egui::Button::new(RichText::new("+").strong()).min_size(vec2(18.0, 18.0)))
            .on_hover_text(tr("Add an effect to this branch"));
        egui::Popup::menu(&add).show(|ui| {
            self.add_menu(
                ui,
                doc,
                channel,
                (Some((id, b)), branch.inserts.len()),
                false,
            );
        });
        // Its switches and level at the right, before the mix.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let color = theme::family_color("Utility");
            let (changed, response) = send_level(ui, 72.0, branch.gain_db, color, branch.mute);
            response.on_hover_text(tr("Level of this branch in the mix"));
            if let Some(db) = changed {
                doc.history_mut().edit_merged(
                    tr("Branch level"),
                    &format!("branch-level:{}:{b}", id.0),
                    |p| {
                        if let Some(branch) = routing::branch_mut(p, channel, id, b) {
                            branch.gain_db = db;
                        }
                    },
                );
            }
            let edit =
                |doc: &mut Document, label: &str, f: &dyn Fn(&mut audiostrada_core::Branch)| {
                    doc.history_mut().edit(label, |p| {
                        if let Some(branch) = routing::branch_mut(p, channel, id, b) {
                            f(branch);
                        }
                    });
                };
            let size = vec2(18.0, 18.0);
            if theme::letter_toggle(ui, size, "S", branch.solo, theme::SOLO)
                .on_hover_text(tr("Hear this branch alone"))
                .clicked()
            {
                edit(doc, tr("Solo branch"), &|b| b.solo = !b.solo);
            }
            if theme::letter_toggle(ui, size, "M", branch.mute, theme::MUTE)
                .on_hover_text(tr("Mute this branch"))
                .clicked()
            {
                edit(doc, tr("Mute branch"), &|b| b.mute = !b.mute);
            }
        });
    }

    /// The channel shown, the master's chain and the pin.
    fn rack_header(
        &mut self,
        ui: &mut Ui,
        doc: &Document,
        project: &Project,
        channel: Option<Channel>,
    ) {
        ui.horizontal(|ui| {
            match channel.and_then(|c| match c {
                Channel::Track(t) => project.track(t),
                Channel::Master => None,
            }) {
                Some(track) => {
                    let tint = color(track);
                    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    crate::track_icons::paint(
                        ui,
                        rect,
                        crate::track_icons::of(project, track),
                        tint,
                    );
                    ui.label(RichText::new(&track.name).strong().size(14.0).color(tint));
                }
                None if channel == Some(Channel::Master) => {
                    ui.label(RichText::new(tr("Master")).strong().size(14.0));
                }
                None => {}
            }
            if let Some(c) = channel {
                let count = routing::flatten(routing::inserts(project, c))
                    .iter()
                    .filter(|i| i.effect != routing::PARALLEL && i.effect != routing::STRIP)
                    .count();
                ui.weak(trf!("Effects: {count}", count = count));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let pinned = self.rack.pin.is_some();
                if ui
                    .selectable_label(pinned, tr("Pin"))
                    .on_hover_text(tr(
                        "Keep this chain in the rack while other tracks are selected",
                    ))
                    .clicked()
                {
                    self.rack.pin = if pinned { None } else { channel };
                }
                let master = self.rack.pin == Some(Channel::Master);
                if ui
                    .selectable_label(master, tr("Master"))
                    .on_hover_text(tr("The master's chain"))
                    .clicked()
                {
                    self.rack.pin = if master { None } else { Some(Channel::Master) };
                }
                let _ = doc;
            });
        });
    }

    /// Ends a drag: the insert goes to the gap under the pointer.
    fn rack_drop(&mut self, ui: &Ui, doc: &mut Document, channel: Channel) {
        let Some(id) = self.rack.drag else {
            return;
        };
        if ui.input(|i| i.pointer.any_down()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return;
        }
        self.rack.drag = None;
        let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if let Some(&(_, place, index)) = self.rack.gaps.iter().find(|(r, ..)| r.contains(pos)) {
            doc.history_mut().edit(tr("Move insert"), |p| {
                routing::move_within(p, channel, id, (place, index));
            });
        }
    }

    /// Opens an insert's window: AudioStrada's for a built-in effect, the plugin's own else.
    fn open_insert(&mut self, channel: Channel, insert: &audiostrada_core::Insert) {
        if audiostrada_dsp::info(&insert.effect).is_some() {
            self.show_editor(channel, insert.id);
        } else if !self.open_gui.contains(&(channel, insert.id)) {
            self.open_gui.push((channel, insert.id));
        }
    }

    /// A plugin's chosen parameters as knobs; a right-click on the face picks them.
    fn macros(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
        tint: Color32,
    ) {
        let id = insert.id;
        if !self.plugin_params.contains_key(&id)
            && let Some(plugin) = doc.player_mut().plugin(channel, id)
        {
            let params = plugin.params().into_iter().filter(|p| !p.hidden).collect();
            self.plugin_params.insert(id, params);
        }
        let params = self.plugin_params.get(&id).cloned().unwrap_or_default();
        let chosen: Vec<&audiostrada_plugins::ParamDesc> = insert
            .macros
            .iter()
            .filter_map(|m| params.iter().find(|p| p.id.to_string() == *m))
            .collect();
        let face = ui
            .horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                for p in &chosen {
                    let key = p.id.to_string();
                    let lane = AutomationTarget::Param {
                        insert: id,
                        param: key.clone(),
                    };
                    let value = doc
                        .automation_value(channel, &lane)
                        .or_else(|| insert.params.get(&key).copied())
                        .or_else(|| {
                            doc.player_mut()
                                .plugin(channel, id)
                                .and_then(|pl| pl.param_value(p.id))
                                .map(|v| v as f32)
                        })
                        .unwrap_or(p.default as f32);
                    ui.vertical(|ui| {
                        ui.set_width(40.0);
                        let (changed, response) = widgets::knob(
                            ui,
                            value,
                            (p.min as f32, p.max as f32),
                            p.default as f32,
                            30.0,
                            false,
                            tint,
                        );
                        ui.add(egui::Label::new(RichText::new(&p.name).size(9.0)).truncate());
                        let response = response.on_hover_text(&p.name);
                        let name = p.name.clone();
                        automated_control(doc, channel, &lane, changed, &response, |doc, v| {
                            doc.history_mut().edit_merged(
                                &name,
                                &format!("insert:{}:{}", id.0, key),
                                |proj| routing::set_insert_param(proj, channel, id, &key, v),
                            );
                        });
                    });
                }
                if chosen.is_empty() {
                    ui.weak(tr("Right-click to put parameters here"));
                }
            })
            .response
            .interact(Sense::click());
        face.context_menu(|ui| {
            ui.set_max_height(320.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for p in &params {
                    let key = p.id.to_string();
                    let mut on = insert.macros.contains(&key);
                    if ui.checkbox(&mut on, &p.name).changed() {
                        doc.history_mut().edit(tr("Rack controls"), |proj| {
                            if let Some(i) = routing::find_insert_mut(proj, channel, id) {
                                i.macros.retain(|m| *m != key);
                                if on && i.macros.len() < 8 {
                                    i.macros.push(key.clone());
                                }
                            }
                        });
                    }
                }
                if params.is_empty() {
                    ui.weak(tr("This plugin exposes no parameters."));
                }
            });
        });
    }

    /// A and B settings of an insert, to compare two of them.
    fn ab_buttons(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let (current, _) = self.rack.ab.get(&id).cloned().unwrap_or_default();
        for (slot, label) in [(0, "A"), (1, "B")] {
            if ui
                .selectable_label(current == slot, label)
                .on_hover_text(tr("Two settings to compare: the other one is kept"))
                .clicked()
                && current != slot
            {
                let entry = self.rack.ab.entry(id).or_default();
                entry.1[current] = Some(insert.params.clone());
                let next = entry.1[slot]
                    .clone()
                    .unwrap_or_else(|| insert.params.clone());
                entry.0 = slot;
                doc.history_mut()
                    .edit(&trf!("Settings {slot}", slot = label), |p| {
                        for (key, value) in &next {
                            routing::set_insert_param(p, channel, id, key, *value);
                        }
                    });
            }
        }
    }

    /// An effect of a branch: its switch and name; click to edit, drag to move.
    fn chip(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let name = effect_name(doc, &insert.effect);
        let look = Slot {
            name: &name,
            color: theme::family_color(family(doc, &insert.effect)),
            lit: !insert.bypass,
            error: doc.player().plugin_error(id).is_some(),
            latency: None,
        };
        let width = (ui.fonts_mut(|f| {
            f.layout_no_wrap(
                name.clone(),
                egui::FontId::proportional(10.5),
                Color32::WHITE,
            )
            .size()
            .x
        }) + 34.0)
            .clamp(56.0, 120.0);
        let (head, power) = insert_slot(ui, vec2(width, 18.0), id.0, &look);
        if power.clicked() {
            let bypass = !insert.bypass;
            doc.history_mut().edit(tr("Bypass insert"), |p| {
                routing::set_bypass(p, channel, id, bypass)
            });
        }
        let head = head.on_hover_text(tr("Click to edit, drag to move, right-click to remove"));
        if head.drag_started() {
            self.rack.drag = Some(id);
        }
        if head.clicked() {
            self.open_insert(channel, insert);
        }
        head.context_menu(|ui| {
            if ui.button(tr("Remove")).clicked() {
                self.remove_insert(doc, channel, id);
                ui.close();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{rack_frame, rect};
    use super::*;
    use crate::document::AppDirs;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};

    #[test]
    fn a_module_drags_into_a_branch() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let (mut t, mut par, mut delay) = (None, None, None);
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            let id = p.add_track("A");
            let c = Channel::Track(id);
            par = routing::add_parallel(p, c, 0);
            delay = routing::add_insert(p, c, "delay", Default::default());
            t = Some(id);
        });
        let (t, par, delay) = (t.unwrap(), par.unwrap(), delay.unwrap());
        doc.borrow_mut()
            .select_track(t, crate::document::SelectMode::Only);
        let mut view = MixerView::new(Rc::clone(&doc));
        let ctx = egui::Context::default();
        rack_frame(&ctx, &mut view, vec![]);
        rack_frame(&ctx, &mut view, vec![]);
        let from = rect(&format!("module {}", delay.0)).center();
        let to = rect(&format!("branch {} 1", par.0)).center();
        let button = |pos: Pos2, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        rack_frame(&ctx, &mut view, vec![Event::PointerMoved(from)]);
        rack_frame(&ctx, &mut view, vec![button(from, true)]);
        for k in 1..=4 {
            let at = from.lerp(to, k as f32 / 4.0);
            rack_frame(&ctx, &mut view, vec![Event::PointerMoved(at)]);
        }
        rack_frame(&ctx, &mut view, vec![button(to, false)]);
        rack_frame(&ctx, &mut view, vec![]);
        let project = doc.borrow().project().clone();
        let list = routing::inserts(&project, Channel::Track(t));
        assert_eq!(list.len(), 1, "only the chain left at the top");
        assert_eq!(list[0].branches[1].inserts[0].id, delay);
    }
}
