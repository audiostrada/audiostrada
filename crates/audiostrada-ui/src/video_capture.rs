//! Development aid: with `AUDIOSTRADA_RECORD=<file>`, records the window as a stream of PPM
//! frames for ffmpeg (`-f image2pipe -c:v ppm`), at `AUDIOSTRADA_RECORD_FPS` frames a second
//! (30), from `AUDIOSTRADA_RECORD_AFTER` seconds after the start (10) for
//! `AUDIOSTRADA_RECORD_SECONDS` (10), then quits. `AUDIOSTRADA_RECORD_RECT=x,y,w,h` (points)
//! crops it; with `AUDIOSTRADA_SCREENSHOT_WINDOW` set, the window on top is recorded alone.
//! With `AUDIOSTRADA_SCREENSHOT_EFFECT`, the effect's editor is recorded alone. Recording one
//! window, the script's positions count from its corner once the recording starts.
//! `AUDIOSTRADA_RECORD_SCRIPT=<file.json>` plays gestures and commands, timed from the start of
//! the recording, and draws a mouse cursor where they point.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, Pos2, Rect, pos2};
use serde::Deserialize;

/// One step of the script: at `t` seconds, move to `to` (over `dur` seconds), press, release
/// or click, hold modifiers (`"alt"`, `"ctrl+shift"`, `""`), or run a command by its name.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Step {
    t: f32,
    to: Option<[f32; 2]>,
    dur: f32,
    down: bool,
    up: bool,
    click: bool,
    double: bool,
    mods: Option<String>,
    action: Option<String>,
}

#[derive(Debug)]
struct Output {
    file: std::io::BufWriter<std::fs::File>,
    written: u64,
}

#[derive(Debug)]
pub(crate) struct Video {
    start: Instant,
    after: f32,
    seconds: f32,
    fps: f32,
    rect: Option<Rect>,
    window: bool,
    steps: Vec<Step>,
    next: usize,
    scripted: bool,
    pos: Pos2,
    moving: Option<(f32, f32, Pos2, Pos2)>,
    held: bool,
    mods: egui::Modifiers,
    actions: Vec<String>,
    area: Option<Rect>,
    /// Where the recorded window starts, in points: the script's positions count from there.
    origin: egui::Vec2,
    out: Arc<Mutex<Output>>,
}

