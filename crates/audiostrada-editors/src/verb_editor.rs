//! Strada-Verb's editor: how long each frequency rings, drawn over the spectra of the input and
//! of the tail, its points dragged for the decay, the lows, the highs and the cuts; the main
//! controls large at the left, the others in a grid.

use audiostrada_dsp::analyzer::{BANDS, band_frequency};
use audiostrada_dsp::reverb::{
    CLEAN, COLOR_BANDS, COLOR_HINTS, COLORS, EIGHTIES, INPUT_SPECTRUM, TAIL_SPECTRUM, TYPE_HINTS,
    TYPES, VINTAGE, decay_seconds, uses,
};
use egui::{
    self, Align, Align2, Color32, FontId, Layout, Pos2, Rect, Response, RichText, Sense, Shape,
    Stroke, Ui, UiBuilder, pos2, vec2,
};

use crate::controls::{EditorParams, wide_knob};
use crate::i18n::tr;
use crate::{theme, trf, widgets};

/// The points' colors, also the tints of their knobs.
pub const LOW_CUT: [u8; 3] = [236, 92, 92];
const LOWS: [u8; 3] = [240, 160, 64];
const DECAY: [u8; 3] = [102, 214, 150];
const HIGHS: [u8; 3] = [92, 176, 255];
pub const HIGH_CUT: [u8; 3] = [196, 120, 255];
/// The decay drawn from 80 ms to 30 s.
const SECONDS: (f32, f32) = (0.08, 30.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Point {
    LowCut,
    Lows,
    Decay,
    Highs,
    HighCut,
}

#[derive(Debug, Default)]
pub struct VerbState {
    held: Option<Point>,
}

fn index(params: &dyn EditorParams, id: &str, len: usize) -> usize {
    (params.get(id).round().max(0.0) as usize).min(len - 1)
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn rgba(c: [u8; 3], a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut VerbState) {
    let kind = index(params, "type", TYPES.len());
    let color = index(params, "color", COLORS.len());
    ui.horizontal(|ui| {
        choice_row(ui, params, "type", &TYPES, &TYPE_HINTS);
        ui.add_space(14.0);
        choice_row(ui, params, "color", &COLORS, &COLOR_HINTS);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            fit_to_tempo(ui, params);
        });
    });
    ui.add_space(8.0);
    tail_view(ui, params, meters, state, (kind, color));
    ui.add_space(8.0);
    controls(ui, params, kind);
}

fn bar_seconds(params: &dyn EditorParams) -> f32 {
    240.0 / (params.tempo() as f32).max(1.0)
}

/// Sets the decay to whole quarters of a bar and the pre-delay to a note value.
fn fit_to_tempo(ui: &mut Ui, params: &mut dyn EditorParams) {
    let response = chip(ui, tr("Fit to the tempo"), false).on_hover_text(tr(
        "Sets the decay to whole quarters of a bar and the pre-delay to a note value",
    ));
    if response.clicked() {
        let bar = bar_seconds(params);
        let quarters = (params.get("decay") / bar * 4.0).round().max(1.0);
        params.set(
            "decay",
            Some((quarters / 4.0 * bar).clamp(0.2, 20.0)),
            &response,
        );
        params.set("predelay_sync", Some(1.0), &response);
        params.end_gesture();
    }
}

// ---------------------------------------------------------------- Shared pieces

/// A small button, lit when on.
pub fn chip(ui: &mut Ui, label: &str, on: bool) -> Response {
    let p = theme::current(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(11.5), p.text);
    let size = vec2(chip_width(ui, label), 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = match (on, response.hovered()) {
        (true, _) => p.accent.gamma_multiply(0.3),
        (false, true) => p.hover,
        (false, false) => p.surface,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, if on { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        if on { p.text } else { p.weak },
    );
    response
}

fn chip_width(ui: &Ui, label: &str) -> f32 {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), FontId::proportional(11.5), Color32::WHITE);
    (galley.size().x + 18.0).max(32.0)
}

/// The values of a choice as small buttons, each with what it does and the parameter's menu.
pub fn choice_row(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    names: &[&'static str],
    hints: &[&'static str],
) {
    let current = index(params, id, names.len());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (k, name) in names.iter().enumerate() {
            let response = chip(ui, tr(name), k == current).on_hover_text(tr(hints[k]));
            let changed = (response.clicked() && k != current).then_some(k as f32);
            params.set(id, changed, &response);
        }
    });
}

