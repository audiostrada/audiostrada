//! Strada-Delay's editor: the echoes on the tempo grid, left above and right below, the first
//! ones dragged for the time, the repeats and the offset; beside them the tone of the repeats
//! over their spectrum, its cuts dragged; the main controls large at the left, the others in a
//! row.

use std::collections::VecDeque;

use audiostrada_dsp::delay::{ECHO_SPECTRUM, MODEL_HINTS, MODELS, notes_by_length, synced_seconds};
use egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::controls::{EditorParams, wide_knob};
use crate::i18n::tr;
use crate::verb_editor::{
    FreqAxis, HIGH_CUT, LOW_CUT, band_gain, big_cells, bubble, cell, chip, chip_row, choice_row,
    format_hz, freq_grid, glow, handle, note_knob, panel, rgb, rgba, screen_panel, shade, spectrum,
    toggle_chip,
};
use crate::{theme, trf};

const DIGITAL: usize = 0;
const TAPE: usize = 1;
/// Echoes are drawn on a 48 dB scale, as far as they stay above it.
const FLOOR: f32 = 0.004;
/// The left and the right echoes.
const LEFT: [u8; 3] = [112, 206, 128];
const RIGHT: [u8; 3] = [240, 160, 72];
const SPECTRUM: [u8; 3] = [176, 186, 202];

#[derive(Debug, Clone, Copy)]
enum Drag {
    /// The first echo: the time across, the feedback up and down.
    First {
        span: f32,
        y: f32,
        feedback: f32,
    },
    /// The first echo on the right: its offset.
    Offset {
        span: f32,
        x: f32,
        offset: f32,
    },
    LowCut,
    HighCut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grip {
    First,
    Offset,
    LowCut,
    HighCut,
}

#[derive(Debug, Default)]
pub struct DelayState {
    /// The display's clock, which runs while something sounds.
    time: f64,
    slow: f32,
    last_hit: f64,
    /// Recent hits on the display's clock, and how loud.
    hits: VecDeque<(f64, f32)>,
    drag: Option<Drag>,
    /// Times of the last clicks on Tap.
    taps: Vec<f64>,
}

fn bpm(params: &dyn EditorParams) -> f64 {
    params.tempo()
}

/// The delay time in seconds, the note's length when synced.
fn seconds(params: &dyn EditorParams) -> f32 {
    if params.get("sync") >= 0.5 {
        synced_seconds(params.get("note"), bpm(params))
    } else {
        params.get("time") * 0.001
    }
}

/// Sets the time to `s` seconds: the nearest note value when synced.
fn set_time(params: &mut dyn EditorParams, s: Option<f32>, response: &Response) {
    if params.get("sync") >= 0.5 {
        let beat = 60.0 / bpm(params).max(1.0) as f32;
        let note = s.and_then(|s| {
            notes_by_length()
                .into_iter()
                .min_by(|a, b| {
                    let far = |q: f32| (q * beat / s.max(1e-3)).ln().abs();
                    far(a.2).total_cmp(&far(b.2))
                })
                .map(|n| n.0 as f32)
        });
        let current = params.get("note").round();
        params.change("note", note.filter(|n| *n != current), response);
    } else {
        let current = params.get("time");
        let time = s
            .map(|s| (s * 1000.0).clamp(1.0, 4000.0))
            .filter(|t| (t - current).abs() > 0.05);
        params.change("time", time, response);
    }
}

/// How high a level stands on the 48 dB scale, from 0 to 1.
fn height(a: f32) -> f32 {
    (1.0 + 20.0 * a.max(1e-6).log10() / 48.0).clamp(0.0, 1.0)
}

/// Echoes heard above the 48 dB floor after the first.
fn repeats(feedback: f32) -> usize {
    if feedback <= 0.0 {
        0
    } else {
        ((FLOOR.ln() / feedback.ln()).floor() as usize).min(999)
    }
}

pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut DelayState) {
    let model = (params.get("model").round().max(0.0) as usize).min(MODELS.len() - 1);
    choice_row(ui, params, "model", &MODELS, &MODEL_HINTS);
    ui.add_space(8.0);
    display(ui, params, meters, state, model);
    ui.add_space(8.0);
    controls(ui, params, state, model);
}

// ---------------------------------------------------------------- The display

/// An echo: when, on which side (0 left, 1 right), how loud, and which repeat.
struct Tap {
    time: f32,
    side: usize,
    level: f32,
    n: i32,
}

