//! Drawn guitar gear: amp heads and cabinets from procedural textures (tolex grain, tweed twill,
//! grille cloths, brushed metal) made once and tiled, with their light, shadows and hardware.

use eframe::egui::{
    self, Align2, Color32, ColorImage, FontId, Mesh, Painter, Pos2, Rect, Stroke, TextureHandle,
    TextureOptions, pos2, vec2,
};

/// A tileable texture, in gray, tinted when drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Texture {
    Tolex,
    Tweed,
    /// Fine woven cloth with silver threads.
    Sparkle,
    /// Coarse basket weave.
    Basket,
    /// Diagonal diamonds.
    Diamond,
    /// Woven cane.
    Cane,
    /// Perforated metal.
    Perforated,
    /// Horizontal brushing.
    Brushed,
}

const SIZE: usize = 256;

/// A pseudo-random value in 0..1 for an integer point, periodic over `period`.
fn hash(x: i32, y: i32, period: i32, seed: u32) -> f32 {
    let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ seed;
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65_535.0
}

/// Smooth value noise in 0..1, tileable over the texture, `cells` across.
fn noise(x: f32, y: f32, cells: i32, seed: u32) -> f32 {
    let (fx, fy) = (x * cells as f32, y * cells as f32);
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(tx), smooth(ty));
    let a = hash(ix, iy, cells, seed);
    let b = hash(ix + 1, iy, cells, seed);
    let c = hash(ix, iy + 1, cells, seed);
    let d = hash(ix + 1, iy + 1, cells, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// The brightness (0..1) and opacity of a texture at (u, v) in 0..1.
fn texel(texture: Texture, u: f32, v: f32) -> (f32, f32) {
    use std::f32::consts::PI;
    match texture {
        Texture::Tolex => {
            // A pebbled grain lit from the top left.
            let h = |u: f32, v: f32| {
                0.6 * noise(u, v, 48, 7) + 0.3 * noise(u, v, 96, 11) + 0.1 * noise(u, v, 24, 3)
            };
            let e = 1.0 / SIZE as f32;
            let dx = h(u + e, v) - h(u - e, v);
            let dy = h(u, v + e) - h(u, v - e);
            let light = 0.5 - 6.0 * (dx + dy);
            (0.72 + 0.28 * light.clamp(0.0, 1.0), 1.0)
        }
        Texture::Tweed => {
            // A twill of light and dark yarns on the diagonal, under lacquer.
            let d = (u + v) * 64.0;
            let yarn = (d * PI).sin() * 0.5 + 0.5;
            let cross = ((u - v) * 192.0 * PI).sin() * 0.5 + 0.5;
            let grain = noise(u, v, 64, 5);
            (0.55 + 0.3 * yarn * (0.6 + 0.4 * cross) + 0.15 * grain, 1.0)
        }
        Texture::Sparkle => {
            let warp = (u * 128.0 * PI).sin().abs();
            let weft = (v * 128.0 * PI).sin().abs();
            let over = if ((u * 128.0) as i32 + (v * 128.0) as i32) % 2 == 0 {
                warp
            } else {
                weft
            };
            let glint = if hash((u * 256.0) as i32, (v * 256.0) as i32, 256, 21) > 0.985 {
                0.35
            } else {
                0.0
            };
            ((0.55 + 0.4 * over + glint).min(1.0), 1.0)
        }
        Texture::Basket => {
            let (cu, cv) = ((u * 32.0) as i32, (v * 32.0) as i32);
            let (fu, fv) = ((u * 32.0).fract(), (v * 32.0).fract());
            let along_u = (cu + cv) % 2 == 0;
            let thread = if along_u {
                (fv * 3.0 * PI).sin().abs()
            } else {
                (fu * 3.0 * PI).sin().abs()
            };
            let edge = (fu * PI).sin().min((fv * PI).sin());
            (
                0.45 + 0.45 * thread * (0.6 + 0.4 * edge) + 0.1 * noise(u, v, 64, 9),
                1.0,
            )
        }
        Texture::Diamond => {
            let a = ((u + v) * 24.0).fract();
            let b = ((u - v) * 24.0).rem_euclid(1.0);
            let ridge = (a - 0.5).abs().max((b - 0.5).abs());
            let line = if ridge > 0.42 { 0.35 } else { 0.0 };
            let weave = ((u * 160.0 * PI).sin() * (v * 160.0 * PI).sin()).abs();
            (0.7 + 0.2 * weave - line, 1.0)
        }
        Texture::Cane => {
            // Strands crossing at three angles, with holes between.
            let s = |x: f32| ((x * 18.0).fract() - 0.5).abs() < 0.18;
            let strands = [s(u), s(u * 0.5 + v * 0.866), s(u * 0.5 - v * 0.866 + 2.0)];
            let n = strands.iter().filter(|&&on| on).count() as f32;
            if n == 0.0 {
                (0.15, 1.0)
            } else {
                (0.6 + 0.12 * n + 0.1 * noise(u, v, 64, 13), 1.0)
            }
        }
        Texture::Perforated => {
            let (fu, fv) = ((u * 40.0).fract() - 0.5, (v * 40.0).fract() - 0.5);
            let r = (fu * fu + fv * fv).sqrt();
            if r < 0.28 {
                (0.05, 1.0)
            } else if r < 0.34 {
                (0.95, 1.0)
            } else {
                (0.7 + 0.1 * noise(u, v, 32, 17), 1.0)
            }
        }
        Texture::Brushed => {
            let streak = 0.6 * noise(u * 0.05, v, 256, 23) + 0.4 * noise(u * 0.02, v, 128, 29);
            (0.78 + 0.22 * streak, 1.0)
        }
    }
}

/// A texture, made the first time it is drawn.
pub fn texture(ctx: &egui::Context, which: Texture) -> TextureHandle {
    let id = egui::Id::unique(("gear-texture", which));
    if let Some(handle) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return handle;
    }
    let mut pixels = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, v) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let (light, alpha) = texel(which, u, v);
            let l = (light.clamp(0.0, 1.0) * 255.0) as u8;
            pixels.extend_from_slice(&[l, l, l, (alpha * 255.0) as u8]);
        }
    }
    let image = ColorImage::from_rgba_unmultiplied([SIZE, SIZE], &pixels);
    let handle = ctx.load_texture(
        format!("gear-{which:?}"),
        image,
        TextureOptions {
            magnification: egui::TextureFilter::Linear,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::Repeat,
            mipmap_mode: None,
        },
    );
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// How big a texture's pattern shows: screen pixels per texture pixel.
fn grain(which: Texture) -> f32 {
    match which {
        Texture::Tolex => 1.0,
        Texture::Tweed => 1.8,
        Texture::Sparkle => 2.6,
        Texture::Basket => 1.1,
        Texture::Diamond => 1.3,
        Texture::Cane => 1.2,
        Texture::Perforated => 0.8,
        Texture::Brushed => 1.0,
    }
}

