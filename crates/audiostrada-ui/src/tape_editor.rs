//! Strada-Tape's editor: the machine drawn with its reels turning at the chosen speed and its
//! input and output VU meters, the machine, speed and tape chosen above, then the knobs.

use audiostrada_dsp::tape::{MACHINE_HINTS, TAPE_HINTS};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::drums_editor::{MODULE, modules, screen};
use crate::effect_editors::{Params, VuFace, segmented, vu_meter};
use crate::gear::{self, Texture};
use crate::i18n::tr;
use crate::tape_deck;
use crate::theme;

/// Every machine's face is this tall, so the window keeps its size between them.
const FACE_HEIGHT: f32 = 460.0;
const INK: Color32 = Color32::from_rgb(236, 232, 222);

/// Each machine's panel color and its accent.
fn colors(machine: usize) -> (Color32, Color32) {
    match machine {
        1 => (
            Color32::from_rgb(150, 154, 160),
            Color32::from_rgb(90, 170, 230),
        ),
        2 => (
            Color32::from_rgb(120, 112, 98),
            Color32::from_rgb(230, 170, 80),
        ),
        3 => (
            Color32::from_rgb(58, 56, 60),
            Color32::from_rgb(240, 110, 90),
        ),
        _ => (
            Color32::from_rgb(176, 178, 182),
            Color32::from_rgb(232, 190, 90),
        ),
    }
}

/// The meters' needles, and the tape run so far with how fast it runs now (0 to 1).
#[derive(Debug, Default)]
pub struct TapeState {
    needles: [f32; 2],
    travel: f32,
    spin: f32,
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut TapeState) {
    let machine = (params.get("machine").round() as usize).min(3);
    let (panel, accent) = colors(machine);
    // The reels roll while sound comes in, starting and stopping gently.
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let sounding = meters.first().is_some_and(|v| *v > -40.0);
    let target = if sounding { 1.0 } else { 0.0 };
    state.spin += (target - state.spin) * (1.0 - (-dt / 0.6).exp());
    let speed = [0.5, 1.0, 2.0][(params.get("speed").round() as usize).min(2)];
    let wow = params.get("wow") / 100.0;
    let time = ui.input(|i| i.time) as f32;
    state.travel += dt * state.spin * speed * 70.0 * (1.0 + wow * 0.08 * (time * 3.3).sin());
    if machine == 3 {
        machine_face(ui, params, meters, state, panel);
    } else {
        deck_face(ui, params, meters, state, machine);
    }
    ui.add_space(10.0);
    screen(ui, |ui| {
        ui.horizontal(|ui| {
            selector(ui, params, tr("Machine"), "machine", &MACHINE_HINTS);
            ui.add_space(18.0);
            selector(ui, params, tr("Speed"), "speed", &[]);
            ui.add_space(18.0);
            selector(ui, params, tr("Tape"), "tape", &TAPE_HINTS);
        });
        ui.add_space(12.0);
        let knob = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        modules(
            ui,
            params,
            &[
                (
                    tr("Recording"),
                    vec![knob("drive", "Input"), knob("bias", "Bias")],
                ),
                (
                    tr("Tape"),
                    vec![
                        knob("bump", "Head bump"),
                        knob("wow", "Wow & flutter"),
                        knob("hiss", "Hiss"),
                    ],
                ),
                (
                    tr("Output"),
                    vec![knob("output", "Output"), knob("mix", "Mix")],
                ),
            ],
            accent,
            48.0,
        );
    });
    if sounding || state.spin > 0.002 {
        crate::animate(ui.ctx());
    } else {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// A reel-to-reel machine, its meters where it has them.
fn deck_face(
    ui: &mut Ui,
    params: &mut Params,
    meters: &[f32],
    state: &mut TapeState,
    machine: usize,
) {
    let width = ui.available_width();
    let height = FACE_HEIGHT;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect.expand(1.0), 4.0, Color32::from_black_alpha(120));
    let face = match machine {
        1 => tape_deck::mastering(ui, rect, params, state.travel),
        2 => tape_deck::vintage(ui, rect, params, state.travel),
        _ => tape_deck::studio(ui, rect, params, state.travel),
    };
    for (k, caption) in [tr("RECORD"), tr("PLAYBACK")].into_iter().enumerate() {
        let place = face.places[k];
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(place));
        let value = meters.get(k).copied().unwrap_or(-60.0);
        let look = face.face;
        vu_meter(
            &mut child,
            value,
            &mut state.needles[k],
            place.size(),
            (look, caption),
        );
    }
}

/// A labeled segmented choice.
fn selector(ui: &mut Ui, params: &mut Params, label: &str, id: &str, hints: &[&'static str]) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(10.5)
                .color(INK.gamma_multiply(0.6)),
        );
        ui.horizontal(|ui| segmented(ui, params, id, hints));
    });
}

