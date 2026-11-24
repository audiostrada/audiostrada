//! Strada-Rotary's editor: the cabinet seen from above, its horn and drum turning as the
//! effect turns them, the microphones beside them; then the speed, the cabinet and the output.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};

use audiostrada_dsp::rotary::meter;

use crate::controls::{EditorParams, animate, section_frame, wide_knob};
use crate::effects::display;
use crate::gear::{self, Texture};
use crate::i18n::tr;
use crate::theme;

pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    let p = theme::current(ui);
    cabinet(ui, params, meters);
    animate(ui.ctx());
    ui.add_space(8.0);
    let accent = p.accent;
    let knob = |ui: &mut Ui, params: &mut dyn EditorParams, id: &str| {
        let name = params.param(id).map_or("", |q| tr(q.name));
        wide_knob(ui, params, id, name, 44.0, accent);
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("Speed"), p.weak, 118.0, |ui| {
            // The speeds stacked beside the inertia, as high as a knob: one click while playing.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    speed_buttons(ui, params);
                });
                knob(ui, params, "inertia");
            });
        });
        section_frame(ui, tr("Rotary cabinet"), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for id in ["drive", "balance", "width"] {
                    knob(ui, params, id);
                }
            });
        });
        section_frame(ui, tr("Output"), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                knob(ui, params, "mix");
                knob(ui, params, "output");
            });
        });
    });
}

/// Stop, Slow and Fast, one under the other, the speed chosen lit.
fn speed_buttons(ui: &mut Ui, params: &mut dyn EditorParams) {
    let Some(p) = params.param("speed") else {
        return;
    };
    params.shown("speed");
    let value = params.get("speed").round();
    let hints = [
        "The rotors slow down to a stop",
        "The slow chorale",
        "The fast tremolo",
    ];
    for (i, name) in p.choices.iter().enumerate() {
        let v = p.min + i as f32;
        let response = ui
            .add_sized([76.0, 22.0], egui::Button::selectable(value == v, tr(name)))
            .on_hover_text(tr(hints[i.min(hints.len() - 1)]));
        if response.clicked() && value != v {
            params.set("speed", Some(v), &response);
            params.end_gesture();
        }
    }
}

const WOOD: Color32 = Color32::from_rgb(118, 72, 40);
const CLOTH: Color32 = Color32::from_rgb(26, 23, 21);
const BEAM: Color32 = Color32::from_rgb(255, 196, 110);
const MIC: Color32 = Color32::from_rgb(120, 220, 255);

/// The cabinet seen from above: the horn's chamber and the drum's side by side, each rotor
/// throwing its sound out in a beam, blurred as it speeds up, the microphones in front lit as
/// the beam passes them; under each, its speed between stopped and fast.
fn cabinet(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    let p = theme::current(ui);
    let get = |i: usize| meters.get(i).copied().unwrap_or(0.0);
    let (rect, painter) = display(ui, 250.0);
    let gauges = 34.0;
    let size = vec2(
        (rect.width() - 40.0).min(620.0),
        rect.height() - gauges - 40.0,
    );
    let case = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + 14.0 + size.y / 2.0),
        size,
    );
    gear::shadow(&painter, case);
    painter.rect_filled(case, 14.0, WOOD);
    gear::tile(&painter, case.shrink(6.0), Texture::Wood, 1.0, WOOD);
    painter.rect_stroke(
        case,
        14.0,
        Stroke::new(1.0, Color32::from_black_alpha(160)),
        egui::StrokeKind::Inside,
    );
    let radius = (case.height() / 2.0 - 16.0).min(case.width() / 4.0 - 18.0);
    let width = params.get("width") / 100.0;
    let balance = params.get("balance") / 100.0;
    let side = width * FRAC_PI_2;
    let rotors = [
        (
            tr("Horn"),
            meter::HORN,
            meter::HORN_SPEED,
            6.75,
            (1.0 + balance).clamp(0.0, 1.0),
        ),
        (
            tr("Drum rotor"),
            meter::DRUM,
            meter::DRUM_SPEED,
            5.9,
            (1.0 - balance).clamp(0.0, 1.0),
        ),
    ];
    for (k, (name, turn, speed, fast, level)) in rotors.into_iter().enumerate() {
        let horn = k == 0;
        let center = pos2(
            case.left() + case.width() * (0.25 + 0.5 * k as f32),
            case.center().y,
        );
        let speed = get(speed);
        // The screen's angle: pointing down (to the front) when the rotor faces the front.
        let angle = FRAC_PI_2 + get(turn) * TAU;
        let ink = Color32::from_rgb(214, 206, 192).gamma_multiply(0.45 + 0.55 * level);
        painter.circle_filled(center, radius, CLOTH);
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(2.0, Color32::from_black_alpha(140)),
        );
        // The beam, then the rotor blurred over its last few milliseconds.
        beam(
            &painter,
            center,
            radius,
            angle,
            if horn { 0.5 } else { 0.9 },
            level,
        );
        let blur = (speed / 2.0).min(1.0);
        for j in (1..=6).rev() {
            let ghost = angle - TAU * speed * 0.004 * j as f32;
            let fade = blur * 0.22 * (1.0 - j as f32 / 7.0);
            if fade > 0.01 {
                rotor(
                    &painter,
                    center,
                    radius,
                    ghost,
                    horn,
                    ink.gamma_multiply(fade),
                );
            }
        }
        rotor(&painter, center, radius, angle, horn, ink);
        // The microphones in front, lit as the beam turns toward them.
        for at in [side, -side] {
            let a = FRAC_PI_2 + at;
            let mic = center + vec2(a.cos(), a.sin()) * (radius + 10.0);
            let facing = (0.5 + 0.5 * (angle - a).cos()).powi(4) * level;
            gear::glow(
                &painter,
                mic,
                12.0,
                MIC.gamma_multiply(0.6 * facing),
                Color32::TRANSPARENT,
            );
            painter.circle_filled(mic, 4.5, Color32::from_rgb(30, 32, 36));
            painter.circle_filled(mic, 3.0, MIC.gamma_multiply(0.35 + 0.65 * facing));
        }
        gauge(
            &painter,
            Rect::from_min_size(
                pos2(center.x - radius * 1.2, rect.bottom() - gauges),
                vec2(radius * 2.4, gauges - 8.0),
            ),
            name,
            speed,
            fast,
            p.weak,
            p.accent,
        );
    }
}