/// `rect` covered with a texture, its pattern `scale` times its natural size, tinted.
pub fn tile(painter: &Painter, rect: Rect, which: Texture, scale: f32, tint: Color32) {
    let handle = texture(painter.ctx(), which);
    let mut mesh = Mesh::with_texture(handle.id());
    let span = SIZE as f32 * scale * grain(which);
    let uv = Rect::from_min_max(
        pos2(rect.left() / span, rect.top() / span),
        pos2(rect.right() / span, rect.bottom() / span),
    );
    mesh.add_rect_with_uv(rect, uv, tint);
    painter.add(mesh);
}

/// A light falling from the top: brighter above, darker below.
pub fn sheen(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// A soft disc: `inner` at the center fading to `outer` at `radius`.
pub fn glow(painter: &Painter, center: Pos2, radius: f32, inner: Color32, outer: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, inner);
    let n = 40;
    for k in 0..=n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(center + vec2(a.cos(), a.sin()) * radius, outer);
        if k > 0 {
            mesh.add_triangle(0, k, k + 1);
        }
    }
    painter.add(mesh);
}

/// A soft shadow under a box.
pub fn shadow(painter: &Painter, rect: Rect) {
    for k in 0..6 {
        let grow = 2.0 + k as f32 * 3.0;
        painter.rect_filled(
            rect.translate(vec2(0.0, 6.0)).expand(grow),
            8.0 + grow,
            Color32::from_black_alpha(26 - (k * 4) as u8),
        );
    }
}

