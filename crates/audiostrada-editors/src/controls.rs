//! Controls shared by the effect editors (knobs, lists, switches, meters) and the parameters
//! they change, through [`EditorParams`]: AudioStrada's document or a plugin's host.

use egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, pos2,
    vec2,
};

use audiostrada_dsp::{EffectInfo, ParamInfo};

use crate::i18n::tr;
use crate::{theme, widgets};

/// The parameters of one effect, read and changed by its editor.
pub trait EditorParams {
    fn info(&self) -> &'static EffectInfo;
    fn get(&self, id: &str) -> f32;
    /// Applies a control's change and ends the gesture when it is released, without a
    /// right-click menu (for areas that have their own).
    fn change(&mut self, id: &str, changed: Option<f32>, response: &Response);
    /// `change`, with the control's right-click menu and `more` items in it.
    fn set_with(
        &mut self,
        id: &str,
        changed: Option<f32>,
        response: &Response,
        more: &mut dyn FnMut(&mut Ui),
    );
    /// Ends the current gesture: one undo step for a click that changed a value.
    fn end_gesture(&mut self) {}
    /// Keeps the widgets of two editors of the same effect apart.
    fn salt(&self) -> u64 {
        0
    }
    /// A control of `id` was drawn (checked by AudioStrada's tests).
    fn shown(&self, _id: &str) {}
    /// Sets several parameters as one undo step named `label`.
    fn edit_many(&mut self, label: &str, changes: &[(String, f32)]);
    /// Plays parameter `id` at `value` for listening only, never saved; `None` gives it back.
    fn hear(&mut self, _id: &str, _value: Option<f32>) {}
    /// The sample rate the effect runs at.
    fn sample_rate(&self) -> u32;
    /// The tempo the delays sync to, in beats per minute.
    fn tempo(&self) -> f64;
    /// Tells the user something.
    fn notify(&mut self, _message: String) {}
}

impl dyn EditorParams + '_ {
    pub fn param(&self, id: &str) -> Option<&'static ParamInfo> {
        self.info().params.iter().find(|p| p.id == id)
    }

    /// Applies a control's change and ends the gesture when it is released.
    pub fn set(&mut self, id: &str, changed: Option<f32>, response: &Response) {
        self.set_with(id, changed, response, &mut |_| {});
    }

    /// Every value, in the order of the effect's parameters.
    pub fn values(&self) -> Vec<f32> {
        self.info().params.iter().map(|p| self.get(p.id)).collect()
    }

    /// Applies an on/off control drawn by `add`, which gets whether the switch is on.
    pub fn toggle(
        &mut self,
        ui: &mut Ui,
        id: &str,
        add: impl FnOnce(&mut Ui, &mut bool) -> Response,
    ) {
        let mut on = self.get(id) >= 0.5;
        let response = add(ui, &mut on);
        let changed = response.changed().then_some(if on { 1.0 } else { 0.0 });
        self.set(id, changed, &response);
    }
}

fn is_log(p: &ParamInfo) -> bool {
    matches!(p.unit, "Hz" | "ms") && p.min > 0.0
}

pub fn to_unit(p: &ParamInfo, v: f32) -> f32 {
    if is_log(p) {
        (v / p.min).ln() / (p.max / p.min).ln()
    } else {
        (v - p.min) / (p.max - p.min)
    }
}

pub fn from_unit(p: &ParamInfo, t: f32) -> f32 {
    if is_log(p) {
        p.min * (p.max / p.min).powf(t)
    } else {
        p.min + t * (p.max - p.min)
    }
}

