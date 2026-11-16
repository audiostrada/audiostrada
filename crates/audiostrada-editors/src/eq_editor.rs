//! Strada-EQ's editor: the curve over the spectrum, its bands dragged, their panel below.

use audiostrada_dsp::eq::{BANDS as EQ_BANDS, BandType, Place};
use egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::controls::{
    EditorParams, format_value, pill_switch, section_frame, solo_band, wide_knob,
};
use crate::i18n::tr;
use crate::theme;

/// The EQ window's memory between frames.
#[derive(Debug, Default)]
pub struct EqState {
    /// The band shown in the panel.
    pub selected: usize,
    /// The band heard alone (listening only; stopped when the window closes).
    pub solo: Option<usize>,
    view: EqView,
}

/// How the EQ display looks: its range and its spectra.
#[derive(Debug)]
struct EqView {
    /// Index in [`EQ_RANGES`].
    range: usize,
    /// Index in [`ANALYZER_MODES`].
    analyzer: usize,
    /// Highest level of each band of the output spectrum while holding.
    hold: Option<Vec<f32>>,
}

impl Default for EqView {
    fn default() -> Self {
        Self {
            range: 2,
            analyzer: 2,
            hold: None,
        }
    }
}

/// Sets several parameters as one undo step.
fn edit_params(params: &mut dyn EditorParams, label: &str, changes: &[(String, f32)]) {
    params.edit_many(label, changes);
}

/// Placements of a band as on its handle, and what they mean.
const PLACE_SHORT: [&str; 5] = ["St", "L", "R", "M", "S"];
const PLACE_HINTS: [&str; 5] = [
    "Both channels",
    "Left channel only",
    "Right channel only",
    "Mid only: what both channels share",
    "Sides only: what differs between the channels",
];

/// Small buttons in a row, the current one lit; the index of the one clicked.
fn chips(
    ui: &mut Ui,
    labels: &[&str],
    hints: &[&'static str],
    current: Option<usize>,
) -> Option<usize> {
    let p = theme::current(ui);
    let mut clicked = None;
    ui.spacing_mut().item_spacing.x = 3.0;
    for (i, label) in labels.iter().enumerate() {
        let galley =
            ui.painter()
                .layout_no_wrap((*label).to_owned(), FontId::proportional(11.0), p.text);
        let size = vec2((galley.size().x + 16.0).max(30.0), 22.0);
        let (r, mut response) = ui.allocate_exact_size(size, Sense::click());
        let on = current == Some(i);
        let fill = match (on, response.hovered()) {
            (true, _) => p.accent.gamma_multiply(0.3),
            (false, true) => p.hover,
            (false, false) => p.surface,
        };
        let painter = ui.painter();
        painter.rect_filled(r, 6.0, fill);
        painter.rect_stroke(
            r,
            6.0,
            Stroke::new(1.0, if on { p.accent } else { p.border }),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            r.center() - galley.size() / 2.0,
            galley,
            if on { p.text } else { p.weak },
        );
        if let Some(hint) = hints.get(i) {
            response = response.on_hover_text(tr(hint));
        }
        if response.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// Above the EQ display: what the analyzer shows, and the range in dB.
fn eq_toolbar(ui: &mut Ui, state: &mut EqState) {
    let view = &mut state.view;
    let weak = theme::current(ui).weak;
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Analyzer")).size(11.0).color(weak));
        let modes = ANALYZER_MODES.map(tr);
        let hints = [
            "No spectrum",
            "The spectrum of the output",
            "The input as a line, the output filled",
        ];
        if let Some(m) = chips(ui, &modes, &hints, Some(view.analyzer)) {
            view.analyzer = m;
        }
        ui.add_space(6.0);
        let hold = [tr("Hold")];
        let lit = view.hold.is_some().then_some(0);
        let hint = ["Keep the highest level of the output spectrum; click again to clear"];
        if chips(ui, &hold, &hint, lit).is_some() {
            view.hold = match view.hold {
                Some(_) => None,
                None => Some(Vec::new()),
            };
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let labels: Vec<String> = EQ_RANGES
                .iter()
                .rev()
                .map(|(r, _)| format!("±{r}"))
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let n = EQ_RANGES.len();
            let current = Some(n - 1 - view.range.min(n - 1));
            if let Some(i) = chips(ui, &labels, &[], current) {
                view.range = n - 1 - i;
            }
            ui.label(RichText::new(tr("Range")).size(11.0).color(weak));
        });
    });
}

