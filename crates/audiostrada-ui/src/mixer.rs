//! Mixing console: one vertical strip per track (input, inserts, sends, routing, VCA, pan,
//! fader and meter), the master with loudness readings on the right, insert editors in
//! windows. Every change goes through the undo history.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use audiostrada_core::automation::{AutomationMode, AutomationTarget};
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{History, InsertId, Project, Track, TrackId, TrackKind};
use audiostrada_io::tools::gain_to_db;
use eframe::egui::{self, Align, Color32, Layout, Rect, Response, RichText, Sense, Ui, pos2, vec2};

use crate::dialogs;
use crate::document::Document;
use crate::effect_editors;
use crate::i18n::tr;
use crate::settings::{ERROR, OK, WARN};
use crate::theme;
use crate::trf;
use crate::widgets::{self, MeterState};
use audiostrada_core::groups::{self, Link};

mod rack;

const STRIP_WIDTH: f32 = 104.0;
const MIN_FADER: f32 = 60.0;
/// Name plate and the space after it.
const PLATE: f32 = 26.0;
/// Width of a strip of the compact mixer.
const NARROW_WIDTH: f32 = 46.0;
/// Height kept above a narrow strip's fader for its name.
const NAME_ROOM: f32 = 110.0;
/// Strip height from which a docked mixer shows the insert and send slots, as in its own
/// window: room for the fader and both lists.
const FULL_STRIPS_HEIGHT: f32 = 560.0;
/// Height of the channel strip section of tall strips.
const STRIP_SECTION: f32 = 56.0;
const FADER_MIN_DB: f32 = -60.0;
const FADER_MAX_DB: f32 = audiostrada_core::model::MAX_TRACK_GAIN_DB;

#[derive(Debug)]
pub struct MixerView {
    doc: Rc<RefCell<Document>>,
    /// Narrow strips, but for the one opened by a click.
    pub compact: bool,
    rack: rack::RackState,
    expanded: Option<TrackId>,
    /// Height under the fader rows, as measured last frame.
    bottom_last: f32,
    master_levels: MeterState,
    last_frame: Instant,
    /// Plugins whose own window opens (or comes to the front) once they are loaded.
    open_gui: Vec<(Channel, InsertId)>,
    /// Where this view's windows were, kept in the context's data (see [`WindowPlaces`]).
    places: egui::Id,
    /// The project went to the background: its windows reopen where they were last.
    parked: bool,
    /// Commands chosen in track menus, run by the application.
    pub requests: Vec<crate::actions::Action>,
    /// Track to export, chosen in its menu.
    pub stem_request: Option<TrackId>,
    editors: Vec<(Channel, InsertId)>,
    /// Tracks whose name plate is being dragged to another place.
    dragging: Option<Vec<TrackId>>,
    /// Insert being dragged (its channel, id and name), and where it would land.
    fx_drag: Option<(Channel, InsertId, String)>,
    /// Window the insert drag started in: it is dropped there.
    fx_viewport: egui::ViewportId,
    fx_target: Option<FxTarget>,
    /// The strip under the pointer, when not over its insert list: the end of that list.
    fx_fallback: Option<FxTarget>,
    /// Editor to bring to the front (its insert was clicked again).
    raise: Option<InsertId>,
    error: Option<String>,
    browser: crate::effect_browser::Shared,
    /// Parameters of open plugin editors.
    plugin_params: HashMap<InsertId, Vec<audiostrada_plugins::ParamDesc>>,
    param_filter: String,
    /// Fader travel, sized to fill the panel height.
    fader_height: f32,
    /// Memory of the dedicated effect editors.
    editor_states: HashMap<InsertId, effect_editors::EditorState>,
    /// Sections shown, from the panel height.
    sizes: StripSizes,
    /// Tallest parts above and below the faders at the last frame, so every fader lines up.
    top_height: f32,
    /// Height of the tracks' fader row, which the master's matches.
    row_height: f32,
    /// Tallest top part of the last frame: strips drawn before a taller one still line up.
    top_last: f32,
    /// The same for the narrow strips of the compact mixer, which line up among themselves:
    /// the strip open full width has a taller top.
    narrow_top: f32,
    narrow_last: f32,
    bottom_height: f32,
    /// The mixer is in its own window rather than docked.
    pub detached: bool,
    /// The strips show their insert and send slots (own window, or docked and tall enough);
    /// else buttons beside the fader open them in windows, leaving the height to the fader.
    full_strips: bool,
    /// Insert, send and monitor lists open in their own windows (docked mixer).
    lists: Vec<(Channel, List)>,
}

/// A list of a strip shown in its own window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum List {
    Inserts,
    Sends,
    Monitor,
}

/// How an insert's slot looks.
struct Slot<'a> {
    name: &'a str,
    color: Color32,
    lit: bool,
    error: bool,
    /// The delay it adds, when it adds one.
    latency: Option<&'a str>,
}

/// An insert's slot: its family's color, an on/off button, its name and the delay it adds.
/// Returns the name's response (click, drag, menu) and the button's.
fn insert_slot(ui: &mut Ui, size: egui::Vec2, salt: u64, slot: &Slot) -> (Response, Response) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    // Clear of the family bar.
    let power_rect = Rect::from_min_size(rect.min + vec2(4.0, 0.0), vec2(size.y, size.y));
    let name_rect = Rect::from_min_max(pos2(power_rect.right(), rect.top()), rect.max);
    let name = ui.interact(
        name_rect,
        ui.scope_id().with(("slot", salt)),
        Sense::click_and_drag(),
    );
    let power = ui.interact(
        power_rect,
        ui.scope_id().with(("power", salt)),
        Sense::click(),
    );
    let painter = ui.painter();
    let fill = if name.hovered() || power.hovered() {
        p.hover
    } else {
        p.surface
    };
    painter.rect_filled(rect, 3.0, fill);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
        egui::CornerRadius {
            nw: 3,
            sw: 3,
            ..Default::default()
        },
        slot.color.gamma_multiply(if slot.lit { 1.0 } else { 0.3 }),
    );
    let glyph = if !slot.lit && power.hovered() {
        p.text
    } else {
        theme::power_ink(slot.lit, &p)
    };
    theme::paint_power(painter, power_rect.shrink(3.0), glyph);
    let ink = if slot.error {
        ERROR
    } else if slot.lit {
        p.text
    } else {
        p.weak
    };
    let mut text_rect = name_rect.shrink2(vec2(2.0, 0.0));
    if let Some(latency) = slot.latency {
        let galley =
            painter.layout_no_wrap(latency.to_owned(), egui::FontId::proportional(8.5), p.weak);
        let at = pos2(
            text_rect.right() - galley.size().x,
            text_rect.center().y - galley.size().y / 2.0,
        );
        text_rect.max.x = at.x - 3.0;
        painter.galley(at, galley, p.weak);
    }
    let font = egui::FontId::proportional(if size.y >= 20.0 { 11.5 } else { 10.5 });
    let galley = painter.layout_no_wrap(slot.name.to_owned(), font, ink);
    let width = galley.size().x.min(text_rect.width());
    let at = pos2(
        text_rect.left(),
        text_rect.center().y - galley.size().y / 2.0,
    );
    painter.with_clip_rect(text_rect).galley(at, galley, ink);
    if !slot.lit {
        let y = text_rect.center().y;
        painter.line_segment(
            [pos2(at.x, y), pos2(at.x + width, y)],
            egui::Stroke::new(1.0, p.weak.gamma_multiply(0.7)),
        );
    }
    (name, power)
}

/// A send's level as a bar in its destination's color: drag (Shift for fine steps), wheel,
/// double-click for 0 dB.
fn send_level(
    ui: &mut Ui,
    width: f32,
    db: f32,
    color: Color32,
    muted: bool,
) -> (Option<f32>, Response) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 14.0), Sense::click_and_drag());
    let position = widgets::fader_position(db);
    let mut changed = None;
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        let delta = response.drag_delta().x / width * if fine { 0.2 } else { 1.0 };
        changed = Some(widgets::fader_db(position + delta));
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            changed = Some(
                (db + wheel.signum() * if fine { 0.1 } else { 0.5 })
                    .clamp(FADER_MIN_DB, FADER_MAX_DB),
            );
        }
    }
    if response.double_clicked() {
        changed = Some(0.0);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, p.base);
    let shown = changed.map_or(position, widgets::fader_position);
    let fill = Rect::from_min_size(rect.min, vec2(rect.width() * shown, rect.height()));
    painter.rect_filled(
        fill,
        3.0,
        color.gamma_multiply(if muted { 0.25 } else { 0.7 }),
    );
    let unity = rect.left() + rect.width() * widgets::fader_position(0.0);
    painter.line_segment(
        [
            pos2(unity, rect.top() + 2.0),
            pos2(unity, rect.bottom() - 2.0),
        ],
        egui::Stroke::new(1.0, p.text.gamma_multiply(0.35)),
    );
    let value = changed.unwrap_or(db);
    let text = if value <= FADER_MIN_DB + 0.05 {
        "-∞".to_owned()
    } else {
        format!("{value:.1}")
    };
    painter.text(
        pos2(rect.right() - 3.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::proportional(8.5),
        p.text.gamma_multiply(if muted { 0.5 } else { 0.9 }),
    );
    (changed, response)
}

/// The modules of the channel strip: label, help and family.
const STRIP_MODULES: [(&str, &str, &str); 5] = [
    ("HP", "Filters: high pass and low pass", "EQ & Filter"),
    ("GT", "Gate", "Dynamics"),
    ("CP", "Compressor", "Dynamics"),
    ("EQ", "Four-band EQ", "EQ & Filter"),
    ("DR", "Drive: a gentle saturation", "Color"),
];

/// Each module of a channel's strip (its switch parameter) and whether it is on.
/// The modules of a channel's strip in the order they play: their index into
/// [`STRIP_MODULES`] and whether each is on.
fn strip_modules(project: &Project, channel: Channel) -> [(usize, bool); 5] {
    let strip = routing::strip(project, channel);
    let order = audiostrada_dsp::channel::module_order(
        strip
            .and_then(|s| s.params.get("order").copied())
            .unwrap_or(0.0),
    );
    order.map(|m| {
        let id = audiostrada_dsp::channel::MODULES[m];
        (
            m,
            strip.is_some_and(|s| !s.bypass && s.params.get(id).is_some_and(|&v| v >= 0.5)),
        )
    })
}

/// A thin bar per send of a compact strip, as long as its level, in its destination's color.
fn send_pills(ui: &mut Ui, project: &Project, track: &Track, width: f32) {
    let p = theme::current(ui);
    if track.sends.is_empty() {
        return;
    }
    ui.add_space(2.0);
    for send in &track.sends {
        let color = project.track(send.target).map_or(Color32::GRAY, |t| {
            Color32::from_rgb(t.color[0], t.color[1], t.color[2])
        });
        let (rect, response) = ui.allocate_exact_size(vec2(width, 4.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, p.base);
        let level = widgets::fader_position(send.gain_db);
        let fill = Rect::from_min_size(rect.min, vec2(rect.width() * level, rect.height()));
        ui.painter().rect_filled(
            fill,
            2.0,
            color.gamma_multiply(if send.mute { 0.25 } else { 0.85 }),
        );
        let name = project.track(send.target).map_or("?", |t| t.name.as_str());
        response.on_hover_text(trf!(
            "Send to {name}: {db} dB{pre}",
            name = name,
            db = format!("{:.1}", send.gain_db),
            pre = if send.pre_fader {
                tr(" (pre-fader)")
            } else {
                ""
            }
        ));
    }
}

/// Where the `shown`-th insert of a list (as shown, without the channel strip) is in the whole
/// list; past the last one for a larger index.
fn listed_index(project: &Project, channel: Channel, monitor: bool, shown: usize) -> usize {
    let list: Vec<&audiostrada_core::Insert> = routing::inserts(project, channel)
        .iter()
        .filter(|i| i.monitor == monitor)
        .collect();
    let visible: Vec<usize> = (0..list.len())
        .filter(|&i| list[i].effect != routing::STRIP)
        .collect();
    match visible.get(shown) {
        Some(&i) => i,
        None => visible.last().map_or(list.len(), |&i| i + 1).max(
            list.iter()
                .position(|i| i.effect == routing::STRIP)
                .map_or(0, |s| s + 1),
        ),
    }
}

/// The family of an insert's effect (built-in or plugin), for its color.
fn family(doc: &Document, effect: &str) -> &'static str {
    if effect == routing::PARALLEL {
        return "Utility";
    }
    match audiostrada_dsp::info(effect) {
        Some(info) => info.category,
        None => doc.player().plugins().find(effect).map_or("", |d| {
            if d.instrument {
                "Instrument"
            } else {
                crate::plugin_categories::guess(&d.tags)
            }
        }),
    }
}

/// The delay an insert adds, as a short label, when it adds one.
fn latency_label(doc: &Document, channel: Channel, id: InsertId) -> Option<String> {
    let samples = doc.player().insert_latency(channel, id);
    let rate = doc.player().engine().sample_rate().max(1);
    (samples > 0).then(|| {
        let ms = samples as f32 * 1000.0 / rate as f32;
        if ms < 10.0 {
            format!("{ms:.1} ms")
        } else {
            format!("{ms:.0} ms")
        }
    })
}

/// Height of an insert or send slot, with its spacing.
const SLOT: f32 = 20.0;

/// An empty slot of a list: clicking it adds an effect or a send.
fn empty_slot(ui: &mut Ui, width: f32, hint: &str) -> Response {
    add_slot(ui, vec2(width, SLOT - 2.0), "+", hint)
}

/// A slot with a faint outline, labelled with what clicking it adds.
fn add_slot(ui: &mut Ui, size: egui::Vec2, label: &str, hint: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let p = theme::current(ui);
    let hovered = response.hovered();
    let fill = if hovered { p.hover } else { p.panel };
    ui.painter().rect_filled(rect, 3.0, fill);
    let line = p.weak.gamma_multiply(if hovered { 0.8 } else { 0.35 });
    ui.painter().rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, line),
        egui::StrokeKind::Inside,
    );
    let plus = p.weak.gamma_multiply(if hovered { 1.0 } else { 0.5 });
    let size = if label == "+" { 13.0 } else { 11.0 };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(size),
        plus,
    );
    response.on_hover_text(hint)
}

/// A small cross without a frame, lit when hovered.
fn remove_button(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let p = theme::current(ui);
    let ink = if response.hovered() {
        ERROR
    } else {
        p.weak.gamma_multiply(0.6)
    };
    let r = rect.shrink(5.5);
    let stroke = egui::Stroke::new(1.4, ink);
    ui.painter()
        .line_segment([r.left_top(), r.right_bottom()], stroke);
    ui.painter()
        .line_segment([r.right_top(), r.left_bottom()], stroke);
    response.on_hover_text(tr("Remove"))
}

/// Where a dragged insert would land: before the `index`-th insert of a channel's list.
#[derive(Clone, Copy, Debug)]
struct FxTarget {
    channel: Channel,
    monitor: bool,
    index: usize,
    line: [egui::Pos2; 2],
    /// A whole strip to outline instead of a line.
    outline: Option<Rect>,
}

/// Adds the inserts of `chain` at the end of a channel's list; returns their ids.
fn apply_chain(
    doc: &mut Document,
    (channel, monitor): (Channel, bool),
    chain: &crate::chains::Chain,
) -> Vec<InsertId> {
    let mut added = Vec::new();
    doc.history_mut()
        .edit(&trf!("Add chain {name}", name = chain.label()), |p| {
            for insert in &chain.inserts {
                let mut insert = insert.clone();
                insert.monitor = monitor;
                added.extend(routing::add_insert_copy(p, channel, &insert));
            }
        });
    if added.is_empty() {
        doc.notify(tr("This track cannot hold effects").into());
    }
    added
}

/// "Save as a chain" submenu: names the effects of a channel's list and saves them, with
/// their files as absolute paths.
pub(crate) fn save_chain_menu(
    ui: &mut Ui,
    doc: &mut Document,
    (channel, monitor): (Channel, bool),
    suggested: &str,
) {
    let inserts: Vec<audiostrada_core::Insert> = routing::inserts(doc.project(), channel)
        .iter()
        .filter(|i| i.monitor == monitor)
        .cloned()
        .map(|mut i| {
            for file in i.files.values_mut() {
                *file = doc.resolve(file);
            }
            i
        })
        .collect();
    let button = ui.add_enabled(
        !inserts.is_empty(),
        egui::Button::new(tr("Save effects as a chain…")),
    );
    if button.clicked() {
        let dialog = ChainDialog {
            viewport: ui.ctx().viewport_id(),
            name: suggested.to_owned(),
            inserts,
            focused: false,
        };
        ui.ctx()
            .data_mut(|d| d.insert_temp(chain_dialog_id(), dialog));
        ui.close();
    }
}

/// The "Save as a chain" dialog, while open.
#[derive(Clone)]
struct ChainDialog {
    /// The window it shows in: the one whose menu opened it.
    viewport: egui::ViewportId,
    name: String,
    inserts: Vec<audiostrada_core::Insert>,
    focused: bool,
}

fn chain_dialog_id() -> egui::Id {
    egui::Id::unique("save-chain-dialog")
}

/// Shows the "Save as a chain" dialog when it was opened from the window being drawn.
pub(crate) fn save_chain_dialog(ctx: &egui::Context, doc: &mut Document) {
    let id = chain_dialog_id();
    let Some(mut dialog) = ctx.data(|d| d.get_temp::<ChainDialog>(id)) else {
        return;
    };
    if dialog.viewport != ctx.viewport_id() {
        return;
    }
    let mut done = false;
    let modal = egui::Modal::new(egui::Id::unique("save-chain-modal")).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.heading(tr("Save effects as a chain"));
        ui.weak(trf!("{count} effect(s)", count = dialog.inserts.len()));
        ui.add_space(6.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut dialog.name)
                .hint_text(tr("Chain name"))
                .desired_width(f32::INFINITY),
        );
        if !dialog.focused {
            field.request_focus();
            dialog.focused = true;
        }
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let named = !dialog.name.trim().is_empty();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(named, egui::Button::new(tr("Save")))
                .clicked()
                || (enter && named)
            {
                let result = crate::chains::root()
                    .ok_or_else(|| tr("no configuration folder").to_owned())
                    .and_then(|root| crate::chains::save(&root, &dialog.name, &dialog.inserts));
                match result {
                    Ok(()) => doc.notify(trf!("Chain \"{name}\" saved", name = dialog.name.trim())),
                    Err(e) => doc.notify(trf!("Cannot save the chain: {e}", e = e)),
                }
                done = true;
            }
            if ui.button(tr("Cancel")).clicked() {
                done = true;
            }
        });
    });
    if done || modal.should_close() {
        ctx.data_mut(|d| d.remove::<ChainDialog>(id));
    } else {
        ctx.data_mut(|d| d.insert_temp(id, dialog));
    }
}

/// Ctrl (Cmd) held: the control moves its own track alone, out of its group.
pub(crate) fn clutch(ui: &Ui) -> bool {
    ui.input(|i| i.modifiers.command)
}

/// The other selected tracks when `id` is part of a selection of several, else none.
fn selection_with(doc: &Document, id: TrackId) -> Vec<TrackId> {
    if doc.is_track_selected(id) && doc.selected_tracks().len() > 1 {
        doc.selected_tracks().to_vec()
    } else {
        Vec::new()
    }
}

/// The audio tracks of the selection `id` is part of, else `id` alone.
fn audio_selection_with(doc: &Document, project: &Project, id: TrackId) -> Vec<TrackId> {
    let mut tracks = selection_with(doc, id);
    tracks.retain(|t| {
        project
            .track(*t)
            .is_some_and(|t| t.kind == TrackKind::Audio)
    });
    if tracks.is_empty() {
        tracks.push(id);
    }
    tracks
}

/// Payload of a strip or track header being dragged to reorder tracks.
#[derive(Clone, Debug)]
pub(crate) struct DraggedTracks(pub Vec<TrackId>);

