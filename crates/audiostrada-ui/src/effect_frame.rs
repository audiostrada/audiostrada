//! The frame of an effect's window, for built-in effects and plugins alike: the top bar (logo,
//! name, presets, A/B, Bypass), the preset panel, and the bottom bar (sidechain, levels,
//! output, Match).

use std::collections::BTreeMap;
use std::rc::Rc;

use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{Insert, InsertId};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Painter, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};

use crate::document::Document;
use crate::effect_editors::{menu_order, preset_category};
use crate::i18n::tr;
use crate::presets::Preset;
use crate::theme::{self, Palette};
use crate::trf;

/// Heights of the bars, their edge included.
const TOP: f32 = 57.0;
const BOTTOM: f32 = 45.0;
/// Rest of the pointer on a preset before it is heard, in seconds.
const HOVER_REST: f64 = 0.12;

// Colors and shapes shared with the effect browser.

/// The bars at the top and bottom of the windows.
pub(crate) fn bar_fill(p: &Palette) -> Color32 {
    if p.dark {
        theme::mix(p.panel, p.surface, 0.3)
    } else {
        Color32::WHITE
    }
}

/// Text fields and value boxes.
pub(crate) fn field_fill(p: &Palette) -> Color32 {
    if p.dark {
        theme::mix(p.base, Color32::BLACK, 0.25)
    } else {
        Color32::WHITE
    }
}

/// The line between a bar and the content.
pub(crate) fn line_edge(p: &Palette) -> Color32 {
    if p.dark {
        p.border
    } else {
        theme::mix(p.border, Color32::WHITE, 0.5)
    }
}

pub(crate) fn button_edge(p: &Palette) -> Color32 {
    if p.dark {
        p.border
    } else {
        theme::mix(p.border, Color32::WHITE, 0.3)
    }
}

pub(crate) fn radius(p: &Palette) -> f32 {
    f32::from(p.look.radius) + 2.0
}

/// A row chosen in a list, as the sidebars show it.
pub(crate) fn chosen_fill(p: &Palette) -> Color32 {
    if p.dark {
        theme::mix(bar_fill(p), p.accent, 0.2)
    } else {
        theme::mix(Color32::WHITE, p.accent, 0.14)
    }
}

/// The count beside a chosen row.
pub(crate) fn chosen_count(p: &Palette) -> Color32 {
    if p.dark {
        theme::mix(p.accent, Color32::WHITE, 0.7)
    } else {
        p.accent
    }
}

/// Gold of the favorites' stars.
pub(crate) fn star_color(p: &Palette) -> Color32 {
    if p.dark {
        theme::SOLO
    } else {
        theme::mix(theme::SOLO, Color32::BLACK, 0.3)
    }
}

/// How a button looks at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Face {
    /// A raised button with an edge.
    Plain,
    /// Only its icon, a background under the pointer.
    Ghost,
    /// Filled with a color.
    Lit(Color32),
}

pub(crate) fn paint_face(
    ui: &Ui,
    rect: Rect,
    corners: impl Into<CornerRadius>,
    response: &Response,
    face: Face,
) {
    let p = theme::current(ui);
    let down = response.is_pointer_button_down_on();
    let hovered = response.hovered();
    let (fill, stroke) = match face {
        Face::Plain => (
            if down {
                p.pressed
            } else if hovered {
                p.hover
            } else {
                p.surface
            },
            Stroke::new(1.0, button_edge(&p)),
        ),
        Face::Ghost => (
            if down {
                p.pressed
            } else if hovered {
                p.hover
            } else {
                Color32::TRANSPARENT
            },
            Stroke::NONE,
        ),
        Face::Lit(c) => (
            if hovered {
                theme::mix(c, Color32::WHITE, 0.1)
            } else {
                c
            },
            Stroke::new(1.0, theme::mix(c, Color32::BLACK, 0.25)),
        ),
    };
    ui.painter()
        .rect(rect, corners, fill, stroke, StrokeKind::Inside);
}

/// A button at `rect`; when disabled it only shows its hint.
pub(crate) fn button(ui: &Ui, rect: Rect, id: egui::Id, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    ui.interact(rect, id, sense)
}

/// A button holding a text, `face` at rest; ink chosen to read on it.
pub(crate) fn text_button(
    ui: &Ui,
    rect: Rect,
    id: egui::Id,
    text: &str,
    face: Face,
    corners: impl Into<CornerRadius>,
) -> Response {
    let p = theme::current(ui);
    let response = button(ui, rect, id, true);
    paint_face(ui, rect, corners, &response, face);
    let ink = match face {
        Face::Lit(c) => theme::text_on(c),
        _ => p.text,
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme::strong(12.0),
        ink,
    );
    response
}

/// A square button with a glyph.
pub(crate) fn glyph_button(
    ui: &Ui,
    rect: Rect,
    id: egui::Id,
    glyph: Glyph,
    color: Option<Color32>,
    enabled: bool,
) -> Response {
    let p = theme::current(ui);
    let response = button(ui, rect, id, enabled);
    let mut painter = ui.painter().clone();
    if !enabled {
        painter.multiply_opacity(0.4);
    }
    if enabled {
        paint_face(ui, rect, radius(&p), &response, Face::Ghost);
    }
    let ink = color.unwrap_or(if response.hovered() { p.text } else { p.weak });
    paint_glyph(
        &painter,
        Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
        glyph,
        ink,
    );
    response
}

/// The line icons of the bars, drawn on a 24 by 24 grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Glyph {
    Left,
    Right,
    Down,
    /// Filled or not.
    Star(bool),
    Save,
    Copy,
    External,
    Key,
    Search,
    Close,
}

pub(crate) fn paint_glyph(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - vec2(12.0, 12.0) * s;
    let at = |x: f32, y: f32| o + vec2(x, y) * s;
    let width = if matches!(glyph, Glyph::Left | Glyph::Right | Glyph::Down) {
        2.5
    } else {
        2.1
    };
    let stroke = Stroke::new((width * s).max(1.1), color);
    let line = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    match glyph {
        Glyph::Left => line(&[(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)]),
        Glyph::Right => line(&[(9.0, 5.0), (16.0, 12.0), (9.0, 19.0)]),
        Glyph::Down => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Glyph::Star(filled) => {
            let outline: Vec<Pos2> = [
                (12.0, 3.0),
                (14.7, 8.6),
                (20.8, 9.4),
                (16.3, 13.6),
                (17.4, 19.6),
                (12.0, 16.6),
                (6.6, 19.6),
                (7.7, 13.6),
                (3.2, 9.4),
                (9.3, 8.6),
            ]
            .iter()
            .map(|&(x, y)| at(x, y))
            .collect();
            if filled {
                // A fan from the center: the star is not convex.
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(at(12.0, 12.4), color);
                for &q in &outline {
                    mesh.colored_vertex(q, color);
                }
                for i in 0..10u32 {
                    mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
                }
                painter.add(Shape::mesh(mesh));
            } else {
                painter.add(Shape::closed_line(outline, stroke));
            }
        }
        Glyph::Save => {
            painter.add(Shape::closed_line(
                [
                    (5.0, 3.0),
                    (16.0, 3.0),
                    (19.0, 6.0),
                    (19.0, 21.0),
                    (5.0, 21.0),
                ]
                .iter()
                .map(|&(x, y)| at(x, y))
                .collect(),
                stroke,
            ));
            line(&[(8.0, 3.0), (8.0, 8.0), (15.0, 8.0), (15.0, 3.0)]);
            line(&[(8.0, 21.0), (8.0, 14.0), (16.0, 14.0), (16.0, 21.0)]);
        }
        Glyph::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(at(8.0, 8.0), at(20.0, 20.0)),
                2.0 * s,
                stroke,
                StrokeKind::Middle,
            );
            line(&[(4.0, 16.0), (4.0, 5.0), (5.0, 4.0), (16.0, 4.0)]);
        }
        Glyph::External => {
            line(&[(14.0, 4.0), (20.0, 4.0), (20.0, 10.0)]);
            line(&[(20.0, 4.0), (11.0, 13.0)]);
            line(&[
                (18.0, 14.0),
                (18.0, 20.0),
                (4.0, 20.0),
                (4.0, 6.0),
                (10.0, 6.0),
            ]);
        }
        Glyph::Key => {
            painter.circle_stroke(at(8.0, 15.0), 4.0 * s, stroke);
            line(&[(11.0, 12.0), (20.0, 3.0)]);
            line(&[(17.0, 6.0), (20.0, 9.0)]);
        }
        Glyph::Search => {
            painter.circle_stroke(at(11.0, 11.0), 7.0 * s, stroke);
            line(&[(20.0, 20.0), (16.5, 16.5)]);
        }
        Glyph::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
    }
}

