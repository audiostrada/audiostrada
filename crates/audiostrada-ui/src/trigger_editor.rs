//! Strada-Trigger's editor: the drum and its sound as cards, the track's hits drawn live under
//! a threshold to drag, then the detection and sound knobs, and two faders: close mics to
//! room, and the track to the trigger.

use std::collections::VecDeque;
use std::sync::Arc;

use audiostrada_dsp::trigger::{self, KICKS, SNARES};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::drums_editor::{MODULE, family, modules, screen, waveform};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;

/// What the editor remembers between frames: the detection lately and the hits found.
#[derive(Debug, Default)]
pub struct TriggerState {
    /// Time, detection and input level (linear), oldest first.
    levels: VecDeque<(f64, f32, f32)>,
    /// Time and strength (0 to 1) of each hit.
    hits: VecDeque<(f64, f32)>,
    count: Option<f32>,
}

/// Seconds of history drawn.
const SPAN: f64 = 4.0;
/// The display's range, in dB.
const FLOOR: f32 = -60.0;
const INK: Color32 = Color32::from_rgb(236, 240, 246);

/// A sound's short description, shown under its name.
fn subtitle(kind: usize, sound: usize) -> &'static str {
    let list: &[&str] = if kind == 0 {
        &[
            "20-inch, clean",
            "20-inch, rough",
            "24-inch, damped",
            "24-inch, open and long",
            "Vintage, stereo mix",
            "Two mics and room",
        ]
    } else {
        &[
            "13-inch, center",
            "14-inch, center",
            "14-inch, rimshot",
            "Short and dry",
            "Long ring",
            "Top, bottom and room",
            "Top, bottom and room",
        ]
    };
    list.get(sound).copied().unwrap_or("")
}

/// Where a sound comes from, for its tooltip and the credits.
fn source(kind: usize, sound: usize) -> &'static str {
    match (kind, sound) {
        (0, 0..=1) | (1, 0..=1) => "Karoryfer Samples, Unruly Drums (CC0)",
        (0, 2..=3) | (1, 2) => "Karoryfer Samples, Big Rusty Drums (CC0)",
        (0, 4) | (1, 3) => "SM Drums (public domain)",
        (1, 4) => "Karoryfer Samples, Swirly Drums (CC0)",
        (0, 5) | (1, 5) => "DrumGizmo, MuldjordKit (CC BY 4.0)",
        _ => "DrumGizmo, DRSKit (CC BY 4.0)",
    }
}

fn tint(kind: usize) -> Color32 {
    family(if kind == 0 { 0 } else { 2 })
}

fn db(linear: f32) -> f32 {
    20.0 * linear.max(1e-6).log10()
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut TriggerState) {
    let kind = (params.get("kind").round() as usize).min(1);
    let color = tint(kind);
    record(ui, meters, state);
    screen(ui, |ui| {
        kind_switch(ui, params, kind);
        ui.add_space(10.0);
        cards(ui, params, kind);
        ui.add_space(12.0);
        display(ui, params, state, color);
        ui.add_space(12.0);
        let knob = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        modules(
            ui,
            params,
            &[
                (
                    tr("Detection"),
                    vec![
                        knob("threshold", "Threshold"),
                        knob("tolerance", "Tolerance"),
                        knob("dynamics", "Dynamics"),
                    ],
                ),
                (
                    tr("Sound"),
                    vec![
                        knob("tune", "Tune"),
                        knob("decay", "Decay"),
                        knob("level", "Level"),
                    ],
                ),
            ],
            color,
            46.0,
        );
        ui.add_space(14.0);
        fader(
            ui,
            params,
            "room",
            (tr("Close mics"), tr("Room mics")),
            color,
            30.0,
        );
        ui.add_space(12.0);
        fader(
            ui,
            params,
            "amount",
            (tr("Track"), tr("Triggered sound")),
            color,
            44.0,
        );
    });
    ui.ctx().request_repaint();
}