/// The machine: a brushed panel, two reels turning with the speed and wobbling with the wow,
/// the tape running over the heads, and the input and output meters.
fn machine_face(
    ui: &mut Ui,
    params: &mut Params,
    meters: &[f32],
    state: &mut TapeState,
    panel: Color32,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, FACE_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect.expand(12.0));
    gear::shadow(&painter, rect);
    gear::tile(&painter, rect, Texture::Brushed, 1.0, panel);
    gear::sheen(
        &painter,
        rect,
        Color32::from_white_alpha(40),
        Color32::from_black_alpha(70),
    );
    gear::bevel(&painter, rect, 1.0);
    for corner in [
        rect.left_top() + vec2(12.0, 12.0),
        rect.right_top() + vec2(-12.0, 12.0),
        rect.left_bottom() + vec2(12.0, -12.0),
        rect.right_bottom() + vec2(-12.0, -12.0),
    ] {
        painter.circle_filled(corner, 4.0, Color32::from_black_alpha(90));
        painter.circle_filled(corner - vec2(0.5, 0.5), 3.0, Color32::from_gray(200));
    }
    // Input on the left, output on the right, the cassette between them.
    let meter = vec2(214.0, 142.0);
    let middle = rect.center().y - 14.0;
    let places = [
        Rect::from_min_size(pos2(rect.left() + 26.0, middle - meter.y / 2.0), meter),
        Rect::from_min_size(
            pos2(rect.right() - 26.0 - meter.x, middle - meter.y / 2.0),
            meter,
        ),
    ];
    let room = places[1].left() - places[0].right() - 40.0;
    let scale = (room / 300.0).min(1.6);
    cassette(
        &painter,
        pos2(rect.center().x, middle),
        scale,
        state.travel / 40.0,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 24.0),
        Align2::CENTER_BOTTOM,
        format!(
            "{}  ·  {}",
            tr(audiostrada_dsp::tape::MACHINES[3]),
            audiostrada_dsp::tape::SPEEDS[(params.get("speed").round() as usize).min(2)]
        ),
        theme::strong(14.0),
        INK,
    );
    for (k, label, caption) in [
        (0, tr("Input"), tr("RECORD")),
        (1, tr("Output"), tr("PLAYBACK")),
    ] {
        let place = places[k];
        // A child of its own: drawn over the face without moving what follows.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(place));
        let value = meters.get(k).copied().unwrap_or(-60.0);
        vu_meter(
            &mut child,
            value,
            &mut state.needles[k],
            place.size(),
            (VuFace::Amber, caption),
        );
        painter.text(
            pos2(place.center().x, place.bottom() + 10.0),
            Align2::CENTER_TOP,
            label,
            theme::strong(12.0),
            INK,
        );
    }
}

/// A cassette in its deck, `scale` times its natural size: the shell and its screws, the
/// label, and the two hubs turning.
fn cassette(painter: &egui::Painter, center: egui::Pos2, scale: f32, angle: f32) {
    let s = scale;
    let shell = Rect::from_center_size(center, vec2(300.0, 150.0) * s);
    gear::shadow(painter, shell);
    painter.rect_filled(shell, 10.0 * s, Color32::from_rgb(34, 34, 38));
    gear::sheen(
        painter,
        shell.shrink(6.0 * s),
        Color32::from_white_alpha(14),
        Color32::from_black_alpha(0),
    );
    for corner in [
        shell.left_top() + vec2(9.0, 9.0) * s,
        shell.right_top() + vec2(-9.0, 9.0) * s,
        shell.left_bottom() + vec2(9.0, -9.0) * s,
        shell.right_bottom() + vec2(-9.0, -9.0) * s,
        pos2(shell.center().x, shell.bottom() - 9.0 * s),
    ] {
        painter.circle_filled(corner, 3.0 * s, Color32::from_gray(70));
        painter.line_segment(
            [corner - vec2(2.0, 0.0) * s, corner + vec2(2.0, 0.0) * s],
            Stroke::new(1.0, Color32::from_gray(30)),
        );
    }
    let label = shell.shrink2(vec2(18.0, 14.0) * s);
    let label = Rect::from_min_max(label.min, pos2(label.right(), label.bottom() - 30.0 * s));
    painter.rect_filled(label, 6.0 * s, Color32::from_rgb(232, 224, 200));
    painter.rect_filled(
        Rect::from_min_size(
            label.min + vec2(0.0, 10.0 * s),
            vec2(label.width(), 6.0 * s),
        ),
        0.0,
        Color32::from_rgb(220, 90, 70),
    );
    let window = Rect::from_center_size(
        label.center() + vec2(0.0, 12.0 * s),
        vec2(label.width() * 0.62, 38.0 * s),
    );
    painter.rect_filled(window, 8.0 * s, Color32::from_rgb(28, 24, 22));
    for (k, x) in [window.left() + 26.0 * s, window.right() - 26.0 * s]
        .into_iter()
        .enumerate()
    {
        let center = pos2(x, window.center().y);
        painter.circle_filled(
            center,
            if k == 0 { 15.0 } else { 11.0 } * s,
            Color32::from_rgb(70, 46, 30),
        );
        painter.circle_filled(center, 8.0 * s, Color32::from_gray(225));
        for t in 0..6 {
            let a = angle * 2.0 + t as f32 * std::f32::consts::TAU / 6.0;
            painter.line_segment(
                [
                    center + vec2(a.cos(), a.sin()) * 4.0 * s,
                    center + vec2(a.cos(), a.sin()) * 7.5 * s,
                ],
                Stroke::new(1.5 * s, Color32::from_gray(90)),
            );
        }
    }
    painter.rect_filled(
        Rect::from_center_size(
            pos2(shell.center().x, shell.bottom() - 12.0 * s),
            vec2(shell.width() * 0.55, 14.0 * s),
        ),
        3.0 * s,
        MODULE,
    );
    painter.text(
        label.left_top() + vec2(10.0, 22.0) * s,
        Align2::LEFT_TOP,
        "C-90",
        FontId::proportional(11.0 * s),
        Color32::from_rgb(40, 34, 30),
    );
}
