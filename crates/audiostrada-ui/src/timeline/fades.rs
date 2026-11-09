//! Fades: the pointer drawn as the fade's curve, and the shapes and lengths offered by its menu.

use audiostrada_core::model::FadeShape;
use audiostrada_core::{Clip, ClipId, Project, edit};
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use super::TimelineView;
use crate::document::Document;
use crate::i18n::tr;
use crate::theme;

/// A fade's curve in `rect`: rising for a fade-in, falling for a fade-out.
pub(super) fn paint_curve(
    painter: &Painter,
    rect: Rect,
    shape: FadeShape,
    fade_in: bool,
    color: Color32,
) {
    let points: Vec<Pos2> = (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let gain = shape.gain(if fade_in { t } else { 1.0 - t });
            pos2(
                rect.left() + rect.width() * t,
                rect.bottom() - rect.height() * gain,
            )
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    let fill = color.gamma_multiply(0.18);
    for pair in points.windows(2) {
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pair[0], fill);
        mesh.colored_vertex(pair[1], fill);
        mesh.colored_vertex(pos2(pair[1].x, rect.bottom()), fill);
        mesh.colored_vertex(pos2(pair[0].x, rect.bottom()), fill);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(points, Stroke::new(1.6, color)));
}

/// The pointer over a fade handle: the fade's curve, centered on the pointer.
pub(super) fn paint_cursor(ctx: &egui::Context, at: Pos2, shape: FadeShape, fade_in: bool) {
    let p = theme::current_for(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("fade-cursor"),
    ));
    let rect = Rect::from_center_size(at, vec2(22.0, 18.0));
    painter.rect_filled(rect, 3.0, p.surface.gamma_multiply(0.92));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, p.text.gamma_multiply(0.7)),
        egui::StrokeKind::Inside,
    );
    paint_curve(
        &painter,
        rect.shrink2(vec2(4.0, 3.0)),
        shape,
        fade_in,
        p.text,
    );
}

/// Fade lengths offered, with their name.
fn lengths(project: &Project, clip: &Clip) -> Vec<(String, u64)> {
    let ms = |m: f64| super::menus::ms(project, m);
    vec![
        (tr("None").to_owned(), 0),
        ("10 ms".to_owned(), ms(10.0)),
        ("50 ms".to_owned(), ms(50.0)),
        ("250 ms".to_owned(), ms(250.0)),
        (
            tr("1 beat").to_owned(),
            crate::transport::step_length(project, clip.start, Some(1.0)),
        ),
        (
            tr("1 bar").to_owned(),
            crate::transport::step_length(project, clip.start, None),
        ),
    ]
}

impl TimelineView {
    /// The shapes, drawn, and the lengths of a fade, for `ids` (`clip` showing the current ones).
    pub(super) fn fade_options(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        clip: &Clip,
        ids: &[ClipId],
        fade_in: bool,
    ) {
        let choices = lengths(doc.project(), clip);
        let p = theme::current(ui);
        let current = if fade_in {
            clip.fade_shape
        } else {
            clip.out_shape()
        };
        ui.label(egui::RichText::new(tr("Shape")).weak().size(11.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for shape in FadeShape::ALL {
                let (rect, response) = ui.allocate_exact_size(vec2(54.0, 40.0), Sense::click());
                let chosen = shape == current;
                let fill = if chosen {
                    p.accent.gamma_multiply(0.25)
                } else if response.hovered() {
                    p.hover
                } else {
                    p.surface
                };
                ui.painter().rect_filled(rect, 4.0, fill);
                if chosen {
                    ui.painter().rect_stroke(
                        rect,
                        4.0,
                        Stroke::new(1.0, p.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                paint_curve(
                    ui.painter(),
                    rect.shrink2(vec2(8.0, 7.0)),
                    shape,
                    fade_in,
                    if chosen { p.accent } else { p.text },
                );
                if response.on_hover_text(tr(shape.name())).clicked() {
                    doc.history_mut().edit(tr("Fade shape"), |p| {
                        for id in ids {
                            if let Some(m) = edit::clip_mut(p, *id) {
                                if fade_in {
                                    let out = m.out_shape();
                                    m.fade_shape = shape;
                                    m.fade_out_shape = Some(out);
                                } else {
                                    m.fade_out_shape = Some(shape);
                                }
                            }
                        }
                    });
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tr("Length")).weak().size(11.0));
        ui.horizontal_wrapped(|ui| {
            for (text, length) in choices {
                let label = if fade_in {
                    tr("Fade in")
                } else {
                    tr("Fade out")
                };
                if ui.button(text).clicked() {
                    doc.history_mut().edit(label, |p| {
                        for id in ids {
                            if let Some(m) = edit::clip(p, *id).cloned() {
                                let (i, o) = if fade_in {
                                    (length, m.fade_out)
                                } else {
                                    (m.fade_in, length)
                                };
                                edit::set_fades(p, *id, i, o);
                            }
                        }
                    });
                    ui.close();
                }
            }
        });
    }

    /// The menu of a crossfade: its shape, drawn as both curves crossing, and its removal.
    pub(super) fn crossfade_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        left: ClipId,
        right: ClipId,
    ) {
        let Some(current) = edit::clip(doc.project(), right).map(|c| c.fade_shape) else {
            ui.close();
            return;
        };
        let p = theme::current(ui);
        ui.label(egui::RichText::new(tr("Crossfade")).strong());
        ui.label(egui::RichText::new(tr("Shape")).weak().size(11.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for shape in FadeShape::ALL {
                let (rect, response) = ui.allocate_exact_size(vec2(54.0, 40.0), Sense::click());
                let chosen = shape == current;
                let fill = if chosen {
                    p.accent.gamma_multiply(0.25)
                } else if response.hovered() {
                    p.hover
                } else {
                    p.surface
                };
                ui.painter().rect_filled(rect, 4.0, fill);
                let ink = if chosen { p.accent } else { p.text };
                let area = rect.shrink2(vec2(8.0, 7.0));
                for fade_in in [false, true] {
                    let points: Vec<Pos2> = (0..=20)
                        .map(|i| {
                            let t = i as f32 / 20.0;
                            let gain = shape.gain(if fade_in { t } else { 1.0 - t });
                            pos2(
                                area.left() + area.width() * t,
                                area.bottom() - area.height() * gain,
                            )
                        })
                        .collect();
                    ui.painter().add(Shape::line(points, Stroke::new(1.5, ink)));
                }
                if response.on_hover_text(tr(shape.name())).clicked() {
                    doc.history_mut().edit(tr("Crossfade shape"), |p| {
                        edit::set_crossfade_shape(p, left, right, shape)
                    });
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button(tr("Remove the crossfade")).clicked() {
            doc.history_mut()
                .edit(tr("Crossfade"), |p| edit::set_crossfade(p, left, right, 0));
            ui.close();
        }
    }

    /// The menu of a fade, right-clicked on it.
    pub(super) fn fade_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        clip: ClipId,
        fade_in: bool,
    ) {
        let Some(c) = edit::clip(doc.project(), clip).cloned() else {
            ui.close();
            return;
        };
        let ids = self.menu_targets(doc.project(), clip);
        ui.label(
            egui::RichText::new(if fade_in {
                tr("Fade in")
            } else {
                tr("Fade out")
            })
            .strong(),
        );
        self.fade_options(ui, doc, &c, &ids, fade_in);
    }
}
