//! The rack: the chain of the selected track (or of a pinned channel) as modules side by side,
//! each with its main controls, A/B and its editor a click away; parallel chains split the
//! signal into branches of their own and mix them back.

use std::collections::BTreeMap;

use super::*;

/// Width of an effect's module.
const MODULE: f32 = 268.0;
/// Height of the modules' faceplates.
const HEIGHT: f32 = 180.0;
/// Width of the chassis' sides.
const EAR: f32 = 16.0;
/// Height of the channel strip's sections, inside their frames.
const SECTION: f32 = 92.0;
/// Height of the rails above and below the modules.
const RAIL: f32 = 14.0;
/// Width of the gaps between modules, with their add button.
const GAP: f32 = 34.0;
/// Width of a parallel chain's module.
const PARALLEL: f32 = 470.0;
/// Width of the empty slot at the end of the rack.
const EMPTY: f32 = 132.0;
/// Height of a module's footer: A/B and the editor's button.
const FOOT: f32 = 30.0;
/// Columns of knobs on a module's face, at most.
const COLUMNS: usize = 4;

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
    /// The channel strip's unit folded to one line.
    pub strip_folded: bool,
    /// Module of the channel strip being dragged to another place.
    strip_drag: Option<usize>,
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

/// The parameters a plugin's face shows: the ones chosen, else its dry/wet mix when it has one.
fn shown_macros(
    insert: &audiostrada_core::Insert,
    params: &[audiostrada_plugins::ParamDesc],
) -> Vec<String> {
    if !insert.macros.is_empty() {
        return insert.macros.clone();
    }
    let mix = |name: &str| {
        let name = name.to_lowercase();
        [
            "mix",
            "dry/wet",
            "dry / wet",
            "wet/dry",
            "drywet",
            "dry wet",
            "blend",
        ]
        .iter()
        .any(|w| {
            name == *w || name.starts_with(&format!("{w} ")) || name.ends_with(&format!(" {w}"))
        })
    };
    params
        .iter()
        .find(|p| mix(&p.name))
        .map(|p| vec![p.id.to_string()])
        .unwrap_or_default()
}

/// An add menu, opened where it was clicked: a plain toggle would leave it no place to show.
fn add_popup(response: &Response) -> egui::Popup<'_> {
    egui::Popup::menu(response).at_pointer_fixed().open_memory(
        response
            .clicked()
            .then_some(egui::SetOpenCommand::Bool(true)),
    )
}

/// A small dark screen, as the hardware's displays.
pub(super) fn lcd_box(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(13, 15, 19));
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, Color32::from_black_alpha(160)),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, Color32::from_white_alpha(22)),
    );
}

/// `count` columns side by side in `face`, at most [`COLUMNS`] to a row; two rows past it.
fn cells(face: Rect, count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let rows = count.div_ceil(COLUMNS);
    let columns = count.div_ceil(rows);
    let (w, h) = (face.width() / columns as f32, face.height() / rows as f32);
    (0..count)
        .map(|k| {
            let (row, column) = (k / columns, k % columns);
            Rect::from_min_size(
                face.min + vec2(column as f32 * w, row as f32 * h),
                vec2(w, h),
            )
        })
        .collect()
}

/// A text cut to `width` with an ellipsis, centered on `at`.
fn cut_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    text: &str,
    width: f32,
    size: f32,
    color: Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        egui::FontId::proportional(size),
        color,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    painter.galley(pos2(at.x - galley.size().x / 2.0, at.y), galley, color);
}

/// One parameter on a face, in its cell: its name cut to the cell, its knob, and its value on
/// a small screen (only under the name when the cell is short, two rows of them).
fn knob_cell(
    ui: &mut Ui,
    cell: Rect,
    (name, value_text): (&str, &str),
    knob: impl FnOnce(&mut Ui, f32) -> Response,
    tint: Color32,
) -> Response {
    let p = theme::current(ui);
    let painter = ui.painter_at(cell);
    let compact = cell.height() < 80.0;
    let size = if compact { 26.0 } else { 34.0 };
    let block = if compact { 54.0 } else { 78.0 };
    let top = cell.top() + ((cell.height() - block) / 2.0).max(0.0);
    cut_text(
        &painter,
        pos2(cell.center().x, top),
        name,
        cell.width() - 6.0,
        if compact { 9.0 } else { 10.0 },
        p.weak,
    );
    let place = Rect::from_center_size(
        pos2(cell.center().x, top + 14.0 + size / 2.0),
        vec2(size, size),
    );
    // Its scale, printed around it as on the hardware.
    let (start, sweep) = (std::f32::consts::PI * 0.75, std::f32::consts::PI * 1.5);
    let ring = size / 2.0 + 4.0;
    for k in 0..=10 {
        let a = start + sweep * k as f32 / 10.0;
        let dot = place.center() + vec2(a.cos(), a.sin()) * ring;
        let big = k == 0 || k == 5 || k == 10;
        painter.circle_filled(
            dot,
            if big { 1.1 } else { 0.7 },
            p.weak.gamma_multiply(if big { 0.8 } else { 0.5 }),
        );
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(place));
    let response = knob(&mut child, size);
    let value_top = place.bottom() + 4.0;
    if compact {
        cut_text(
            &painter,
            pos2(cell.center().x, value_top),
            value_text,
            cell.width() - 4.0,
            9.0,
            theme::mix(p.text, p.weak, 0.3),
        );
    } else {
        let screen = Rect::from_center_size(
            pos2(cell.center().x, value_top + 8.0),
            vec2((cell.width() - 8.0).min(68.0), 16.0),
        );
        lcd_box(&painter, screen);
        cut_text(
            &painter,
            pos2(screen.center().x, screen.top() + 2.0),
            value_text,
            screen.width() - 6.0,
            10.0,
            theme::mix(tint, Color32::WHITE, 0.45),
        );
    }
    response
}

/// A built-in effect's parameter in a face's cell.
fn face_knob(
    ui: &mut Ui,
    params: &mut effect_editors::Params,
    id: &str,
    cell: Rect,
    tint: Color32,
) {
    use effect_editors::{format_value, from_unit, to_unit};
    let Some(p) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let mut changed = None;
    let response = knob_cell(
        ui,
        cell,
        (tr(p.name), &format_value(p, value)),
        |ui, size| {
            let bipolar = p.min < 0.0 && p.max > 0.0;
            let (moved, response) = widgets::knob(
                ui,
                to_unit(p, value),
                (0.0, 1.0),
                to_unit(p, p.default),
                size,
                bipolar,
                tint,
            );
            changed = moved.map(|t| from_unit(p, t));
            response
        },
        tint,
    );
    let shown = changed.unwrap_or(value);
    let response = response.on_hover_text(trf!(
        "{name}: {value}. Drag, Shift for fine, double-click to reset",
        name = tr(p.name),
        value = format_value(p, shown)
    ));
    params.set(id, changed, &response);
}

