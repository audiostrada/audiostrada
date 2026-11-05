//! Strada-Mod's editor: the modes as tabs, a screen drawing what moves (the level, the side,
//! the pitch, the cutter's steps to click, the ring's tone) with its playhead, then the
//! rhythm, the shape and the output.

use audiostrada_dsp::filter::lfo_value;
use audiostrada_dsp::modulator::{CUTTER, MODE_HINTS, MODES, PAN, PATTERN, RING, VIBRATO};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::drums_editor::{MODULE, modules, screen};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;

const ACCENT: Color32 = Color32::from_rgb(178, 132, 240);
const RIGHT: Color32 = Color32::from_rgb(110, 200, 230);

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    let mode = (params.get("mode").round() as usize).min(MODES.len() - 1);
    screen(ui, |ui| {
        tabs(ui, params, mode);
        ui.add_space(10.0);
        display(ui, params, meters, mode);
        ui.add_space(10.0);
        let k = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        let synced = params.get("sync") >= 0.5;
        let mut rhythm = vec![k("sync", "Sync")];
        rhythm.push(if synced {
            k("note", "Note")
        } else {
            k("rate", "Rate")
        });
        let mut list = Vec::new();
        match mode {
            CUTTER => {
                rhythm.push(k("steps", "Steps"));
                list.push((tr("Rhythm"), rhythm));
                list.push((
                    tr("Gate"),
                    vec![
                        k("gate", "Gate length"),
                        k("depth", "Depth"),
                        k("smooth", "Smooth"),
                    ],
                ));
            }
            RING => {
                list.push((
                    tr("Carrier"),
                    vec![k("freq", "Frequency"), k("depth", "Depth")],
                ));
            }
            PAN => {
                list.push((tr("Rhythm"), rhythm));
                list.push((
                    tr("Movement"),
                    vec![
                        k("shape", "Shape"),
                        k("depth", "Depth"),
                        k("smooth", "Smooth"),
                    ],
                ));
            }
            _ => {
                list.push((tr("Rhythm"), rhythm));
                list.push((
                    tr("Movement"),
                    vec![
                        k("shape", "Shape"),
                        k("depth", "Depth"),
                        k("smooth", "Smooth"),
                        k("phase", "Stereo phase"),
                    ],
                ));
            }
        }
        list.push((tr("Output"), vec![k("mix", "Mix"), k("output", "Output")]));
        let list: Vec<(&str, Vec<(String, &str)>)> =
            list.iter().map(|(t, v)| (*t, v.clone())).collect();
        modules(ui, params, &list, ACCENT, 46.0);
    });
    ui.ctx().request_repaint();
}

/// The modes, each a tab with its hint.
fn tabs(ui: &mut Ui, params: &mut Params, mode: usize) {
    let width = ui.available_width();
    let gap = 6.0;
    let (row, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    let cell = (width - gap * (MODES.len() - 1) as f32) / MODES.len() as f32;
    for (k, name) in MODES.iter().enumerate() {
        let tab = Rect::from_min_size(
            pos2(row.left() + k as f32 * (cell + gap), row.top()),
            vec2(cell, 34.0),
        );
        let chosen = k == mode;
        let response = ui
            .interact(tab, ui.scope_id().with(("mod-tab", k)), Sense::click())
            .on_hover_text(tr(MODE_HINTS[k]));
        let painter = ui.painter();
        painter.rect_filled(
            tab,
            8.0,
            if chosen {
                ACCENT.gamma_multiply(0.3)
            } else if response.hovered() {
                Color32::from_rgb(34, 39, 50)
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
                    ACCENT
                } else {
                    Color32::from_white_alpha(18)
                },
            ),
            egui::StrokeKind::Inside,
        );
        painter.text(
            tab.center(),
            Align2::CENTER_CENTER,
            tr(name),
            theme::strong(13.0),
            if chosen {
                Color32::WHITE
            } else {
                Color32::from_gray(180)
            },
        );
        let changed = (response.clicked() && !chosen).then_some(k as f32);
        params.set("mode", changed, &response);
        if changed.is_some() {
            params.doc.history_mut().end_gesture();
        }
    }
}