/// Heights of the EQ display in dB above and below 0, and the step of their grid.
const EQ_RANGES: [(f32, f32); 4] = [(3.0, 1.0), (6.0, 2.0), (12.0, 3.0), (30.0, 6.0)];
const ANALYZER_MODES: [&str; 3] = ["Off", "Output", "In + out"];

/// Color of an EQ band, spread around the wheel.
fn band_color(band: usize) -> Color32 {
    egui::ecolor::Hsva::new((band as f32 * 0.618_034 + 0.58).fract(), 0.55, 0.95, 1.0).into()
}

fn band_type_name(kind: BandType) -> &'static str {
    tr(audiostrada_dsp::eq::TYPES[kind.index()])
}

/// Equalizer in the style of the big studio EQs: double-click the curve to add a band,
/// drag a band to move it, the wheel for its Q, double-click it to remove it, right-click
/// for its type, dynamics and solo.
pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EqState) {
    let p = theme::current(ui);
    let rate = f64::from(params.sample_rate().max(1));
    let width = ui.available_width().max(600.0);
    eq_toolbar(ui, state);
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 340.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    let x_of = |f: f32| rect.left() + (f / 20.0).log10() / 3.0 * rect.width();
    let f_of = |x: f32| 20.0 * 1000f32.powf(((x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    let half = rect.height() / 2.0 - 14.0;
    let (db_range, db_step) = EQ_RANGES[state.view.range.min(EQ_RANGES.len() - 1)];
    let y_of = |db: f32| rect.center().y - db / db_range * half;
    let db_of = |y: f32| ((rect.center().y - y) / half * db_range).clamp(-30.0, 30.0);
    let grid = p.weak.gamma_multiply(0.16);
    for f in [
        30.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0,
    ] {
        let x = x_of(f);
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, grid),
        );
        let label = if f >= 1000.0 {
            format!("{}k", f / 1000.0)
        } else {
            format!("{f}")
        };
        painter.text(
            pos2(x + 3.0, rect.bottom() - 3.0),
            Align2::LEFT_BOTTOM,
            label,
            FontId::proportional(9.5),
            p.weak,
        );
    }
    let lines = (db_range / db_step).round() as i32;
    for k in -lines..=lines {
        let db = k as f32 * db_step;
        let y = y_of(db).clamp(rect.top() + 12.0, rect.bottom() - 2.0);
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(
                1.0,
                if db == 0.0 {
                    grid.gamma_multiply(2.2)
                } else {
                    grid
                },
            ),
        );
        painter.text(
            pos2(rect.right() - 4.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            format!("{db:+}"),
            FontId::proportional(9.5),
            p.weak,
        );
    }

    // Spectra, -90 to 0 dBFS over the full height: the input as a line, the output filled.
    let spectrum = |levels: &[f32]| -> Vec<Pos2> {
        levels
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let f = audiostrada_dsp::analyzer::band_frequency(i);
                let level = ((m + 90.0) / 90.0).clamp(0.0, 1.0);
                pos2(x_of(f), rect.bottom() - level * rect.height())
            })
            .collect()
    };
    let bins = audiostrada_dsp::analyzer::BANDS;
    let analyzer = &meters[..meters.len().min(bins)];
    let mode = state.view.analyzer;
    if mode == 2
        && let Some(input) = meters.get(audiostrada_dsp::eq::INPUT_SPECTRUM..)
        && input.len() >= bins
    {
        let mut points = spectrum(&input[..bins]);
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        painter.add(Shape::line(
            points,
            Stroke::new(1.0, p.weak.gamma_multiply(0.45)),
        ));
    }
    if let Some(hold) = &mut state.view.hold {
        hold.resize(analyzer.len(), -120.0);
        for (h, m) in hold.iter_mut().zip(analyzer) {
            *h = h.max(*m);
        }
        let mut points = spectrum(hold);
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        painter.add(Shape::line(
            points,
            Stroke::new(1.0, theme::SOLO.gamma_multiply(0.7)),
        ));
    }
    if mode > 0 && !analyzer.is_empty() {
        let mut points = spectrum(analyzer);
        // Filled from the spectrum down, fading toward the bottom.
        let mut mesh = egui::Mesh::default();
        let (top, bottom) = (p.accent.gamma_multiply(0.18), p.accent.gamma_multiply(0.02));
        for pair in points.windows(2) {
            let i = mesh.vertices.len() as u32;
            for (q, c) in [
                (pair[0], top),
                (pair[1], top),
                (pos2(pair[1].x, rect.bottom()), bottom),
                (pos2(pair[0].x, rect.bottom()), bottom),
            ] {
                mesh.colored_vertex(q, c);
            }
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        painter.add(Shape::mesh(mesh));
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        painter.add(Shape::line(
            points,
            Stroke::new(1.0, p.accent.gamma_multiply(0.45)),
        ));
    }

    let values = params.values();
    let offsets: Vec<f32> = (0..EQ_BANDS)
        .map(|b| {
            meters
                .get(audiostrada_dsp::analyzer::BANDS + b)
                .copied()
                .unwrap_or(0.0)
        })
        .collect();
    let bands: Vec<(usize, audiostrada_dsp::eq::Band)> = (0..EQ_BANDS)
        .filter_map(|b| audiostrada_dsp::eq::band(&values, b).map(|band| (b, band)))
        .collect();
    if state.selected >= EQ_BANDS || !bands.iter().any(|(b, _)| *b == state.selected) {
        state.selected = bands.first().map_or(0, |(b, _)| *b);
    }
    let steps = 240;
    let freq_at = |i: usize| 20.0 * 1000f32.powf(i as f32 / steps as f32);
    let zero = y_of(0.0);
    // Each band's shape, filled from 0 dB; the selected one stronger.
    for &(b, band) in &bands {
        let curve: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let f = freq_at(i);
                let db = audiostrada_dsp::eq::band_db(&values, b, offsets[b], f64::from(f), rate)
                    .unwrap_or(0.0);
                pos2(
                    x_of(f),
                    y_of((db as f32).clamp(-30.0, 30.0)).clamp(rect.top(), rect.bottom()),
                )
            })
            .collect();
        let selected = b == state.selected;
        let color = band_color(b);
        let alpha = if band.bypass {
            0.0
        } else if selected {
            0.22
        } else {
            0.08
        };
        if alpha > 0.0 {
            let mut mesh = egui::Mesh::default();
            let fill = color.gamma_multiply(alpha);
            for pair in curve.windows(2) {
                let i = mesh.vertices.len() as u32;
                for q in [
                    pair[0],
                    pair[1],
                    pos2(pair[1].x, zero),
                    pos2(pair[0].x, zero),
                ] {
                    mesh.colored_vertex(q, fill);
                }
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
            painter.add(Shape::mesh(mesh));
        }
        if selected {
            painter.add(Shape::line(
                curve,
                Stroke::new(1.2, color.gamma_multiply(0.8)),
            ));
        }
    }
    let total: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let f = freq_at(i);
            let db = audiostrada_dsp::eq::response_db(&values, &offsets, f64::from(f), rate);
            pos2(x_of(f), y_of((db as f32).clamp(-30.0, 30.0)))
        })
        .collect();
    painter.add(Shape::line(total, Stroke::new(2.2, p.text)));

    // Band handles: on the curve for cuts, notches and band passes, at their gain otherwise.
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let handle_of = |b: usize, band: &audiostrada_dsp::eq::Band| {
        let db = if band.kind.has_gain() {
            band.gain
        } else {
            audiostrada_dsp::eq::band_db(&values, b, 0.0, f64::from(band.freq), rate).unwrap_or(0.0)
                as f32
        };
        pos2(
            x_of(band.freq),
            y_of(db.clamp(-db_range, db_range)).clamp(rect.top() + 9.0, rect.bottom() - 9.0),
        )
    };
    let handles: Vec<(usize, Pos2)> = bands
        .iter()
        .map(|(b, band)| (*b, handle_of(*b, band)))
        .collect();
    // The nearest handle within reach of a point: generous, the dots are small.
    let pick = |q: Pos2| {
        handles
            .iter()
            .filter(|(_, c)| c.distance(q) < 18.0)
            .min_by(|a, b| a.1.distance(q).total_cmp(&b.1.distance(q)))
            .map(|h| h.0)
    };
    let hovered = pointer.and_then(pick);
    for (&(b, band), &(_, center)) in bands.iter().zip(&handles) {
        let color = band_color(b);
        // A dynamic band: its range as a line, the gain it has now as a ring.
        if let Some((_, range)) = band.dynamic {
            let reach = y_of((band.gain + range).clamp(-db_range, db_range));
            painter.line_segment(
                [center, pos2(center.x, reach)],
                Stroke::new(3.0, color.gamma_multiply(0.35)),
            );
            let now = pos2(
                center.x,
                y_of((band.gain + offsets[b]).clamp(-db_range, db_range)),
            );
            painter.circle_stroke(now, 6.0, Stroke::new(2.0, color));
        }
        let r = if Some(b) == hovered || b == state.selected {
            9.0
        } else {
            7.5
        };
        if band.bypass {
            painter.circle_stroke(center, r, Stroke::new(1.5, color.gamma_multiply(0.6)));
        } else {
            painter.circle_filled(center, r, color);
            painter.circle_stroke(
                center,
                r,
                Stroke::new(if b == state.selected { 2.0 } else { 1.0 }, p.base),
            );
        }
        painter.text(
            center,
            Align2::CENTER_CENTER,
            (b + 1).to_string(),
            theme::strong(9.5),
            if band.bypass {
                color
            } else {
                Color32::from_rgb(20, 20, 24)
            },
        );
        if band.place != Place::Stereo {
            painter.text(
                center + vec2(0.0, r + 7.0),
                Align2::CENTER_CENTER,
                PLACE_SHORT[band.place as usize],
                theme::strong(9.0),
                color,
            );
        }
    }
    if bands.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr("Double-click to add a band"),
            theme::strong(14.0),
            p.weak,
        );
    }
    // Where the pointer is: its frequency, note and gain, in the corner.
    if let Some(at) = pointer.filter(|q| rect.contains(*q)) {
        let f = f_of(at.x);
        let note = {
            const NAMES: [&str; 12] = [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ];
            let n = (69.0 + 12.0 * (f / 440.0).log2()).round() as i32;
            format!(
                "{}{}",
                NAMES[n.rem_euclid(12) as usize],
                n.div_euclid(12) - 1
            )
        };
        let freq = if f >= 1000.0 {
            format!("{:.2} kHz", f / 1000.0)
        } else {
            format!("{f:.0} Hz")
        };
        painter.text(
            rect.left_top() + vec2(10.0, 8.0),
            Align2::LEFT_TOP,
            format!("{freq}  {note}  {:+.1} dB", db_of(at.y)),
            FontId::monospace(11.0),
            p.weak,
        );
        painter.line_segment(
            [pos2(at.x, rect.top()), pos2(at.x, rect.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.18)),
        );
    }

    // Mouse: drag a band (frequency and gain), wheel for Q, double-click to add or remove.
    let id = ui.scope_id().with(("eq-drag", params.salt()));
    let drag = ui.interact(rect, id, Sense::click_and_drag());
    // Picked where the button went down: the drag starts once the pointer has moved off it.
    if drag.drag_started()
        && let Some(b) = ui
            .input(|i| i.pointer.press_origin())
            .and_then(pick)
            .or(hovered)
    {
        state.selected = b;
        ui.data_mut(|d| d.insert_temp(id, b));
    }
    let grabbed: Option<usize> = ui.data(|d| d.get_temp(id));
    if let Some(b) = grabbed
        && let Some(band) = audiostrada_dsp::eq::band(&values, b)
    {
        // Ctrl: up narrows the band (higher Q), down widens it; else frequency and gain.
        let ctrl = ui.input(|i| i.modifiers.command);
        let at = drag
            .interact_pointer_pos()
            .filter(|_| drag.dragged() && !ctrl);
        let freq = audiostrada_dsp::eq::param_id(b, "freq");
        params.change(&freq, at.map(|q| f_of(q.x)), &drag);
        if band.kind.has_gain() {
            let gain = audiostrada_dsp::eq::param_id(b, "gain");
            params.change(&gain, at.map(|q| db_of(q.y)), &drag);
        }
        let q = audiostrada_dsp::eq::param_id(b, "q");
        let dy = drag.drag_delta().y;
        let narrowed =
            (ctrl && drag.dragged() && dy != 0.0).then(|| params.get(&q) * 2f32.powf(-dy / 80.0));
        params.change(&q, narrowed, &drag);
        if drag.drag_stopped() {
            ui.data_mut(|d| d.remove::<usize>(id));
        }
    }
    if hovered.is_some() || grabbed.is_some() {
        let ctrl = ui.input(|i| i.modifiers.command);
        ui.ctx().set_cursor_icon(if ctrl {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::Grab
        });
    }
    if let Some(b) = hovered {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            let q = audiostrada_dsp::eq::param_id(b, "q");
            let factor = if wheel > 0.0 { 1.06 } else { 1.0 / 1.06 };
            params.change(&q, Some(params.get(&q) * factor), &drag);
            params.end_gesture();
        }
        if drag.clicked() {
            state.selected = b;
        }
    }
    if drag.double_clicked()
        && let Some(at) = drag.interact_pointer_pos()
    {
        match hovered {
            Some(b) => remove_band(params, state, b),
            None => add_band(params, state, f_of(at.x), db_of(at.y)),
        }
    }
    if drag.secondary_clicked()
        && let Some(b) = hovered
    {
        state.selected = b;
        ui.data_mut(|d| d.insert_temp(id.with("menu"), b));
    }
    let menu_band: Option<usize> = ui.data(|d| d.get_temp(id.with("menu")));
    egui::Popup::context_menu(&drag)
        .id(id.with("band-menu"))
        .show(|ui| {
            match menu_band.and_then(|b| audiostrada_dsp::eq::band(&values, b).map(|x| (b, x))) {
                Some((b, band)) => band_menu(ui, params, state, b, &band),
                None => {
                    ui.weak(tr("Double-click the curve to add a band"));
                }
            }
        });
    let hint = match hovered.and_then(|b| audiostrada_dsp::eq::band(&values, b).map(|x| (b, x))) {
        Some((b, band)) => {
            let mut text = format!(
                "{} {}: {}, {}",
                tr("Band"),
                b + 1,
                band_type_name(band.kind),
                format_value(&audiostrada_dsp::eq::INFO.params[3], band.freq)
            );
            if band.kind.has_gain() {
                text.push_str(&format!(", {:+.1} dB", band.gain));
            }
            text.push_str(&format!(", Q {:.2}", band.q));
            format!(
                "{text}\n{}",
                tr(
                    "Drag to move, Ctrl+drag or wheel for Q, double-click to remove, \
                     right-click for more"
                )
            )
        }
        None => tr("Double-click to add a band, drag a band to move it").to_owned(),
    };
    drag.on_hover_text(hint);

    ui.add_space(6.0);
    band_panel(ui, params, state, &bands, &offsets);
}

