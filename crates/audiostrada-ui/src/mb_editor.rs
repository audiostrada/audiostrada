//! Strada-MB-Comp's editor: the output spectrum split into its bands, each with its threshold
//! and lift lines to drag and its gain change hanging from the top, the crossovers dragged
//! sideways; below, the bands as tabs (solo, bypass) and the chosen band's settings.

use audiostrada_dsp::multiband::{BANDS, GAINS, LEVELS};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::drums_editor::{MODULE, SCREEN, modules, screen};
use crate::effect_editors::{Params, solo_band};
use crate::i18n::tr;
use crate::theme;

const COLORS: [Color32; BANDS] = [
    Color32::from_rgb(236, 112, 90),
    Color32::from_rgb(238, 184, 72),
    Color32::from_rgb(112, 204, 122),
    Color32::from_rgb(92, 172, 242),
];
const TOP_DB: f32 = 0.0;
const BOTTOM_DB: f32 = -72.0;

#[derive(Debug, Default)]
pub struct MbState {
    band: usize,
    solo: Option<usize>,
}

fn id(band: usize, field: &str) -> String {
    format!("b{}_{field}", band + 1)
}

/// The bands' names for a count.
fn names(count: usize) -> &'static [&'static str] {
    match count {
        2 => &["Low", "High"],
        3 => &["Low", "Mids", "High"],
        _ => &["Low", "Low mid", "High mid", "High"],
    }
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut MbState) {
    let count = params.get("count").round().clamp(0.0, 2.0) as usize + 2;
    state.band = state.band.min(count - 1);
    if state.solo.is_some_and(|s| s >= count) {
        state.solo = None;
        solo_band(params, None);
    }
    screen(ui, |ui| {
        graph(ui, params, meters, state, count);
        ui.add_space(10.0);
        tabs(ui, params, meters, state, count);
        ui.add_space(10.0);
        let b = state.band;
        let knob = |field: &str, label: &'static str| (id(b, field), tr(label));
        modules(
            ui,
            params,
            &[
                (
                    tr("Compress above"),
                    vec![knob("thresh", "Threshold"), knob("ratio", "Ratio")],
                ),
                (
                    tr("Lift below"),
                    vec![knob("below", "Threshold"), knob("up", "Ratio")],
                ),
                (
                    tr("Response"),
                    vec![knob("attack", "Attack"), knob("release", "Release")],
                ),
                (tr("Band"), vec![knob("gain", "Gain")]),
            ],
            COLORS[b],
            46.0,
        );
        ui.add_space(8.0);
        let global = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        modules(
            ui,
            params,
            &[
                (tr("Band split"), vec![global("count", "Bands")]),
                (
                    tr("Whole"),
                    vec![
                        global("time", "Time"),
                        global("mix", "Mix"),
                        global("output", "Output"),
                    ],
                ),
            ],
            Color32::from_rgb(170, 180, 200),
            46.0,
        );
    });
}

