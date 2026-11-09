//! Visual system shared by every view: palette, fonts, spacing, and icons drawn as vectors.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2,
    Rect, Response, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui, Vec2, Visuals, epaint, pos2,
    vec2,
};
use serde::{Deserialize, Serialize};

/// How meters light up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeterStyle {
    /// A smooth bar, green to red.
    Gradient,
    /// Separate lights, as on a console.
    Segments,
    /// A bar with a soft halo.
    Glow,
}

/// How fader caps are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Strips and bars as glossy cards lifted off the panel by a soft shadow.
    pub gloss: bool,
    /// Color of the strips' and bars' faces, when it differs from the panels'.
    pub face: Option<Color32>,
}

pub const fn look(radius: u8, meter: MeterStyle, cap: CapStyle) -> Look {
    Look {
        radius,
        meter,
        cap,
        gloss: false,
        face: None,
    }
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

/// Light gray, white glossy buttons and strips, blue accent.
pub const LIGHT: Palette = Palette {
    base: Color32::from_rgb(160, 165, 173),
    panel: Color32::from_rgb(200, 204, 210),
    surface: Color32::from_rgb(250, 251, 252),
    hover: Color32::from_rgb(255, 255, 255),
    pressed: Color32::from_rgb(220, 225, 232),
    border: Color32::from_rgb(110, 116, 125),
    text: Color32::from_rgb(8, 10, 14),
    weak: Color32::from_rgb(52, 56, 64),
    accent: Color32::from_rgb(0, 98, 220),
    dark: false,
    look: Look {
        gloss: true,
        face: Some(Color32::from_rgb(236, 238, 241)),
        ..look(5, MeterStyle::Gradient, CapStyle::Modern)
    },
};

/// A theme made from a background, an accent and a contrast (0 soft, 1 strong), readable
/// whatever they are: the background stays clearly dark or clearly light and only lightly
/// colored, the texts reach a contrast of 7:1 and 4.5:1 on it, the accent 3:1, and the accent
/// keeps away from the record red.
pub fn custom(background: Color32, accent: Color32, contrast: f32, look: Look) -> Palette {
    let c = contrast.clamp(0.0, 1.0);
    let (h, s, l) = hsl(background);
    let dark = l < 0.5;
    let s = s.min(0.3);
    let l = if dark {
        l.clamp(0.04, 0.22)
    } else {
        l.clamp(0.7, 0.95)
    };
    let tone = |l: f32| from_hsl(h, s, l.clamp(0.0, 1.0));
    let (base, panel, surface, hover, pressed, border) = if dark {
        (
            tone(l - 0.04 - 0.04 * c),
            tone(l),
            tone(l + 0.06 + 0.05 * c),
            tone(l + 0.1 + 0.06 * c),
            tone(l + 0.14 + 0.07 * c),
            tone(l - 0.08 - 0.04 * c),
        )
    } else {
        (
            tone(l - 0.12 - 0.08 * c),
            tone(l),
            tone(0.97 + 0.02 * c),
            tone(1.0),
            tone(l + 0.05),
            tone(l - 0.3 - 0.15 * c),
        )
    };
    let ink = if dark { Color32::WHITE } else { Color32::BLACK };
    let mut text = tone(if dark {
        0.88 + 0.08 * c
    } else {
        0.1 - 0.06 * c
    });
    while (contrast_ratio(text, panel) < 7.0 || contrast_ratio(text, surface) < 4.5) && text != ink
    {
        text = mix(text, ink, 0.1);
    }
    let mut weak = mix(text, panel, 0.42 - 0.17 * c);
    while contrast_ratio(weak, panel) < 4.5 && weak != text {
        weak = mix(weak, text, 0.1);
    }
    let accent = readable_accent(accent, panel, dark);
    let face = (!dark).then(|| mix(panel, Color32::WHITE, 0.65));
    Palette {
        base,
        panel,
        surface,
        hover,
        pressed,
        border,
        text,
        weak,
        accent,
        dark,
        look: Look {
            gloss: !dark,
            face,
            ..look
        },
    }
}

/// `accent` turned away from the record red, then lightened or darkened until it stands out
/// on `panel` (and, on light themes, carries white text).
fn readable_accent(accent: Color32, panel: Color32, dark: bool) -> Color32 {
    let (mut h, s, mut l) = hsl(accent);
    let (red, _, _) = hsl(RECORD);
    // Signed distance to the red's hue, -180 to 180 degrees.
    let away = (h - red + 540.0).rem_euclid(360.0) - 180.0;
    if s > 0.3 && away.abs() < 22.0 {
        h = (red + 22.0_f32.copysign(away)).rem_euclid(360.0);
    }
    let mut color = from_hsl(h, s, l);
    for _ in 0..40 {
        let on_panel = contrast_ratio(color, panel) >= 3.0;
        let white = dark || contrast_ratio(Color32::WHITE, color) >= 3.0;
        if on_panel && white {
            break;
        }
        l = if dark { l + 0.02 } else { l - 0.02 }.clamp(0.0, 1.0);
        color = from_hsl(h, s, l);
    }
    color
}

/// Hue (degrees), saturation and lightness (0 to 1) of a color.
fn hsl(c: Color32) -> (f32, f32, f32) {
    let [r, g, b] = [c.r(), c.g(), c.b()].map(|v| f32::from(v) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let v = |u: f32| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(v(r), v(g), v(b))
}

/// Contrast between two colors, as the web's accessibility guidelines measure it (1 to 21).
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let luminance = |c: Color32| {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

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

/// Ink of every on/off switch: green while on, grey when off.
pub fn power_ink(on: bool, p: &Palette) -> Color32 {
    if on { PLAY } else { p.weak.gamma_multiply(0.7) }
}

/// The active theme's palette when it matches `dark`, else the default dark or light one.
pub fn palette(dark: bool) -> Palette {
    let active = ACTIVE.read().ok().and_then(|a| *a);
    match active {
        Some(p) if p.dark == dark => p,
        _ if dark => DARK,
        _ => LIGHT,
    }
}

/// A card of controls (an editor's section, an effect in the browser): sunken in the base on
/// dark themes, a card in the theme's button color with an edge on light ones.
pub fn card(p: &Palette) -> (Color32, Stroke) {
    if p.dark {
        (p.base, Stroke::NONE)
    } else {
        (p.surface, Stroke::new(1.0, mix(p.border, p.surface, 0.45)))
    }
}

/// Behind plots, curves and waveforms in the effect editors: the base on dark themes, nearly
/// white on light ones, where gray would dull them.
pub fn screen(p: &Palette) -> Color32 {
    if p.dark {
        p.base
    } else {
        mix(p.base, Color32::WHITE, 0.82)
    }
}

/// The face of a strip or a bar: `fill`, lifted by a soft shadow and lit on its top half when
/// the theme is glossy; as shapes to put under what is drawn on it.
pub fn face(rect: Rect, radius: f32, fill: Color32, p: &Palette) -> Vec<Shape> {
    let mut shapes = Vec::new();
    if p.look.gloss {
        for k in 0..4u8 {
            let grow = 1.0 + f32::from(k) * 2.0;
            shapes.push(Shape::rect_filled(
                rect.translate(vec2(0.0, 2.0)).expand(grow),
                radius + grow,
                Color32::from_black_alpha(14 - k * 3),
            ));
        }
    }
    shapes.push(Shape::rect_filled(rect, radius, fill));
    if p.look.gloss {
        let inner = rect.shrink(1.0);
        let top = Rect::from_min_max(inner.min, pos2(inner.right(), inner.center().y));
        shapes.push(crate::gear::sheen_shape(
            top,
            Color32::from_white_alpha(70),
            Color32::TRANSPARENT,
        ));
    }
    shapes
}

/// A track's color as drawn: deeper and more saturated on light themes, where pale colors
/// would wash out.
pub fn track_color(rgb: [u8; 3]) -> Color32 {
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let light = ACTIVE.read().ok().and_then(|a| *a).is_some_and(|a| !a.dark);
    if !light {
        return color;
    }
    let luma =
        (0.3 * f32::from(rgb[0]) + 0.59 * f32::from(rgb[1]) + 0.11 * f32::from(rgb[2])) as u8;
    let saturate = |c: u8| {
        let v = f32::from(luma) + (f32::from(c) - f32::from(luma)) * 1.45;
        (v * 0.84).clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(saturate(rgb[0]), saturate(rgb[1]), saturate(rgb[2]))
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
    // Instrument icons (Lucide, ISC) in the private use area, after the text fonts.
    fonts.font_data.insert(
        "track-icons".into(),
        std::sync::Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Lucide-TrackIcons.ttf"
        ))),
    );
    let mut fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fallbacks.push("track-icons".to_owned());
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
    if !dark {
        // Menus and dialogs float as white sheets over the light panels.
        v.window_fill = Color32::WHITE;
        // Text fields are white, as on the sheets they sit on.
        v.text_edit_bg_color = Some(Color32::WHITE);
    }
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
    /// A robot's head: the AI assistants.
    Robot,
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
        Icon::Robot => {
            let head = Rect::from_center_size(pos2(c.x, c.y + s * 0.18), vec2(s * 1.5, s * 1.1));
            painter.rect_stroke(head, s * 0.25, stroke, StrokeKind::Middle);
            for dx in [-0.33, 0.33] {
                painter.circle_filled(pos2(c.x + dx * s, c.y + s * 0.1), s * 0.14, color);
            }
            painter.line_segment(
                [
                    pos2(c.x - s * 0.25, c.y + s * 0.45),
                    pos2(c.x + s * 0.25, c.y + s * 0.45),
                ],
                stroke,
            );
            painter.line_segment([pos2(c.x, head.top()), pos2(c.x, c.y - s * 0.7)], stroke);
            painter.circle_filled(pos2(c.x, c.y - s * 0.78), s * 0.13, color);
            for side in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        pos2(c.x + side * s * 0.75, c.y + s * 0.1),
                        pos2(c.x + side * s * 0.92, c.y + s * 0.1),
                    ],
                    stroke,
                );
            }
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