fn taps(params: &dyn EditorParams, seconds: f32) -> Vec<Tap> {
    let frozen = params.get("freeze") >= 0.5;
    let feedback = if frozen {
        1.0
    } else {
        params.get("feedback") / 100.0
    };
    let pingpong = params.get("pingpong") >= 0.5;
    let offset = params.get("offset") * 0.001;
    let mut level = params.get("mix") / 100.0;
    let mut out = Vec::new();
    for n in 1..=24 {
        if n > 1 && level < FLOOR {
            break;
        }
        if pingpong {
            out.push(Tap {
                time: n as f32 * seconds + (n / 2) as f32 * offset,
                side: if n % 2 == 1 { 0 } else { 1 },
                level,
                n,
            });
        } else {
            for side in 0..2 {
                out.push(Tap {
                    time: n as f32 * (seconds + side as f32 * offset),
                    side,
                    level,
                    n,
                });
            }
        }
        level *= feedback;
    }
    out
}

/// The tone of one pass through the loop at `hz`: the cuts, and the darkening of tape and
/// analog.
fn pass_db(params: &dyn EditorParams, model: usize, seconds: f32, hz: f32) -> f32 {
    let (low, high) = (params.get("low_cut"), params.get("high_cut"));
    let dark = match model {
        DIGITAL => 1.0,
        TAPE => 1.0 / (1.0 + (hz / 6000.0).powi(2)).sqrt(),
        _ => {
            let fc = (2500.0 * (0.4 / seconds.max(0.02)).sqrt()).clamp(1200.0, 7000.0);
            1.0 / (1.0 + (hz / fc).powi(2)).sqrt()
        }
    };
    20.0 * (band_gain(hz, low, high) * dark).max(1e-6).log10()
}