/// The sound thrown out of a rotor's mouth: a cone fading away, `spread` radians wide.
fn beam(painter: &Painter, center: Pos2, radius: f32, angle: f32, spread: f32, level: f32) {
    for (reach, alpha) in [(2.6, 0.05), (2.0, 0.08), (1.5, 0.12)] {
        let points: Vec<Pos2> = std::iter::once(center)
            .chain((0..=12).map(|i| {
                let a = angle - spread / 2.0 + spread * i as f32 / 12.0;
                center + vec2(a.cos(), a.sin()) * radius * reach
            }))
            .collect();
        painter.add(Shape::convex_polygon(
            points,
            BEAM.gamma_multiply(alpha * level),
            Stroke::NONE,
        ));
    }
}

/// The horn (a flared mouth and its short counterweight) or the drum (its cylinder and the
/// scoop open on one side), pointing at `angle`.
fn rotor(painter: &Painter, center: Pos2, radius: f32, angle: f32, horn: bool, ink: Color32) {
    let dir = vec2(angle.cos(), angle.sin());
    let across = vec2(-dir.y, dir.x);
    if horn {
        // The flare, as a strip of triangles: narrow at the hub, opening at the mouth.
        let mut mesh = egui::Mesh::default();
        let steps = 16;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let along = center + dir * radius * (0.12 + 0.8 * t);
            let half = radius * (0.05 + 0.3 * t.powi(3));
            mesh.colored_vertex(along + across * half, ink.gamma_multiply(0.55 + 0.45 * t));
            mesh.colored_vertex(along - across * half, ink.gamma_multiply(0.55 + 0.45 * t));
            if i > 0 {
                let b = 2 * i as u32;
                mesh.add_triangle(b - 2, b - 1, b);
                mesh.add_triangle(b - 1, b + 1, b);
            }
        }
        painter.add(Shape::mesh(mesh));
        let mouth = center + dir * radius * 0.92;
        painter.line_segment(
            [
                mouth + across * radius * 0.35,
                mouth - across * radius * 0.35,
            ],
            Stroke::new(2.0, ink),
        );
        let back = center - dir * radius * 0.5;
        painter.line_segment(
            [center, back],
            Stroke::new(radius * 0.1, ink.gamma_multiply(0.7)),
        );
        painter.circle_filled(back, radius * 0.08, ink);
        painter.circle_filled(center, radius * 0.13, ink);
        painter.circle_filled(center, radius * 0.06, CLOTH);
    } else {
        painter.circle_filled(center, radius * 0.86, ink.gamma_multiply(0.25));
        let points: Vec<Pos2> = std::iter::once(center)
            .chain((0..=16).map(|i| {
                let a = angle - 0.9 + 1.8 * i as f32 / 16.0;
                center + vec2(a.cos(), a.sin()) * radius * 0.86
            }))
            .collect();
        painter.add(Shape::convex_polygon(
            points,
            ink.gamma_multiply(0.7),
            Stroke::new(1.5, ink),
        ));
        painter.circle_filled(center, radius * 0.12, ink);
        painter.circle_filled(center, radius * 0.05, CLOTH);
    }
}

/// A rotor's speed on a bar from stopped to a little past fast, slow and fast marked.
fn gauge(
    painter: &Painter,
    rect: Rect,
    name: &str,
    speed: f32,
    fast: f32,
    weak: Color32,
    accent: Color32,
) {
    let top = fast * 1.1;
    let bar = Rect::from_min_size(rect.left_bottom() - vec2(0.0, 6.0), vec2(rect.width(), 5.0));
    painter.rect_filled(bar, 2.5, weak.gamma_multiply(0.2));
    let fill = Rect::from_min_size(
        bar.min,
        vec2(bar.width() * (speed / top).clamp(0.0, 1.0), 5.0),
    );
    painter.rect_filled(fill, 2.5, accent);
    for mark in [0.83f32.min(fast), fast] {
        let x = bar.left() + bar.width() * mark / top;
        painter.line_segment(
            [pos2(x, bar.top() - 3.0), pos2(x, bar.bottom() + 1.0)],
            Stroke::new(1.0, weak),
        );
    }
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(11.0),
        weak,
    );
    painter.text(
        rect.right_top(),
        Align2::RIGHT_TOP,
        format!("{speed:.1} Hz"),
        FontId::monospace(11.0),
        accent,
    );
}
