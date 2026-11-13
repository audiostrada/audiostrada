//! What the large editors (automation, notes) share: a window over the main one, the bar of
//! tools above it, the help of the tool in hand on a pill, the zoom tool and the buttons of the
//! view. Both editors put the same things at the same places and answer to the same keys.

use eframe::egui::{
    self, Align, CursorIcon, FontId, Layout, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};

use crate::i18n::tr;
pub(crate) use crate::timeline::toolbar::{Glyph, button, separator};
use crate::trf;

/// Height of the bar of tools.
pub(crate) const TOOLS: f32 = 36.0;

/// The editors' windows, by the id their state is kept under.
pub(crate) const AUTOMATION: &str = "automation-editor";
pub(crate) const NOTES: &str = "note-editor";
pub(crate) const AUDIO: &str = "audio-editor";

/// Whether the editor `id` fills the main window, kept while the application runs.
pub(crate) fn full_screen(ctx: &egui::Context, id: &str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(full_id(id)))
        .unwrap_or(false)
}

fn set_full_screen(ctx: &egui::Context, id: &str, on: bool) {
    ctx.data_mut(|d| d.insert_temp(full_id(id), on));
}

fn full_id(id: &str) -> egui::Id {
    egui::Id::unique((id, "full-screen"))
}

fn escape_id(id: &str) -> egui::Id {
    egui::Id::unique((id, "escape"))
}

/// Escape, while the editor `id` is open, is its own: taken before the shortcuts, which would
/// deselect what it shows. Its window reads it with [`take_escape`].
pub(crate) fn keep_escape(ctx: &egui::Context, id: &str) {
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ctx.data_mut(|d| d.insert_temp(escape_id(id), true));
    }
}

/// Whether Escape was pressed for the editor `id` this frame.
pub(crate) fn take_escape(ctx: &egui::Context, id: &str) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(escape_id(id)))
        .unwrap_or(false)
}

/// A tool of the editors; each editor offers some of them, in this order in its bar (the zoom
/// with the view's buttons), always on the same key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Adds by a click, draws by a drag; what is grabbed moves.
    #[default]
    Pencil,
    /// Selects, by a click or a rectangle; what is selected moves together.
    Select,
    /// Paints along the grid.
    Brush,
    /// Cuts where it clicks.
    Scissors,
    /// Removes what it clicks or passes over.
    Erase,
    /// Drag across a span to fill the view with it; click zooms in, Alt+click out.
    Zoom,
}

impl Tool {
    pub fn key(self) -> egui::Key {
        match self {
            Tool::Pencil => egui::Key::P,
            Tool::Select => egui::Key::S,
            Tool::Brush => egui::Key::B,
            Tool::Scissors => egui::Key::C,
            Tool::Erase => egui::Key::E,
            Tool::Zoom => egui::Key::Z,
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Tool::Pencil => Glyph::Pencil,
            Tool::Select => Glyph::Smart,
            Tool::Brush => Glyph::Brush,
            Tool::Scissors => Glyph::Scissors,
            Tool::Erase => Glyph::Eraser,
            Tool::Zoom => Glyph::ZoomIn,
        }
    }

    /// Its name in the list of keys.
    fn name(self) -> &'static str {
        match self {
            Tool::Pencil => tr("pencil"),
            Tool::Select => tr("selection"),
            Tool::Brush => tr("brush"),
            Tool::Scissors => tr("scissors"),
            Tool::Erase => tr("eraser"),
            Tool::Zoom => tr("zoom"),
        }
    }
}

