//! Effect browser: built-in effects and installed plugins by family, saved effect chains, a
//! search field and vendor and format filters. Opens when an insert is added.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use audiostrada_core::routing::{self, Channel};
use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Ui, pos2, vec2};

use crate::document::Document;
use crate::effect_frame::{self, Face, Glyph};
use crate::i18n::tr;
use crate::theme;

/// Where the chosen effect goes: channel, and the master's monitoring section.
pub type Target = (Channel, bool);

#[derive(Debug, Default)]
pub struct EffectBrowser {
    target: Option<Target>,
    filter: String,
    /// Family shown; `None` for all, `Some("Plugins")` for installed plugins.
    family: Option<String>,
    focus_search: bool,
    chains: Vec<crate::chains::Chain>,
    /// Shown vendor and format, `None` for all.
    vendor: Option<String>,
    format: Option<String>,
    /// Plugin families chosen by the user, by plugin key.
    plugin_families: BTreeMap<String, String>,
    /// Entries for the current target, gathered by the window that opened the browser.
    all: Vec<Entry>,
    /// The choice made in the browser's window, for the window that opened it.
    result: Option<(Target, String, String)>,
    /// The window was hidden when it closed, and must be shown again if it opens again.
    hidden: bool,
    /// Opened again while open: it comes to the front, wherever it was.
    raise: bool,
    scan: Option<Scan>,
    /// The entry chosen with a click or the arrows, added with Enter or Add.
    selected: Option<String>,
    /// The chosen entry was moved to with the keys: it scrolls into view.
    scroll: bool,
    /// Cards per row, for Up and Down.
    columns: usize,
    /// Where the effect goes, under the title.
    owner: String,
    /// Presets of the chosen entry, counted once.
    presets_of: Option<(String, usize)>,
}

/// The background plugin scan: what it says, files scanned and to scan, the current one.
#[derive(Debug)]
struct Scan {
    status: String,
    counts: Option<(usize, usize)>,
    current: String,
}

/// The browser, shared with its own window.
pub type Shared = Rc<RefCell<EffectBrowser>>;

const VIEWPORT: &str = "effect-browser";

/// Shows the browser in a window of its own, above every other; returns where to insert and
/// the chosen effect key and name once it is chosen. The window runs on its own, so that it
/// closes at once whatever the window that opened it is doing.
pub fn show(
    browser: &Shared,
    ctx: &egui::Context,
    doc: &Document,
) -> Option<(Target, String, String)> {
    let mut b = browser.borrow_mut();
    if let Some(result) = b.result.take() {
        return Some(result);
    }
    let target = b.target?;
    // Instruments lead a track; the master has none.
    let instruments = matches!(target, (Channel::Track(_), false));
    b.all = entries(doc, &b.chains, &b.plugin_families, instruments);
    b.owner = crate::mixer::owner_name(doc.project(), target.0).to_owned();
    if target.1 {
        b.owner = format!("{} · {}", b.owner, tr("Monitoring"));
    }
    b.scan = doc.plugin_scan().map(|s| Scan {
        status: scan_status(s),
        counts: s.counts(),
        current: s.current(),
    });
    let id = egui::ViewportId::from_hash_of(VIEWPORT);
    if std::mem::take(&mut b.hidden) {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
    }
    if std::mem::take(&mut b.raise) {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
    }
    drop(b);
    let builder = crate::floating_builder(
        ctx,
        id,
        crate::Floating::Browser,
        egui::ViewportBuilder::default()
            .with_title(format!("{} — AudioStrada", tr("Add an effect")))
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([640.0, 400.0]),
    );
    let parent = ctx.viewport_id();
    let shared = crate::MainThread(Rc::clone(browser));
    ctx.show_viewport_deferred(id, builder, move |ui, _| {
        if let Ok(mut b) = shared.get().try_borrow_mut()
            && let Some(target) = b.target
        {
            b.window(ui, target, parent);
        }
    });
    None
}

/// What a background plugin scan is doing, in a few words.
pub fn scan_status(scan: &audiostrada_plugins::scan::Progress) -> String {
    match scan.counts() {
        None => tr("Looking for plugins…").to_owned(),
        Some((done, total)) => crate::trf!(
            "Scanning plugins: {done} of {total}",
            done = done.min(total),
            total = total
        ),
    }
}