/// A key cap, its left side centered on `left`; returns its width.
pub(crate) fn keycap(painter: &Painter, p: &Palette, left: Pos2, text: &str) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), theme::strong(10.5), p.text);
    let rect = Rect::from_min_size(
        pos2(left.x, left.y - 10.0),
        vec2((galley.size().x + 10.0).max(22.0), 20.0),
    );
    painter.rect(
        rect,
        4.0,
        p.surface,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            rect.left_bottom() + vec2(1.0, -1.0),
            rect.right_bottom() + vec2(-1.0, -1.0),
        ],
        Stroke::new(1.0, p.border),
    );
    let size = galley.size();
    painter.galley(rect.center() - size / 2.0 - vec2(0.0, 1.0), galley, p.text);
    rect.width()
}

/// Key caps followed by what they do, one group after the other from `left`; returns where
/// the last one ends.
pub(crate) fn key_hints(
    painter: &Painter,
    p: &Palette,
    left: Pos2,
    hints: &[(&[&str], &str)],
) -> f32 {
    let mut x = left.x;
    for (keys, what) in hints {
        for key in *keys {
            x += keycap(painter, p, pos2(x, left.y), key) + 4.0;
        }
        x += 2.0;
        let r = painter.text(
            pos2(x, left.y),
            Align2::LEFT_CENTER,
            *what,
            FontId::proportional(11.5),
            p.weak,
        );
        x = r.right() + 16.0;
    }
    x
}

/// The width [`key_hints`] takes.
pub(crate) fn key_hints_width(painter: &Painter, p: &Palette, hints: &[(&[&str], &str)]) -> f32 {
    let width = |text: &str, font: FontId| {
        painter
            .layout_no_wrap(text.to_owned(), font, p.text)
            .size()
            .x
    };
    let mut total = 0.0;
    for (keys, what) in hints {
        for key in *keys {
            total += (width(key, theme::strong(10.5)) + 10.0).max(22.0) + 4.0;
        }
        total += 2.0 + width(what, FontId::proportional(11.5)) + 16.0;
    }
    total - 16.0
}

/// The 2 pixel line of `color` and the bar under it, over the whole `rect`; returns the bar.
pub(crate) fn paint_top_band(painter: &Painter, p: &Palette, rect: Rect, color: Color32) -> Rect {
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 2.0)),
        0.0,
        color,
    );
    let bar = rect.with_min_y(rect.top() + 2.0);
    painter.rect_filled(bar, 0.0, bar_fill(p));
    painter.line_segment(
        [
            pos2(bar.left(), bar.bottom() - 0.5),
            pos2(bar.right(), bar.bottom() - 0.5),
        ],
        Stroke::new(1.0, line_edge(p)),
    );
    bar.with_max_y(bar.bottom() - 1.0)
}

/// A bar at the bottom of a window over the whole `rect`, its edge on top.
pub(crate) fn paint_bottom_band(painter: &Painter, p: &Palette, rect: Rect) {
    painter.rect_filled(rect, 0.0, bar_fill(p));
    painter.line_segment(
        [
            pos2(rect.left(), rect.top() + 0.5),
            pos2(rect.right(), rect.top() + 0.5),
        ],
        Stroke::new(1.0, line_edge(p)),
    );
}

/// The logo, the name and the line under it.
pub(crate) fn paint_title(
    painter: &Painter,
    p: &Palette,
    bar: Rect,
    name_width: f32,
    name: &str,
    detail: &dyn Fn(&Painter, Pos2),
    dim: bool,
) {
    let cy = bar.center().y;
    let mut logo = painter.clone();
    if dim {
        logo.multiply_opacity(0.45);
    }
    crate::paint_logo(
        &logo,
        Rect::from_center_size(pos2(bar.left() + 29.0, cy), vec2(30.0, 30.0)),
    );
    let left = bar.left() + 56.0;
    let column = painter.with_clip_rect(Rect::from_min_max(
        pos2(left, bar.top()),
        pos2(left + name_width, bar.bottom()),
    ));
    column.text(
        pos2(left, cy - 3.0),
        Align2::LEFT_BOTTOM,
        name,
        theme::strong(16.0),
        if dim { p.weak } else { p.text },
    );
    detail(&column, pos2(left, cy + 8.0));
}

// The effect window.

/// Who the window is for, as the top bar shows it.
pub struct Identity {
    pub name: String,
    /// Under the name: the family, or a plugin's vendor and format.
    pub detail: String,
    /// The family's color, a gray for plugins.
    pub color: Color32,
    /// A dot of the family's color before the detail.
    pub dot: bool,
    /// Presets and A/B (meters have none).
    pub presets: bool,
    /// A plugin's own window.
    pub interface: Option<Interface>,
}

#[derive(Clone, Copy, Debug)]
pub struct Interface {
    /// The plugin has a window AudioStrada can open.
    pub available: bool,
    pub open: bool,
}

/// What was clicked in the top bar.
#[derive(Debug, Default)]
pub struct TopActions {
    pub bypass: Option<bool>,
    pub interface: bool,
}

/// Parameters and plugin state of an insert, as A/B slots and presets hold them.
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    params: BTreeMap<String, f32>,
    state: Option<Vec<u8>>,
}

impl Settings {
    fn of(preset: &Preset) -> Self {
        Settings {
            params: preset.params.clone(),
            state: preset
                .state
                .as_deref()
                .and_then(audiostrada_core::base64::decode),
        }
    }
}

/// What an A/B slot keeps: the settings and the preset they came from.
#[derive(Clone, Debug)]
struct Slot {
    settings: Settings,
    preset: Option<String>,
    loaded: Option<BTreeMap<String, f32>>,
}

/// What a preset panel holds while open.
#[derive(Debug)]
struct Panel {
    search: String,
    group: Group,
    /// Preset chosen with the keyboard.
    cursor: Option<String>,
    /// Preset under the pointer, since when.
    hover: Option<(String, f64)>,
    /// The pointer moved since the last key: it chooses what is heard.
    mouse: bool,
    /// Preset being heard, not kept yet.
    heard: Option<String>,
    /// The settings when the panel opened, back on Esc.
    before: Settings,
    /// A preset being renamed, and its new name.
    rename: Option<(String, String)>,
    focus: bool,
    opened: u64,
}

/// Presets shown in the panel.
#[derive(Clone, Debug, PartialEq)]
enum Group {
    All,
    Favorites,
    /// Factory or the user's, and a category (`None`: those without one).
    Category(bool, Option<String>),
}

/// What the frame keeps between frames.
#[derive(Debug, Default)]
pub struct FrameState {
    ab: [Option<Slot>; 2],
    ab_current: usize,
    /// Last preset loaded, and its parameters as loaded.
    preset: Option<String>,
    loaded: Option<BTreeMap<String, f32>>,
    /// Name typed while saving a preset.
    saving: Option<String>,
    focus_saving: bool,
    panel: Option<Panel>,
    list: Option<Rc<Vec<Preset>>>,
}

impl FrameState {
    /// The keys the window takes before the shortcuts: Left and Right step the presets,
    /// the panel and the save box take the keys that browse and answer.
    pub fn keys(&self, presets: bool) -> Vec<Key> {
        if !presets {
            return Vec::new();
        }
        let mut keys = vec![Key::ArrowLeft, Key::ArrowRight];
        if self.panel.is_some() || self.saving.is_some() {
            keys.extend([Key::ArrowUp, Key::ArrowDown, Key::Enter, Key::Escape]);
        }
        keys
    }

    /// The window closes: a preset only heard goes back to the settings before it.
    pub fn close(&mut self, doc: &mut Document, channel: Channel, insert: InsertId) {
        if let Some(panel) = self.panel.take()
            && panel.heard.is_some()
        {
            put(doc, channel, insert, &panel.before, None);
        }
        self.saving = None;
    }

    fn presets(&mut self, effect: &str) -> Rc<Vec<Preset>> {
        self.list
            .get_or_insert_with(|| {
                Rc::new(crate::presets::list(
                    crate::presets::root().as_deref(),
                    effect,
                ))
            })
            .clone()
    }
}

fn current_settings(doc: &mut Document, channel: Channel, insert: &Insert) -> Settings {
    let state = doc
        .player_mut()
        .plugin(channel, insert.id)
        .and_then(|p| p.save_state());
    Settings {
        params: insert.params.clone(),
        state,
    }
}

