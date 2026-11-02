//! Drawn controls of the console and the effect editors: fader, knob, meter.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    pos2, vec2,
};

use crate::theme;

/// Fader law: dB at fractions of the travel, piecewise linear between them.
const LAW: [(f32, f32); 9] = [
    (0.0, -60.0),
    (0.16, -40.0),
    (0.26, -30.0),
    (0.38, -20.0),
    (0.52, -12.0),
    (0.64, -6.0),
    (0.78, 0.0),
    (0.9, 6.0),
    (1.0, 12.0),
];

pub fn fader_position(db: f32) -> f32 {
    let db = db.clamp(LAW[0].1, LAW[LAW.len() - 1].1);
    for w in LAW.windows(2) {
        let ((p0, d0), (p1, d1)) = (w[0], w[1]);
        if db <= d1 {
            return p0 + (db - d0) / (d1 - d0) * (p1 - p0);
        }
    }
    1.0
}

pub fn fader_db(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    for w in LAW.windows(2) {
        let ((p0, d0), (p1, d1)) = (w[0], w[1]);
        if position <= p1 {
            return d0 + (position - p0) / (p1 - p0) * (d1 - d0);
        }
    }
    LAW[LAW.len() - 1].1
}

/// Vertical fader with a dB scale; drag (Shift for fine), wheel, double-click for 0 dB.
pub fn fader(ui: &mut Ui, db: f32, height: f32, show_scale: bool) -> (Option<f32>, Response) {
    let width = if show_scale { 40.0 } else { 22.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let p = theme::current(ui);
    let track = Rect::from_center_size(
        pos2(rect.left() + 11.0, rect.center().y),
        vec2(4.0, height - 16.0),
    );
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, p.border);
    let y_of = |db: f32| track.bottom() - fader_position(db) * track.height();
    if show_scale {
        // Most useful marks first; a mark too close to one already drawn is left out.
        let mut drawn: Vec<f32> = Vec::new();
        for mark in [0.0, -60.0, 12.0, -20.0, 6.0, -6.0, -12.0, -40.0, -30.0] {
            let y = y_of(mark);
            if drawn.iter().any(|d| (d - y).abs() < 10.0) {
                continue;
            }
            drawn.push(y);
            painter.line_segment(
                [pos2(track.right() + 5.0, y), pos2(track.right() + 9.0, y)],
                Stroke::new(1.0, p.weak.gamma_multiply(0.6)),
            );
            let label = if mark <= -60.0 {
                "∞".to_owned()
            } else {
                format!("{}", mark.abs() as i32)
            };
            painter.text(
                pos2(rect.right(), y),
                Align2::RIGHT_CENTER,
                label,
                FontId::proportional(8.5),
                if mark == 0.0 { p.text } else { p.weak },
            );
        }
    }
    let mut value = db;
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        let dy = response.drag_delta().y;
        let step = if fine { 0.15 } else { 1.0 };
        let position = fader_position(value) - dy / track.height() * step;
        value = fader_db(position);
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            value = (value + wheel.signum() * if fine { 0.1 } else { 0.5 }).clamp(-60.0, 12.0);
        }
    }
    if response.double_clicked() {
        value = 0.0;
    }
    // Cap.
    let y = y_of(value);
    let cap = Rect::from_center_size(pos2(track.center().x, y), vec2(20.0, 30.0));
    let active = response.dragged() || response.hovered();
    let radius = f32::from(p.look.radius.min(4));
    painter.rect_filled(
        cap.translate(vec2(0.0, 1.5)),
        radius,
        Color32::from_black_alpha(90),
    );
    match p.look.cap {
        theme::CapStyle::Modern => {
            let shade = if active { p.pressed } else { p.hover };
            theme::gradient_rect(painter, cap.shrink(0.5), shade, shade.gamma_multiply(1.35));
            painter.rect_stroke(cap, radius, Stroke::new(1.0, p.border), StrokeKind::Inside);
            painter.line_segment(
                [pos2(cap.left() + 3.0, y), pos2(cap.right() - 3.0, y)],
                Stroke::new(1.5, p.text),
            );
        }
        theme::CapStyle::Classic => {
            // A light console cap with grooves and a black index line.
            let light = if active { 222 } else { 200 };
            theme::gradient_rect(
                painter,
                cap.shrink(0.5),
                Color32::from_gray(light - 50),
                Color32::from_gray(light),
            );
            painter.rect_stroke(cap, radius, Stroke::new(1.0, p.border), StrokeKind::Inside);
            for dy in [-10.0, -7.0, -4.0, 4.0, 7.0, 10.0] {
                painter.line_segment(
                    [
                        pos2(cap.left() + 2.0, y + dy),
                        pos2(cap.right() - 2.0, y + dy),
                    ],
                    Stroke::new(1.0, Color32::from_black_alpha(70)),
                );
            }
            painter.line_segment(
                [pos2(cap.left() + 1.0, y), pos2(cap.right() - 1.0, y)],
                Stroke::new(2.0, Color32::from_gray(20)),
            );
        }
    }
    let changed = (value != db).then_some(value);
    (changed, response)
}