/// A switch as a small button.
pub fn toggle_chip(ui: &mut Ui, params: &mut dyn EditorParams, id: &str, label: &str, hint: &str) {
    params.toggle(ui, id, |ui, on| {
        let mut response = chip(ui, label, *on).on_hover_text(hint);
        if response.clicked() {
            *on = !*on;
            // `toggle` applies changed responses only: a chip reports a click alone.
            response.mark_changed();
        }
        response
    });
}

/// Small buttons side by side, centered in a column.
pub fn chip_row(ui: &mut Ui, labels: &[&str], add: impl FnOnce(&mut Ui)) {
    let width = labels.iter().map(|l| chip_width(ui, l)).sum::<f32>()
        + 4.0 * labels.len().saturating_sub(1) as f32;
    ui.allocate_ui_with_layout(
        vec2(width, 24.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            add(ui);
        },
    );
}

/// A knob over note values from the shortest, with its name above and its note below.
pub fn note_knob(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    (id, label): (&str, &str),
    size: f32,
    tint: Color32,
    longest: Option<f32>,
) {
    let Some(param) = params.param(id) else {
        return;
    };
    let notes: Vec<_> = audiostrada_dsp::delay::notes_by_length()
        .into_iter()
        .filter(|n| longest.is_none_or(|l| n.2 <= l))
        .collect();
    let current = params.get(id).round() as usize;
    let at = notes.iter().position(|n| n.0 == current).unwrap_or(0);
    let last = notes.len().saturating_sub(1).max(1) as f32;
    let default = notes
        .iter()
        .position(|n| n.0 == param.default.round() as usize)
        .unwrap_or(0);
    let weak = theme::current(ui).weak;
    ui.label(RichText::new(label).size(10.5).color(weak));
    // A drag goes on between the notes; it lands on the nearest.
    let key = ui.scope_id().with(("note-knob", id));
    let unit = at as f32 / last;
    let held = ui.data(|d| d.get_temp::<f32>(key));
    let (changed, response) = widgets::knob(
        ui,
        held.unwrap_or(unit),
        (0.0, 1.0),
        default as f32 / last,
        size,
        false,
        tint,
    );
    let step = match changed {
        _ if response.double_clicked() => Some(default),
        Some(t) if response.dragged() => {
            ui.data_mut(|d| d.insert_temp(key, t));
            Some((t * last).round() as usize)
        }
        Some(t) if t > unit => Some((at + 1).min(notes.len() - 1)),
        Some(t) if t < unit => Some(at.saturating_sub(1)),
        _ => None,
    };
    if !response.dragged() {
        ui.data_mut(|d| d.remove::<f32>(key));
    }
    let shown = step.unwrap_or(at);
    let response = response.on_hover_text(trf!(
        "{name}: {value}. Drag, Shift for fine, double-click to reset",
        name = tr(param.name),
        value = tr(notes[shown].1)
    ));
    let value = step.filter(|s| *s != at).map(|s| notes[s].0 as f32);
    params.set(id, value, &response);
    ui.label(RichText::new(tr(notes[shown].1)).monospace().size(11.0));
}

/// Where the controls go: the big ones in a card at the left, the others in rows of titled
/// groups at the right, all cells of a row as wide.
#[derive(Debug)]
pub struct Panel {
    pub big: Vec<Rect>,
    pub rows: Vec<Vec<Rect>>,
}

const PAD: f32 = 12.0;
const TITLE: f32 = 18.0;
const ROW_GAP: f32 = 10.0;