/// A metal corner protector on a corner of `rect`.
fn corner(painter: &Painter, rect: Rect, at: Pos2, size: f32, metal: Color32) {
    let sx = if at.x <= rect.center().x { 1.0 } else { -1.0 };
    let sy = if at.y <= rect.center().y { 1.0 } else { -1.0 };
    let points = vec![
        at,
        at + vec2(size * sx, 0.0),
        at + vec2(size * sx, size * 0.35 * sy),
        at + vec2(size * 0.35 * sx, size * sy),
        at + vec2(0.0, size * sy),
    ];
    painter.add(egui::Shape::convex_polygon(
        points,
        metal,
        Stroke::new(1.0, Color32::from_black_alpha(120)),
    ));
    painter.circle_filled(
        at + vec2(size * 0.32 * sx, size * 0.32 * sy),
        1.6,
        Color32::from_black_alpha(140),
    );
    painter.line_segment(
        [
            at + vec2(2.0 * sx, 2.0 * sy),
            at + vec2(size * 0.8 * sx, 2.0 * sy),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(90)),
    );
}

/// The materials of a piece of gear.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub covering: (Texture, Color32),
    pub cloth: (Texture, Color32),
    pub panel: Color32,
    pub piping: Option<Color32>,
    pub corners: bool,
    pub knob: KnobStyle,
    pub letters: Color32,
    pub lamp: Color32,
}

/// How the amp's knobs look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobStyle {
    /// Black cap over a numbered metal skirt.
    Skirted,
    /// Black with a gold cap.
    GoldCap,
    /// A pointer.
    ChickenHead,
    /// Knurled black with a white line.
    Knurled,
}

/// A box covered in its material, with its sheen, piping-free.
fn covered(painter: &Painter, rect: Rect, look: &Look) {
    shadow(painter, rect);
    let (texture, tint) = look.covering;
    tile(painter, rect, texture, 1.0, tint);
    sheen(
        painter,
        rect,
        Color32::from_white_alpha(22),
        Color32::from_black_alpha(80),
    );
    bevel(painter, rect, 1.0);
}

/// Edges caught by the light: bright along the top and left, dark along the bottom and right.
pub fn bevel(painter: &Painter, rect: Rect, strength: f32) {
    let light = Color32::from_white_alpha((60.0 * strength) as u8);
    let dark = Color32::from_black_alpha((150.0 * strength) as u8);
    let r = rect.shrink(0.5);
    painter.line_segment([r.left_top(), r.right_top()], Stroke::new(1.5, light));
    painter.line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, light));
    painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(2.0, dark));
    painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.5, dark));
}

/// Cloth inside a frame of covering, with its piping.
fn cloth(painter: &Painter, rect: Rect, look: &Look) {
    let (texture, tint) = look.cloth;
    tile(painter, rect, texture, 1.0, tint);
    // The cloth sinks a little behind the frame: shadowed under its top and left edges.
    let r = rect;
    painter.rect_filled(
        Rect::from_min_size(r.min, vec2(r.width(), 4.0)),
        0.0,
        Color32::from_black_alpha(80),
    );
    painter.rect_filled(
        Rect::from_min_size(r.min, vec2(3.0, r.height())),
        0.0,
        Color32::from_black_alpha(60),
    );
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, Color32::from_black_alpha(140)),
        egui::StrokeKind::Inside,
    );
    if let Some(piping) = look.piping {
        painter.rect_stroke(
            rect.expand(1.5),
            2.0,
            Stroke::new(1.6, piping),
            egui::StrokeKind::Middle,
        );
    }
}