/// Peak and hold of a stereo meter, in dBFS.
#[derive(Clone, Copy, Debug)]
pub struct MeterState {
    pub level: [f32; 2],
    pub hold: [f32; 2],
    pub held_for: [f32; 2],
    pub clipped: bool,
}

impl Default for MeterState {
    fn default() -> Self {
        Self {
            level: [METER_FLOOR; 2],
            hold: [METER_FLOOR; 2],
            held_for: [0.0; 2],
            clipped: false,
        }
    }
}

pub const METER_FLOOR: f32 = -60.0;
const METER_TOP: f32 = 6.0;

impl MeterState {
    /// Takes new peaks: falls at `fall_db` per call, holds peaks for 1.5 s.
    pub fn push(&mut self, peaks: [f32; 2], fall_db: f32, dt: f32) {
        for (ch, peak) in peaks.into_iter().enumerate() {
            let peak = peak.max(METER_FLOOR);
            self.level[ch] = peak.max(self.level[ch] - fall_db);
            self.held_for[ch] += dt;
            if peak >= self.hold[ch] || self.held_for[ch] > 1.5 {
                self.hold[ch] = peak.max(self.level[ch]);
                self.held_for[ch] = 0.0;
            }
            self.clipped |= peak > -0.05;
        }
    }
}

pub(crate) fn meter_fraction(db: f32) -> f32 {
    ((db - METER_FLOOR) / (METER_TOP - METER_FLOOR)).clamp(0.0, 1.0)
}

/// Stereo peak meter with a clip light on top; clicking the light clears it.
pub fn meter(ui: &mut Ui, state: &mut MeterState, height: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(14.0, height), Sense::click());
    let p = theme::current(ui);
    let painter = ui.painter();
    let clip = Rect::from_min_size(rect.min, vec2(rect.width(), 5.0));
    painter.rect_filled(
        clip,
        1.0,
        if state.clipped { theme::RECORD } else { p.base },
    );
    let bars = Rect::from_min_max(pos2(rect.left(), clip.bottom() + 2.0), rect.max);
    for ch in 0..2 {
        let x = bars.left() + ch as f32 * 7.0;
        let bar = Rect::from_min_max(pos2(x, bars.top()), pos2(x + 6.0, bars.bottom()));
        painter.rect_filled(bar, 1.0, p.base);
        let f = meter_fraction(state.level[ch]);
        let lit = Rect::from_min_max(pos2(x, bar.bottom() - bar.height() * f), bar.max);
        match p.look.meter {
            theme::MeterStyle::Segments => paint_meter_lights(painter, bar, state.level[ch]),
            theme::MeterStyle::Glow if f > 0.0 => {
                let halo = meter_color(state.level[ch]).gamma_multiply(0.22);
                painter.rect_filled(lit.expand2(vec2(1.5, 2.0)), 3.0, halo);
                paint_meter_bar(painter, bar, lit);
            }
            _ if f > 0.0 => paint_meter_bar(painter, bar, lit),
            _ => {}
        }
        let hold = meter_fraction(state.hold[ch]);
        if hold > 0.0 {
            let y = bar.bottom() - bar.height() * hold;
            painter.line_segment(
                [pos2(x, y), pos2(x + 6.0, y)],
                Stroke::new(1.5, meter_color(state.hold[ch])),
            );
        }
    }
    if response.clicked() {
        state.clipped = false;
    }
    response.on_hover_text(crate::trf!(
        "Peak {left} / {right} dBFS. Click to clear the clip light",
        left = format!("{:.1}", state.hold[0]),
        right = format!("{:.1}", state.hold[1])
    ))
}