/// Puts `settings` into the insert: an undo step named `label`, or only heard without one.
fn put(
    doc: &mut Document,
    channel: Channel,
    insert: InsertId,
    settings: &Settings,
    label: Option<&str>,
) {
    let encoded = settings
        .state
        .as_deref()
        .map(audiostrada_core::base64::encode);
    let change = |p: &mut audiostrada_core::Project| {
        let inserts = match channel {
            Channel::Master => Some(&mut p.master.inserts),
            Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
            Channel::Clip(c) => audiostrada_core::edit::clip_mut(p, c).map(|c| &mut c.inserts),
        };
        if let Some(i) = inserts.and_then(|list| list.iter_mut().find(|i| i.id == insert)) {
            i.params = settings.params.clone();
            if encoded.is_some() {
                i.state = encoded.clone();
            }
        }
    };
    match label {
        Some(label) => {
            doc.history_mut().edit(label, change);
        }
        None => doc.history_mut().modify_unrecorded(change),
    }
    if let (Some(state), Some(plugin)) = (&settings.state, doc.player_mut().plugin(channel, insert))
    {
        plugin.load_state(state);
    }
}

/// Loads a preset as an undo step.
fn load(
    doc: &mut Document,
    channel: Channel,
    insert: InsertId,
    state: &mut FrameState,
    preset: &Preset,
) {
    let settings = Settings::of(preset);
    put(
        doc,
        channel,
        insert,
        &settings,
        Some(&trf!("Preset {name}", name = preset.name)),
    );
    state.preset = Some(preset.name.clone());
    state.loaded = Some(settings.params);
}

/// "Category › name" or the name, with its category apart.
fn split_name(name: &str) -> (Option<String>, String) {
    match preset_category(name) {
        Some((c, n)) => (Some(c), n),
        None => (None, name.to_owned()),
    }
}

/// The top bar of an effect's window; `keys` are those the window took for it.
pub fn top(
    ui: &mut Ui,
    keys: &[Key],
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    who: &Identity,
) -> TopActions {
    egui::Panel::top(("effect-top", insert.id.0))
        .frame(egui::Frame::NONE)
        .exact_size(TOP)
        .show_separator_line(false)
        .show(ui, |ui| top_bar(ui, keys, doc, channel, insert, state, who))
        .inner
}

