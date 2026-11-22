//! The application's windows and dialogs, all alike: a title band with the logo, what the window
//! is for and a close button; a body in sections of labelled fields; a band of actions at the
//! bottom. They open centered, floating (moved by any empty part) or modal (the rest dimmed,
//! Escape closes them).

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use crate::effect_frame::{
    Face, Glyph, field_fill, glyph_button, line_edge, paint_bottom_band, paint_header_band,
    paint_title, radius, text_button,
};
use crate::theme::{self, Palette};

/// Height of the title band and of the actions band.
const TITLE: f32 = 56.0;
const ACTIONS: f32 = 52.0;
/// Width of the labels of [`field`].
const LABEL: f32 = 200.0;

/// What [`Dialog::show`] asks to be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// The body, between the bands.
    Body,
    /// The actions band at the bottom, laid out from the left.
    Actions,
}

/// A window or a dialog being built.
pub(crate) struct Dialog<'a> {
    id: &'a str,
    title: &'a str,
    detail: String,
    width: f32,
    color: Option<Color32>,
    modal: bool,
}

impl<'a> Dialog<'a> {
    pub(crate) fn new(id: &'a str, title: &'a str) -> Self {
        Self {
            id,
            title,
            detail: String::new(),
            width: 460.0,
            color: None,
            modal: false,
        }
    }

    /// The line under the title: what the window does, or what it acts on.
    pub(crate) fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// The color of the band's top edge (the theme's accent otherwise).
    pub(crate) fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// Over everything, the rest dimmed: nothing else can be done until it closes.
    pub(crate) fn modal(mut self) -> Self {
        self.modal = true;
        self
    }