fn meter_color(db: f32) -> Color32 {
    if db > -0.1 {
        theme::RECORD
    } else if db > -9.0 {
        Color32::from_rgb(236, 200, 70)
    } else {
        Color32::from_rgb(80, 200, 120)
    }
}

/// Green to yellow to red along the full bar, showing only the lit part.
pub(crate) fn paint_meter_bar(painter: &egui::Painter, bar: Rect, lit: Rect) {
    let y_of = |db: f32| bar.bottom() - bar.height() * meter_fraction(db);
    let stops = [
        (METER_FLOOR, Color32::from_rgb(40, 150, 90)),
        (-18.0, Color32::from_rgb(80, 200, 120)),
        (-9.0, Color32::from_rgb(200, 210, 80)),
        (-3.0, Color32::from_rgb(240, 170, 60)),
        (0.0, Color32::from_rgb(240, 80, 70)),
        (METER_TOP, Color32::from_rgb(240, 80, 70)),
    ];
    let mut mesh = egui::Mesh::default();
    for w in stops.windows(2) {
        let (y0, y1) = (y_of(w[0].0), y_of(w[1].0));
        let (top, bottom) = (y1.max(lit.top()), y0.min(lit.bottom()));
        if top >= bottom {
            continue;
        }
        let at = |y: f32| {
            let t = ((y0 - y) / (y0 - y1).max(1e-3)).clamp(0.0, 1.0);
            lerp_color(w[0].1, w[1].1, t)
        };
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(lit.left(), bottom), at(bottom));
        mesh.colored_vertex(pos2(lit.right(), bottom), at(bottom));
        mesh.colored_vertex(pos2(lit.right(), top), at(top));
        mesh.colored_vertex(pos2(lit.left(), top), at(top));
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(Shape::mesh(mesh));
}

/// Separate lights along the bar, lit up to `db`, the others dimmed.
pub(crate) fn paint_meter_lights(painter: &egui::Painter, bar: Rect, db: f32) {
    const LIGHT: f32 = 3.0;
    const GAP: f32 = 1.0;
    let mut bottom = bar.bottom();
    while bottom - LIGHT >= bar.top() {
        let middle = 1.0 - (bottom - LIGHT / 2.0 - bar.top()) / bar.height();
        let at = METER_FLOOR + middle * (METER_TOP - METER_FLOOR);
        let color = meter_color(at);
        let color = if at <= db {
            color
        } else {
            color.gamma_multiply(0.14)
        };
        painter.rect_filled(
            Rect::from_min_max(pos2(bar.left(), bottom - LIGHT), pos2(bar.right(), bottom)),
            0.0,
            color,
        );
        bottom -= LIGHT + GAP;
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Rotary knob over `min..=max`; vertical drag (Shift for fine), wheel, double-click resets.
/// `bipolar` draws the arc from the center (pan, gains around zero).
/// Horizontal balance bar from -1 (left) to 1 (right): drag along it (Shift for fine steps,
/// the center holds the pointer a little), wheel for 1 % steps, double-click to center.
/// `label` is drawn inside it.
pub fn pan_slider(ui: &mut Ui, value: f32, width: f32, label: &str) -> (Option<f32>, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 16.0), Sense::click_and_drag());
    let p = theme::current(ui);
    let fine = ui.input(|i| i.modifiers.shift);
    let mut v = value;
    if response.dragged() {
        let d = response.drag_delta().x / rect.width() * 2.0 * if fine { 0.1 } else { 1.0 };
        v = (v + d).clamp(-1.0, 1.0);
        // A coarse drag crossing the center rests on it.
        if !fine && value != 0.0 && v.signum() != value.signum() {
            v = 0.0;
        }
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            v = ((v * 100.0).round() + wheel.signum()).clamp(-100.0, 100.0) / 100.0;
        }
    }
    if response.double_clicked() {
        v = 0.0;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, if response.hovered() { p.hover } else { p.base });
    let x = |v: f32| rect.center().x + v * (rect.width() / 2.0 - 2.0);
    if v != 0.0 {
        let (a, b) = (x(0.0).min(x(v)), x(0.0).max(x(v)));
        painter.rect_filled(
            Rect::from_min_max(pos2(a, rect.top() + 2.0), pos2(b, rect.bottom() - 2.0)),
            2.0,
            p.accent.gamma_multiply(0.55),
        );
    }
    painter.line_segment(
        [pos2(x(0.0), rect.top()), pos2(x(0.0), rect.bottom())],
        Stroke::new(1.0, p.border),
    );
    painter.line_segment(
        [
            pos2(x(v), rect.top() + 1.0),
            pos2(x(v), rect.bottom() - 1.0),
        ],
        Stroke::new(2.0, p.text),
    );
    // On the side away from the cursor, so that the text stays readable.
    let (anchor, align) = if v > 0.0 {
        (rect.left_center() + vec2(4.0, 0.0), Align2::LEFT_CENTER)
    } else {
        (rect.right_center() - vec2(4.0, 0.0), Align2::RIGHT_CENTER)
    };
    painter.text(anchor, align, label, FontId::monospace(10.0), p.text);
    ((v != value).then_some(v), response)
}