fn top_bar(
    ui: &mut Ui,
    keys: &[Key],
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    who: &Identity,
) -> TopActions {
    let p = theme::current(ui);
    let r = radius(&p);
    let bypass = insert.bypass;
    let painter = ui.painter().clone();
    let line = if bypass {
        theme::mix(p.weak, p.panel, 0.55)
    } else {
        who.color
    };
    let bar = paint_top_band(&painter, &p, ui.max_rect(), line);
    ui.allocate_rect(ui.max_rect(), Sense::hover());
    let cy = bar.center().y;
    let id = ui.scope_id();
    let mut out = TopActions::default();

    // From the right: Bypass, the plugin's window, A/B.
    let mut x = bar.right() - 14.0;
    let text = tr("Bypass");
    let width = painter
        .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
        .size()
        .x
        + 24.0;
    let rect = Rect::from_min_max(pos2(x - width, cy - 15.0), pos2(x, cy + 15.0));
    let response = button(ui, rect, id.with("bypass"), true);
    if bypass {
        paint_face(ui, rect, r, &response, Face::Lit(crate::settings::WARN));
    } else {
        paint_face(ui, rect, r, &response, Face::Plain);
    }
    let ink = if bypass {
        Color32::from_rgb(17, 17, 17)
    } else if response.hovered() {
        p.text
    } else {
        p.weak
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        theme::strong(12.0),
        ink,
    );
    if response
        .on_hover_text(if bypass {
            tr("Bypassed: click to enable")
        } else {
            tr("Bypass the effect: hear the track without it")
        })
        .clicked()
    {
        out.bypass = Some(!bypass);
    }
    x = rect.left() - 6.0;
    if let Some(interface) = who.interface {
        let text = tr("Interface");
        let width = painter
            .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
            .size()
            .x
            + 38.0;
        let rect = Rect::from_min_max(pos2(x - width, cy - 14.0), pos2(x, cy + 14.0));
        let response = button(ui, rect, id.with("interface"), interface.available);
        let mut face = painter.clone();
        if !interface.available {
            face.multiply_opacity(0.45);
        }
        let lit = if interface.open { Some(p.accent) } else { None };
        paint_face(ui, rect, r, &response, Face::Plain);
        if let Some(c) = lit {
            painter.rect_stroke(rect, r, Stroke::new(1.0, c), StrokeKind::Inside);
        }
        face.text(
            pos2(rect.left() + 10.0, cy),
            Align2::LEFT_CENTER,
            text,
            theme::strong(12.0),
            p.text,
        );
        paint_glyph(
            &face,
            Rect::from_center_size(pos2(rect.right() - 16.0, cy), vec2(12.0, 12.0)),
            Glyph::External,
            p.text,
        );
        let hint = if !interface.available {
            tr("This plugin has no window AudioStrada can open yet")
        } else if interface.open {
            tr("Close plugin window")
        } else {
            tr("Open plugin window")
        };
        out.interface = response.on_hover_text(hint).clicked();
        x = rect.left() - 6.0;
    }
    let effect = insert.effect.as_str();
    let list = if who.presets {
        state.presets(effect)
    } else {
        Rc::default()
    };
    let mut dimmed = ui.new_child(UiBuilder::new().max_rect(bar));
    if bypass {
        dimmed.multiply_opacity(0.55);
    }
    if who.presets {
        x -= 6.0;
        painter.line_segment(
            [pos2(x, cy - 10.0), pos2(x, cy + 10.0)],
            Stroke::new(1.0, if p.dark { p.surface } else { line_edge(&p) }),
        );
        x -= 7.0;
        let copy = Rect::from_min_max(pos2(x - 30.0, cy - 15.0), pos2(x, cy + 15.0));
        let hint = if state.ab_current == 0 {
            tr("Copy A to B")
        } else {
            tr("Copy B to A")
        };
        if glyph_button(&dimmed, copy, id.with("copy"), Glyph::Copy, None, true)
            .on_hover_text(hint)
            .clicked()
        {
            let now = current_settings(doc, channel, insert);
            state.ab[1 - state.ab_current] = Some(Slot {
                settings: now,
                preset: state.preset.clone(),
                loaded: state.loaded.clone(),
            });
        }
        x = copy.left() - 4.0;
        for (slot, label) in [(1, "B"), (0, "A")] {
            let rect = Rect::from_min_max(pos2(x - 34.0, cy - 15.0), pos2(x, cy + 15.0));
            let corners = if slot == 0 {
                CornerRadius {
                    nw: r as u8,
                    sw: r as u8,
                    ne: 0,
                    se: 0,
                }
            } else {
                CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: r as u8,
                    se: r as u8,
                }
            };
            let face = if state.ab_current == slot {
                Face::Lit(p.accent)
            } else {
                Face::Plain
            };
            let response = text_button(&dimmed, rect, id.with(label), label, face, corners)
                .on_hover_text(tr("Compare two settings: each slot keeps its own"));
            if response.clicked() && state.ab_current != slot {
                let now = Slot {
                    settings: current_settings(doc, channel, insert),
                    preset: state.preset.clone(),
                    loaded: state.loaded.clone(),
                };
                let other = state.ab[slot].clone().unwrap_or_else(|| now.clone());
                state.ab[state.ab_current] = Some(now);
                put(
                    doc,
                    channel,
                    insert.id,
                    &other.settings,
                    Some(&trf!("Compare {slot}", slot = label)),
                );
                state.preset = other.preset;
                state.loaded = other.loaded;
                state.ab_current = slot;
            }
            x = rect.left();
        }
        x -= 12.0;
    }

    // The name, and the presets centered in what is left.
    let left = bar.left() + 56.0;
    let room = x - left;
    let name_width = if room >= 190.0 + 12.0 + 132.0 + 240.0 {
        190.0
    } else {
        120.0f32.min(room.max(0.0))
    };
    let detail = |column: &Painter, at: Pos2| {
        if bypass {
            column.text(
                at,
                Align2::LEFT_TOP,
                tr("Bypassed: the effect does not play"),
                theme::strong(10.5),
                crate::settings::WARN,
            );
            return;
        }
        let mut x = at.x;
        if who.dot {
            column.circle_filled(pos2(x + 3.5, at.y + 6.5), 3.5, who.color);
            x += 12.0;
        }
        column.text(
            pos2(x, at.y),
            Align2::LEFT_TOP,
            &who.detail,
            FontId::proportional(10.5),
            p.weak,
        );
    };
    paint_title(&painter, &p, bar, name_width, &who.name, &detail, bypass);
    if !who.presets {
        return out;
    }
    let start = left + name_width + 12.0;
    let room = x - start;
    let field_width = (room - 132.0).clamp(60.0, 300.0);
    let total = field_width + 132.0;
    let x0 = start + ((room - total) / 2.0).max(0.0);
    let prev = Rect::from_min_size(pos2(x0, cy - 15.0), vec2(30.0, 30.0));
    let field = Rect::from_min_size(pos2(prev.right(), cy - 15.0), vec2(field_width, 30.0));
    let next = Rect::from_min_size(pos2(field.right(), cy - 15.0), vec2(30.0, 30.0));
    let star = Rect::from_min_size(pos2(next.right() + 6.0, cy - 15.0), vec2(30.0, 30.0));
    let save = Rect::from_min_size(pos2(star.right() + 6.0, cy - 15.0), vec2(30.0, 30.0));

    let order = menu_order(&list);
    let at = state
        .preset
        .as_ref()
        .and_then(|name| list.iter().position(|q| &q.name == name))
        .and_then(|i| order.iter().position(|&j| j == i));
    let step = |forward: bool| -> Option<usize> {
        let n = order.len();
        (n > 0).then(|| {
            order[match (at, forward) {
                (Some(k), true) => (k + 1) % n,
                (Some(k), false) => (k + n - 1) % n,
                (None, true) => 0,
                (None, false) => n - 1,
            }]
        })
    };
    let mut chosen = None;
    let any = !list.is_empty();
    let corners = |left: bool| {
        let r = r as u8;
        if left {
            CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            }
        } else {
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: r,
                se: r,
            }
        }
    };
    for (rect, forward, glyph, hint, name) in [
        (prev, false, Glyph::Left, tr("Previous preset"), "prev"),
        (next, true, Glyph::Right, tr("Next preset"), "next"),
    ] {
        let response = button(&dimmed, rect, id.with(name), any);
        let mut ink = dimmed.painter().clone();
        if !any {
            ink.multiply_opacity(0.4);
        }
        paint_face(&dimmed, rect, corners(!forward), &response, Face::Plain);
        paint_glyph(
            &ink,
            Rect::from_center_size(rect.center(), vec2(12.0, 12.0)),
            glyph,
            p.text,
        );
        if response.on_hover_text(hint).clicked() {
            chosen = step(forward);
        }
    }
    // The field: category, name, a dot when changed since it was loaded.
    let open = state.panel.is_some();
    let response = button(&dimmed, field, id.with("field"), true);
    let edge = if open {
        p.accent
    } else if response.hovered() {
        theme::mix(button_edge(&p), p.accent, 0.5)
    } else {
        button_edge(&p)
    };
    dimmed.painter().rect_filled(field, 0.0, field_fill(&p));
    dimmed
        .painter()
        .rect_stroke(field, 0.0, Stroke::new(1.0, edge), StrokeKind::Inside);
    let text = dimmed
        .painter()
        .with_clip_rect(field.shrink2(vec2(12.0, 0.0)));
    let mut tx = field.left() + 12.0;
    let font = FontId::proportional(13.0);
    match state.preset.as_deref().map(split_name) {
        Some((category, name)) => {
            if let Some(category) = category {
                tx = text
                    .text(
                        pos2(tx, cy),
                        Align2::LEFT_CENTER,
                        category,
                        font.clone(),
                        p.weak,
                    )
                    .right()
                    + 7.0;
                tx = text
                    .text(
                        pos2(tx, cy),
                        Align2::LEFT_CENTER,
                        "›",
                        font.clone(),
                        theme::mix(p.weak, field_fill(&p), 0.45),
                    )
                    .right()
                    + 7.0;
            }
            tx = text
                .text(
                    pos2(tx, cy),
                    Align2::LEFT_CENTER,
                    name,
                    theme::strong(13.0),
                    p.text,
                )
                .right();
            let changed = state.loaded.as_ref().is_some_and(|l| *l != insert.params);
            if changed {
                text.circle_filled(pos2(tx + 8.0, cy + 1.0), 3.0, p.accent);
            }
        }
        None => {
            text.text(
                pos2(tx, cy),
                Align2::LEFT_CENTER,
                if any {
                    tr("Presets")
                } else {
                    tr("No presets yet")
                },
                font,
                p.weak,
            );
        }
    }
    paint_glyph(
        dimmed.painter(),
        Rect::from_center_size(pos2(field.right() - 14.0, cy), vec2(11.0, 11.0)),
        Glyph::Down,
        p.weak,
    );
    let changed = state.loaded.as_ref().is_some_and(|l| *l != insert.params);
    let response = response.on_hover_text(if changed {
        tr("Changed since the preset was loaded. Click for all the presets")
    } else {
        tr("Click for all the presets; Left and Right arrows step through them")
    });
    if response.clicked() {
        if open {
            state.close(doc, channel, insert.id);
        } else {
            open_panel(
                doc,
                channel,
                insert,
                state,
                &list,
                ui.ctx().cumulative_frame_nr(),
            );
        }
    }
    // Favorite and save.
    let favorite = state
        .preset
        .as_ref()
        .is_some_and(|n| crate::favorites::with(|f| f.preset(effect, n)));
    let response = glyph_button(
        &dimmed,
        star,
        id.with("star"),
        Glyph::Star(favorite),
        favorite.then(|| star_color(&p)),
        state.preset.is_some(),
    );
    let response = if favorite {
        response.on_hover_text(tr("Remove this preset from the favorites"))
    } else {
        response.on_hover_text(tr("Add this preset to the favorites"))
    };
    if response.clicked()
        && let Some(name) = &state.preset
    {
        crate::favorites::change(|f| f.toggle_preset(effect, name));
    }
    if glyph_button(&dimmed, save, id.with("save"), Glyph::Save, None, true)
        .on_hover_text(tr("Save the current settings as a preset"))
        .clicked()
    {
        state.close(doc, channel, insert.id);
        state.saving = Some(state.preset.clone().unwrap_or_default());
        state.focus_saving = true;
    }
    // Left and Right step through the presets, the panel closed.
    if state.panel.is_none() && state.saving.is_none() {
        for key in keys {
            match key {
                Key::ArrowLeft => chosen = step(false),
                Key::ArrowRight => chosen = step(true),
                _ => {}
            }
        }
    }
    if let Some(preset) = chosen.and_then(|i| list.get(i)) {
        load(doc, channel, insert.id, state, preset);
    }
    if state.panel.is_some() {
        preset_panel(ui, keys, doc, channel, insert, state, &list, field);
    }
    if state.saving.is_some() {
        save_box(ui, keys, doc, channel, insert, state, save);
    }
    out
}

fn open_panel(
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    list: &[Preset],
    frame: u64,
) {
    // On the loaded preset's group, to see its neighbors.
    let group = state
        .preset
        .as_ref()
        .and_then(|name| list.iter().find(|q| &q.name == name))
        .map_or(Group::All, |q| {
            Group::Category(q.factory, preset_category(&q.name).map(|c| c.0))
        });
    state.panel = Some(Panel {
        search: String::new(),
        group,
        cursor: state.preset.clone(),
        hover: None,
        mouse: true,
        // What plays is the loaded preset, maybe changed since: nothing to hear yet.
        heard: state.preset.clone(),
        before: current_settings(doc, channel, insert),
        rename: None,
        focus: true,
        opened: frame,
    });
}

fn in_group(preset: &Preset, group: &Group, effect: &str) -> bool {
    match group {
        Group::All => true,
        Group::Favorites => crate::favorites::with(|f| f.preset(effect, &preset.name)),
        Group::Category(factory, category) => {
            preset.factory == *factory
                && preset_category(&preset.name).map(|c| c.0).as_ref() == category.as_ref()
        }
    }
}