/// The editor's window over the main one, or filling it in full screen; false once it is
/// closed, by its button or by `escape` (Escape pressed, nothing else taking it). In full
/// screen, Escape first comes back to the window; a double-click on the title bar switches.
pub(crate) fn window(
    ctx: &egui::Context,
    id: &str,
    title: String,
    escape: bool,
    add: impl FnOnce(&mut Ui),
) -> bool {
    let screen = ctx.content_rect();
    let full = full_screen(ctx, id);
    let mut open = true;
    // Full screen is a window of its own: the other keeps its place and size to come back to.
    let window_id = if full {
        egui::Id::unique((id, "full-screen-window"))
    } else {
        egui::Id::unique(id)
    };
    let window = egui::Window::new(title)
        .id(window_id)
        .collapsible(false)
        .min_size(vec2(560.0, 260.0));
    let window = if full {
        window
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .fixed_rect(screen)
    } else {
        window
            .open(&mut open)
            .resizable(true)
            .default_size(vec2(screen.width() * 0.82, screen.height() * 0.62))
            .default_pos(pos2(
                screen.left() + screen.width() * 0.09,
                screen.top() + screen.height() * 0.2,
            ))
    };
    let shown = window.show(ctx, add);
    // A double-click on the title bar, away from the close button.
    if !full && let Some(shown) = shown {
        let rect = shown.response.rect;
        let bar = Rect::from_min_max(rect.min, pos2(rect.right() - 32.0, rect.top() + 28.0));
        let double = ctx.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                && i.pointer.interact_pos().is_some_and(|p| bar.contains(p))
        });
        if double {
            set_full_screen(ctx, id, true);
        }
    }
    if escape {
        if full {
            set_full_screen(ctx, id, false);
        } else {
            return false;
        }
    }
    open
}

/// The bar of tools: `tools` first, from the left (each with its hint), then what `add`
/// lays out, and at the right the keys of the tools. Gives the tool clicked.
pub(crate) fn tool_bar(
    ui: &mut Ui,
    id: &str,
    tools: &[(Tool, &str)],
    current: Tool,
    add: impl FnOnce(&mut Ui),
) -> Option<Tool> {
    let p = crate::theme::current(ui);
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOOLS), Sense::hover());
    let mut clicked = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal_centered(|ui| {
            for &(tool, hint) in tools {
                if button(ui, tool.glyph(), current == tool, hint).clicked() {
                    clicked = Some(tool);
                }
            }
            separator(ui);
            add(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                let full = full_screen(ui.ctx(), id);
                let hint = if full {
                    tr("Back to the window (F11 or Esc)")
                } else {
                    tr("Full screen: the editor fills the main window (F11)")
                };
                if button(ui, Glyph::FullScreen, full, hint).clicked() {
                    set_full_screen(ui.ctx(), id, !full);
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new(keys(tools)).size(11.0).color(p.weak));
            });
        });
    });
    clicked
}

/// The keys of `tools`, the zoom's and the one that closes: "P: pencil · Z: zoom · Esc: close".
fn keys(tools: &[(Tool, &str)]) -> String {
    let mut parts: Vec<String> = tools
        .iter()
        .map(|(t, _)| *t)
        .chain([Tool::Zoom])
        .map(|t| trf!("{key}: {tool}", key = t.key().name(), tool = t.name()))
        .collect();
    parts.push(trf!("{key}: {tool}", key = "F", tool = tr("frame")));
    parts.push(trf!("{key}: {tool}", key = "F11", tool = tr("full screen")));
    parts.push(trf!("{key}: {tool}", key = tr("Esc"), tool = tr("close")));
    parts.join(" · ")
}

/// The tool whose key was pressed, taken before the shortcuts.
pub(crate) fn take_tool_key(ctx: &egui::Context, tools: &[Tool]) -> Option<Tool> {
    if ctx.text_edit_focused() {
        return None;
    }
    tools
        .iter()
        .chain([&Tool::Zoom])
        .copied()
        .find(|t| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, t.key())))
}

/// The keys of the editor `id` beside its tools: F11 switches the full screen; F frames, as
/// in the timeline (true when pressed).
pub(crate) fn take_view_keys(ctx: &egui::Context, id: &str) -> bool {
    if ctx.text_edit_focused() {
        return false;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
        set_full_screen(ctx, id, !full_screen(ctx, id));
    }
    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F))
}

/// The snap switch of an editor, its own.
pub(crate) fn snap_toggle(ui: &mut Ui, on: bool, hint: &str) -> bool {
    let p = crate::theme::current(ui);
    crate::theme::labeled_toggle(ui, crate::theme::Icon::Snap, tr("Snap"), on, p.accent, hint)
        .clicked()
}

