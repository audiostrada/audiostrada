//! The timeline's tool bar: common edits, markers, navigation and zooms, each telling what it
//! does and its keys and mouse gestures.

use eframe::egui::{self, Color32, Painter, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use super::TimelineView;
use crate::actions::Action;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

pub(super) const HEIGHT: f32 = 26.0;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Run(Action),
    /// Scrolls to the playhead.
    ShowPlayhead,
}

#[derive(Clone, Copy)]
enum Glyph {
    Split,
    Duplicate,
    Delete,
    Heal,
    Consolidate,
    StripSilence,
    Marker,
    PreviousMarker,
    NextMarker,
    ToStart,
    ToEnd,
    Playhead,
    ZoomOut,
    ZoomIn,
    Fit,
    Shorter,
    Taller,
    WaveSmaller,
    WaveBigger,
}

const GROUPS: [&[(Tool, Glyph)]; 4] = [
    &[
        (Tool::Run(Action::Split), Glyph::Split),
        (Tool::Run(Action::Duplicate), Glyph::Duplicate),
        (Tool::Run(Action::Delete), Glyph::Delete),
        (Tool::Run(Action::Heal), Glyph::Heal),
        (Tool::Run(Action::Consolidate), Glyph::Consolidate),
        (Tool::Run(Action::StripSilence), Glyph::StripSilence),
    ],
    &[
        (Tool::Run(Action::GoToStart), Glyph::ToStart),
        (Tool::Run(Action::PreviousMarker), Glyph::PreviousMarker),
        (Tool::Run(Action::AddMarker), Glyph::Marker),
        (Tool::Run(Action::NextMarker), Glyph::NextMarker),
        (Tool::Run(Action::GoToEnd), Glyph::ToEnd),
        (Tool::ShowPlayhead, Glyph::Playhead),
    ],
    &[
        (Tool::Run(Action::ZoomOut), Glyph::ZoomOut),
        (Tool::Run(Action::ZoomIn), Glyph::ZoomIn),
        (Tool::Run(Action::ZoomToFit), Glyph::Fit),
    ],
    &[
        (Tool::Run(Action::ShorterTracks), Glyph::Shorter),
        (Tool::Run(Action::TallerTracks), Glyph::Taller),
        (Tool::Run(Action::WaveformZoomOut), Glyph::WaveSmaller),
        (Tool::Run(Action::WaveformZoomIn), Glyph::WaveBigger),
    ],
];

/// What the mouse does for a tool, if anything.
fn gesture(glyph: Glyph) -> Option<&'static str> {
    Some(match glyph {
        Glyph::ZoomIn | Glyph::ZoomOut => tr("Ctrl+wheel (or pinch), around the pointer"),
        Glyph::Shorter | Glyph::Taller => tr("Alt+wheel, around the pointer"),
        Glyph::WaveSmaller | Glyph::WaveBigger => tr("Alt+Shift+wheel"),
        Glyph::Playhead => tr(
            "Shift+wheel or a sideways wheel moves in time, the wheel moves through the tracks, \
             and the bar at the bottom can be dragged",
        ),
        Glyph::Split => tr("Shift-drag on the clips draws a range to split or delete"),
        Glyph::Duplicate => tr("Alt-drag a clip to copy it"),
        _ => return None,
    })
}

fn hint(tool: Tool, glyph: Glyph) -> String {
    let (label, keys) = match tool {
        Tool::Run(action) => (action.label(), crate::shortcut_labels(action)),
        Tool::ShowPlayhead => (tr("Show the playhead"), Vec::new()),
    };
    let mut text = label.to_owned();
    if !keys.is_empty() {
        text.push('\n');
        text.push_str(&trf!("Keys: {keys}", keys = keys.join(", ")));
    }
    if let Some(mouse) = gesture(glyph) {
        text.push('\n');
        text.push_str(&trf!("Mouse: {gesture}", gesture = mouse));
    }
    text
}