/// Keeps the levels and the hits of the last seconds.
fn record(ui: &Ui, meters: &[f32], state: &mut TriggerState) {
    let now = ui.input(|i| i.time);
    let [input, detection, count, velocity] = [0, 1, 2, 3].map(|k| meters.get(k).copied());
    if let (Some(input), Some(detection)) = (input, detection) {
        state.levels.push_back((now, detection, input));
    }
    if let (Some(count), Some(velocity)) = (count, velocity) {
        if state.count.is_some_and(|c| c != count) {
            state.hits.push_back((now, velocity));
        }
        state.count = Some(count);
    }
    while state.levels.front().is_some_and(|l| now - l.0 > SPAN + 0.2) {
        state.levels.pop_front();
    }
    while state.hits.front().is_some_and(|h| now - h.0 > SPAN + 0.2) {
        state.hits.pop_front();
    }
}

/// Kick or snare, as two wide pills, with what the delay is.
fn kind_switch(ui: &mut Ui, params: &mut Params, kind: usize) {
    ui.horizontal(|ui| {
        for (k, label) in [(0, tr("Kick")), (1, tr("Snare"))] {
            let on = kind == k;
            let (rect, response) = ui.allocate_exact_size(vec2(150.0, 32.0), Sense::click());
            let color = tint(k);
            let fill = if on {
                color.gamma_multiply(0.85)
            } else if response.hovered() {
                theme::mix(MODULE, color, 0.25)
            } else {
                MODULE
            };
            ui.painter().rect_filled(rect, 16.0, fill);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                theme::strong(14.0),
                if on {
                    Color32::from_rgb(20, 16, 14)
                } else {
                    INK
                },
            );
            let changed = (response.clicked() && !on).then_some(k as f32);
            params.set("kind", changed, &response);
            if changed.is_some() {
                params.doc.history_mut().end_gesture();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(tr("20 ms look-ahead, compensated by the mix"))
                    .size(11.0)
                    .color(INK.gamma_multiply(0.45)),
            );
        });
    });
}

/// The sounds of the drum as cards across the width: click one to choose and hear it.
fn cards(ui: &mut Ui, params: &mut Params, kind: usize) {
    let (id, names): (&str, &[&str]) = if kind == 0 {
        ("kick", &KICKS)
    } else {
        ("snare", &SNARES)
    };
    let chosen = params.get(id).round() as usize;
    let color = tint(kind);
    let gap = 8.0;
    let width =
        ((ui.available_width() - gap * (names.len() - 1) as f32) / names.len() as f32).floor();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (k, name) in names.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(width, 78.0), Sense::click());
            let on = k == chosen;
            let painter = ui.painter_at(rect.expand(3.0));
            let fill = theme::mix(
                MODULE,
                color,
                if on {
                    0.32
                } else if response.hovered() {
                    0.16
                } else {
                    0.06
                },
            );
            painter.rect_filled(rect, 8.0, fill);
            if on {
                painter.rect_stroke(
                    rect,
                    8.0,
                    Stroke::new(2.0, color),
                    egui::StrokeKind::Outside,
                );
            }
            painter.text(
                rect.left_top() + vec2(9.0, 8.0),
                Align2::LEFT_TOP,
                tr(name),
                theme::strong(13.0),
                INK,
            );
            painter.text(
                rect.left_top() + vec2(9.0, 26.0),
                Align2::LEFT_TOP,
                tr(subtitle(kind, k)),
                FontId::proportional(10.5),
                INK.gamma_multiply(0.6),
            );
            let wave = Rect::from_min_max(
                rect.left_top() + vec2(9.0, 44.0),
                rect.right_bottom() - vec2(9.0, 7.0),
            );
            waveform(
                &painter,
                wave,
                &trigger::shape(kind, k, 80),
                if on { color } else { INK.gamma_multiply(0.55) },
            );
            let response = response.on_hover_text(format!(
                "{}\n{}",
                tr("Click to choose it and hear it"),
                source(kind, k)
            ));
            let changed = (response.clicked() && !on).then_some(k as f32);
            params.set(id, changed, &response);
            if changed.is_some() {
                params.doc.history_mut().end_gesture();
            }
            if response.clicked() {
                audition(params, kind, k);
            }
        }
    });
}

/// Plays a hard hit of a sound, close to room as set.
fn audition(params: &mut Params, kind: usize, sound: usize) {
    let rate = params.doc.player().engine().sample_rate();
    let room = params.get("room") / 100.0;
    let hit = trigger::audition(kind, sound, 0.9, room, rate);
    let data = audiostrada_media::AudioData::new(vec![hit.clone(), hit], rate);
    params.doc.player_mut().preview(Some(Arc::new(data)));
}