/// An editor in its own system window, which can go to another screen; false once closed.
/// Numbers the mixer views, one per project tab, to keep their windows' places apart.
static NEXT_VIEW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Where the windows of a mixer view are: last seen, and given when each one opened (kept
/// while it stays open: a builder that changed would move the window under the user's hand).
#[derive(Clone, Debug, Default)]
struct WindowPlaces {
    last: HashMap<egui::ViewportId, egui::Pos2>,
    opened: HashMap<egui::ViewportId, Option<egui::Pos2>>,
}

#[allow(clippy::too_many_arguments)]
fn editor_window(
    ctx: &egui::Context,
    places: egui::Id,
    key: impl std::hash::Hash + std::fmt::Debug + Copy,
    title: &str,
    size: [f32; 2],
    raise: bool,
    fit: bool,
    add: impl FnOnce(&mut Ui),
) -> bool {
    fn contents(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> f32 {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, add)
            .content_size
            .y
    }
    let id = egui::ViewportId::from_hash_of(key);
    if raise {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
    }
    let position = ctx.data_mut(|d| {
        let p = d.get_temp_mut_or_default::<WindowPlaces>(places);
        let last = p.last.get(&id).copied();
        *p.opened.entry(id).or_insert(last)
    });
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("{title} — AudioStrada"))
        .with_inner_size(size)
        .with_min_inner_size([280.0, 180.0])
        .with_window_level(crate::floating_level(ctx, crate::Floating::Editor(id)));
    let builder = match position {
        Some(p) => builder.with_position(p),
        None => builder,
    };
    let mut add = Some(add);
    let open = ctx.show_viewport_immediate(id, builder, |ui, _| {
        // Before the widgets, so that Space plays instead of pressing a focused button.
        crate::window_shortcuts(ui.ctx());
        let mut height = None;
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(add) = add.take() {
                height = Some(contents(ui, add));
            }
        });
        // The window follows its content's height, as the content changes.
        if fit && let Some(height) = height {
            let ctx = ui.ctx().clone();
            let wanted = (height
                + egui::Frame::central_panel(&ctx.global_style())
                    .total_margin()
                    .sum()
                    .y)
                .ceil();
            let key = egui::Id::unique(("fit-height", id));
            if ctx.data(|d| d.get_temp::<f32>(key)) != Some(wanted) {
                ctx.data_mut(|d| d.insert_temp(key, wanted));
                let (width, screen) = ctx.input(|i| {
                    let v = i.viewport();
                    (
                        v.inner_rect.map_or(size[0], |r| r.width()),
                        v.monitor_size.map_or(f32::MAX, |m| m.y - 60.0),
                    )
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    width,
                    wanted.min(screen),
                )));
            }
        }
        if let Some(rect) = ui.ctx().input(|i| i.viewport().outer_rect) {
            ui.ctx().data_mut(|d| {
                let p = d.get_temp_mut_or_default::<WindowPlaces>(places);
                p.last.insert(id, rect.min);
            });
        }
        !ui.ctx().input(|i| i.viewport().close_requested())
    });
    if !open {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<WindowPlaces>(places)
                .opened
                .remove(&id)
        });
    }
    // The window goes at the next frame of the one that opened it: right away. Its close
    // button is only read when that one draws: often enough.
    if open {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    } else {
        ctx.request_repaint();
    }
    open
}

/// Heights of the strip sections, from the panel height.
#[derive(Clone, Copy, Debug)]
struct StripSizes {
    fx: f32,
    sends: Option<f32>,
    input: bool,
}

impl StripSizes {
    /// Splits the strip height: the fader gets its share first, the insert and send lists
    /// what is left (`fixed` is the height of everything else).
    fn new(available: f32, fixed: f32) -> Self {
        // The channel strip section is taken from the fader, not from the lists.
        let fader = ((available * 0.4).clamp(120.0, 320.0) - STRIP_SECTION).max(120.0);
        let lists = available - fixed - fader;
        let fx = (available * 0.22).clamp(46.0, 150.0);
        let sends = (available * 0.11).clamp(34.0, 90.0);
        let (fx, sends) = if lists >= fx + sends {
            (fx, Some(sends))
        } else if lists >= 46.0 + 34.0 {
            let fx = (lists * 0.62).max(46.0);
            (fx, Some((lists - fx).max(34.0)))
        } else {
            // The controls and one insert at least: the list scrolls.
            (lists.max(44.0), None)
        };
        Self {
            fx,
            sends,
            input: available >= 520.0,
        }
    }

    fn lists(&self) -> f32 {
        self.fx + self.sends.unwrap_or(0.0)
    }

    fn rounded(self) -> Self {
        Self {
            fx: self.fx.round(),
            sends: self.sends.map(f32::round),
            input: self.input,
        }
    }

    /// Less than a pixel apart, with the same sections.
    fn close_to(&self, other: &Self) -> bool {
        let near = |a: f32, b: f32| (a - b).abs() < 1.0;
        self.input == other.input
            && near(self.fx, other.fx)
            && match (self.sends, other.sends) {
                (Some(a), Some(b)) => near(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

pub(crate) fn format_time(samples: u64, rate: u32) -> String {
    let secs = samples as f64 / f64::from(rate.max(1));
    format!("{:02}:{:06.3}", (secs / 60.0) as u64, secs % 60.0)
}

/// Ends a merged undo gesture once the pointer is released.
pub(crate) fn end_gesture(history: &mut History, response: &Response) {
    if released(response) {
        history.end_gesture();
    }
}

/// Display name of a built-in effect or plugin key.
pub(crate) fn effect_name(doc: &Document, key: &str) -> String {
    if key == routing::PARALLEL {
        return tr("Parallel chain").to_owned();
    }
    if let Some(info) = audiostrada_dsp::info(key) {
        return tr(info.name).to_owned();
    }
    doc.player()
        .plugins()
        .find(key)
        .map_or_else(|| key.to_owned(), |p| p.name.clone())
}

/// A built-in instrument or an instrument plugin.
fn is_instrument(doc: &Document, effect: &str) -> bool {
    audiostrada_dsp::info(effect).map_or_else(
        || {
            doc.player()
                .plugins()
                .find(effect)
                .is_some_and(|d| d.instrument)
        },
        |i| i.instrument,
    )
}

/// Name of a channel's owner in titles and hints.
fn owner_name(project: &Project, channel: Channel) -> &str {
    match channel {
        Channel::Master => tr("Master"),
        Channel::Track(t) => project.track(t).map_or("?", |t| t.name.as_str()),
    }
}

fn color(track: &Track) -> Color32 {
    Color32::from_rgb(track.color[0], track.color[1], track.color[2])
}

/// File slots of a built-in effect: what is loaded, with buttons to load or remove.
fn effect_files(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &audiostrada_core::Insert,
    info: &audiostrada_dsp::EffectInfo,
) {
    egui::Grid::new(("effect-files", insert.id.0))
        .num_columns(3)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for slot in info.files {
                ui.label(RichText::new(tr(slot.name)).strong());
                let loaded = insert.files.get(slot.id);
                ui.vertical(|ui| match loaded {
                    None => {
                        ui.add(egui::Label::new(RichText::new(tr("None")).weak()).extend());
                    }
                    Some(path) => {
                        let name = path
                            .file_stem()
                            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                        ui.label(name).on_hover_text(path.display().to_string());
                        match doc.player().effect_file(insert.id, slot.id) {
                            Some(Ok(text)) if !text.is_empty() => {
                                ui.label(RichText::new(text).small().weak());
                            }
                            Some(Err(e)) => {
                                ui.colored_label(ERROR, e);
                            }
                            _ => {}
                        }
                    }
                });
                ui.horizontal(|ui| {
                    load_button(ui, doc, channel, insert.id, slot);
                    if loaded.is_some() && ui.button(tr("Remove")).clicked() {
                        let _ = doc.set_effect_file(channel, insert.id, slot.id, None);
                    }
                });
                ui.end_row();
            }
        });
}

/// "Load…" for an effect's file slot, through the system's file chooser; true once a file
/// was chosen and loaded.
pub(crate) fn load_button(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: InsertId,
    slot: &'static audiostrada_dsp::FileSlot,
) -> bool {
    let open = ui.button(tr("Load…")).clicked();
    let (pack, name, extensions) = (
        slot.kind == audiostrada_dsp::FileKind::Pack,
        slot.name,
        slot.extensions,
    );
    let id = ("effect-file", channel, insert, slot.id);
    let Some(file) = crate::dialogs::ask(ui.ctx(), id, open, move || {
        let dialog = rfd::FileDialog::new().set_title(tr(name));
        if pack {
            dialog.pick_folder()
        } else {
            dialog.add_filter(tr(name), extensions).pick_file()
        }
    }) else {
        return false;
    };
    match doc.set_effect_file(channel, insert, slot.id, Some(&file)) {
        Ok(()) => true,
        Err(e) => {
            doc.notify(trf!(
                "Cannot load {file}: {e}",
                file = file.display(),
                e = e
            ));
            false
        }
    }
}

/// Track name on its color, at the bottom of a strip.
fn name_plate(
    ui: &mut Ui,
    width: f32,
    name: &str,
    color: Color32,
    folder: bool,
    icon: Option<crate::track_icons::TrackIcon>,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click_and_drag());
    let painter = ui.painter();
    if folder {
        // A folder: a tab on top of the plate.
        painter.rect_filled(
            Rect::from_min_size(rect.min - vec2(0.0, 3.0), vec2(width * 0.4, 4.0)),
            2.0,
            color,
        );
    }
    // Opaque, so the name reads the same on dark and light themes.
    let p = theme::current(ui);
    let fill = theme::mix(p.panel, color, if p.dark { 0.45 } else { 0.7 });
    let ink = theme::text_on(fill);
    painter.rect_filled(rect, 4.0, fill);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
        egui::CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        },
        color,
    );
    // The instrument at the left, the name centered in what is left.
    let mut text = rect.shrink2(vec2(5.0, 0.0));
    if let Some(icon) = icon.filter(|_| name.is_empty()) {
        let at = Rect::from_center_size(rect.center(), vec2(15.0, 15.0));
        crate::track_icons::paint(ui, at, icon, ink);
        return response;
    }
    if let Some(icon) = icon {
        let at =
            Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(14.0, 14.0));
        crate::track_icons::paint(ui, at, icon, ink);
        text.min.x = at.right() + 2.0;
    }
    let painter = ui.painter();
    let galley = painter.layout(name.to_owned(), theme::strong(12.0), ink, text.width());
    let galley = if galley.rows.len() > 1 {
        let mut job =
            egui::text::LayoutJob::simple_singleline(name.to_owned(), theme::strong(12.0), ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text.width());
        ui.fonts_mut(|f| f.layout_job(job))
    } else {
        galley
    };
    painter.galley(
        pos2(
            text.center().x - galley.size().x / 2.0,
            text.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    response
}

/// A track's name read upwards, in the room above a narrow strip's fader.
fn upright_name(ui: &Ui, room: Rect, name: &str) {
    let p = theme::current(ui);
    let mut job =
        egui::text::LayoutJob::simple_singleline(name.to_owned(), theme::strong(11.5), p.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room.height() - 8.0);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let at = pos2(room.center().x - galley.size().y / 2.0, room.bottom() - 4.0);
    ui.painter_at(room).add(
        egui::epaint::TextShape::new(at, galley, p.text).with_angle(-std::f32::consts::FRAC_PI_2),
    );
}

/// A value in a small well `width` wide.
fn readout(ui: &mut Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    theme::well(ui.painter(), rect, ui.visuals().dark_mode);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(11.0),
        theme::current(ui).text,
    );
}

fn pan_label(pan: f32) -> String {
    let percent = (pan * 100.0).round() as i32;
    match percent {
        0 => "C".to_owned(),
        p if p < 0 => trf!("L: {amount}%", amount = -p),
        p => trf!("R: {amount}%", amount = p),
    }
}

fn db_label(db: f32) -> String {
    if db <= FADER_MIN_DB {
        "-∞".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

/// What each column of the groups table links, with its help.
fn link_labels(link: Link) -> (&'static str, &'static str) {
    match link {
        Link::Gain => (
            tr("Level"),
            tr("Faders move together, keeping their balance"),
        ),
        Link::Pan => (tr("Pan"), tr("Pans move together, by as much")),
        Link::Sends => (
            tr("Sends"),
            tr("Sends to the same bus move together, keeping their balance"),
        ),
        Link::Mute => (tr("Mute"), tr("Mute")),
        Link::Solo => (tr("Solo"), tr("Solo")),
        Link::Arm => (tr("Arm"), tr("Record arm")),
        Link::Fx => (tr("FX"), tr("Bypass of the inserts")),
        Link::Automation => (tr("Auto"), tr("Automation mode")),
        Link::Polarity => ("Ø", tr("Polarity")),
        Link::Edit => (
            tr("Edit"),
            tr(
                "Clicking a clip or a track in the timeline selects the members' too: cuts, \
                 moves, trims and fades apply to all",
            ),
        ),
    }
}

/// Track groups: create, rename, choose what they link, switch off or delete.
fn groups_menu(ui: &mut Ui, doc: &mut Document) {
    ui.set_min_width(760.0);
    ui.horizontal(|ui| {
        let selected = doc.selected_tracks().len();
        if ui
            .add_enabled(selected > 0, egui::Button::new(tr("Group selected tracks")))
            .on_hover_text(tr(
                "Ctrl+G. Select tracks first (click, Ctrl or Shift on their names)",
            ))
            .clicked()
        {
            group_selection(doc);
        }
        ui.weak(trf!("{count} track(s) selected", count = selected));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut on = !doc.project().groups_off;
            if ui
                .checkbox(&mut on, tr("Groups on"))
                .on_hover_text(tr(
                    "Ctrl+Shift+G. Off, every track behaves on its own. To move one track \
                     alone, hold Ctrl while using its control",
                ))
                .changed()
            {
                doc.history_mut().modify_unrecorded(|p| p.groups_off = !on);
            }
        });
    });
    let groups = doc.project().groups.clone();
    if groups.is_empty() {
        ui.weak(tr(
            "No groups yet. Members of a group share level changes, mute, solo, arm, editing \
             and more. A folder can also act as a group, from its menu.",
        ));
        return;
    }
    let project = doc.project().clone();
    egui::Grid::new("groups-matrix")
        .num_columns(15)
        .spacing([6.0, 6.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(RichText::new(tr("Name")).size(10.0).weak());
            ui.label(RichText::new(tr("On")).size(10.0).weak());
            for link in Link::ALL {
                let (label, help) = link_labels(link);
                ui.label(RichText::new(label).size(10.0).weak())
                    .on_hover_text(help);
            }
            ui.label(RichText::new(tr("Members")).size(10.0).weak());
            ui.label("");
            ui.end_row();
            for (i, group) in groups.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                ui.painter().circle_filled(
                    rect.center(),
                    6.0,
                    Color32::from_rgb(group.color[0], group.color[1], group.color[2]),
                );
                let mut name = group.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(90.0))
                    .changed()
                {
                    doc.history_mut()
                        .edit_merged(tr("Rename group"), &format!("group:{i}"), |p| {
                            if let Some(g) = p.groups.get_mut(i) {
                                g.name = name;
                            }
                        });
                }
                let mut g = group.clone();
                let mut changed = ui.checkbox(&mut g.active, "").changed();
                for link in Link::ALL {
                    let help = link_labels(link).1;
                    changed |= ui
                        .checkbox(g.link_mut(link), "")
                        .on_hover_text(help)
                        .changed();
                }
                if changed {
                    doc.history_mut().edit(tr("Group links"), |p| {
                        if let Some(slot) = p.groups.get_mut(i) {
                            *slot = g;
                        }
                    });
                }
                let tracks = groups::members(&project, group);
                let names: Vec<&str> = tracks
                    .iter()
                    .filter_map(|t| project.track(*t).map(|t| t.name.as_str()))
                    .collect();
                let summary = match names.len() {
                    0 => tr("none").to_owned(),
                    1..=2 => names.join(", "),
                    n => format!("{}, … ({n})", names[..2].join(", ")),
                };
                let folder = group.folder.and_then(|f| project.track(f));
                let summary = match folder {
                    Some(f) => trf!("Folder {name}: {members}", name = f.name, members = summary),
                    None => summary,
                };
                ui.label(RichText::new(summary).size(11.0))
                    .on_hover_text(names.join("\n"));
                ui.horizontal(|ui| {
                    if ui
                        .small_button(tr("Select"))
                        .on_hover_text(tr("Select the members"))
                        .clicked()
                    {
                        doc.set_track_selection(tracks.clone());
                    }
                    let selected = doc.selected_tracks().to_vec();
                    if folder.is_none()
                        && ui
                            .add_enabled(
                                !selected.is_empty(),
                                egui::Button::new(tr("Add selected")).small(),
                            )
                            .clicked()
                    {
                        doc.history_mut().edit(tr("Join group"), |p| {
                            for &t in &selected {
                                groups::set_group(p, t, Some(i));
                            }
                        });
                    }
                    if ui.small_button(tr("Delete")).clicked() {
                        doc.history_mut().edit(tr("Delete group"), |p| {
                            if i < p.groups.len() {
                                p.groups.remove(i);
                            }
                        });
                    }
                });
                ui.end_row();
            }
        });
}

/// Makes a folder act as a group of its tracks, or stop.
pub(crate) fn toggle_folder_group(doc: &mut Document, folder: TrackId) {
    let project = doc.project();
    let existing = groups::folder_group(project, folder);
    let Some(track) = project.track(folder) else {
        return;
    };
    let (name, color) = (track.name.clone(), track.color);
    doc.history_mut()
        .edit(tr("Folder group"), |p| match existing {
            Some(i) => {
                p.groups.remove(i);
            }
            None => {
                let mut group = groups::TrackGroup::new(&name, color);
                group.folder = Some(folder);
                p.groups.push(group);
            }
        });
}

/// Adds a VCA controlling the selected tracks.
pub(crate) fn add_vca(doc: &mut Document) {
    let n = doc
        .project()
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Vca)
        .count()
        + 1;
    let members: Vec<TrackId> = doc
        .selected_tracks()
        .iter()
        .copied()
        .filter(|&t| {
            doc.project()
                .track(t)
                .is_some_and(|t| t.kind == TrackKind::Audio)
        })
        .collect();
    let mut created = None;
    doc.history_mut().edit(tr("Add VCA"), |p| {
        let vca = routing::add_vca(p, &trf!("VCA {n}", n = n));
        for &t in &members {
            let _ = routing::set_vca(p, t, Some(vca));
        }
        created = Some(vca);
    });
    if members.is_empty() {
        doc.notify(tr("VCA added: assign tracks from their output menu (→ Master)").into());
    } else if let Some(vca) = created {
        doc.select_track(vca, crate::document::SelectMode::Only);
    }
}

/// Makes a new group of the selected tracks (taking them out of their groups).
pub(crate) fn group_selection(doc: &mut Document) {
    let tracks = doc.selected_tracks().to_vec();
    group_tracks(doc, &tracks);
}

fn group_tracks(doc: &mut Document, tracks: &[TrackId]) {
    if tracks.is_empty() {
        return;
    }
    let n = doc.project().groups.len();
    let color = crate::timeline::TRACK_COLORS[(n * 5 + 2) % crate::timeline::TRACK_COLORS.len()];
    doc.history_mut().edit(tr("Group tracks"), |p| {
        p.groups.push(groups::TrackGroup::new(
            &trf!("Group {n}", n = n + 1),
            color,
        ));
        for &t in tracks {
            groups::set_group(p, t, Some(n));
        }
    });
}

/// Default name of the snapshot after `count` others: Mix A to Mix Z.
fn snapshot_name(count: usize) -> String {
    trf!("Mix {letter}", letter = (b'A' + (count % 26) as u8) as char)
}

/// Name of an input, or of a pair from `input` in stereo.
fn input_label(input: u16, stereo: bool) -> String {
    if stereo {
        trf!("In {first}-{second}", first = input + 1, second = input + 2)
    } else {
        trf!("In {input}", input = input + 1)
    }
}

/// The inputs of the audio stream a track can record: each one, then each pair in stereo.
pub(crate) fn input_choices(ui: &mut Ui, doc: &mut Document, track: &Track) {
    let id = track.id;
    let count = doc.stream_inputs().max(1) as u16;
    let mono = (0..count).map(|i| (i, false));
    let pairs = (0..count.saturating_sub(1)).step_by(2).map(|i| (i, true));
    let mut choices: Vec<(u16, bool)> = mono.chain(pairs).collect();
    // Keep the current choice visible even if the device has fewer inputs now.
    if !choices.contains(&(track.input, track.stereo)) {
        choices.push((track.input, track.stereo));
    }
    for (input, stereo) in choices {
        let text = input_label(input, stereo);
        let on = track.input == input && track.stereo == stereo;
        if ui.selectable_label(on, text).clicked() {
            doc.history_mut().edit(tr("Track input"), |p| {
                if let Some(t) = p.track_mut(id) {
                    t.input = input;
                    t.stereo = stereo;
                }
            });
        }
    }
}