    /// Shows it while `open`: `add` draws the body, then the actions band (its buttons usually
    /// through [`buttons`]). The close button and Escape (for a modal) set `open` to false.
    pub(crate) fn show(
        self,
        ctx: &egui::Context,
        open: &mut bool,
        mut add: impl FnMut(&mut Ui, Part),
    ) {
        if !*open {
            return;
        }
        let p = theme::current_for(ctx);
        let frame = egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(radius(&p) + 2.0)
            .shadow(egui::Shadow {
                offset: [0, 10],
                blur: 28,
                spread: 0,
                color: Color32::from_black_alpha(110),
            });
        // A window of its own, except drawn inside the main one (tests, screenshots).
        if !ctx.embed_viewports() {
            if self.window(ctx, &p, &mut add) {
                *open = false;
            }
            return;
        }
        let width = self.width.min(ctx.content_rect().width() - 32.0);
        let mut closed = false;
        let mut content = |ui: &mut Ui, closed: &mut bool| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            if self.header(ui, &p, width).0 {
                *closed = true;
            }
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(20, 16))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    ui.set_width(width - 40.0);
                    add(ui, Part::Body);
                });
            footer(ui, &p, width, |ui| add(ui, Part::Actions));
        };
        if self.modal {
            let modal = egui::Modal::new(egui::Id::unique(("dialog", self.id)))
                .frame(frame)
                .show(ctx, |ui| content(ui, &mut closed));
            if modal.should_close() {
                closed = true;
            }
        } else {
            egui::Window::new(self.title)
                .id(egui::Id::unique(("dialog", self.id)))
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .frame(frame)
                .pivot(Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| content(ui, &mut closed));
        }
        if closed {
            *open = false;
        }
    }

    /// Shows it in a window of its own, as the other floating windows: on any screen, in front
    /// of them when opened, the transport's shortcuts working there. It opens centered on the
    /// window it comes from and fits its content; it is moved by its title band. A modal one
    /// dims that window and takes its clicks until it closes. True once closed.
    fn window(
        &self,
        ctx: &egui::Context,
        p: &Palette,
        add: &mut impl FnMut(&mut Ui, Part),
    ) -> bool {
        let id = egui::ViewportId::from_hash_of(("dialog", self.id));
        let size_key = egui::Id::unique(("dialog-size", self.id));
        let place_key = egui::Id::unique(("dialog-place", self.id));
        let width = self.width;
        let size = ctx
            .data(|d| d.get_temp::<egui::Vec2>(size_key))
            .unwrap_or(vec2(width, 320.0));
        // Chosen once per opening: a position that changes between frames moves the window.
        let place = ctx
            .data(|d| d.get_temp::<egui::Pos2>(place_key))
            .or_else(|| {
                let around = ctx.input(|i| i.viewport().outer_rect).or_else(|| {
                    ctx.input_for(egui::ViewportId::ROOT, |i| i.viewport().outer_rect)
                })?;
                let place = (around.center() - size / 2.0).max(around.min);
                ctx.data_mut(|d| d.insert_temp(place_key, place));
                // Linux: a window opened over a floating one stayed behind it, unfocused.
                if cfg!(all(unix, not(target_os = "macos"))) {
                    ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
                }
                Some(place)
            });
        let mut builder = egui::ViewportBuilder::default()
            .with_title(format!("{} — AudioStrada", self.title))
            .with_inner_size(size)
            .with_decorations(false)
            .with_resizable(false);
        if let Some(place) = place {
            builder = builder.with_position(place);
        }
        let builder = crate::floating_builder(ctx, id, crate::Floating::Dialog, builder);
        let modal = self.modal;
        let closed = ctx.show_viewport_immediate(id, builder, |ui, _| {
            let ctx = ui.ctx().clone();
            let mut closed = ctx.input(|i| i.viewport().close_requested());
            // Before the shortcuts, which may bind it.
            if modal && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                closed = true;
            }
            crate::window_shortcuts(&ctx);
            let frame = egui::Frame::new()
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border));
            egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (close, band) = self.header(ui, p, width);
                closed |= close;
                if band.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(20, 16))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                        ui.set_width(width - 40.0);
                        add(ui, Part::Body);
                    });
                footer(ui, p, width, |ui| add(ui, Part::Actions));
                // The window follows its content.
                let wanted = vec2(width, ui.min_rect().height().ceil());
                let inner = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
                if inner.is_some_and(|s| (s - wanted).abs().max_elem() > 1.0)
                    && ctx.data(|d| d.get_temp::<egui::Vec2>(size_key)) != Some(wanted)
                {
                    ctx.data_mut(|d| d.insert_temp(size_key, wanted));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(wanted));
                }
            });
            closed
        });
        if modal && !closed {
            self.backdrop(ctx, id);
        }
        if closed {
            ctx.data_mut(|d| d.remove::<egui::Pos2>(place_key));
            ctx.request_repaint();
        }
        closed
    }

    /// Dims the window a modal dialog comes from and takes its clicks, which bring the dialog
    /// to the front instead.
    fn backdrop(&self, ctx: &egui::Context, dialog: egui::ViewportId) {
        let rect = ctx.content_rect();
        egui::Area::new(egui::Id::unique(("dialog-backdrop", self.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha(90));
                if ui.allocate_rect(rect, Sense::click()).clicked() {
                    ctx.send_viewport_cmd_to(dialog, egui::ViewportCommand::Focus);
                }
            });
    }

    /// The title band; true when its close button was clicked, and the band's response (a
    /// drag on it moves a dialog's window).
    fn header(&self, ui: &mut Ui, p: &Palette, width: f32) -> (bool, Response) {
        let (rect, band) = ui.allocate_exact_size(vec2(width, TITLE), Sense::click_and_drag());
        let painter = ui.painter().clone();
        let bar = paint_header_band(&painter, p, rect, self.color.unwrap_or(p.accent));
        let (weak, detail) = (p.weak, self.detail.clone());
        paint_title(
            &painter,
            p,
            bar,
            width - 110.0,
            self.title,
            &|painter, at| {
                painter.text(
                    at,
                    Align2::LEFT_CENTER,
                    &detail,
                    FontId::proportional(11.5),
                    weak,
                );
            },
            false,
        );
        let close =
            Rect::from_center_size(pos2(bar.right() - 24.0, bar.center().y), vec2(28.0, 28.0));
        let closed = glyph_button(
            ui,
            close,
            ui.scope_id().with("dialog-close"),
            Glyph::Close,
            None,
            true,
        )
        .on_hover_text(crate::i18n::tr("Close"))
        .clicked();
        (closed, band)
    }
}