/// The plugin scan running, above the list: the plugins it finds show when it ends.
fn scan_banner(ui: &mut Ui, scan: &Scan) {
    let p = theme::current(ui);
    let full = ui.max_rect();
    let rect = Rect::from_min_max(
        pos2(full.left() + 14.0, full.top() + 10.0),
        pos2(full.right() - 14.0, full.bottom() - 4.0),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect(
        rect,
        6.0,
        theme::mix(
            effect_frame::field_fill(&p),
            p.accent,
            if p.dark { 0.25 } else { 0.12 },
        ),
        egui::Stroke::new(1.0, p.accent),
        egui::StrokeKind::Inside,
    );
    let cy = rect.center().y;
    let light = if p.dark {
        theme::mix(p.accent, Color32::WHITE, 0.7)
    } else {
        theme::mix(p.accent, Color32::BLACK, 0.3)
    };
    ui.put(
        Rect::from_center_size(pos2(rect.left() + 18.0, cy), vec2(14.0, 14.0)),
        egui::Spinner::new().size(13.0).color(light),
    );
    let mut x = painter
        .text(
            pos2(rect.left() + 34.0, cy),
            Align2::LEFT_CENTER,
            &scan.status,
            theme::strong(12.0),
            p.text,
        )
        .right()
        + 10.0;
    if let Some((done, total)) = scan.counts
        && total > 0
    {
        let bar = Rect::from_min_size(pos2(x, cy - 3.0), vec2(160.0, 6.0));
        painter.rect_filled(bar, 3.0, effect_frame::field_fill(&p));
        painter.rect_filled(
            bar.with_max_x(bar.left() + bar.width() * (done as f32 / total as f32).min(1.0)),
            3.0,
            p.accent,
        );
        x = bar.right() + 10.0;
    }
    painter.text(
        pos2(x, cy),
        Align2::LEFT_CENTER,
        tr("New plugins show here once the scan is done; the ones already known can be used now."),
        egui::FontId::proportional(12.0),
        light,
    );
    if !scan.current.is_empty() {
        ui.interact(rect, ui.scope_id().with("scan"), Sense::hover())
            .on_hover_text(&scan.current);
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
}

/// What each family holds, under its name.
fn family_summary(family: Option<&str>) -> &'static str {
    tr(summary_of(family))
}

/// [`family_summary`] in English, which the search also reads.
fn summary_of(family: Option<&str>) -> &'static str {
    match family {
        None => "every effect and plugin",
        Some(FAVORITES) => "the effects you starred",
        Some(RECENT) => "the effects added lately",
        Some("Dynamics") => "compressors, gates, limiters, de-essers",
        Some("EQ & Filter") => "equalizers and filters",
        Some("Space") => "reverbs and delays",
        Some("Modulation") => "chorus, flanger, phaser, pitch",
        Some("Color") => "saturation, distortion, tape",
        Some("Stereo") => "width, panning, imaging",
        Some("Amp & Cab") => "guitar and bass amps, cabinets",
        Some("Utility") => "gain, meters, tuners, analyzers",
        Some(INSTRUMENT) => "synths, samplers and drum machines",
        Some(CHAINS) => "your effect chains, saved from the rack",
        Some(PLUGINS) => "the plugins installed on this computer",
        _ => "plugins of another kind",
    }
}

/// Lower case without accents, for a search that finds "réverbe" from "reverbe".
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ÿ' => 'y',
            c => c,
        })
        .collect()
}

/// What the search reads of an entry: its texts in English and translated, its family and
/// what the family holds.
fn search_words(family: &'static str, texts: &[&str]) -> String {
    let summary = summary_of(Some(family));
    let mut all: Vec<&str> = texts.to_vec();
    all.extend([family, tr(family), summary, tr(summary)]);
    fold(&all.join(" "))
}

/// One entry of the browser.
#[derive(Clone, Debug)]
struct Entry {
    key: String,
    name: String,
    /// Family key (an English name, translated when shown).
    family: &'static str,
    detail: String,
    /// A chain of the user's, which can be deleted.
    removable: bool,
    /// A third-party plugin, whose family can be changed.
    plugin: bool,
    /// "AudioStrada" for built-in effects, empty for chains.
    vendor: String,
    format: &'static str,
    /// What the search reads, folded.
    words: String,
}

const PLUGINS: &str = "Plugins";
const CHAINS: &str = "Chains";
/// Key prefix of a chain chosen in the browser, followed by its name.
pub const CHAIN_KEY: &str = "chain:";
const AUDIOSTRADA: &str = "AudioStrada";
const INSTRUMENT: &str = "Instrument";

/// Views of the left column besides the families.
const FAVORITES: &str = "Favorites";
const RECENT: &str = "Recent";

/// What a card reports.
struct CardOut {
    response: egui::Response,
    vendor: bool,
    star: bool,
}

impl EffectBrowser {
    pub fn open(&mut self, target: Target) {
        self.raise = true;
        self.target = Some(target);
        self.filter.clear();
        self.focus_search = true;
        self.selected = None;
        self.chains = crate::chains::list(crate::chains::root().as_deref());
        self.plugin_families = crate::plugin_categories::load();
    }

    /// Opens the browser on the instruments, to put one at the head of a track.
    pub fn open_instruments(&mut self, target: Target) {
        self.open(target);
        self.family = Some(INSTRUMENT.to_owned());
    }