/// What a monitoring mode does, for its button.
fn monitoring_help(mode: audiostrada_core::Monitoring) -> &'static str {
    use audiostrada_core::Monitoring;
    match mode {
        Monitoring::Auto => tr(
            "Automatic: hears its input while armed, as set in the preferences (always, or only \
             while stopped or recording)",
        ),
        Monitoring::Input => tr("Input: always hears its input, armed or not"),
        Monitoring::Off => tr("Off: never hears its input (heard through the audio interface)"),
    }
}

/// The input monitoring button of a track: a click goes to the next mode (for the selection
/// the track is part of), a right-click chooses one.
pub(crate) fn monitor_toggle(ui: &mut Ui, rect: Rect, doc: &mut Document, track: &Track) {
    use audiostrada_core::Monitoring;
    let id = track.id;
    let response = ui.interact(rect, ui.scope_id().with(("monitor", id.0)), Sense::click());
    let hearing = doc.player().monitoring(id);
    theme::paint_monitor_toggle(ui, rect, &response, track.monitor, hearing);
    let state = if hearing {
        tr("It hears its input now.")
    } else {
        tr("It does not hear its input now.")
    };
    let response = response.on_hover_text(format!(
        "{}\n{state}\n{}",
        monitoring_help(track.monitor),
        tr("Click for the next mode, right-click to choose")
    ));
    let mut tracks = selection_with(doc, id);
    if tracks.is_empty() {
        tracks.push(id);
    }
    let set = |doc: &mut Document, mode: Monitoring| {
        doc.history_mut().edit(tr("Input monitoring"), |p| {
            for &t in &tracks {
                if let Some(t) = p.track_mut(t) {
                    t.monitor = mode;
                }
            }
        });
    };
    if response.clicked() {
        set(
            doc,
            match track.monitor {
                Monitoring::Auto => Monitoring::Input,
                Monitoring::Input => Monitoring::Off,
                Monitoring::Off => Monitoring::Auto,
            },
        );
    }
    response.context_menu(|ui| {
        for mode in [Monitoring::Auto, Monitoring::Input, Monitoring::Off] {
            if ui
                .selectable_label(track.monitor == mode, monitoring_help(mode))
                .clicked()
            {
                set(doc, mode);
                ui.close();
            }
        }
    });
}

/// Group choice for one track, in its menus.
pub(crate) fn group_submenu(ui: &mut Ui, doc: &mut Document, track: TrackId) {
    let current = groups::group_of(doc.project(), track);
    // The selection when it holds this track, else this track alone.
    let tracks = if doc.selected_tracks().contains(&track) {
        doc.selected_tracks().to_vec()
    } else {
        vec![track]
    };
    ui.menu_button(tr("Group"), |ui| {
        if ui
            .button(if tracks.len() > 1 {
                tr("New group from the selected tracks")
            } else {
                tr("New group with this track")
            })
            .on_hover_text("Ctrl+G")
            .clicked()
        {
            group_tracks(doc, &tracks);
            ui.close();
        }
        ui.separator();
        if ui
            .selectable_label(current.is_none(), tr("No group"))
            .clicked()
        {
            doc.history_mut().edit(tr("Leave group"), |p| {
                for &t in &tracks {
                    groups::set_group(p, t, None);
                }
            });
            ui.close();
        }
        let names: Vec<(String, bool)> = doc
            .project()
            .groups
            .iter()
            .map(|g| (g.name.clone(), g.folder.is_some()))
            .collect();
        for (i, (name, folder)) in names.iter().enumerate() {
            if *folder {
                ui.add_enabled(false, egui::Button::selectable(current == Some(i), name))
                    .on_disabled_hover_text(tr("Its members are the tracks of its folder"));
                continue;
            }
            if ui.selectable_label(current == Some(i), name).clicked() {
                doc.history_mut().edit(tr("Join group"), |p| {
                    for &t in &tracks {
                        groups::set_group(p, t, Some(i));
                    }
                });
                ui.close();
            }
        }
    });
}

/// Mix snapshots: save the console, recall, update, rename or delete a snapshot.
fn snapshots_menu(ui: &mut Ui, doc: &mut Document) {
    use audiostrada_core::snapshot::MixSnapshot;
    ui.set_min_width(300.0);
    // The name typed for a new snapshot, and the snapshot being renamed, between frames.
    let name_id = egui::Id::unique("snapshot-name");
    let renaming_id = egui::Id::unique("snapshot-renaming");
    let count = doc.project().snapshots.len();
    let default = snapshot_name(count);
    let mut name: String = ui.data_mut(|d| d.get_temp(name_id)).unwrap_or(default);
    ui.horizontal(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(170.0));
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.button(tr("Save snapshot")).clicked() || enter) && !name.trim().is_empty() {
            let title = name.trim().to_owned();
            doc.history_mut().edit(tr("Save snapshot"), |p| {
                let snapshot = MixSnapshot::capture(p, &title);
                p.snapshots.push(snapshot);
            });
            name = snapshot_name(count + 1);
        }
    });
    ui.data_mut(|d| d.insert_temp(name_id, name));
    let snapshots = doc.project().snapshots.clone();
    if snapshots.is_empty() {
        ui.weak(tr(
            "Snapshots keep the levels, pans, mutes, sends and effect settings.",
        ));
        return;
    }
    ui.separator();
    let mut renaming: Option<(usize, String)> = ui.data_mut(|d| d.get_temp(renaming_id));
    for (i, snapshot) in snapshots.iter().enumerate() {
        ui.horizontal(|ui| {
            match &mut renaming {
                Some((at, text)) if *at == i => {
                    let edit = ui.add(egui::TextEdit::singleline(text).desired_width(170.0));
                    edit.request_focus();
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("OK").clicked() || enter {
                        let text = text.trim().to_owned();
                        if !text.is_empty() {
                            doc.history_mut().edit(tr("Rename snapshot"), |p| {
                                if let Some(s) = p.snapshots.get_mut(i) {
                                    s.name = text;
                                }
                            });
                        }
                        renaming = None;
                    }
                    return;
                }
                _ => {}
            }
            let recall = egui::Button::new(&snapshot.name).min_size(vec2(150.0, 0.0));
            if ui
                .add(recall)
                .on_hover_text(tr("Recall these settings"))
                .clicked()
            {
                doc.history_mut()
                    .edit(&trf!("Recall {name}", name = snapshot.name), |p| {
                        snapshot.recall(p)
                    });
            }
            if ui.small_button(tr("Rename")).clicked() {
                renaming = Some((i, snapshot.name.clone()));
            }
            if ui
                .small_button(tr("Update"))
                .on_hover_text(tr("Replace with the current settings"))
                .clicked()
            {
                doc.history_mut().edit(tr("Update snapshot"), |p| {
                    let fresh = MixSnapshot::capture(p, &snapshot.name);
                    if let Some(s) = p.snapshots.get_mut(i) {
                        *s = fresh;
                    }
                });
            }
            if ui.small_button("✕").on_hover_text(tr("Delete")).clicked() {
                doc.history_mut().edit(tr("Delete snapshot"), |p| {
                    if i < p.snapshots.len() {
                        p.snapshots.remove(i);
                    }
                });
            }
        });
    }
    ui.data_mut(|d| match renaming {
        Some(r) => {
            d.insert_temp(renaming_id, r);
        }
        None => d.remove::<(usize, String)>(renaming_id),
    });
}

/// A titled box of fixed height whose content scrolls.
fn section(
    ui: &mut Ui,
    title: Option<&str>,
    height: f32,
    id: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui),
) {
    let p = theme::current(ui);
    ui.add_space(2.0);
    if let Some(title) = title {
        ui.horizontal(|ui| {
            ui.set_height(13.0);
            ui.label(RichText::new(title).size(8.5).color(p.weak));
        });
    }
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(4.0)
        .inner_margin(2.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .max_height(height)
                .min_scrolled_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    add(ui);
                });
        });
}

/// True when a gesture on the control ended this frame.
fn released(response: &Response) -> bool {
    response.drag_stopped() || (response.changed() && !response.dragged())
}

/// Routes a control change through automation, falling back to a static edit; ends the
/// gesture when the control is released.
pub(crate) fn automated_control(
    doc: &mut Document,
    channel: Channel,
    target: &AutomationTarget,
    changed: Option<f32>,
    response: &Response,
    static_edit: impl FnOnce(&mut Document, f32),
) {
    if let Some(value) = changed
        && !doc.automation_gesture(channel, target, Some(value))
    {
        static_edit(doc, value);
    }
    if released(response) {
        doc.automation_gesture(channel, target, None);
        doc.history_mut().end_gesture();
    }
}

/// Right-click menu mapping a MIDI controller to the control over `min..=max`.
pub(crate) fn learn_menu(
    response: &Response,
    doc: &mut Document,
    channel: Channel,
    target: &AutomationTarget,
    (min, max): (f32, f32),
) {
    // Its own popup id: egui gives a context menu the one of a drop-down list on the same
    // widget, which would close the list as soon as it opens.
    egui::Popup::context_menu(response)
        .id(response.id.with("learn-menu"))
        .show(|ui| {
            let mapped = doc.mapping(channel, target);
            if ui.button(tr("MIDI learn")).clicked() {
                doc.learn(channel, target.clone(), min, max);
                ui.close();
            }
            if let Some((midi_channel, controller)) = mapped
                && ui
                    .button(trf!(
                        "Forget MIDI mapping (CC {controller}, ch {channel})",
                        controller = controller,
                        channel = midi_channel + 1
                    ))
                    .clicked()
            {
                doc.forget_mapping(channel, target);
                ui.close();
            }
        });
}

/// What each automation mode does, for the mode menus.
fn mode_help(mode: AutomationMode) -> &'static str {
    match mode {
        AutomationMode::Off => tr("Off: the automation lanes are ignored"),
        AutomationMode::Read => {
            tr("Read: the lanes play; moving a control overrides them while held")
        }
        AutomationMode::Touch => {
            tr("Touch: records while a control is held, then returns to the lane")
        }
        AutomationMode::Latch => {
            tr("Latch: records from the first touch and keeps the last value until stop")
        }
        AutomationMode::Write => tr("Write: records every control during the whole playback"),
        AutomationMode::Trim => {
            tr("Trim: moving a control offsets the lane instead of replacing it")
        }
    }
}

/// Small button with the automation mode of a track; its menu explains each mode.
fn mode_selector(ui: &mut Ui, doc: &mut Document, track: &Track, width: f32) {
    let id = track.id;
    let mode = track.automation_mode;
    let color = match mode {
        AutomationMode::Off => ui.visuals().weak_text_color(),
        AutomationMode::Read => OK,
        _ => ERROR,
    };
    let letter = match mode {
        AutomationMode::Off => "–",
        AutomationMode::Trim => "Tr",
        m => &m.name()[..1],
    };
    let button = egui::Button::new(RichText::new(letter).size(11.0).strong().color(color))
        .min_size(vec2(width, 18.0));
    let response = ui.add(button).on_hover_text(trf!(
        "Automation: {mode}. {help}",
        mode = mode.name(),
        help = mode_help(mode)
    ));
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(330.0);
        ui.label(RichText::new(tr("Automation mode")).strong());
        for m in AutomationMode::ALL {
            if ui.selectable_label(mode == m, mode_help(m)).clicked() && mode != m {
                let alone = clutch(ui);
                doc.history_mut().edit(tr("Automation mode"), |p| {
                    groups::maybe_alone(p, alone, |p| groups::set_automation_mode(p, id, m))
                });
                ui.close();
            }
        }
    });
}

impl MixerView {
    pub fn new(doc: Rc<RefCell<Document>>) -> Self {
        Self {
            doc,
            compact: false,
            rack: Default::default(),
            expanded: None,
            bottom_last: 0.0,
            master_levels: MeterState::default(),
            last_frame: Instant::now(),
            open_gui: Vec::new(),
            places: egui::Id::unique((
                "mixer-windows",
                NEXT_VIEW.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )),
            parked: false,
            requests: Vec::new(),
            stem_request: None,
            editors: Vec::new(),
            dragging: None,
            fx_drag: None,
            fx_viewport: egui::ViewportId::ROOT,
            fx_target: None,
            fx_fallback: None,
            raise: None,
            error: None,
            browser: Default::default(),
            plugin_params: HashMap::new(),
            param_filter: String::new(),
            fader_height: 160.0,
            editor_states: HashMap::new(),
            sizes: StripSizes::new(600.0, 230.0),
            top_height: 0.0,
            row_height: 0.0,
            top_last: 0.0,
            narrow_top: 0.0,
            narrow_last: 0.0,
            bottom_height: 0.0,
            detached: true,
            full_strips: true,
            lists: Vec::new(),
        }
    }

    /// The project goes to the background: the plugin windows it closed open again on return.
    pub fn park(&mut self, plugin_windows: Vec<(Channel, InsertId)>) {
        self.parked = true;
        for window in plugin_windows {
            if !self.open_gui.contains(&window) {
                self.open_gui.push(window);
            }
        }
    }

    /// Clears per-project state after another document was opened.
    pub fn reset(&mut self) {
        self.master_levels = MeterState::default();
        self.editors.clear();
    }

    pub fn show(&mut self, ui: &mut Ui) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;