impl TimelineView {
    /// The tool bar above the ruler.
    pub(super) fn toolbar(&mut self, ui: &mut Ui, playhead: u64) {
        let p = theme::current(ui);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, p.panel);
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            Stroke::new(1.0, p.border),
        );
        let size = vec2(26.0, 22.0);
        let mut x = bar.left() + 6.0;
        for (g, group) in GROUPS.iter().enumerate() {
            if g > 0 {
                x += 6.0;
                ui.painter().line_segment(
                    [pos2(x, bar.top() + 6.0), pos2(x, bar.bottom() - 6.0)],
                    Stroke::new(1.0, p.border),
                );
                x += 7.0;
            }
            for &(tool, glyph) in *group {
                let rect = Rect::from_min_size(pos2(x, bar.center().y - size.y / 2.0), size);
                x += size.x + 2.0;
                if rect.right() > bar.right() {
                    return;
                }
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("tool", g, x as i32)),
                    Sense::click(),
                );
                let fill = if response.is_pointer_button_down_on() {
                    p.pressed
                } else if response.hovered() {
                    p.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 4.0, fill);
                let color = if response.hovered() {
                    p.text
                } else {
                    p.text.gamma_multiply(0.75)
                };
                paint(ui.painter(), rect.shrink2(vec2(6.0, 4.0)), glyph, color);
                if response.on_hover_text(hint(tool, glyph)).clicked() {
                    match tool {
                        Tool::Run(action) => self.requests.push(action),
                        Tool::ShowPlayhead => self.show_playhead(playhead),
                    }
                }
            }
        }
    }

    /// Scrolls so that the playhead is a third into the lanes.
    fn show_playhead(&mut self, playhead: u64) {
        let visible = f64::from(self.lanes_width) * self.zoom;
        self.scroll = (playhead as f64 - visible / 3.0).max(0.0);
    }
}