    /// The entries of the view chosen at the left, or those matching the search.
    fn shown<'a>(&self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        let filter = fold(&self.filter);
        if !filter.trim().is_empty() {
            return entries
                .iter()
                .filter(|e| filter.split_whitespace().all(|w| e.words.contains(w)))
                .collect();
        }
        match self.family.as_deref() {
            Some(RECENT) => crate::favorites::with(|f| {
                f.recent
                    .iter()
                    .filter_map(|k| entries.iter().find(|e| &e.key == k))
                    .collect()
            }),
            Some(FAVORITES) => crate::favorites::with(|f| {
                entries
                    .iter()
                    .filter(|e| f.effects.contains(&e.key))
                    .collect()
            }),
            family => entries.iter().filter(|e| in_family(e, family)).collect(),
        }
    }

    /// Draws the browser in its window: on a choice or when closed, leaves the result for
    /// the window that opened it, hides at once and has that window draw again.
    fn window(&mut self, ui: &mut Ui, target: Target, parent: egui::ViewportId) {
        let p = theme::current(ui);
        let vendors = choices(self.all.iter().map(|e| e.vendor.as_str()));
        let formats = choices(self.all.iter().map(|e| e.format));
        // Families count what the vendor and format filters leave.
        let entries: Vec<Entry> = self
            .all
            .iter()
            .filter(|e| self.vendor.as_ref().is_none_or(|v| e.vendor == *v))
            .filter(|e| self.format.as_ref().is_none_or(|f| e.format == f))
            .cloned()
            .collect();
        let shown: Vec<String> = self.shown(&entries).iter().map(|e| e.key.clone()).collect();
        let mut chosen: Option<String> = None;
        let mut open = true;
        // Keys: arrows choose, Enter adds, / searches, F stars, Esc closes.
        let typing = ui.ctx().text_edit_focused();
        let (escape, enter, star, slash, moves) = ui.input_mut(|i| {
            let mut moves = Vec::new();
            for (key, step) in [
                (Key::ArrowUp, -(self.columns as isize)),
                (Key::ArrowDown, self.columns as isize),
                (Key::ArrowLeft, -1),
                (Key::ArrowRight, 1),
            ] {
                if (!typing || matches!(key, Key::ArrowUp | Key::ArrowDown))
                    && i.consume_key(egui::Modifiers::NONE, key)
                {
                    moves.push(step);
                }
            }
            let slash = !typing && {
                let before = i.events.len();
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t == "/"));
                i.events.len() != before
            };
            (
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                !typing && i.consume_key(egui::Modifiers::NONE, Key::F),
                slash,
                moves,
            )
        });
        if escape {
            open = false;
        }
        if slash {
            self.focus_search = true;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|k| shown.iter().position(|s| s == k));
        if !shown.is_empty() {
            for step in moves {
                let k = at.map_or(0, |a| {
                    (a as isize + step).clamp(0, shown.len() as isize - 1) as usize
                });
                self.selected = Some(shown[k].clone());
                self.scroll = true;
            }
        }
        if enter {
            chosen = at
                .map(|a| shown[a].clone())
                .or_else(|| shown.first().cloned());
        }
        if star && let Some(key) = &self.selected {
            crate::favorites::change(|f| f.toggle_effect(key));
        }
        egui::Panel::top("browser-top")
            .frame(egui::Frame::NONE)
            .exact_size(58.0)
            .show_separator_line(false)
            .show(ui, |ui| self.top_bar(ui, &vendors, &formats, &mut open));
        let selected = self
            .selected
            .as_ref()
            .and_then(|k| entries.iter().find(|e| &e.key == k))
            .cloned();
        egui::Panel::bottom("browser-bottom")
            .frame(egui::Frame::NONE)
            .exact_size(49.0)
            .show_separator_line(false)
            .show(ui, |ui| {
                if self.bottom_bar(ui, selected.as_ref()) {
                    chosen = selected.as_ref().map(|e| e.key.clone());
                }
            });
        let body = if p.dark { p.base } else { p.panel };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(body))
            .show(ui, |ui| {
                if let Some(scan) = &self.scan {
                    egui::Panel::top("browser-scan")
                        .frame(egui::Frame::NONE)
                        .exact_size(50.0)
                        .show_separator_line(false)
                        .show(ui, |ui| scan_banner(ui, scan));
                }
                egui::Panel::left("browser-side")
                    .frame(egui::Frame::NONE)
                    .exact_size(210.0)
                    .resizable(false)
                    .show_separator_line(false)
                    .show(ui, |ui| self.sidebar(ui, &entries));
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 10,
                        bottom: 0,
                    }))
                    .show(ui, |ui| {
                        if let Some(key) = self.cards(ui, &entries) {
                            chosen = Some(key);
                        }
                    });
            });
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            open = false;
        }
        if chosen.is_some() || !open {
            let chosen = chosen
                .and_then(|k| self.all.iter().find(|e| e.key == k))
                .map(|e| (e.key.clone(), e.name.clone()));
            if let Some((key, _)) = &chosen {
                crate::favorites::change(|f| f.used(key));
            }
            self.target = None;
            self.result = chosen.map(|(key, name)| (target, key, name));
            self.hidden = true;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Chains and families changed here show in the entries the opener gathers.
        ui.ctx().request_repaint_of(parent);
    }

    /// Logo and title, the search in the middle, the format and vendor filters, Close.
    fn top_bar(&mut self, ui: &mut Ui, vendors: &[String], formats: &[String], open: &mut bool) {
        let p = theme::current(ui);
        let r = effect_frame::radius(&p);
        let painter = ui.painter().clone();
        let bar = effect_frame::paint_top_band(&painter, &p, ui.max_rect(), p.accent);
        ui.allocate_rect(ui.max_rect(), Sense::hover());
        let cy = bar.center().y;
        let id = ui.scope_id();
        let mut x = bar.right() - 14.0;
        let close = Rect::from_min_max(pos2(x - 30.0, cy - 15.0), pos2(x, cy + 15.0));
        if effect_frame::glyph_button(ui, close, id.with("close"), Glyph::Close, None, true)
            .on_hover_text(tr("Close"))
            .clicked()
        {
            *open = false;
        }
        x = close.left() - 10.0;
        // The vendor, from a menu.
        let font = FontId::proportional(12.0);
        let text = self
            .vendor
            .clone()
            .unwrap_or_else(|| tr("Vendor").to_owned());
        let width = painter
            .layout_no_wrap(text.clone(), font.clone(), p.text)
            .size()
            .x
            .min(150.0)
            + 34.0;
        let rect = Rect::from_min_max(pos2(x - width, cy - 15.0), pos2(x, cy + 15.0));
        let response = effect_frame::button(ui, rect, id.with("vendor"), true);
        effect_frame::paint_face(ui, rect, r, &response, Face::Plain);
        painter
            .with_clip_rect(
                rect.shrink2(vec2(10.0, 0.0))
                    .with_max_x(rect.right() - 22.0),
            )
            .text(
                pos2(rect.left() + 10.0, cy),
                Align2::LEFT_CENTER,
                &text,
                font.clone(),
                theme::mix(p.text, p.weak, 0.2),
            );
        effect_frame::paint_glyph(
            &painter,
            Rect::from_center_size(pos2(rect.right() - 13.0, cy), vec2(10.0, 10.0)),
            Glyph::Down,
            p.weak,
        );
        egui::Popup::menu(&response).show(|ui| {
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    if ui
                        .selectable_label(self.vendor.is_none(), tr("All"))
                        .clicked()
                    {
                        self.vendor = None;
                        ui.close();
                    }
                    for vendor in vendors {
                        if ui
                            .selectable_label(self.vendor.as_ref() == Some(vendor), vendor)
                            .clicked()
                        {
                            self.vendor = Some(vendor.clone());
                            ui.close();
                        }
                    }
                });
        });
        x = rect.left() - 10.0;
        // The formats, side by side.
        let segments: Vec<(Option<&String>, &str)> = std::iter::once((None, tr("All")))
            .chain(formats.iter().map(|f| {
                (
                    Some(f),
                    if f == AUDIOSTRADA {
                        "Strada"
                    } else {
                        f.as_str()
                    },
                )
            }))
            .collect();
        let widths: Vec<f32> = segments
            .iter()
            .map(|(_, label)| {
                painter
                    .layout_no_wrap((*label).to_owned(), theme::strong(12.0), p.text)
                    .size()
                    .x
                    + 22.0
            })
            .collect();
        let mut sx = x - widths.iter().sum::<f32>();
        let last = segments.len() - 1;
        for (k, ((value, label), w)) in segments.iter().zip(&widths).enumerate() {
            let rect = Rect::from_min_size(pos2(sx, cy - 15.0), vec2(*w, 30.0));
            let on = self.format.as_ref() == *value;
            let corners = egui::CornerRadius {
                nw: if k == 0 { r as u8 } else { 0 },
                sw: if k == 0 { r as u8 } else { 0 },
                ne: if k == last { r as u8 } else { 0 },
                se: if k == last { r as u8 } else { 0 },
            };
            let face = if on { Face::Lit(p.accent) } else { Face::Plain };
            if effect_frame::text_button(ui, rect, id.with(("format", k)), label, face, corners)
                .clicked()
            {
                self.format = value.cloned();
            }
            sx = rect.right();
        }
        x -= widths.iter().sum::<f32>() + 16.0;
        // The title, and where the effect goes.
        let detail = |column: &egui::Painter, at: Pos2| {
            column.text(
                at,
                Align2::LEFT_TOP,
                &self.owner,
                FontId::proportional(10.5),
                p.weak,
            );
        };
        let left = bar.left() + 56.0;
        let name_width = 190.0f32.min((x - left - 140.0).max(0.0));
        effect_frame::paint_title(
            &painter,
            &p,
            bar,
            name_width,
            tr("Add an effect"),
            &detail,
            false,
        );
        // The search.
        let start = left + name_width + 16.0;
        let room = x - start;
        let width = room.clamp(120.0, 420.0);
        let field = Rect::from_min_size(
            pos2(start + ((room - width) / 2.0).max(0.0), cy - 16.0),
            vec2(width, 32.0),
        );
        let search_id = id.with("search");
        let focused = ui.ctx().memory(|m| m.has_focus(search_id));
        painter.rect(
            field,
            r,
            effect_frame::field_fill(&p),
            egui::Stroke::new(
                1.0,
                if focused {
                    p.accent
                } else {
                    effect_frame::button_edge(&p)
                },
            ),
            egui::StrokeKind::Inside,
        );
        effect_frame::paint_glyph(
            &painter,
            Rect::from_center_size(field.left_center() + vec2(17.0, 0.0), vec2(14.0, 14.0)),
            Glyph::Search,
            p.weak,
        );
        let text_rect = Rect::from_min_max(
            pos2(field.left() + 30.0, field.top() + 1.0),
            pos2(field.right() - 8.0, field.bottom() - 1.0),
        );
        let search = ui.put(
            text_rect,
            egui::TextEdit::singleline(&mut self.filter)
                .id(search_id)
                .frame(egui::Frame::NONE)
                .hint_text(tr("Search effects and plugins"))
                .align(Align2::LEFT_CENTER)
                .desired_width(text_rect.width()),
        );
        if std::mem::take(&mut self.focus_search) {
            search.request_focus();
        }
    }

    /// All, favorites, recent, the families, the chains.
    fn sidebar(&mut self, ui: &mut Ui, entries: &[Entry]) {
        let p = theme::current(ui);
        let full = ui.max_rect();
        ui.painter().line_segment(
            [
                pos2(full.right() - 0.5, full.top()),
                pos2(full.right() - 0.5, full.bottom()),
            ],
            egui::Stroke::new(1.0, effect_frame::line_edge(&p)),
        );
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
            pos2(full.left() + 10.0, full.top() + 6.0),
            pos2(full.right() - 10.0, full.bottom()),
        )));
        let favorites = crate::favorites::with(Clone::clone);
        let count = |family: Option<&str>| match family {
            Some(FAVORITES) => entries
                .iter()
                .filter(|e| favorites.effects.contains(&e.key))
                .count(),
            Some(RECENT) => favorites
                .recent
                .iter()
                .filter(|k| entries.iter().any(|e| &e.key == *k))
                .count(),
            family => entries.iter().filter(|e| in_family(e, family)).count(),
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut inner, |ui| {
                let mut row = |ui: &mut Ui, family: Option<&str>, label: &str, mark: Mark| {
                    let n = count(family);
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    let on = self.filter.is_empty() && self.family.as_deref() == family;
                    let r = effect_frame::radius(&p);
                    if on {
                        ui.painter()
                            .rect_filled(rect, r, effect_frame::chosen_fill(&p));
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, r, p.hover);
                    }
                    let mut x = rect.left() + 10.0;
                    match mark {
                        Mark::Star => {
                            effect_frame::paint_glyph(
                                ui.painter(),
                                Rect::from_center_size(
                                    pos2(x + 6.0, rect.center().y),
                                    vec2(12.0, 12.0),
                                ),
                                Glyph::Star(true),
                                effect_frame::star_color(&p),
                            );
                            x += 19.0;
                        }
                        Mark::Dot(color) => {
                            ui.painter()
                                .circle_filled(pos2(x + 4.0, rect.center().y), 4.0, color);
                            x += 16.0;
                        }
                        Mark::None => {}
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
                        if on {
                            effect_frame::chosen_count(&p)
                        } else {
                            p.weak
                        },
                    );
                    if response.clicked() {
                        self.family = family.map(str::to_owned);
                        self.filter.clear();
                    }
                };
                row(ui, None, tr("All"), Mark::None);
                row(ui, Some(FAVORITES), tr("Favorites"), Mark::Star);
                row(ui, Some(RECENT), tr("Recent"), Mark::None);
                ui.add_space(4.0);
                effect_frame::section_title(ui, tr("Families"));
                for family in audiostrada_dsp::CATEGORIES
                    .iter()
                    .copied()
                    .chain([crate::plugin_categories::OTHER])
                {
                    if count(Some(family)) > 0 {
                        row(
                            ui,
                            Some(family),
                            tr(family),
                            Mark::Dot(family_color(family)),
                        );
                    }
                }
                ui.add_space(4.0);
                effect_frame::section_title(ui, tr("Mine"));
                row(
                    ui,
                    Some(CHAINS),
                    tr(CHAINS),
                    Mark::Dot(family_color(CHAINS)),
                );
            });
    }

    /// The title of the view, then its entries as cards; returns the key of the one to add.
    fn cards(&mut self, ui: &mut Ui, entries: &[Entry]) -> Option<String> {
        let p = theme::current(ui);
        let shown = self.shown(entries);
        let (title, summary) = if self.filter.is_empty() {
            let family = self.family.as_deref();
            (family.map_or(tr("All"), tr_family), family_summary(family))
        } else {
            (tr("Search"), "")
        };
        let count = if shown.len() == 1 {
            tr("1 effect").to_owned()
        } else {
            crate::trf!("{count} effects", count = shown.len())
        };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let end = ui
            .painter()
            .text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                title,
                theme::strong(15.0),
                p.text,
            )
            .right();
        let line = if summary.is_empty() {
            count
        } else {
            format!("{count} · {summary}")
        };
        ui.painter().text(
            pos2(end + 10.0, rect.center().y + 1.0),
            Align2::LEFT_CENTER,
            line,
            FontId::proportional(11.5),
            p.weak,
        );
        ui.add_space(8.0);
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.add_space(8.0);
                    ui.weak(match self.family.as_deref() {
                        Some(FAVORITES) if self.filter.is_empty() => {
                            tr("No favorites yet: star an effect, or press F on it.")
                        }
                        _ => {
                            tr("Nothing matches. Plugin folders are set in Preferences › Plugins.")
                        }
                    });
                }
                let width = ui.available_width();
                let columns = ((width + 10.0) / 290.0).floor().max(1.0) as usize;
                self.columns = columns;
                let card_width = (width - 10.0 * (columns - 1) as f32) / columns as f32;
                let mut removed = None;
                let mut refiled = None;
                let scroll = std::mem::take(&mut self.scroll);
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        for entry in row {
                            let selected = self.selected.as_ref() == Some(&entry.key);
                            let out = card(ui, entry, card_width, selected);
                            if selected && scroll {
                                ui.scroll_to_rect(out.response.rect, None);
                            }
                            if out.star {
                                crate::favorites::change(|f| f.toggle_effect(&entry.key));
                            } else if out.vendor {
                                self.vendor = Some(entry.vendor.clone());
                            } else if out.response.double_clicked() {
                                chosen = Some(entry.key.clone());
                            } else if out.response.clicked() {
                                self.selected = Some(entry.key.clone());
                            }
                            if entry.removable {
                                out.response.context_menu(|ui| {
                                    if ui.button(tr("Delete chain")).clicked() {
                                        removed = Some(entry.key.clone());
                                        ui.close();
                                    }
                                });
                            } else if entry.plugin && entry.family != INSTRUMENT {
                                out.response.context_menu(|ui| {
                                    if let Some(family) = family_menu(ui, entry) {
                                        refiled = Some((entry.key.clone(), family));
                                    }
                                });
                            }
                        }
                    });
                    ui.add_space(10.0);
                }
                if let Some((key, family)) = refiled {
                    match family {
                        Some(f) => self.plugin_families.insert(key, f.to_owned()),
                        None => self.plugin_families.remove(&key),
                    };
                    let _ = crate::plugin_categories::save(&self.plugin_families);
                }
                if let (Some(key), Some(root)) = (removed, crate::chains::root()) {
                    let name = key.strip_prefix(CHAIN_KEY).unwrap_or(&key);
                    let _ = crate::chains::remove(&root, name);
                    self.chains = crate::chains::list(Some(&root));
                }
            });
        chosen
    }

    /// The chosen entry and Add; returns whether Add was clicked.
    fn bottom_bar(&mut self, ui: &mut Ui, selected: Option<&Entry>) -> bool {
        let p = theme::current(ui);
        let painter = ui.painter().clone();
        let full = ui.max_rect();
        effect_frame::paint_bottom_band(&painter, &p, full);
        ui.allocate_rect(full, Sense::hover());
        let cy = full.center().y + 0.5;
        let left = full.left() + 14.0;
        let text = painter.with_clip_rect(Rect::from_min_max(
            pos2(left, full.top()),
            pos2(left + 300.0, full.bottom()),
        ));
        match selected {
            Some(entry) => {
                text.text(
                    pos2(left, cy - 1.0),
                    Align2::LEFT_BOTTOM,
                    &entry.name,
                    theme::strong(13.0),
                    p.text,
                );
                if self
                    .presets_of
                    .as_ref()
                    .is_none_or(|(k, _)| *k != entry.key)
                {
                    let n = if entry.family == CHAINS {
                        0
                    } else {
                        crate::presets::list(crate::presets::root().as_deref(), &entry.key).len()
                    };
                    self.presets_of = Some((entry.key.clone(), n));
                }
                let presets = self.presets_of.as_ref().map_or(0, |(_, n)| *n);
                let mut parts = vec![tr_family(entry.family).to_owned()];
                if !entry.vendor.is_empty() {
                    parts.push(entry.vendor.clone());
                }
                match presets {
                    0 => {}
                    1 => parts.push(tr("1 preset").to_owned()),
                    n => parts.push(crate::trf!("{count} presets", count = n)),
                }
                text.text(
                    pos2(left, cy + 2.0),
                    Align2::LEFT_TOP,
                    parts.join(" · "),
                    FontId::proportional(10.5),
                    p.weak,
                );
            }
            None => {
                text.text(
                    pos2(left, cy),
                    Align2::LEFT_CENTER,
                    tr("Choose an effect, then Add, or double-click it"),
                    FontId::proportional(12.0),
                    p.weak,
                );
            }
        }
        // Add, at the right.
        let label = tr("Add");
        let width = painter
            .layout_no_wrap(label.to_owned(), theme::strong(13.0), p.text)
            .size()
            .x
            + 36.0;
        let add = Rect::from_min_max(
            pos2(full.right() - 14.0 - width, cy - 16.0),
            pos2(full.right() - 14.0, cy + 16.0),
        );
        let enabled = selected.is_some();
        let response = effect_frame::button(ui, add, ui.scope_id().with("add"), enabled);
        let mut ink = painter.clone();
        if !enabled {
            ink.multiply_opacity(0.45);
        }
        ink.rect_filled(
            add,
            effect_frame::radius(&p),
            if response.hovered() {
                theme::mix(p.accent, Color32::WHITE, 0.1)
            } else {
                p.accent
            },
        );
        ink.text(
            add.center(),
            Align2::CENTER_CENTER,
            label,
            theme::strong(13.0),
            theme::text_on(p.accent),
        );
        // The keys, between.
        let hints: [(&[&str], &str); 4] = [
            (&["↑", "↓", "←", "→"], tr("browse")),
            (&["/"], tr("search")),
            (&["F"], tr("favorite")),
            (&[tr("Esc")], tr("close")),
        ];
        let width = effect_frame::key_hints_width(&painter, &p, &hints);
        let room = Rect::from_min_max(
            pos2(left + 300.0, full.top()),
            pos2(add.left() - 16.0, full.bottom()),
        );
        if room.width() >= width {
            effect_frame::key_hints(
                &painter,
                &p,
                pos2(room.center().x - width / 2.0, cy),
                &hints,
            );
        }
        response.clicked()
    }
}