/// Where the head's controls go once it is drawn.
pub struct Head {
    pub panel: Rect,
    pub plate: Rect,
    pub lamp: Pos2,
}

/// An amp head in `rect`: its handle, covering, grille with the name plate, and control panel.
pub fn head(painter: &Painter, rect: Rect, look: &Look, name: &str) -> Head {
    covered(painter, rect, look);
    // The handle, a leather strap with metal ends, on top.
    let strap = Rect::from_center_size(
        pos2(rect.center().x, rect.top() - 6.0),
        vec2(rect.width().min(160.0), 12.0),
    );
    painter.rect_filled(
        strap.translate(vec2(0.0, 2.0)),
        6.0,
        Color32::from_black_alpha(90),
    );
    painter.rect_filled(strap, 6.0, Color32::from_rgb(28, 24, 22));
    painter.line_segment(
        [
            strap.left_center() + vec2(12.0, -2.0),
            strap.right_center() + vec2(-12.0, -2.0),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(30)),
    );
    for end in [strap.left_center(), strap.right_center()] {
        let cap = Rect::from_center_size(end + vec2(0.0, 3.0), vec2(18.0, 12.0));
        painter.rect_filled(cap, 3.0, Color32::from_rgb(170, 172, 176));
        bevel(painter, cap, 1.0);
    }
    let inner = rect.shrink2(vec2(rect.width() * 0.035, rect.height() * 0.09));
    let split = inner.top() + inner.height() * 0.42;
    let grille = Rect::from_min_max(inner.min, pos2(inner.right(), split - 4.0));
    cloth(painter, grille, look);
    let panel = Rect::from_min_max(pos2(inner.left(), split + 4.0), inner.max);
    tile(painter, panel, Texture::Brushed, 1.0, look.panel);
    sheen(
        painter,
        panel,
        Color32::from_white_alpha(45),
        Color32::from_black_alpha(40),
    );
    painter.rect_stroke(
        panel,
        2.0,
        Stroke::new(1.0, Color32::from_black_alpha(140)),
        egui::StrokeKind::Inside,
    );
    // The name, on a plate over the cloth.
    let font = FontId::new(
        grille.height().clamp(14.0, 22.0) * 0.8,
        egui::FontFamily::Proportional,
    );
    let galley = painter.layout_no_wrap(name.to_owned(), font, look.letters);
    let plate = Rect::from_center_size(
        grille.center(),
        vec2(galley.size().x + 28.0, galley.size().y + 10.0),
    );
    painter.rect_filled(
        plate.translate(vec2(0.0, 2.0)),
        4.0,
        Color32::from_black_alpha(90),
    );
    tile(painter, plate, Texture::Brushed, 1.0, plate_color(look));
    painter.rect_stroke(
        plate,
        4.0,
        Stroke::new(1.0, Color32::from_black_alpha(150)),
        egui::StrokeKind::Inside,
    );
    painter.galley(plate.center() - galley.size() / 2.0, galley, look.letters);
    if look.corners {
        let size = rect.height().min(rect.width()) * 0.16;
        for at in [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ] {
            corner(painter, rect, at, size, Color32::from_rgb(150, 152, 156));
        }
    }
    let lamp = pos2(panel.right() - panel.height() * 0.32, panel.center().y);
    Head { panel, plate, lamp }
}

fn plate_color(look: &Look) -> Color32 {
    let [r, g, b, _] = look.panel.to_array();
    Color32::from_rgb(
        r.saturating_sub(30),
        g.saturating_sub(30),
        b.saturating_sub(30),
    )
}

