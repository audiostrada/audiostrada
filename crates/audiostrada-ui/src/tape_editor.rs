//! Strada-Tape's editor: the machine drawn with its reels turning at the chosen speed and its
//! input and output VU meters, the machine, speed and tape chosen above, then the knobs.

use audiostrada_dsp::tape::{MACHINE_HINTS, TAPE_HINTS};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::drums_editor::{MODULE, modules, screen};
use crate::effect_editors::{Params, segmented, vu_meter};
use crate::gear::{self, Texture};
use crate::i18n::tr;
use crate::theme;

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

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], needles: &mut [f32; 2]) {
    let machine = (params.get("machine").round() as usize).min(3);
    let (panel, accent) = colors(machine);
    machine_face(ui, params, meters, needles, machine, panel);
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
    ui.ctx().request_repaint();
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
    needles: &mut [f32; 2],
    machine: usize,
    panel: Color32,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 210.0), Sense::hover());
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
    let meter_width = 168.0;
    let deck = Rect::from_min_max(
        rect.min + vec2(24.0, 18.0),
        pos2(
            rect.right() - 2.0 * meter_width - 40.0,
            rect.bottom() - 18.0,
        ),
    );
    let speed = [0.5, 1.0, 2.0][(params.get("speed").round() as usize).min(2)];
    let wow = params.get("wow") / 100.0;
    let time = ui.input(|i| i.time) as f32;
    let angle = time * speed * 1.6 + wow * 0.12 * (time * 3.3).sin();
    if machine == 3 {
        cassette(&painter, deck, angle);
    } else {
        reels(&painter, deck, angle);
    }
    // The name on the panel.
    let name = audiostrada_dsp::tape::MACHINES[machine];
    let letters = if machine == 3 {
        INK
    } else {
        Color32::from_rgb(28, 28, 30)
    };
    painter.text(
        pos2(deck.center().x, rect.bottom() - 14.0),
        Align2::CENTER_BOTTOM,
        format!(
            "{}  ·  {}",
            tr(name),
            audiostrada_dsp::tape::SPEEDS[(params.get("speed").round() as usize).min(2)]
        ),
        theme::strong(13.0),
        letters,
    );
    // The meters.
    for (k, label, caption) in [
        (0, tr("Input"), tr("RECORD")),
        (1, tr("Output"), tr("PLAYBACK")),
    ] {
        let place = Rect::from_min_size(
            pos2(
                rect.right() - (2 - k) as f32 * (meter_width + 12.0) - 12.0,
                rect.top() + 22.0,
            ),
            vec2(meter_width, 140.0),
        );
        // A child of its own: drawn over the face without moving what follows.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(place));
        let value = meters.get(k).copied().unwrap_or(-60.0);
        vu_meter(
            &mut child,
            value,
            &mut needles[k],
            place.size(),
            (true, caption),
        );
        painter.text(
            pos2(place.center().x, place.bottom() + 8.0),
            Align2::CENTER_TOP,
            label,
            theme::strong(11.0),
            letters,
        );
    }
}

/// Two open reels with their tape and the head block between them.
fn reels(painter: &egui::Painter, deck: Rect, angle: f32) {
    let r = (deck.height() * 0.42).min(deck.width() * 0.22);
    let left = pos2(deck.left() + r + 10.0, deck.top() + r + 4.0);
    let right = pos2(deck.right() - r - 10.0, deck.top() + r + 4.0);
    let head = Rect::from_center_size(
        pos2(deck.center().x, deck.bottom() - 26.0),
        vec2(70.0, 26.0),
    );
    // The tape path: from each reel's pack round the guides to the heads.
    let tape = Stroke::new(3.0, Color32::from_rgb(70, 46, 30));
    painter.line_segment(
        [
            left + vec2(-r * 0.78, r * 0.3),
            head.left_top() + vec2(-8.0, 6.0),
        ],
        tape,
    );
    painter.line_segment(
        [
            head.left_top() + vec2(-8.0, 6.0),
            head.right_top() + vec2(8.0, 6.0),
        ],
        tape,
    );
    painter.line_segment(
        [
            head.right_top() + vec2(8.0, 6.0),
            right + vec2(r * 0.6, r * 0.5),
        ],
        tape,
    );
    for (center, pack, turn) in [(left, 0.8, angle), (right, 0.5, angle * 1.35)] {
        reel(painter, center, r, pack, turn);
    }
    painter.rect_filled(head, 4.0, Color32::from_rgb(60, 62, 66));
    gear::bevel(painter, head, 1.0);
    for k in 0..3 {
        let x = head.left() + 14.0 + k as f32 * 21.0;
        painter.rect_filled(
            Rect::from_center_size(pos2(x, head.top() + 4.0), vec2(12.0, 8.0)),
            2.0,
            Color32::from_gray(180),
        );
    }
}