/// Puts a band in the first free slot, at `freq` and `gain`.
pub fn add_band(params: &mut dyn EditorParams, state: &mut EqState, freq: f32, gain: f32) {
    let Some(b) = (0..EQ_BANDS).find(|&b| audiostrada_dsp::eq::band(&params.values(), b).is_none())
    else {
        params.notify(tr("The EQ has 16 bands at most").into());
        return;
    };
    let kind = audiostrada_dsp::eq::new_band_type(freq);
    let id = |field: &str| audiostrada_dsp::eq::param_id(b, field);
    let changes = vec![
        (id("used"), 1.0),
        (id("bypass"), 0.0),
        (id("type"), kind.index() as f32),
        (id("freq"), freq),
        (id("gain"), if kind.has_gain() { gain } else { 0.0 }),
        (id("q"), if kind == BandType::Bell { 1.0 } else { 0.707 }),
        (id("slope"), 1.0),
        (id("dyn"), 0.0),
    ];
    edit_params(params, tr("Add EQ band"), &changes);
    state.selected = b;
}

pub fn remove_band(params: &mut dyn EditorParams, state: &mut EqState, b: usize) {
    if state.solo == Some(b) {
        state.solo = None;
        solo_band(params, None);
    }
    edit_params(
        params,
        tr("Remove EQ band"),
        &[(audiostrada_dsp::eq::param_id(b, "used"), 0.0)],
    );
}