/// What the view's buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewButton {
    /// The zoom tool taken.
    ZoomTool,
    ZoomOut,
    Fit,
}

/// The zoom tool, zoom out and fit, in this order in every editor.
pub(crate) fn view_buttons(ui: &mut Ui, current: Tool, fit: &str) -> Option<ViewButton> {
    let mut asked = None;
    if button(
        ui,
        Glyph::ZoomIn,
        current == Tool::Zoom,
        tr("Zoom: drag across a span to fill the view with it, click to zoom in, Alt+click to zoom out"),
    )
    .clicked()
    {
        asked = Some(ViewButton::ZoomTool);
    }
    if button(ui, Glyph::ZoomOut, false, tr("Zoom out")).clicked() {
        asked = Some(ViewButton::ZoomOut);
    }
    if button(ui, Glyph::Fit, false, &format!("{fit} (F)")).clicked() {
        asked = Some(ViewButton::Fit);
    }
    asked
}

/// What the zoom tool asks of the view, in screen x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Zoomed {
    /// This span fills the view.
    Span(f32, f32),
    /// Twice closer around this x.
    In(f32),
    /// Twice farther around this x.
    Out(f32),
}

/// The zoom tool over `area`: a span dragged across fills the view, a click zooms in
/// (Alt or right-click: out). `from` keeps where the drag began.
pub(crate) fn zoom_tool(
    ui: &mut Ui,
    area: Rect,
    painter: &Painter,
    from: &mut Option<f32>,
) -> Option<Zoomed> {
    let response = ui.interact(area, ui.scope_id().with("zoom"), Sense::click_and_drag());
    let alt = ui.input(|i| i.modifiers.alt);
    if response.hovered() {
        ui.ctx().set_cursor_icon(if alt {
            CursorIcon::ZoomOut
        } else {
            CursorIcon::ZoomIn
        });
    }
    // The press is kept: the pointer has let go of it by the time the drag stops.
    if response.drag_started() {
        *from = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos())
            .map(|p| p.x);
    }
    let mut asked = None;
    if let (Some(a), Some(b)) = (*from, response.interact_pointer_pos())
        && (response.dragged() || response.drag_stopped())
    {
        let span = Rect::from_x_y_ranges(a.min(b.x)..=a.max(b.x), area.y_range());
        let accent = ui.visuals().selection.bg_fill;
        painter.rect_filled(span, 0.0, accent.gamma_multiply(0.18));
        painter.rect_stroke(span, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
        if response.drag_stopped() {
            *from = None;
            if span.width() > 6.0 {
                asked = Some(Zoomed::Span(span.left(), span.right()));
            }
        }
    }
    if (response.clicked() || response.secondary_clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        asked = Some(if alt || response.secondary_clicked() {
            Zoomed::Out(pos.x)
        } else {
            Zoomed::In(pos.x)
        });
    }
    asked
}

/// What the tool in hand does, on a pill at the top of `area`, on as many lines as it takes.
pub(crate) fn help(ui: &Ui, painter: &Painter, area: Rect, text: &str) {
    let p = crate::theme::current(ui);
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        FontId::proportional(11.5),
        p.text,
        (area.width() - 60.0).max(120.0),
    );
    job.halign = Align::Center;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let size = galley.rect.size();
    let pill = Rect::from_center_size(
        pos2(area.center().x, area.top() + 26.0 + size.y / 2.0),
        size + vec2(20.0, 8.0),
    );
    let visuals = ui.visuals();
    painter.rect_filled(
        pill,
        6.0,
        crate::theme::mix(visuals.extreme_bg_color, p.accent, 0.22),
    );
    painter.rect_stroke(
        pill,
        6.0,
        Stroke::new(1.0, p.accent.gamma_multiply(0.5)),
        StrokeKind::Inside,
    );
    painter.galley(pos2(pill.center().x, pill.top() + 4.0), galley, p.text);
}

/// The playhead across an editor, from `top` to `bottom`.
pub(crate) fn playhead(painter: &Painter, x: f32, top: f32, bottom: f32) {
    painter.vline(
        x,
        top..=bottom,
        Stroke::new(1.5, egui::Color32::from_rgb(255, 78, 66)),
    );
}