pub fn knob(
    ui: &mut Ui,
    value: f32,
    (min, max): (f32, f32),
    default: f32,
    diameter: f32,
    bipolar: bool,
    tint: Color32,
) -> (Option<f32>, Response) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(diameter, diameter), Sense::click_and_drag());
    let p = theme::current(ui);
    let mut v = value;
    let fine = ui.input(|i| i.modifiers.shift);
    let span = max - min;
    if response.dragged() {
        let d = -response.drag_delta().y / 150.0 * if fine { 0.15 } else { 1.0 };
        v = (v + d * span).clamp(min, max);
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            v = (v + wheel.signum() * span * if fine { 0.002 } else { 0.02 }).clamp(min, max);
        }
    }
    if response.double_clicked() {
        v = default;
    }
    paint_knob(
        ui.painter(),
        rect,
        (v - min) / span,
        bipolar,
        tint,
        &p,
        response.hovered(),
    );
    ((v != value).then_some(v), response)
}

pub fn paint_knob(
    painter: &egui::Painter,
    rect: Rect,
    fraction: f32,
    bipolar: bool,
    tint: Color32,
    p: &theme::Palette,
    hovered: bool,
) {
    use std::f32::consts::PI;
    let c = rect.center();
    let r = rect.width() * 0.5;
    let start = PI * 0.75;
    let sweep = PI * 1.5;
    let angle = |f: f32| start + sweep * f.clamp(0.0, 1.0);
    let arc = |from: f32, to: f32, radius: f32| -> Vec<Pos2> {
        let n = ((to - from).abs() / 0.12).ceil().max(1.0) as usize;
        (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                pos2(c.x + radius * a.cos(), c.y + radius * a.sin())
            })
            .collect()
    };
    let ring = r - 2.0;
    painter.add(Shape::line(
        arc(angle(0.0), angle(1.0), ring),
        Stroke::new(3.0, p.base),
    ));
    let from = if bipolar { angle(0.5) } else { angle(0.0) };
    let to = angle(fraction);
    if (to - from).abs() > 0.01 {
        painter.add(Shape::line(
            arc(from.min(to), from.max(to), ring),
            Stroke::new(3.0, tint),
        ));
    }
    let body = r - 6.0;
    painter.circle_filled(c, body, if hovered { p.pressed } else { p.hover });
    painter.circle_stroke(c, body, Stroke::new(1.0, p.border));
    let a = angle(fraction);
    painter.line_segment(
        [
            pos2(c.x + body * 0.35 * a.cos(), c.y + body * 0.35 * a.sin()),
            pos2(c.x + body * 0.9 * a.cos(), c.y + body * 0.9 * a.sin()),
        ],
        Stroke::new(2.0, p.text),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_law_round_trips() {
        for db in [-60.0, -45.0, -20.0, -3.0, 0.0, 4.5, 12.0] {
            assert!((fader_db(fader_position(db)) - db).abs() < 1e-3, "{db}");
        }
        assert_eq!(fader_position(0.0), 0.78);
        assert_eq!(fader_db(1.5), 12.0);
    }

    #[test]
    fn meter_holds_and_clips() {
        let mut m = MeterState::default();
        m.push([-6.0, -12.0], 1.0, 0.1);
        assert_eq!(m.hold, [-6.0, -12.0]);
        m.push([-30.0, -30.0], 1.0, 0.1);
        assert_eq!(m.level, [-7.0, -13.0]);
        assert_eq!(m.hold[0], -6.0, "held");
        for _ in 0..20 {
            m.push([-30.0, -30.0], 1.0, 0.1);
        }
        assert!(m.hold[0] < -6.0, "released after 1.5 s");
        assert!(!m.clipped);
        m.push([0.0, -1.0], 1.0, 0.1);
        assert!(m.clipped);
    }
}
