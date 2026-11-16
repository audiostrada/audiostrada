//! Strada-Drive's editor: how hard each frequency is driven and the tone after the clipping,
//! over the spectra of the input and of the output, where the harmonics show; points dragged
//! for the drive, the focus, the low cut and the tone; the main controls large at the left.

use audiostrada_dsp::drive::{
    INPUT_SPECTRUM, OUTPUT_SPECTRUM, Response, TYPE_HINTS, TYPES, clip_db,
};
use egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::controls::{EditorParams, wide_knob};
use crate::i18n::tr;
use crate::verb_editor::{
    FreqAxis, HIGH_CUT, LOW_CUT, big_cells, bubble, cell, choice_row, format_hz, freq_grid, glow,
    handle, panel, rgb, rgba, screen_panel, spectrum, toggle_chip,
};
use crate::{theme, trf};

const HEAT: [u8; 3] = [255, 146, 64];
const FOCUS: [u8; 3] = [250, 204, 72];
const BASS: [u8; 3] = [92, 176, 255];
const MIDDLE: [u8; 3] = [102, 214, 150];
const INPUT: [u8; 3] = [214, 220, 230];
const OUTPUT: [u8; 3] = [72, 196, 222];
/// The drive drawn from -30 to +60 dB, the tone from -18 to +18 dB.
const DRIVE_DB: (f32, f32) = (-30.0, 60.0);
const TONE_DB: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Point {
    Drive,
    LowCut,
    Focus,
    Bass,
    Middle,
    Tone,
}

#[derive(Debug, Default)]
pub struct DriveState {
    /// The point held, where the drag began and the values it moves then.
    held: Option<(Point, Pos2, [f32; 2])>,
}

pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut DriveState) {
    choice_row(ui, params, "mode", &TYPES, &TYPE_HINTS);
    ui.add_space(8.0);
    display(ui, params, meters, state);
    ui.add_space(8.0);
    controls(ui, params);
}

fn mode(params: &dyn EditorParams) -> usize {
    (params.get("mode").round().max(0.0) as usize).min(TYPES.len() - 1)
}

/// Where the drive's point stands: the frequency farthest from the focus and the low cut.
fn drive_hz(focus: f32, low_cut: f32) -> f32 {
    let far = |hz: f32| (hz / focus).ln().abs().min((hz / low_cut).ln().abs());
    [300.0f32, 1500.0, 6000.0]
        .into_iter()
        .max_by(|a, b| far(*a).total_cmp(&far(*b)))
        .unwrap_or(1500.0)
}