/// The panel of presets under the field: search, groups, the list; heard while browsing.
#[allow(clippy::too_many_arguments)]
fn preset_panel(
    ui: &mut Ui,
    keys: &[Key],
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    list: &[Preset],
    field: Rect,
) {
    let p = theme::current(ui);
    let ctx = ui.ctx().clone();
    let effect = insert.effect.as_str();
    let screen = ctx.content_rect();
    let width = 628.0f32.min(screen.width() - 24.0);
    let height = 430.0f32
        .min(screen.bottom() - field.bottom() - 20.0)
        .max(160.0);
    let left = (field.center().x - width / 2.0).clamp(
        screen.left() + 12.0,
        (screen.right() - 12.0 - width).max(0.0),
    );
    let rect = Rect::from_min_size(pos2(left, field.bottom() + 10.0), vec2(width, height));
    let Some(mut panel) = state.panel.take() else {
        return;
    };
    let order = menu_order(list);
    let search = panel.search.to_lowercase();
    let shown: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| {
            if search.is_empty() {
                in_group(&list[i], &panel.group, effect)
            } else {
                list[i].name.to_lowercase().contains(&search)
            }
        })
        .collect();
    let mut keep: Option<String> = None;
    let mut cancel = false;
    let mut save_as = false;
    let mut delete: Option<String> = None;
    let mut renamed: Option<(String, String)> = None;
    // Keys: browse, keep, cancel; a rename answers first.
    let names: Vec<&str> = shown.iter().map(|&i| list[i].name.as_str()).collect();
    for key in keys {
        let forward = match key {
            Key::ArrowDown | Key::ArrowRight => Some(true),
            Key::ArrowUp | Key::ArrowLeft => Some(false),
            _ => None,
        };
        if let Some(forward) = forward
            && !names.is_empty()
            && panel.rename.is_none()
        {
            let here = panel
                .cursor
                .as_ref()
                .and_then(|c| names.iter().position(|n| n == c));
            let k = match (here, forward) {
                (Some(k), true) => (k + 1).min(names.len() - 1),
                (Some(k), false) => k.saturating_sub(1),
                (None, true) => 0,
                (None, false) => names.len() - 1,
            };
            panel.cursor = Some(names[k].to_owned());
            panel.mouse = false;
            panel.hover = None;
        }
        match key {
            Key::Enter => match panel.rename.take() {
                Some(r) => renamed = Some(r),
                None => keep = panel.heard.clone().or(panel.cursor.clone()),
            },
            Key::Escape if panel.rename.take().is_none() => cancel = true,
            _ => {}
        }
    }
    if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
        panel.mouse = true;
    }
    let area = egui::Area::new(ui.scope_id().with("preset-panel"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(&ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            let rect = Rect::from_min_size(ui.max_rect().min, rect.size());
            let painter = ui.painter().clone();
            let fill = if p.dark {
                theme::mix(p.panel, p.surface, 0.35)
            } else {
                Color32::WHITE
            };
            let shadow = egui::Shadow {
                offset: [0, 18],
                blur: 50,
                spread: 0,
                color: Color32::from_black_alpha(if p.dark { 140 } else { 60 }),
            };
            painter.add(shadow.as_shape(rect, CornerRadius::same(10)));
            painter.rect(
                rect,
                10.0,
                fill,
                Stroke::new(1.0, line_edge(&p)),
                StrokeKind::Inside,
            );
            ui.allocate_rect(rect, Sense::click());
            let edge = Stroke::new(1.0, line_edge(&p));
            // Search and Save as.
            let head = Rect::from_min_size(rect.min, vec2(rect.width(), 52.0));
            painter.line_segment([head.left_bottom(), head.right_bottom()], edge);
            let save_text = tr("Save as…");
            let save_width = painter
                .layout_no_wrap(save_text.to_owned(), FontId::proportional(12.0), p.text)
                .size()
                .x
                + 24.0;
            let save = Rect::from_min_max(
                pos2(head.right() - 10.0 - save_width, head.top() + 10.0),
                pos2(head.right() - 10.0, head.top() + 42.0),
            );
            let search_box = Rect::from_min_max(
                pos2(head.left() + 10.0, head.top() + 10.0),
                pos2(save.left() - 8.0, head.top() + 42.0),
            );
            painter.rect(
                search_box,
                radius(&p),
                field_fill(&p),
                Stroke::new(1.0, button_edge(&p)),
                StrokeKind::Inside,
            );
            paint_glyph(
                &painter,
                Rect::from_center_size(
                    search_box.left_center() + vec2(17.0, 0.0),
                    vec2(14.0, 14.0),
                ),
                Glyph::Search,
                p.weak,
            );
            let text_rect = Rect::from_min_max(
                pos2(search_box.left() + 30.0, search_box.top() + 1.0),
                pos2(search_box.right() - 8.0, search_box.bottom() - 1.0),
            );
            let edit = ui.put(
                text_rect,
                egui::TextEdit::singleline(&mut panel.search)
                    .frame(egui::Frame::NONE)
                    .hint_text(tr("Search presets"))
                    .align(egui::Align2::LEFT_CENTER)
                    .desired_width(text_rect.width()),
            );
            if std::mem::take(&mut panel.focus) {
                edit.request_focus();
            }
            let response = button(ui, save, ui.scope_id().with("save-as"), true);
            paint_face(ui, save, radius(&p), &response, Face::Plain);
            painter.text(
                save.center(),
                Align2::CENTER_CENTER,
                save_text,
                FontId::proportional(12.0),
                p.text,
            );
            save_as = response.clicked();
            // Hints at the bottom.
            let foot = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 38.0), rect.max);
            painter.rect_filled(
                foot.shrink(1.0),
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 9,
                    se: 9,
                },
                if p.dark {
                    theme::mix(fill, Color32::BLACK, 0.12)
                } else {
                    theme::mix(Color32::WHITE, p.panel, 0.25)
                },
            );
            painter.line_segment([foot.left_top(), foot.right_top()], edge);
            let foot_painter = painter.with_clip_rect(foot);
            let end = key_hints(
                &foot_painter,
                &p,
                pos2(foot.left() + 12.0, foot.center().y),
                &[
                    (&["←", "→"], tr("previous, next")),
                    (&["↑", "↓"], tr("browse")),
                    (&[tr("Enter")], tr("keep")),
                    (&[tr("Esc")], tr("cancel")),
                ],
            );
            let right = foot_painter.layout_no_wrap(
                tr("right-click: rename, delete").to_owned(),
                FontId::proportional(11.5),
                p.weak,
            );
            if end + right.size().x < foot.right() - 12.0 {
                foot_painter.galley(
                    pos2(
                        foot.right() - 12.0 - right.size().x,
                        foot.center().y - right.size().y / 2.0,
                    ),
                    right,
                    p.weak,
                );
            }
            // Groups at the left.
            let body = Rect::from_min_max(
                pos2(rect.left(), head.bottom()),
                pos2(rect.right(), foot.top()),
            );
            let side_width = 180.0f32.min(body.width() * 0.35);
            let side = Rect::from_min_max(body.min, pos2(body.left() + side_width, body.bottom()));
            painter.line_segment([side.right_top(), side.right_bottom()], edge);
            let mut side_ui = ui.new_child(UiBuilder::new().max_rect(side.shrink(8.0)));
            egui::ScrollArea::vertical()
                .id_salt("preset-groups")
                .auto_shrink([false, false])
                .show(&mut side_ui, |ui| {
                    groups(ui, list, effect, &mut panel.group, &mut panel.search);
                });
            // The presets.
            let column = Rect::from_min_max(pos2(side.right(), body.top()), body.max).shrink(8.0);
            let note = column.height() > 230.0;
            let rows = if note {
                column.with_max_y(column.bottom() - 62.0)
            } else {
                column
            };
            let mut rows_ui = ui.new_child(UiBuilder::new().max_rect(rows));
            let mut hovered = None;
            egui::ScrollArea::vertical()
                .id_salt("preset-rows")
                .auto_shrink([false, false])
                .show(&mut rows_ui, |ui| {
                    if shown.is_empty() {
                        ui.add_space(8.0);
                        ui.weak(if list.is_empty() {
                            tr("No presets yet")
                        } else {
                            tr("No preset matches")
                        });
                    }
                    let flat = !search.is_empty() || !matches!(panel.group, Group::Category(..));
                    for &i in &shown {
                        let preset = &list[i];
                        let row = preset_row(
                            ui,
                            preset,
                            effect,
                            flat,
                            state.preset.as_deref() == Some(&preset.name),
                            panel.cursor.as_deref() == Some(&preset.name),
                            &mut panel.rename,
                        );
                        if row.hovered {
                            hovered = Some(preset.name.clone());
                        }
                        if row.clicked {
                            keep = Some(preset.name.clone());
                        }
                        if row.scroll {
                            ui.scroll_to_rect(row.rect, None);
                        }
                        if let Some(action) = row.action {
                            match action {
                                RowAction::Rename => {
                                    panel.rename = Some((preset.name.clone(), preset.name.clone()))
                                }
                                RowAction::Delete => delete = Some(preset.name.clone()),
                            }
                        }
                    }
                });
            if note {
                let note = Rect::from_min_max(pos2(column.left(), rows.bottom() + 8.0), column.max);
                painter.rect_filled(
                    note,
                    8.0,
                    if p.dark {
                        theme::mix(fill, Color32::BLACK, 0.18)
                    } else {
                        theme::mix(Color32::WHITE, p.panel, 0.3)
                    },
                );
                let galley = painter.layout(
                    tr(
                        "Hover or browse with the keyboard to hear a preset; Esc goes back to the \
                        settings before, Enter keeps it.",
                    )
                    .to_owned(),
                    FontId::proportional(11.5),
                    p.weak,
                    note.width() - 20.0,
                );
                painter.galley(
                    pos2(note.left() + 10.0, note.center().y - galley.size().y / 2.0),
                    galley,
                    p.weak,
                );
            }
            hovered
        });
    let hovered = area.inner.filter(|_| panel.mouse);
    // What is heard: the preset under the resting pointer, else the keyboard's.
    let now = ui.input(|i| i.time);
    match (&hovered, &panel.hover) {
        (Some(name), Some((h, _))) if name == h => {}
        (Some(name), _) => panel.hover = Some((name.clone(), now)),
        (None, _) => panel.hover = None,
    }
    let rested = panel
        .hover
        .as_ref()
        .filter(|(_, since)| now - since >= HOVER_REST)
        .map(|(n, _)| n.clone());
    let waiting = panel.hover.is_some() && rested.is_none();
    if waiting {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOVER_REST));
    }
    let target = if waiting {
        panel.heard.clone()
    } else {
        rested.or(panel.cursor.clone())
    };
    if target != panel.heard && keep.is_none() && !cancel && panel.rename.is_none() {
        match target
            .as_ref()
            .and_then(|n| list.iter().find(|q| &q.name == n))
        {
            Some(preset) => put(doc, channel, insert.id, &Settings::of(preset), None),
            None => put(doc, channel, insert.id, &panel.before, None),
        }
        panel.heard = target;
    }
    // A click elsewhere closes the panel, as Esc.
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|at| !rect.contains(at) && !field.contains(at))
    });
    if outside
        && ctx.cumulative_frame_nr() > panel.opened
        && !egui::Popup::is_any_open(&ctx)
        && panel.rename.is_none()
    {
        cancel = true;
    }
    let root = crate::presets::root();
    if let Some((old, new)) = renamed {
        let new = new.trim().to_owned();
        if !new.is_empty()
            && new != old
            && let (Some(root), Some(preset)) = (&root, list.iter().find(|q| q.name == old))
        {
            let mut preset = preset.clone();
            preset.name = new.clone();
            match crate::presets::save(root, effect, &preset)
                .and_then(|()| crate::presets::delete(root, effect, &old))
            {
                Ok(()) => {
                    if crate::favorites::with(|f| f.preset(effect, &old)) {
                        crate::favorites::change(|f| {
                            f.toggle_preset(effect, &old);
                            f.toggle_preset(effect, &new);
                        });
                    }
                    for name in [&mut state.preset, &mut panel.cursor, &mut panel.heard]
                        .into_iter()
                        .flatten()
                    {
                        if *name == old {
                            *name = new.clone();
                        }
                    }
                }
                Err(e) => doc.notify(trf!("Cannot rename the preset: {error}", error = e)),
            }
            state.list = None;
        }
    }
    if let (Some(name), Some(root)) = (delete, &root) {
        match crate::presets::delete(root, effect, &name) {
            Ok(()) => {
                if state.preset.as_ref() == Some(&name) {
                    state.preset = None;
                    state.loaded = None;
                }
            }
            Err(e) => doc.notify(trf!("Cannot delete the preset: {error}", error = e)),
        }
        state.list = None;
    }
    if let Some(name) = keep
        && let Some(preset) = list.iter().find(|q| q.name == name)
    {
        if panel.heard.is_some() {
            put(doc, channel, insert.id, &panel.before, None);
        }
        load(doc, channel, insert.id, state, preset);
        return;
    }
    if cancel || save_as {
        if panel.heard.is_some() {
            put(doc, channel, insert.id, &panel.before, None);
        }
        if save_as {
            state.saving = Some(state.preset.clone().unwrap_or_default());
            state.focus_saving = true;
        }
        return;
    }
    state.panel = Some(panel);
}