/// The editor's button of a module, at the right of its footer.
fn edit_button(ui: &mut Ui, rect: Rect, salt: u64) -> Response {
    let p = theme::current(ui);
    let response = ui.interact(
        rect,
        ui.scope_id().with(("rack-edit", salt)),
        Sense::click(),
    );
    effect_frame::paint_face(ui, rect, 4.0, &response, effect_frame::Face::Plain);
    let ink = if response.hovered() {
        p.text
    } else {
        theme::mix(p.text, p.weak, 0.3)
    };
    let glyph = Rect::from_center_size(rect.left_center() + vec2(12.0, 0.0), vec2(12.0, 12.0));
    effect_frame::paint_glyph(ui.painter(), glyph, effect_frame::Glyph::External, ink);
    cut_text(
        ui.painter(),
        pos2(rect.center().x + 8.0, rect.center().y - 7.0),
        tr("Edit"),
        rect.width() - 26.0,
        11.0,
        ink,
    );
    response
}

/// The response of an EQ module's bands, on a screen in its face.
fn eq_curve(ui: &mut Ui, values: &[f32], tint: Color32, rect: Rect) {
    let painter = ui.painter_at(rect);
    lcd_box(&painter, rect);
    let x_of = |f: f32| rect.left() + rect.width() * (f / 20.0).log10() / 3.0;
    let y_of = |db: f32| rect.center().y - db / 18.0 * rect.height() * 0.42;
    let grid = egui::Stroke::new(1.0, Color32::from_white_alpha(14));
    for db in [-12.0f32, -6.0, 6.0, 12.0] {
        painter.line_segment(
            [pos2(rect.left(), y_of(db)), pos2(rect.right(), y_of(db))],
            grid,
        );
    }
    painter.line_segment(
        [pos2(rect.left(), y_of(0.0)), pos2(rect.right(), y_of(0.0))],
        egui::Stroke::new(1.0, Color32::from_white_alpha(32)),
    );
    for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        let x = x_of(f);
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], grid);
        painter.text(
            pos2(x + 3.0, rect.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(8.5),
            Color32::from_white_alpha(70),
        );
    }
    let offsets = [0.0; audiostrada_dsp::eq::BANDS];
    let points: Vec<egui::Pos2> = (0..=90)
        .map(|i| {
            let t = i as f32 / 90.0;
            let freq = 20.0 * 1000f64.powf(f64::from(t));
            let db = audiostrada_dsp::eq::response_db(values, &offsets, freq, 48_000.0)
                .clamp(-18.0, 18.0) as f32;
            pos2(rect.left() + rect.width() * t, y_of(db))
        })
        .collect();
    // The area under the curve, faintly lit, then the curve.
    let base = y_of(0.0);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        painter.add(egui::Shape::convex_polygon(
            vec![a, b, pos2(b.x, base), pos2(a.x, base)],
            tint.gamma_multiply(0.12),
            egui::Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.8, tint)));
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

/// Rails above and below a column of the rack, and the inside between them, in the theme's
/// tones.
fn rails(ui: &Ui, rect: Rect) {
    let p = theme::current(ui);
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.left(), rect.top() + RAIL),
            pos2(rect.right(), rect.bottom() - RAIL),
        ),
        0.0,
        p.base,
    );
    for rail in [
        Rect::from_min_size(rect.min, vec2(rect.width(), RAIL)),
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - RAIL),
            vec2(rect.width(), RAIL),
        ),
    ] {
        theme::gradient_rect(&painter, rail, p.hover, p.surface);
        crate::gear::bevel(&painter, rail, 0.35);
        // Its row of mounting holes, on a fixed pitch so neighbours line up.
        let pitch = 18.0;
        let mut x = (rail.left() / pitch).ceil() * pitch + pitch / 2.0;
        while x < rail.right() - 4.0 {
            let hole = Rect::from_center_size(pos2(x, rail.center().y), vec2(7.0, 4.0));
            painter.rect_filled(hole, 2.0, theme::mix(p.base, Color32::BLACK, 0.4));
            painter.line_segment(
                [hole.left_bottom(), hole.right_bottom()],
                egui::Stroke::new(1.0, Color32::from_white_alpha(18)),
            );
            x += pitch;
        }
    }
}

/// A screw holding a faceplate, its slot turned a little.
fn screw(painter: &egui::Painter, at: egui::Pos2, p: &theme::Palette) {
    painter.circle_filled(at + vec2(0.0, 0.6), 3.6, Color32::from_black_alpha(90));
    let head = if p.dark {
        theme::mix(p.hover, Color32::WHITE, 0.12)
    } else {
        theme::mix(p.surface, p.border, 0.25)
    };
    painter.circle_filled(at, 3.0, head);
    painter.circle_stroke(
        at,
        3.0,
        egui::Stroke::new(0.8, Color32::from_black_alpha(130)),
    );
    painter.line_segment(
        [at + vec2(-1.9, 1.3), at + vec2(1.9, -1.3)],
        egui::Stroke::new(1.0, Color32::from_black_alpha(170)),
    );
}