/// What a row of the left column shows before its name.
#[derive(Clone, Copy)]
enum Mark {
    None,
    Star,
    Dot(Color32),
}

/// A family's name, as shown.
fn tr_family(family: &str) -> &'static str {
    match family {
        FAVORITES => tr("Favorites"),
        RECENT => tr("Recent"),
        CHAINS => tr(CHAINS),
        PLUGINS => tr(PLUGINS),
        INSTRUMENT => tr(INSTRUMENT),
        f => family_key(f).map_or(tr(crate::plugin_categories::OTHER), tr),
    }
}

/// A card: the family's line, the name and format, the vendor (a plugin's is clickable, to
/// show only its plugins), what it does, a star for the favorites.
fn card(ui: &mut Ui, entry: &Entry, width: f32, selected: bool) -> CardOut {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 118.0), Sense::click());
    let painter = ui.painter().clone();
    let r = 8.0;
    let fill = if p.dark { p.panel } else { Color32::WHITE };
    let edge = if selected {
        p.accent
    } else if response.hovered() {
        theme::mix(effect_frame::line_edge(&p), p.accent, 0.5)
    } else {
        effect_frame::line_edge(&p)
    };
    painter.rect(
        rect,
        r,
        if response.hovered() && !selected {
            theme::mix(fill, p.hover, 0.4)
        } else {
            fill
        },
        egui::Stroke::new(if selected { 2.0 } else { 1.0 }, edge),
        egui::StrokeKind::Inside,
    );
    let color = family_color(entry.family);
    painter.with_clip_rect(rect.shrink(1.0)).rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)),
        egui::CornerRadius {
            nw: r as u8,
            ne: r as u8,
            sw: 0,
            se: 0,
        },
        color,
    );
    let inner = rect.shrink2(vec2(12.0, 0.0));
    // The format, at the right of the name.
    let (badge, strada) = if entry.family == CHAINS {
        (tr("chain").to_uppercase(), true)
    } else if entry.format == AUDIOSTRADA {
        ("STRADA".to_owned(), true)
    } else {
        (entry.format.to_owned(), false)
    };
    let galley = painter.layout_no_wrap(badge, theme::strong(9.5), p.weak);
    let badge_rect = Rect::from_min_size(
        pos2(inner.right() - galley.size().x - 12.0, rect.top() + 13.0),
        vec2(galley.size().x + 12.0, galley.size().y + 4.0),
    );
    let (badge_fill, badge_ink) = if strada {
        if p.dark {
            (
                theme::mix(effect_frame::field_fill(&p), p.accent, 0.3),
                theme::mix(p.accent, Color32::WHITE, 0.7),
            )
        } else {
            (theme::mix(Color32::WHITE, p.accent, 0.14), p.accent)
        }
    } else {
        (p.surface, theme::mix(p.text, p.weak, 0.4))
    };
    painter.rect_filled(badge_rect, 4.0, badge_fill);
    painter.galley(badge_rect.center() - galley.size() / 2.0, galley, badge_ink);
    painter
        .with_clip_rect(inner.with_max_x(badge_rect.left() - 8.0))
        .text(
            pos2(inner.left(), rect.top() + 12.0),
            Align2::LEFT_TOP,
            &entry.name,
            theme::strong(14.0),
            p.text,
        );
    // The vendor.
    let mut vendor = false;
    let vendor_text = if entry.family == CHAINS && entry.removable {
        tr("My chains").to_owned()
    } else if entry.family == CHAINS {
        AUDIOSTRADA.to_owned()
    } else {
        entry.vendor.clone()
    };
    let link = painter.layout_no_wrap(
        vendor_text,
        FontId::proportional(10.5),
        if entry.plugin { p.accent } else { p.weak },
    );
    let link_rect = Rect::from_min_size(pos2(inner.left(), rect.top() + 34.0), link.size());
    if entry.plugin {
        let hovered = ui
            .interact(
                link_rect.expand(2.0),
                ui.scope_id().with(("vendor", &entry.key)),
                Sense::click(),
            )
            .on_hover_text(tr("Show only this vendor's plugins"))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if hovered.hovered() {
            painter.line_segment(
                [link_rect.left_bottom(), link_rect.right_bottom()],
                egui::Stroke::new(1.0, p.accent),
            );
        }
        vendor = hovered.clicked();
    }
    painter.galley(link_rect.min, link, p.weak);
    // What it does: two lines at most.
    let detail = if entry.plugin {
        let version = entry.detail.rsplit(' ').next().unwrap_or_default();
        if version.is_empty() {
            tr(entry.family).to_owned()
        } else {
            format!("{} · {version}", tr_family(entry.family))
        }
    } else {
        entry.detail.clone()
    };
    let mut job = egui::text::LayoutJob::simple(
        detail,
        FontId::proportional(11.5),
        theme::mix(p.text, p.weak, 0.35),
        inner.width() - 20.0,
    );
    job.wrap.max_rows = 3;
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(inner.left(), rect.top() + 54.0),
        galley,
        theme::mix(p.text, p.weak, 0.35),
    );
    // The star, shown on a favorite or under the pointer.
    let favorite = crate::favorites::with(|f| f.effects.contains(&entry.key));
    let star_rect =
        Rect::from_center_size(rect.right_bottom() - vec2(17.0, 17.0), vec2(22.0, 22.0));
    let star = ui
        .interact(
            star_rect,
            ui.scope_id().with(("star", &entry.key)),
            Sense::click(),
        )
        .on_hover_text(if favorite {
            tr("Remove this effect from the favorites (F)")
        } else {
            tr("Add this effect to the favorites (F)")
        });
    if favorite || response.hovered() || star.hovered() {
        let color = if favorite {
            effect_frame::star_color(&p)
        } else if star.hovered() {
            p.text
        } else {
            p.weak
        };
        effect_frame::paint_glyph(
            &painter,
            Rect::from_center_size(star_rect.center(), vec2(13.0, 13.0)),
            Glyph::Star(favorite),
            color,
        );
    }
    CardOut {
        response: response.on_hover_cursor(egui::CursorIcon::PointingHand),
        vendor,
        star: star.clicked(),
    }
}
/// Right-click menu of a plugin's card: the family to file it under; `Some(None)` goes back
/// to the guessed one.
fn family_menu(ui: &mut Ui, entry: &Entry) -> Option<Option<&'static str>> {
    ui.label(egui::RichText::new(tr("Family")).weak());
    let mut chosen = None;
    let families = audiostrada_dsp::CATEGORIES
        .iter()
        .copied()
        .filter(|f| *f != INSTRUMENT)
        .chain([crate::plugin_categories::OTHER]);
    for family in families {
        if ui
            .selectable_label(entry.family == family, tr(family))
            .clicked()
        {
            chosen = Some(Some(family));
            ui.close();
        }
    }
    ui.separator();
    if ui
        .button(tr("Automatic"))
        .on_hover_text(tr(
            "The family found from what the plugin declares, its name and a list of known plugins",
        ))
        .clicked()
    {
        chosen = Some(None);
        ui.close();
    }
    chosen
}