fn display(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut DriveState) {
    let p = theme::current(ui);
    let width = ui.available_width().max(520.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 280.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    screen_panel(&painter, rect, &p);
    let plot = Rect::from_min_max(rect.min + vec2(48.0, 50.0), rect.max - vec2(44.0, 26.0));
    let axis = FreqAxis::new(plot);
    let y_drive = |db: f32| {
        plot.bottom()
            - ((db - DRIVE_DB.0) / (DRIVE_DB.1 - DRIVE_DB.0)).clamp(0.0, 1.0) * plot.height()
    };
    let y_tone = |db: f32| plot.center().y - (db / TONE_DB).clamp(-1.0, 1.0) * plot.height() / 2.0;
    let y_of_db = |db: f32| plot.bottom() - ((db + 96.0) / 112.0).clamp(0.0, 1.0) * plot.height();
    let drive_px = plot.height() / (DRIVE_DB.1 - DRIVE_DB.0);
    let tone_px = plot.height() / (2.0 * TONE_DB);
    freq_grid(
        &painter,
        plot,
        axis,
        &p,
        &[50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0],
    );
    let weak = p.weak;
    for db in [-24.0f32, 0.0, 24.0, 48.0] {
        let y = y_drive(db);
        painter.hline(
            plot.x_range(),
            y,
            Stroke::new(1.0, weak.gamma_multiply(0.07)),
        );
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            if db == 0.0 {
                "0".to_owned()
            } else {
                format!("{db:+.0}")
            },
            FontId::monospace(9.5),
            rgba(HEAT, 0.8),
        );
    }
    for db in [-12.0f32, 0.0, 12.0] {
        painter.text(
            pos2(plot.right() + 8.0, y_tone(db)),
            Align2::LEFT_CENTER,
            if db == 0.0 {
                "0".to_owned()
            } else {
                format!("{db:+.0}")
            },
            FontId::monospace(9.5),
            weak,
        );
    }
    painter.extend(Shape::dashed_line(
        &[
            pos2(plot.left(), y_tone(0.0)),
            pos2(plot.right(), y_tone(0.0)),
        ],
        Stroke::new(1.0, weak.gamma_multiply(0.3)),
        3.0,
        5.0,
    ));
    // The spectra: the output's harmonics glow over the input's line.
    if let Some(points) = spectrum(meters.get(OUTPUT_SPECTRUM..), axis, y_of_db) {
        glow(&painter, &points, plot.bottom(), OUTPUT, 0.3);
        painter.add(Shape::line(points, Stroke::new(1.2, rgba(OUTPUT, 0.6))));
    }
    if let Some(points) = spectrum(meters.get(INPUT_SPECTRUM..), axis, y_of_db) {
        painter.add(Shape::line(points, Stroke::new(1.2, rgba(INPUT, 0.4))));
    }

    let response_of = Response::new(&params.values());
    let along = |f: &dyn Fn(f32) -> f32| -> Vec<Pos2> {
        (0..=200)
            .map(|k| {
                let x = plot.left() + k as f32 / 200.0 * plot.width();
                pos2(x, f(axis.hz(x)))
            })
            .collect()
    };
    let heat = along(&|hz| y_drive(response_of.drive_at(hz)));
    glow(&painter, &heat, plot.bottom(), HEAT, 0.22);
    painter.add(Shape::line(heat, Stroke::new(2.2, rgb(HEAT))));
    let ink = if p.dark {
        Color32::from_rgb(236, 239, 244)
    } else {
        p.text
    };
    let tone = along(&|hz| y_tone(response_of.tone_at(hz)));
    painter.add(Shape::line(tone, Stroke::new(2.0, ink)));

    // The points.
    let (low_cut, focus_hz, mid_hz, tone_hz) = (
        params.get("low_cut"),
        params.get("focus_freq"),
        params.get("mid_freq"),
        params.get("tone"),
    );
    let at_drive = drive_hz(focus_hz, low_cut);
    let on_heat = |hz: f32| pos2(axis.x(hz), y_drive(response_of.drive_at(hz)));
    let on_tone = |hz: f32| pos2(axis.x(hz), y_tone(response_of.tone_at(hz)));
    let points = [
        (Point::Drive, on_heat(at_drive), HEAT),
        (Point::LowCut, on_heat(low_cut), LOW_CUT),
        (Point::Focus, on_heat(focus_hz), FOCUS),
        (Point::Bass, on_tone(50.0), BASS),
        (Point::Middle, on_tone(mid_hz), MIDDLE),
        (Point::Tone, on_tone(tone_hz.min(19_000.0)), HIGH_CUT),
    ];
    let nearest = |at: Pos2| {
        points
            .iter()
            .filter(|(_, h, _)| h.distance(at) < 14.0)
            .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
            .map(|(k, ..)| *k)
    };
    if response.drag_started()
        && let Some(at) = response.interact_pointer_pos()
    {
        state.held = nearest(at).map(|k| {
            let values = match k {
                Point::Drive => [params.get("drive"), 0.0],
                Point::Focus => [params.get("focus"), 0.0],
                Point::Bass => [params.get("bass"), 0.0],
                Point::Middle => [params.get("mid"), 0.0],
                _ => [0.0; 2],
            };
            (k, at, values)
        });
    }
    let held = state.held.map(|h| h.0);
    let hovered = response.hover_pos().and_then(nearest);
    for (k, at, c) in &points {
        let lit = held == Some(*k) || hovered == Some(*k);
        handle(&painter, *at, *c, lit, theme::screen(&p));
    }
    if let Some(k) = held.or(hovered)
        && let Some((_, at, _)) = points.iter().find(|(q, ..)| *q == k)
    {
        let text = match k {
            Point::Drive => trf!(
                "Drive {percent} % · {db} dB into the clipping",
                percent = format!("{:.0}", params.get("drive")),
                db = format!("{:+.1}", clip_db(params.get("mode"), params.get("drive")))
            ),
            Point::LowCut => trf!("Low cut {hz}", hz = format_hz(low_cut)),
            Point::Focus => trf!(
                "Focus {db} dB at {hz}",
                db = format!("{:+.1}", params.get("focus")),
                hz = format_hz(focus_hz)
            ),
            Point::Bass => trf!("Bass {db} dB", db = format!("{:+.1}", params.get("bass"))),
            Point::Middle => trf!(
                "Middle {db} dB at {hz}",
                db = format!("{:+.1}", params.get("mid")),
                hz = format_hz(mid_hz)
            ),
            Point::Tone => trf!("Tone {hz}", hz = format_hz(tone_hz)),
        };
        bubble(&painter, plot, *at, text);
        ui.ctx().set_cursor_icon(if held.is_some() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    readout(&painter, rect, params, &p);
    legend(&painter, rect, ink, &p);

    let response = response.on_hover_text(tr(
        "Drag the points: the orange line is how hard each frequency is driven, the white one the tone after it; double-click a point to reset it",
    ));
    if response.double_clicked()
        && let Some(k) = hovered
    {
        let ids: &[&str] = match k {
            Point::Drive => &["drive"],
            Point::LowCut => &["low_cut"],
            Point::Focus => &["focus", "focus_freq"],
            Point::Bass => &["bass"],
            Point::Middle => &["mid", "mid_freq"],
            Point::Tone => &["tone"],
        };
        for id in ids {
            let default = params.param(id).map(|q| q.default);
            params.set(id, default, &response);
        }
        params.end_gesture();
    }
    if let Some((k, from, start)) = state.held {
        let at = response
            .interact_pointer_pos()
            .filter(|_| response.dragged());
        let up = |px: f32| at.map(|at| (from.y - at.y) / px);
        let across = |lo: f32, hi: f32| at.map(|at| axis.hz(at.x).clamp(lo, hi));
        match k {
            Point::Drive => {
                let v = up(drive_px).map(|db| start[0] + db / 0.72);
                params.change("drive", v, &response);
            }
            Point::LowCut => params.change("low_cut", across(20.0, 600.0), &response),
            Point::Focus => {
                params.change("focus_freq", across(40.0, 10_000.0), &response);
                params.change("focus", up(drive_px).map(|db| start[0] + db), &response);
            }
            Point::Bass => {
                params.change("bass", up(tone_px).map(|db| start[0] + db), &response);
            }
            Point::Middle => {
                params.change("mid_freq", across(200.0, 5000.0), &response);
                params.change("mid", up(tone_px).map(|db| start[0] + db), &response);
            }
            Point::Tone => params.change("tone", across(500.0, 20_000.0), &response),
        }
        if !response.dragged() {
            state.held = None;
        }
    }
    let loud = |from: usize| {
        meters
            .get(from..)
            .is_some_and(|m| m.iter().take(128).any(|db| *db > -100.0))
    };
    if loud(INPUT_SPECTRUM) || loud(OUTPUT_SPECTRUM) {
        crate::controls::animate(ui.ctx());
    }
}

/// The type, the drive and where it is focused.
fn readout(painter: &egui::Painter, rect: Rect, params: &dyn EditorParams, p: &theme::Palette) {
    let corner = rect.left_top() + vec2(16.0, 12.0);
    painter.text(
        corner,
        Align2::LEFT_TOP,
        tr(TYPES[mode(params)]).to_uppercase(),
        theme::strong(11.5),
        p.weak,
    );
    let drive = params.get("drive");
    let mut line = trf!(
        "Drive {percent} % · {db} dB",
        percent = format!("{drive:.0}"),
        db = format!("{:+.1}", clip_db(params.get("mode"), drive))
    );
    let focus = params.get("focus");
    if focus.abs() >= 0.1 {
        line += &format!(
            "  ·  {}",
            trf!(
                "focus {db} dB at {hz}",
                db = format!("{focus:+.1}"),
                hz = format_hz(params.get("focus_freq"))
            )
        );
    }
    if params.get("auto_gain") >= 0.5 {
        line += &format!("  ·  {}", tr("auto gain"));
    }
    painter.text(
        corner + vec2(0.0, 18.0),
        Align2::LEFT_TOP,
        line,
        FontId::monospace(12.0),
        p.text,
    );
}

/// What the lines and the glow are.
fn legend(painter: &egui::Painter, rect: Rect, ink: Color32, p: &theme::Palette) {
    let mut x = rect.right() - 16.0;
    let y = rect.top() + 18.0;
    let font = FontId::proportional(10.5);
    for (label, swatch) in [
        (tr("Tone"), ink),
        (tr("Drive"), rgb(HEAT)),
        (tr("Output"), rgba(OUTPUT, 0.8)),
        (tr("Input"), rgba(INPUT, 0.7)),
    ] {
        let galley = painter.layout_no_wrap(label.to_owned(), font.clone(), p.weak);
        x -= galley.size().x;
        painter.galley(pos2(x, y - galley.size().y / 2.0), galley, p.weak);
        x -= 20.0;
        painter.line_segment([pos2(x, y), pos2(x + 14.0, y)], Stroke::new(2.5, swatch));
        x -= 14.0;
    }
}

fn controls(ui: &mut Ui, params: &mut dyn EditorParams) {
    let p = theme::current(ui);
    let accent = p.accent;
    let panel = panel(
        ui,
        (3, 104.0),
        &[&[("Drive", 2), ("Focus", 2)], &[("Tone", 4)]],
        (78.0, 150.0),
    );
    let auto = params.get("auto_gain") >= 0.5;
    big_cells(ui, &panel.big, |ui, k| match k {
        0 => {
            wide_knob(ui, params, "drive", tr("Drive"), 72.0, rgb(HEAT));
            ui.add_space(28.0);
        }
        1 => {
            ui.add_enabled_ui(!auto, |ui| {
                wide_knob(ui, params, "level", tr("Output"), 72.0, accent);
            });
            ui.add_space(4.0);
            toggle_chip(
                ui,
                params,
                "auto_gain",
                tr("Auto gain"),
                tr(
                    "Keeps the level where it came in, whatever the drive, so that you judge the color and not the loudness",
                ),
            );
        }
        _ => {
            wide_knob(ui, params, "mix", tr("Mix"), 72.0, theme::SOLO);
            ui.add_space(28.0);
        }
    });
    let rows: [[(&str, &str, Color32); 4]; 2] = [
        [
            ("bias", "Bias", accent),
            ("low_cut", "Low cut", rgb(LOW_CUT)),
            ("focus", "Amount", rgb(FOCUS)),
            ("focus_freq", "Frequency", rgb(FOCUS)),
        ],
        [
            ("tone", "Tone", rgb(HIGH_CUT)),
            ("bass", "Bass", rgb(BASS)),
            ("mid", "Middle", rgb(MIDDLE)),
            ("mid_freq", "Frequency", rgb(MIDDLE)),
        ],
    ];
    for (row, knobs) in panel.rows.iter().zip(rows) {
        for (rect, (id, label, tint)) in row.iter().zip(knobs) {
            cell(ui, *rect, |ui| {
                wide_knob(ui, params, id, tr(label), 38.0, tint)
            });
        }
    }
}