pub fn format_value(p: &ParamInfo, v: f32) -> String {
    if let Some(name) = choice_name(p, v) {
        return name.to_owned();
    }
    match p.unit {
        "Hz" if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
        "Hz" if v < 10.0 => format!("{v:.2} Hz"),
        "Hz" if v < 100.0 => format!("{v:.1} Hz"),
        "Hz" => format!("{v:.0} Hz"),
        "ms" if v < 10.0 => format!("{v:.1} ms"),
        "ms" => format!("{v:.0} ms"),
        "dB" => format!("{v:+.1} dB"),
        ":1" => format!("{v:.1}:1"),
        "%" => format!("{v:.0} %"),
        // Whole steps: octaves, unison voices.
        "oct" => format!("{:+} oct", v.round() as i32),
        "voices" => format!("{}", v.round() as i32),
        "octaves" => format!("{} oct", v.round() as i32),
        "semi" => format!("{:+} st", v.round() as i32),
        "cents" => format!("{v:.0} cents"),
        "" => format!("{v:.2}"),
        unit => format!("{v:.1} {unit}"),
    }
}

pub fn choice_name(p: &ParamInfo, v: f32) -> Option<&'static str> {
    let i = (v - p.min).round();
    (!p.choices.is_empty() && i >= 0.0)
        .then(|| p.choices.get(i as usize).copied())
        .flatten()
        .map(tr)
}

/// Buttons side by side for a parameter with a few named values, a drop-down list for more.
pub fn choice_box(ui: &mut Ui, params: &mut dyn EditorParams, id: &str) {
    let Some(p) = params.param(id) else {
        return;
    };
    if p.choices.len() <= 4 {
        ui.horizontal(|ui| segmented(ui, params, id, &[]));
        return;
    }
    choice_list(ui, params, id);
}

/// A drop-down list of the choices, however few: for a narrow place.
pub fn choice_list(ui: &mut Ui, params: &mut dyn EditorParams, id: &str) {
    let Some(p) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let mut chosen = None;
    let response = egui::ComboBox::from_id_salt(("choice", params.salt(), id))
        .selected_text(choice_name(p, value).unwrap_or("?"))
        .show_ui(ui, |ui| {
            for (i, name) in p.choices.iter().enumerate() {
                let v = p.min + i as f32;
                if ui
                    .selectable_label((value - v).abs() < 0.5, tr(name))
                    .clicked()
                {
                    chosen = Some(v);
                }
            }
        })
        .response;
    params.set(id, chosen, &response);
    if chosen.is_some() {
        params.set(id, None, &response);
        params.end_gesture();
    }
}

/// A knob with its name above and value below; a list for named values.
pub fn param_knob(ui: &mut Ui, params: &mut dyn EditorParams, id: &str, size: f32, tint: Color32) {
    let Some(p) = params.param(id) else {
        return;
    };
    named_knob(ui, params, id, tr(p.name), size, tint);
}

/// A knob labelled `label` instead of its parameter's name.
pub fn named_knob(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    label: &str,
    size: f32,
    tint: Color32,
) {
    knob_with(ui, params, (id, label), size, tint, false, |_| {});
}

/// A knob as wide as its label and its longest value, which stay on one line.
pub fn wide_knob(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    label: &str,
    size: f32,
    tint: Color32,
) {
    knob_with(ui, params, (id, label), size, tint, true, |_| {});
}

/// `wide_knob`, with `more` items in its right-click menu.
pub fn wide_knob_with(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    (id, label): (&str, &str),
    size: f32,
    tint: Color32,
    more: impl FnOnce(&mut Ui),
) {
    knob_with(ui, params, (id, label), size, tint, true, more);
}