/// Whether an entry is in a family of the left column (`None`: all).
fn in_family(entry: &Entry, family: Option<&str>) -> bool {
    match family {
        None => true,
        Some(PLUGINS) => entry.plugin,
        Some(f) => entry.family == f,
    }
}

/// The distinct non-empty values, sorted.
fn choices<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = values
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect();
    out.sort_by_key(|v| v.to_lowercase());
    out.dedup();
    out
}

pub(crate) fn family_color(family: &str) -> Color32 {
    match family {
        "Dynamics" => Color32::from_rgb(232, 110, 90),
        "EQ & Filter" => Color32::from_rgb(240, 170, 70),
        "Space" => Color32::from_rgb(90, 160, 240),
        "Modulation" => Color32::from_rgb(160, 120, 240),
        "Color" => Color32::from_rgb(230, 90, 150),
        "Stereo" => Color32::from_rgb(70, 200, 200),
        "Amp & Cab" => Color32::from_rgb(200, 150, 90),
        "Utility" => Color32::from_rgb(150, 160, 175),
        CHAINS => Color32::from_rgb(230, 196, 80),
        _ => Color32::from_rgb(120, 200, 130),
    }
}

fn entries(
    doc: &Document,
    chains: &[crate::chains::Chain],
    plugin_families: &BTreeMap<String, String>,
    instruments: bool,
) -> Vec<Entry> {
    let name_of = |effect: &str| crate::mixer::effect_name(doc, effect);
    let chains = chains.iter().map(|c| {
        let (name, detail) = (c.label(), c.summary(name_of));
        Entry {
            key: format!("{CHAIN_KEY}{}", c.name),
            words: search_words(CHAINS, &[&name, &c.name, &detail]),
            name,
            family: CHAINS,
            detail,
            removable: !c.factory,
            plugin: false,
            vendor: String::new(),
            format: "",
        }
    });
    let mut out: Vec<Entry> = audiostrada_dsp::EFFECTS
        .iter()
        .filter(|i| (instruments || !i.instrument) && i.id != routing::STRIP)
        .map(|i| Entry {
            key: i.id.to_owned(),
            name: tr(i.name).to_owned(),
            family: i.category,
            detail: tr(i.description).to_owned(),
            removable: false,
            plugin: false,
            vendor: AUDIOSTRADA.to_owned(),
            format: AUDIOSTRADA,
            words: search_words(
                i.category,
                &[
                    i.id,
                    i.name,
                    tr(i.name),
                    i.description,
                    tr(i.description),
                    AUDIOSTRADA,
                ],
            ),
        })
        .collect();
    out.extend(
        doc.player()
            .plugins()
            .plugins()
            .filter(|p| audiostrada_plugins::enabled() && (instruments || !p.instrument))
            .map(|p| {
                let key = p.key();
                let family = if p.instrument {
                    INSTRUMENT
                } else {
                    plugin_families
                        .get(&key)
                        .and_then(|f| family_key(f))
                        .unwrap_or_else(|| {
                            crate::plugin_categories::family(&p.vendor, &p.name, &p.tags)
                        })
                };
                let mut texts = vec![p.name.as_str(), p.vendor.as_str(), p.format.name()];
                texts.extend(p.tags.iter().map(String::as_str));
                Entry {
                    words: search_words(family, &texts),
                    name: p.name.clone(),
                    family,
                    detail: format!(" · {} {}", p.format.name(), p.version),
                    removable: false,
                    plugin: true,
                    vendor: p.vendor.clone(),
                    format: p.format.name(),
                    key,
                }
            }),
    );
    // By family, built-in effects first, then by name.
    out.sort_by_key(|e| {
        (
            audiostrada_dsp::CATEGORIES
                .iter()
                .position(|c| *c == e.family)
                .unwrap_or(usize::MAX),
            e.plugin,
            e.name.to_lowercase(),
        )
    });
    chains.chain(out).collect()
}