/// A square icon button, `k` times its normal size; `on` shows it lit in `tint`.
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    on: bool,
    tint: Color32,
    hint: &str,
    k: f32,
) -> Response {
    let size = vec2(30.0, 26.0) * k;
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
    paint_icon(ui.painter(), rect.shrink(6.0 * k), icon, color);
    response.on_hover_text(hint)
}

/// An icon and its name as one toggle, lit in `tint` when on: for a mode to find at a glance.
pub fn labeled_toggle(
    ui: &mut Ui,
    icon: Icon,
    text: &str,
    on: bool,
    tint: Color32,
    hint: &str,
) -> Response {
    let p = current(ui);
    let font = strong(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(28.0 + galley.size().x + 10.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        tint.gamma_multiply(0.28)
    } else if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    let border = if on { tint } else { p.border };
    ui.painter()
        .rect_stroke(rect, 5.0, Stroke::new(1.0, border), StrokeKind::Inside);
    let color = if on { tint } else { p.text.gamma_multiply(0.8) };
    let icon_rect = Rect::from_center_size(rect.left_center() + vec2(15.0, 0.0), vec2(14.0, 14.0));
    paint_icon(ui.painter(), icon_rect, icon, color);
    let ink = if on {
        p.text
    } else {
        p.text.gamma_multiply(0.8)
    };
    ui.painter().galley(
        egui::pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
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

/// An on/off sign: a ring open at the top with a bar through the gap.
pub fn paint_power(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.36;
    let stroke = Stroke::new((r * 0.32).max(1.2), color);
    let points: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2
                + 0.6
                + (std::f32::consts::TAU - 1.2) * i as f32 / 20.0;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    painter.add(Shape::line(points, stroke));
    painter.line_segment(
        [pos2(c.x, c.y - r * 1.15), pos2(c.x, c.y - r * 0.15)],
        stroke,
    );
}

/// The color of an effect family (as the effect browser names them).
pub fn family_color(family: &str) -> Color32 {
    match family {
        "Dynamics" => Color32::from_rgb(236, 152, 64),
        "EQ & Filter" => Color32::from_rgb(86, 164, 244),
        "Color" => Color32::from_rgb(226, 98, 84),
        "Amp & Cab" => Color32::from_rgb(204, 78, 78),
        "Modulation" => Color32::from_rgb(170, 116, 234),
        "Space" => Color32::from_rgb(72, 192, 196),
        "Stereo" => Color32::from_rgb(120, 204, 140),
        "Instrument" => Color32::from_rgb(226, 194, 84),
        _ => Color32::from_rgb(140, 150, 164),
    }
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