/// The jewel of the pilot lamp, lit.
pub fn lamp(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let [r, g, b, _] = color.to_array();
    glow(
        painter,
        center,
        radius * 2.6,
        Color32::from_rgba_unmultiplied(r, g, b, 90),
        Color32::from_rgba_unmultiplied(r, g, b, 0),
    );
    painter.circle_filled(center, radius + 2.0, Color32::from_rgb(170, 172, 176));
    painter.circle_filled(center, radius, color);
    painter.circle_filled(
        center - vec2(radius * 0.3, radius * 0.35),
        radius * 0.35,
        Color32::from_white_alpha(160),
    );
}

/// A knob of the face at `center`, its pointer at `t` (0 to 1) of its travel.
pub fn knob(
    painter: &Painter,
    center: Pos2,
    diameter: f32,
    t: f32,
    style: KnobStyle,
    letters: Color32,
) {
    let r = diameter / 2.0;
    let angle = std::f32::consts::PI * (0.75 + 1.5 * t.clamp(0.0, 1.0));
    let dir = vec2(angle.cos(), angle.sin());
    glow(
        painter,
        center + vec2(2.0, 4.0),
        r * 1.25,
        Color32::from_black_alpha(110),
        Color32::from_black_alpha(0),
    );
    // The scale around it.
    for k in 0..=10 {
        let a = std::f32::consts::PI * (0.75 + 1.5 * k as f32 / 10.0);
        let d = vec2(a.cos(), a.sin());
        let long = k % 5 == 0;
        painter.line_segment(
            [
                center + d * (r + 3.0),
                center + d * (r + if long { 8.0 } else { 6.0 }),
            ],
            Stroke::new(if long { 1.4 } else { 1.0 }, letters.gamma_multiply(0.8)),
        );
    }
    match style {
        KnobStyle::Skirted => {
            painter.circle_filled(center, r, Color32::from_rgb(190, 192, 196));
            for k in 0..20 {
                let a = k as f32 / 20.0 * std::f32::consts::TAU;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment(
                    [center + d * r * 0.78, center + d * r * 0.95],
                    Stroke::new(1.0, Color32::from_black_alpha(70)),
                );
            }
            painter.circle_filled(center, r * 0.68, Color32::from_rgb(18, 18, 20));
            painter.line_segment(
                [center + dir * r * 0.72, center + dir * r * 0.98],
                Stroke::new(2.0, Color32::from_rgb(20, 20, 22)),
            );
            painter.line_segment(
                [center + dir * r * 0.1, center + dir * r * 0.62],
                Stroke::new(2.0, Color32::from_rgb(235, 235, 235)),
            );
        }
        KnobStyle::GoldCap => {
            painter.circle_filled(center, r, Color32::from_rgb(16, 16, 16));
            painter.circle_filled(center, r * 0.62, Color32::from_rgb(206, 170, 90));
            glow(
                painter,
                center - vec2(r * 0.2, r * 0.25),
                r * 0.5,
                Color32::from_rgba_unmultiplied(255, 240, 200, 110),
                Color32::from_rgba_unmultiplied(255, 240, 200, 0),
            );
            painter.line_segment(
                [center + dir * r * 0.15, center + dir * r * 0.95],
                Stroke::new(2.0, Color32::from_rgb(250, 250, 250)),
            );
        }
        KnobStyle::ChickenHead => {
            painter.circle_filled(center, r * 0.6, Color32::from_rgb(24, 20, 18));
            let side = vec2(-dir.y, dir.x);
            let points = vec![
                center + dir * r * 1.05,
                center + side * r * 0.42 - dir * r * 0.15,
                center - dir * r * 0.55,
                center - side * r * 0.42 - dir * r * 0.15,
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                Color32::from_rgb(30, 26, 22),
                Stroke::new(1.0, Color32::from_white_alpha(40)),
            ));
            painter.line_segment(
                [center, center + dir * r * 0.95],
                Stroke::new(1.6, Color32::from_rgb(240, 236, 226)),
            );
        }
        KnobStyle::Knurled => {
            painter.circle_filled(center, r, Color32::from_rgb(26, 26, 28));
            for k in 0..28 {
                let a = k as f32 / 28.0 * std::f32::consts::TAU;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment(
                    [center + d * r * 0.86, center + d * r],
                    Stroke::new(1.0, Color32::from_white_alpha(25)),
                );
            }
            painter.circle_filled(center, r * 0.8, Color32::from_rgb(34, 34, 38));
            painter.line_segment(
                [center + dir * r * 0.2, center + dir * r * 0.95],
                Stroke::new(2.0, Color32::from_rgb(240, 240, 240)),
            );
        }
    }
    painter.circle_stroke(center, r, Stroke::new(1.0, Color32::from_black_alpha(150)));
}