/// The spectrum and the bands over it.
fn graph(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut MbState, count: usize) {
    let width = ui.available_width();
    let (rect, back) = ui.allocate_exact_size(vec2(width, 300.0), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(10, 12, 17));
    let x_of = |f: f32| rect.left() + (f.max(20.0) / 20.0).log10() / 3.0 * rect.width();
    let f_of = |x: f32| 20.0 * 1000f32.powf(((x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    let plot = rect.shrink2(vec2(0.0, 14.0));
    let y_of = |db: f32| {
        let t = ((db - BOTTOM_DB) / (TOP_DB - BOTTOM_DB)).clamp(0.0, 1.0);
        plot.bottom() - t * plot.height()
    };
    let db_of = |y: f32| BOTTOM_DB + (plot.bottom() - y) / plot.height() * (TOP_DB - BOTTOM_DB);
    let grid = Color32::from_white_alpha(14);
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        let x = x_of(f);
        painter.vline(x, rect.y_range(), Stroke::new(1.0, grid));
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
            Color32::from_gray(110),
        );
    }
    for db in [-12.0, -24.0, -36.0, -48.0, -60.0] {
        let y = y_of(db);
        painter.hline(rect.x_range(), y, Stroke::new(1.0, grid));
        painter.text(
            pos2(rect.right() - 4.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            format!("{db}"),
            FontId::proportional(9.5),
            Color32::from_gray(110),
        );
    }
    // The bands' edges.
    let rate = params.doc.player().engine().sample_rate().max(1) as f32;
    let crossovers = audiostrada_dsp::multiband::crossovers(
        &[0.0, params.get("x1"), params.get("x2"), params.get("x3")],
        rate,
    );
    let mut edges = vec![rect.left()];
    edges.extend(crossovers.iter().take(count - 1).map(|f| x_of(*f)));
    edges.push(rect.right());
    let region = |b: usize| Rect::from_x_y_ranges(edges[b]..=edges[b + 1], rect.y_range());
    for (b, color) in COLORS.iter().enumerate().take(count) {
        let alpha = if b == state.band { 30 } else { 14 };
        let [r, g, bl, _] = color.to_array();
        painter.rect_filled(
            region(b),
            0.0,
            Color32::from_rgba_unmultiplied(r, g, bl, alpha),
        );
    }
    // The output spectrum.
    let levels = meters
        .get(..audiostrada_dsp::analyzer::BANDS)
        .unwrap_or(&[]);
    if !levels.is_empty() {
        let points: Vec<Pos2> = levels
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let f = audiostrada_dsp::analyzer::band_frequency(i);
                pos2(x_of(f), y_of(*m))
            })
            .collect();
        let mut mesh = egui::Mesh::default();
        for (i, pt) in points.iter().enumerate() {
            mesh.colored_vertex(*pt, Color32::from_rgba_unmultiplied(170, 190, 230, 60));
            mesh.colored_vertex(
                pos2(pt.x, plot.bottom()),
                Color32::from_rgba_unmultiplied(170, 190, 230, 8),
            );
            if i > 0 {
                let j = 2 * i as u32;
                mesh.add_triangle(j - 2, j - 1, j);
                mesh.add_triangle(j - 1, j + 1, j);
            }
        }
        painter.add(mesh);
        painter.add(Shape::line(
            points,
            Stroke::new(1.2, Color32::from_rgb(190, 205, 235)),
        ));
    }
    if back.clicked()
        && let Some(at) = back.interact_pointer_pos()
    {
        state.band = (0..count)
            .find(|b| region(*b).contains(at))
            .unwrap_or(state.band);
    }
    for (b, &color) in COLORS.iter().enumerate().take(count) {
        let area = region(b);
        let bypassed = params.get(&id(b, "bypass")) >= 0.5;
        let ink = if bypassed {
            color.gamma_multiply(0.35)
        } else {
            color
        };
        // The band's level, a bar along its right edge.
        let level = meters.get(LEVELS + b).copied().unwrap_or(-120.0);
        let bar = Rect::from_min_max(
            pos2(area.right() - 7.0, y_of(level)),
            pos2(area.right() - 3.0, plot.bottom()),
        );
        if level > BOTTOM_DB {
            painter.rect_filled(bar, 1.0, ink.gamma_multiply(0.7));
        }
        // The gain change, hanging from the top: reduction warm, lift green.
        let change = meters.get(GAINS + b).copied().unwrap_or(0.0);
        if change.abs() > 0.05 && !bypassed {
            let height = (change.abs() * 5.0).min(plot.height() * 0.6);
            let column = Rect::from_center_size(
                pos2(area.center().x, rect.top() + 4.0 + height / 2.0),
                vec2((area.width() * 0.3).clamp(10.0, 36.0), height),
            );
            let tint = if change < 0.0 {
                Color32::from_rgb(240, 120, 80)
            } else {
                Color32::from_rgb(110, 220, 130)
            };
            painter.rect_filled(column, 2.0, tint.gamma_multiply(0.75));
            painter.text(
                pos2(column.center().x, column.bottom() + 8.0),
                Align2::CENTER_CENTER,
                format!("{change:+.1}"),
                theme::strong(10.0),
                tint,
            );
        }
        painter.text(
            pos2(area.center().x, plot.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            tr(names(count)[b]),
            theme::strong(11.0),
            ink,
        );
        // The threshold, and the lift threshold while it lifts: lines to drag.
        let lifting = params.get(&id(b, "up")) > 1.01;
        for (field, dashed) in [("thresh", false), ("below", true)] {
            if dashed && !lifting {
                continue;
            }
            let key = id(b, field);
            let y = y_of(params.get(&key));
            let span = area.x_range().shrink(6.0);
            let handle = Rect::from_x_y_ranges(span, (y - 5.0)..=(y + 5.0));
            let response = ui
                .interact(
                    handle,
                    ui.scope_id().with(("mb-line", b, field)),
                    Sense::drag(),
                )
                .on_hover_cursor(egui::CursorIcon::ResizeVertical);
            let hot = response.hovered() || response.dragged();
            if response.drag_started() {
                state.band = b;
            }
            let changed = response
                .dragged()
                .then(|| {
                    response
                        .interact_pointer_pos()
                        .map(|at| db_of(at.y).round())
                })
                .flatten();
            params.set(&key, changed, &response);
            let stroke = Stroke::new(if hot { 2.4 } else { 1.6 }, ink);
            if dashed {
                painter.extend(Shape::dashed_line(
                    &[pos2(span.min, y), pos2(span.max, y)],
                    stroke,
                    6.0,
                    4.0,
                ));
            } else {
                painter.hline(span, y, stroke);
            }
            painter.text(
                pos2(span.min + 4.0, y - 2.0),
                Align2::LEFT_BOTTOM,
                format!(
                    "{} {:.0} dB",
                    if dashed { tr("Lift") } else { tr("Comp") },
                    params.get(&key)
                ),
                FontId::proportional(9.5),
                ink,
            );
        }
    }
    // The crossovers, dragged sideways.
    for k in 0..count - 1 {
        let key = format!("x{}", k + 1);
        let x = edges[k + 1];
        let handle = Rect::from_x_y_ranges((x - 5.0)..=(x + 5.0), rect.y_range());
        let response = ui
            .interact(
                handle,
                ui.scope_id().with(("mb-crossover", k)),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let hot = response.hovered() || response.dragged();
        let changed = response
            .dragged()
            .then(|| response.interact_pointer_pos().map(|at| f_of(at.x).round()))
            .flatten();
        params.set(&key, changed, &response);
        painter.vline(
            x,
            rect.y_range(),
            Stroke::new(
                if hot { 2.0 } else { 1.2 },
                Color32::from_white_alpha(if hot { 200 } else { 110 }),
            ),
        );
        let tag = Rect::from_center_size(pos2(x, rect.top() + 10.0), vec2(54.0, 16.0));
        painter.rect_filled(tag, 8.0, Color32::from_rgb(40, 46, 58));
        let f = crossovers[k];
        let label = if f >= 1000.0 {
            format!("{:.1} kHz", f / 1000.0)
        } else {
            format!("{f:.0} Hz")
        };
        painter.text(
            tag.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(9.5),
            Color32::from_gray(220),
        );
    }
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, Color32::from_white_alpha(20)),
        egui::StrokeKind::Inside,
    );
}

