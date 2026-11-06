//! Pictures of a sound for agents, as PNG: its spectrum (several sounds at once), its
//! spectrogram, and its waveform over its short-term loudness. Dark, with readable labels.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use audiostrada_dsp::report::{BANDS, Report};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke, Transform};

const W: u32 = 1200;
const H: u32 = 640;
const SURFACE: u32 = 0x141920;
const GRID: u32 = 0x262d38;
const AXIS: u32 = 0x3a4350;
const TEXT: u32 = 0xe9edf2;
const DIM: u32 = 0x98a2b3;
/// Series colors, in this order (validated for the dark surface).
pub const SERIES: [u32; 3] = [0x3987e5, 0xd95926, 0x199e70];
/// Plot area.
const LEFT: f32 = 78.0;
const RIGHT: f32 = 1170.0;
const TOP: f32 = 88.0;
const BOTTOM: f32 = 572.0;

fn color(hex: u32) -> Color {
    Color::from_rgba8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

struct Canvas {
    pix: Pixmap,
    font: FontRef<'static>,
}

impl Canvas {
    fn new() -> Self {
        let mut pix = Pixmap::new(W, H).expect("canvas");
        pix.fill(color(SURFACE));
        let font = FontRef::try_from_slice(include_bytes!("../assets/fonts/Inter-Regular.ttf"))
            .expect("font");
        Self { pix, font }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, hex: u32) {
        // Whole pixels, without anti-aliasing: crisp grid lines and cells.
        let (x0, y0) = (x.round(), y.round());
        let (x1, y1) = ((x + w).round().max(x0 + 1.0), (y + h).round().max(y0 + 1.0));
        if let Some(r) = Rect::from_ltrb(x0, y0, x1, y1) {
            let mut paint = Paint {
                anti_alias: false,
                ..Paint::default()
            };
            paint.set_color(color(hex));
            self.pix.fill_rect(r, &paint, Transform::identity(), None);
        }
    }

    fn line(&mut self, points: &[(f32, f32)], hex: u32, width: f32) {
        let points: Vec<(f32, f32)> = points
            .iter()
            .copied()
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .collect();
        if points.len() < 2 {
            return;
        }
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in points.iter().enumerate() {
            if i == 0 {
                pb.move_to(*x, *y);
            } else {
                pb.line_to(*x, *y);
            }
        }
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color(color(hex));
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            line_join: tiny_skia::LineJoin::Round,
            line_cap: tiny_skia::LineCap::Round,
            ..Stroke::default()
        };
        self.pix
            .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        let font = self.font.as_scaled(PxScale::from(size));
        text.chars()
            .map(|c| font.h_advance(self.font.glyph_id(c)))
            .sum()
    }

    /// `text` with its baseline at `y`, starting `align` (0 left, 0.5 centred, 1 right) of `x`.
    fn text(&mut self, x: f32, y: f32, size: f32, hex: u32, text: &str, align: f32) {
        let scale = PxScale::from(size);
        let scaled = self.font.as_scaled(scale);
        let mut pen = x - self.width(text, size) * align;
        let [r, g, b] = [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8];
        let (w, h) = (self.pix.width() as i32, self.pix.height() as i32);
        for c in text.chars() {
            let id = self.font.glyph_id(c);
            let glyph = id.with_scale_and_position(scale, point(pen, y));
            pen += scaled.h_advance(id);
            let Some(outline) = self.font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            let pixels = self.pix.pixels_mut();
            outline.draw(|gx, gy, cover| {
                let (px, py) = (
                    bounds.min.x as i32 + gx as i32,
                    bounds.min.y as i32 + gy as i32,
                );
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let at = (py * w + px) as usize;
                let under = pixels[at];
                let mix = |a: u8, b: u8| {
                    (f32::from(a) * cover + f32::from(b) * (1.0 - cover)).round() as u8
                };
                if let Some(c) = PremultipliedColorU8::from_rgba(
                    mix(r, under.red()),
                    mix(g, under.green()),
                    mix(b, under.blue()),
                    255,
                ) {
                    pixels[at] = c;
                }
            });
        }
    }

    fn png(self) -> Vec<u8> {
        let data = self.pix.data().to_vec();
        let mut out = Vec::new();
        if let Some(image) = image::RgbaImage::from_raw(W, H, data) {
            let _ = image.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
        }
        out
    }

    fn title(&mut self, title: &str, subtitle: &str) {
        self.text(LEFT, 40.0, 22.0, TEXT, title, 0.0);
        self.text(LEFT, 66.0, 15.0, DIM, subtitle, 0.0);
    }

    /// Names with their color, at the top right.
    fn legend(&mut self, names: &[(String, u32)]) {
        let mut x = RIGHT;
        for (name, hex) in names.iter().rev() {
            let w = self.width(name, 15.0);
            x -= w;
            self.text(x, 66.0, 15.0, DIM, name, 0.0);
            self.rect(x - 18.0, 56.0, 11.0, 11.0, *hex);
            x -= 36.0;
        }
    }
}