/// Right-click menu of a band: its shape, slope and placement, its switches, quick changes of
/// its gain, its removal.
fn band_menu(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    state: &mut EqState,
    b: usize,
    band: &audiostrada_dsp::eq::Band,
) {
    let id = |field: &str| audiostrada_dsp::eq::param_id(b, field);
    let color = band_color(b);
    ui.set_width(180.0);
    ui.label(
        RichText::new(format!("{} {}", tr("Band"), b + 1))
            .strong()
            .color(color),
    );
    if let Some(kind) = type_picker(ui, band.kind, color) {
        edit_params(
            params,
            tr("EQ band type"),
            &[(id("type"), kind.index() as f32)],
        );
        ui.close();
    }
    if band.kind.has_slope() {
        ui.horizontal(|ui| {
            if let Some(i) = chips(
                ui,
                &SLOPE_SHORT,
                &audiostrada_dsp::eq::SLOPES,
                Some(band.slope),
            ) {
                edit_params(params, tr("EQ band slope"), &[(id("slope"), i as f32)]);
            }
        });
    }
    ui.horizontal(|ui| {
        if let Some(i) = chips(ui, &PLACE_SHORT, &PLACE_HINTS, Some(band.place as usize)) {
            edit_params(params, tr("EQ band placement"), &[(id("place"), i as f32)]);
        }
    });
    ui.separator();
    if band.kind.can_be_dynamic() {
        let mut dynamic = band.dynamic.is_some();
        if ui.checkbox(&mut dynamic, tr("Dynamic")).changed() {
            let on = if dynamic { 1.0 } else { 0.0 };
            edit_params(params, tr("Dynamic EQ band"), &[(id("dyn"), on)]);
        }
    }
    let mut soloed = state.solo == Some(b);
    if ui.checkbox(&mut soloed, tr("Solo")).changed() {
        state.solo = soloed.then_some(b);
        solo_band(params, state.solo);
    }
    let mut bypassed = band.bypass;
    if ui.checkbox(&mut bypassed, tr("Bypass")).changed() {
        let off = if bypassed { 1.0 } else { 0.0 };
        edit_params(params, tr("Bypass EQ band"), &[(id("bypass"), off)]);
    }
    if band.kind.has_gain() && band.gain.abs() > 0.05 {
        ui.separator();
        if ui.button(tr("Gain to 0 dB")).clicked() {
            edit_params(params, tr("EQ band gain"), &[(id("gain"), 0.0)]);
            ui.close();
        }
        if ui.button(tr("Flip the gain")).clicked() {
            edit_params(params, tr("EQ band gain"), &[(id("gain"), -band.gain)]);
            ui.close();
        }
    }
    ui.separator();
    if ui
        .button(RichText::new(tr("Remove")).color(theme::RECORD))
        .clicked()
    {
        remove_band(params, state, b);
        ui.close();
    }
}