pub fn panel(
    ui: &mut Ui,
    (big, big_width): (usize, f32),
    rows: &[&[(&'static str, usize)]],
    (row_height, big_height): (f32, f32),
) -> Panel {
    let p = theme::current(ui);
    let width = ui.available_width();
    let grid =
        rows.len() as f32 * (TITLE + row_height) + rows.len().saturating_sub(1) as f32 * ROW_GAP;
    let height = grid.max(big_height) + 2.0 * PAD;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let left = Rect::from_min_size(rect.min, vec2(big as f32 * big_width + 2.0 * PAD, height));
    let right = Rect::from_min_max(pos2(left.right() + 8.0, rect.top()), rect.max);
    let painter = ui.painter();
    let (fill, edge) = theme::card(&p);
    for r in [left, right] {
        painter.rect_filled(r, 8.0, fill);
        painter.rect_stroke(r, 8.0, edge, egui::StrokeKind::Inside);
    }
    let rule = Stroke::new(1.0, p.weak.gamma_multiply(0.16));
    let big_cells = (0..big)
        .map(|k| {
            Rect::from_min_size(
                pos2(left.left() + PAD + k as f32 * big_width, left.top() + PAD),
                vec2(big_width, height - 2.0 * PAD),
            )
        })
        .collect();
    let inner = right.shrink2(vec2(PAD, PAD));
    let mut y = inner.top() + (inner.height() - grid) / 2.0;
    let mut cells = Vec::new();
    for (r, groups) in rows.iter().enumerate() {
        if r > 0 {
            painter.hline(inner.x_range(), y - ROW_GAP / 2.0, rule);
        }
        let count: usize = groups.iter().map(|g| g.1).sum();
        let cell = inner.width() / count.max(1) as f32;
        let mut x = inner.left();
        let mut row = Vec::new();
        for (g, (title, n)) in groups.iter().enumerate() {
            if g > 0 {
                painter.vline(x, (y + 2.0)..=(y + TITLE + row_height - 2.0), rule);
            }
            painter.text(
                pos2(x + 8.0, y),
                Align2::LEFT_TOP,
                tr(title).to_uppercase(),
                theme::strong(9.5),
                p.weak.gamma_multiply(0.85),
            );
            for _ in 0..*n {
                row.push(Rect::from_min_size(
                    pos2(x, y + TITLE),
                    vec2(cell, row_height),
                ));
                x += cell;
            }
        }
        cells.push(row);
        y += TITLE + row_height + ROW_GAP;
    }
    Panel {
        big: big_cells,
        rows: cells,
    }
}

/// Draws in a cell, centered across.
pub fn cell<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            add(ui)
        },
    )
    .inner
}

/// Draws the big cells, all their contents as low as the tallest is centered.
pub fn big_cells(ui: &mut Ui, cells: &[Rect], mut add: impl FnMut(&mut Ui, usize)) {
    let key = ui.scope_id().with("big-cells");
    let tallest = ui.data(|d| d.get_temp::<f32>(key));
    let mut measured = 0.0f32;
    for (k, rect) in cells.iter().enumerate() {
        let top = tallest.map_or(0.0, |h| ((rect.height() - h) / 2.0).max(0.0));
        let area = Rect::from_min_max(rect.min + vec2(0.0, top), rect.max);
        let used = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(area)
                    .layout(Layout::top_down(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    add(ui, k);
                },
            )
            .response
            .rect;
        measured = measured.max(used.height());
    }
    ui.data_mut(|d| d.insert_temp(key, measured));
}

/// A log axis of frequencies from 20 Hz to 20 kHz over `plot`.
#[derive(Clone, Copy, Debug)]
pub struct FreqAxis {
    left: f32,
    width: f32,
}

impl FreqAxis {
    pub fn new(plot: Rect) -> Self {
        Self {
            left: plot.left(),
            width: plot.width(),
        }
    }

    pub fn x(&self, hz: f32) -> f32 {
        self.left + (hz.max(1.0) / 20.0).ln() / 1000f32.ln() * self.width
    }

    pub fn hz(&self, x: f32) -> f32 {
        20.0 * (((x - self.left) / self.width).clamp(0.0, 1.0) * 1000f32.ln()).exp()
    }
}

/// The background of a display: the screen, lit a little from the top.
pub fn screen_panel(painter: &egui::Painter, rect: Rect, p: &theme::Palette) {
    painter.rect_filled(rect, 8.0, theme::screen(p));
    let mut mesh = egui::Mesh::default();
    let lit = if p.dark {
        Color32::from_white_alpha(10)
    } else {
        Color32::from_black_alpha(0)
    };
    let inner = rect.shrink(1.0);
    for (at, c) in [
        (inner.left_top(), lit),
        (inner.right_top(), lit),
        (inner.right_bottom(), Color32::TRANSPARENT),
        (inner.left_bottom(), Color32::TRANSPARENT),
    ] {
        mesh.colored_vertex(at, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
}

/// The decades of `plot`, thin lines with the main frequencies named under it.
pub fn freq_grid(
    painter: &egui::Painter,
    plot: Rect,
    axis: FreqAxis,
    p: &theme::Palette,
    labels: &[f32],
) {
    for decade in [10.0f32, 100.0, 1000.0, 10_000.0] {
        for m in 1..=9 {
            let hz = decade * m as f32;
            if !(20.0..=20_000.0).contains(&hz) {
                continue;
            }
            let a = if m == 1 { 0.16 } else { 0.06 };
            painter.vline(
                axis.x(hz),
                plot.y_range(),
                Stroke::new(1.0, p.weak.gamma_multiply(a)),
            );
        }
    }
    for &hz in labels {
        painter.text(
            pos2(axis.x(hz), plot.bottom() + 5.0),
            Align2::CENTER_TOP,
            short_hz(hz),
            FontId::monospace(9.5),
            p.weak,
        );
    }
}

fn short_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{}k", hz / 1000.0)
    } else {
        format!("{hz:.0}")
    }
}