fn display(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut DelayState,
    model: usize,
) {
    let p = theme::current(ui);
    let width = ui.available_width().max(520.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 280.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    screen_panel(&painter, rect, &p);
    let now = ui.input(|i| i.time);
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let level = |k: usize| 10f32.powf(meters.get(k).copied().unwrap_or(-200.0) / 20.0);
    let (input, echoes) = (level(0), level(1));
    let frozen = params.get("freeze") >= 0.5;
    // A hit is the input rising well over its recent level.
    state.slow += (input - state.slow) * (1.0 - (-dt / 0.25).exp());
    if !frozen && input > 0.01 && input > state.slow * 1.3 + 0.003 && now - state.last_hit > 0.09 {
        state.last_hit = now;
        let vel = ((20.0 * input.log10() + 42.0) / 36.0).clamp(0.15, 1.0);
        state.hits.push_back((state.time, vel));
    }
    let active = input > 1e-4 || echoes > 1e-4;
    if active {
        state.time += f64::from(dt);
    }
    while state
        .hits
        .front()
        .is_some_and(|(t, _)| *t < state.time - 12.0)
    {
        state.hits.pop_front();
    }

    let split = rect.left() + (rect.width() * 0.58).round();
    let (top, bottom) = (rect.top() + 50.0, rect.bottom() - 26.0);
    let echo_rect = Rect::from_min_max(pos2(rect.left() + 18.0, top), pos2(split - 18.0, bottom));
    let tone_rect = Rect::from_min_max(pos2(split + 18.0, top), pos2(rect.right() - 16.0, bottom));
    painter.vline(
        split,
        (rect.top() + 12.0)..=(rect.bottom() - 12.0),
        Stroke::new(1.0, p.weak.gamma_multiply(0.14)),
    );
    let seconds = seconds(params);
    let grips = echo_zone(&painter, echo_rect, params, state, (seconds, &p));
    let (cuts, axis) = tone_zone(&painter, tone_rect, params, meters, (model, seconds, &p));
    readout(&painter, rect, split, params, seconds, &p);

    let points = [
        (Grip::First, grips.0),
        (Grip::Offset, grips.1),
        (Grip::LowCut, Some(cuts.0)),
        (Grip::HighCut, Some(cuts.1)),
    ];
    let nearest = |at: Pos2| {
        points
            .iter()
            .filter_map(|(g, h)| h.map(|h| (*g, h)))
            .filter(|(_, h)| h.distance(at) < 14.0)
            .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
            .map(|(g, _)| g)
    };
    let span = grips.2;
    if response.drag_started()
        && let Some(at) = response.interact_pointer_pos()
    {
        state.drag = nearest(at).map(|g| match g {
            Grip::First => Drag::First {
                span,
                y: at.y,
                feedback: params.get("feedback"),
            },
            Grip::Offset => Drag::Offset {
                span,
                x: at.x,
                offset: params.get("offset"),
            },
            Grip::LowCut => Drag::LowCut,
            Grip::HighCut => Drag::HighCut,
        });
    }
    let held = state.drag.map(|d| match d {
        Drag::First { .. } => Grip::First,
        Drag::Offset { .. } => Grip::Offset,
        Drag::LowCut => Grip::LowCut,
        Drag::HighCut => Grip::HighCut,
    });
    let hovered = response.hover_pos().and_then(nearest);
    let back = theme::screen(&p);
    for (g, at) in points {
        let Some(at) = at else { continue };
        let c = match g {
            Grip::First => LEFT,
            Grip::Offset => RIGHT,
            Grip::LowCut => LOW_CUT,
            Grip::HighCut => HIGH_CUT,
        };
        handle(&painter, at, c, held == Some(g) || hovered == Some(g), back);
    }
    if let Some(g) = held.or(hovered)
        && let Some((_, Some(at))) = points.iter().find(|(q, _)| *q == g)
    {
        let text = match g {
            Grip::First => trf!(
                "{ms} ms · feedback {feedback} %",
                ms = format!("{:.0}", seconds * 1000.0),
                feedback = format!("{:.0}", params.get("feedback"))
            ),
            Grip::Offset => trf!(
                "Right {offset} ms",
                offset = format!("{:+.1}", params.get("offset"))
            ),
            Grip::LowCut => trf!("Low cut {hz}", hz = format_hz(params.get("low_cut"))),
            Grip::HighCut => trf!("High cut {hz}", hz = format_hz(params.get("high_cut"))),
        };
        let bounds = if matches!(g, Grip::LowCut | Grip::HighCut) {
            tone_rect
        } else {
            echo_rect
        };
        bubble(&painter, bounds, *at, text);
        ui.ctx().set_cursor_icon(if held.is_some() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }

    let response = response.on_hover_text(tr(
        "Drag the first echoes: the time across, the feedback up and down, the right one for its offset; drag the cuts beside",
    ));
    if response.double_clicked()
        && let Some(id) = hovered.and_then(|g| match g {
            Grip::LowCut => Some("low_cut"),
            Grip::HighCut => Some("high_cut"),
            Grip::Offset => Some("offset"),
            Grip::First => None,
        })
    {
        let default = params.param(id).map(|q| q.default);
        params.set(id, default, &response);
        params.end_gesture();
    }
    let at = response
        .interact_pointer_pos()
        .filter(|_| response.dragged());
    let (x0, x1) = (echo_rect.left(), echo_rect.right());
    match state.drag {
        Some(Drag::First { span, y, feedback }) => {
            let s = at.map(|at| ((at.x - x0) / (x1 - x0) * span).max(0.001));
            set_time(params, s, &response);
            let fb = at.map(|at| (feedback + (y - at.y) * 0.4).clamp(0.0, 95.0));
            params.change("feedback", fb, &response);
        }
        Some(Drag::Offset { span, x, offset }) => {
            let o =
                at.map(|at| (offset + (at.x - x) / (x1 - x0) * span * 1000.0).clamp(-50.0, 50.0));
            params.change("offset", o, &response);
        }
        Some(Drag::LowCut) => {
            let hz = at.map(|at| axis.hz(at.x).clamp(20.0, 2000.0));
            params.change("low_cut", hz, &response);
        }
        Some(Drag::HighCut) => {
            let hz = at.map(|at| axis.hz(at.x).clamp(1000.0, 20_000.0));
            params.change("high_cut", hz, &response);
        }
        None => {}
    }
    if state.drag.is_some() && !response.dragged() {
        state.drag = None;
    }
    if active
        || state
            .hits
            .back()
            .is_some_and(|(h, _)| state.time - h < f64::from(span))
    {
        crate::controls::animate(ui.ctx());
    }
}

/// The echoes over the tempo grid; where the first left and right echoes are, and the span.
fn echo_zone(
    painter: &egui::Painter,
    zone: Rect,
    params: &dyn EditorParams,
    state: &DelayState,
    (seconds, p): (f32, &theme::Palette),
) -> (Option<Pos2>, Option<Pos2>, f32) {
    let beat = 60.0 / bpm(params).max(1.0) as f32;
    let taps = taps(params, seconds);
    let last = taps.iter().map(|t| t.time).fold(0.0, f32::max);
    let span = match state.drag {
        Some(Drag::First { span, .. } | Drag::Offset { span, .. }) => span,
        _ => {
            let span = (last + seconds).max(seconds * 3.0).clamp(0.1, 10.0);
            if span / beat < 64.0 {
                (span / beat).ceil() * beat
            } else {
                span
            }
        }
    };
    let (x0, x1) = (zone.left() + 6.0, zone.right());
    let to_x = |t: f32| x0 + t / span * (x1 - x0);
    let mid = zone.center().y;
    let half = zone.height() / 2.0 - 4.0;
    // The tempo: sixteenths, beats, bars, the bars named.
    let sixteenth = beat / 4.0;
    if span / sixteenth < 400.0 {
        let mut k = 0;
        while k as f32 * sixteenth <= span + 1e-4 {
            let x = to_x(k as f32 * sixteenth);
            let a = if k % 16 == 0 {
                0.3
            } else if k % 4 == 0 {
                0.14
            } else if (x1 - x0) / (span / sixteenth) > 8.0 {
                0.05
            } else {
                0.0
            };
            if a > 0.0 {
                painter.vline(
                    x,
                    zone.y_range(),
                    Stroke::new(1.0, p.weak.gamma_multiply(a)),
                );
            }
            if k % 16 == 0 && k > 0 {
                painter.text(
                    pos2(x, zone.bottom() + 5.0),
                    Align2::CENTER_TOP,
                    trf!("Bar {n}", n = k / 16 + 1),
                    FontId::monospace(9.5),
                    p.weak,
                );
            }
            k += 1;
        }
    }
    painter.hline(x0..=x1, mid, Stroke::new(1.0, p.weak.gamma_multiply(0.25)));
    for (label, y, align) in [
        ("L", zone.top(), Align2::LEFT_TOP),
        ("R", zone.bottom(), Align2::LEFT_BOTTOM),
    ] {
        painter.text(
            pos2(zone.left() - 12.0, y),
            align,
            label,
            theme::strong(10.0),
            rgb(if label == "L" { LEFT } else { RIGHT }),
        );
    }
    let tip = |t: &Tap| {
        let side = if t.side == 0 { -1.0 } else { 1.0 };
        pos2(to_x(t.time), mid + side * half * height(t.level))
    };
    // The dry sound at zero.
    let dry = height(1.0 - params.get("mix") / 100.0) * half;
    painter.rect_filled(
        Rect::from_center_size(pos2(x0, mid), vec2(3.0, 2.0 * dry)),
        1.5,
        p.weak.gamma_multiply(0.45),
    );
    if params.get("pingpong") >= 0.5 {
        let path: Vec<Pos2> = std::iter::once(pos2(x0, mid))
            .chain(taps.iter().filter(|t| t.time <= span).map(tip))
            .collect();
        painter.extend(Shape::dashed_line(
            &path,
            Stroke::new(1.0, p.weak.gamma_multiply(0.45)),
            4.0,
            4.0,
        ));
    }
    // Each pass through the filters loses some of the band: echoes darken as they repeat.
    let (low, high) = (params.get("low_cut"), params.get("high_cut"));
    let band = ((high / low).log2() / (20_000.0f32 / 20.0).log2()).clamp(0.15, 1.0);
    let now = state.time;
    for t in taps.iter().filter(|t| t.time <= span) {
        let at = tip(t);
        let c = if t.side == 0 { LEFT } else { RIGHT };
        let a = band.powi(t.n - 1).max(0.3);
        let bar = Rect::from_two_pos(pos2(at.x - 2.5, mid), pos2(at.x + 2.5, at.y));
        painter.rect_filled(bar.expand(2.0), 4.0, rgba(c, 0.12 * a));
        painter.rect_filled(bar, 2.5, rgba(c, a));
        // Lit as it is heard after each hit.
        let lit = state
            .hits
            .iter()
            .map(|(h, vel)| {
                let age = (now - h) as f32 - t.time;
                if (0.0..0.3).contains(&age) {
                    vel * (1.0 - age / 0.3)
                } else {
                    0.0
                }
            })
            .fold(0.0, f32::max);
        if lit > 0.0 {
            painter.rect_filled(bar.expand(4.0 * lit), 5.0, rgba(c, 0.25 * lit));
            painter.rect_filled(bar, 2.5, Color32::WHITE.gamma_multiply(0.6 * lit));
        }
    }
    // The last hit's sweep, as its echoes come.
    if let Some((h, _)) = state.hits.back() {
        let age = (now - h) as f32;
        if age < span {
            painter.vline(
                to_x(age),
                zone.y_range(),
                Stroke::new(1.0, p.text.gamma_multiply(0.25 * (1.0 - age / span))),
            );
        }
    }
    let first = taps.iter().find(|t| t.side == 0).map(tip);
    let right = taps.iter().find(|t| t.side == 1).map(tip);
    (first, right, span)
}

/// The tone of the repeats over their spectrum, the later passes fainter; where the cuts'
/// points are, and the axis.
fn tone_zone(
    painter: &egui::Painter,
    zone: Rect,
    params: &dyn EditorParams,
    meters: &[f32],
    (model, seconds, p): (usize, f32, &theme::Palette),
) -> ((Pos2, Pos2), FreqAxis) {
    let axis = FreqAxis::new(zone);
    let y_of = |db: f32| zone.top() + ((3.0 - db) / 42.0).clamp(0.0, 1.0) * zone.height();
    let y_of_db = |db: f32| zone.bottom() - ((db + 96.0) / 112.0).clamp(0.0, 1.0) * zone.height();
    freq_grid(painter, zone, axis, p, &[50.0, 200.0, 1000.0, 5000.0]);
    for db in [0.0f32, -12.0, -24.0, -36.0] {
        let y = y_of(db);
        painter.hline(
            zone.x_range(),
            y,
            Stroke::new(
                1.0,
                p.weak.gamma_multiply(if db == 0.0 { 0.18 } else { 0.07 }),
            ),
        );
        painter.text(
            pos2(zone.left() + 3.0, y - 2.0),
            Align2::LEFT_BOTTOM,
            format!("{db:.0}"),
            FontId::monospace(9.0),
            p.weak.gamma_multiply(0.8),
        );
    }
    if let Some(points) = spectrum(meters.get(ECHO_SPECTRUM..), axis, y_of_db) {
        glow(painter, &points, zone.bottom(), SPECTRUM, 0.2);
        painter.add(Shape::line(points, Stroke::new(1.0, rgba(SPECTRUM, 0.4))));
    }
    let (low, high) = (params.get("low_cut"), params.get("high_cut"));
    shade(painter, zone, (zone.left(), axis.x(low)), LOW_CUT);
    shade(painter, zone, (zone.right(), axis.x(high)), HIGH_CUT);
    let feedback_db = 20.0 * (params.get("feedback") / 100.0).max(1e-4).log10();
    let curve = |n: f32| -> Vec<Pos2> {
        (0..=160)
            .map(|k| {
                let x = zone.left() + k as f32 / 160.0 * zone.width();
                let db = n * pass_db(params, model, seconds, axis.hz(x)) + (n - 1.0) * feedback_db;
                pos2(x, y_of(db))
            })
            .collect()
    };
    if params.get("freeze") < 0.5 {
        for (n, a) in [(4.0, 0.15), (3.0, 0.25), (2.0, 0.4)] {
            painter.add(Shape::line(
                curve(n),
                Stroke::new(1.2, rgba([232, 200, 64], a)),
            ));
        }
    }
    let first = curve(1.0);
    glow(painter, &first, zone.bottom(), [232, 200, 64], 0.14);
    painter.add(Shape::line(first, Stroke::new(2.2, theme::SOLO)));
    let on = |hz: f32| pos2(axis.x(hz), y_of(pass_db(params, model, seconds, hz)));
    ((on(low), on(high)), axis)
}

/// The time, the repeats and the tone, over the two zones.
fn readout(
    painter: &egui::Painter,
    rect: Rect,
    split: f32,
    params: &dyn EditorParams,
    seconds: f32,
    p: &theme::Palette,
) {
    let ms = format!("{:.0} ms", seconds * 1000.0);
    let time = if params.get("sync") >= 0.5 {
        let names = audiostrada_dsp::delay::NOTE_NAMES;
        let note = names[(params.get("note").round().max(0.0) as usize).min(names.len() - 1)];
        format!("{}  ·  {ms}", tr(note))
    } else {
        ms
    };
    let corner = rect.left_top() + vec2(18.0, 10.0);
    let galley = painter.layout_no_wrap(time, theme::strong(17.0), p.text);
    let width = galley.size().x;
    painter.galley(corner, galley, p.text);
    let repeats = if params.get("freeze") >= 0.5 {
        tr("Frozen: the loop holds").to_owned()
    } else {
        trf!(
            "Repeats: ≈ {n}",
            n = repeats(params.get("feedback") / 100.0)
        )
    };
    painter.text(
        corner + vec2(width + 14.0, 4.0),
        Align2::LEFT_TOP,
        repeats,
        FontId::proportional(12.0),
        p.weak,
    );
    let right = pos2(split + 18.0, rect.top() + 12.0);
    let title = tr("Tone of the repeats").to_uppercase();
    painter.text(right, Align2::LEFT_TOP, title, theme::strong(11.0), p.weak);
    painter.text(
        right + vec2(0.0, 17.0),
        Align2::LEFT_TOP,
        trf!(
            "{low} to {high}",
            low = format_hz(params.get("low_cut")),
            high = format_hz(params.get("high_cut"))
        ),
        FontId::monospace(11.5),
        p.text,
    );
}

// ---------------------------------------------------------------- Controls

fn tap_button(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut DelayState) {
    let now = ui.input(|i| i.time);
    state.taps.retain(|t| now - t < 2.0);
    let response = chip(ui, tr("Tap"), false).on_hover_text(tr(
        "Click in time: the delay takes the interval between the clicks",
    ));
    if response.clicked() {
        state.taps.push(now);
        if state.taps.len() >= 2 {
            let n = state.taps.len();
            let interval = (state.taps[n - 1] - state.taps[0]) / (n - 1) as f64;
            params.set("time", Some((interval * 1000.0) as f32), &response);
            params.end_gesture();
        }
    }
}

fn controls(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut DelayState, model: usize) {
    let p = theme::current(ui);
    let accent = p.accent;
    let panel = panel(
        ui,
        (3, 128.0),
        &[&[("Tone", 2), ("Character", 2), ("Stereo", 1), ("Ducking", 1)]],
        (96.0, 150.0),
    );
    let synced = params.get("sync") >= 0.5;
    big_cells(ui, &panel.big, |ui, k| match k {
        0 => {
            if synced {
                note_knob(ui, params, ("note", tr("Time")), 72.0, accent, None);
            } else {
                wide_knob(ui, params, "time", tr("Time"), 72.0, accent);
            }
            ui.add_space(4.0);
            chip_row(ui, &[tr("Sync"), tr("Tap")], |ui| {
                toggle_chip(
                    ui,
                    params,
                    "sync",
                    tr("Sync"),
                    tr("A note value at the project's tempo"),
                );
                ui.add_enabled_ui(!synced, |ui| tap_button(ui, params, state));
            });
        }
        1 => {
            wide_knob(ui, params, "feedback", tr("Feedback"), 72.0, rgb(LEFT));
            ui.add_space(4.0);
            chip_row(ui, &[tr("Ping-pong"), tr("Freeze")], |ui| {
                toggle_chip(
                    ui,
                    params,
                    "pingpong",
                    tr("Ping-pong"),
                    tr("The echoes bounce from one side to the other"),
                );
                toggle_chip(
                    ui,
                    params,
                    "freeze",
                    tr("Freeze"),
                    tr("Holds what is in the delay, repeating for ever; the input is not added"),
                );
            });
        }
        _ => {
            wide_knob(ui, params, "mix", tr("Mix"), 72.0, theme::SOLO);
            ui.add_space(28.0);
        }
    });
    let knobs = [
        ("low_cut", "Low cut", rgb(LOW_CUT)),
        ("high_cut", "High cut", rgb(HIGH_CUT)),
        ("wow", "Wow & flutter", accent),
        ("drive", "Drive", accent),
        ("offset", "Right offset", rgb(RIGHT)),
        ("duck", "Amount", accent),
    ];
    for (rect, (id, label, tint)) in panel.rows[0].iter().zip(knobs) {
        cell(ui, *rect, |ui| {
            let character = matches!(id, "wow" | "drive");
            ui.add_enabled_ui(!character || model != DIGITAL, |ui| {
                wide_knob(ui, params, id, tr(label), 46.0, tint);
            });
        });
    }
}
