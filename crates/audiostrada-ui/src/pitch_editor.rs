//! Strada-Pitch and Strada-Doubler's editors. Pitch: a keyboard of four octaves around the
//! original note where a click sets voice A and a right click voice B, each voice's interval
//! stepped by semitones, then their cents, levels and sides, the echo and the output.
//! Doubler: the stereo field seen from above, the dry sound in the middle and the doubles
//! placed by side and delay, drifting; then the voices, the character and the output.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::drums_editor::{MODULE, modules, screen};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;

const VOICE_A: Color32 = Color32::from_rgb(242, 150, 70);
const VOICE_B: Color32 = Color32::from_rgb(90, 200, 230);
const SPAN: i32 = 24;

/// The name of an interval in semitones, up or down.
fn interval(semitones: i32) -> String {
    const NAMES: [&str; 12] = [
        "unison",
        "minor second",
        "second",
        "minor third",
        "major third",
        "fourth",
        "tritone",
        "fifth",
        "minor sixth",
        "major sixth",
        "minor seventh",
        "major seventh",
    ];
    let n = semitones.unsigned_abs() as usize;
    let (octaves, rest) = (n / 12, n % 12);
    let name = match (octaves, rest) {
        (0, r) => tr(NAMES[r]).to_owned(),
        (o, 0) => {
            if o == 1 {
                tr("octave").to_owned()
            } else {
                crate::trf!("{count} octaves", count = o)
            }
        }
        (o, r) => crate::trf!(
            "{interval} + {count} octave(s)",
            interval = tr(NAMES[r]),
            count = o
        ),
    };
    match semitones.signum() {
        1 => crate::trf!("{interval} up", interval = name),
        -1 => crate::trf!("{interval} down", interval = name),
        _ => name,
    }
}

pub fn pitch(ui: &mut Ui, params: &mut Params) {
    screen(ui, |ui| {
        keyboard(ui, params);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let half = (ui.available_width() - 8.0) / 2.0;
            for (voice, color) in [("a", VOICE_A), ("b", VOICE_B)] {
                stepper(ui, params, voice, color, half);
            }
        });
        ui.add_space(10.0);
        let k = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        ui.columns(2, |columns| {
            for (column, (voice, title, color)) in columns
                .iter_mut()
                .zip([("a", tr("Voice A"), VOICE_A), ("b", tr("Voice B"), VOICE_B)])
            {
                let id = |field: &str| format!("{voice}_{field}");
                modules(
                    column,
                    params,
                    &[(
                        title,
                        vec![
                            (id("cents"), tr("Cents")),
                            (id("level"), tr("Level")),
                            (id("pan"), tr("Pan")),
                        ],
                    )],
                    color,
                    44.0,
                );
            }
        });
        ui.add_space(8.0);
        modules(
            ui,
            params,
            &[
                (
                    tr("Echo"),
                    vec![
                        k("delay", "Delay"),
                        k("feedback", "Feedback"),
                        k("tone", "Tone"),
                    ],
                ),
                (tr("Grain"), vec![k("window", "Window")]),
                (tr("Output"), vec![k("mix", "Mix"), k("output", "Output")]),
            ],
            Color32::from_rgb(170, 180, 200),
            44.0,
        );
    });
}