fn knob_with(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    (id, label): (&str, &str),
    size: f32,
    tint: Color32,
    wide: bool,
    more: impl FnOnce(&mut Ui),
) {
    let Some(p) = params.param(id) else {
        return;
    };
    if !p.choices.is_empty() {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(label)
                    .size(10.5)
                    .color(theme::current(ui).weak),
            );
            choice_box(ui, params, id);
        });
        return;
    }
    let value = params.get(id);
    let width = if wide {
        let text = |t: String, font: FontId| {
            ui.painter()
                .layout_no_wrap(t, font, Color32::WHITE)
                .size()
                .x
        };
        let values = [p.min, p.max, value]
            .into_iter()
            .map(|v| text(format_value(p, v), FontId::monospace(11.0)))
            .fold(0.0, f32::max);
        (size + 18.0)
            .max(text(label.to_owned(), FontId::proportional(10.5)) + 6.0)
            .max(values + 6.0)
    } else {
        size + 18.0
    };
    let draw = |ui: &mut Ui| {
        let mut name = egui::Label::new(
            RichText::new(label)
                .size(10.5)
                .color(theme::current(ui).weak),
        );
        if wide {
            name = name.extend();
        }
        ui.add(name);
        let bipolar = p.min < 0.0 && p.max > 0.0;
        let (changed, response) = widgets::knob(
            ui,
            to_unit(p, value),
            (0.0, 1.0),
            to_unit(p, p.default),
            size,
            bipolar,
            tint,
        );
        let changed = changed.map(|t| from_unit(p, t));
        let shown = changed.unwrap_or(value);
        let response = response.on_hover_text(crate::trf!(
            "{name}: {value}. Drag, Shift for fine, double-click to reset",
            name = tr(p.name),
            value = format_value(p, shown)
        ));
        let mut more = Some(more);
        params.set_with(id, changed, &response, &mut |ui| {
            if let Some(more) = more.take() {
                more(ui);
            }
        });
        let mut text =
            egui::Label::new(RichText::new(format_value(p, shown)).monospace().size(11.0));
        if wide {
            text = text.extend();
        }
        ui.add(text);
    };
    // In a centered column the knob sits in the middle, elsewhere at the cursor.
    let layout = ui.layout();
    if layout.is_vertical() && layout.horizontal_align() == egui::Align::Center {
        ui.allocate_ui_with_layout(
            vec2(width, 0.0),
            egui::Layout::top_down(egui::Align::Center),
            draw,
        );
    } else {
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.vertical_centered(draw);
        });
    }
}

/// The face of an analog meter: backlit amber, cream, or the theme's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VuFace {
    Amber,
    /// Amber, in a light bezel.
    Bright,
    Cream,
    Theme,
}