/// A module's faceplate: brushed metal in the theme's tone, lit from above, its family's color
/// along its top glowing down its header, held by four screws; its edge takes the color under
/// the pointer.
fn faceplate(ui: &Ui, rect: Rect, tint: Color32, lit: bool, hot: bool) {
    use crate::gear;
    let p = theme::current(ui);
    let painter = ui.painter_at(rect.expand(8.0));
    for k in 0..3u8 {
        let grow = 1.0 + f32::from(k) * 2.0;
        painter.rect_filled(
            rect.translate(vec2(0.0, 2.0)).expand(grow),
            4.0 + grow,
            Color32::from_black_alpha(46 - k * 14),
        );
    }
    let metal = if p.dark {
        theme::mix(p.surface, p.panel, 0.45)
    } else {
        theme::mix(p.surface, p.panel, 0.3)
    };
    painter.rect_filled(rect, 4.0, metal);
    let inner = rect.shrink(1.5);
    let clipped = painter.with_clip_rect(inner);
    gear::tile(
        &clipped,
        inner,
        gear::Texture::Brushed,
        1.0,
        Color32::from_white_alpha(if p.dark { 10 } else { 34 }),
    );
    gear::sheen(
        &clipped,
        inner,
        Color32::from_white_alpha(if p.dark { 14 } else { 70 }),
        Color32::from_black_alpha(if p.dark { 46 } else { 14 }),
    );
    let strength = if lit { 1.0 } else { 0.3 };
    gear::sheen(
        &clipped,
        Rect::from_min_size(inner.min, vec2(inner.width(), 26.0)),
        tint.gamma_multiply(0.22 * strength),
        Color32::TRANSPARENT,
    );
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)),
        egui::CornerRadius {
            nw: 4,
            ne: 4,
            sw: 0,
            se: 0,
        },
        tint.gamma_multiply(strength),
    );
    gear::bevel(&painter, rect, if p.dark { 0.55 } else { 0.35 });
    let edge = if hot {
        theme::mix(p.border, tint, 0.7)
    } else {
        p.border
    };
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    for at in [
        pos2(rect.left() + 7.0, rect.top() + 12.0),
        pos2(rect.right() - 7.0, rect.top() + 12.0),
        pos2(rect.left() + 7.0, rect.bottom() - 9.0),
        pos2(rect.right() - 7.0, rect.bottom() - 9.0),
    ] {
        screw(&painter, at, &p);
    }
}

/// The button removing a module, in its header at the right, shown while the module is under
/// the pointer.
fn close_button(ui: &mut Ui, plate: Rect, salt: u64) -> bool {
    let p = theme::current(ui);
    let rect = Rect::from_center_size(plate.right_top() + vec2(-26.0, 19.0), vec2(18.0, 18.0));
    if !ui.rect_contains_pointer(plate.expand(2.0)) {
        return false;
    }
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("rack-remove", salt)),
            Sense::click(),
        )
        .on_hover_text(tr("Remove from the rack"));
    let painter = ui.painter();
    let ink = if response.hovered() {
        painter.circle_filled(rect.center(), 9.0, ERROR);
        Color32::WHITE
    } else {
        p.weak
    };
    let r = rect.shrink(5.5);
    let stroke = egui::Stroke::new(1.6, ink);
    painter.line_segment([r.left_top(), r.right_bottom()], stroke);
    painter.line_segment([r.right_top(), r.left_bottom()], stroke);
    response.clicked()
}

/// A side of the chassis, from the top of the channel strip to the bottom rail.
fn chassis_side(ui: &Ui, rect: Rect) {
    let p = theme::current(ui);
    theme::gradient_rect(&ui.painter_at(rect), rect, p.hover, p.surface);
}

/// A plain faceplate: the background, border and family color of the modules.
fn faceplate_plain(ui: &Ui, rect: Rect, tint: Color32, lit: bool) {
    let p = theme::current(ui);
    let painter = ui.painter_at(rect.expand(1.0));
    let fill = if p.dark {
        theme::mix(p.panel, p.surface, 0.3)
    } else {
        theme::mix(p.panel, p.surface, 0.45)
    };
    if !p.look.gloss {
        painter.rect_filled(rect, 2.0, fill);
    } else {
        let face = p.look.face.unwrap_or(fill);
        painter.extend(theme::face(rect, 2.0, face, &p));
    }
    painter.rect_stroke(
        rect,
        2.0,
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)),
        0.0,
        tint.gamma_multiply(if lit { 1.0 } else { 0.35 }),
    );
}

/// The name of a channel strip section.
fn section_title(module: &str) -> &'static str {
    match module {
        "filters" => tr("Filters"),
        "gate" => tr("Gate"),
        "comp" => tr("Compressor"),
        "eq" => tr("EQ"),
        _ => tr("Drive"),
    }
}