fn number(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn modifiers(text: &str) -> egui::Modifiers {
    let mut m = egui::Modifiers::NONE;
    for part in text.split('+') {
        match part.trim() {
            "alt" => m.alt = true,
            "ctrl" => {
                m.ctrl = true;
                m.command = true;
            }
            "shift" => m.shift = true,
            _ => {}
        }
    }
    m
}

impl Video {
    pub(crate) fn from_env() -> Option<Self> {
        let path = std::env::var_os("AUDIOSTRADA_RECORD")?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .ok()?;
        let rect = std::env::var("AUDIOSTRADA_RECORD_RECT").ok().and_then(|r| {
            let v: Vec<f32> = r.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 4).then(|| Rect::from_min_size(pos2(v[0], v[1]), egui::vec2(v[2], v[3])))
        });
        let steps: Vec<Step> = std::env::var_os("AUDIOSTRADA_RECORD_SCRIPT")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Some(Self {
            start: Instant::now(),
            after: number("AUDIOSTRADA_RECORD_AFTER", 10.0),
            seconds: number("AUDIOSTRADA_RECORD_SECONDS", 10.0),
            fps: number("AUDIOSTRADA_RECORD_FPS", 30.0),
            rect,
            window: std::env::var_os("AUDIOSTRADA_SCREENSHOT_WINDOW").is_some(),
            scripted: !steps.is_empty(),
            steps,
            next: 0,
            pos: pos2(-100.0, -100.0),
            moving: None,
            held: false,
            mods: egui::Modifiers::NONE,
            actions: Vec::new(),
            area: None,
            origin: egui::Vec2::ZERO,
            out: Arc::new(Mutex::new(Output {
                file: std::io::BufWriter::new(file),
                written: 0,
            })),
        })
    }

    /// Seconds since the recording started, negative before.
    fn time(&self) -> f32 {
        self.start.elapsed().as_secs_f32() - self.after
    }

    fn button(&self, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: self.pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: self.mods,
        }
    }

    /// The script's events due by now, for the main window's next frame.
    pub(crate) fn input(&mut self) -> Vec<egui::Event> {
        let t = self.time();
        let mut events = Vec::new();
        if let Some((from, dur, a, b)) = self.moving {
            let k = ((t - from) / dur.max(0.001)).clamp(0.0, 1.0);
            self.pos = a.lerp(b, k * k * (3.0 - 2.0 * k));
            events.push(egui::Event::PointerMoved(self.pos));
            if k >= 1.0 {
                self.moving = None;
            }
        }
        while let Some(step) = self.steps.get(self.next).filter(|s| s.t <= t) {
            self.next += 1;
            if let Some(m) = &step.mods {
                self.mods = modifiers(m);
                events.push(egui::Event::ModifiersChanged(self.mods));
            }
            if let Some([x, y]) = step.to {
                let to = pos2(x, y) + self.origin;
                if step.dur > 0.0 {
                    self.moving = Some((step.t, step.dur, self.pos, to));
                } else {
                    self.pos = to;
                    events.push(egui::Event::PointerMoved(self.pos));
                }
            }
            let presses = if step.double {
                2
            } else {
                usize::from(step.click)
            };
            for _ in 0..presses {
                events.push(self.button(true));
                events.push(self.button(false));
            }
            if step.down {
                self.held = true;
                events.push(self.button(true));
            }
            if step.up {
                self.held = false;
                events.push(self.button(false));
            }
            if let Some(action) = &step.action {
                self.actions.push(action.clone());
            }
        }
        events
    }

    /// Draws the cursor and asks for this frame's picture; gives the commands due.
    pub(crate) fn tick(&mut self, ctx: &egui::Context) -> Vec<String> {
        ctx.request_repaint();
        if self.window {
            ctx.set_embed_viewports(true);
        }
        let t = self.time();
        if t > self.seconds {
            if let Ok(mut out) = self.out.lock() {
                let _ = out.file.flush();
            }
            std::process::exit(0);
        }
        if self.scripted {
            self.paint_cursor(ctx);
        }
        if t >= 0.0 {
            let scale = ctx.pixels_per_point();
            if self.area.is_none() && crate::effect_shot::active() {
                let Some(area) = crate::effect_shot::area() else {
                    return std::mem::take(&mut self.actions);
                };
                self.area = Some(area * scale);
            }
            let area = *self.area.get_or_insert_with(|| {
                let top = self.window.then(|| {
                    ctx.memory(|m| {
                        let areas = m.areas();
                        let top = areas
                            .top_layer_id(egui::Order::Foreground)
                            .or_else(|| areas.top_layer_id(egui::Order::Middle))?;
                        m.area_rect(top.id)
                    })
                });
                self.rect
                    .or(top.flatten())
                    .unwrap_or_else(|| ctx.content_rect())
                    * scale
            });
            if self.window || crate::effect_shot::active() {
                self.origin = area.min.to_vec2() / scale;
            }
            let (out, fps) = (Arc::clone(&self.out), self.fps);
            ctx.request_screenshot(move |image| {
                let Ok(mut out) = out.lock() else {
                    return;
                };
                let due = (t * fps) as u64 + 1;
                if out.written >= due {
                    return;
                }
                let frame = ppm(&image, area);
                while out.written < due {
                    let _ = out.file.write_all(&frame);
                    out.written += 1;
                }
            });
        }
        std::mem::take(&mut self.actions)
    }

    fn paint_cursor(&self, ctx: &egui::Context) {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Debug,
            egui::Id::unique("demo-cursor"),
        ));
        let p = self.pos;
        let at = |x: f32, y: f32| p + egui::vec2(x, y) * 1.15;
        if self.held {
            painter.circle_filled(
                p,
                13.0,
                egui::Color32::from_rgba_unmultiplied(90, 162, 255, 70),
            );
        }
        let outline = vec![
            at(0.0, 0.0),
            at(0.0, 17.0),
            at(4.2, 13.2),
            at(7.0, 19.6),
            at(9.8, 18.4),
            at(7.1, 12.2),
            at(12.4, 12.2),
        ];
        let fill = egui::Color32::WHITE;
        painter.add(egui::Shape::convex_polygon(
            vec![at(0.0, 0.0), at(0.0, 17.0), at(12.4, 12.2)],
            fill,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::convex_polygon(
            vec![at(4.2, 13.2), at(7.0, 19.6), at(9.8, 18.4), at(7.1, 12.2)],
            fill,
            egui::Stroke::NONE,
        ));
        painter.add(egui::Shape::closed_line(
            outline,
            egui::Stroke::new(1.3, egui::Color32::BLACK),
        ));
    }
}

/// The part `area` (pixels) of `image`, as a PPM frame.
fn ppm(image: &egui::ColorImage, area: Rect) -> Vec<u8> {
    let [w, h] = image.size;
    let full = Rect::from_min_size(Pos2::ZERO, egui::vec2(w as f32, h as f32));
    let r = area.round().intersect(full);
    let (x0, y0, x1, y1) = (
        r.left() as usize,
        r.top() as usize,
        r.right() as usize,
        r.bottom() as usize,
    );
    let mut frame = format!("P6\n{} {}\n255\n", x1 - x0, y1 - y0).into_bytes();
    for y in y0..y1 {
        frame.extend(
            image.pixels[y * w + x0..y * w + x1]
                .iter()
                .flat_map(|p| [p.r(), p.g(), p.b()]),
        );
    }
    frame
}