/// How a cabinet is built: its proportions, its speakers (columns, rows) and their size.
fn cab_build(cab: usize) -> Option<(f32, usize, usize)> {
    Some(match cab {
        0 | 1 => (1.1, 2, 2),
        2 | 3 => (1.75, 2, 1),
        4 | 6 => (1.0, 2, 2),
        5 => (1.15, 1, 1),
        7 => (0.62, 2, 4),
        8 => (0.85, 1, 1),
        _ => return None,
    })
}

/// The width over height of cabinet `cab`, or `None` for a DI or an impulse.
pub fn cab_ratio(cab: usize) -> Option<f32> {
    cab_build(cab).map(|b| b.0)
}

/// A cabinet in `rect` (already at its proportions), dressed in `look`.
pub fn cabinet(painter: &Painter, rect: Rect, cab: usize, look: &Look) {
    let Some((_, columns, rows)) = cab_build(cab) else {
        return;
    };
    covered(painter, rect, look);
    let border = rect.width().min(rect.height()) * 0.075;
    let grille = rect.shrink(border);
    cloth(painter, grille, look);
    // The speakers behind the cloth: darker cones, a lighter surround.
    let painter = painter.with_clip_rect(grille);
    let (w, h) = (
        grille.width() / columns as f32,
        grille.height() / rows as f32,
    );
    for c in 0..columns {
        for r in 0..rows {
            let center = pos2(
                grille.left() + w * (c as f32 + 0.5),
                grille.top() + h * (r as f32 + 0.5),
            );
            let radius = w.min(h) * 0.44;
            glow(
                &painter,
                center,
                radius,
                Color32::from_black_alpha(130),
                Color32::from_black_alpha(40),
            );
            // The surround catches a little light along its top.
            painter.circle_stroke(
                center,
                radius,
                Stroke::new(radius * 0.07, Color32::from_black_alpha(60)),
            );
            painter.circle_stroke(
                center - vec2(0.0, radius * 0.03),
                radius * 0.93,
                Stroke::new(1.0, Color32::from_white_alpha(14)),
            );
            glow(
                &painter,
                center,
                radius * 0.28,
                Color32::from_black_alpha(80),
                Color32::from_black_alpha(0),
            );
        }
    }
    sheen(
        &painter,
        grille,
        Color32::from_white_alpha(14),
        Color32::from_black_alpha(40),
    );
    // A small badge at the top left of the grille.
    let badge = Rect::from_min_size(
        grille.left_top() + vec2(border * 0.6, border * 0.5),
        vec2(64.0, 16.0),
    );
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(badge, 3.0, plate_color(look));
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        "STRADA",
        FontId::proportional(10.0),
        look.letters,
    );
    if look.corners {
        let size = rect.width().min(rect.height()) * 0.1;
        for at in [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ] {
            corner(&painter, rect, at, size, Color32::from_rgb(150, 152, 156));
        }
    }
}

/// A pedalboard: a dark plain board, with a light outline where each pedal can stand.
pub fn board(painter: &Painter, rect: Rect, places: &[Rect]) {
    painter.rect_filled(rect, 8.0, Color32::from_rgb(22, 23, 26));
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, Color32::from_white_alpha(14)),
        egui::StrokeKind::Inside,
    );
    for place in places {
        painter.rect_stroke(
            place.expand(3.0),
            11.0,
            Stroke::new(1.0, Color32::from_white_alpha(20)),
            egui::StrokeKind::Inside,
        );
    }
}