/// The actions band at the bottom, laid out from the left; [`buttons`] puts buttons at its right.
fn footer(ui: &mut Ui, p: &Palette, width: f32, actions: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, ACTIONS), Sense::hover());
    paint_bottom_band(ui.painter(), p, rect);
    let mut band = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(16.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    band.spacing_mut().item_spacing.x = 8.0;
    actions(&mut band);
}

/// Buttons at the right of the actions band, the first one given rightmost.
pub(crate) fn buttons<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(Layout::right_to_left(Align::Center), add)
        .inner
}

/// The window's main action, in the accent color; disabled, it only shows its hint.
pub(crate) fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let p = theme::current(ui);
    let face = if enabled {
        Face::Lit(p.accent)
    } else {
        Face::Plain
    };
    let response = chip(ui, text, face);
    if enabled {
        response
    } else {
        response.on_hover_cursor(egui::CursorIcon::NotAllowed)
    }
}

/// Another action.
pub(crate) fn secondary(ui: &mut Ui, text: &str) -> Response {
    chip(ui, text, Face::Plain)
}

/// A button of the actions band, as wide as its text.
fn chip(ui: &mut Ui, text: &str, face: Face) -> Response {
    let p = theme::current(ui);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
        .size()
        .x
        + 28.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width.max(80.0), 30.0), Sense::hover());
    let id = ui.scope_id().with(("dialog-button", text));
    text_button(ui, rect, id, text, face, radius(&p) as u8)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A section of the body: its title in small capitals, then its content.
pub(crate) fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::current(ui);
    ui.add_space(4.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(10.5)
            .strong()
            .color(p.weak),
    );
    ui.add_space(2.0);
    let inner = add(ui);
    ui.add_space(8.0);
    inner
}

/// A labelled field: its name in a column at the left, its control at the right.
pub(crate) fn field<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::current(ui);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL, 24.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            p.text,
        );
        add(ui)
    })
    .inner
}

/// A short explanation, under what it explains.
pub(crate) fn hint(ui: &mut Ui, text: &str) {
    let p = theme::current(ui);
    ui.label(RichText::new(text).size(11.0).color(p.weak));
}

/// Choices side by side, the one chosen lit; true when another was chosen.
pub(crate) fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    choices: &[(T, &str, bool)],
) -> bool {
    let p = theme::current(ui);
    let mut changed = false;
    // Its own place among the window's ids, by its choices.
    let salt: Vec<&str> = choices.iter().map(|c| c.1).collect();
    ui.push_id(salt, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let count = choices.len();
            for (k, &(choice, text, enabled)) in choices.iter().enumerate() {
                let on = *value == choice;
                let width = ui
                    .painter()
                    .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
                    .size()
                    .x
                    + 24.0;
                let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
                let r = radius(&p) as u8;
                let corners = egui::CornerRadius {
                    nw: if k == 0 { r } else { 0 },
                    sw: if k == 0 { r } else { 0 },
                    ne: if k + 1 == count { r } else { 0 },
                    se: if k + 1 == count { r } else { 0 },
                };
                let face = if on {
                    Face::Lit(theme::mix(p.surface, p.accent, 0.7))
                } else {
                    Face::Plain
                };
                let id = ui.scope_id().with(("segment", k));
                if enabled {
                    if text_button(ui, rect, id, text, face, corners).clicked() && !on {
                        *value = choice;
                        changed = true;
                    }
                } else {
                    let mut painter = ui.painter().clone();
                    painter.multiply_opacity(0.4);
                    painter.rect(
                        rect,
                        corners,
                        p.surface,
                        Stroke::new(1.0, line_edge(&p)),
                        StrokeKind::Inside,
                    );
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        text,
                        theme::strong(12.0),
                        p.text,
                    );
                }
            }
        })
    });
    changed
}

/// A framed area inside the body, for a list or a preview.
pub(crate) fn well<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(field_fill(&p))
        .stroke(Stroke::new(1.0, line_edge(&p)))
        .corner_radius(radius(&p))
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}