/// The detection of the last seconds, the input behind it, the threshold as a line to drag,
/// and a mark at each hit as strong as it was.
fn display(ui: &mut Ui, params: &mut Params, state: &TriggerState, color: Color32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, MODULE);
    let plot = rect.shrink2(vec2(10.0, 10.0));
    let y_of = |db: f32| plot.bottom() - (db - FLOOR) / -FLOOR * plot.height();
    for line in [-48.0, -36.0, -24.0, -12.0, 0.0] {
        let y = y_of(line);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, INK.gamma_multiply(0.06)),
        );
        painter.text(
            pos2(plot.left() + 2.0, y - 1.0),
            Align2::LEFT_BOTTOM,
            format!("{line:.0}"),
            FontId::monospace(9.0),
            INK.gamma_multiply(0.3),
        );
    }
    let now = ui.input(|i| i.time);
    let x_of = |t: f64| plot.right() - ((now - t) / SPAN) as f32 * plot.width();
    let threshold = params.get("threshold");
    // The input, faint, then the detection, filled, brighter over the threshold.
    let curve = |pick: fn(&(f64, f32, f32)) -> f32| -> Vec<Pos2> {
        state
            .levels
            .iter()
            .filter(|l| now - l.0 <= SPAN)
            .map(|l| pos2(x_of(l.0), y_of(db(pick(l)).max(FLOOR - 2.0))))
            .collect()
    };
    let input = curve(|l| l.2);
    let detection = curve(|l| l.1);
    fill(&painter, &input, plot.bottom(), INK.gamma_multiply(0.07));
    fill(
        &painter,
        &detection,
        plot.bottom(),
        color.gamma_multiply(0.28),
    );
    if detection.len() > 1 {
        painter.add(Shape::line(
            detection.clone(),
            Stroke::new(1.5, color.gamma_multiply(0.9)),
        ));
    }
    // Each hit: a line from the bottom and a dot as high as it was strong.
    for &(t, strength) in &state.hits {
        if now - t > SPAN {
            continue;
        }
        let x = x_of(t);
        let age = ((now - t) / 0.25).min(1.0) as f32;
        let top = plot.bottom() - (0.2 + 0.8 * strength) * plot.height();
        painter.line_segment(
            [pos2(x, plot.bottom()), pos2(x, top)],
            Stroke::new(2.0, color.gamma_multiply(0.75)),
        );
        painter.circle_filled(pos2(x, top), 4.0 + 5.0 * (1.0 - age), color);
    }
    // The light of the last hit.
    if let Some(&(t, _)) = state.hits.back() {
        let glow = (1.0 - (now - t) / 0.3).max(0.0) as f32;
        if glow > 0.0 {
            painter.circle_filled(
                rect.right_top() + vec2(-18.0, 18.0),
                7.0,
                color.gamma_multiply(glow),
            );
        }
    }
    let y = y_of(threshold);
    let dashes = Shape::dashed_line(
        &[pos2(plot.left(), y), pos2(plot.right(), y)],
        Stroke::new(1.5, INK.gamma_multiply(0.85)),
        6.0,
        4.0,
    );
    painter.extend(dashes);
    painter.text(
        pos2(plot.right() - 30.0, y - 3.0),
        Align2::RIGHT_BOTTOM,
        format!("{} {threshold:.0} dB", tr("Threshold")),
        FontId::proportional(11.0),
        INK,
    );
    if state.levels.iter().all(|l| l.1 < 1e-3 && l.2 < 1e-3) {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr("Play the track: its hits show here, each one found is marked"),
            FontId::proportional(12.5),
            INK.gamma_multiply(0.45),
        );
    }
    let response = response.on_hover_text(tr(
        "Drag up or down to set the threshold: hits over the line play the sample",
    ));
    let changed = (response.dragged() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|pos| (FLOOR * (1.0 - (plot.bottom() - pos.y) / plot.height())).clamp(FLOOR, 0.0))
        .map(|v| v.round());
    params.set("threshold", changed, &response);
}

