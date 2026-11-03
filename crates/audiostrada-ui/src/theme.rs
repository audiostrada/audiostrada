//! Visual system shared by every view: palette, fonts, spacing, and icons drawn as vectors.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2,
    Rect, Response, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2, Visuals, epaint, pos2,
    vec2,
};

/// How meters light up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterStyle {
    /// A smooth bar, green to red.
    Gradient,
    /// Separate lights, as on a console.
    Segments,
    /// A bar with a soft halo.
    Glow,
}

/// How fader caps are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapStyle {
    /// Flat with a line.
    Modern,
    /// Light, ridged, as on a console.
    Classic,
}

/// What sets a theme apart besides its colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Rounding of buttons and fields; windows get a little more.
    pub radius: u8,
    pub meter: MeterStyle,
    pub cap: CapStyle,
}

const fn look(radius: u8, meter: MeterStyle, cap: CapStyle) -> Look {
    Look { radius, meter, cap }
}

/// Colors and look of one theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Behind lanes and meters.
    pub base: Color32,
    pub panel: Color32,
    /// Buttons and fields at rest.
    pub surface: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub border: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub accent: Color32,
    pub dark: bool,
    pub look: Look,
}

pub const DARK: Palette = Palette {
    base: Color32::from_rgb(20, 22, 26),
    panel: Color32::from_rgb(30, 33, 38),
    surface: Color32::from_rgb(43, 47, 54),
    hover: Color32::from_rgb(54, 59, 68),
    pressed: Color32::from_rgb(64, 70, 81),
    border: Color32::from_rgb(14, 15, 18),
    text: Color32::from_rgb(222, 226, 233),
    weak: Color32::from_rgb(134, 142, 156),
    accent: Color32::from_rgb(82, 160, 255),
    dark: true,
    look: look(4, MeterStyle::Gradient, CapStyle::Modern),
};

/// Deep blue night, cyan accent.
pub const MIDNIGHT: Palette = Palette {
    base: Color32::from_rgb(10, 14, 24),
    panel: Color32::from_rgb(18, 24, 38),
    surface: Color32::from_rgb(30, 39, 60),
    hover: Color32::from_rgb(40, 52, 78),
    pressed: Color32::from_rgb(50, 64, 94),
    border: Color32::from_rgb(6, 9, 16),
    text: Color32::from_rgb(214, 226, 245),
    weak: Color32::from_rgb(120, 140, 172),
    accent: Color32::from_rgb(64, 208, 255),
    dark: true,
    look: look(6, MeterStyle::Glow, CapStyle::Modern),
};

/// Walnut and cream, amber accent, like an old console.
pub const VINTAGE: Palette = Palette {
    base: Color32::from_rgb(40, 31, 23),
    panel: Color32::from_rgb(68, 52, 38),
    surface: Color32::from_rgb(100, 79, 58),
    hover: Color32::from_rgb(118, 94, 70),
    pressed: Color32::from_rgb(136, 109, 81),
    border: Color32::from_rgb(26, 19, 13),
    text: Color32::from_rgb(250, 238, 214),
    weak: Color32::from_rgb(200, 176, 140),
    accent: Color32::from_rgb(244, 172, 64),
    dark: true,
    look: look(3, MeterStyle::Segments, CapStyle::Classic),
};

/// Neutral studio gray, orange accent.
pub const STUDIO: Palette = Palette {
    base: Color32::from_rgb(42, 43, 46),
    panel: Color32::from_rgb(58, 60, 64),
    surface: Color32::from_rgb(76, 78, 84),
    hover: Color32::from_rgb(88, 91, 98),
    pressed: Color32::from_rgb(100, 104, 112),
    border: Color32::from_rgb(30, 31, 33),
    text: Color32::from_rgb(236, 237, 240),
    weak: Color32::from_rgb(160, 164, 172),
    accent: Color32::from_rgb(255, 146, 56),
    dark: true,
    look: look(2, MeterStyle::Gradient, CapStyle::Classic),
};

/// Soft gray daylight, blue accent.
pub const LIGHT: Palette = Palette {
    base: Color32::from_rgb(176, 181, 189),
    panel: Color32::from_rgb(214, 217, 223),
    surface: Color32::from_rgb(234, 236, 240),
    hover: Color32::from_rgb(245, 246, 248),
    pressed: Color32::from_rgb(198, 203, 211),
    border: Color32::from_rgb(150, 156, 166),
    text: Color32::from_rgb(24, 27, 32),
    weak: Color32::from_rgb(84, 91, 104),
    accent: Color32::from_rgb(22, 102, 206),
    dark: false,
    look: look(5, MeterStyle::Gradient, CapStyle::Modern),
};