fn x_of_hz(hz: f32) -> f32 {
    LEFT + (hz / 20.0).log2() / (1000.0f32).log2() * (RIGHT - LEFT)
}

fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{}k", hz / 1000.0)
    } else {
        format!("{hz}")
    }
}

const HZ_TICKS: [f32; 10] = [
    20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
];

/// The spectra of up to three sounds, from 20 Hz to 20 kHz, in dB against the loudest.
pub fn spectrum(sounds: &[(String, &Report)], subtitle: &str) -> Vec<u8> {
    let mut c = Canvas::new();
    let top = sounds
        .iter()
        .flat_map(|(_, r)| r.curve.iter().map(|p| p.1 + r.rms))
        .fold(-200.0f32, f32::max);
    let top = (top / 6.0).ceil() * 6.0;
    let floor = top - 60.0;
    let y_of = |db: f32| BOTTOM - ((db - floor) / 60.0).clamp(0.0, 1.0) * (BOTTOM - TOP);
    for k in 0..=5 {
        let db = floor + 12.0 * k as f32;
        c.rect(LEFT, y_of(db), RIGHT - LEFT, 1.0, GRID);
        c.text(
            LEFT - 10.0,
            y_of(db) + 5.0,
            14.0,
            DIM,
            &format!("{:.0}", db - top),
            1.0,
        );
    }
    for hz in HZ_TICKS {
        c.rect(x_of_hz(hz), TOP, 1.0, BOTTOM - TOP, GRID);
        c.text(x_of_hz(hz), BOTTOM + 22.0, 14.0, DIM, &hz_label(hz), 0.5);
    }
    // The bands' names, between their edges.
    for (name, lo, hi) in BANDS {
        c.text(
            (x_of_hz(lo) + x_of_hz(hi.min(20_000.0))) / 2.0,
            TOP + 18.0,
            13.0,
            DIM,
            name,
            0.5,
        );
        c.rect(x_of_hz(lo), TOP, 1.0, 26.0, AXIS);
    }
    c.rect(LEFT, BOTTOM, RIGHT - LEFT, 1.0, AXIS);
    c.text(LEFT - 10.0, TOP - 8.0, 13.0, DIM, "dB", 1.0);
    for (k, (_, r)) in sounds.iter().enumerate() {
        let points: Vec<(f32, f32)> = r
            .curve
            .iter()
            .map(|(hz, db)| (x_of_hz(*hz), y_of(db + r.rms)))
            .collect();
        c.line(&points, SERIES[k % SERIES.len()], 2.0);
    }
    let names: Vec<String> = sounds.iter().map(|(n, _)| n.clone()).collect();
    c.title(&format!("Spectrum: {}", names.join(" / ")), subtitle);
    if sounds.len() > 1 {
        c.legend(
            &names
                .iter()
                .enumerate()
                .map(|(k, n)| (n.clone(), SERIES[k % SERIES.len()]))
                .collect::<Vec<_>>(),
        );
    }
    c.png()
}