/// A metal reel: the brown pack of tape, three windows and the hub.
fn reel(painter: &egui::Painter, center: Pos2, r: f32, pack: f32, angle: f32) {
    gear::glow(
        painter,
        center + vec2(3.0, 5.0),
        r * 1.08,
        Color32::from_black_alpha(110),
        Color32::from_black_alpha(0),
    );
    painter.circle_filled(
        center,
        r * (0.32 + 0.6 * pack),
        Color32::from_rgb(64, 42, 28),
    );
    painter.circle_stroke(
        center,
        r * (0.32 + 0.6 * pack),
        Stroke::new(1.0, Color32::from_rgb(90, 62, 42)),
    );
    painter.circle_stroke(center, r, Stroke::new(3.0, Color32::from_gray(205)));
    painter.circle_stroke(
        center,
        r - 2.0,
        Stroke::new(1.0, Color32::from_black_alpha(90)),
    );
    for k in 0..3 {
        let a = angle + k as f32 * std::f32::consts::TAU / 3.0;
        let dir = vec2(a.cos(), a.sin());
        let side = vec2(-dir.y, dir.x);
        painter.add(egui::Shape::convex_polygon(
            vec![
                center + dir * r * 0.3 + side * r * 0.05,
                center + dir * r * 0.94 + side * r * 0.32,
                center + dir * r * 0.94 - side * r * 0.32,
                center + dir * r * 0.3 - side * r * 0.05,
            ],
            Color32::from_gray(185),
            Stroke::new(1.0, Color32::from_black_alpha(80)),
        ));
    }
    painter.circle_filled(center, r * 0.22, Color32::from_gray(160));
    painter.circle_filled(center, r * 0.08, Color32::from_gray(60));
    for k in 0..3 {
        let a = angle + (k as f32 + 0.5) * std::f32::consts::TAU / 3.0;
        painter.circle_filled(
            center + vec2(a.cos(), a.sin()) * r * 0.15,
            2.0,
            Color32::from_gray(90),
        );
    }
}

/// A cassette in its deck: the shell, the label and the two hubs turning.
fn cassette(painter: &egui::Painter, deck: Rect, angle: f32) {
    let shell = Rect::from_center_size(
        deck.center() - vec2(0.0, 6.0),
        vec2(deck.width().min(300.0), deck.height().min(150.0)),
    );
    gear::shadow(painter, shell);
    painter.rect_filled(shell, 10.0, Color32::from_rgb(34, 34, 38));
    let label = shell.shrink2(vec2(18.0, 14.0));
    let label = Rect::from_min_max(label.min, pos2(label.right(), label.bottom() - 30.0));
    painter.rect_filled(label, 6.0, Color32::from_rgb(232, 224, 200));
    painter.rect_filled(
        Rect::from_min_size(label.min + vec2(0.0, 10.0), vec2(label.width(), 6.0)),
        0.0,
        Color32::from_rgb(220, 90, 70),
    );
    let window = Rect::from_center_size(
        label.center() + vec2(0.0, 12.0),
        vec2(label.width() * 0.62, 38.0),
    );
    painter.rect_filled(window, 8.0, Color32::from_rgb(28, 24, 22));
    for (k, x) in [window.left() + 26.0, window.right() - 26.0]
        .into_iter()
        .enumerate()
    {
        let center = pos2(x, window.center().y);
        painter.circle_filled(
            center,
            if k == 0 { 15.0 } else { 11.0 },
            Color32::from_rgb(70, 46, 30),
        );
        painter.circle_filled(center, 8.0, Color32::from_gray(225));
        for t in 0..6 {
            let a = angle * 2.0 + t as f32 * std::f32::consts::TAU / 6.0;
            painter.line_segment(
                [
                    center + vec2(a.cos(), a.sin()) * 4.0,
                    center + vec2(a.cos(), a.sin()) * 7.5,
                ],
                Stroke::new(1.5, Color32::from_gray(90)),
            );
        }
    }
    painter.rect_filled(
        Rect::from_center_size(
            pos2(shell.center().x, shell.bottom() - 12.0),
            vec2(shell.width() * 0.55, 14.0),
        ),
        3.0,
        MODULE,
    );
    painter.text(
        label.left_top() + vec2(10.0, 22.0),
        Align2::LEFT_TOP,
        "C-90",
        FontId::proportional(11.0),
        Color32::from_rgb(40, 34, 30),
    );
}