/// A big console: charcoal, sharp corners, lights for meters, ridged caps, red accent.
pub const CONSOLE: Palette = Palette {
    base: Color32::from_rgb(16, 17, 18),
    panel: Color32::from_rgb(36, 37, 39),
    surface: Color32::from_rgb(54, 55, 58),
    hover: Color32::from_rgb(66, 67, 71),
    pressed: Color32::from_rgb(80, 81, 86),
    border: Color32::from_rgb(8, 8, 9),
    text: Color32::from_rgb(232, 230, 224),
    weak: Color32::from_rgb(150, 148, 142),
    accent: Color32::from_rgb(242, 88, 60),
    dark: true,
    look: look(0, MeterStyle::Segments, CapStyle::Classic),
};

/// Black with neon: magenta accent, glowing meters, round corners.
pub const NEON: Palette = Palette {
    base: Color32::from_rgb(6, 4, 12),
    panel: Color32::from_rgb(16, 12, 26),
    surface: Color32::from_rgb(32, 24, 50),
    hover: Color32::from_rgb(46, 34, 72),
    pressed: Color32::from_rgb(60, 44, 92),
    border: Color32::from_rgb(2, 1, 6),
    text: Color32::from_rgb(240, 232, 255),
    weak: Color32::from_rgb(150, 132, 190),
    accent: Color32::from_rgb(255, 64, 190),
    dark: true,
    look: look(8, MeterStyle::Glow, CapStyle::Modern),
};

/// Warm paper and ink, for daylight.
pub const PAPER: Palette = Palette {
    base: Color32::from_rgb(190, 178, 156),
    panel: Color32::from_rgb(226, 216, 196),
    surface: Color32::from_rgb(240, 232, 216),
    hover: Color32::from_rgb(248, 242, 230),
    pressed: Color32::from_rgb(210, 198, 176),
    border: Color32::from_rgb(156, 140, 114),
    text: Color32::from_rgb(42, 34, 24),
    weak: Color32::from_rgb(104, 90, 70),
    accent: Color32::from_rgb(24, 112, 122),
    dark: false,
    look: look(3, MeterStyle::Segments, CapStyle::Classic),
};

static ACTIVE: std::sync::RwLock<Option<Palette>> = std::sync::RwLock::new(None);

/// Makes `palette` the theme every view draws with.
pub fn set_palette(palette: Palette) {
    if let Ok(mut active) = ACTIVE.write() {
        *active = Some(palette);
    }
}

pub const RECORD: Color32 = Color32::from_rgb(232, 72, 72);
pub const MUTE: Color32 = Color32::from_rgb(232, 140, 50);
pub const SOLO: Color32 = Color32::from_rgb(232, 200, 64);
pub const PLAY: Color32 = Color32::from_rgb(86, 196, 110);

/// The active theme's palette when it matches `dark`, else the default dark or light one.
pub fn palette(dark: bool) -> Palette {
    let active = ACTIVE.read().ok().and_then(|a| *a);
    match active {
        Some(p) if p.dark == dark => p,
        _ if dark => DARK,
        _ => LIGHT,
    }
}

/// Color between `a` and `b` (0 = a, 1 = b).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Palette matching the current visuals.
pub fn current(ui: &Ui) -> Palette {
    palette(ui.visuals().dark_mode)
}

/// Palette matching the visuals of `ctx`, outside any `Ui`.
pub fn current_for(ctx: &egui::Context) -> Palette {
    palette(ctx.global_style().visuals.dark_mode)
}

const SEMIBOLD: &str = "inter-semibold";

/// Inter for the interface, the egui defaults as fallbacks.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        std::sync::Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        SEMIBOLD.into(),
        std::sync::Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut proportional = vec!["inter".to_owned()];
    proportional.extend(fallbacks.iter().cloned());
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    let mut strong = vec![SEMIBOLD.to_owned()];
    strong.extend(fallbacks);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), strong);
    ctx.set_fonts(fonts);
}