/// One hue, from the surface to light: louder is lighter.
fn ramp(t: f32) -> u32 {
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [20.0, 25.0, 32.0]),
        (0.35, [27.0, 58.0, 99.0]),
        (0.6, [47.0, 109.0, 179.0]),
        (0.82, [111.0, 168.0, 238.0]),
        (1.0, [222.0, 236.0, 255.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    let k = STOPS
        .iter()
        .position(|s| s.0 >= t)
        .unwrap_or(STOPS.len() - 1)
        .max(1);
    let ((a, ca), (b, cb)) = (STOPS[k - 1], STOPS[k]);
    let f = if b > a { (t - a) / (b - a) } else { 0.0 };
    let v = |i: usize| (ca[i] + (cb[i] - ca[i]) * f).round() as u32;
    (v(0) << 16) | (v(1) << 8) | v(2)
}

/// The cells of a spectrogram: `columns` by `rows` (lowest first), from `low` to `high` Hz.
pub struct Cells<'a> {
    pub values: &'a [f32],
    pub columns: usize,
    pub rows: usize,
    pub low: f32,
    pub high: f32,
}

/// Time across, frequency up (octaves), louder lighter.
pub fn spectrogram(name: &str, grid: &Cells, start: f64, seconds: f64, subtitle: &str) -> Vec<u8> {
    let Cells {
        values: cells,
        columns,
        rows,
        low,
        high,
    } = *grid;
    let mut c = Canvas::new();
    let (cw, rh) = (
        (RIGHT - LEFT) / columns as f32,
        (BOTTOM - TOP) / rows as f32,
    );
    for r in 0..rows {
        for col in 0..columns {
            let db = cells[r * columns + col];
            c.rect(
                LEFT + col as f32 * cw,
                BOTTOM - (r + 1) as f32 * rh,
                cw + 0.6,
                rh + 0.6,
                ramp(1.0 + db / 72.0),
            );
        }
    }
    let y_of = |hz: f32| BOTTOM - (hz / low).log2() / (high / low).log2() * (BOTTOM - TOP);
    for hz in HZ_TICKS.into_iter().filter(|h| *h >= low && *h <= high) {
        c.text(LEFT - 10.0, y_of(hz) + 5.0, 14.0, DIM, &hz_label(hz), 1.0);
        c.rect(LEFT - 6.0, y_of(hz), 6.0, 1.0, AXIS);
    }
    c.text(LEFT - 10.0, TOP - 8.0, 13.0, DIM, "Hz", 1.0);
    time_axis(&mut c, start, seconds);
    c.title(&format!("Spectrogram: {name}"), subtitle);
    // The scale of the colours.
    for k in 0..120 {
        c.rect(
            RIGHT - 240.0 + 2.0 * k as f32,
            54.0,
            2.0,
            12.0,
            ramp(k as f32 / 119.0),
        );
    }
    c.text(RIGHT - 248.0, 66.0, 13.0, DIM, "-72 dB", 1.0);
    c.text(RIGHT + 4.0, 66.0, 13.0, DIM, "0", 0.0);
    c.png()
}

fn time_axis(c: &mut Canvas, start: f64, seconds: f64) {
    let step = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0]
        .into_iter()
        .find(|s| seconds / s <= 12.0)
        .unwrap_or(300.0);
    let mut t = (start / step).ceil() * step;
    while t <= start + seconds + 1e-6 {
        let x = LEFT + ((t - start) / seconds.max(1e-9)) as f32 * (RIGHT - LEFT);
        let label = if t >= 60.0 {
            format!("{}:{:02}", (t / 60.0) as u32, (t % 60.0).round() as u32)
        } else {
            format!("{t:.0}s")
        };
        c.text(x, BOTTOM + 22.0, 14.0, DIM, &label, 0.5);
        c.rect(x, BOTTOM, 1.0, 6.0, AXIS);
        t += step;
    }
}