/// The family key named `name`, as stored in the user's choices.
fn family_key(name: &str) -> Option<&'static str> {
    audiostrada_dsp::CATEGORIES
        .iter()
        .copied()
        .chain([crate::plugin_categories::OTHER])
        .find(|f| *f == name)
}

#[cfg(test)]
mod tests {
    use crate::document::AppDirs;

    /// Draws the browser, in its embedded window, for a few frames.
    fn draw(browser: &Shared, doc: &Document) -> Option<(Target, String, String)> {
        let ctx = egui::Context::default();
        let mut chosen = None;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                chosen = chosen.take().or(show(browser, ui.ctx(), doc))
            })
            .drop_without_applying_deltas();
        }
        chosen
    }

    #[test]
    fn every_family_vendor_and_search_draws() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut()
            .edit("Setup", |p| track = Some(p.add_track("A")));
        let track = Channel::Track(track.unwrap());
        let browser: Shared = Rc::default();
        for target in [(track, false), (track, true), (Channel::Master, false)] {
            browser.borrow_mut().open(target);
            assert!(draw(&browser, &doc).is_none());
            let families: Vec<String> = browser
                .borrow()
                .all
                .iter()
                .map(|e| e.family.to_owned())
                .collect();
            for family in families
                .into_iter()
                .chain([CHAINS.to_owned(), INSTRUMENT.to_owned()])
            {
                browser.borrow_mut().family = Some(family);
                draw(&browser, &doc);
            }
            let mut b = browser.borrow_mut();
            b.family = None;
            b.filter = "verb".into();
            b.vendor = Some(AUDIOSTRADA.to_owned());
            drop(b);
            draw(&browser, &doc);
        }
        browser.borrow_mut().open_instruments((track, false));
        draw(&browser, &doc);
    }

    use super::*;

    fn entry(family: &'static str, plugin: bool) -> Entry {
        Entry {
            key: "k".into(),
            name: "n".into(),
            family,
            detail: String::new(),
            removable: false,
            plugin,
            vendor: String::new(),
            format: "",
            words: String::new(),
        }
    }

    #[test]
    fn filters_and_families() {
        assert_eq!(
            choices(["AudioStrada", "", "acme", "AudioStrada"].into_iter()),
            ["acme", "AudioStrada"]
        );
        let reverb = entry("Space", true);
        assert!(in_family(&reverb, None));
        assert!(in_family(&reverb, Some(PLUGINS)));
        assert!(in_family(&reverb, Some("Space")));
        assert!(!in_family(&entry("Space", false), Some(PLUGINS)));
        assert_eq!(family_key("Dynamics"), Some("Dynamics"));
        let other = crate::plugin_categories::OTHER;
        assert_eq!(family_key(other), Some(other));
        assert_eq!(family_key("Bogus"), None);
    }

    #[test]
    fn the_search_reads_english_and_ignores_accents() {
        assert_eq!(fold("Réverbe Égaliseur"), "reverbe egaliseur");
        let doc = Document::new(AppDirs::default());
        let mut browser = EffectBrowser {
            all: entries(&doc, &[], &BTreeMap::new(), false),
            ..Default::default()
        };
        for search in ["reverb", "Réverb", "delay space", "strada"] {
            browser.filter = search.into();
            let all = browser.all.clone();
            assert!(
                browser.shown(&all).iter().any(|e| e.key == "reverb"),
                "{search}"
            );
        }
    }
}