/// Semibold text, for names and headings.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn visuals(p: Palette) -> Visuals {
    let dark = p.dark;
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let radius = CornerRadius::same(p.look.radius);
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.base;
    v.faint_bg_color = mix(p.base, p.panel, 0.4);
    v.code_bg_color = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(p.look.radius.saturating_add(2));
    v.menu_corner_radius = CornerRadius::same(p.look.radius.saturating_add(2));
    v.selection.bg_fill = p.accent.gamma_multiply(0.85);
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = Color32::from_rgb(240, 180, 60);
    v.error_fg_color = Color32::from_rgb(240, 90, 80);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut w.inactive, p.surface),
        (&mut w.hovered, p.hover),
        (&mut w.active, p.pressed),
        (&mut w.open, p.pressed),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = radius;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.expansion = 0.0;
    }
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.35));
    if !dark {
        // On light colors buttons need an edge to stand out.
        w.inactive.bg_stroke = Stroke::new(1.0, p.border);
        w.hovered.bg_stroke = Stroke::new(1.0, mix(p.border, p.accent, 0.6));
    }
    v.weak_text_color = Some(p.weak);
    // Windows and menus float above the panels.
    let shade = if dark { 120 } else { 45 };
    v.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(shade),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(shade),
    };
    v
}

/// Spacing and text sizes.
pub fn style(style: &mut egui::Style) {
    let s = &mut style.spacing;
    s.item_spacing = vec2(6.0, 5.0);
    s.button_padding = vec2(8.0, 3.0);
    s.interact_size = vec2(24.0, 22.0);
    s.menu_margin = egui::Margin::same(6);
    s.window_margin = egui::Margin::same(12);
    s.menu_width = 240.0;
    s.tooltip_width = 320.0;
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(10.5)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Heading, strong(16.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    ToStart,
    Play,
    Pause,
    Stop,
    Record,
    Loop,
    Punch,
    Metronome,
    Snap,
    /// A stopwatch: the low latency mode.
    Latency,
}

/// Draws `icon` centered in `rect`.
pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) * 0.5;
    let stroke = Stroke::new((s * 0.16).max(1.4), color);
    match icon {
        Icon::Play => {
            let points = vec![
                pos2(c.x - s * 0.45, c.y - s * 0.6),
                pos2(c.x + s * 0.6, c.y),
                pos2(c.x - s * 0.45, c.y + s * 0.6),
            ];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Pause => {
            for dx in [-0.3, 0.3] {
                painter.rect_filled(
                    Rect::from_center_size(pos2(c.x + dx * s, c.y), vec2(s * 0.3, s * 1.15)),
                    1.0,
                    color,
                );
            }
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(s * 1.05)), 2.0, color);
        }
        Icon::Record => {
            painter.circle_filled(c, s * 0.58, color);
        }
        Icon::ToStart => {
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x - s * 0.5, c.y), vec2(s * 0.2, s * 1.1)),
                1.0,
                color,
            );
            let points = vec![
                pos2(c.x + s * 0.55, c.y - s * 0.55),
                pos2(c.x - s * 0.3, c.y),
                pos2(c.x + s * 0.55, c.y + s * 0.55),
            ];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Loop => {
            let r = s * 0.55;
            let arc = |from: f32, to: f32| -> Vec<Pos2> {
                (0..=12)
                    .map(|i| {
                        let a = from + (to - from) * i as f32 / 12.0;
                        pos2(c.x + r * 1.15 * a.cos(), c.y + r * a.sin())
                    })
                    .collect()
            };
            use std::f32::consts::PI;
            painter.add(Shape::line(arc(PI * 0.15, PI * 0.95), stroke));
            painter.add(Shape::line(arc(PI * 1.15, PI * 1.95), stroke));
            for (angle, dir) in [(PI * 0.15, -1.0), (PI * 1.15, 1.0)] {
                let tip = pos2(c.x + r * 1.15 * angle.cos(), c.y + r * angle.sin());
                let head = s * 0.28;
                painter.add(Shape::convex_polygon(
                    vec![
                        tip + vec2(0.0, -head * dir),
                        tip + vec2(head * dir, head * 0.2 * dir),
                        tip + vec2(-head * 0.2 * dir, head * dir),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Icon::Punch => {
            let h = s * 0.6;
            let w = s * 0.25;
            for side in [-1.0f32, 1.0] {
                let x = c.x + side * s * 0.55;
                painter.add(Shape::line(
                    vec![
                        pos2(x - side * w, c.y - h),
                        pos2(x, c.y - h),
                        pos2(x, c.y + h),
                        pos2(x - side * w, c.y + h),
                    ],
                    stroke,
                ));
            }
            painter.circle_filled(c, s * 0.2, color);
        }
        Icon::Metronome => {
            let body = vec![
                pos2(c.x - s * 0.18, c.y - s * 0.7),
                pos2(c.x + s * 0.18, c.y - s * 0.7),
                pos2(c.x + s * 0.55, c.y + s * 0.65),
                pos2(c.x - s * 0.55, c.y + s * 0.65),
            ];
            painter.add(Shape::closed_line(body, stroke));
            painter.line_segment(
                [
                    pos2(c.x, c.y + s * 0.45),
                    pos2(c.x + s * 0.45, c.y - s * 0.55),
                ],
                stroke,
            );
        }
        Icon::Latency => {
            let center = pos2(c.x, c.y + s * 0.12);
            let r = s * 0.62;
            painter.circle_stroke(center, r, stroke);
            painter.line_segment(
                [
                    pos2(c.x - s * 0.18, c.y - s * 0.72),
                    pos2(c.x + s * 0.18, c.y - s * 0.72),
                ],
                stroke,
            );
            painter.line_segment(
                [center, pos2(center.x + r * 0.45, center.y - r * 0.55)],
                stroke,
            );
        }
        Icon::Snap => {
            let r = s * 0.5;
            let points: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 16.0;
                    pos2(c.x - r * a.cos(), c.y + r * a.sin() - s * 0.1)
                })
                .collect();
            painter.add(Shape::line(points, Stroke::new(stroke.width * 1.6, color)));
            for side in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        pos2(c.x + side * r, c.y - s * 0.1),
                        pos2(c.x + side * r, c.y - s * 0.65),
                    ],
                    Stroke::new(stroke.width * 1.6, color),
                );
            }
        }
    }
}