/// An analog VU meter showing gain reduction: cream face, needle with VU ballistics.
pub fn vu_meter(
    ui: &mut Ui,
    reduction: f32,
    needle: &mut f32,
    size: egui::Vec2,
    (face, caption): (VuFace, &str),
) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    // VU ballistics: about 300 ms to settle, from the frame time.
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let target = reduction.clamp(-20.0, 3.0);
    *needle += (target - *needle) * (1.0 - (-dt / 0.12).exp());
    // Face, ink and needle: backlit amber, or the theme's dark or cream.
    let (face_top, face_bottom, ink, red, hand) = if matches!(face, VuFace::Amber | VuFace::Bright)
    {
        (
            Color32::from_rgb(250, 206, 112),
            Color32::from_rgb(222, 152, 52),
            Color32::from_rgb(52, 34, 18),
            Color32::from_rgb(214, 46, 24),
            Color32::from_rgb(30, 22, 14),
        )
    } else if face == VuFace::Cream {
        (
            Color32::from_rgb(250, 246, 232),
            Color32::from_rgb(230, 222, 200),
            Color32::from_rgb(40, 36, 30),
            Color32::from_rgb(196, 48, 36),
            Color32::from_rgb(20, 18, 16),
        )
    } else if p.dark {
        (
            Color32::from_rgb(70, 74, 79),
            Color32::from_rgb(46, 49, 53),
            Color32::from_rgb(228, 230, 232),
            Color32::from_rgb(236, 92, 72),
            Color32::from_rgb(244, 244, 240),
        )
    } else {
        (
            Color32::from_rgb(246, 244, 238),
            Color32::from_rgb(228, 225, 216),
            Color32::from_rgb(30, 30, 30),
            Color32::from_rgb(210, 58, 40),
            Color32::from_rgb(24, 24, 24),
        )
    };
    // The bezel, then the face behind its glass.
    let (bezel_top, bezel_bottom) = if face == VuFace::Cream {
        (Color32::from_gray(44), Color32::from_gray(12))
    } else if p.dark && face != VuFace::Bright {
        (Color32::from_gray(58), Color32::from_gray(26))
    } else {
        (Color32::from_gray(212), Color32::from_gray(170))
    };
    theme::gradient_rect(&painter, rect, bezel_top, bezel_bottom);
    let face = rect.shrink(9.0);
    theme::gradient_rect(&painter, face, face_top, face_bottom);
    painter.rect_stroke(
        face,
        3.0,
        Stroke::new(1.5, Color32::from_black_alpha(140)),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter_at(face);
    // The scale as on a real VU: positions follow the voltage, -20 to +3 dB.
    let span = (10f32.powf(3.0 / 20.0) - 0.1).max(1e-3);
    let position = |db: f32| (10f32.powf(db / 20.0) - 0.1) / span;
    let radius = face.height() * 0.95;
    let pivot = pos2(face.center().x, face.top() + 20.0 + radius);
    let sweep = 0.82;
    let at = |db: f32, r: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + (position(db) - 0.5) * 2.0 * sweep;
        pivot + vec2(a.cos(), a.sin()) * r
    };
    let arc_r = radius * 0.8;
    let arc = |from: f32, to: f32, r: f32| -> Vec<Pos2> {
        (0..=48)
            .map(|i| at(from + (to - from) * i as f32 / 48.0, r))
            .collect()
    };
    painter.add(Shape::line(arc(-20.0, 0.0, arc_r), Stroke::new(1.4, ink)));
    painter.add(Shape::line(arc(0.0, 3.0, arc_r), Stroke::new(1.4, red)));
    // The red zone over 0, as a band.
    painter.add(Shape::line(
        arc(0.0, 3.0, arc_r + 4.0),
        Stroke::new(6.0, red),
    ));
    let big = FontId::proportional((face.height() * 0.075).clamp(9.0, 14.0));
    let small = FontId::proportional((face.height() * 0.058).clamp(8.0, 11.0));
    for (db, label) in [
        (-20.0, "-20"),
        (-10.0, "-10"),
        (-7.0, "-7"),
        (-5.0, "-5"),
        (-3.0, "-3"),
        (-2.0, "-2"),
        (-1.0, "-1"),
        (0.0, "0"),
        (1.0, "+1"),
        (2.0, "+2"),
        (3.0, "+3"),
    ] {
        let color = if db > 0.0 { red } else { ink };
        painter.line_segment(
            [at(db, arc_r), at(db, arc_r + 9.0)],
            Stroke::new(1.3, color),
        );
        painter.text(
            at(db, arc_r + 20.0),
            Align2::CENTER_CENTER,
            label,
            big.clone(),
            color,
        );
    }
    for db in [-15.0f32, -0.5, 0.5, 1.5, 2.5] {
        let color = if db > 0.0 { red } else { ink };
        painter.line_segment(
            [at(db, arc_r), at(db, arc_r + 5.0)],
            Stroke::new(1.0, color),
        );
    }
    // Percent of the reference level, inside the arc.
    for pct in [0.0f32, 20.0, 40.0, 60.0, 80.0, 100.0] {
        let db = 20.0 * (pct.max(10.0) / 100.0).log10();
        let db = if pct == 0.0 { -20.0 } else { db };
        painter.line_segment([at(db, arc_r - 5.0), at(db, arc_r)], Stroke::new(1.0, ink));
        painter.text(
            at(db, arc_r - 14.0),
            Align2::CENTER_CENTER,
            format!("{pct:.0}"),
            small.clone(),
            ink.gamma_multiply(0.85),
        );
    }
    for (x, align) in [
        (face.left() + 10.0, Align2::LEFT_BOTTOM),
        (face.right() - 10.0, Align2::RIGHT_BOTTOM),
    ] {
        painter.text(pos2(x, face.bottom() - 8.0), align, "VU", big.clone(), ink);
    }
    // The maker's mark and what it reads, in the middle.
    let mark = pos2(face.center().x, face.top() + face.height() * 0.6);
    let h = (face.height() * 0.09).clamp(10.0, 18.0);
    paint_logo(
        &painter,
        Rect::from_center_size(mark - vec2(0.0, h * 0.7), vec2(h * 105.0 / 96.0, h)),
    );
    painter.text(
        mark + vec2(0.0, 6.0),
        Align2::CENTER_CENTER,
        caption,
        small,
        ink.gamma_multiply(0.75),
    );
    // The needle from its pivot, under a cap at the bottom.
    let tip = at(*needle, arc_r + 12.0);
    painter.line_segment(
        [pivot, tip],
        Stroke::new(2.0, Color32::from_black_alpha(50)),
    );
    painter.line_segment([pivot, tip], Stroke::new(1.4, hand));
    let cap = pos2(face.center().x, face.bottom());
    painter.circle_filled(cap, face.height() * 0.11, bezel_bottom);
    painter.circle_stroke(
        cap,
        face.height() * 0.11,
        Stroke::new(1.0, Color32::from_black_alpha(120)),
    );
    // A light reflection on the glass.
    let mut glass = egui::Mesh::default();
    for (pos, alpha) in [
        (face.left_top(), 30u8),
        (face.right_top(), 6),
        (pos2(face.right(), face.top() + face.height() * 0.45), 0),
        (pos2(face.left(), face.top() + face.height() * 0.45), 8),
    ] {
        glass.colored_vertex(pos, Color32::from_white_alpha(alpha));
    }
    glass.add_triangle(0, 1, 2);
    glass.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(glass));
    animate(ui.ctx());
}