/// The slopes of the cuts, as their chips show them (dB per octave in the hints).
const SLOPE_SHORT: [&str; 6] = ["6", "12", "18", "24", "36", "48"];

/// A small drawing of a band type's curve in `rect`, around its 0 dB line.
fn shape_glyph(painter: &egui::Painter, rect: Rect, kind: BandType, color: Color32) {
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let bump = |t: f32, width: f32| (-((t - 0.5) / width).powi(2)).exp();
    let level = |t: f32| match kind {
        BandType::Bell => bump(t, 0.16),
        BandType::LowShelf => 1.0 - smooth((t - 0.25) / 0.45),
        BandType::HighShelf => smooth((t - 0.3) / 0.45),
        BandType::LowCut => -1.2 * (1.0 - smooth((t - 0.02) / 0.4)),
        BandType::HighCut => -1.2 * smooth((t - 0.58) / 0.4),
        BandType::Notch => -1.2 * bump(t, 0.06),
        BandType::BandPass => -1.2 * (1.0 - bump(t, 0.2)),
        BandType::Tilt => (t - 0.5) * 1.8,
    };
    let middle = rect.center().y;
    let amp = rect.height() * 0.4;
    painter.hline(
        rect.x_range(),
        middle,
        Stroke::new(1.0, color.gamma_multiply(0.25)),
    );
    let points: Vec<Pos2> = (0..=28)
        .map(|i| {
            let t = i as f32 / 28.0;
            pos2(rect.left() + t * rect.width(), middle - level(t) * amp)
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.6, color)));
}