/// A square icon button; `on` shows it lit in `tint`.
pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool, tint: Color32, hint: &str) -> Response {
    let size = vec2(30.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let p = current(ui);
    let fill = if on {
        tint.gamma_multiply(0.22)
    } else if response.is_pointer_button_down_on() {
        p.pressed
    } else if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    if on {
        ui.painter().rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0, tint.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
    } else if !p.dark {
        ui.painter()
            .rect_stroke(rect, 5.0, Stroke::new(1.0, p.border), StrokeKind::Inside);
    }
    let color = if on || matches!(icon, Icon::Record) {
        tint
    } else if response.hovered() {
        p.text
    } else {
        p.text.gamma_multiply(0.85)
    };
    paint_icon(ui.painter(), rect.shrink(6.0), icon, color);
    response.on_hover_text(hint)
}

/// A small rounded toggle with a letter, as in track headers and channel strips.
/// The button of a window's main action, filled with the accent.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    primary_button_enabled(ui, true, text)
}

pub fn primary_button_enabled(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let accent = current(ui).accent;
    let [r, g, b, _] = accent.to_array();
    let light = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 150.0;
    let ink = if light {
        Color32::from_rgb(20, 20, 24)
    } else {
        Color32::WHITE
    };
    let button = egui::Button::new(egui::RichText::new(text).color(ink).strong()).fill(accent);
    ui.add_enabled(enabled, button)
}

/// The bottom of a window: a line, then buttons from the right (the main one first).
pub fn footer(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.separator();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}

/// A small heading over a group of settings.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .size(10.5)
            .strong()
            .color(current(ui).weak),
    );
}

/// The record arm button: a red dot, ringed while off, on red once armed.
pub fn arm_toggle(ui: &mut Ui, size: Vec2, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    paint_arm_toggle(ui, rect, &response, on);
    response
}