/// The groups of presets: all, favorites, the factory's and the user's categories.
fn groups(ui: &mut Ui, list: &[Preset], effect: &str, group: &mut Group, search: &mut String) {
    let p = theme::current(ui);
    let count = |g: &Group| list.iter().filter(|q| in_group(q, g, effect)).count();
    let mut row = |ui: &mut Ui, g: Group, label: String, star: bool| {
        let n = count(&g);
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        let on = *group == g && search.is_empty();
        if on {
            ui.painter().rect_filled(rect, radius(&p), chosen_fill(&p));
        } else if response.hovered() {
            ui.painter().rect_filled(rect, radius(&p), p.hover);
        }
        let mut x = rect.left() + 10.0;
        if star {
            paint_glyph(
                ui.painter(),
                Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0)),
                Glyph::Star(true),
                star_color(&p),
            );
            x += 18.0;
        }
        ui.painter().text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            if on {
                p.text
            } else {
                theme::mix(p.text, p.weak, 0.3)
            },
        );
        ui.painter().text(
            pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            n.to_string(),
            FontId::proportional(11.0),
            if on { chosen_count(&p) } else { p.weak },
        );
        if response.clicked() {
            *group = g;
            search.clear();
        }
    };
    row(ui, Group::All, tr("All").to_owned(), false);
    row(ui, Group::Favorites, tr("Favorites").to_owned(), true);
    for (factory, title) in [(true, tr("Factory")), (false, tr("My presets"))] {
        let members: Vec<&Preset> = list.iter().filter(|q| q.factory == factory).collect();
        if members.is_empty() {
            continue;
        }
        ui.add_space(8.0);
        section_title(ui, title);
        let mut categories: Vec<Option<String>> = Vec::new();
        for q in &members {
            let c = preset_category(&q.name).map(|c| c.0);
            if !categories.contains(&c) {
                categories.push(c);
            }
        }
        // Those without a category last.
        categories.sort_by_key(Option::is_none);
        for c in categories {
            let label = c.clone().unwrap_or_else(|| tr("Other").to_owned());
            row(ui, Group::Category(factory, c), label, false);
        }
    }
}

/// A small title in capitals above a group of rows.
pub(crate) fn section_title(ui: &mut Ui, title: &str) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        title.to_uppercase(),
        theme::strong(9.5),
        theme::mix(p.weak, p.panel, 0.3),
    );
}

enum RowAction {
    Rename,
    Delete,
}

struct Row {
    rect: Rect,
    hovered: bool,
    clicked: bool,
    scroll: bool,
    action: Option<RowAction>,
}