        self.toolbar(ui);
        self.missing_media(ui);
        let doc_rc = Rc::clone(&self.doc);
        let mut doc = doc_rc.borrow_mut();
        doc.update_levels();
        let project = doc.project().clone();
        let master_width = STRIP_WIDTH + 40.0;
        // Strips fill the panel: the fader takes what the other sections leave.
        let available = ui.available_height() - 30.0;
        self.full_strips = self.detached || available >= FULL_STRIPS_HEIGHT;
        if self.full_strips && !self.compact {
            // The strips show the lists opened in windows from the buttons.
            self.lists.clear();
        }
        // Height of the strip without the fader and the lists, as measured last frame.
        let fixed = if self.top_height > 0.0 {
            (self.top_height + self.bottom_height + PLATE - self.sizes.lists()).max(120.0)
        } else {
            230.0
        };
        // Whole pixels, changed by a pixel at least: measuring last frame's layout must not make
        // this one drift or swing (the name plates would shake).
        let mut sizes = StripSizes::new(available, fixed).rounded();
        if !self.full_strips {
            sizes.fx = 0.0;
            sizes.sends = None;
        }
        if !sizes.close_to(&self.sizes) {
            self.sizes = sizes;
        }
        let fader = (available - self.top_height - self.bottom_height - PLATE - 14.0)
            .max(MIN_FADER)
            .round();
        if (fader - self.fader_height).abs() >= 1.0 {
            self.fader_height = fader;
        }
        self.top_last = self.top_height;
        self.top_height = 0.0;
        self.narrow_last = self.narrow_top;
        self.narrow_top = 0.0;
        self.bottom_last = self.bottom_height;
        self.bottom_height = 0.0;
        self.fx_target = None;
        self.fx_fallback = None;
        egui::ScrollArea::vertical()
            .id_salt("mixer-vertical")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let width = (ui.available_width() - master_width - 8.0).max(80.0);
                    ui.allocate_ui_with_layout(
                        vec2(width, ui.available_height()),
                        Layout::left_to_right(Align::Min),
                        |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("mixer-strips")
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.horizontal_top(|ui| {
                                        for (index, depth) in routing::visible_tracks(&project) {
                                            let track = &project.tracks[index];
                                            if self.compact && self.expanded != Some(track.id) {
                                                self.narrow_strip(ui, &mut doc, &project, track);
                                            } else {
                                                self.strip(ui, &mut doc, &project, track, depth);
                                            }
                                        }
                                        if project.tracks.is_empty() {
                                            ui.weak(tr(
                                        "No tracks yet. Import audio, or add a track with the \
                                         buttons above.",
                                    ));
                                        }
                                    });
                                });
                        },
                    );
                    ui.separator();
                    self.master_strip(ui, &mut doc, &project, elapsed);
                })
            });
        self.drop_insert(ui, &mut doc);
        if ui.input(|i| i.pointer.any_down()) {
            self.track_drag_ghost(ui, &project);
        } else {
            self.dragging = None;
        }
    }

    /// The mixer's windows (effect editors, lists, effect browser), drawn at every frame by the
    /// window holding the mixer, whether its tab is shown or not.
    pub fn windows(&mut self, ctx: &egui::Context) {
        let doc_rc = Rc::clone(&self.doc);
        let Ok(mut doc) = doc_rc.try_borrow_mut() else {
            return;
        };
        let project = doc.project().clone();
        self.insert_editors(ctx, &mut doc, &project);
        self.list_windows(ctx, &mut doc, &project);
        if let Some(((channel, monitor), key, name)) =
            crate::effect_browser::show(&self.browser, ctx, &doc)
        {
            let added = self.add_insert(&mut doc, channel, monitor, &key, &name);
            // Asked from the rack: where it was asked.
            if let (Some(id), Some((at, place, index))) = (added, self.rack.place.take())
                && at == channel
            {
                doc.history_mut().edit(tr("Move insert"), |p| {
                    routing::move_within(p, channel, id, (place, index));
                });
            }
        }
        drop(doc);
        if let Some(error) = self.error.clone() {
            egui::Modal::new(egui::Id::unique("routing-error")).show(ctx, |ui| {
                ui.label(&error);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let mut doc = self.doc.borrow_mut();
        ui.horizontal_wrapped(|ui| {
            ui.menu_button(tr("Add"), |ui| {
                use crate::actions::Action;
                for (action, help) in [
                    (
                        Action::AddTrack,
                        tr("A mono audio track, below the selected one"),
                    ),
                    (
                        Action::AddStereoTrack,
                        tr("A stereo audio track, below the selected one"),
                    ),
                    (
                        Action::AddTracks,
                        tr("Several audio tracks at once, with their name and color"),
                    ),
                    (
                        Action::AddInstrumentTrack,
                        tr("A MIDI track with the built-in synth, ready to play"),
                    ),
                    (
                        Action::AddDrumTrack,
                        tr("A MIDI track with the built-in drum machine, ready to play"),
                    ),
                    (
                        Action::AddVca,
                        tr(
                            "A fader without audio that moves the level and mute of the selected \
                             tracks together, keeping their balance",
                        ),
                    ),
                ] {
                    let mut button = egui::Button::new(action.label());
                    if let Some(keys) = crate::shortcut_labels(action).first() {
                        button = button.shortcut_text(keys.as_str());
                    }
                    if ui.add(button).on_hover_text(help).clicked() {
                        self.requests.push(action);
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(tr("Audio, instrument or drum machine tracks, and VCAs"));
            let label = if doc.project().groups_off {
                tr("Groups (off)")
            } else {
                tr("Groups")
            };
            ui.menu_button(label, |ui| groups_menu(ui, &mut doc))
                .response
                .on_hover_text(tr(
                    "Tracks whose level, mute, solo, arm, editing and more move together",
                ));
            ui.menu_button(tr("Snapshots"), |ui| snapshots_menu(ui, &mut doc))
                .response
                .on_hover_text(tr("Save and recall the console settings"));
            let importing = doc.importing();
            if importing > 0 {
                ui.spinner();
                ui.weak(trf!("importing {count} file(s)…", count = importing));
            }
            // At the right: the compact switch, then the console.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let console = doc.project().console.model;
                let mut label = RichText::new(crate::console_ui::button_label(&doc));
                if console != audiostrada_core::console::ConsoleModel::Clean {
                    label = label.color(crate::console_ui::color(console));
                }
                if doc.project().console_off {
                    label = label.strikethrough();
                }
                // Right to left: the button, then its switch at its left.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let button = ui.button(label).on_hover_text(tr(
                        "The character of an analog console on every track, the buses and the master",
                    ));
                    // Stays open while its settings change.
                    egui::Popup::menu(&button)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| crate::console_ui::console_menu(ui, &mut doc));
                    crate::console_ui::power_switch(ui, &mut doc);
                });
                let keys = crate::shortcut_labels(crate::actions::Action::CompactMixer)
                    .first()
                    .cloned()
                    .unwrap_or_default();
                if ui
                    .selectable_label(self.compact, tr("Compact"))
                    .on_hover_text(trf!(
                        "Narrow strips: a click opens one full width ({keys})",
                        keys = keys
                    ))
                    .clicked()
                {
                    self.requests.push(crate::actions::Action::CompactMixer);
                }
            });
        });
        ui.separator();
    }

    fn missing_media(&mut self, ui: &mut Ui) {
        let missing = self.doc.borrow().missing();
        if missing.is_empty() {
            return;
        }
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.colored_label(
                WARN,
                RichText::new(trf!(
                    "{count} media file(s) cannot be found",
                    count = missing.len()
                ))
                .strong(),
            );
            for (id, path, reason) in missing {
                ui.horizontal(|ui| {
                    ui.label(path.display().to_string()).on_hover_text(&reason);
                    let open = ui.button(tr("Locate…")).clicked();
                    if let Some(file) =
                        dialogs::ask(ui.ctx(), ("locate", id), open, dialogs::pick_audio_file)
                    {
                        self.doc.borrow_mut().relink(id, &file);
                    }
                });
            }
        });
        ui.add_space(6.0);
    }

    fn strip_frame(
        ui: &mut Ui,
        width: f32,
        accent: Color32,
        selected: bool,
        add: impl FnOnce(&mut Ui),
    ) -> Response {
        let p = theme::current(ui);
        let fill = match (ui.visuals().dark_mode, selected) {
            (true, false) => theme::mix(p.panel, p.surface, 0.3),
            (true, true) => theme::mix(p.surface, p.accent, 0.12),
            (false, false) => theme::mix(p.panel, p.surface, 0.45),
            (false, true) => theme::mix(p.surface, p.accent, 0.1),
        };
        let stroke = if selected {
            egui::Stroke::new(1.5, p.accent)
        } else {
            egui::Stroke::new(1.0, p.border)
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(6.0)
            .inner_margin(egui::Margin {
                left: 4,
                right: 4,
                top: 0,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.set_width(width);
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.set_width(width);
                    let (bar, _) = ui.allocate_exact_size(vec2(width + 8.0, 4.0), Sense::hover());
                    ui.painter().rect_filled(
                        bar,
                        egui::CornerRadius {
                            nw: 6,
                            ne: 6,
                            sw: 0,
                            se: 0,
                        },
                        accent,
                    );
                    ui.add_space(2.0);
                    add(ui);
                });
            })
            .response
    }

    /// Pads to the tallest top part of the last frame and remembers this one.
    fn align_top(&mut self, ui: &mut Ui, top: f32) {
        let used = ui.cursor().top() - top;
        self.top_height = self.top_height.max(used);
        let pad = (self.top_height.max(self.top_last) - used).max(0.0);
        // add_space is ignored in this centered layout; the allocation adds the item spacing.
        let pad = pad - ui.spacing().item_spacing.y;
        if pad > 0.0 {
            ui.allocate_exact_size(vec2(1.0, pad), Sense::hover());
        }
    }

    /// [`Self::align_top`] for a narrow strip: with the other narrow ones only.
    fn align_narrow_top(&mut self, ui: &mut Ui, top: f32) {
        let used = ui.cursor().top() - top;
        self.top_height = self.top_height.max(used);
        self.narrow_top = self.narrow_top.max(used);
        let pad = (self.narrow_top.max(self.narrow_last) - used).max(0.0);
        let pad = pad - ui.spacing().item_spacing.y;
        if pad > 0.0 {
            ui.allocate_exact_size(vec2(1.0, pad), Sense::hover());
        }
    }

    fn strip(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        track: &Track,
        depth: usize,
    ) {
        let id = track.id;
        let width = STRIP_WIDTH;
        let vca = track.kind == TrackKind::Vca;
        let mut levels = doc.track_level(id);
        let fader_height = self.fader_height;
        let sizes = self.sizes;
        let selected = doc.is_track_selected(id);
        // Clicks on the strip's background select the track, drags move it; its controls come
        // first.
        let background = egui::UiBuilder::new().sense(Sense::click_and_drag());
        let strip = ui.scope_builder(background, |ui| {
            Self::strip_frame(ui, width, color(track), selected, |ui| {
                let folder = routing::has_children(project, id);
                let top = ui.cursor().top();
                let channel = Channel::Track(id);
                if vca {
                    ui.add_space(4.0);
                    let members = project.tracks.iter().filter(|t| t.vca == Some(id)).count();
                    ui.label(RichText::new("VCA").strong().size(11.0));
                    ui.label(
                    RichText::new(trf!(
                        "Moves the level and mute of {members} track(s), keeping their balance.",
                        members = members
                    ))
                    .size(9.5)
                    .weak(),
                );
                    let selected: Vec<TrackId> = doc
                        .selected_tracks()
                        .iter()
                        .copied()
                        .filter(|&t| t != id)
                        .collect();
                    if ui
                        .add_enabled(
                            !selected.is_empty(),
                            egui::Button::new(tr("Assign selected")).small(),
                        )
                        .on_hover_text(tr("Make this VCA control the selected tracks"))
                        .clicked()
                    {
                        doc.history_mut().edit(tr("Assign VCA"), |p| {
                            for &t in &selected {
                                let _ = routing::set_vca(p, t, Some(id));
                            }
                        });
                    }
                } else {
                    if self.full_strips {
                        ui.add_space(3.0);
                        self.channel_section(ui, doc, channel, width);
                        self.inserts_title(ui, doc, channel, tr("INSERTS"), true);
                        section(ui, None, sizes.fx, ("inserts", id.0), |ui| {
                            self.inserts_section(
                                ui,
                                doc,
                                channel,
                                &track.inserts,
                                &track.name,
                                false,
                                Some(sizes.fx),
                            );
                        });
                        if let Some(height) = sizes.sends {
                            section(ui, Some(tr("SENDS")), height, ("sends", id.0), |ui| {
                                self.sends_section(ui, doc, project, track, Some(height));
                            });
                        }
                    }
                    if sizes.input {
                        ui.add_space(2.0);
                        self.input_section(ui, doc, track);
                    }
                    ui.add_space(2.0);
                    self.routing_section(ui, doc, project, track, width - 4.0);
                    self.pan_control(ui, doc, track, width - 4.0, true);
                }
                self.align_top(ui, top);
                let shown_gain = doc
                    .automation_value(channel, &AutomationTarget::Gain)
                    .unwrap_or(track.gain_db);
                let row_top = ui.cursor().top();
                let strip_right = ui.max_rect().right();
                ui.horizontal(|ui| {
                    self.gain_fader(ui, doc, track, shown_gain, fader_height, true);
                    // The switches stand in a column right of the meter.
                    if !vca {
                        widgets::meter(ui, &mut levels, fader_height);
                    }
                    if self.full_strips || vca {
                        // Centered in the room left of the strip.
                        let free = strip_right - ui.cursor().left() - 24.0;
                        ui.add_space((free / 2.0 - ui.spacing().item_spacing.x).max(0.0));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            self.track_switches(ui, doc, track, vec2(24.0, 20.0));
                        });
                    } else {
                        ui.vertical(|ui| {
                            let top = ui.cursor().top();
                            // Two by two at the bottom of the column.
                            let rows = 2.0 * 21.0;
                            self.list_buttons(ui, doc, track, fader_height - rows - 6.0);
                            let left = fader_height - (ui.cursor().top() - top) - rows;
                            if left > 0.0 {
                                ui.add_space(left);
                            }
                            ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.set_max_width(51.0);
                                self.track_switches(ui, doc, track, vec2(24.0, 18.0));
                            });
                        });
                    }
                });
                let bottom = ui.cursor().top();
                self.row_height = bottom - row_top;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let group = groups::group_of(project, id).map(|g| &project.groups[g]);
                    let others =
                        if vca { 0.0 } else { 27.0 } + if group.is_some() { 19.0 } else { 0.0 };
                    readout(ui, width - 4.0 - others, &db_label(shown_gain));
                    if !vca {
                        mode_selector(ui, doc, track, 24.0);
                    }
                    if let Some(g) = group {
                        let c = Color32::from_rgb(g.color[0], g.color[1], g.color[2]);
                        let c = if g.active { c } else { c.gamma_multiply(0.4) };
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(16.0, 18.0), Sense::click());
                        ui.painter().rect_filled(rect, 3.0, c);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            g.name.chars().next().unwrap_or('G').to_string(),
                            egui::FontId::proportional(10.0),
                            Color32::BLACK,
                        );
                        if response
                            .on_hover_text(trf!(
                                "Group {name}. Click to select its tracks",
                                name = g.name
                            ))
                            .clicked()
                        {
                            doc.set_track_selection(groups::members(project, g));
                        }
                    }
                });
                let name = format!("{}{}", "› ".repeat(depth), track.name);
                // Height down to the name plate, which the master strip lines up with.
                self.bottom_height = self.bottom_height.max(ui.cursor().top() - bottom);
                self.pad_bottom(ui, bottom);
                let icon = crate::track_icons::of(project, track);
                let plate = name_plate(ui, width, &name, color(track), folder, Some(icon));
                let plate = plate.on_hover_text(if vca {
                    tr("VCA. Click to select, drag to reorder, right-click for options")
                } else if folder {
                    tr("Folder. Click to select, drag to reorder, right-click for options")
                } else {
                    tr(
                        "Click to select (Ctrl adds, Shift extends), drag to reorder, right-click \
                     for options",
                    )
                });
                if plate.clicked() {
                    let mode =
                        crate::document::SelectMode::from_modifiers(ui.input(|i| i.modifiers));
                    doc.select_track(id, mode);
                }
                #[cfg(test)]
                tests::seen(format!("plate {}", id.0), plate.rect);
                if plate.drag_started() {
                    self.start_track_drag(doc, id, selected);
                }
                plate.context_menu(|ui| self.track_menu(ui, doc, id));
            })
        });
        let frame = strip.inner;
        if track.kind == TrackKind::Audio {
            let count = track.inserts.len();
            self.insert_drop_strip(ui, Channel::Track(id), count, strip.response.rect);
        }
        #[cfg(test)]
        tests::seen(format!("strip {}", id.0), strip.response.rect);
        if strip.response.clicked() {
            let mode = crate::document::SelectMode::from_modifiers(ui.input(|i| i.modifiers));
            doc.select_track(id, mode);
            // Compact: a click folds the opened strip back.
            if self.compact {
                self.expanded = None;
            }
        }
        if strip.response.drag_started() {
            self.start_track_drag(doc, id, selected);
        }
        strip
            .response
            .context_menu(|ui| self.track_menu(ui, doc, id));
        self.drop_tracks(ui, doc, &frame, id);
        doc.set_track_level(id, levels);
    }

    /// A narrow strip of the compact mixer: pan, fader, meter, switches and the track's icon,
    /// its name upright above the fader. A click opens it full width.
    fn narrow_strip(&mut self, ui: &mut Ui, doc: &mut Document, project: &Project, track: &Track) {
        let id = track.id;
        let width = NARROW_WIDTH;
        let vca = track.kind == TrackKind::Vca;
        let mut levels = doc.track_level(id);
        // Its top is shorter than the open strip's: the fader takes the difference.
        let taller = (self.top_last - self.narrow_last).max(0.0).round();
        let fader_height = self.fader_height + taller;
        let selected = doc.is_track_selected(id);
        let icon = crate::track_icons::of(project, track);
        let folder = routing::has_children(project, id);
        let mut plate = None;
        let background = egui::UiBuilder::new().sense(Sense::click_and_drag());
        let strip = ui.scope_builder(background, |ui| {
            Self::strip_frame(ui, width, color(track), selected, |ui| {
                let top = ui.cursor().top();
                if !vca {
                    ui.add_space(2.0);
                    self.pan_control(ui, doc, track, width, false);
                }
                // The room above the fader shows the name, upright.
                let before = ui.cursor().top();
                ui.allocate_exact_size(vec2(width, NAME_ROOM), Sense::hover());
                self.align_narrow_top(ui, top);
                let room = Rect::from_min_max(
                    pos2(ui.max_rect().left(), before),
                    pos2(ui.max_rect().right(), ui.cursor().top()),
                );
                if room.height() > 30.0 {
                    upright_name(ui, room, &track.name);
                }
                #[cfg(test)]
                tests::seen(format!("room {}", id.0), room);
                let shown_gain = doc
                    .automation_value(Channel::Track(id), &AutomationTarget::Gain)
                    .unwrap_or(track.gain_db);
                let row_top = ui.cursor().top();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    self.gain_fader(ui, doc, track, shown_gain, fader_height, false);
                    if !vca {
                        widgets::meter(ui, &mut levels, fader_height);
                    }
                });
                let bottom = ui.cursor().top();
                if taller == 0.0 {
                    self.row_height = bottom - row_top;
                }
                readout(ui, width, &db_label(shown_gain));
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(width);
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    self.track_switches(ui, doc, track, vec2(22.0, 18.0));
                });
                self.bottom_height = self.bottom_height.max(ui.cursor().top() - bottom);
                self.pad_bottom(ui, bottom);
                plate = Some(name_plate(ui, width, "", color(track), folder, Some(icon)));
            })
        });
        let frame = strip.inner;
        if track.kind == TrackKind::Audio {
            let count = track.inserts.len();
            self.insert_drop_strip(ui, Channel::Track(id), count, strip.response.rect);
        }
        #[cfg(test)]
        tests::seen(format!("narrow {}", id.0), strip.response.rect);
        let hint = trf!("{name}. Click to open the strip", name = track.name);
        let response = strip.response.on_hover_text(&hint);
        let plate = plate.map(|p| p.on_hover_text(&hint));
        let clicked = response.clicked() || plate.as_ref().is_some_and(Response::clicked);
        if clicked {
            let mode = crate::document::SelectMode::from_modifiers(ui.input(|i| i.modifiers));
            doc.select_track(id, mode);
            self.expanded = Some(id);
        }
        let dragged = response.drag_started() || plate.as_ref().is_some_and(Response::drag_started);
        if dragged {
            self.start_track_drag(doc, id, selected);
        }
        response.context_menu(|ui| self.track_menu(ui, doc, id));
        if let Some(plate) = &plate {
            plate.context_menu(|ui| self.track_menu(ui, doc, id));
        }
        self.drop_tracks(ui, doc, &frame, id);
        doc.set_track_level(id, levels);
    }

    /// Pads to the tallest part under the fader rows of the last frame, so the name plates
    /// line up.
    fn pad_bottom(&mut self, ui: &mut Ui, bottom: f32) {
        let used = ui.cursor().top() - bottom;
        let pad = self.bottom_last - used - ui.spacing().item_spacing.y;
        if pad > 0.5 {
            // add_space is ignored in this centered layout.
            ui.allocate_exact_size(vec2(1.0, pad), Sense::hover());
        }
    }

    /// A track's pan, automated, grouped and MIDI-learnable.
    fn pan_control(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        width: f32,
        labelled: bool,
    ) {
        let id = track.id;
        let channel = Channel::Track(id);
        let shown = doc
            .automation_value(channel, &AutomationTarget::Pan)
            .unwrap_or(track.pan);
        let label = if labelled {
            pan_label(shown)
        } else {
            String::new()
        };
        let (changed, response) = widgets::pan_slider(ui, shown, width, &label);
        let pan = changed.unwrap_or(shown);
        let response = response.on_hover_text(trf!(
            "Pan {pan}. Drag (Shift for fine steps) or use the wheel; double-click \
             to center",
            pan = pan_label(pan)
        ));
        let name = track.name.clone();
        let alone = clutch(ui);
        automated_control(
            doc,
            channel,
            &AutomationTarget::Pan,
            (pan != shown).then_some(pan),
            &response,
            |doc, pan| {
                doc.history_mut().edit_merged(
                    &trf!("Pan {name}", name = name),
                    &format!("pan:{}", id.0),
                    |p| groups::maybe_alone(p, alone, |p| groups::set_pan(p, id, pan)),
                );
            },
        );
        learn_menu(&response, doc, channel, &AutomationTarget::Pan, (-1.0, 1.0));
    }

    /// A track's fader, automated, grouped and MIDI-learnable.
    fn gain_fader(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: &Track,
        shown_gain: f32,
        height: f32,
        scale: bool,
    ) {
        let id = track.id;
        let channel = Channel::Track(id);
        let (changed, response) = widgets::fader(ui, shown_gain, height, scale);
        let name = track.name.clone();
        let alone = clutch(ui);
        let also = if alone {
            Vec::new()
        } else {
            selection_with(doc, id)
        };
        automated_control(
            doc,
            channel,
            &AutomationTarget::Gain,
            changed,
            &response,
            |doc, db| {
                doc.history_mut().edit_merged(
                    &trf!("Gain {name}", name = name),
                    &format!("gain:{}", id.0),
                    |p| groups::maybe_alone(p, alone, |p| groups::set_gain_with(p, id, db, &also)),
                );
            },
        );
        learn_menu(
            &response,
            doc,
            channel,
            &AutomationTarget::Gain,
            (FADER_MIN_DB, FADER_MAX_DB),
        );
    }

    /// Mute, solo, and for audio tracks arm and input monitoring, in the layout of `ui`.
    fn track_switches(&mut self, ui: &mut Ui, doc: &mut Document, track: &Track, size: egui::Vec2) {
        let id = track.id;
        let channel = Channel::Track(id);
        let vca = track.kind == TrackKind::Vca;
        let monitored = !vca && !doc.is_instrument_track(id);
        let toggle = |ui: &mut Ui, text: &str, on: bool, color: Color32, hint: &str| {
            theme::letter_toggle(ui, size, text, on, color)
                .on_hover_text(hint)
                .clicked()
        };
        let auto_mute = doc.automation_value(channel, &AutomationTarget::Mute);
        let shown_mute = track.mute || auto_mute.is_some_and(|v| v >= 0.5);
        if toggle(ui, "M", shown_mute, theme::MUTE, tr("Mute")) {
            let mute = !shown_mute;
            if doc.automation_gesture(
                channel,
                &AutomationTarget::Mute,
                Some(f32::from(u8::from(mute))),
            ) {
                // Switches keep writing until playback stops; in Read they only flip once.
                if !(doc.automation_mode(channel).writes() && doc.player().engine().playing()) {
                    doc.automation_gesture(channel, &AutomationTarget::Mute, None);
                }
            } else {
                let alone = clutch(ui);
                doc.history_mut()
                    .edit(&trf!("Mute {name}", name = track.name), |p| {
                        groups::maybe_alone(p, alone, |p| {
                            groups::set_switch(p, id, Link::Mute, mute)
                        })
                    });
            }
        }
        if toggle(ui, "S", track.solo, theme::SOLO, tr("Solo")) {
            let solo = !track.solo;
            let alone = clutch(ui);
            doc.history_mut()
                .edit(&trf!("Solo {name}", name = track.name), |p| {
                    groups::maybe_alone(p, alone, |p| groups::set_switch(p, id, Link::Solo, solo))
                });
        }
        if !vca
            && theme::arm_toggle(ui, size, track.armed)
                .on_hover_text(tr("Record arm"))
                .clicked()
        {
            let armed = !track.armed;
            let alone = clutch(ui);
            doc.history_mut().modify_unrecorded(|p| {
                groups::maybe_alone(p, alone, |p| groups::set_switch(p, id, Link::Arm, armed))
            });
        }
        if monitored {
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            monitor_toggle(ui, rect, doc, track);
        }
    }

    /// A track's right-click menu; its commands go to the application.
    fn track_menu(&mut self, ui: &mut Ui, doc: &mut Document, id: TrackId) {
        for request in crate::track_menu::show(ui, doc, id) {
            match request {
                crate::track_menu::Request::Action(a) => self.requests.push(a),
                crate::track_menu::Request::Export(t) => self.stem_request = Some(t),
            }
        }
    }

    /// Removes an insert and closes its editor.
    fn remove_insert(&mut self, doc: &mut Document, channel: Channel, id: InsertId) {
        doc.history_mut().edit(tr("Remove insert"), |p| {
            routing::remove_insert(p, channel, id)
        });
        self.editors.retain(|e| *e != (channel, id));
    }

    /// Drags a track, with the others selected along with it.
    fn start_track_drag(&mut self, doc: &Document, id: TrackId, selected: bool) {
        let dragged = if selected {
            doc.selected_tracks().to_vec()
        } else {
            vec![id]
        };
        // Our own drag state: egui's drag and drop is cleared by the other windows' frames when
        // the mixer is detached.
        self.dragging = Some(dragged);
    }

    /// The names of the dragged tracks under the pointer.
    fn track_drag_ghost(&self, ui: &Ui, project: &Project) {
        let Some(dragged) = &self.dragging else {
            return;
        };
        let Some(pos) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let names: Vec<&str> = dragged
            .iter()
            .filter_map(|id| project.track(*id))
            .map(|t| t.name.as_str())
            .collect();
        let text = match names.as_slice() {
            [] => return,
            [one] => (*one).to_owned(),
            [first, rest @ ..] => trf!("{name} and {n} more", name = first, n = rest.len()),
        };
        let p = theme::current(ui);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("track-drag"),
        ));
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), p.text);
        let rect = Rect::from_min_size(pos + vec2(14.0, 6.0), galley.size() + vec2(10.0, 4.0));
        painter.rect_filled(rect, 3.0, p.surface);
        painter.rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + vec2(5.0, 2.0), galley, p.text);
        ui.ctx().request_repaint();
    }

    /// Tracks dropped on a strip move before or after it, from the side of the pointer.
    fn drop_tracks(&mut self, ui: &mut Ui, doc: &mut Document, frame: &Response, target: TrackId) {
        let Some(dragged) = &self.dragging else {
            return;
        };
        let Some(pointer) = ui.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        if !frame.rect.contains(pointer) {
            return;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let after = pointer.x > frame.rect.center().x;
        let x = if after {
            frame.rect.right() + 2.0
        } else {
            frame.rect.left() - 2.0
        };
        ui.painter().line_segment(
            [pos2(x, frame.rect.top()), pos2(x, frame.rect.bottom())],
            egui::Stroke::new(3.0, theme::current(ui).accent),
        );
        if ui.input(|i| i.pointer.any_released()) && !dragged.contains(&target) {
            let dragged = dragged.clone();
            doc.history_mut().edit(tr("Move tracks"), |p| {
                routing::move_tracks(p, &dragged, target, after)
            });
            self.dragging = None;
        }
    }

    fn input_section(&mut self, ui: &mut Ui, doc: &mut Document, track: &Track) {
        let id = track.id;
        if doc.is_instrument_track(id) {
            // An instrument plays MIDI: no audio input to choose.
            ui.add_sized(
                [STRIP_WIDTH - 8.0, 20.0],
                egui::Label::new(RichText::new(tr("MIDI")).weak()),
            )
            .on_hover_text(tr(
                "Plays the MIDI it receives while armed: keyboards and controllers, its \
                 instrument's own keyboard or pads",
            ));
        } else {
            let label = input_label(track.input, track.stereo);
            egui::ComboBox::from_id_salt(("mixer-input", id.0))
                .width(STRIP_WIDTH - 16.0)
                .selected_text(label)
                .show_ui(ui, |ui| input_choices(ui, doc, track));
        }
        ui.horizontal(|ui| {
            let mut trim = track.trim_db;
            let response = ui
                .add(
                    egui::DragValue::new(&mut trim)
                        .range(-24.0..=24.0)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .suffix(" dB"),
                )
                .on_hover_text(tr("Input trim"));
            if response.changed() {
                doc.history_mut()
                    .edit_merged(tr("Trim"), &format!("trim:{}", id.0), |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.trim_db = trim;
                        }
                    });
            }
            end_gesture(doc.history_mut(), &response);
            let mut invert = track.invert;
            if ui
                .toggle_value(&mut invert, "Ø")
                .on_hover_text(tr("Invert polarity"))
                .changed()
            {
                let alone = clutch(ui);
                doc.history_mut().edit(tr("Polarity"), |p| {
                    groups::maybe_alone(p, alone, |p| {
                        groups::set_switch(p, id, Link::Polarity, invert)
                    })
                });
            }
        });
    }

    /// The buttons bypassing a channel's inserts (and every insert, on the master).
    fn fx_toggles(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel, small: bool) {
        ui.spacing_mut().item_spacing.x = 2.0;
        let project = doc.project();
        let all_off = project.all_fx_off;
        let channel_off = match channel {
            Channel::Track(t) => project.track(t).is_some_and(|t| t.fx_off),
            Channel::Master => project.master.fx_off,
        };
        // Lit while the effects play, dark once bypassed.
        let accent = theme::current(ui).accent;
        let toggle = |ui: &mut Ui, label: &str, off: bool, hint: &str| {
            let text = RichText::new(label).size(if small { 8.5 } else { 10.0 });
            let mut button = if off {
                egui::Button::new(text.weak().strikethrough())
            } else {
                egui::Button::new(text.strong().color(Color32::WHITE))
                    .fill(accent.gamma_multiply(0.85))
            };
            if small {
                button = button.min_size(vec2(22.0, 13.0)).corner_radius(3.0);
            }
            let padding = ui.spacing().button_padding;
            if small {
                ui.spacing_mut().button_padding = vec2(4.0, 0.0);
            }
            let clicked = ui.add(button).on_hover_text(hint).clicked();
            ui.spacing_mut().button_padding = padding;
            clicked
        };
        let master = channel == Channel::Master;
        let hint = match (channel_off, all_off, master) {
            (false, false, true) => tr("Effects on: click to bypass the master's inserts"),
            (false, false, false) => tr("Effects on: click to bypass every insert of this track"),
            (true, _, true) => tr("Effects bypassed: click to turn the master's inserts back on"),
            (true, _, false) => {
                tr("Effects bypassed: click to turn every insert of this track back on")
            }
            (false, true, _) => tr("Every effect is bypassed by ALL on the master"),
        };
        if toggle(ui, tr("FX"), channel_off || all_off, hint) {
            let alone = clutch(ui);
            doc.history_mut()
                .edit(tr("Bypass inserts"), |p| match channel {
                    Channel::Track(t) => groups::maybe_alone(p, alone, |p| {
                        groups::set_switch(p, t, Link::Fx, !channel_off)
                    }),
                    Channel::Master => p.master.fx_off = !p.master.fx_off,
                });
        }
        if channel == Channel::Master
            && toggle(
                ui,
                tr("ALL"),
                all_off,
                if all_off {
                    tr("Every effect bypassed: click to turn them back on")
                } else {
                    tr(
                        "Effects on: click to bypass every insert of every track and of the \
                        master (an instrument leading a track keeps playing)",
                    )
                },
            )
        {
            doc.history_mut()
                .edit(tr("Bypass all inserts"), |p| p.all_fx_off = !p.all_fx_off);
        }
    }

    /// The title of a tall strip's insert list, with its bypass buttons.
    fn inserts_title(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        title: &str,
        toggles: bool,
    ) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.set_height(13.0);
            ui.label(
                RichText::new(title)
                    .size(8.5)
                    .color(theme::current(ui).weak),
            );
            if toggles {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(2.0);
                    self.fx_toggles(ui, doc, channel, true);
                    let padding = ui.spacing().button_padding;
                    ui.spacing_mut().button_padding = vec2(4.0, 0.0);
                    let rack = ui
                        .add(
                            egui::Button::new(RichText::new(tr("Rack")).size(8.5))
                                .min_size(vec2(22.0, 13.0))
                                .corner_radius(3.0),
                        )
                        .on_hover_text(tr("Open this chain in the rack"));
                    ui.spacing_mut().button_padding = padding;
                    if rack.clicked() {
                        self.reveal_rack(doc, channel);
                    }
                });
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn inserts_section(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        inserts: &[audiostrada_core::Insert],
        owner: &str,
        monitor: bool,
        height: Option<f32>,
    ) {
        let start = ui.cursor().top();
        // Without a height, the list has a window of its own: wider rows with their buttons.
        let wide = height.is_none();
        let project = doc.project();
        let all_off = project.all_fx_off;
        let channel_off = match channel {
            Channel::Track(t) => project.track(t).is_some_and(|t| t.fx_off),
            Channel::Master => project.master.fx_off,
        };
        let off = all_off || (channel_off && !monitor);
        // The channel strip has its own section.
        let shown: Vec<&audiostrada_core::Insert> = inserts
            .iter()
            .filter(|i| i.monitor == monitor && i.effect != routing::STRIP)
            .collect();
        if wide && !monitor {
            ui.add_space(4.0);
            self.channel_section(ui, doc, channel, ui.available_width());
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr("INSERTS"))
                    .size(8.5)
                    .color(theme::current(ui).weak),
            );
        }
        for (row, insert) in shown.iter().enumerate() {
            let id = insert.id;
            let name = effect_name(doc, &insert.effect);
            let error = doc.player().plugin_error(id).map(str::to_owned);
            let forced = doc.player().low_latency_bypassed().contains(&id);
            let hint = match &error {
                None if forced => trf!(
                    "{owner}: {name}, bypassed by the low latency mode (it adds more than 1 ms)",
                    owner = owner,
                    name = name
                ),
                Some(e) => trf!(
                    "{owner}: {name} cannot be loaded: {e}",
                    owner = owner,
                    name = name,
                    e = e
                ),
                None => trf!(
                    "{owner}: {name}. Click to edit, Shift+click to bypass, Ctrl+click to \
                     remove; drag to reorder, onto another track to copy (Alt: move)",
                    owner = owner,
                    name = name
                ),
            };
            let plugin = audiostrada_dsp::info(&insert.effect).is_none();
            let lit = !(insert.bypass || off || forced);
            let color = theme::family_color(family(doc, &insert.effect));
            let latency = latency_label(doc, channel, id);
            let power_hint = if insert.bypass {
                tr("Bypassed: click to enable")
            } else {
                tr("Playing: click to bypass")
            };
            let look = Slot {
                name: &name,
                color,
                lit,
                error: error.is_some(),
                latency: latency.as_deref(),
            };
            let (response, row_rect, toggled, removed) = if wide {
                let row = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.add_sized(
                        [14.0, 24.0],
                        egui::Label::new(RichText::new(format!("{}", row + 1)).size(10.0).weak()),
                    );
                    let size = vec2(ui.available_width() - 22.0, 24.0);
                    let (name, power) = insert_slot(ui, size, id.0, &look);
                    let name = name.on_hover_text(hint);
                    let toggled = power.on_hover_text(power_hint).clicked();
                    let removed = remove_button(ui).clicked();
                    (name, toggled, removed)
                });
                let (name, toggled, removed) = row.inner;
                (name, row.response.rect, toggled, removed)
            } else {
                let (name, power) = insert_slot(ui, vec2(STRIP_WIDTH - 12.0, 18.0), id.0, &look);
                let name = name.on_hover_text(hint);
                let toggled = power.on_hover_text(power_hint).clicked();
                let rect = name.rect.union(Rect::from_min_size(
                    name.rect.min - vec2(18.0, 0.0),
                    vec2(18.0, 18.0),
                ));
                (name, rect, toggled, false)
            };
            if response.drag_started() {
                self.fx_drag = Some((channel, id, name.clone()));
                self.fx_viewport = ui.ctx().viewport_id();
            }
            #[cfg(test)]
            tests::seen(format!("insert {}", id.0), response.rect);
            self.insert_drop_row(ui, (channel, monitor, row), row_rect, true);
            let modifiers = ui.input(|i| i.modifiers);
            if removed {
                self.remove_insert(doc, channel, id);
                continue;
            }
            if toggled || (response.clicked() && modifiers.shift) {
                let bypass = !insert.bypass;
                doc.history_mut().edit(tr("Bypass insert"), |p| {
                    routing::set_bypass(p, channel, id, bypass)
                });
            } else if response.clicked() && modifiers.command {
                self.remove_insert(doc, channel, id);
            } else if response.clicked() && insert.effect == routing::PARALLEL {
                self.reveal_rack(doc, channel);
            } else if response.clicked() && plugin {
                // A plugin shows its own window; AudioStrada's is in the right-click menu.
                if !self.open_gui.contains(&(channel, id)) {
                    self.open_gui.push((channel, id));
                }
            } else if response.clicked() {
                self.show_editor(channel, id);
            }
            let instrument = row == 0
                && !monitor
                && matches!(channel, Channel::Track(_))
                && is_instrument(doc, &insert.effect);
            response.context_menu(|ui| {
                if instrument {
                    if ui
                        .button(tr("Change instrument…"))
                        .on_hover_text(tr(
                            "The built-in synth or an installed instrument plugin (CLAP, VST3, \
                             LV2) in its place",
                        ))
                        .clicked()
                    {
                        self.rack.place = None;
                        self.browser.borrow_mut().open_instruments((channel, false));
                        ui.close();
                    }
                    ui.separator();
                }
                if plugin {
                    if ui.button(tr("Plugin window")).clicked() {
                        self.open_gui.push((channel, id));
                        ui.close();
                    }
                    if ui
                        .button(tr("Presets, levels and parameters…"))
                        .on_hover_text(tr(
                            "AudioStrada's window for this plugin: presets, A/B, input and output \
                             levels, Match, every parameter",
                        ))
                        .clicked()
                    {
                        self.show_editor(channel, id);
                        ui.close();
                    }
                    ui.separator();
                }
                if ui
                    .button(if insert.bypass {
                        tr("Enable")
                    } else {
                        tr("Bypass")
                    })
                    .clicked()
                {
                    let bypass = !insert.bypass;
                    doc.history_mut().edit(tr("Bypass insert"), |p| {
                        routing::set_bypass(p, channel, id, bypass)
                    });
                    ui.close();
                }
                if ui.button(tr("Move up")).clicked() {
                    doc.history_mut().edit(tr("Move insert"), |p| {
                        routing::move_insert(p, channel, id, true)
                    });
                    ui.close();
                }
                if ui.button(tr("Move down")).clicked() {
                    doc.history_mut().edit(tr("Move insert"), |p| {
                        routing::move_insert(p, channel, id, false)
                    });
                    ui.close();
                }
                if channel == Channel::Master {
                    let label = if insert.monitor {
                        tr("Move to the mix (exported)")
                    } else {
                        tr("Move to monitoring only")
                    };
                    if ui.button(label).clicked() {
                        let to_monitor = !insert.monitor;
                        doc.history_mut().edit(tr("Move insert"), |p| {
                            if let Some(i) = p.master.inserts.iter_mut().find(|i| i.id == id) {
                                i.monitor = to_monitor;
                            }
                        });
                        ui.close();
                    }
                }
                if error.is_some() && ui.button(tr("Reload plugin")).clicked() {
                    doc.player_mut().reload_plugin(id);
                    ui.close();
                }
                save_chain_menu(ui, doc, (channel, monitor), owner);
                if ui.button(tr("Remove")).clicked() {
                    self.remove_insert(doc, channel, id);
                    ui.close();
                }
            });
        }
        if wide {
            ui.add_space(2.0);
            let add = add_slot(
                ui,
                vec2(ui.available_width() - 22.0, 26.0),
                tr("+ Add an effect or a plugin"),
                tr("The effect browser: built-in effects and installed plugins"),
            );
            if add.clicked() {
                self.rack.place = None;
                self.browser.borrow_mut().open((channel, monitor));
            }
            self.insert_drop_row(ui, (channel, monitor, shown.len()), add.rect, false);
            if !shown.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(tr(
                        "Drag an effect to reorder the chain. Click to edit, right-click for \
                         more.",
                    ))
                    .size(9.5)
                    .color(theme::current(ui).weak.gamma_multiply(0.8)),
                );
            }
        }
        // Empty slots down to the bottom of the section, one at least in a strip.
        let least = if wide { 0.0 } else { 1.0 };
        let free = height.map_or(least, |h| {
            ((h - (ui.cursor().top() - start)) / SLOT).floor()
        });
        for _ in 0..(free.max(least) as usize) {
            let slot = empty_slot(ui, STRIP_WIDTH - 12.0, tr("Add an effect or a plugin"));
            if slot.clicked() {
                self.rack.place = None;
                self.browser.borrow_mut().open((channel, monitor));
            }
            self.insert_drop_row(ui, (channel, monitor, shown.len()), slot.rect, false);
        }
        // The free room below the list: its end.
        let rest = Rect::from_min_max(
            pos2(ui.max_rect().left(), ui.cursor().top()),
            ui.max_rect().right_bottom(),
        );
        self.insert_drop_row(ui, (channel, monitor, shown.len()), rest, false);
    }

    /// While an insert is dragged over `rect` (row `index` of a channel's list, or the end of
    /// the list), remembers where it would land.
    fn insert_drop_row(
        &mut self,
        ui: &Ui,
        (channel, monitor, index): (Channel, bool, usize),
        rect: Rect,
        row: bool,
    ) {
        if self.fx_drag.is_none() || self.fx_viewport != ui.ctx().viewport_id() {
            return;
        }
        let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !rect.contains(pos) || !ui.clip_rect().contains(pos) {
            return;
        }
        let after = row && pos.y > rect.center().y;
        let y = if after {
            rect.bottom() + 1.0
        } else {
            rect.top() - 1.0
        };
        let width = (STRIP_WIDTH - 12.0).max(rect.width());
        self.fx_target = Some(FxTarget {
            channel,
            monitor,
            index: index + usize::from(after),
            line: [pos2(rect.left(), y), pos2(rect.left() + width, y)],
            outline: None,
        });
    }

    /// A dragged insert over a strip but not over its list goes at the end of the list.
    fn insert_drop_strip(&mut self, ui: &Ui, channel: Channel, count: usize, rect: Rect) {
        if self.fx_drag.is_none()
            || self.fx_viewport != ui.ctx().viewport_id()
            || !ui.rect_contains_pointer(rect)
        {
            return;
        }
        self.fx_fallback = Some(FxTarget {
            channel,
            monitor: false,
            index: count,
            line: [rect.left_top(), rect.right_top()],
            outline: Some(rect),
        });
    }

    /// Shows the dragged insert under the pointer and puts it where it is dropped: moved
    /// within its list, copied to another track (moved with Alt).
    fn drop_insert(&mut self, ui: &Ui, doc: &mut Document) {
        let Some((from, id, name)) = self.fx_drag.clone() else {
            return;
        };
        if self.fx_viewport != ui.ctx().viewport_id() {
            return;
        }
        let p = theme::current(ui);
        let (pointer, released, alt) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                !i.pointer.any_down(),
                i.modifiers.alt,
            )
        });
        let target = self.fx_target.or(self.fx_fallback);
        let copy = target.is_some_and(|t| t.channel != from) && !alt;
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::unique("insert-drag"),
        ));
        if let Some(pos) = pointer {
            let text = if copy { format!("+ {name}") } else { name };
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), p.text);
            let rect = Rect::from_min_size(pos + vec2(14.0, 6.0), galley.size() + vec2(10.0, 4.0));
            painter.rect_filled(rect, 3.0, p.surface);
            painter.galley(rect.min + vec2(5.0, 2.0), galley, p.text);
        }
        if let Some(t) = &target {
            match t.outline {
                Some(rect) => painter.rect_stroke(
                    rect,
                    6.0,
                    egui::Stroke::new(2.0, p.accent),
                    egui::StrokeKind::Inside,
                ),
                None => painter.line_segment(t.line, egui::Stroke::new(2.0, p.accent)),
            };
        }
        if !released {
            ui.ctx().request_repaint();
            return;
        }
        self.fx_drag = None;
        let Some(t) = target else {
            return;
        };
        // The lists show the inserts without the channel strip: the drop goes among those.
        let index = listed_index(doc.project(), t.channel, t.monitor, t.index);
        // Dropped where it already is.
        let list = routing::inserts(doc.project(), from);
        let monitor = list.iter().find(|i| i.id == id).is_some_and(|i| i.monitor);
        let at = list
            .iter()
            .filter(|i| i.monitor == monitor)
            .position(|i| i.id == id);
        if !copy
            && t.channel == from
            && t.monitor == monitor
            && at.is_some_and(|a| index == a || index == a + 1)
        {
            return;
        }
        let mut placed = None;
        let label = if copy {
            tr("Copy insert")
        } else {
            tr("Move insert")
        };
        doc.history_mut().edit(label, |p| {
            placed = routing::place_insert(p, (from, id), (t.channel, t.monitor, index), copy);
        });
        if placed.is_none() {
            doc.notify(tr("This track cannot hold effects").into());
        } else if !copy && t.channel != from {
            for editor in &mut self.editors {
                if *editor == (from, id) {
                    editor.0 = t.channel;
                }
            }
        }
    }

    /// Adds an effect to a channel and opens its editor (and its own window for a plugin).
    fn add_insert(
        &mut self,
        doc: &mut Document,
        channel: Channel,
        monitor: bool,
        key: &str,
        name: &str,
    ) -> Option<InsertId> {
        if let Some(chain) = key.strip_prefix(crate::effect_browser::CHAIN_KEY) {
            let root = crate::chains::root();
            if let Some(chain) = crate::chains::list(root.as_deref())
                .into_iter()
                .find(|c| c.name == chain)
            {
                apply_chain(doc, (channel, monitor), &chain);
            }
            return None;
        }
        let mut added = None;
        let settings = if key == "trigger" {
            crate::trigger_editor::first_settings(doc, channel)
        } else {
            Default::default()
        };
        // An instrument leads its track, in place of the one there.
        let replaced = match channel {
            Channel::Track(t) if is_instrument(doc, key) => Some(
                doc.project()
                    .track(t)
                    .and_then(|t| t.inserts.first())
                    .filter(|i| is_instrument(doc, &i.effect))
                    .map(|i| i.id),
            ),
            _ => None,
        };
        doc.history_mut()
            .edit(&trf!("Insert {name}", name = name), |p| {
                added = routing::add_insert(p, channel, key, settings);
                if monitor
                    && let Some(i) = p.master.inserts.iter_mut().find(|i| Some(i.id) == added)
                {
                    i.monitor = true;
                }
                if let (Some(old), Some(id)) = (replaced, added) {
                    if let Some(old) = old {
                        routing::remove_insert(p, channel, old);
                    }
                    added = routing::place_insert(p, (channel, id), (channel, false, 0), false);
                }
            });
        if let Some(Some(old)) = replaced {
            self.editors.retain(|e| *e != (channel, old));
        }
        if let Some(id) = added {
            if audiostrada_dsp::info(key).is_none() {
                self.open_gui.push((channel, id));
            } else {
                self.editors.push((channel, id));
            }
        }
        added
    }

    fn sends_section(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        track: &Track,
        height: Option<f32>,
    ) {
        let start = ui.cursor().top();
        let id = track.id;
        let edit_send = |doc: &mut Document,
                         label: &str,
                         target: TrackId,
                         f: &dyn Fn(&mut audiostrada_core::TrackSend)| {
            doc.history_mut().edit(label, |p| {
                if let Some(s) = p
                    .track_mut(id)
                    .and_then(|t| t.sends.iter_mut().find(|s| s.target == target))
                {
                    f(s);
                }
            });
        };
        let wide = height.is_none();
        for send in &track.sends {
            let target = send.target;
            let name = project.track(target).map_or("?", |t| t.name.as_str());
            let color = project.track(target).map_or(Color32::GRAY, |t| {
                Color32::from_rgb(t.color[0], t.color[1], t.color[2])
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // The destination's color, then its name: a send reads as where it goes.
                let row_height = if wide { 22.0 } else { 16.0 };
                let (bar, _) = ui.allocate_exact_size(vec2(3.0, row_height), Sense::hover());
                ui.painter().rect_filled(
                    bar,
                    1.5,
                    color.gamma_multiply(if send.mute { 0.3 } else { 1.0 }),
                );
                let mut text = RichText::new(name).size(if wide { 11.5 } else { 10.0 });
                if send.mute {
                    text = text.strikethrough().weak();
                }
                let label = ui
                    .allocate_ui_with_layout(
                        vec2(if wide { 80.0 } else { 34.0 }, row_height),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_size(vec2(if wide { 80.0 } else { 34.0 }, row_height));
                            ui.add(egui::Label::new(text).truncate().sense(Sense::click()))
                        },
                    )
                    .inner
                    .on_hover_text(if send.pre_fader {
                        trf!(
                            "Send to {name} (pre-fader). Right-click for options",
                            name = name
                        )
                    } else {
                        trf!(
                            "Send to {name} (post-fader). Right-click for options",
                            name = name
                        )
                    });
                let lane = AutomationTarget::Send(target);
                let shown = doc
                    .automation_value(Channel::Track(id), &lane)
                    .unwrap_or(send.gain_db);
                let level_width = if wide {
                    (ui.available_width() - 70.0).max(60.0)
                } else {
                    40.0
                };
                let (changed, response) = send_level(ui, level_width, shown, color, send.mute);
                let db = changed.unwrap_or(shown);
                let alone = clutch(ui);
                automated_control(
                    doc,
                    Channel::Track(id),
                    &lane,
                    (db != shown).then_some(db),
                    &response,
                    |doc, db| {
                        doc.history_mut().edit_merged(
                            tr("Send level"),
                            &format!("send:{}:{}", id.0, target.0),
                            |p| {
                                groups::maybe_alone(p, alone, |p| {
                                    groups::set_send_gain(p, id, target, db)
                                })
                            },
                        );
                    },
                );
                learn_menu(
                    &response,
                    doc,
                    Channel::Track(id),
                    &lane,
                    (FADER_MIN_DB, FADER_MAX_DB),
                );
                let badge = if send.pre_fader {
                    tr("PRE")
                } else {
                    tr("POST")
                };
                let badge = RichText::new(badge).size(7.5).color(if send.pre_fader {
                    WARN
                } else {
                    theme::current(ui).weak
                });
                if ui
                    .add(egui::Label::new(badge).sense(Sense::click()))
                    .on_hover_text(if send.pre_fader {
                        tr("Pre-fader: the level does not follow the fader. Click for post-fader")
                    } else {
                        tr("Post-fader: the level follows the fader. Click for pre-fader")
                    })
                    .clicked()
                {
                    edit_send(doc, tr("Send position"), target, &|s| {
                        s.pre_fader = !s.pre_fader
                    });
                }
                if wide {
                    let mute = ui
                        .add(
                            egui::Button::new(RichText::new("M").size(9.5).strong())
                                .min_size(vec2(18.0, 18.0))
                                .selected(send.mute),
                        )
                        .on_hover_text(if send.mute {
                            tr("Send muted: click to hear it again")
                        } else {
                            tr("Mute this send")
                        });
                    if mute.clicked() {
                        edit_send(doc, tr("Mute send"), target, &|s| s.mute = !s.mute);
                    }
                    if remove_button(ui).clicked() {
                        doc.history_mut()
                            .edit(tr("Remove send"), |p| routing::remove_send(p, id, target));
                    }
                }
                label.context_menu(|ui| {
                    let position = if send.pre_fader {
                        tr("Post-fader")
                    } else {
                        tr("Pre-fader")
                    };
                    if ui.button(position).clicked() {
                        edit_send(doc, tr("Send position"), target, &|s| {
                            s.pre_fader = !s.pre_fader
                        });
                        ui.close();
                    }
                    if ui
                        .button(if send.mute { tr("Unmute") } else { tr("Mute") })
                        .clicked()
                    {
                        edit_send(doc, tr("Mute send"), target, &|s| s.mute = !s.mute);
                        ui.close();
                    }
                    if ui.button(tr("Remove")).clicked() {
                        doc.history_mut()
                            .edit(tr("Remove send"), |p| routing::remove_send(p, id, target));
                        ui.close();
                    }
                });
            });
        }
        // Empty slots down to the bottom of the section, one at least; each offers the buses.
        let free = height.map_or(1.0, |h| ((h - (ui.cursor().top() - start)) / SLOT).floor());
        for _ in 0..(free.max(1.0) as usize) {
            let slot = if wide {
                ui.add_space(2.0);
                add_slot(
                    ui,
                    vec2(ui.available_width(), 26.0),
                    tr("+ Add a send"),
                    tr("Add a send"),
                )
            } else {
                empty_slot(ui, STRIP_WIDTH - 12.0, tr("Add a send"))
            };
            // Stays open when its checkbox is clicked.
            egui::Popup::menu(&slot)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.add_send_menu(ui, doc, project, id));
        }
    }

    /// The buses a send can go to, with the option to cut the output to the master.
    fn add_send_menu(&mut self, ui: &mut Ui, doc: &mut Document, project: &Project, id: TrackId) {
        // Like the output, a send is added to every selected track.
        let tracks = audio_selection_with(doc, project, id);
        // The tracks of `tracks` that can send to `target`.
        let senders = |target: TrackId| -> Vec<TrackId> {
            tracks
                .iter()
                .copied()
                .filter(|&t| {
                    t != target
                        && !routing::reaches(project, target, t)
                        && project
                            .track(t)
                            .is_some_and(|t| !t.sends.iter().any(|s| s.target == target))
                })
                .collect()
        };
        if tracks.len() > 1 {
            ui.weak(trf!("{count} tracks selected", count = tracks.len()));
        }
        let cut_id = egui::Id::unique("send-cuts-main-output");
        let mut cut = ui.data(|d| d.get_temp::<bool>(cut_id)).unwrap_or(false);
        if ui
            .checkbox(&mut cut, tr("Cut the output to the master"))
            .on_hover_text(tr(
                "The track is then heard only through the send (its folder or master \
             output is turned off)",
            ))
            .changed()
        {
            ui.data_mut(|d| d.insert_temp(cut_id, cut));
        }
        ui.separator();
        let candidates: Vec<&Track> = project
            .tracks
            .iter()
            .map(|t| t.as_ref())
            .filter(|t| {
                t.kind == TrackKind::Audio
                    && !tracks.contains(&t.id)
                    && tracks.iter().any(|&from| {
                        project
                            .track(from)
                            .is_some_and(|f| !f.sends.iter().any(|s| s.target == t.id))
                    })
            })
            .collect();
        if candidates.is_empty() {
            ui.weak(tr("Add a bus first"));
        }
        // A long list scrolls rather than leaving its window.
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for target in candidates {
                    let from = senders(target.id);
                    let response = ui
                        .add_enabled(!from.is_empty(), egui::Button::new(&target.name))
                        .on_disabled_hover_text(tr("Would create a feedback loop"));
                    if response.clicked() {
                        let mut result = Ok(());
                        doc.history_mut().edit(tr("Add send"), |p| {
                            for &t in &from {
                                result = result.and(routing::add_send(p, t, target.id));
                                if cut {
                                    result = result.and(routing::set_master_send(p, t, false));
                                }
                            }
                        });
                        if let Err(e) = result {
                            self.error = Some(e.to_string());
                        }
                        ui.close();
                    }
                }
            });
    }

    fn routing_section(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        track: &Track,
        width: f32,
    ) {
        let id = track.id;
        let parent = track.parent.and_then(|p| project.track(p));
        let output = match (track.master_send, parent) {
            (false, _) => tr("No output").to_owned(),
            (true, Some(p)) => format!("→ {}", p.name),
            (true, None) => tr("→ Master").to_owned(),
        };
        let vca = track.vca.and_then(|v| project.track(v));
        let label = match vca {
            Some(v) => format!("{output} · {}", v.name),
            None => output,
        };
        let button = egui::Button::new(RichText::new(&label).size(10.5))
            .wrap_mode(egui::TextWrapMode::Truncate);
        let response = ui
            .add_sized([width, 20.0], button)
            .on_hover_text(trf!("Output: {label}", label = label));
        // Routing changes apply to every selected track when this one is part of the selection.
        let tracks = audio_selection_with(doc, project, id);
        egui::Popup::menu(&response).show(|ui| {
            if tracks.len() > 1 {
                ui.weak(trf!("{count} tracks selected", count = tracks.len()));
            }
            let vcas: Vec<&Track> = project
                .tracks
                .iter()
                .map(|t| t.as_ref())
                .filter(|t| t.kind == TrackKind::Vca)
                .collect();
            if !vcas.is_empty() {
                ui.label(RichText::new(tr("VCA (its fader and mute act on this track)")).weak());
                let choices = std::iter::once((None, tr("None")))
                    .chain(vcas.iter().map(|v| (Some(v.id), v.name.as_str())));
                for (choice, name) in choices {
                    if ui.selectable_label(track.vca == choice, name).clicked() {
                        doc.history_mut().edit(tr("Assign VCA"), |p| {
                            for &t in &tracks {
                                let _ = routing::set_vca(p, t, choice);
                            }
                        });
                        ui.close();
                    }
                }
                ui.separator();
            }
            let mut on = track.master_send;
            if ui
                .checkbox(&mut on, tr("Send to folder / master"))
                .changed()
            {
                let mut result = Ok(());
                doc.history_mut().edit(tr("Output"), |p| {
                    for &t in &tracks {
                        result = result.and(routing::set_master_send(p, t, on));
                    }
                });
                if let Err(e) = result {
                    self.error = Some(e.to_string());
                }
            }
            ui.separator();
            ui.label(RichText::new(tr("Folder")).weak());
            let inside: Vec<TrackId> = tracks
                .iter()
                .flat_map(|&t| routing::subtree(project, t))
                .collect();
            let mut targets: Vec<(Option<TrackId>, &str)> = vec![(None, tr("None (top level)"))];
            targets.extend(
                project
                    .tracks
                    .iter()
                    .filter(|t| t.kind == TrackKind::Audio && !inside.contains(&t.id))
                    .map(|t| (Some(t.id), t.name.as_str())),
            );
            for (target, name) in targets {
                if ui.selectable_label(track.parent == target, name).clicked() {
                    let mut result = Ok(());
                    doc.history_mut().edit(tr("Move to folder"), |p| {
                        result = routing::set_parents(p, &tracks, target);
                    });
                    if let Err(e) = result {
                        self.error = Some(e.to_string());
                    }
                    ui.close();
                }
            }
        });
    }

    fn master_strip(&mut self, ui: &mut Ui, doc: &mut Document, project: &Project, elapsed: f32) {
        self.master_levels
            .take(doc.player().master_meter(), elapsed);
        let mut levels = self.master_levels;
        let sizes = self.sizes;
        let fader_height = self.fader_height;
        let width = STRIP_WIDTH + 24.0;
        // Compact, the master stays short so the narrow faders get the height.
        let full = self.full_strips && !self.compact;
        Self::strip_frame(ui, width, Color32::from_gray(200), false, |ui| {
            let top = ui.cursor().top();
            if full {
                ui.add_space(3.0);
                self.channel_section(ui, doc, Channel::Master, width);
                self.inserts_title(ui, doc, Channel::Master, tr("INSERTS"), true);
                section(ui, None, sizes.fx, "master-inserts", |ui| {
                    self.inserts_section(
                        ui,
                        doc,
                        Channel::Master,
                        &project.master.inserts,
                        tr("Master"),
                        false,
                        Some(sizes.fx),
                    );
                });
                let monitor = sizes.sends.unwrap_or(24.0);
                self.inserts_title(
                    ui,
                    doc,
                    Channel::Master,
                    tr("MONITOR (NOT EXPORTED)"),
                    false,
                );
                section(ui, None, monitor, "monitor-inserts", |ui| {
                    self.inserts_section(
                        ui,
                        doc,
                        Channel::Master,
                        &project.master.inserts,
                        tr("Monitor"),
                        true,
                        Some(monitor),
                    );
                });
            } else {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let inserts = &project.master.inserts;
                    let monitor = inserts.iter().filter(|i| i.monitor).count();
                    let fx = inserts
                        .iter()
                        .filter(|i| !i.monitor && i.effect != routing::STRIP)
                        .count();
                    self.list_button(ui, Channel::Master, List::Inserts, fx, vec2(54.0, 22.0));
                    self.list_button(
                        ui,
                        Channel::Master,
                        List::Monitor,
                        monitor,
                        vec2(54.0, 22.0),
                    );
                });
                ui.horizontal(|ui| {
                    self.strip_button(ui, doc, Channel::Master, vec2(54.0, 22.0));
                });
            }
            self.align_top(ui, top);
            let row_top = ui.cursor().top();
            ui.horizontal(|ui| {
                ui.set_max_height(fader_height);
                let shown = doc
                    .automation_value(Channel::Master, &AutomationTarget::Gain)
                    .unwrap_or(project.master.gain_db);
                let (changed, response) = widgets::fader(ui, shown, fader_height, true);
                automated_control(
                    doc,
                    Channel::Master,
                    &AutomationTarget::Gain,
                    changed,
                    &response,
                    |doc, db| {
                        doc.history_mut()
                            .edit_merged(tr("Master gain"), "gain:master", |p| {
                                p.master.gain_db = db
                            });
                    },
                );
                learn_menu(
                    &response,
                    doc,
                    Channel::Master,
                    &AutomationTarget::Gain,
                    (FADER_MIN_DB, FADER_MAX_DB),
                );
                widgets::meter(ui, &mut levels, fader_height);
                // The readings, then Reset and REF at the bottom: in the tracks' switch column.
                ui.vertical(|ui| {
                    let top = ui.cursor().top();
                    let buttons = 2.0 * 21.0;
                    self.loudness(ui, doc, fader_height - buttons);
                    let left = fader_height - (ui.cursor().top() - top) - buttons;
                    if left > 0.0 {
                        ui.add_space(left);
                    }
                    ui.spacing_mut().item_spacing.y = 3.0;
                    self.loudness_buttons(ui, doc);
                });
            });
            // As tall as the tracks' fader row.
            let short = self.row_height - (ui.cursor().top() - row_top);
            if short > 0.5 {
                ui.allocate_exact_size(
                    vec2(1.0, short - ui.spacing().item_spacing.y),
                    Sense::hover(),
                );
            }
            let bottom = ui.cursor().top();
            readout(
                ui,
                64.0,
                &db_label(
                    doc.automation_value(Channel::Master, &AutomationTarget::Gain)
                        .unwrap_or(project.master.gain_db),
                ),
            );
            // Same height as the track strips' buttons, so the name plates line up.
            let used = ui.cursor().top() - bottom;
            let pad = (self.bottom_height - used - ui.spacing().item_spacing.y).max(0.0);
            if pad > 0.0 {
                // add_space is ignored in this centered layout.
                ui.allocate_exact_size(vec2(1.0, pad), Sense::hover());
            }
            name_plate(ui, width, "Master", Color32::from_gray(150), false, None);
        });
        self.master_levels = levels;
    }

    /// The buttons beside a docked strip's fader that open its inserts and sends.
    fn list_buttons(&mut self, ui: &mut Ui, doc: &mut Document, track: &Track, room: f32) {
        let top = ui.cursor().top();
        let channel = Channel::Track(track.id);
        let size = vec2(48.0, 22.0);
        let fx = track
            .inserts
            .iter()
            .filter(|i| i.effect != routing::STRIP)
            .count();
        self.list_button(ui, channel, List::Inserts, fx, size);
        self.list_button(ui, channel, List::Sends, track.sends.len(), size);
        self.strip_button(ui, doc, channel, size);
        // The pills scroll rather than make the strip taller.
        let left = (room - (ui.cursor().top() - top)).max(0.0);
        egui::ScrollArea::vertical()
            .id_salt(("pills", track.id.0))
            .max_height(left)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(size.x);
                self.insert_pills(ui, doc, channel, size.x);
                send_pills(ui, doc.project(), track, size.x);
            });
    }

    /// The button of the channel strip, lit when one of its modules is on; opens it.
    fn strip_button(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        channel: Channel,
        size: egui::Vec2,
    ) {
        let p = theme::current(ui);
        let on = strip_modules(doc.project(), channel)
            .iter()
            .any(|&(_, on)| on);
        let mut button = egui::Button::new("").min_size(size);
        if on {
            button = button.fill(p.accent.gamma_multiply(0.35));
        }
        let response = ui.add(button);
        // A small EQ curve: the strip.
        let r = response.rect.shrink2(vec2(8.0, 7.0));
        let points: Vec<egui::Pos2> = (0..=12)
            .map(|i| {
                let t = i as f32 / 12.0;
                let bump = (-(t - 0.6).powi(2) / 0.02).exp() * 0.8 - (1.0 - t).powi(6) * 0.9;
                pos2(
                    r.left() + r.width() * t,
                    r.center().y - bump * r.height() * 0.5,
                )
            })
            .collect();
        ui.painter().add(egui::Shape::line(
            points,
            egui::Stroke::new(1.5, if on { p.text } else { p.weak }),
        ));
        // The console's color along the bottom of the button.
        let console = crate::console_ui::of_channel(doc, channel);
        if console.colors() && !doc.project().console_off {
            let r = response.rect.shrink2(vec2(6.0, 3.0));
            ui.painter().line_segment(
                [r.left_bottom(), r.right_bottom()],
                egui::Stroke::new(2.0, crate::console_ui::color(console.model)),
            );
        }
        let response = response.on_hover_text(tr(
            "Channel strip: filters, gate, compressor, EQ and drive, ahead of the inserts. Click \
             to open, right-click for the console",
        ));
        if response.clicked() {
            self.open_strip(doc, channel);
        }
        egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| match channel {
                Channel::Track(t) => crate::console_ui::track_menu(ui, doc, t),
                Channel::Master => crate::console_ui::console_menu(ui, doc),
            });
    }

    /// A pill per insert of a compact strip, in its family's color: lit while it plays, a click
    /// bypasses it or plays it again, a right-click opens it.
    fn insert_pills(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel, width: f32) {
        let p = theme::current(ui);
        let project = doc.project().clone();
        let off = project.all_fx_off
            || match channel {
                Channel::Track(t) => project.track(t).is_some_and(|t| t.fx_off),
                Channel::Master => project.master.fx_off,
            };
        let inserts: Vec<&audiostrada_core::Insert> = routing::inserts(&project, channel)
            .iter()
            .filter(|i| !i.monitor && i.effect != routing::STRIP)
            .collect();
        if inserts.is_empty() {
            return;
        }
        ui.add_space(2.0);
        for insert in &inserts {
            let (rect, response) = ui.allocate_exact_size(vec2(width, 7.0), Sense::click());
            let lit = !(insert.bypass || off);
            let color = theme::family_color(family(doc, &insert.effect));
            let fill = if lit {
                color.gamma_multiply(0.9)
            } else {
                p.surface
            };
            ui.painter().rect_filled(rect, 3.0, fill);
            if !lit {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.0, color.gamma_multiply(0.5)),
                    egui::StrokeKind::Inside,
                );
            }
            if response.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    egui::Stroke::new(1.0, p.text),
                    egui::StrokeKind::Inside,
                );
            }
            let name = effect_name(doc, &insert.effect);
            let response = response.on_hover_text(trf!(
                "{name}: {state}. Click to switch it, right-click to open it",
                name = name,
                state = if lit { tr("playing") } else { tr("bypassed") }
            ));
            let id = insert.id;
            if response.clicked() {
                let bypass = !insert.bypass;
                doc.history_mut().edit(tr("Bypass insert"), |p| {
                    routing::set_bypass(p, channel, id, bypass)
                });
            }
            if response.secondary_clicked() {
                if insert.effect == routing::PARALLEL {
                    self.reveal_rack(doc, channel);
                } else if audiostrada_dsp::info(&insert.effect).is_some() {
                    self.show_editor(channel, id);
                } else if !self.open_gui.contains(&(channel, id)) {
                    self.open_gui.push((channel, id));
                }
            }
        }
    }

    /// Opens the channel strip of a channel, adding it if it has none yet.
    fn open_strip(&mut self, doc: &mut Document, channel: Channel) {
        let id = match routing::strip(doc.project(), channel) {
            Some(strip) => Some(strip.id),
            None => {
                let after_first =
                    matches!(channel, Channel::Track(t) if doc.is_instrument_track(t));
                let mut id = None;
                doc.history_mut().edit(tr("Channel strip"), |p| {
                    id = routing::ensure_strip(p, channel, after_first)
                });
                id
            }
        };
        if let Some(id) = id {
            self.show_editor(channel, id);
        }
    }

    /// The console a channel plays through, as a dot of its color; a click picks it.
    fn console_dot(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel) {
        let console = crate::console_ui::of_channel(doc, channel);
        let own = match channel {
            Channel::Track(t) => doc.project().track(t).is_some_and(|t| t.console.is_some()),
            Channel::Master => false,
        };
        let (rect, response) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::click());
        let tint = crate::console_ui::color(console.model);
        let lit = console.colors() && !doc.project().console_off;
        let center = rect.center();
        if lit {
            ui.painter().circle_filled(center, 3.5, tint);
        } else {
            ui.painter().circle_stroke(
                center,
                3.0,
                egui::Stroke::new(1.0, tint.gamma_multiply(0.6)),
            );
        }
        if own {
            // Its own console: a ring around the dot.
            ui.painter()
                .circle_stroke(center, 5.5, egui::Stroke::new(1.0, tint));
        }
        let hint = if console.colors() {
            trf!(
                "Console: {name}, {amount} %. Click to change it",
                name = crate::console_ui::name(console.model),
                amount = console.amount.round()
            )
        } else {
            tr("Console: neutral. Click to change it").to_owned()
        };
        let response = response.on_hover_text(hint);
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| match channel {
                Channel::Track(t) => crate::console_ui::track_menu(ui, doc, t),
                Channel::Master => crate::console_ui::console_menu(ui, doc),
            });
    }

    /// Switches a module of the channel strip, adding the strip if it has none yet.
    fn set_strip_module(&mut self, doc: &mut Document, channel: Channel, module: &str, on: bool) {
        let after_first = matches!(channel, Channel::Track(t) if doc.is_instrument_track(t));
        let label = if on {
            tr("Channel strip module on")
        } else {
            tr("Channel strip module off")
        };
        doc.history_mut().edit(label, |p| {
            if let Some(id) = routing::ensure_strip(p, channel, after_first) {
                routing::set_insert_param(p, channel, id, module, if on { 1.0 } else { 0.0 });
            }
        });
    }

    /// The channel strip on a tall strip: its modules, lit when on (a click switches one), and
    /// its curve and gain reduction (a click opens it).
    fn channel_section(&mut self, ui: &mut Ui, doc: &mut Document, channel: Channel, width: f32) {
        let p = theme::current(ui);
        let modules = strip_modules(doc.project(), channel);
        let open = routing::strip(doc.project(), channel)
            .is_some_and(|s| self.editors.contains(&(channel, s.id)));
        ui.horizontal(|ui| {
            ui.set_height(13.0);
            ui.label(RichText::new(tr("CHANNEL STRIP")).size(8.5).color(p.weak));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(2.0);
                let (rect, response) = ui.allocate_exact_size(vec2(20.0, 13.0), Sense::click());
                let fill = if open {
                    p.accent.gamma_multiply(0.6)
                } else if response.hovered() {
                    p.hover
                } else {
                    p.surface
                };
                ui.painter().rect_filled(rect, 3.0, fill);
                let response = response
                    .on_hover_text(tr("Open the channel strip: its curve and every setting"));
                // An arrow out of a box: opens its window.
                let r = Rect::from_center_size(response.rect.center(), vec2(6.0, 6.0));
                let ink = egui::Stroke::new(1.2, if response.hovered() { p.text } else { p.weak });
                let painter = ui.painter();
                painter.line_segment([r.left_bottom(), r.right_top()], ink);
                painter.line_segment([r.right_top(), r.right_top() + vec2(-4.0, 0.0)], ink);
                painter.line_segment([r.right_top(), r.right_top() + vec2(0.0, 4.0)], ink);
                if response.clicked() {
                    self.open_strip(doc, channel);
                }
                self.console_dot(ui, doc, channel);
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let w = ((width - 8.0) / modules.len() as f32).floor();
            for &(k, on) in &modules {
                let module = audiostrada_dsp::channel::MODULES[k];
                let (label, hint, family) = STRIP_MODULES[k];
                let (rect, response) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click());
                let color = theme::family_color(family);
                let fill = if on {
                    color.gamma_multiply(0.85)
                } else if response.hovered() {
                    p.hover
                } else {
                    p.surface
                };
                ui.painter().rect_filled(rect, 3.0, fill);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(8.5),
                    if on { theme::text_on(fill) } else { p.weak },
                );
                if response.on_hover_text(tr(hint)).clicked() {
                    self.set_strip_module(doc, channel, module, !on);
                }
            }
        });
        let (rect, response) = ui.allocate_exact_size(vec2(width - 4.0, 22.0), Sense::click());
        ui.painter().rect_filled(rect, 3.0, p.base);
        let strip = routing::strip(doc.project(), channel).cloned();
        if let Some(strip) = &strip {
            // The response of its filters and EQ.
            let info = &audiostrada_dsp::channel::INFO;
            use audiostrada_dsp::Effect;
            let mut dsp = audiostrada_dsp::channel::Channel::default();
            dsp.prepare(48_000, 64);
            for (i, param) in info.params.iter().enumerate() {
                if param.id == "character" {
                    dsp.set_param(i, crate::console_ui::character(doc, channel) as f32);
                } else if let Some(&v) = strip.params.get(param.id) {
                    dsp.set_param(i, v);
                }
            }
            let mid = rect.center().y;
            ui.painter().line_segment(
                [pos2(rect.left(), mid), pos2(rect.right(), mid)],
                egui::Stroke::new(1.0, p.weak.gamma_multiply(0.2)),
            );
            let points: Vec<egui::Pos2> = (0..=40)
                .map(|i| {
                    let t = i as f32 / 40.0;
                    let freq = 20.0 * 1000f64.powf(f64::from(t));
                    let db = dsp.eq_response_db(freq).clamp(-18.0, 18.0) as f32;
                    pos2(
                        rect.left() + rect.width() * t,
                        mid - db / 18.0 * rect.height() * 0.45,
                    )
                })
                .collect();
            let lit = !strip.bypass;
            ui.painter().add(egui::Shape::line(
                points,
                egui::Stroke::new(1.5, if lit { p.accent } else { p.weak }),
            ));
            if let Some(meters) = doc.player_mut().insert_meters(channel, strip.id) {
                let reduction = meters.get(1).copied().unwrap_or(0.0).abs().min(24.0);
                if reduction > 0.1 {
                    let bar = Rect::from_min_max(
                        pos2(rect.right() - 4.0, rect.top() + 2.0),
                        pos2(
                            rect.right() - 1.0,
                            rect.top() + 2.0 + (rect.height() - 4.0) * reduction / 24.0,
                        ),
                    );
                    ui.painter()
                        .rect_filled(bar, 1.0, theme::family_color("Dynamics"));
                }
            }
        } else {
            let mid = rect.center().y;
            ui.painter().line_segment(
                [pos2(rect.left() + 4.0, mid), pos2(rect.right() - 4.0, mid)],
                egui::Stroke::new(1.0, p.weak.gamma_multiply(0.35)),
            );
        }
        if response
            .on_hover_text(tr("Channel strip: click to open it"))
            .clicked()
        {
            self.open_strip(doc, channel);
        }
    }

    /// A button opening (or closing) one list of a strip in its own window; lit when the list
    /// holds something.
    fn list_button(
        &mut self,
        ui: &mut Ui,
        channel: Channel,
        list: List,
        count: usize,
        size: egui::Vec2,
    ) {
        let p = theme::current(ui);
        let (label, hint) = match list {
            List::Inserts => (
                tr("FX"),
                trf!("Effects: {count}. Click to open the list", count = count),
            ),
            List::Sends => (
                tr("Send"),
                trf!("Sends: {count}. Click to open the list", count = count),
            ),
            List::Monitor => (
                tr("Mon"),
                trf!(
                    "Monitoring effects (not exported): {count}. Click to open the list",
                    count = count
                ),
            ),
        };
        let open = self.lists.contains(&(channel, list));
        let text = if count > 0 {
            RichText::new(format!("{label} {count}"))
                .size(10.0)
                .strong()
        } else {
            RichText::new(label).size(10.0).color(p.weak)
        };
        let mut button = egui::Button::new(text).min_size(size).selected(open);
        if count > 0 && !open {
            button = button.fill(p.accent.gamma_multiply(0.35));
        }
        if ui.add(button).on_hover_text(hint).clicked() {
            if open {
                self.lists.retain(|l| *l != (channel, list));
            } else {
                self.lists.push((channel, list));
            }
        }
    }

    /// The lists opened from the docked strips, each in its own window.
    fn list_windows(&mut self, ctx: &egui::Context, doc: &mut Document, project: &Project) {
        let places = self.places;
        let mut closed = Vec::new();
        for (channel, list) in self.lists.clone() {
            let track = match channel {
                Channel::Track(t) => match project.track(t) {
                    Some(t) => Some(t),
                    None => {
                        closed.push((channel, list));
                        continue;
                    }
                },
                Channel::Master => None,
            };
            let owner = owner_name(project, channel);
            let title = match list {
                List::Inserts => trf!("{owner}: effects", owner = owner),
                List::Sends => trf!("{owner}: sends", owner = owner),
                List::Monitor => trf!("{owner}: monitoring", owner = owner),
            };
            let key = ("strip-list", channel, list);
            let size = if list == List::Sends {
                [320.0, 400.0]
            } else {
                [300.0, 460.0]
            };
            let open = editor_window(ctx, places, key, &title, size, false, false, |ui| {
                self.list_header(ui, doc, project, channel, list);
                ui.add_space(6.0);
                match list {
                    List::Sends => {
                        if let Some(track) = track {
                            self.sends_section(ui, doc, project, track, None);
                        }
                    }
                    List::Inserts | List::Monitor => {
                        let inserts = track.map_or(&project.master.inserts, |t| &t.inserts);
                        let monitor = list == List::Monitor;
                        self.fx_target = None;
                        self.fx_fallback = None;
                        self.inserts_section(ui, doc, channel, inserts, owner, monitor, None);
                        self.drop_insert(ui, doc);
                        save_chain_dialog(ui.ctx(), doc);
                    }
                }
            });
            if !open {
                closed.push((channel, list));
                if self.fx_viewport == egui::ViewportId::from_hash_of(key) {
                    self.fx_drag = None;
                }
            }
        }
        self.lists.retain(|l| !closed.contains(l));
    }

    /// The head of a list window: the owner's color and name, what the list holds and, for
    /// the effects, their bypass buttons.
    fn list_header(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        channel: Channel,
        list: List,
    ) {
        let p = theme::current(ui);
        let (name, color) = match channel {
            Channel::Master => (tr("Master"), Color32::from_gray(200)),
            Channel::Track(t) => project.track(t).map_or(("?", Color32::GRAY), |t| {
                (
                    t.name.as_str(),
                    Color32::from_rgb(t.color[0], t.color[1], t.color[2]),
                )
            }),
        };
        let summary = match list {
            List::Sends => {
                let count = match channel {
                    Channel::Track(t) => project.track(t).map_or(0, |t| t.sends.len()),
                    Channel::Master => 0,
                };
                trf!("Sends: {count}", count = count)
            }
            List::Inserts | List::Monitor => {
                let monitor = list == List::Monitor;
                let shown: Vec<_> = routing::inserts(project, channel)
                    .iter()
                    .filter(|i| i.monitor == monitor && i.effect != routing::STRIP)
                    .collect();
                let bypassed = shown.iter().filter(|i| i.bypass).count();
                if bypassed > 0 {
                    trf!(
                        "Effects: {count}, bypassed: {bypassed}",
                        count = shown.len(),
                        bypassed = bypassed
                    )
                } else {
                    trf!("Effects: {count}", count = shown.len())
                }
            }
        };
        let frame = egui::Frame::new()
            .fill(color.gamma_multiply(0.16))
            .corner_radius(6.0)
            .inner_margin(egui::Margin {
                left: 12,
                right: 6,
                top: 6,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add(
                            egui::Label::new(RichText::new(name).size(14.0).strong().color(p.text))
                                .truncate(),
                        );
                        ui.label(RichText::new(summary).size(10.0).color(p.weak));
                    });
                    if list == List::Inserts {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            self.fx_toggles(ui, doc, channel, false);
                        });
                    }
                });
            });
        let rect = frame.response.rect;
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(4.0, rect.height())),
            egui::CornerRadius {
                nw: 6,
                sw: 6,
                ..Default::default()
            },
            color,
        );
    }

    /// Loudness readings of the master, beside its meter.
    fn loudness(&mut self, ui: &mut Ui, doc: &mut Document, height: f32) {
        let p = theme::current(ui);
        let loudness = doc.player().loudness();
        let show = |v: Option<f64>| match v {
            Some(v) if v.is_finite() => format!("{v:.1}"),
            _ => "--".to_owned(),
        };
        let peak = gain_to_db(loudness.peak());
        let rows = [
            (
                "I",
                show(loudness.integrated()),
                tr("Integrated loudness since reset, LUFS"),
            ),
            (
                "PK",
                show(Some(f64::from(peak))),
                tr("Highest sample since reset, dBFS"),
            ),
            (
                "M",
                show(loudness.momentary()),
                tr("Momentary loudness (400 ms), LUFS"),
            ),
            (
                "S",
                show(loudness.short_term()),
                tr("Short-term loudness (3 s), LUFS"),
            ),
        ];
        // As many readings as the fader height allows.
        const ROW: f32 = 16.0;
        let shown = ((height / ROW) as usize).min(rows.len());
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.spacing_mut().interact_size.y = ROW;
        for (label, value, hint) in rows.into_iter().take(shown) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(RichText::new(label).size(8.5).color(p.weak));
                ui.label(RichText::new(value).monospace().size(10.5));
            })
            .response
            .on_hover_text(trf!("{hint}. Click Reset to restart", hint = hint));
        }
    }

    /// Reset of the loudness readings and the reference track switch, beside the master fader.
    fn loudness_buttons(&mut self, ui: &mut Ui, doc: &mut Document) {
        let p = theme::current(ui);
        {
            let size = vec2(44.0, 18.0);
            if theme::letter_toggle(ui, size, tr("Reset"), false, p.accent)
                .on_hover_text(tr("Restart the loudness measurement"))
                .clicked()
            {
                doc.player_mut().loudness_mut().reset();
            }
            // Lit while the reference is heard instead of the mix.
            let listening = doc.player().reference_listening();
            let response = theme::letter_toggle(ui, size, tr("REF"), listening, p.accent)
                .on_hover_text(if doc.project().reference.is_some() {
                    tr(
                        "Click to hear the reference track instead of the mix, at the same \
                         loudness; right-click for its settings",
                    )
                } else {
                    tr("Choose a reference track to compare the mix with")
                });
            if response.clicked() {
                crate::reference_window::toggle(ui.ctx(), doc);
            }
            if response.secondary_clicked() {
                crate::reference_window::open(ui.ctx());
            }
        }
    }

    /// Shows a channel's chain in the rack.
    fn reveal_rack(&mut self, doc: &mut Document, channel: Channel) {
        match channel {
            Channel::Track(t) => {
                doc.select_track(t, crate::document::SelectMode::Only);
                if self.rack.pin.is_some_and(|p| p != channel) {
                    self.rack.pin = None;
                }
            }
            Channel::Master => self.rack.pin = Some(Channel::Master),
        }
        self.requests.push(crate::actions::Action::RevealRack);
    }

    /// Opens an insert's window in AudioStrada, or brings it to the front.
    fn show_editor(&mut self, channel: Channel, id: InsertId) {
        if self.editors.contains(&(channel, id)) {
            self.raise = Some(id);
        } else {
            self.editors.push((channel, id));
        }
    }

    /// Opens the plugins' own windows asked for, once each plugin is loaded; a plugin without
    /// a window this system can show (or that failed to load) gets AudioStrada's window instead.
    fn plugin_windows(&mut self, doc: &mut Document, project: &Project) {
        for (channel, id) in std::mem::take(&mut self.open_gui) {
            let Some(insert) = routing::find_insert(project, channel, id) else {
                continue;
            };
            if doc.player().plugin_error(id).is_some() {
                self.show_editor(channel, id);
                continue;
            }
            let title = format!(
                "{}: {}",
                owner_name(project, channel),
                effect_name(doc, &insert.effect)
            );
            let Some(plugin) = doc.player_mut().plugin(channel, id) else {
                // Still loading.
                self.open_gui.push((channel, id));
                continue;
            };
            if !plugin.has_gui() {
                self.show_editor(channel, id);
            } else if plugin.gui_open() {
                plugin.raise_gui();
            } else if let Err(e) = plugin.open_gui(&title) {
                self.error = Some(e);
                self.show_editor(channel, id);
            }
        }
    }

    fn insert_editors(&mut self, ctx: &egui::Context, doc: &mut Document, project: &Project) {
        if std::mem::take(&mut self.parked) {
            ctx.data_mut(|d| {
                d.get_temp_mut_or_default::<WindowPlaces>(self.places)
                    .opened
                    .clear()
            });
        }
        self.plugin_windows(doc, project);
        let mut closed = Vec::new();
        for (channel, id) in self.editors.clone() {
            let Some(insert) = routing::find_insert(project, channel, id) else {
                closed.push((channel, id));
                continue;
            };
            let owner = owner_name(project, channel);
            let Some(info) = audiostrada_dsp::info(&insert.effect) else {
                if !self.plugin_editor(ctx, doc, channel, insert, owner) {
                    closed.push((channel, id));
                }
                continue;
            };
            let title = format!("{owner}: {}", tr(info.name));
            let size = match info.id {
                "eq" => [1040.0, 800.0],
                "amp_rig" => [1000.0, 960.0],
                "channel" => [1060.0, 720.0],
                "synth" => [1180.0, 1000.0],
                "drums" => [1000.0, 720.0],
                "trigger" => [960.0, 760.0],
                "compressor" => [980.0, 720.0],
                "gate" => [980.0, 700.0],
                "deesser" => [980.0, 680.0],
                "transient" => [900.0, 600.0],
                "limiter" => [900.0, 650.0],
                "delay" => [980.0, 620.0],
                "filter" => [1000.0, 600.0],
                "imager" => [900.0, 620.0],
                "utility" => [940.0, 480.0],
                "drive" => [1000.0, 620.0],
                "pitch_correct" => [860.0, 560.0],
                "reverb" => [980.0, 800.0],
                "maximizer" => [900.0, 730.0],
                "vintage_eq" => [820.0, 660.0],
                _ => [760.0, 560.0],
            };
            // The rig picks its own files.
            let rig = info.id == "amp_rig";
            let raise = self.raise == Some(id);
            let places = self.places;
            let open = editor_window(
                ctx,
                places,
                ("insert-editor", id.0),
                &title,
                size,
                raise,
                matches!(info.id, "drums" | "trigger" | "channel" | "amp_rig"),
                |ui| {
                    let state = self.editor_states.entry(id).or_default();
                    let meter = effect_editors::is_meter(info.id);
                    let toggled = effect_editors::header(ui, info, insert.bypass, |ui| {
                        if !meter {
                            effect_editors::preset_bar(ui, doc, channel, insert, state);
                        }
                    });
                    if let Some(bypass) = toggled {
                        doc.history_mut().edit(tr("Bypass insert"), |p| {
                            routing::set_bypass(p, channel, id, bypass)
                        });
                    }
                    // An instrument has no input to match.
                    if !meter && !info.instrument {
                        effect_editors::levels_bar(ui, doc, channel, insert, state);
                    }
                    ui.add_space(4.0);
                    // The synth shows its wavetable files on its own face.
                    if !rig
                        && !info.files.is_empty()
                        && !matches!(info.id, "synth" | "drums" | "trigger")
                    {
                        effect_files(ui, doc, channel, insert, info);
                        ui.separator();
                    }
                    let values: Vec<f32> = info
                        .params
                        .iter()
                        .map(|p| {
                            let lane = AutomationTarget::Param {
                                insert: id,
                                param: p.id.to_owned(),
                            };
                            doc.automation_value(channel, &lane).unwrap_or_else(|| {
                                insert.params.get(p.id).copied().unwrap_or(p.default)
                            })
                        })
                        .collect();
                    let meters = doc
                        .player_mut()
                        .insert_meters(channel, id)
                        .unwrap_or_default();
                    let state = self.editor_states.entry(id).or_default();
                    let mut params = effect_editors::Params {
                        doc: &mut *doc,
                        channel,
                        insert: id,
                        info,
                        values,
                    };
                    effect_editors::show(ui, &mut params, &meters, state);
                },
            );
            if !open {
                closed.push((channel, id));
            }
        }
        self.editors.retain(|e| !closed.contains(e));
        // Match only lasts while its window is open.
        for (channel, id) in closed {
            doc.player_mut().set_insert_match(channel, id, false);
            doc.player_mut().set_insert_watched(channel, id, false);
            if let Some(state) = self.editor_states.get_mut(&id) {
                state.matching = false;
                state.synth.release(doc);
                state.drums.release(doc);
                // A soloed EQ band only lasts while its window is open too.
                if state.eq_solo.take().is_some() {
                    let target = AutomationTarget::Param {
                        insert: id,
                        param: "solo".into(),
                    };
                    doc.player_mut()
                        .set_override(project, channel, &target, None);
                }
            }
        }
        self.raise = None;
        let open: Vec<InsertId> = self.editors.iter().map(|e| e.1).collect();
        self.plugin_params.retain(|id, _| open.contains(id));
    }

    /// Generic editor of a plugin insert; returns false once closed.
    fn plugin_editor(
        &mut self,
        ctx: &egui::Context,
        doc: &mut Document,
        channel: Channel,
        insert: &audiostrada_core::Insert,
        owner: &str,
    ) -> bool {
        let id = insert.id;
        let name = effect_name(doc, &insert.effect);
        let error = doc.player().plugin_error(id).map(str::to_owned);
        if !self.plugin_params.contains_key(&id)
            && let Some(plugin) = doc.player_mut().plugin(channel, id)
        {
            let params = plugin.params().into_iter().filter(|p| !p.hidden).collect();
            self.plugin_params.insert(id, params);
        }
        let params = self.plugin_params.get(&id).cloned().unwrap_or_default();
        let filter = self.param_filter.to_lowercase();
        let shown: Vec<&audiostrada_plugins::ParamDesc> = params
            .iter()
            .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
            .take(400)
            .collect();
        // Values and their text, read from the plugin before drawing.
        let mut rows = Vec::with_capacity(shown.len());
        let mut has_gui = false;
        let mut gui_open = false;
        let automated: Vec<Option<f32>> = shown
            .iter()
            .map(|p| {
                let lane = AutomationTarget::Param {
                    insert: id,
                    param: p.id.to_string(),
                };
                doc.automation_value(channel, &lane)
            })
            .collect();
        if let Some(plugin) = doc.player_mut().plugin(channel, id) {
            has_gui = plugin.has_gui();
            gui_open = plugin.gui_open();
            for (p, auto) in shown.iter().zip(&automated) {
                let value = auto
                    .map(f64::from)
                    .or_else(|| insert.params.get(&p.id.to_string()).map(|&v| f64::from(v)))
                    .or_else(|| plugin.param_value(p.id))
                    .unwrap_or(p.default);
                let text = plugin
                    .value_text(p.id, value)
                    .unwrap_or_else(|| format!("{value:.2}"));
                rows.push((*p, value, text));
            }
        }
        let mut toggle_gui = false;
        let title = format!("{owner}: {name}");
        let raise = self.raise == Some(id);
        let places = self.places;
        let open = editor_window(
            ctx,
            places,
            ("insert-editor", id.0),
            &title,
            [600.0, 520.0],
            raise,
            false,
            |ui| {
                if let Some(error) = &error {
                    ui.colored_label(
                        ERROR,
                        trf!("This plugin cannot be loaded: {error}", error = error),
                    );
                    return;
                }
                ui.horizontal(|ui| {
                    let mut bypass = insert.bypass;
                    if ui.checkbox(&mut bypass, tr("Bypass")).changed() {
                        doc.history_mut().edit(tr("Bypass insert"), |p| {
                            routing::set_bypass(p, channel, id, bypass)
                        });
                    }
                    let label = if gui_open {
                        tr("Close plugin window")
                    } else {
                        tr("Open plugin window")
                    };
                    if ui
                        .add_enabled(has_gui, egui::Button::new(label))
                        .on_disabled_hover_text(tr(
                            "This plugin has no window AudioStrada can open yet",
                        ))
                        .clicked()
                    {
                        toggle_gui = true;
                    }
                });
                let state = self.editor_states.entry(id).or_default();
                effect_editors::preset_bar(ui, doc, channel, insert, state);
                effect_editors::levels_bar(ui, doc, channel, insert, state);
                ui.add(
                    egui::TextEdit::singleline(&mut self.param_filter)
                        .hint_text(tr("Filter parameters"))
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new(("plugin-params", id.0))
                            .num_columns(3)
                            .spacing([10.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for (p, current, text) in &rows {
                                    // Wide enough to read, the whole name on hover.
                                    ui.scope(|ui| {
                                        ui.set_width(150.0);
                                        ui.add(egui::Label::new(&p.name).truncate())
                                    })
                                    .inner
                                    .on_hover_text(
                                        if p.module.is_empty() {
                                            p.name.clone()
                                        } else {
                                            format!("{}\n{}", p.name, p.module)
                                        },
                                    );
                                    let mut value = *current;
                                    let mut slider = egui::Slider::new(&mut value, p.min..=p.max)
                                        .show_value(false);
                                    if p.stepped {
                                        slider = slider.step_by(1.0);
                                    }
                                    let response = ui.add_enabled(!p.read_only, slider);
                                    if response.double_clicked() {
                                        value = p.default;
                                    }
                                    ui.monospace(text);
                                    let lane = AutomationTarget::Param {
                                        insert: id,
                                        param: p.id.to_string(),
                                    };
                                    let changed = (value != *current).then_some(value as f32);
                                    automated_control(
                                        doc,
                                        channel,
                                        &lane,
                                        changed,
                                        &response,
                                        |doc, value| {
                                            doc.history_mut().edit_merged(
                                                &format!("{name} {}", p.name),
                                                &format!("insert:{}:{}", id.0, p.id),
                                                |proj| {
                                                    routing::set_insert_param(
                                                        proj,
                                                        channel,
                                                        id,
                                                        &p.id.to_string(),
                                                        value,
                                                    )
                                                },
                                            );
                                        },
                                    );
                                    learn_menu(
                                        &response,
                                        doc,
                                        channel,
                                        &lane,
                                        (p.min as f32, p.max as f32),
                                    );
                                    ui.end_row();
                                }
                            });
                        if params.is_empty() {
                            ui.weak(tr("This plugin exposes no parameters."));
                        }
                    });
            },
        );
        if toggle_gui && let Some(plugin) = doc.player_mut().plugin(channel, id) {
            if plugin.gui_open() {
                plugin.close_gui();
            } else if let Err(e) = plugin.open_gui(&title) {
                self.error = Some(e);
            }
        }
        open
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use audiostrada_core::routing::{self, Channel};
    use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, vec2};

    use super::MixerView;
    use crate::document::{AppDirs, Document};

    #[test]
    fn parked_views_reopen_plugin_windows_and_forget_where_windows_opened() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let mut view = MixerView::new(Rc::clone(&doc));
        let other = MixerView::new(doc);
        assert_ne!(view.places, other.places, "each tab keeps its own places");
        let window = (Channel::Master, audiostrada_core::InsertId(3));
        view.park(vec![window]);
        view.park(vec![window]);
        assert_eq!(view.open_gui, [window]);
        assert!(view.parked);
    }

    #[test]
    fn labels_read_as_on_a_console() {
        assert_eq!(super::pan_label(0.004), "C");
        assert_eq!(super::pan_label(-0.4), "L: 40%");
        assert_eq!(super::pan_label(0.25), "R: 25%");
        assert_eq!(super::db_label(super::FADER_MIN_DB), "-∞");
        assert_eq!(super::db_label(3.0), "+3.0");
        assert_eq!(super::input_label(0, false), "In 1");
        assert_eq!(super::input_label(2, true), "In 3-4");
        assert_eq!(
            super::format_time(48_000 * 90 + 24_000, 48_000),
            "01:30.500"
        );
        assert_eq!(super::format_time(0, 0), "00:00.000");
    }

    #[test]
    fn strips_give_the_fader_its_share_first() {
        let tall = super::StripSizes::new(1000.0, 230.0);
        assert_eq!((tall.fx, tall.sends, tall.input), (150.0, Some(90.0), true));
        let short = super::StripSizes::new(300.0, 230.0);
        assert_eq!((short.fx, short.sends, short.input), (44.0, None, false));
        assert!(tall.close_to(&super::StripSizes::new(1000.4, 230.0)));
        assert!(!tall.close_to(&short));
    }

    thread_local! {
        static RECTS: RefCell<HashMap<String, Rect>> = RefCell::new(HashMap::new());
    }

    /// Where a widget was drawn, for the tests to point at it.
    pub(super) fn seen(name: String, rect: Rect) {
        RECTS.with(|r| r.borrow_mut().insert(name, rect));
    }

    pub(super) fn rect(name: &str) -> Rect {
        RECTS.with(|r| r.borrow()[name])
    }

    /// A frame of the rack's panel.
    pub(super) fn rack_frame(ctx: &egui::Context, view: &mut MixerView, events: Vec<Event>) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 400.0))),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| view.rack(ui))
            .drop_without_applying_deltas();
    }

    fn frame(ctx: &egui::Context, view: &mut MixerView, events: Vec<Event>) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            view.show(ui);
            view.windows(ui.ctx());
        })
        .drop_without_applying_deltas();
    }

    fn drag(ctx: &egui::Context, view: &mut MixerView, from: Pos2, to: Pos2, alt: bool) {
        let modifiers = if alt { Modifiers::ALT } else { Modifiers::NONE };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        };
        frame(ctx, view, vec![Event::PointerMoved(from)]);
        frame(ctx, view, vec![button(from, true)]);
        for t in [0.1, 0.5, 0.9, 1.0] {
            frame(ctx, view, vec![Event::PointerMoved(from.lerp(to, t))]);
        }
        frame(ctx, view, vec![button(to, false)]);
        frame(ctx, view, vec![]);
    }

    #[test]
    fn a_list_with_midi_learn_stays_open() {
        let mut doc = Document::new(AppDirs::default());
        let ctx = egui::Context::default();
        let mut button = Rect::NOTHING;
        let mut frame = |events: Vec<Event>, button: &mut Rect| {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let response = egui::ComboBox::from_id_salt("note")
                    .selected_text("1/8")
                    .show_ui(ui, |ui| {
                        for i in 0..12 {
                            ui.label(i.to_string());
                        }
                    })
                    .response;
                super::learn_menu(
                    &response,
                    &mut doc,
                    Channel::Master,
                    &audiostrada_core::automation::AutomationTarget::Gain,
                    (0.0, 1.0),
                );
                *button = response.rect;
            })
            .drop_without_applying_deltas();
        };
        frame(vec![], &mut button);
        let at = button.center();
        let click = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(vec![Event::PointerMoved(at)], &mut button);
        frame(vec![click(true)], &mut button);
        frame(vec![click(false)], &mut button);
        frame(vec![], &mut button);
        assert!(
            egui::Popup::is_any_open(&ctx),
            "the list opened and stays open"
        );
    }

    #[test]
    fn an_instrument_leads_its_track_in_place_of_the_last_one() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let mut track = None;
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            let t = p.add_track("Keys");
            routing::add_insert(p, Channel::Track(t), "eq", Default::default());
            track = Some(t);
        });
        let channel = Channel::Track(track.unwrap());
        let mut view = MixerView::new(Rc::clone(&doc));
        let effects = || {
            routing::inserts(doc.borrow().project(), channel)
                .iter()
                .map(|i| i.effect.clone())
                .collect::<Vec<_>>()
        };
        view.add_insert(&mut doc.borrow_mut(), channel, false, "synth", "Synth");
        assert_eq!(effects(), ["synth", "eq"], "first");
        let first = routing::inserts(doc.borrow().project(), channel)[0].id;
        view.add_insert(&mut doc.borrow_mut(), channel, false, "synth", "Synth");
        assert_eq!(effects(), ["synth", "eq"], "replaced, not added");
        assert_ne!(
            routing::inserts(doc.borrow().project(), channel)[0].id,
            first
        );
        view.add_insert(
            &mut doc.borrow_mut(),
            channel,
            false,
            "compressor",
            "Compressor",
        );
        assert_eq!(effects(), ["synth", "eq", "compressor"], "effects go last");
    }

    #[test]
    fn a_chain_adds_its_effects_with_their_settings() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut()
            .edit("Setup", |p| track = Some(p.add_track("Vox")));
        let channel = Channel::Track(track.unwrap());
        let vocal = crate::chains::list(None)
            .into_iter()
            .find(|c| c.name == "Vocal")
            .unwrap();
        let added = super::apply_chain(&mut doc, (channel, false), &vocal);
        let inserts = routing::inserts(doc.project(), channel);
        assert_eq!(added.len(), 4);
        assert_eq!(inserts[1].effect, "compressor");
        assert_eq!(inserts[1].params["ratio"], 3.0);
        assert_eq!(
            doc.history_mut().undo().as_deref(),
            Some("Add chain Vocal"),
            "one undo step"
        );
    }

    #[test]
    fn inserts_drag_to_another_strip_and_within_their_list() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let (mut a, mut b) = (None, None);
        let mut inserts = Vec::new();
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            let (ta, tb) = (p.add_track("A"), p.add_track("B"));
            for effect in ["eq", "compressor"] {
                inserts.extend(routing::add_insert(
                    p,
                    Channel::Track(ta),
                    effect,
                    Default::default(),
                ));
            }
            (a, b) = (Some(ta), Some(tb));
        });
        let (a, b) = (a.unwrap(), b.unwrap());
        let mut view = MixerView::new(Rc::clone(&doc));
        let ctx = egui::Context::default();
        frame(&ctx, &mut view, vec![]);
        frame(&ctx, &mut view, vec![]);
        let effects = |t| {
            routing::inserts(doc.borrow().project(), Channel::Track(t))
                .iter()
                .map(|i| i.effect.clone())
                .collect::<Vec<_>>()
        };
        // Onto the other strip: a copy.
        let eq = rect(&format!("insert {}", inserts[0].0)).center();
        drag(
            &ctx,
            &mut view,
            eq,
            rect(&format!("strip {}", b.0)).center(),
            false,
        );
        assert_eq!(effects(b), ["eq"]);
        assert_eq!(effects(a), ["eq", "compressor"]);
        // Below the compressor: reordered.
        let below = rect(&format!("insert {}", inserts[1].0)).center() + vec2(0.0, 6.0);
        drag(&ctx, &mut view, eq, below, false);
        assert_eq!(effects(a), ["compressor", "eq"]);
    }

    #[test]
    fn narrow_strips_line_up_on_both_sides_of_the_open_one() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let mut ids = Vec::new();
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            ids = ["A", "B", "C"].map(|n| p.add_track(n)).to_vec();
        });
        let mut view = MixerView::new(Rc::clone(&doc));
        view.compact = true;
        view.expanded = Some(ids[1]);
        let ctx = egui::Context::default();
        for _ in 0..4 {
            frame(&ctx, &mut view, vec![]);
        }
        let (before, after) = (
            rect(&format!("room {}", ids[0].0)),
            rect(&format!("room {}", ids[2].0)),
        );
        assert_eq!(before.bottom(), after.bottom(), "{before:?} {after:?}");
        // Not pushed down to the open strip's fader: its top is much taller.
        assert!(before.bottom() < 300.0, "{before:?}");
        // And their faders reach as low as the open strip's.
        let strips = [0, 1, 2].map(|i| {
            let key = if i == 1 { "strip" } else { "narrow" };
            rect(&format!("{key} {}", ids[i].0))
        });
        for narrow in [strips[0], strips[2]] {
            assert!(
                (narrow.bottom() - strips[1].bottom()).abs() <= 1.0,
                "{strips:?}"
            );
        }
    }

    #[test]
    fn a_compact_mixer_opens_the_strip_clicked() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let mut ids = Vec::new();
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            ids = ["A", "B"].map(|n| p.add_track(n)).to_vec();
        });
        let mut view = MixerView::new(Rc::clone(&doc));
        view.compact = true;
        let ctx = egui::Context::default();
        frame(&ctx, &mut view, vec![]);
        frame(&ctx, &mut view, vec![]);
        let click = |view: &mut MixerView, at: Pos2| {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            frame(&ctx, view, vec![Event::PointerMoved(at)]);
            frame(&ctx, view, vec![button(true)]);
            frame(&ctx, view, vec![button(false)]);
            frame(&ctx, view, vec![]);
        };
        let narrow = rect(&format!("narrow {}", ids[1].0));
        assert!(narrow.width() < super::STRIP_WIDTH);
        click(
            &mut view,
            egui::pos2(narrow.center().x, narrow.top() + 60.0),
        );
        assert_eq!(view.expanded, Some(ids[1]));
        assert!(doc.borrow().is_track_selected(ids[1]));
        // Opened full width; a click on it folds it back.
        let full = rect(&format!("strip {}", ids[1].0));
        assert!(full.width() >= super::STRIP_WIDTH);
        click(&mut view, egui::pos2(full.center().x, full.top() + 8.0));
        assert_eq!(view.expanded, None);
    }

    #[test]
    fn tracks_drag_to_a_new_place() {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let mut ids = Vec::new();
        doc.borrow_mut().history_mut().edit("Setup", |p| {
            ids = ["A", "B", "C"].map(|n| p.add_track(n)).to_vec();
        });
        let mut view = MixerView::new(Rc::clone(&doc));
        let ctx = egui::Context::default();
        frame(&ctx, &mut view, vec![]);
        frame(&ctx, &mut view, vec![]);
        let names = || {
            doc.borrow()
                .project()
                .tracks
                .iter()
                .map(|t| t.name.clone())
                .collect::<Vec<_>>()
        };
        let plate = rect(&format!("plate {}", ids[0].0)).center();
        let c = rect(&format!("strip {}", ids[2].0));
        drag(
            &ctx,
            &mut view,
            plate,
            c.center() + vec2(c.width() / 4.0, 0.0),
            false,
        );
        assert_eq!(names(), ["B", "C", "A"]);
        // From the strip's background as well.
        let b = rect(&format!("strip {}", ids[1].0));
        let a = rect(&format!("strip {}", ids[0].0));
        drag(
            &ctx,
            &mut view,
            egui::pos2(b.center().x, b.top() + 8.0),
            a.center() + vec2(a.width() / 4.0, 0.0),
            false,
        );
        assert_eq!(names(), ["C", "A", "B"]);
    }
}