pub fn paint_arm_toggle(ui: &Ui, rect: Rect, response: &Response, on: bool) {
    let p = current(ui);
    let fill = if on {
        RECORD
    } else if response.hovered() {
        RECORD.gamma_multiply(0.25)
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    if !on {
        ui.painter().rect_stroke(
            rect,
            3.0,
            Stroke::new(1.0, RECORD.gamma_multiply(0.4)),
            StrokeKind::Inside,
        );
    }
    let radius = (rect.height() * 0.24).clamp(3.5, 6.0);
    if on {
        ui.painter()
            .circle_filled(rect.center(), radius, Color32::from_rgb(255, 245, 245));
    } else {
        ui.painter()
            .circle_filled(rect.center(), radius, RECORD.gamma_multiply(0.75));
    }
}

pub fn letter_toggle(ui: &mut Ui, size: Vec2, text: &str, on: bool, tint: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    paint_letter_toggle(ui, rect, &response, text, on, tint);
    response
}

pub fn paint_letter_toggle(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    text: &str,
    on: bool,
    tint: Color32,
) {
    let p = current(ui);
    let fill = if on {
        tint
    } else if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    if !on && !p.dark {
        ui.painter()
            .rect_stroke(rect, 3.0, Stroke::new(1.0, p.border), StrokeKind::Inside);
    }
    let color = if on {
        Color32::from_rgb(24, 24, 24)
    } else {
        p.weak
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        strong((rect.height() * 0.55).clamp(9.0, 13.0)),
        color,
    );
}

/// Lit while the track hears its input.
pub const MONITOR: Color32 = Color32::from_rgb(70, 190, 200);

/// The input monitoring button: a speaker, lit while the input is heard; a small "A" in
/// automatic mode, struck through when off.
pub fn paint_monitor_toggle(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    mode: audiostrada_core::Monitoring,
    hearing: bool,
) {
    use audiostrada_core::Monitoring;
    let p = current(ui);
    let fill = if hearing {
        MONITOR
    } else if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    if !hearing && !p.dark {
        ui.painter()
            .rect_stroke(rect, 3.0, Stroke::new(1.0, p.border), StrokeKind::Inside);
    }
    let color = if hearing {
        Color32::from_rgb(24, 24, 24)
    } else {
        p.weak
    };
    let s = rect.height() * 0.3;
    let shift = if mode == Monitoring::Auto { 1.0 } else { 0.5 };
    let c = rect.center() - vec2(s * shift, 0.0);
    let body = vec![
        pos2(c.x - s * 0.7, c.y - s * 0.35),
        pos2(c.x - s * 0.2, c.y - s * 0.35),
        pos2(c.x + s * 0.4, c.y - s * 0.9),
        pos2(c.x + s * 0.4, c.y + s * 0.9),
        pos2(c.x - s * 0.2, c.y + s * 0.35),
        pos2(c.x - s * 0.7, c.y + s * 0.35),
    ];
    ui.painter()
        .add(Shape::convex_polygon(body, color, Stroke::NONE));
    let stroke = Stroke::new(1.3, color);
    match mode {
        Monitoring::Off => {
            ui.painter().line_segment(
                [pos2(c.x - s, c.y + s), pos2(c.x + s * 1.6, c.y - s)],
                Stroke::new(1.6, color),
            );
        }
        _ => {
            for r in [0.75, 1.25] {
                let points: Vec<Pos2> = (0..=8)
                    .map(|i| {
                        let a = (i as f32 / 8.0 - 0.5) * 1.6;
                        pos2(c.x + s * 0.4 + r * s * a.cos(), c.y + r * s * a.sin())
                    })
                    .collect();
                ui.painter().add(Shape::line(points, stroke));
            }
        }
    }
    if mode == Monitoring::Auto {
        ui.painter().text(
            pos2(rect.right() - 2.0, rect.bottom() - 1.0),
            egui::Align2::RIGHT_BOTTOM,
            "A",
            strong(8.0),
            color,
        );
    }
}

/// A sunken box, as behind counters and meters.
pub fn well(painter: &Painter, rect: Rect, dark: bool) {
    let p = palette(dark);
    painter.rect_filled(rect, 4.0, p.base);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, p.border), StrokeKind::Inside);
}

/// Black or white, whichever reads better on `fill`.
pub fn text_on(fill: Color32) -> Color32 {
    let luminance =
        0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if luminance > 150.0 {
        Color32::from_rgb(20, 20, 20)
    } else {
        Color32::WHITE
    }
}

/// Vertical gradient between two colors, bottom to top.
pub fn gradient_rect(painter: &Painter, rect: Rect, bottom: Color32, top: Color32) {
    let mut mesh = epaint::Mesh::default();
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_mix_and_text_reads() {
        let (black, white) = (Color32::BLACK, Color32::WHITE);
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        assert_eq!(mix(black, white, 0.5), Color32::from_gray(128));
        assert_eq!(text_on(white), Color32::from_rgb(20, 20, 20));
        assert_eq!(text_on(black), Color32::WHITE);
    }
}