/// What moves, drawn over two cycles with the playhead; the cutter's steps to click.
fn display(ui: &mut Ui, params: &mut Params, meters: &[f32], mode: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 180.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(10, 12, 17));
    let inner = rect.shrink2(vec2(14.0, 18.0));
    let phase = meters.get(2).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    let depth = params.get("depth") / 100.0;
    let shape = params.get("shape").round().max(0.0) as usize;
    let offset = params.get("phase") / 360.0;
    let grid = Color32::from_white_alpha(14);
    let held = |cycle: usize, side: usize| {
        let x = ((cycle * 7 + side * 13 + 3) as f32 * 12.9898).sin() * 43_758.547;
        (x - x.floor()) * 2.0 - 1.0
    };
    match mode {
        CUTTER => cutter(ui, &painter, inner, params, meters),
        RING => {
            let freq = params.get("freq");
            let cycles = (freq / 40.0).clamp(2.0, 40.0);
            let points: Vec<Pos2> = (0..=400)
                .map(|i| {
                    let t = i as f32 / 400.0;
                    let v = (t * cycles * std::f32::consts::TAU).sin();
                    pos2(
                        inner.left() + t * inner.width(),
                        inner.center().y - v * depth * inner.height() / 2.0,
                    )
                })
                .collect();
            painter.hline(inner.x_range(), inner.center().y, Stroke::new(1.0, grid));
            painter.add(Shape::line(points, Stroke::new(2.0, ACCENT)));
            painter.text(
                rect.right_top() + vec2(-12.0, 12.0),
                Align2::RIGHT_TOP,
                format!("{freq:.0} Hz"),
                theme::strong(14.0),
                Color32::from_gray(220),
            );
        }
        _ => {
            // Two cycles of the level (or the side, or the pitch) as the engine draws them.
            let value = |t: f32, side: usize| {
                let cycle = t.floor() as usize;
                let at = (t + if side == 1 { offset } else { 0.0 }).fract();
                let lfo = lfo_value(shape, at, held(cycle, side));
                match mode {
                    PAN | VIBRATO => lfo * depth,
                    _ => 1.0 - depth * (1.0 - (lfo + 1.0) / 2.0),
                }
            };
            let centered = matches!(mode, PAN | VIBRATO);
            let y_of = |v: f32| {
                if centered {
                    inner.center().y - v * inner.height() / 2.0
                } else {
                    inner.bottom() - v * inner.height()
                }
            };
            for k in 0..=4 {
                let y = inner.top() + inner.height() * k as f32 / 4.0;
                painter.hline(inner.x_range(), y, Stroke::new(1.0, grid));
            }
            painter.vline(inner.center().x, inner.y_range(), Stroke::new(1.0, grid));
            let sides = if mode == PAN || offset < 0.001 { 1 } else { 2 };
            for side in (0..sides).rev() {
                let points: Vec<Pos2> = (0..=400)
                    .map(|i| {
                        let t = i as f32 / 200.0;
                        pos2(inner.left() + t / 2.0 * inner.width(), y_of(value(t, side)))
                    })
                    .collect();
                let color = if side == 0 { ACCENT } else { RIGHT };
                if side == 0 && !centered {
                    let mut mesh = egui::Mesh::default();
                    for (i, pt) in points.iter().enumerate() {
                        mesh.colored_vertex(*pt, ACCENT.gamma_multiply(0.35));
                        mesh.colored_vertex(
                            pos2(pt.x, inner.bottom()),
                            ACCENT.gamma_multiply(0.04),
                        );
                        if i > 0 {
                            let j = 2 * i as u32;
                            mesh.add_triangle(j - 2, j - 1, j);
                            mesh.add_triangle(j - 1, j + 1, j);
                        }
                    }
                    painter.add(mesh);
                }
                painter.add(Shape::line(points, Stroke::new(2.0, color)));
            }
            let labels: &[(&str, f32)] = match mode {
                PAN => &[("L", 1.0), ("R", -1.0)],
                VIBRATO => &[("+", 1.0), ("-", -1.0)],
                _ => &[],
            };
            for (text, v) in labels {
                painter.text(
                    pos2(rect.left() + 6.0, y_of(*v)),
                    Align2::LEFT_CENTER,
                    *text,
                    theme::strong(11.0),
                    Color32::from_gray(140),
                );
            }
            // The playhead, in both cycles.
            for cycle in 0..2 {
                let x = inner.left() + (cycle as f32 + phase) / 2.0 * inner.width();
                painter.vline(
                    x,
                    inner.y_range(),
                    Stroke::new(1.5, Color32::from_white_alpha(150)),
                );
                painter.circle_filled(
                    pos2(x, y_of(value(cycle as f32 + phase, 0))),
                    4.5,
                    Color32::WHITE,
                );
            }
        }
    }
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, Color32::from_white_alpha(20)),
        egui::StrokeKind::Inside,
    );
}

/// The cutter's steps: each a block to click, lit for its gate's length, the one playing
/// outlined.
fn cutter(ui: &mut Ui, painter: &egui::Painter, area: Rect, params: &mut Params, meters: &[f32]) {
    let count = [4usize, 8, 16][params.get("steps").round().clamp(0.0, 2.0) as usize];
    let playing = meters.get(1).copied().unwrap_or(0.0).round() as usize % count;
    let gate = params.get("gate") / 100.0;
    let depth = params.get("depth") / 100.0;
    let gap = 6.0;
    let cell = (area.width() - gap * (count - 1) as f32) / count as f32;
    for k in 0..count.min(PATTERN) {
        let block = Rect::from_min_size(
            pos2(area.left() + k as f32 * (cell + gap), area.top()),
            vec2(cell, area.height()),
        );
        let key = format!("s{}", k + 1);
        let on = params.get(&key) >= 0.5;
        let response = ui
            .interact(block, ui.scope_id().with(("mod-step", k)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let beat = k % 4 == 0;
        painter.rect_filled(
            block,
            5.0,
            if beat {
                Color32::from_rgb(30, 34, 44)
            } else {
                Color32::from_rgb(22, 26, 34)
            },
        );
        let floor = block.bottom() - block.height() * (1.0 - depth);
        if on {
            let lit = Rect::from_min_max(
                block.min,
                pos2(block.left() + block.width() * gate, block.bottom()),
            );
            painter.rect_filled(
                lit.shrink(2.0),
                4.0,
                ACCENT.gamma_multiply(if k == playing { 1.0 } else { 0.7 }),
            );
        }
        if depth < 0.999 {
            painter.hline(
                block.x_range().shrink(2.0),
                floor,
                Stroke::new(1.0, Color32::from_white_alpha(40)),
            );
        }
        if k == playing {
            painter.rect_stroke(
                block,
                5.0,
                Stroke::new(2.0, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
        painter.text(
            pos2(block.center().x, block.bottom() - 10.0),
            Align2::CENTER_CENTER,
            format!("{}", k + 1),
            FontId::proportional(9.5),
            Color32::from_gray(if on { 30 } else { 120 }),
        );
        if response.clicked() {
            params.set(&key, Some(if on { 0.0 } else { 1.0 }), &response);
            params.doc.history_mut().end_gesture();
        }
    }
}