/// A switch as a pill: its light and its name, lit in the accent while on.
pub fn pill_switch(ui: &mut Ui, on: bool, label: &str) -> Response {
    let p = theme::current(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(11.5), p.text);
    let size = vec2(galley.size().x + 34.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    let fill = match (on, response.hovered()) {
        (true, _) => p.accent.gamma_multiply(0.3),
        (false, true) => p.hover,
        (false, false) => p.surface,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 11.0, fill);
    painter.rect_stroke(
        rect,
        11.0,
        Stroke::new(1.0, if on { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    let light = pos2(rect.left() + 13.0, rect.center().y);
    painter.circle_filled(light, 4.0, theme::power_ink(on, &p));
    painter.galley(
        pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        if on {
            p.text
        } else {
            p.text.gamma_multiply(0.8)
        },
    );
    if response.clicked() {
        response.mark_changed();
    }
    response
}

/// One button per named value of a parameter, the current one lit.
pub fn segmented(ui: &mut Ui, params: &mut dyn EditorParams, id: &str, hints: &[&'static str]) {
    let Some(p) = params.param(id) else {
        return;
    };
    params.shown(id);
    let value = params.get(id);
    ui.spacing_mut().item_spacing.x = 2.0;
    for (i, name) in p.choices.iter().enumerate() {
        let v = p.min + i as f32;
        let mut response = ui.selectable_label((value - v).abs() < 0.5, tr(name));
        if let Some(hint) = hints.get(i) {
            response = response.on_hover_text(tr(hint));
        }
        if response.clicked() {
            params.set(id, Some(v), &response);
            params.end_gesture();
        }
    }
}

/// The dark screen behind the pads and the pad's editor.
pub const SCREEN: Color32 = Color32::from_rgb(16, 19, 25);

/// The dark screen filling the width, with light controls whatever the theme.
pub fn screen(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(SCREEN)
        .corner_radius(10.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            let selection = ui.visuals().selection;
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().selection = selection;
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Modules side by side across the width, each as wide as its knobs, titled in `tint`; the
/// place of each control, in order, from its top to the bottom of what it drew.
pub fn modules(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    list: &[(&str, Vec<(String, &str)>)],
    tint: Color32,
    size: f32,
) -> Vec<Rect> {
    let p = theme::current(ui);
    let gap = 8.0;
    let mut places = Vec::new();
    let full = ui.available_width();
    let count: usize = list.iter().map(|m| m.1.len()).sum();
    let unit = (full - gap * (list.len().max(1) - 1) as f32) / count.max(1) as f32;
    let top = ui.cursor().min;
    let backs: Vec<_> = list.iter().map(|_| ui.painter().add(Shape::Noop)).collect();
    let (mut x, mut bottom) = (top.x, top.y);
    let mut rects = Vec::new();
    for (title, knobs) in list {
        let width = unit * knobs.len() as f32;
        let rect = Rect::from_min_size(pos2(x, top.y), vec2(width, 0.0));
        let inner = rect.shrink2(vec2(8.0, 0.0)).translate(vec2(0.0, 8.0));
        let builder = egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(inner.min, vec2(inner.width(), 400.0)))
            .layout(egui::Layout::top_down(egui::Align::Min));
        let used = ui
            .scope_builder(builder, |ui| {
                ui.label(RichText::new(*title).strong().size(12.0).color(tint));
                let row = ui.cursor().min;
                let cell = inner.width() / knobs.len() as f32;
                let mut low = row.y;
                for (k, (id, label)) in knobs.iter().enumerate() {
                    let place = Rect::from_min_size(
                        pos2(row.x + k as f32 * cell, row.y),
                        vec2(cell, 200.0),
                    );
                    let builder = egui::UiBuilder::new()
                        .max_rect(place)
                        .layout(egui::Layout::top_down(egui::Align::Center));
                    let choice = params.param(id).is_some_and(|p| !p.choices.is_empty());
                    let toggle = params.param(id).is_some_and(|p| p.toggle);
                    let shown = ui
                        .scope_builder(builder, |ui| {
                            if toggle {
                                // A switch in a knob's place, level with the knobs' centers.
                                ui.add_space(size / 2.0 + 2.0);
                                let on = params.get(id) >= 0.5;
                                let response = pill_switch(ui, on, label);
                                let changed =
                                    response.clicked().then_some(if on { 0.0 } else { 1.0 });
                                params.set(id, changed, &response);
                                if changed.is_some() {
                                    params.end_gesture();
                                }
                            } else if choice {
                                // A list in a knob's place, level with the knobs' centers.
                                ui.label(RichText::new(*label).size(10.5).color(p.weak));
                                ui.add_space(size / 2.0 - 12.0);
                                choice_box(ui, params, id);
                            } else {
                                named_knob(ui, params, id, label, size, tint);
                            }
                        })
                        .response
                        .rect;
                    low = low.max(shown.bottom());
                    places.push(Rect::from_min_max(
                        place.min,
                        pos2(place.right(), shown.bottom()),
                    ));
                }
                ui.allocate_rect(
                    Rect::from_min_max(row, pos2(row.x + inner.width(), low)),
                    Sense::hover(),
                );
            })
            .response
            .rect;
        bottom = bottom.max(used.bottom() + 8.0);
        rects.push(rect);
        x += width + gap;
    }
    for (rect, back) in rects.into_iter().zip(backs) {
        let rect = Rect::from_min_max(rect.min, pos2(rect.right(), bottom));
        ui.painter()
            .set(back, Shape::rect_filled(rect, 8.0, MODULE));
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + full, bottom)),
        Sense::hover(),
    );
    places
}

/// The panels inside the screen.
pub const MODULE: Color32 = Color32::from_rgb(25, 29, 38);

/// AudioStrada's mark (packaging/icons/audiostrada-mark.svg) drawn as shapes, sharp at any
/// size: five slanted rounded bars, centered in `rect` at their own proportions.
pub fn paint_logo(painter: &egui::Painter, rect: egui::Rect) {
    use egui::{Color32, Pos2};
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

/// Redraws a window that moves on its own (meters, scopes, a held key). On Windows at the
/// main window's rate: an editor redrawn nonstop on another screen held back the main
/// window's redraws, the timeline frozen while it played. Elsewhere at every frame, as before.
pub fn animate(ctx: &egui::Context) {
    if cfg!(windows) {
        ctx.request_repaint_after(crate::REFRESH);
    } else {
        ctx.request_repaint();
    }
}

/// A titled panel of controls, `height` tall at least.
pub fn section_frame(
    ui: &mut Ui,
    title: &str,
    color: Color32,
    height: f32,
    add: impl FnOnce(&mut Ui),
) {
    let p = theme::current(ui);
    // Sections side by side share a row: as tall as the tallest, the free width shared among
    // them; stacked, each takes the whole width. Sizes come from the previous frame, each
    // section's own (natural) size measured without what the row gave it.
    let layout = *ui.layout();
    let in_row = layout.main_dir().is_horizontal() && !layout.main_wrap;
    let row = ui.scope_id().with("section-row");
    let frame_nr = ui.ctx().cumulative_frame_nr();
    let spacing = ui.spacing().item_spacing.x;
    let available = ui.available_width();
    let (index, given) = ui.ctx().data_mut(|d| {
        let memo = d.get_temp_mut_or_default::<SectionRow>(row);
        if memo.frame != frame_nr {
            memo.frame = frame_nr;
            memo.last = std::mem::take(&mut memo.sizes);
            memo.last_titles = std::mem::take(&mut memo.titles);
            memo.start = available;
            memo.changed = false;
        }
        let index = memo.sizes.len();
        memo.sizes.push(egui::Vec2::ZERO);
        memo.titles.push(title.to_owned());
        // A row whose sections changed (a band picked, shown or gone) shares nothing out
        // until they are measured: the free room counted for the old ones would be given
        // to each new one, and the window would follow, several times too wide.
        memo.changed |= memo.last_titles.get(index).is_none_or(|t| t != title);
        let natural = memo.last.get(index).copied().unwrap_or_default();
        let given = if in_row && memo.changed {
            egui::Vec2::ZERO
        } else if in_row && !memo.last.is_empty() {
            let used: f32 =
                memo.last.iter().map(|v| v.x).sum::<f32>() + spacing * (memo.last.len() - 1) as f32;
            let extra = ((memo.start - used) / memo.last.len() as f32).max(0.0);
            let tallest = memo.last.iter().map(|v| v.y).fold(0.0f32, f32::max);
            egui::vec2(natural.x + extra, tallest)
        } else if in_row {
            // The first frame of a row: each at its own size until they are measured.
            egui::Vec2::ZERO
        } else {
            egui::vec2(available, natural.y)
        };
        (index, given)
    });
    let margin = egui::Margin::symmetric(10, 8);
    let pad = margin.sum();
    let mut natural = egui::Vec2::ZERO;
    let (fill, edge) = theme::card(&p);
    egui::Frame::new()
        .fill(fill)
        .stroke(edge)
        .corner_radius(8.0)
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.set_min_height(height.max(given.y - pad.y));
            ui.set_min_width((given.x - pad.x).max(0.0));
            let inner = ui.vertical(|ui| {
                ui.label(RichText::new(title).strong().size(11.5).color(color));
                ui.add_space(2.0);
                add(ui);
            });
            natural = inner.response.rect.size() + pad;
        });
    ui.ctx().data_mut(|d| {
        let memo = d.get_temp_mut_or_default::<SectionRow>(row);
        if let Some(slot) = memo.sizes.get_mut(index) {
            *slot = egui::vec2(natural.x, natural.y.max(height + pad.y));
        }
    });
}

/// The sections of a row as they were last drawn, and the row's width.
#[derive(Clone, Debug, Default)]
struct SectionRow {
    frame: u64,
    start: f32,
    sizes: Vec<egui::Vec2>,
    last: Vec<egui::Vec2>,
    titles: Vec<String>,
    last_titles: Vec<String>,
    changed: bool,
}

/// Hears band `band` alone, or all of them again (EQ, multiband); never saved.
pub fn solo_band(params: &mut dyn EditorParams, band: Option<usize>) {
    params.hear("solo", band.map(|b| (b + 1) as f32));
}

/// The area between a polyline and the horizontal line at `bottom`.
pub fn fill_under(points: &[Pos2], bottom: f32, color: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    for pair in points.windows(2) {
        let i = mesh.vertices.len() as u32;
        for q in [
            pair[0],
            pair[1],
            pos2(pair[1].x, bottom),
            pos2(pair[0].x, bottom),
        ] {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    Shape::mesh(mesh)
}