/// A preset of the panel's list: its name, "loaded", its star.
fn preset_row(
    ui: &mut Ui,
    preset: &Preset,
    effect: &str,
    flat: bool,
    loaded: bool,
    cursor: bool,
    rename: &mut Option<(String, String)>,
) -> Row {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let mut row = Row {
        rect,
        hovered: response.hovered(),
        clicked: false,
        scroll: false,
        action: None,
    };
    if let Some((old, new)) = rename
        && *old == preset.name
    {
        let edit = ui.put(
            rect.shrink2(vec2(4.0, 3.0)),
            egui::TextEdit::singleline(new).align(egui::Align2::LEFT_CENTER),
        );
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        return row;
    }
    let ink = if cursor {
        theme::text_on(p.accent)
    } else {
        p.text
    };
    if cursor {
        ui.painter().rect_filled(rect, radius(&p), p.accent);
        row.scroll = true;
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius(&p), p.surface);
    }
    let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(10.0, 0.0)));
    let (category, name) = split_name(&preset.name);
    let mut x = rect.left() + 10.0;
    if flat && let Some(category) = category {
        x = painter
            .text(
                pos2(x, rect.center().y),
                Align2::LEFT_CENTER,
                format!("{category} ›"),
                FontId::proportional(13.0),
                if cursor {
                    ink.gamma_multiply(0.7)
                } else {
                    p.weak
                },
            )
            .right()
            + 6.0;
    }
    painter.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        if cursor {
            theme::strong(13.0)
        } else {
            FontId::proportional(13.0)
        },
        ink,
    );
    let favorite = crate::favorites::with(|f| f.preset(effect, &preset.name));
    let star = Rect::from_center_size(rect.right_center() - vec2(18.0, 0.0), vec2(22.0, 22.0));
    if loaded {
        painter.text(
            pos2(star.left() - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            tr("loaded"),
            theme::strong(10.5),
            if cursor {
                ink.gamma_multiply(0.75)
            } else {
                p.weak
            },
        );
    }
    let star_response = ui
        .interact(star, response.id.with("star"), Sense::click())
        .on_hover_text(if favorite {
            tr("Remove this preset from the favorites")
        } else {
            tr("Add this preset to the favorites")
        });
    let color = if favorite {
        if cursor { ink } else { star_color(&p) }
    } else if star_response.hovered() {
        p.text
    } else if cursor {
        ink.gamma_multiply(0.6)
    } else {
        theme::mix(p.weak, p.panel, 0.35)
    };
    paint_glyph(
        ui.painter(),
        Rect::from_center_size(star.center(), vec2(13.0, 13.0)),
        Glyph::Star(favorite),
        color,
    );
    if star_response.clicked() {
        crate::favorites::change(|f| f.toggle_preset(effect, &preset.name));
    } else if response.clicked() {
        row.clicked = true;
    }
    row.hovered |= star_response.hovered();
    if !preset.factory {
        response.context_menu(|ui| {
            if ui.button(tr("Rename")).clicked() {
                row.action = Some(RowAction::Rename);
                ui.close();
            }
            if ui.button(tr("Delete")).clicked() {
                row.action = Some(RowAction::Delete);
                ui.close();
            }
        });
    }
    row
}

/// The box to name a preset being saved, under the save button.
fn save_box(
    ui: &mut Ui,
    keys: &[Key],
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    state: &mut FrameState,
    anchor: Rect,
) {
    let ctx = ui.ctx().clone();
    let Some(mut name) = state.saving.take() else {
        return;
    };
    let mut save = keys.contains(&Key::Enter);
    let mut close = keys.contains(&Key::Escape);
    let width = 300.0;
    let screen = ctx.content_rect();
    let left = (anchor.center().x - width / 2.0)
        .clamp(screen.left() + 8.0, (screen.right() - 8.0 - width).max(0.0));
    let area = egui::Area::new(ui.scope_id().with("save-preset"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(left, anchor.bottom() + 10.0))
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_width(width - 24.0);
                    ui.label(egui::RichText::new(tr("Save the preset")).font(theme::strong(13.0)));
                    ui.add_space(4.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text(tr("Category: name"))
                            .desired_width(f32::INFINITY),
                    );
                    if std::mem::take(&mut state.focus_saving) {
                        edit.request_focus();
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if theme::primary_button_enabled(ui, !name.trim().is_empty(), tr("Save"))
                            .clicked()
                        {
                            save = true;
                        }
                        if ui.button(tr("Cancel")).clicked() {
                            close = true;
                        }
                    });
                });
        });
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|at| !area.response.rect.contains(at) && !anchor.contains(at))
    });
    if save && !name.trim().is_empty() {
        let settings = current_settings(doc, channel, insert);
        let preset = Preset {
            name: name.trim().to_owned(),
            params: settings.params.clone(),
            state: settings
                .state
                .as_deref()
                .map(audiostrada_core::base64::encode),
            factory: false,
        };
        match crate::presets::root()
            .as_deref()
            .map(|r| crate::presets::save(r, &insert.effect, &preset))
        {
            Some(Ok(())) => {
                state.preset = Some(preset.name);
                state.loaded = Some(settings.params);
                state.list = None;
                return;
            }
            Some(Err(e)) => doc.notify(trf!("Cannot save the preset: {error}", error = e)),
            None => doc.notify(tr("No configuration folder for presets").into()),
        }
    }
    if close || outside {
        return;
    }
    state.saving = Some(name);
}

/// The bottom bar: the sidechain when the effect has one, levels in and out, the output gain
/// and Match.
pub fn bottom(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    matching: &mut bool,
    sidechain: bool,
) {
    egui::Panel::bottom(("effect-bottom", insert.id.0))
        .frame(egui::Frame::NONE)
        .exact_size(BOTTOM)
        .show_separator_line(false)
        .show(ui, |ui| {
            bottom_bar(ui, doc, channel, insert, matching, sidechain)
        });
}