/// The bands as tabs: each its name, its gain change, its solo and its bypass.
fn tabs(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut MbState, count: usize) {
    let width = ui.available_width();
    let gap = 8.0;
    let (row, _) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
    let cell = (width - gap * (count - 1) as f32) / count as f32;
    for (b, &color) in COLORS.iter().enumerate().take(count) {
        let tab = Rect::from_min_size(
            pos2(row.left() + b as f32 * (cell + gap), row.top()),
            vec2(cell, 40.0),
        );
        let chosen = b == state.band;
        let painter = ui.painter();
        painter.rect_filled(
            tab,
            8.0,
            if chosen {
                color.gamma_multiply(0.22)
            } else {
                MODULE
            },
        );
        painter.rect_stroke(
            tab,
            8.0,
            Stroke::new(
                if chosen { 1.5 } else { 1.0 },
                if chosen {
                    color
                } else {
                    Color32::from_white_alpha(18)
                },
            ),
            egui::StrokeKind::Inside,
        );
        let pick = ui.interact(tab, ui.scope_id().with(("mb-tab", b)), Sense::click());
        if pick.clicked() {
            state.band = b;
        }
        painter.circle_filled(pos2(tab.left() + 14.0, tab.center().y), 5.0, color);
        painter.text(
            pos2(tab.left() + 26.0, tab.center().y),
            Align2::LEFT_CENTER,
            tr(names(count)[b]),
            theme::strong(12.5),
            Color32::from_gray(230),
        );
        let change = meters.get(GAINS + b).copied().unwrap_or(0.0);
        painter.text(
            pos2(tab.center().x + 18.0, tab.center().y),
            Align2::CENTER_CENTER,
            format!("{change:+.1} dB"),
            FontId::monospace(11.0),
            Color32::from_gray(170),
        );
        // Solo and bypass, as small lit squares at the right.
        let soloed = state.solo == Some(b);
        let bypass_key = id(b, "bypass");
        let bypassed = params.get(&bypass_key) >= 0.5;
        for (k, (label, lit, tint)) in [
            ("S", soloed, Color32::from_rgb(240, 200, 70)),
            ("", !bypassed, Color32::from_rgb(110, 210, 130)),
        ]
        .into_iter()
        .enumerate()
        {
            let button = Rect::from_center_size(
                pos2(tab.right() - 18.0 - (1 - k) as f32 * 28.0, tab.center().y),
                vec2(22.0, 22.0),
            );
            let response = ui
                .interact(
                    button,
                    ui.scope_id().with(("mb-button", b, k)),
                    Sense::click(),
                )
                .on_hover_text(if k == 0 {
                    tr("Solo: hear this band alone")
                } else {
                    tr("Band on or bypassed")
                });
            let painter = ui.painter();
            painter.rect_filled(
                button,
                5.0,
                if lit {
                    tint.gamma_multiply(0.85)
                } else {
                    SCREEN
                },
            );
            painter.rect_stroke(
                button,
                5.0,
                Stroke::new(1.0, tint.gamma_multiply(0.6)),
                egui::StrokeKind::Inside,
            );
            let ink = if lit { Color32::from_gray(20) } else { tint };
            if k == 0 {
                painter.text(
                    button.center(),
                    Align2::CENTER_CENTER,
                    label,
                    theme::strong(11.0),
                    ink,
                );
            } else {
                // A power sign: a ring open at the top and its stroke.
                let c = button.center() + vec2(0.0, 1.0);
                let ring: Vec<Pos2> = (0..=20)
                    .map(|i| {
                        let a = (-50.0 + 280.0 * i as f32 / 20.0f32).to_radians();
                        c + vec2(a.cos(), a.sin()) * 5.5
                    })
                    .collect();
                painter.add(Shape::line(ring, Stroke::new(1.6, ink)));
                painter.line_segment(
                    [c - vec2(0.0, 7.0), c - vec2(0.0, 1.5)],
                    Stroke::new(1.6, ink),
                );
            }
            if response.clicked() {
                if k == 0 {
                    state.solo = (!soloed).then_some(b);
                    solo_band(params, state.solo);
                } else {
                    params.set(
                        &bypass_key,
                        Some(if bypassed { 0.0 } else { 1.0 }),
                        &response,
                    );
                    params.doc.history_mut().end_gesture();
                }
            }
        }
    }
}