pub fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// A spectrum from the analyzer's bands, smoothed a little, `None` while silent.
pub fn spectrum(
    levels: Option<&[f32]>,
    axis: FreqAxis,
    y_of_db: impl Fn(f32) -> f32,
) -> Option<Vec<Pos2>> {
    let levels = levels.filter(|l| l.len() >= BANDS)?;
    if levels[..BANDS].iter().all(|db| *db < -110.0) {
        return None;
    }
    Some(
        (0..BANDS)
            .map(|b| {
                let a = levels[b.saturating_sub(1)];
                let c = levels[(b + 1).min(BANDS - 1)];
                let db = 0.25 * a + 0.5 * levels[b] + 0.25 * c;
                pos2(axis.x(band_frequency(b)), y_of_db(db))
            })
            .collect(),
    )
}

/// Fills from a curve down to `bottom`, the color fading away from the curve.
pub fn glow(painter: &egui::Painter, points: &[Pos2], bottom: f32, c: [u8; 3], a: f32) {
    let mut mesh = egui::Mesh::default();
    for pt in points {
        mesh.colored_vertex(*pt, rgba(c, a));
        mesh.colored_vertex(pos2(pt.x, bottom), rgba(c, 0.0));
    }
    for k in 0..points.len().saturating_sub(1) as u32 {
        let i = k * 2;
        mesh.add_triangle(i, i + 1, i + 3);
        mesh.add_triangle(i, i + 3, i + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// A band fading across, from `from` (full) to `to` (clear).
pub fn shade(painter: &egui::Painter, plot: Rect, (from, to): (f32, f32), c: [u8; 3]) {
    let mut mesh = egui::Mesh::default();
    for (x, a) in [(from, 0.2), (to, 0.0)] {
        mesh.colored_vertex(pos2(x, plot.top()), rgba(c, a));
        mesh.colored_vertex(pos2(x, plot.bottom()), rgba(c, a));
    }
    mesh.add_triangle(0, 1, 3);
    mesh.add_triangle(0, 3, 2);
    painter.add(Shape::mesh(mesh));
}

/// A point to drag: a ring, filled while hovered or held.
pub fn handle(painter: &egui::Painter, at: Pos2, c: [u8; 3], lit: bool, back: Color32) {
    if lit {
        painter.circle_filled(at, 13.0, rgba(c, 0.16));
        painter.circle(at, 7.0, rgb(c), Stroke::new(2.0, Color32::WHITE));
    } else {
        painter.circle(at, 6.5, back, Stroke::new(2.2, rgb(c)));
    }
}

/// What a point sets, in a label beside it that stays inside `bounds`.
pub fn bubble(painter: &egui::Painter, bounds: Rect, at: Pos2, text: String) {
    let galley = painter.layout_no_wrap(text, FontId::monospace(11.0), Color32::WHITE);
    let size = galley.size() + vec2(14.0, 8.0);
    let mut min = at + vec2(-size.x / 2.0, -size.y - 14.0);
    if min.y < bounds.top() + 2.0 {
        min.y = at.y + 14.0;
    }
    min.x = min
        .x
        .clamp(bounds.left() + 2.0, bounds.right() - size.x - 2.0);
    let rect = Rect::from_min_size(min, size);
    painter.rect_filled(rect, 5.0, Color32::from_black_alpha(215));
    painter.rect_stroke(
        rect,
        5.0,
        Stroke::new(1.0, Color32::from_white_alpha(40)),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + vec2(7.0, 4.0), galley, Color32::WHITE);
}

/// The gain of one-pole filters at `hz`: a low-pass at `high`, a high-pass at `low`.
pub fn band_gain(hz: f32, low: f32, high: f32) -> f32 {
    let lp = 1.0 / (1.0 + (hz / high).powi(2)).sqrt();
    let r = hz / low;
    lp * r / (1.0 + r * r).sqrt()
}

// ---------------------------------------------------------------- The tail

/// What sets how long each frequency rings.
#[derive(Clone, Copy)]
struct Tail {
    kind: usize,
    decay: f32,
    bass: f32,
    treble: f32,
    crossover: f32,
}

impl Tail {
    fn of(params: &dyn EditorParams, kind: usize) -> Self {
        Self {
            kind,
            decay: params.get("decay"),
            bass: params.get("bass"),
            treble: params.get("treble"),
            crossover: params.get("crossover"),
        }
    }

    /// How long `hz` rings, in seconds, from the engines' decays of the lows, the mids and
    /// the highs (measured at 8 kHz).
    fn at(&self, hz: f32) -> f32 {
        let decay = self.decay;
        let (lows, highs) = decay_seconds(self.kind, decay, self.bass, self.treble);
        let lows = if uses(self.kind, "bass") { lows } else { decay };
        let crossover = if uses(self.kind, "crossover") {
            self.crossover
        } else {
            250.0
        };
        let w = 1.0 / (1.0 + (hz / crossover).powi(2));
        let s = (hz / 8000.0).powi(2);
        let high = 2.0 * s / (1.0 + s);
        let rate = w * 60.0 / lows.max(0.05)
            + (1.0 - w) * 60.0 / decay.max(0.05)
            + high * (60.0 / highs.max(0.05) - 60.0 / decay.max(0.05));
        60.0 / rate.max(1e-3)
    }

    /// Where the highs' point stands, clear of the high cut.
    fn highs_hz(high_cut: f32) -> f32 {
        (high_cut / 1.8).clamp(2500.0, 8000.0)
    }

    /// Where the decay's point stands, between the lows and the highs.
    fn decay_hz(&self, high_cut: f32) -> f32 {
        if uses(self.kind, "crossover") {
            (self.crossover * Self::highs_hz(high_cut)).sqrt()
        } else {
            1000.0
        }
    }
}

/// The value in `range` that makes `f` reach `target`, `f` rising with it.
fn solve(range: (f32, f32), target: f32, f: impl Fn(f32) -> f32) -> f32 {
    let (mut lo, mut hi) = range;
    for _ in 0..30 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// The tail's color in each era.
fn tail_color(color: usize) -> [u8; 3] {
    match color {
        EIGHTIES => [255, 96, 206],
        VINTAGE => [245, 172, 86],
        _ => [70, 205, 222],
    }
}

const INPUT: [u8; 3] = [232, 210, 120];

fn tail_view(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut VerbState,
    (kind, color): (usize, usize),
) {
    let p = theme::current(ui);
    let width = ui.available_width().max(480.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 280.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    screen_panel(&painter, rect, &p);
    let plot = Rect::from_min_max(rect.min + vec2(46.0, 50.0), rect.max - vec2(16.0, 26.0));
    let axis = FreqAxis::new(plot);
    let span = (SECONDS.1 / SECONDS.0).ln();
    let y_of = |s: f32| {
        plot.bottom() - (s.clamp(SECONDS.0, SECONDS.1) / SECONDS.0).ln() / span * plot.height()
    };
    let s_of =
        |y: f32| SECONDS.0 * (((plot.bottom() - y) / plot.height()).clamp(0.0, 1.0) * span).exp();
    let y_of_db = |db: f32| plot.bottom() - ((db + 96.0) / 112.0).clamp(0.0, 1.0) * plot.height();
    let weak = p.weak;
    freq_grid(
        &painter,
        plot,
        axis,
        &p,
        &[50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0],
    );
    for s in [0.1f32, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0] {
        let y = y_of(s);
        let a = if s == 1.0 || s == 10.0 { 0.16 } else { 0.07 };
        painter.hline(plot.x_range(), y, Stroke::new(1.0, weak.gamma_multiply(a)));
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            format!("{s} s"),
            FontId::monospace(9.5),
            weak,
        );
    }
    // A beat and a bar at the project's tempo.
    let bar = bar_seconds(params);
    for (s, label) in [(bar / 4.0, tr("1 beat")), (bar, tr("1 bar"))] {
        if !(SECONDS.0..SECONDS.1).contains(&s) {
            continue;
        }
        let y = y_of(s);
        painter.extend(Shape::dashed_line(
            &[pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, weak.gamma_multiply(0.35)),
            3.0,
            5.0,
        ));
        painter.text(
            pos2(plot.right() - 4.0, y - 2.0),
            Align2::RIGHT_BOTTOM,
            label,
            FontId::proportional(10.0),
            weak.gamma_multiply(0.8),
        );
    }
    // The spectra: the tail as a glow in the era's color, the input as a line.
    let tail_rgb = tail_color(color);
    if let Some(points) = spectrum(meters.get(TAIL_SPECTRUM..), axis, y_of_db) {
        glow(&painter, &points, plot.bottom(), tail_rgb, 0.3);
        painter.add(Shape::line(points, Stroke::new(1.2, rgba(tail_rgb, 0.55))));
    }
    if let Some(points) = spectrum(meters.get(INPUT_SPECTRUM..), axis, y_of_db) {
        painter.add(Shape::line(points, Stroke::new(1.2, rgba(INPUT, 0.42))));
    }
    // The cuts.
    let (low, high) = (params.get("low_cut"), params.get("high_cut"));
    shade(&painter, plot, (plot.left(), axis.x(low)), LOW_CUT);
    shade(&painter, plot, (plot.right(), axis.x(high)), HIGH_CUT);
    if color != CLEAN {
        let x = axis.x(COLOR_BANDS[color]);
        painter.extend(Shape::dashed_line(
            &[pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(1.0, rgba(tail_rgb, 0.7)),
            4.0,
            4.0,
        ));
        painter.text(
            pos2(x - 6.0, plot.top() + 4.0),
            Align2::RIGHT_TOP,
            trf!(
                "{color}: up to {khz} kHz",
                color = tr(COLORS[color]),
                khz = format!("{:.0}", COLOR_BANDS[color] / 1000.0)
            ),
            FontId::proportional(10.5),
            rgb(tail_rgb),
        );
    }
    // The decay of each frequency, dimmer where the cuts take it away.
    let tail = Tail::of(params, kind);
    let ink = if p.dark {
        Color32::from_rgb(236, 239, 244)
    } else {
        p.text
    };
    let points: Vec<Pos2> = (0..=200)
        .map(|k| {
            let x = plot.left() + k as f32 / 200.0 * plot.width();
            pos2(x, y_of(tail.at(axis.hz(x))))
        })
        .collect();
    glow(&painter, &points, plot.bottom(), [236, 239, 244], 0.08);
    let (from, to) = (axis.x(low), axis.x(high));
    let heard: Vec<Pos2> = points
        .iter()
        .copied()
        .filter(|q| (from..=to).contains(&q.x))
        .collect();
    painter.add(Shape::line(
        points,
        Stroke::new(2.2, ink.gamma_multiply(0.35)),
    ));
    painter.add(Shape::line(heard, Stroke::new(2.2, ink)));

    // The points.
    let rooms = uses(kind, "crossover");
    let highs_hz = Tail::highs_hz(high);
    let decay_hz = tail.decay_hz(high);
    let on_curve = |hz: f32| pos2(axis.x(hz), y_of(tail.at(hz)));
    let points: Vec<(Point, Pos2, [u8; 3])> = [
        (Point::LowCut, on_curve(low), LOW_CUT),
        (Point::Lows, on_curve(tail.crossover), LOWS),
        (Point::Decay, on_curve(decay_hz), DECAY),
        (Point::Highs, on_curve(highs_hz), HIGHS),
        (Point::HighCut, on_curve(high), HIGH_CUT),
    ]
    .into_iter()
    .filter(|(k, ..)| rooms || *k != Point::Lows)
    .collect();
    let nearest = |at: Pos2| {
        points
            .iter()
            .filter(|(_, h, _)| h.distance(at) < 14.0)
            .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
            .map(|(k, ..)| *k)
    };
    if response.drag_started() {
        state.held = response.interact_pointer_pos().and_then(nearest);
    }
    let hovered = response.hover_pos().and_then(nearest);
    for (k, at, c) in &points {
        let lit = state.held == Some(*k) || hovered == Some(*k);
        handle(&painter, *at, *c, lit, theme::screen(&p));
    }
    if let Some(k) = state.held.or(hovered)
        && let Some((_, at, _)) = points.iter().find(|(q, ..)| *q == k)
    {
        let text = match k {
            Point::LowCut => trf!("Low cut {hz}", hz = format_hz(low)),
            Point::HighCut => trf!("High cut {hz}", hz = format_hz(high)),
            Point::Lows => trf!(
                "Lows {seconds} s below {hz}",
                seconds = format!("{:.2}", tail.at(30.0)),
                hz = format_hz(tail.crossover)
            ),
            Point::Decay => trf!("Decay {seconds} s", seconds = format!("{:.2}", tail.decay)),
            Point::Highs => trf!(
                "Highs {seconds} s at {hz}",
                seconds = format!("{:.2}", tail.at(highs_hz)),
                hz = format_hz(highs_hz)
            ),
        };
        bubble(&painter, plot, *at, text);
        ui.ctx().set_cursor_icon(if state.held.is_some() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    readout(&painter, rect, params, (kind, color), &p);
    legend(&painter, rect, tail_rgb, ink, &p);

    let response = response.on_hover_text(tr(
        "Drag the points: the decay, how long the lows and the highs ring, the cuts; double-click one to reset it",
    ));
    if response.double_clicked()
        && let Some(k) = hovered
    {
        let ids: &[&str] = match k {
            Point::LowCut => &["low_cut"],
            Point::HighCut => &["high_cut"],
            Point::Lows => &["bass", "crossover"],
            Point::Decay => &["decay"],
            Point::Highs => &["treble"],
        };
        for id in ids {
            let default = params.param(id).map(|q| q.default);
            params.set(id, default, &response);
        }
        params.end_gesture();
    }
    if let Some(k) = state.held {
        let at = response
            .interact_pointer_pos()
            .filter(|_| response.dragged());
        match k {
            Point::LowCut => {
                let hz = at.map(|at| axis.hz(at.x).clamp(20.0, 1000.0));
                params.change("low_cut", hz, &response);
            }
            Point::HighCut => {
                let hz = at.map(|at| axis.hz(at.x).clamp(1000.0, 20_000.0));
                params.change("high_cut", hz, &response);
            }
            Point::Lows => {
                let crossover = at.map(|at| axis.hz(at.x).clamp(80.0, 1000.0));
                let bass = at.zip(crossover).map(|(at, hz)| {
                    let mut t = tail;
                    t.crossover = hz;
                    solve((50.0, 200.0), s_of(at.y), |b| Tail { bass: b, ..t }.at(hz))
                });
                params.change("crossover", crossover, &response);
                params.change("bass", bass, &response);
            }
            Point::Decay => {
                let decay = at.map(|at| {
                    solve((0.2, 20.0), s_of(at.y), |d| {
                        Tail { decay: d, ..tail }.at(decay_hz)
                    })
                });
                params.change("decay", decay, &response);
            }
            Point::Highs => {
                let treble = at.map(|at| {
                    solve((10.0, 100.0), s_of(at.y), |t| {
                        Tail { treble: t, ..tail }.at(highs_hz)
                    })
                });
                params.change("treble", treble, &response);
            }
        }
        if !response.dragged() {
            state.held = None;
        }
    }
    let level = |k: usize| meters.get(k).copied().unwrap_or(-200.0);
    if level(0) > -90.0 || level(1) > -90.0 {
        crate::controls::animate(ui.ctx());
    }
}

/// The space and its color, the decay at the tempo and the pre-delay.
fn readout(
    painter: &egui::Painter,
    rect: Rect,
    params: &dyn EditorParams,
    (kind, color): (usize, usize),
    p: &theme::Palette,
) {
    let corner = rect.left_top() + vec2(16.0, 12.0);
    let title = format!("{} · {}", tr(TYPES[kind]), tr(COLORS[color]));
    let galley = painter.layout_no_wrap(title, theme::strong(11.5), p.weak);
    let width = galley.size().x;
    painter.galley(corner, galley, p.weak);
    if params.get("freeze") >= 0.5 {
        painter.text(
            corner + vec2(width + 10.0, 0.0),
            Align2::LEFT_TOP,
            tr("Frozen: the tail holds"),
            theme::strong(11.5),
            p.accent,
        );
    }
    let decay = params.get("decay");
    let bpm = params.tempo() as f32;
    let pre = if params.get("predelay_sync") >= 0.5 {
        audiostrada_dsp::delay::synced_seconds(params.get("predelay_note"), f64::from(bpm)).min(0.2)
            * 1000.0
    } else {
        params.get("predelay")
    };
    painter.text(
        corner + vec2(0.0, 18.0),
        Align2::LEFT_TOP,
        trf!(
            "Decay {seconds} s · {bars} bars at {bpm} BPM · pre-delay {pre} ms",
            seconds = format!("{decay:.2}"),
            bars = format!("{:.2}", decay / bar_seconds(params)),
            bpm = format!("{bpm:.0}"),
            pre = format!("{pre:.0}")
        ),
        FontId::monospace(12.0),
        p.text,
    );
}

/// What the lines and the glow are.
fn legend(painter: &egui::Painter, rect: Rect, tail: [u8; 3], ink: Color32, p: &theme::Palette) {
    let mut x = rect.right() - 16.0;
    let y = rect.top() + 18.0;
    let font = FontId::proportional(10.5);
    for (label, swatch) in [
        (tr("Decay"), ink),
        (tr("Tail"), rgba(tail, 0.8)),
        (tr("Input"), rgba(INPUT, 0.8)),
    ] {
        let galley = painter.layout_no_wrap(label.to_owned(), font.clone(), p.weak);
        x -= galley.size().x;
        painter.galley(pos2(x, y - galley.size().y / 2.0), galley, p.weak);
        x -= 20.0;
        painter.line_segment([pos2(x, y), pos2(x + 14.0, y)], Stroke::new(2.5, swatch));
        x -= 14.0;
    }
}

// ---------------------------------------------------------------- Controls

fn controls(ui: &mut Ui, params: &mut dyn EditorParams, kind: usize) {
    let p = theme::current(ui);
    let accent = p.accent;
    let panel = panel(
        ui,
        (3, 104.0),
        &[
            &[("Reflections", 3), ("Motion", 2), ("Ducking", 2)],
            &[("Tone", 5), ("Stereo", 1), ("Shimmer", 1)],
        ],
        (78.0, 150.0),
    );
    let synced = params.get("predelay_sync") >= 0.5;
    big_cells(ui, &panel.big, |ui, k| match k {
        0 => {
            wide_knob(ui, params, "decay", tr("Decay"), 72.0, rgb(DECAY));
            ui.add_space(4.0);
            toggle_chip(
                ui,
                params,
                "freeze",
                tr("Freeze"),
                tr("Holds the tail as it is, for ever; the input is not added"),
            );
        }
        1 => {
            if synced {
                note_knob(
                    ui,
                    params,
                    ("predelay_note", tr("Pre-delay")),
                    72.0,
                    accent,
                    Some(0.5),
                );
            } else {
                wide_knob(ui, params, "predelay", tr("Pre-delay"), 72.0, accent);
            }
            ui.add_space(4.0);
            toggle_chip(
                ui,
                params,
                "predelay_sync",
                tr("Sync"),
                tr("A note value at the project's tempo, up to 200 ms"),
            );
        }
        _ => {
            wide_knob(ui, params, "mix", tr("Mix"), 72.0, theme::SOLO);
            ui.add_space(28.0);
        }
    });
    let small: [[(&str, &str, Color32); 7]; 2] = [
        [
            ("size", "Size", accent),
            ("early", "Early / late", accent),
            ("diffusion", "Diffusion", accent),
            ("modulation", "Depth", accent),
            ("mod_rate", "Rate", accent),
            ("duck", "Amount", accent),
            ("duck_release", "Release", accent),
        ],
        [
            ("low_cut", "Low cut", rgb(LOW_CUT)),
            ("bass", "Lows", rgb(LOWS)),
            ("crossover", "Crossover", rgb(LOWS)),
            ("treble", "Highs", rgb(HIGHS)),
            ("high_cut", "High cut", rgb(HIGH_CUT)),
            ("width", "Width", accent),
            ("shimmer", "Amount", theme::SOLO),
        ],
    ];
    for (row, knobs) in panel.rows.iter().zip(small) {
        for (rect, (id, label, tint)) in row.iter().zip(knobs) {
            cell(ui, *rect, |ui| {
                ui.add_enabled_ui(uses(kind, id), |ui| {
                    wide_knob(ui, params, id, tr(label), 38.0, tint);
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput, Rect};

    /// The reverb's parameters, as an editor sees them.
    struct Values(Vec<f32>);

    impl EditorParams for Values {
        fn info(&self) -> &'static audiostrada_dsp::EffectInfo {
            &audiostrada_dsp::reverb::INFO
        }
        fn get(&self, id: &str) -> f32 {
            let i = self.info().params.iter().position(|p| p.id == id);
            i.map_or(0.0, |i| self.0[i])
        }
        fn change(&mut self, id: &str, changed: Option<f32>, _: &Response) {
            let i = self.info().params.iter().position(|p| p.id == id);
            if let (Some(i), Some(v)) = (i, changed) {
                self.0[i] = v;
            }
        }
        fn set_with(&mut self, id: &str, c: Option<f32>, r: &Response, _: &mut dyn FnMut(&mut Ui)) {
            self.change(id, c, r);
        }
        fn edit_many(&mut self, _: &str, _: &[(String, f32)]) {}
        fn sample_rate(&self) -> u32 {
            48_000
        }
        fn tempo(&self) -> f64 {
            120.0
        }
    }

    /// Draws the freeze switch with `events`; returns where it is.
    fn draw(ctx: &egui::Context, values: &mut Values, events: Vec<Event>) -> Rect {
        let mut place = Rect::NOTHING;
        let input = RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let before = ui.cursor().min;
            toggle_chip(ui, values, "freeze", "Freeze", "");
            place = Rect::from_min_size(before, vec2(40.0, 24.0));
        });
        place
    }

    #[test]
    fn a_chip_switch_turns_on_and_off_when_clicked() {
        let info = &audiostrada_dsp::reverb::INFO;
        let mut values = Values(info.params.iter().map(|p| p.default).collect());
        let ctx = egui::Context::default();
        for expected in [1.0, 0.0] {
            let at = draw(&ctx, &mut values, vec![]).center();
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            draw(
                &ctx,
                &mut values,
                vec![Event::PointerMoved(at), button(true)],
            );
            draw(&ctx, &mut values, vec![button(false)]);
            assert_eq!(values.get("freeze"), expected);
        }
    }
}