/// Four octaves of keys around the original note; voice A and B lit on theirs.
fn keyboard(ui: &mut Ui, params: &mut Params) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 112.0), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(10, 12, 17));
    let keys = Rect::from_min_max(rect.min + vec2(10.0, 26.0), rect.max - vec2(10.0, 10.0));
    let a = params.get("a_semis").round() as i32;
    let b = params.get("b_semis").round() as i32;
    let b_on = params.get("b_level") > 0.0;
    // The original note sits on C: the white keys of four octaves around it.
    let black = |s: i32| matches!(s.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
    let whites: Vec<i32> = (-SPAN..=SPAN).filter(|s| !black(*s)).collect();
    let key_w = keys.width() / whites.len() as f32;
    let white_x = |s: i32| {
        whites
            .iter()
            .position(|w| *w == s)
            .map(|i| keys.left() + i as f32 * key_w)
    };
    let mut places: Vec<(i32, Rect)> = Vec::new();
    for s in whites.iter().copied() {
        let x = white_x(s).unwrap_or(keys.left());
        places.push((
            s,
            Rect::from_min_size(pos2(x, keys.top()), vec2(key_w, keys.height())),
        ));
    }
    for s in (-SPAN..=SPAN).filter(|s| black(*s)) {
        let x = white_x(s - 1).unwrap_or(keys.left()) + key_w * 0.68;
        places.push((
            s,
            Rect::from_min_size(pos2(x, keys.top()), vec2(key_w * 0.64, keys.height() * 0.6)),
        ));
    }
    let lit = |s: i32| {
        if s == a {
            Some(VOICE_A)
        } else if b_on && s == b {
            Some(VOICE_B)
        } else {
            None
        }
    };
    for (s, place) in &places {
        let is_black = black(*s);
        let base = if is_black {
            Color32::from_rgb(30, 32, 38)
        } else {
            Color32::from_rgb(222, 222, 216)
        };
        let fill = lit(*s).map_or(base, |c| if is_black { c } else { c.gamma_multiply(0.85) });
        painter.rect_filled(place.shrink2(vec2(0.5, 0.0)), 2.0, fill);
        if !is_black {
            painter.rect_stroke(
                place.shrink2(vec2(0.5, 0.0)),
                2.0,
                Stroke::new(1.0, Color32::from_black_alpha(120)),
                egui::StrokeKind::Inside,
            );
        }
        if *s == 0 {
            painter.circle_filled(
                pos2(place.center().x, place.bottom() - 9.0),
                4.0,
                Color32::from_rgb(40, 40, 44),
            );
        }
    }
    for (s, label) in [
        (-24, "-2 oct"),
        (-12, "-1 oct"),
        (0, tr("Original")),
        (12, "+1 oct"),
        (24, "+2 oct"),
    ] {
        if let Some(x) = white_x(s) {
            painter.text(
                pos2(x + key_w / 2.0, rect.top() + 13.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                Color32::from_gray(150),
            );
        }
    }
    let hovered = response.hover_pos().and_then(|at| {
        places
            .iter()
            .rev()
            .filter(|(s, _)| black(*s))
            .chain(places.iter().filter(|(s, _)| !black(*s)))
            .find(|(_, r)| r.contains(at))
            .map(|(s, _)| *s)
    });
    let response = response.on_hover_text(tr("Click: voice A. Right-click: voice B"));
    if let Some(s) = hovered {
        if response.clicked() {
            params.set("a_semis", Some(s as f32), &response);
            params.doc.history_mut().end_gesture();
        } else if response.secondary_clicked() {
            params.set("b_semis", Some(s as f32), &response);
            if !b_on {
                params.set("b_level", Some(100.0), &response);
            }
            params.doc.history_mut().end_gesture();
        }
    }
}

/// A voice's interval: its name and value, a semitone down or up.
fn stepper(ui: &mut Ui, params: &mut Params, voice: &str, color: Color32, width: f32) {
    let id = format!("{voice}_semis");
    let semis = params.get(&id).round() as i32;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 52.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, MODULE);
    painter.circle_filled(pos2(rect.left() + 16.0, rect.center().y), 5.0, color);
    let name = if voice == "a" {
        tr("Voice A")
    } else {
        tr("Voice B")
    };
    painter.text(
        pos2(rect.left() + 28.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        name,
        theme::strong(12.0),
        color,
    );
    painter.text(
        pos2(rect.left() + 28.0, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        interval(semis),
        FontId::proportional(11.5),
        Color32::from_gray(190),
    );
    let readout =
        Rect::from_center_size(pos2(rect.right() - 92.0, rect.center().y), vec2(78.0, 34.0));
    painter.rect_filled(readout, 6.0, Color32::from_rgb(10, 12, 17));
    painter.text(
        readout.center(),
        Align2::CENTER_CENTER,
        format!("{semis:+} st"),
        FontId::monospace(17.0),
        Color32::from_gray(235),
    );
    for (k, (label, step)) in [("◀", -1), ("▶", 1)].into_iter().enumerate() {
        let button = Rect::from_center_size(
            pos2(
                if k == 0 {
                    readout.left() - 20.0
                } else {
                    readout.right() + 20.0
                },
                rect.center().y,
            ),
            vec2(28.0, 30.0),
        );
        let response = ui.interact(
            button,
            ui.scope_id().with(("pitch-step", voice, k)),
            Sense::click(),
        );
        let painter = ui.painter();
        painter.rect_filled(
            button,
            6.0,
            if response.hovered() {
                color.gamma_multiply(0.35)
            } else {
                Color32::from_rgb(34, 39, 50)
            },
        );
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(13.0),
            color,
        );
        let changed = response
            .clicked()
            .then_some((semis + step).clamp(-SPAN, SPAN) as f32);
        params.set(&id, changed, &response);
        if changed.is_some() {
            params.doc.history_mut().end_gesture();
        }
    }
}

pub fn doubler(ui: &mut Ui, params: &mut Params) {
    screen(ui, |ui| {
        field(ui, params);
        ui.add_space(10.0);
        let k = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        modules(
            ui,
            params,
            &[
                (
                    tr("Voices"),
                    vec![
                        k("voices", "Voices"),
                        k("detune", "Detune"),
                        k("delay", "Delay"),
                    ],
                ),
                (
                    tr("Character"),
                    vec![
                        k("drift", "Drift"),
                        k("width", "Width"),
                        k("low_cut", "Low cut"),
                    ],
                ),
                (
                    tr("Output"),
                    vec![k("mix", "Doubles"), k("output", "Output")],
                ),
            ],
            VOICE_B,
            46.0,
        );
    });
    ui.ctx().request_repaint();
}

/// The stereo field from above: the dry sound in front, each double at its side and as far
/// back as it is late, wandering with the drift.
fn field(ui: &mut Ui, params: &mut Params) {
    use audiostrada_dsp::doubler::SHAPES;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 220.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, Color32::from_rgb(10, 12, 17));
    let listener = pos2(rect.center().x, rect.bottom() - 18.0);
    for k in 1..=5 {
        let r = k as f32 * 38.0;
        let arc: Vec<Pos2> = (0..=40)
            .map(|i| {
                let a = std::f32::consts::PI * (1.0 + i as f32 / 40.0);
                listener + vec2(a.cos(), a.sin()) * r
            })
            .collect();
        painter.add(egui::Shape::line(
            arc,
            Stroke::new(1.0, Color32::from_white_alpha(12)),
        ));
    }
    painter.text(
        pos2(rect.left() + 14.0, rect.bottom() - 14.0),
        Align2::LEFT_CENTER,
        "L",
        theme::strong(12.0),
        Color32::from_gray(120),
    );
    painter.text(
        pos2(rect.right() - 14.0, rect.bottom() - 14.0),
        Align2::RIGHT_CENTER,
        "R",
        theme::strong(12.0),
        Color32::from_gray(120),
    );
    let count = if params.get("voices") >= 0.5 { 4 } else { 2 };
    let detune = params.get("detune");
    let delay = params.get("delay");
    let spread = params.get("width") / 100.0;
    let drift = params.get("drift") / 100.0;
    let level = params.get("mix") / 100.0;
    let time = ui.input(|i| i.time) as f32;
    let front = pos2(rect.center().x, rect.top() + 150.0);
    for (k, (cents, late, side)) in SHAPES.iter().enumerate().take(count) {
        let wobble = drift
            * vec2(
                (time * 0.7 + k as f32 * 1.9).sin(),
                (time * 0.53 + k as f32 * 2.3).cos(),
            )
            * 6.0;
        let x = rect.center().x + side * spread * (rect.width() / 2.0 - 70.0);
        let y = front.y - 26.0 - (delay * late).min(80.0) * 1.3;
        let at = pos2(x, y) + wobble;
        let radius = 8.0 + 10.0 * level;
        // A double taken by the hand: sideways sets the width, up and down the delay, for all
        // of them at once since they share these settings.
        let reach = rect.width() / 2.0 - 70.0;
        let response = ui
            .interact(
                Rect::from_center_size(at, vec2(2.0 * radius + 12.0, 2.0 * radius + 12.0)),
                ui.scope_id().with(("double", k)),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Grab)
            .on_hover_text(tr("Drag sideways for the width, up or down for the delay"));
        let held = response.dragged();
        let pointer = response.interact_pointer_pos().filter(|_| held);
        let width =
            pointer.map(|pos| ((pos.x - rect.center().x) * side / reach * 100.0).clamp(0.0, 100.0));
        let late_ms = pointer.map(|pos| ((front.y - 26.0 - pos.y) / 1.3 / late).clamp(0.0, 60.0));
        params.set("width", width, &response);
        params.change("delay", late_ms, &response);
        if held {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        let radius = if held || response.hovered() {
            radius + 3.0
        } else {
            radius
        };
        painter.line_segment([front, at], Stroke::new(1.0, VOICE_B.gamma_multiply(0.25)));
        painter.circle_filled(at, radius + 6.0, VOICE_B.gamma_multiply(0.12));
        painter.circle_filled(at, radius, VOICE_B.gamma_multiply(0.4 + 0.5 * level));
        painter.text(
            at + vec2(0.0, radius + 10.0),
            Align2::CENTER_CENTER,
            format!("{:+.0} ct · {:.0} ms", cents * detune, delay * late),
            FontId::proportional(10.0),
            Color32::from_gray(190),
        );
    }
    painter.circle_filled(front, 14.0, Color32::from_gray(230));
    painter.text(
        front,
        Align2::CENTER_CENTER,
        tr("Dry"),
        theme::strong(9.5),
        Color32::from_gray(30),
    );
    painter.circle_filled(listener, 5.0, Color32::from_gray(150));
}