/// The band types as drawings in two rows across the width, the current one lit in `color`;
/// the type clicked.
fn type_picker(ui: &mut Ui, current: BandType, color: Color32) -> Option<BandType> {
    let p = theme::current(ui);
    let mut picked = None;
    let cell = ((ui.available_width() - 9.0) / 4.0).clamp(30.0, 48.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        for row in BandType::ALL.chunks(4) {
            ui.horizontal(|ui| {
                for &kind in row {
                    let (rect, response) = ui.allocate_exact_size(vec2(cell, 24.0), Sense::click());
                    let on = kind == current;
                    let fill = if on {
                        color.gamma_multiply(0.2)
                    } else if response.hovered() {
                        p.hover
                    } else {
                        p.surface
                    };
                    let painter = ui.painter();
                    painter.rect_filled(rect, 4.0, fill);
                    painter.rect_stroke(
                        rect,
                        4.0,
                        Stroke::new(1.0, if on { color } else { p.border }),
                        egui::StrokeKind::Inside,
                    );
                    let ink = if on { color } else { p.weak };
                    shape_glyph(painter, rect.shrink2(vec2(6.0, 5.0)), kind, ink);
                    if response.on_hover_text(band_type_name(kind)).clicked() && !on {
                        picked = Some(kind);
                    }
                }
            });
        }
    });
    picked
}