fn paint(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) * 0.5;
    let stroke = Stroke::new(1.4, color);
    let clip = |a: f32, b: f32| {
        Rect::from_min_max(
            pos2(c.x + a * s, c.y - 0.45 * s),
            pos2(c.x + b * s, c.y + 0.45 * s),
        )
    };
    let triangle = |dir: f32| {
        // A triangle pointing left (dir 1) or right (dir -1), on the left or right half.
        vec![
            pos2(c.x - 0.1 * s * dir, c.y - 0.55 * s),
            pos2(c.x - 0.75 * s * dir, c.y),
            pos2(c.x - 0.1 * s * dir, c.y + 0.55 * s),
        ]
    };
    let flag = |x: f32| {
        painter.line_segment([pos2(x, c.y - 0.85 * s), pos2(x, c.y + 0.85 * s)], stroke);
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x, c.y - 0.85 * s),
                pos2(x + 0.7 * s, c.y - 0.55 * s),
                pos2(x, c.y - 0.25 * s),
            ],
            color,
            Stroke::NONE,
        ));
    };
    let magnifier = |plus: bool| {
        let center = pos2(c.x - 0.15 * s, c.y - 0.15 * s);
        let r = 0.6 * s;
        painter.circle_stroke(center, r, stroke);
        painter.line_segment(
            [
                center + vec2(r, r) * 0.7,
                pos2(c.x + 0.9 * s, c.y + 0.9 * s),
            ],
            Stroke::new(2.0, color),
        );
        let d = 0.32 * s;
        painter.line_segment([center - vec2(d, 0.0), center + vec2(d, 0.0)], stroke);
        if plus {
            painter.line_segment([center - vec2(0.0, d), center + vec2(0.0, d)], stroke);
        }
    };
    let arrow = |from: egui::Pos2, to: egui::Pos2| {
        painter.line_segment([from, to], stroke);
        let dir = (to - from).normalized();
        let side = vec2(-dir.y, dir.x);
        let head = 0.3 * s;
        painter.add(Shape::convex_polygon(
            vec![
                to + dir * 0.1,
                to - dir * head + side * head * 0.8,
                to - dir * head - side * head * 0.8,
            ],
            color,
            Stroke::NONE,
        ));
    };
    let wave = |amplitude: f32| {
        let points = (0..=24)
            .map(|i| {
                let t = i as f32 / 24.0;
                let y = (t * std::f32::consts::TAU * 2.0).sin() * amplitude * s;
                pos2(c.x - s + 2.0 * s * t, c.y + y)
            })
            .collect();
        painter.add(Shape::line(points, stroke));
    };
    match glyph {
        Glyph::Split => {
            painter.rect_filled(clip(-1.0, -0.2), 2.0, color);
            painter.rect_filled(clip(0.2, 1.0), 2.0, color);
            painter.line_segment([pos2(c.x, c.y - s), pos2(c.x, c.y + s)], stroke);
        }
        Glyph::Duplicate => {
            let back = Rect::from_min_max(
                pos2(c.x - s, c.y - 0.8 * s),
                pos2(c.x + 0.4 * s, c.y + 0.3 * s),
            );
            painter.rect_stroke(back, 2.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(back.translate(vec2(0.6 * s, 0.5 * s)), 2.0, color);
        }
        Glyph::Delete => {
            painter.line_segment(
                [
                    pos2(c.x - 0.8 * s, c.y - 0.6 * s),
                    pos2(c.x + 0.8 * s, c.y - 0.6 * s),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(c.x - 0.25 * s, c.y - 0.85 * s),
                    pos2(c.x + 0.25 * s, c.y - 0.85 * s),
                ],
                stroke,
            );
            painter.add(Shape::closed_line(
                vec![
                    pos2(c.x - 0.6 * s, c.y - 0.45 * s),
                    pos2(c.x + 0.6 * s, c.y - 0.45 * s),
                    pos2(c.x + 0.45 * s, c.y + 0.9 * s),
                    pos2(c.x - 0.45 * s, c.y + 0.9 * s),
                ],
                stroke,
            ));
        }
        Glyph::Heal => {
            painter.rect_filled(clip(-1.0, 1.0), 2.0, color.gamma_multiply(0.35));
            painter.rect_stroke(clip(-1.0, 1.0), 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([pos2(c.x - 0.3 * s, c.y), pos2(c.x + 0.3 * s, c.y)], stroke);
            painter.line_segment([pos2(c.x, c.y - 0.3 * s), pos2(c.x, c.y + 0.3 * s)], stroke);
        }
        Glyph::Consolidate => {
            for a in [-1.0, -0.3, 0.4] {
                let r = Rect::from_min_max(
                    pos2(c.x + a * s, c.y - 0.85 * s),
                    pos2(c.x + (a + 0.55) * s, c.y - 0.25 * s),
                );
                painter.rect_filled(r, 1.0, color);
            }
            painter.rect_filled(
                Rect::from_min_max(pos2(c.x - s, c.y + 0.2 * s), pos2(c.x + s, c.y + 0.85 * s)),
                1.0,
                color,
            );
        }
        Glyph::StripSilence => {
            for (a, b) in [(-1.0, -0.35), (0.35, 1.0)] {
                let points = (0..=8)
                    .map(|i| {
                        let t = i as f32 / 8.0;
                        let y =
                            if i % 2 == 0 { -0.7 } else { 0.7 } * s * (1.0 - (2.0 * t - 1.0).abs());
                        pos2(c.x + (a + (b - a) * t) * s, c.y + y)
                    })
                    .collect();
                painter.add(Shape::line(points, stroke));
            }
            painter.line_segment(
                [pos2(c.x - 0.35 * s, c.y), pos2(c.x + 0.35 * s, c.y)],
                Stroke::new(1.0, color.gamma_multiply(0.5)),
            );
        }
        Glyph::Marker => flag(c.x - 0.3 * s),
        Glyph::PreviousMarker => {
            painter.add(Shape::convex_polygon(triangle(1.0), color, Stroke::NONE));
            flag(c.x + 0.2 * s);
        }
        Glyph::NextMarker => {
            painter.add(Shape::convex_polygon(triangle(-1.0), color, Stroke::NONE));
            flag(c.x - 0.75 * s);
        }
        Glyph::ToStart => theme::paint_icon(painter, rect, theme::Icon::ToStart, color),
        Glyph::ToEnd => {
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x + s * 0.5, c.y), vec2(s * 0.2, s * 1.1)),
                1.0,
                color,
            );
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - s * 0.55, c.y - s * 0.55),
                    pos2(c.x + s * 0.3, c.y),
                    pos2(c.x - s * 0.55, c.y + s * 0.55),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Playhead => {
            painter.line_segment([pos2(c.x, c.y - 0.4 * s), pos2(c.x, c.y + s)], stroke);
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - 0.5 * s, c.y - s),
                    pos2(c.x + 0.5 * s, c.y - s),
                    pos2(c.x, c.y - 0.35 * s),
                ],
                color,
                Stroke::NONE,
            ));
            arrow(
                pos2(c.x - 0.35 * s, c.y + 0.35 * s),
                pos2(c.x - s, c.y + 0.35 * s),
            );
            arrow(
                pos2(c.x + 0.35 * s, c.y + 0.35 * s),
                pos2(c.x + s, c.y + 0.35 * s),
            );
        }
        Glyph::ZoomOut => magnifier(false),
        Glyph::ZoomIn => magnifier(true),
        Glyph::Fit => {
            for x in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        pos2(c.x + x * s, c.y - 0.8 * s),
                        pos2(c.x + x * s, c.y + 0.8 * s),
                    ],
                    stroke,
                );
            }
            arrow(c, pos2(c.x - 0.75 * s, c.y));
            arrow(c, pos2(c.x + 0.75 * s, c.y));
        }
        Glyph::Shorter | Glyph::Taller => {
            for y in [-1.0, 1.0] {
                painter.line_segment(
                    [pos2(c.x - s, c.y + y * s), pos2(c.x + s, c.y + y * s)],
                    stroke,
                );
            }
            if matches!(glyph, Glyph::Taller) {
                arrow(c, pos2(c.x, c.y - 0.7 * s));
                arrow(c, pos2(c.x, c.y + 0.7 * s));
            } else {
                arrow(pos2(c.x, c.y - 0.75 * s), pos2(c.x, c.y - 0.1 * s));
                arrow(pos2(c.x, c.y + 0.75 * s), pos2(c.x, c.y + 0.1 * s));
            }
        }
        Glyph::WaveSmaller => wave(0.25),
        Glyph::WaveBigger => wave(0.8),
    }
}