/// A curve filled down to `bottom`.
fn fill(painter: &egui::Painter, points: &[Pos2], bottom: f32, color: Color32) {
    if points.len() < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for p in points {
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(pos2(p.x, bottom), color);
        if i >= 2 {
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A wide fader between two ends, each end's share shown at its side.
fn fader(
    ui: &mut Ui,
    params: &mut Params,
    id: &str,
    (left, right): (&str, &str),
    color: Color32,
    height: f32,
) {
    let Some(param) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let t = (value - param.min) / (param.max - param.min);
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), height + 18.0),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect.expand(2.0));
    let labels = Rect::from_min_size(rect.min, vec2(rect.width(), 16.0));
    // The share of each end: both whole at the middle for the trigger, a crossfade for the room.
    let (a, b) = if id == "amount" {
        ((2.0 * (1.0 - t)).min(1.0), (2.0 * t).min(1.0))
    } else {
        (1.0 - t, t)
    };
    painter.text(
        labels.left_center(),
        Align2::LEFT_CENTER,
        format!("{left}  {:.0} %", a * 100.0),
        theme::strong(12.0),
        INK.gamma_multiply(0.45 + 0.55 * a),
    );
    painter.text(
        labels.right_center(),
        Align2::RIGHT_CENTER,
        format!("{:.0} %  {right}", b * 100.0),
        theme::strong(12.0),
        INK.gamma_multiply(0.45 + 0.55 * b),
    );
    let track = Rect::from_min_max(pos2(rect.left(), rect.top() + 20.0), rect.right_bottom());
    let r = track.height() / 2.0;
    painter.rect_filled(track, r, MODULE);
    let x = track.left() + r + t * (track.width() - 2.0 * r);
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(x, track.bottom())),
        r,
        color.gamma_multiply(0.35 + 0.4 * t),
    );
    painter.text(
        track.center(),
        Align2::CENTER_CENTER,
        if id == "amount" {
            tr("Blend")
        } else {
            tr("Room")
        },
        FontId::proportional(11.0),
        INK.gamma_multiply(0.35),
    );
    let knob = Rect::from_center_size(
        pos2(x, track.center().y),
        vec2(2.0 * r - 6.0, 2.0 * r - 6.0),
    );
    painter.rect_filled(knob, r, INK);
    painter.rect_stroke(knob, r, Stroke::new(2.0, color), egui::StrokeKind::Inside);
    let response = response.on_hover_text(tr("Drag, or click a point; double-click to reset"));
    let changed = if response.double_clicked() {
        Some(param.default)
    } else if response.dragged() || response.clicked() {
        response.interact_pointer_pos().map(|pos| {
            let t = ((pos.x - track.left() - r) / (track.width() - 2.0 * r)).clamp(0.0, 1.0);
            (param.min + t * (param.max - param.min)).round()
        })
    } else {
        None
    };
    params.set(id, changed, &response);
}

/// Starting points: reinforce or replace, close or roomy.
pub(crate) fn factory() -> Vec<crate::presets::Preset> {
    use crate::presets::preset;
    vec![
        preset(
            tr("Kick: reinforce"),
            &[
                ("kind", 0.0),
                ("kick", 0.0),
                ("amount", 35.0),
                ("room", 15.0),
            ],
        ),
        preset(
            tr("Kick: replace"),
            &[
                ("kind", 0.0),
                ("kick", 2.0),
                ("amount", 100.0),
                ("room", 25.0),
            ],
        ),
        preset(
            tr("Kick: big and open"),
            &[
                ("kind", 0.0),
                ("kick", 3.0),
                ("amount", 60.0),
                ("room", 40.0),
            ],
        ),
        preset(
            tr("Kick: fast double pedal"),
            &[
                ("kind", 0.0),
                ("kick", 5.0),
                ("amount", 70.0),
                ("tolerance", 85.0),
                ("dynamics", 35.0),
                ("room", 10.0),
            ],
        ),
        preset(
            tr("Snare: reinforce"),
            &[
                ("kind", 1.0),
                ("snare", 1.0),
                ("amount", 35.0),
                ("room", 20.0),
            ],
        ),
        preset(
            tr("Snare: replace"),
            &[
                ("kind", 1.0),
                ("snare", 2.0),
                ("amount", 100.0),
                ("room", 30.0),
            ],
        ),
        preset(
            tr("Snare: in the room"),
            &[
                ("kind", 1.0),
                ("snare", 6.0),
                ("amount", 60.0),
                ("room", 75.0),
            ],
        ),
        preset(
            tr("Snare: tight and dry"),
            &[
                ("kind", 1.0),
                ("snare", 3.0),
                ("amount", 50.0),
                ("room", 0.0),
                ("decay", 60.0),
            ],
        ),
    ]
}