/// The bands as colored chips, and the controls of the selected one.
fn band_panel(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    state: &mut EqState,
    bands: &[(usize, audiostrada_dsp::eq::Band)],
    offsets: &[f32],
) {
    let p = theme::current(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for &(b, band) in bands {
            let color = band_color(b);
            let selected = b == state.selected;
            let (r, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            if band.bypass {
                ui.painter()
                    .circle_stroke(r.center(), 9.0, Stroke::new(1.5, color));
            } else {
                ui.painter().circle_filled(r.center(), 9.0, color);
            }
            if selected {
                ui.painter()
                    .circle_stroke(r.center(), 11.0, Stroke::new(1.5, p.text));
            }
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                (b + 1).to_string(),
                theme::strong(10.0),
                if band.bypass {
                    color
                } else {
                    Color32::from_rgb(20, 20, 24)
                },
            );
            if response
                .on_hover_text(format!(
                    "{} {}: {}",
                    tr("Band"),
                    b + 1,
                    band_type_name(band.kind)
                ))
                .clicked()
            {
                state.selected = b;
            }
        }
        if bands.is_empty() {
            ui.weak(tr("Double-click the curve to add a band"));
        }
        if let Some(b) = state.solo {
            ui.add_space(8.0);
            ui.colored_label(theme::SOLO, crate::trf!("Band {band} alone", band = b + 1));
        }
    });
    ui.add_space(6.0);
    let accent = p.accent;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(&(b, band)) = bands.iter().find(|(b, _)| *b == state.selected) {
            let color = band_color(b);
            let id = |field: &str| audiostrada_dsp::eq::param_id(b, field);
            let title = format!("{} {}", tr("Band"), b + 1);
            eq_section(ui, &title, color, |ui| {
                if let Some(kind) = type_picker(ui, band.kind, color) {
                    edit_params(
                        params,
                        tr("EQ band type"),
                        &[(id("type"), kind.index() as f32)],
                    );
                }
                if band.kind.has_slope() {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let current = Some(band.slope);
                        if let Some(i) =
                            chips(ui, &SLOPE_SHORT, &audiostrada_dsp::eq::SLOPES, current)
                        {
                            edit_params(params, tr("EQ band slope"), &[(id("slope"), i as f32)]);
                        }
                    });
                }
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let current = Some(band.place as usize);
                    if let Some(i) = chips(ui, &PLACE_SHORT, &PLACE_HINTS, current) {
                        edit_params(params, tr("EQ band placement"), &[(id("place"), i as f32)]);
                    }
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mut on = !band.bypass;
                    if pill_switch(ui, on, tr("On")).clicked() {
                        on = !on;
                        edit_params(
                            params,
                            tr("Bypass EQ band"),
                            &[(id("bypass"), if on { 0.0 } else { 1.0 })],
                        );
                    }
                    let soloed = state.solo == Some(b);
                    let response = pill_switch(ui, soloed, tr("Solo"))
                        .on_hover_text(tr("Hear only the frequencies this band works on"));
                    if response.clicked() {
                        state.solo = (!soloed).then_some(b);
                        solo_band(params, state.solo);
                    }
                    if ui.button("✕").on_hover_text(tr("Remove")).clicked() {
                        remove_band(params, state, b);
                    }
                });
            });
            eq_section(ui, tr("Shape"), p.weak, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    wide_knob(ui, params, &id("freq"), tr("Frequency"), 44.0, color);
                    if band.kind.has_gain() {
                        wide_knob(ui, params, &id("gain"), tr("Gain"), 44.0, color);
                    }
                    wide_knob(ui, params, &id("q"), "Q", 44.0, color);
                });
            });
            eq_section(ui, tr("Dynamics"), p.weak, |ui| {
                if !band.kind.can_be_dynamic() {
                    ui.weak(tr("Bells and shelves can be dynamic"));
                    return;
                }
                let dynamic = band.dynamic.is_some();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_enabled_ui(dynamic, |ui| {
                        for (field, name) in [
                            ("thresh", tr("Threshold")),
                            ("range", tr("Range")),
                            ("attack", tr("Attack")),
                            ("release", tr("Release")),
                        ] {
                            wide_knob(ui, params, &id(field), name, 44.0, theme::SOLO);
                        }
                    });
                });
                ui.add_space(4.0);
                let response = pill_switch(ui, dynamic, tr("Dynamic")).on_hover_text(tr(
                    "The gain moves by up to the range while these frequencies are above the \
                     threshold",
                ));
                if response.clicked() {
                    edit_params(
                        params,
                        tr("Dynamic EQ band"),
                        &[(id("dyn"), if dynamic { 0.0 } else { 1.0 })],
                    );
                }
                if let Some((_, range)) = band.dynamic {
                    let what = if range < 0.0 {
                        crate::trf!(
                            "Cuts up to {db} dB while these frequencies pass the threshold",
                            db = format!("{:.1}", -range)
                        )
                    } else {
                        crate::trf!(
                            "Boosts up to {db} dB while these frequencies pass the threshold",
                            db = format!("{range:.1}")
                        )
                    };
                    let now = offsets.get(b).copied().unwrap_or(0.0);
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!("{what} · {:+.1} dB", now))
                            .size(10.5)
                            .color(p.weak),
                    );
                }
            });
        }
        // The output last, at the right end of the row.
        {
            eq_section(ui, tr("Output"), p.weak, |ui| {
                ui.horizontal(|ui| {
                    wide_knob(ui, params, "output", tr("Gain"), 44.0, accent);
                });
                ui.add_space(4.0);
                let values = params.values();
                let rate = f64::from(params.sample_rate().max(1));
                params.toggle(ui, "auto_gain", |ui, on| {
                    let auto = audiostrada_dsp::eq::auto_gain_db(&values, rate);
                    let label = if *on {
                        format!("{} {auto:+.1} dB", tr("Auto"))
                    } else {
                        tr("Auto gain").to_owned()
                    };
                    let response = pill_switch(ui, *on, &label).on_hover_text(tr(
                        "Makes up for the boosts and cuts, so the level stays about the same",
                    ));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            });
        }
    });
}

/// A titled panel of the EQ's controls, all of one height.
fn eq_section(ui: &mut Ui, title: &str, color: Color32, add: impl FnOnce(&mut Ui)) {
    section_frame(ui, title, color, 112.0, add);
}