fn bottom_bar(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    matching: &mut bool,
    sidechain: bool,
) {
    let p = theme::current(ui);
    let r = radius(&p);
    let full = ui.max_rect();
    let painter = ui.painter().clone();
    paint_bottom_band(&painter, &p, full);
    ui.allocate_rect(full, Sense::hover());
    let bar = full.with_min_y(full.top() + 1.0);
    let cy = bar.center().y;
    let id = insert.id;
    let ui_id = ui.scope_id();
    doc.player_mut().set_insert_watched(channel, id, true);
    doc.player_mut().set_insert_match(channel, id, *matching);
    let levels = doc.player_mut().insert_levels(channel, id);
    let [in_l, in_r, out_l, out_r, in_lufs, out_lufs, matched] =
        levels.unwrap_or([-120.0, -120.0, -120.0, -120.0, -120.0, -120.0, 0.0]);

    // Left: the sidechain.
    let mut left = bar.left() + 14.0;
    if sidechain {
        left = sidechain_button(ui, doc, channel, insert, pos2(left, cy)) + 14.0;
    }

    // Right: Match, then the output gain.
    let mut x = bar.right() - 14.0;
    let match_text = tr("Match");
    let match_hint = tr(
        "Listening aid: while on, the output is brought to the input's loudness, so that \
         bypassing compares the effect at equal level. Not saved, not exported; it turns off \
         when this window closes",
    );
    let text_width =
        |t: &str, font: FontId| painter.layout_no_wrap(t.to_owned(), font, p.text).size().x;
    if *matching {
        let keep_text = tr("Keep");
        let value = format!("{matched:+.1} dB");
        let keep_w = text_width(keep_text, theme::strong(11.5)) + 20.0;
        let value_w = text_width(&value, FontId::monospace(11.0)) + 16.0;
        let match_w = text_width(match_text, theme::strong(12.0)) + 20.0;
        let block = Rect::from_min_max(
            pos2(x - keep_w - value_w - match_w, cy - 14.0),
            pos2(x, cy + 14.0),
        );
        painter.rect(
            block,
            r,
            theme::mix(field_fill(&p), p.accent, 0.15),
            Stroke::NONE,
            StrokeKind::Inside,
        );
        let match_rect = Rect::from_min_size(block.min, vec2(match_w, 28.0));
        let response = button(ui, match_rect, ui_id.with("match"), true);
        painter.text(
            match_rect.center(),
            Align2::CENTER_CENTER,
            match_text,
            theme::strong(12.0),
            if p.dark {
                theme::mix(p.accent, Color32::WHITE, 0.7)
            } else {
                theme::mix(p.accent, Color32::BLACK, 0.35)
            },
        );
        if response.on_hover_text(match_hint).clicked() {
            *matching = false;
        }
        painter.text(
            pos2(match_rect.right() + value_w / 2.0, cy),
            Align2::CENTER_CENTER,
            value,
            FontId::monospace(11.0),
            p.accent,
        );
        let keep_rect = Rect::from_min_max(pos2(x - keep_w, cy - 14.0), pos2(x, cy + 14.0));
        let enabled = matched.abs() >= 0.05;
        let response = button(ui, keep_rect, ui_id.with("keep"), enabled);
        let corners = CornerRadius {
            nw: 0,
            sw: 0,
            ne: r as u8,
            se: r as u8,
        };
        let mut keep_painter = painter.clone();
        if !enabled {
            keep_painter.multiply_opacity(0.5);
        }
        keep_painter.rect_filled(
            keep_rect,
            corners,
            if response.hovered() {
                theme::mix(p.accent, Color32::WHITE, 0.1)
            } else {
                p.accent
            },
        );
        keep_painter.text(
            keep_rect.center(),
            Align2::CENTER_CENTER,
            keep_text,
            theme::strong(11.5),
            theme::text_on(p.accent),
        );
        painter.rect_stroke(block, r, Stroke::new(1.0, p.accent), StrokeKind::Inside);
        if response
            .on_hover_text(tr("Put this correction into Output, to keep it in the mix"))
            .clicked()
        {
            let kept = insert.gain_db + matched;
            doc.history_mut().edit(tr("Keep matched gain"), |p| {
                routing::set_insert_gain(p, channel, id, kept)
            });
            *matching = false;
        }
        x = block.left() - 8.0;
    } else {
        let w = text_width(match_text, theme::strong(12.0)) + 20.0;
        let rect = Rect::from_min_max(pos2(x - w, cy - 14.0), pos2(x, cy + 14.0));
        if text_button(ui, rect, ui_id.with("match"), match_text, Face::Plain, r)
            .on_hover_text(match_hint)
            .clicked()
        {
            *matching = true;
        }
        x = rect.left() - 8.0;
    }
    let gain_rect = Rect::from_min_max(pos2(x - 66.0, cy - 14.0), pos2(x, cy + 14.0));
    let mut gain = insert.gain_db;
    let response = ui
        .scope_builder(UiBuilder::new().max_rect(gain_rect), |ui| {
            let v = &mut ui.style_mut().visuals;
            for w in [
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
            ] {
                w.bg_fill = field_fill(&p);
                w.weak_bg_fill = field_fill(&p);
                w.corner_radius = CornerRadius::same(r as u8);
            }
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, button_edge(&p));
            ui.style_mut().override_font_id = Some(FontId::monospace(12.0));
            ui.add_sized(
                gain_rect.size(),
                egui::DragValue::new(&mut gain)
                    .range(-24.0..=24.0)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .suffix(" dB"),
            )
        })
        .inner
        .on_hover_text(tr(
            "Gain after the effect, kept in the mix and the export. Double-click to type",
        ));
    if gain != insert.gain_db {
        doc.history_mut()
            .edit_merged(tr("Insert gain"), &format!("insert-gain:{}", id.0), |p| {
                routing::set_insert_gain(p, channel, id, gain)
            });
    }
    if response.drag_stopped() || response.lost_focus() {
        doc.history_mut().end_gesture();
    }
    let label = painter.text(
        pos2(gain_rect.left() - 8.0, cy),
        Align2::RIGHT_CENTER,
        tr("Output").to_uppercase(),
        theme::strong(9.5),
        p.weak,
    );
    let right = label.left() - 14.0;

    // Center: the levels in and out.
    let room = right - left;
    let label_w = 24.0;
    let lufs_w = 80.0;
    let mut bars = ((room - 20.0) / 2.0 - label_w - 16.0 - lufs_w).min(120.0);
    let lufs = bars >= 50.0;
    if !lufs {
        bars = ((room - 20.0) / 2.0 - label_w - 8.0).min(120.0);
    }
    if bars >= 30.0 {
        let group = label_w + 8.0 + bars + if lufs { 8.0 + lufs_w } else { 0.0 };
        let mut gx = left + (room - (group * 2.0 + 20.0)) / 2.0;
        for (label, peaks, loudness) in [
            (tr("IN"), [in_l, in_r], in_lufs),
            (tr("OUT"), [out_l, out_r], out_lufs),
        ] {
            painter.text(
                pos2(gx, cy),
                Align2::LEFT_CENTER,
                label,
                theme::strong(9.5),
                p.weak,
            );
            let meter = Rect::from_min_size(pos2(gx + label_w + 8.0, cy - 6.0), vec2(bars, 12.0));
            paint_meter(&painter, &p, meter, peaks);
            if lufs {
                let text = if loudness > -70.0 {
                    format!("{loudness:5.1} LUFS")
                } else {
                    "   - LUFS".to_owned()
                };
                painter.text(
                    pos2(meter.right() + 8.0, cy),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::monospace(11.0),
                    p.text,
                );
            }
            gx += group + 20.0;
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
}

/// Two bars, left and right, from -48 dB to 0.
fn paint_meter(painter: &Painter, p: &Palette, rect: Rect, peaks: [f32; 2]) {
    for (i, db) in peaks.into_iter().enumerate() {
        let bar = Rect::from_min_size(
            rect.min + vec2(0.0, i as f32 * 7.0),
            vec2(rect.width(), 5.0),
        );
        painter.rect_filled(bar, 2.0, field_fill(p));
        let x = |db: f32| bar.left() + ((db + 48.0) / 48.0).clamp(0.0, 1.0) * bar.width();
        for (low, high, color) in [
            (-48.0, -12.0, theme::PLAY),
            (-12.0, -3.0, theme::SOLO),
            (-3.0, 0.0, theme::RECORD),
        ] {
            if db > low {
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(x(low), bar.top()),
                        pos2(x(db.min(high)), bar.bottom()),
                    ),
                    if low == -48.0 { 2.0 } else { 0.0 },
                    color,
                );
            }
        }
    }
}

/// Which track drives the detection of a compressor or gate; returns where the button ends.
fn sidechain_button(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &Insert,
    at: Pos2,
) -> f32 {
    let p = theme::current(ui);
    let current = insert.sidechain;
    let tracks = key_tracks(doc, channel);
    let source = current
        .and_then(|id| tracks.iter().find(|t| t.0 == id).map(|t| t.1.clone()))
        .unwrap_or_else(|| tr("Off").to_owned());
    let painter = ui.painter().clone();
    let label = tr("Sidechain");
    let font = FontId::proportional(12.0);
    let w1 = painter
        .layout_no_wrap(label.to_owned(), font.clone(), p.weak)
        .size()
        .x;
    let w2 = painter
        .layout_no_wrap(source.clone(), font.clone(), p.text)
        .size()
        .x
        .min(140.0);
    let rect = Rect::from_min_size(
        pos2(at.x, at.y - 14.0),
        vec2(10.0 + 13.0 + 7.0 + w1 + 5.0 + w2 + 8.0 + 10.0 + 10.0, 28.0),
    );
    let response = button(ui, rect, ui.scope_id().with("sidechain"), true);
    paint_face(ui, rect, radius(&p), &response, Face::Plain);
    let key = star_color(&p);
    if current.is_some() {
        painter.rect_stroke(rect, radius(&p), Stroke::new(1.0, key), StrokeKind::Inside);
    }
    let mut x = rect.left() + 10.0;
    paint_glyph(
        &painter,
        Rect::from_min_size(pos2(x, at.y - 6.5), vec2(13.0, 13.0)),
        Glyph::Key,
        key,
    );
    x += 20.0;
    painter.text(
        pos2(x, at.y),
        Align2::LEFT_CENTER,
        label,
        font.clone(),
        p.weak,
    );
    x += w1 + 5.0;
    painter
        .with_clip_rect(Rect::from_min_size(pos2(x, rect.top()), vec2(w2, 28.0)))
        .text(pos2(x, at.y), Align2::LEFT_CENTER, &source, font, p.text);
    paint_glyph(
        &painter,
        Rect::from_center_size(pos2(rect.right() - 14.0, at.y), vec2(10.0, 10.0)),
        Glyph::Down,
        p.weak,
    );
    let response = response.on_hover_text(tr(
        "Detect on another track's output, e.g. duck the bass with the kick",
    ));
    egui::Popup::menu(&response).show(|ui| sidechain_menu(ui, doc, channel, insert));
    rect.right()
}

/// The tracks that can key an insert of `channel`: those carrying audio, but its own.
fn key_tracks(doc: &Document, channel: Channel) -> Vec<(audiostrada_core::TrackId, String)> {
    let own = channel.track(doc.project());
    doc.project()
        .tracks
        .iter()
        .filter(|t| Some(t.id) != own && t.kind.carries_audio())
        .map(|t| (t.id, t.name.clone()))
        .collect()
}

/// The choices of an insert's sidechain, off or a track: in the bar's menu and the insert's
/// right-click menu.
pub(crate) fn sidechain_menu(ui: &mut Ui, doc: &mut Document, channel: Channel, insert: &Insert) {
    let current = insert.sidechain;
    let mut chosen = None;
    if ui
        .selectable_label(current.is_none(), tr("Off (own signal)"))
        .clicked()
    {
        chosen = Some(None);
        ui.close();
    }
    for (id, track) in key_tracks(doc, channel) {
        if ui.selectable_label(current == Some(id), track).clicked() {
            chosen = Some(Some(id));
            ui.close();
        }
    }
    if let Some(source) = chosen {
        let id = insert.id;
        doc.history_mut().edit(tr("Sidechain"), |p| {
            let inserts = match channel {
                Channel::Master => Some(&mut p.master.inserts),
                Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
                Channel::Clip(c) => audiostrada_core::edit::clip_mut(p, c).map(|c| &mut c.inserts),
            };
            if let Some(i) = inserts.and_then(|l| l.iter_mut().find(|i| i.id == id)) {
                i.sidechain = source;
            }
        });
    }
}