/// The response of the channel strip's filters and EQ.
fn strip_curve(ui: &mut Ui, values: &[f32], width: f32) {
    use audiostrada_dsp::Effect;
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width.max(160.0), SECTION + 4.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, p.base);
    let x_of = |f: f32| rect.left() + rect.width() * (f / 20.0).log10() / 3.0;
    let y_of = |db: f32| rect.center().y - db / 18.0 * rect.height() * 0.45;
    for db in [-12.0f32, -6.0, 0.0, 6.0, 12.0] {
        painter.line_segment(
            [pos2(rect.left(), y_of(db)), pos2(rect.right(), y_of(db))],
            egui::Stroke::new(
                1.0,
                p.weak.gamma_multiply(if db == 0.0 { 0.3 } else { 0.1 }),
            ),
        );
    }
    for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        let x = x_of(f);
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, p.weak.gamma_multiply(0.1)),
        );
        painter.text(
            pos2(x + 3.0, rect.bottom() - 2.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(8.5),
            p.weak,
        );
    }
    let mut dsp = audiostrada_dsp::channel::Channel::default();
    dsp.prepare(48_000, 64);
    for (i, v) in values.iter().enumerate() {
        dsp.set_param(i, *v);
    }
    let points: Vec<egui::Pos2> = (0..=160)
        .map(|i| {
            let t = i as f32 / 160.0;
            let db = dsp
                .eq_response_db(20.0 * 1000f64.powf(f64::from(t)))
                .clamp(-18.0, 18.0) as f32;
            pos2(rect.left() + rect.width() * t, y_of(db))
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.8, p.accent)));
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
        crate::gear::glow(
            painter,
            center,
            13.0,
            theme::PLAY.gamma_multiply(0.35),
            Color32::TRANSPARENT,
        );
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
            .filter(|&t| project.track(t).is_some_and(|t| t.kind.carries_audio()));
        // A clip with effects of its own, selected alone on the selected track: its chain.
        let clip = doc
            .focused_clip()
            .filter(|&c| {
                audiostrada_core::edit::clip(&project, c).is_some_and(|c| !c.inserts.is_empty())
            })
            .filter(|&c| {
                let track = audiostrada_core::edit::track_of_clip(&project, c);
                selected.is_none() || track == selected
            })
            .map(Channel::Clip);
        let channel = self
            .rack
            .pin
            .or(clip)
            .or(selected.map(Channel::Track))
            .or(doc.is_master_selected().then_some(Channel::Master))
            .filter(|c| match c {
                Channel::Track(t) => project.track(*t).is_some(),
                Channel::Master => true,
                Channel::Clip(id) => audiostrada_core::edit::clip(&project, *id).is_some(),
            });
        self.rack_header(ui, &doc, &project, channel);
        ui.add_space(4.0);
        let Some(channel) = channel else {
            ui.weak(tr("Select a track to see its effects here."));
            return;
        };
        self.rack.gaps.clear();
        // Dragging across the modules moves them, it never selects their text.
        ui.style_mut().interaction.selectable_labels = false;
        // One chassis: the channel strip's unit on top, the modules under it; it scrolls when
        // the panel is too low.
        egui::ScrollArea::vertical()
            .id_salt("rack-vertical")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.strip_unit(ui, &mut doc, channel);
                self.rack_row(ui, &mut doc, &project, channel);
            });
        self.rack_drop(ui, &mut doc, channel);
    }

    /// The modules between the chassis' sides, the free slot stretched to its right side.
    fn rack_row(&mut self, ui: &mut Ui, doc: &mut Document, project: &Project, channel: Channel) {
        let doc = &mut *doc;
        let right = ui.max_rect().right();
        egui::ScrollArea::horizontal()
            .id_salt("rack")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (side, _) =
                        ui.allocate_exact_size(vec2(EAR, HEIGHT + 2.0 * RAIL), Sense::hover());
                    chassis_side(ui, side);
                    let shown: Vec<audiostrada_core::Insert> = routing::inserts(project, channel)
                        .iter()
                        .filter(|i| !i.monitor && i.effect != routing::STRIP)
                        .cloned()
                        .collect();
                    for (k, insert) in shown.iter().enumerate() {
                        let at = listed_index(project, channel, false, k);
                        self.rack_gap(ui, doc, channel, (None, at));
                        if insert.effect == routing::PARALLEL {
                            self.parallel_module(ui, doc, channel, insert);
                        } else {
                            self.module(ui, doc, channel, insert);
                        }
                    }
                    let end = listed_index(project, channel, false, shown.len());
                    self.rack_gap(ui, doc, channel, (None, end));
                    let width = (right - ui.cursor().left() - EAR).max(EMPTY);
                    self.empty_slot(ui, doc, channel, end, width);
                    let (side, _) =
                        ui.allocate_exact_size(vec2(EAR, HEIGHT + 2.0 * RAIL), Sense::hover());
                    chassis_side(ui, side);
                });
            });
    }

    /// A column of the rack `width` wide: rails, a faceplate and what `add` puts on it.
    fn slot(
        &mut self,
        ui: &mut Ui,
        (width, insert): (f32, InsertId),
        (tint, lit): (Color32, bool),
        add: impl FnOnce(&mut Self, &mut Ui),
    ) -> Rect {
        let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT + 2.0 * RAIL), Sense::hover());
        rails(ui, rect);
        let plate = Rect::from_min_max(
            pos2(rect.left() + 2.0, rect.top() + RAIL - 3.0),
            pos2(rect.right() - 2.0, rect.bottom() - RAIL + 3.0),
        );
        let hot = ui.rect_contains_pointer(plate) || self.rack.drag == Some(insert);
        faceplate(ui, plate, tint, lit, hot);
        // The whole plate is a handle: its controls, laid over it, keep their own gestures.
        let handle = ui
            .interact(
                plate,
                ui.scope_id().with(("rack-plate", insert.0)),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Grab);
        if handle.drag_started() {
            self.rack.drag = Some(insert);
        }
        let inner = Rect::from_min_max(plate.min + vec2(16.0, 9.0), plate.max - vec2(16.0, 8.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        add(self, &mut child);
        plate
    }

    /// The channel strip on its own 1U unit above the modules: its sections in the order they
    /// play (dragged by their grip to reorder them), each with its switch and main controls,
    /// the output and the curve; folded, one line.
    fn strip_unit(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel) {
        let p = theme::current(ui);
        let folded = self.rack.strip_folded;
        let height = if folded { 30.0 } else { 144.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let tint = theme::family_color("EQ & Filter");
        let strip = routing::strip(doc.project(), channel).cloned();
        let lit = strip.as_ref().is_some_and(|s| !s.bypass);
        // The chassis' sides at both ends, as the modules' below.
        for ear in [
            Rect::from_min_size(rect.min, vec2(EAR, rect.height())),
            Rect::from_min_size(
                pos2(rect.right() - EAR, rect.top()),
                vec2(EAR, rect.height()),
            ),
        ] {
            chassis_side(ui, ear);
        }
        let plate = Rect::from_min_max(rect.min + vec2(EAR, 0.0), rect.max - vec2(EAR, 0.0));
        faceplate_plain(ui, plate, tint, lit);
        let inner = plate.shrink2(vec2(10.0, 6.0)).translate(vec2(0.0, 2.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::Min)),
        );
        let ui = &mut child;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let arrow = if folded { "▸" } else { "▾" };
            if ui
                .add(egui::Button::new(arrow).frame(false))
                .on_hover_text(if folded {
                    tr("Unfold the channel strip")
                } else {
                    tr("Fold the channel strip")
                })
                .clicked()
            {
                self.rack.strip_folded = !folded;
            }
            if let Some(strip) = &strip
                && power_led(ui, !strip.bypass)
                    .on_hover_text(tr("Bypass or play"))
                    .clicked()
            {
                let (id, bypass) = (strip.id, !strip.bypass);
                doc.history_mut().edit(tr("Bypass insert"), |p| {
                    routing::set_bypass(p, channel, id, bypass)
                });
            }
            ui.label(
                RichText::new(tr("CHANNEL STRIP"))
                    .strong()
                    .size(11.0)
                    .color(p.text),
            );
            // The console it plays through.
            let console = crate::console_ui::of_channel(doc, channel);
            let mut text = RichText::new(trf!(
                "Console: {name}",
                name = crate::console_ui::name(console.model)
            ))
            .size(11.0);
            if console.colors() {
                text = text.color(crate::console_ui::color(console.model));
            }
            let button = ui.add(egui::Button::new(text).frame(false));
            egui::Popup::menu(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| match channel {
                    Channel::Track(t) => crate::console_ui::track_menu(ui, doc, t),
                    Channel::Master => crate::console_ui::console_menu(ui, doc),
                    Channel::Clip(_) => {}
                });
            if folded {
                // The modules on, as on the strips.
                for (k, on) in strip_modules(doc.project(), channel) {
                    let (label, _, family) = STRIP_MODULES[k];
                    let color = theme::family_color(family);
                    let chip = RichText::new(label).size(9.5).strong().color(if on {
                        theme::text_on(color)
                    } else {
                        p.weak
                    });
                    let chip = if on {
                        chip.background_color(color)
                    } else {
                        chip
                    };
                    ui.label(chip);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .small_button("↗")
                    .on_hover_text(tr("Open the channel strip: its curve and every setting"))
                    .clicked()
                {
                    self.open_strip(doc, channel);
                }
            });
        });
        if folded {
            return;
        }
        ui.add_space(2.0);
        self.strip_sections(ui, doc, channel, strip.as_ref());
    }

    /// The channel strip's sections, its output and its curve.
    fn strip_sections(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        strip: Option<&audiostrada_core::Insert>,
    ) {
        use audiostrada_dsp::channel::{MODULES, module_order, order_code};
        let p = theme::current(ui);
        let info = &audiostrada_dsp::channel::INFO;
        let character = crate::console_ui::character(doc, channel);
        let values: Vec<f32> = info
            .params
            .iter()
            .map(|q| match (q.id, strip) {
                ("character", _) => character as f32,
                (_, Some(s)) => s.params.get(q.id).copied().unwrap_or(q.default),
                (_, None) => q.default,
            })
            .collect();
        let meters = strip
            .and_then(|s| doc.player_mut().insert_meters(channel, s.id))
            .unwrap_or_default();
        let order = module_order(
            values[info
                .params
                .iter()
                .position(|q| q.id == "order")
                .unwrap_or(0)],
        );
        let mut frames = [Rect::NOTHING; 5];
        // The curve takes what the sections leave: the unit spans the whole rack.
        let right = ui.max_rect().right();
        egui::ScrollArea::horizontal()
            .id_salt(("strip-unit", strip.map(|s| s.id.0)))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (slot, &m) in order.iter().enumerate() {
                        let module = MODULES[m];
                        let (_, hint, family) = STRIP_MODULES[m];
                        let color = theme::family_color(family);
                        let on = values
                            [info.params.iter().position(|q| q.id == module).unwrap_or(0)]
                            >= 0.5;
                        let moving = self.rack.strip_drag == Some(m);
                        let frame = egui::Frame::new()
                            .fill(p.base)
                            .stroke(egui::Stroke::new(
                                1.0,
                                if moving {
                                    p.accent
                                } else {
                                    Color32::TRANSPARENT
                                },
                            ))
                            .corner_radius(5.0)
                            .inner_margin(egui::Margin::symmetric(4, 2))
                            .show(ui, |ui| {
                                ui.set_min_height(SECTION);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 3.0;
                                        let (grip, response) =
                                            ui.allocate_exact_size(vec2(8.0, 18.0), Sense::drag());
                                        let hot = response.hovered() || response.dragged();
                                        for k in 0..6 {
                                            let dot = pos2(
                                                grip.left() + 2.0 + (k % 2) as f32 * 4.0,
                                                grip.center().y - 4.0 + (k / 2) as f32 * 4.0,
                                            );
                                            ui.painter().circle_filled(
                                                dot,
                                                1.0,
                                                if hot {
                                                    p.text
                                                } else {
                                                    p.weak.gamma_multiply(0.6)
                                                },
                                            );
                                        }
                                        if hot {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                                        }
                                        if response.drag_started() {
                                            self.rack.strip_drag = Some(m);
                                        }
                                        response.on_hover_text(tr(
                                            "Drag to change the order of the modules",
                                        ));
                                        if power_led(ui, on).on_hover_text(tr(hint)).clicked() {
                                            self.set_strip_module(doc, channel, module, !on);
                                        }
                                        ui.label(
                                            RichText::new(section_title(module))
                                                .strong()
                                                .size(10.5)
                                                .color(if on { color } else { p.weak }),
                                        );
                                    });
                                    let tint = if on { color } else { p.weak };
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 0.0;
                                        self.strip_knobs(
                                            ui, doc, channel, strip, &values, module, tint, &meters,
                                        );
                                    });
                                });
                            });
                        frames[slot] = frame.response.rect;
                    }
                    // The output, after every module, and the curve.
                    egui::Frame::new()
                        .fill(p.base)
                        .corner_radius(5.0)
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.set_min_height(SECTION);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add_sized(
                                    [20.0, 18.0],
                                    egui::Label::new(
                                        RichText::new(tr("Output")).strong().size(10.5),
                                    )
                                    .extend(),
                                );
                                self.strip_knobs(
                                    ui, doc, channel, strip, &values, "output", p.text, &meters,
                                );
                            });
                        });
                    let width = right - ui.cursor().left() - 2.0;
                    strip_curve(ui, &values, width);
                });
            });
        // A dragged section lands between the others, by the pointer's x.
        let Some(m) = self.rack.strip_drag else {
            return;
        };
        if ui.input(|i| i.pointer.any_down()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return;
        }
        self.rack.strip_drag = None;
        let (Some(strip), Some(x)) = (strip, ui.input(|i| i.pointer.hover_pos()).map(|p| p.x))
        else {
            return;
        };
        let others: Vec<usize> = order.iter().copied().filter(|&o| o != m).collect();
        let at = |o: usize| frames[order.iter().position(|&k| k == o).unwrap_or(0)];
        let target = others.iter().filter(|&&o| at(o).center().x < x).count();
        let mut new = [m; 5];
        let mut rest = others.iter();
        for (k, slot) in new.iter_mut().enumerate() {
            if k != target {
                *slot = *rest.next().unwrap_or(&m);
            }
        }
        if new != order {
            let (id, code) = (strip.id, order_code(new));
            doc.history_mut()
                .edit(tr("Reorder the channel strip"), |p| {
                    routing::set_insert_param(p, channel, id, "order", code)
                });
        }
    }

    /// The controls of one section of the channel strip; without a strip yet, shown as they
    /// would be and not to be turned.
    #[allow(clippy::too_many_arguments)]
    fn strip_knobs(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        strip: Option<&audiostrada_core::Insert>,
        values: &[f32],
        module: &str,
        tint: Color32,
        meters: &[f32],
    ) {
        let p = theme::current(ui);
        let knobs: &[(&str, &'static str)] = match module {
            "filters" => &[("hpf", "High pass"), ("lpf", "Low pass")],
            "gate" => &[("gate_threshold", "Threshold"), ("gate_range", "Range")],
            "comp" => &[
                ("comp_threshold", "Threshold"),
                ("comp_ratio", "Ratio"),
                ("comp_attack", "Attack"),
                ("comp_makeup", "Makeup"),
            ],
            "eq" => &[
                ("low_freq", "Low"),
                ("low_gain", "Gain"),
                ("lmf_freq", "Low mid"),
                ("lmf_gain", "Gain"),
                ("hmf_freq", "High mid"),
                ("hmf_gain", "Gain"),
                ("high_freq", "High"),
                ("high_gain", "Gain"),
            ],
            "drive" => &[("drive_amount", "Amount")],
            _ => &[("output", "Level")],
        };
        let info = &audiostrada_dsp::channel::INFO;
        ui.add_enabled_ui(strip.is_some(), |ui| {
            let Some(strip) = strip else {
                for (_, label) in knobs {
                    ui.add_sized(
                        [52.0, 76.0],
                        egui::Label::new(RichText::new(tr(label)).size(9.5).color(p.weak)),
                    );
                }
                return;
            };
            let mut params = effect_editors::Params {
                values: values.to_vec(),
                doc: &mut *doc,
                channel,
                insert: strip.id,
                info,
            };
            for (id, label) in knobs {
                effect_editors::wide_knob(ui, &mut params, id, tr(label), 34.0, tint);
            }
        });
        // Gain reduction and gate meters beside their sections.
        let meter = match module {
            "comp" => Some((
                meters.get(1).copied().unwrap_or(0.0).abs().min(24.0) / 24.0,
                tint,
            )),
            "gate" => Some((
                (-meters.first().copied().unwrap_or(0.0)).clamp(0.0, 40.0) / 40.0,
                theme::PLAY,
            )),
            _ => None,
        };
        if let Some((amount, color)) = meter {
            let (rect, _) = ui.allocate_exact_size(vec2(6.0, 70.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, p.panel);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    rect.min,
                    pos2(rect.right(), rect.top() + rect.height() * amount),
                ),
                2.0,
                color,
            );
        }
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
        add_popup(&response).show(|ui| {
            self.add_menu(ui, doc, channel, (place, index), place.is_none());
        });
    }

    /// The free slot at the end: a large add button.
    fn empty_slot(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        index: usize,
        width: f32,
    ) {
        let p = theme::current(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT + 2.0 * RAIL), Sense::hover());
        rails(ui, rect);
        self.rack.gaps.push((rect, None, index));
        let hole = Rect::from_min_max(
            pos2(rect.left() + 6.0, rect.top() + RAIL + 4.0),
            pos2(rect.right() - 6.0, rect.bottom() - RAIL - 4.0),
        );
        let response = ui
            .interact(hole, ui.scope_id().with("rack-empty"), Sense::click())
            .on_hover_text(tr("Add an effect or a parallel chain"));
        #[cfg(test)]
        super::tests::seen("rack empty".into(), hole);
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
        add_popup(&response).show(|ui| {
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
        let effect = ui.button(tr("Effect or plugin…"));
        #[cfg(test)]
        super::tests::seen("rack add effect".into(), effect.rect);
        if effect.clicked() {
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
        let plate = self.slot(ui, (MODULE, id), (tint, !insert.bypass), |view, ui| {
            let p = theme::current(ui);
            let full = ui.max_rect();
            // Its header: switch, name, delay.
            ui.horizontal(|ui| {
                ui.set_height(20.0);
                ui.spacing_mut().item_spacing.x = 6.0;
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
                let pill = latency.as_ref().map(|l| {
                    ui.painter().layout_no_wrap(
                        l.clone(),
                        egui::FontId::monospace(9.0),
                        theme::mix(tint, Color32::WHITE, 0.55),
                    )
                });
                let room = pill.as_ref().map_or(0.0, |g| g.size().x + 16.0);
                let width = (full.right() - ui.cursor().left() - room - 26.0).max(30.0);
                let head = ui
                    .add_sized(
                        [width, 20.0],
                        egui::Label::new(RichText::new(&name).strong().size(13.0).color(ink))
                            .truncate()
                            .sense(Sense::click_and_drag()),
                    )
                    .on_hover_text(tr(
                        "Click to edit, Shift+click to bypass, Ctrl+click to remove; drag the \
                         module to move it",
                    ));
                #[cfg(test)]
                super::tests::seen(format!("module {}", id.0), head.rect);
                if head.drag_started() {
                    view.rack.drag = Some(id);
                }
                let modifiers = ui.input(|i| i.modifiers);
                if head.clicked() && modifiers.shift {
                    let bypass = !insert.bypass;
                    doc.history_mut().edit(tr("Bypass insert"), |p| {
                        routing::set_bypass(p, channel, id, bypass)
                    });
                } else if head.clicked() && modifiers.command {
                    view.remove_insert(doc, channel, id);
                } else if head.clicked() {
                    view.open_insert(channel, insert);
                }
                // The delay it adds, on a small screen.
                if let Some(galley) = pill {
                    let (rect, response) =
                        ui.allocate_exact_size(galley.size() + vec2(10.0, 5.0), Sense::hover());
                    response.on_hover_text(tr("Delay this effect adds, compensated"));
                    lcd_box(ui.painter(), rect);
                    ui.painter().galley(
                        rect.center() - galley.size() / 2.0,
                        galley,
                        theme::mix(tint, Color32::WHITE, 0.45),
                    );
                }
            });
            // A line engraved under the header.
            let y = full.top() + 24.5;
            ui.painter().line_segment(
                [pos2(full.left(), y), pos2(full.right(), y)],
                egui::Stroke::new(1.0, Color32::from_black_alpha(110)),
            );
            ui.painter().line_segment(
                [pos2(full.left(), y + 1.0), pos2(full.right(), y + 1.0)],
                egui::Stroke::new(1.0, Color32::from_white_alpha(if p.dark { 14 } else { 90 })),
            );
            // Its face: the main parameters in columns, or an EQ's curve.
            let area = Rect::from_min_max(
                pos2(full.left(), full.top() + 26.0),
                pos2(full.right(), full.bottom() - FOOT),
            );
            let face_area = ui.interact(
                area,
                ui.scope_id().with(("rack-face", id.0)),
                Sense::click_and_drag(),
            );
            if face_area.drag_started() {
                view.rack.drag = Some(id);
            }
            if audiostrada_dsp::info(&insert.effect).is_none() {
                face_area
                    .on_hover_text(tr("Right-click to choose the parameters shown here"))
                    .context_menu(|ui| view.macro_menu(ui, doc, channel, insert));
            }
            let mut face_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(area)
                    .layout(Layout::top_down(Align::Min)),
            );
            if insert.bypass {
                face_ui.multiply_opacity(0.45);
            }
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
                        eq_curve(
                            &mut face_ui,
                            &params.values,
                            tint,
                            area.shrink2(vec2(2.0, 6.0)),
                        );
                    } else {
                        let shown = face(info);
                        for (k, cell) in cells(area, shown.len()).into_iter().enumerate() {
                            face_knob(&mut face_ui, &mut params, shown[k], cell, tint);
                        }
                    }
                }
                None => view.macros(&mut face_ui, doc, channel, insert, tint, area),
            }
            // Its footer: A and B, and the editor.
            let foot = Rect::from_min_max(
                pos2(full.left(), full.bottom() - FOOT + 6.0),
                full.right_bottom(),
            );
            let mut foot_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(foot)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            view.ab_buttons(&mut foot_ui, doc, channel, insert, tint);
            let edit = Rect::from_min_max(
                pos2(foot.right() - 74.0, foot.center().y - 10.0),
                pos2(foot.right(), foot.center().y + 10.0),
            );
            if edit_button(ui, edit, id.0)
                .on_hover_text(tr("Every setting, in the effect's window"))
                .clicked()
            {
                view.open_insert(channel, insert);
            }
        });
        if close_button(ui, plate, id.0) {
            self.remove_insert(doc, channel, id);
        }
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
        let plate = self.slot(ui, (PARALLEL, id), (tint, !insert.bypass), |view, ui| {
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
            });
        });
        if close_button(ui, plate, id.0) {
            self.remove_insert(doc, channel, id);
        }
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
        add_popup(&add).show(|ui| {
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
                Channel::Master | Channel::Clip(_) => None,
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
                None if let Some(Channel::Clip(id)) = channel => {
                    let name =
                        audiostrada_core::edit::clip(project, id).map_or("", |c| c.name.as_str());
                    ui.label(
                        RichText::new(trf!("Clip: {name}", name = name))
                            .strong()
                            .size(14.0),
                    );
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
            // The module's name follows the pointer to where it will land.
            if let (Some(pos), Some(insert)) = (
                ui.input(|i| i.pointer.hover_pos()),
                routing::find_insert(doc.project(), channel, id),
            ) {
                let name = if insert.effect == routing::PARALLEL {
                    tr("Parallel chain").to_owned()
                } else {
                    effect_name(doc, &insert.effect)
                };
                let p = theme::current(ui);
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::unique("rack-drag"),
                ));
                let galley = painter.layout_no_wrap(name, egui::FontId::proportional(12.0), p.text);
                let label =
                    Rect::from_min_size(pos + vec2(14.0, 10.0), galley.size() + vec2(16.0, 8.0));
                painter.rect_filled(label, 6.0, p.surface);
                painter.rect_stroke(
                    label,
                    6.0,
                    egui::Stroke::new(1.0, p.accent),
                    egui::StrokeKind::Inside,
                );
                painter.galley(label.min + vec2(8.0, 4.0), galley, p.text);
            }
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

    /// A plugin's chosen parameters as knobs in the cells of `face`; a right-click on the face
    /// picks them.
    fn macros(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
        tint: Color32,
        face: Rect,
    ) {
        let id = insert.id;
        let params = self.plugin_params_of(doc, channel, id);
        let shown = shown_macros(insert, &params);
        let chosen: Vec<&audiostrada_plugins::ParamDesc> = shown
            .iter()
            .filter_map(|m| params.iter().find(|p| p.id.to_string() == *m))
            .take(2 * COLUMNS)
            .collect();
        if chosen.is_empty() {
            let p = theme::current(ui);
            cut_text(
                ui.painter(),
                pos2(face.center().x, face.center().y - 6.0),
                tr("Right-click to put parameters here"),
                face.width() - 8.0,
                11.0,
                p.weak,
            );
            return;
        }
        for (p, cell) in chosen.iter().zip(cells(face, chosen.len())) {
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
            let text = doc
                .player_mut()
                .plugin(channel, id)
                .and_then(|pl| pl.value_text(p.id, f64::from(value)))
                .unwrap_or_else(|| format!("{value:.2}"));
            let mut changed = None;
            let response = knob_cell(
                ui,
                cell,
                (&p.name, &text),
                |ui, size| {
                    let (moved, response) = widgets::knob(
                        ui,
                        value,
                        (p.min as f32, p.max as f32),
                        p.default as f32,
                        size,
                        false,
                        tint,
                    );
                    changed = moved;
                    response
                },
                tint,
            );
            let response = response.on_hover_text(format!("{}: {text}", p.name));
            let name = p.name.clone();
            automated_control(doc, channel, &lane, changed, &response, |doc, v| {
                doc.history_mut()
                    .edit_merged(&name, &format!("insert:{}:{}", id.0, key), |proj| {
                        routing::set_insert_param(proj, channel, id, &key, v)
                    });
            });
        }
    }

    /// A plugin's parameters, read from it once.
    fn plugin_params_of(
        &mut self,
        doc: &mut Document,
        channel: Channel,
        id: InsertId,
    ) -> Vec<audiostrada_plugins::ParamDesc> {
        if !self.plugin_params.contains_key(&id)
            && let Some(plugin) = doc.player_mut().plugin(channel, id)
        {
            let params = plugin.params().into_iter().filter(|p| !p.hidden).collect();
            self.plugin_params.insert(id, params);
        }
        self.plugin_params.get(&id).cloned().unwrap_or_default()
    }

    /// The parameters to show on a plugin's face, to pick from its whole list.
    fn macro_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
    ) {
        let id = insert.id;
        let params = self.plugin_params_of(doc, channel, id);
        let shown = shown_macros(insert, &params);
        ui.set_max_height(320.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for p in &params {
                let key = p.id.to_string();
                let mut on = shown.contains(&key);
                if ui.checkbox(&mut on, &p.name).changed() {
                    let mut list = shown.clone();
                    list.retain(|m| *m != key);
                    if on && list.len() < 8 {
                        list.push(key.clone());
                    }
                    doc.history_mut().edit(tr("Rack controls"), |proj| {
                        if let Some(i) = routing::find_insert_mut(proj, channel, id) {
                            i.macros = list.clone();
                        }
                    });
                }
            }
            if params.is_empty() {
                ui.weak(tr("This plugin exposes no parameters."));
            }
        });
    }

    /// A and B settings of an insert, to compare two of them: a switch of two halves, the one
    /// in use lit in the module's color.
    fn ab_buttons(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
        tint: Color32,
    ) {
        let p = theme::current(ui);
        let id = insert.id;
        let (current, _) = self.rack.ab.get(&id).cloned().unwrap_or_default();
        let (whole, _) = ui.allocate_exact_size(vec2(52.0, 20.0), Sense::hover());
        ui.painter()
            .rect_filled(whole, 5.0, theme::mix(p.base, Color32::BLACK, 0.2));
        ui.painter().rect_stroke(
            whole,
            5.0,
            egui::Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
        for (slot, label) in [(0, "A"), (1, "B")] {
            let half = Rect::from_min_size(
                whole.min + vec2(slot as f32 * whole.width() / 2.0, 0.0),
                vec2(whole.width() / 2.0, whole.height()),
            )
            .shrink(2.0);
            let response = ui
                .interact(
                    half,
                    ui.scope_id().with(("rack-ab", id.0, slot)),
                    Sense::click(),
                )
                .on_hover_text(tr("Two settings to compare: the other one is kept"));
            let on = current == slot;
            if on {
                ui.painter()
                    .rect_filled(half, 3.5, tint.gamma_multiply(0.85));
            } else if response.hovered() {
                ui.painter().rect_filled(half, 3.5, p.hover);
            }
            ui.painter().text(
                half.center(),
                egui::Align2::CENTER_CENTER,
                label,
                theme::strong(11.0),
                if on { theme::text_on(tint) } else { p.weak },
            );
            if response.clicked() && !on {
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
        let modifiers = ui.input(|i| i.modifiers);
        if power.clicked() || (head.clicked() && modifiers.shift) {
            let bypass = !insert.bypass;
            doc.history_mut().edit(tr("Bypass insert"), |p| {
                routing::set_bypass(p, channel, id, bypass)
            });
        }
        let head = head.on_hover_text(tr(
            "Click to edit, Shift+click to bypass, Ctrl+click to remove, drag to move, \
             right-click to remove",
        ));
        if head.drag_started() {
            self.rack.drag = Some(id);
        }
        if head.clicked() && modifiers.command {
            self.remove_insert(doc, channel, id);
            return;
        }
        if head.clicked() && !modifiers.shift {
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
    fn a_face_keeps_its_knobs_inside_whatever_their_names() {
        let face = Rect::from_min_size(pos2(10.0, 20.0), vec2(236.0, 120.0));
        assert!(cells(face, 0).is_empty());
        for count in 1..=2 * COLUMNS {
            let all = cells(face, count);
            assert_eq!(all.len(), count);
            for cell in &all {
                assert!(face.expand(0.01).contains_rect(*cell), "{count}: {cell:?}");
            }
            // Past a row, two rows of shorter cells.
            assert_eq!(all[0].height() < face.height(), count > COLUMNS);
        }
    }

    #[test]
    fn a_plugin_shows_its_mix_until_others_are_chosen() {
        let param = |id: u32, name: &str| audiostrada_plugins::ParamDesc {
            id,
            name: name.into(),
            module: String::new(),
            min: 0.0,
            max: 1.0,
            default: 0.5,
            stepped: false,
            hidden: false,
            read_only: false,
        };
        let params = vec![param(1, "Drive"), param(7, "Dry/Wet"), param(9, "Output")];
        let mut doc = Document::new(AppDirs::default());
        let mut insert = None;
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("A");
            insert = routing::add_insert(p, Channel::Track(t), "clap:x", Default::default());
        });
        let t = doc.project().tracks[0].id;
        let mut plugin = routing::find_insert(doc.project(), Channel::Track(t), insert.unwrap())
            .unwrap()
            .clone();
        assert_eq!(shown_macros(&plugin, &params), ["7"]);
        assert!(shown_macros(&plugin, &params[..1]).is_empty());
        plugin.macros = vec!["9".into()];
        assert_eq!(shown_macros(&plugin, &params), ["9"]);
    }

    #[test]
    fn a_module_drags_by_its_plate_not_only_its_name() {
        for below in [None, Some(false), Some(true)] {
            let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
            let (mut t, mut delay, mut comp) = (None, None, None);
            doc.borrow_mut().history_mut().edit("Setup", |p| {
                let id = p.add_track("A");
                let c = Channel::Track(id);
                delay = routing::add_insert(p, c, "delay", Default::default());
                comp = routing::add_insert(p, c, "compressor", Default::default());
                t = Some(id);
            });
            let (t, delay, comp) = (t.unwrap(), delay.unwrap(), comp.unwrap());
            doc.borrow_mut()
                .select_track(t, crate::document::SelectMode::Only);
            let mut view = MixerView::new(Rc::clone(&doc));
            let ctx = egui::Context::default();
            rack_frame(&ctx, &mut view, vec![]);
            rack_frame(&ctx, &mut view, vec![]);
            // Taken by the grip at the left edge, or by the face under the knobs.
            let title = rect(&format!("module {}", comp.0));
            let from = match below {
                None => title.center(),
                Some(true) => pos2(title.left() + 30.0, title.bottom() + 106.0),
                Some(false) => pos2(title.left() - 30.0, title.center().y + 60.0),
            };
            let to = rect(&format!("module {}", delay.0)).left_center() - vec2(55.0, 0.0);
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
            let order: Vec<_> = list
                .iter()
                .filter(|i| i.effect != routing::STRIP)
                .map(|i| i.id)
                .collect();
            assert_eq!(order, vec![comp, delay], "taken below: {below:?}");
        }
    }

    #[test]
    fn the_free_slot_opens_the_browser_where_it_was_clicked() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let mut t = None;
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            let id = p.add_track("A");
            routing::add_insert(p, Channel::Track(id), "delay", Default::default());
            t = Some(id);
        });
        let t = t.unwrap();
        doc.borrow_mut()
            .select_track(t, crate::document::SelectMode::Only);
        let mut view = MixerView::new(Rc::clone(&doc));
        let ctx = egui::Context::default();
        let click = |view: &mut MixerView, at: Pos2| {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            rack_frame(&ctx, view, vec![Event::PointerMoved(at)]);
            rack_frame(&ctx, view, vec![button(true)]);
            rack_frame(&ctx, view, vec![button(false)]);
            rack_frame(&ctx, view, vec![]);
            rack_frame(&ctx, view, vec![]);
        };
        rack_frame(&ctx, &mut view, vec![]);
        rack_frame(&ctx, &mut view, vec![]);
        click(&mut view, rect("rack empty").center());
        click(&mut view, rect("rack add effect").center());
        assert_eq!(
            view.rack.place.map(|(c, ..)| c),
            Some(Channel::Track(t)),
            "the browser opened for the track"
        );
    }

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