/// The waveform above, the short-term loudness below, on one time axis.
pub fn waveform(
    name: &str,
    envelope: &[(f32, f32)],
    loudness: &[f32],
    start: f64,
    seconds: f64,
    reference: Option<f32>,
    subtitle: &str,
) -> Vec<u8> {
    let mut c = Canvas::new();
    let split = TOP + (BOTTOM - TOP) * 0.55;
    let (wave_top, wave_bottom) = (TOP, split - 18.0);
    let mid = (wave_top + wave_bottom) / 2.0;
    let half = (wave_bottom - wave_top) / 2.0;
    for (v, label) in [
        (1.0, "0"),
        (0.5, "-6"),
        (0.0, ""),
        (-0.5, "-6"),
        (-1.0, "0"),
    ] {
        let y = mid - v * half;
        c.rect(
            LEFT,
            y,
            RIGHT - LEFT,
            1.0,
            if v == 0.0 { AXIS } else { GRID },
        );
        c.text(LEFT - 10.0, y + 5.0, 13.0, DIM, label, 1.0);
    }
    c.text(LEFT - 10.0, wave_top - 8.0, 13.0, DIM, "dBFS", 1.0);
    let cw = (RIGHT - LEFT) / envelope.len().max(1) as f32;
    for (k, (lo, hi)) in envelope.iter().enumerate() {
        let (y1, y2) = (
            mid - hi.clamp(-1.0, 1.0) * half,
            mid - lo.clamp(-1.0, 1.0) * half,
        );
        let clipping = hi.max(-lo) >= 0.999;
        c.rect(
            LEFT + k as f32 * cw,
            y1,
            cw.max(1.0),
            (y2 - y1).max(1.0),
            if clipping { SERIES[1] } else { SERIES[0] },
        );
    }
    // Loudness: from 24 dB under its loudest to a little above it.
    let (lo_top, lo_bottom) = (split + 18.0, BOTTOM);
    let loudest = loudness
        .iter()
        .copied()
        .filter(|v| *v > -70.0)
        .fold(-60.0f32, f32::max);
    let high = ((loudest.max(reference.unwrap_or(-60.0)) + 3.0) / 6.0).ceil() * 6.0;
    let low = high - 30.0;
    let y_of =
        |v: f32| lo_bottom - ((v - low) / (high - low)).clamp(0.0, 1.0) * (lo_bottom - lo_top);
    for k in 0..=5 {
        let v = low + 6.0 * k as f32;
        c.rect(LEFT, y_of(v), RIGHT - LEFT, 1.0, GRID);
        c.text(
            LEFT - 10.0,
            y_of(v) + 5.0,
            13.0,
            DIM,
            &format!("{v:.0}"),
            1.0,
        );
    }
    c.text(LEFT - 10.0, lo_top - 8.0, 13.0, DIM, "LUFS", 1.0);
    if let Some(r) = reference {
        c.rect(LEFT, y_of(r), RIGHT - LEFT, 1.0, DIM);
        c.text(
            RIGHT,
            y_of(r) - 6.0,
            13.0,
            DIM,
            &format!("{r:.0} LUFS, streaming"),
            1.0,
        );
    }
    let points: Vec<(f32, f32)> = loudness
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > -70.0)
        .map(|(k, v)| {
            (
                LEFT + (k as f32 + 0.5) * (RIGHT - LEFT) / loudness.len().max(1) as f32,
                y_of(*v),
            )
        })
        .collect();
    c.line(&points, SERIES[2], 2.0);
    c.rect(LEFT, BOTTOM, RIGHT - LEFT, 1.0, AXIS);
    time_axis(&mut c, start, seconds);
    c.title(
        &format!("Waveform and short-term loudness: {name}"),
        subtitle,
    );
    c.png()
}

/// Standard base64, for the MCP image content.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_as_the_standard_says() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_pictures_are_png() {
        let rate = 48_000;
        let x: Vec<f32> = (0..4 * rate)
            .map(|i| 0.3 * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect();
        let r = audiostrada_dsp::report::report(&x, &x, rate as u32);
        let png = spectrum(&[("a".into(), &r)], "test");
        assert_eq!(&png[1..4], b"PNG");
        let cells =
            audiostrada_dsp::report::spectrogram(&x, &x, rate as u32, 50, 40, 30.0, 16_000.0);
        assert_eq!(
            &spectrogram(
                "a",
                &Cells {
                    values: &cells,
                    columns: 50,
                    rows: 40,
                    low: 30.0,
                    high: 16_000.0
                },
                0.0,
                4.0,
                ""
            )[1..4],
            b"PNG"
        );
        let env = audiostrada_dsp::report::envelope(&x, &x, 100);
        let lufs = audiostrada_dsp::report::loudness_curve(&x, &x, rate as u32, 100);
        assert_eq!(
            &waveform("a", &env, &lufs, 0.0, 4.0, Some(-14.0), "")[1..4],
            b"PNG"
        );
    }
}
