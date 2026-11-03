//! Effect browser: built-in effects and installed plugins by family, saved effect chains, a
//! search field and vendor and format filters. Opens when an insert is added.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use audiostrada_core::routing::{self, Channel};
use eframe::egui::{self, Color32, Sense, Ui, vec2};

use crate::document::Document;
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
    let id = egui::ViewportId::from_hash_of(VIEWPORT);
    if std::mem::take(&mut b.hidden) {
        ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
    }
    drop(b);
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("{} — AudioStrada", tr("Add an effect")))
        .with_inner_size([860.0, 600.0])
        .with_min_inner_size([520.0, 320.0])
        .with_window_level(crate::floating_level(ctx, crate::Floating::Browser));
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
}

const PLUGINS: &str = "Plugins";
const CHAINS: &str = "Chains";
/// Key prefix of a chain chosen in the browser, followed by its name.
pub const CHAIN_KEY: &str = "chain:";
const AUDIOSTRADA: &str = "AudioStrada";
const INSTRUMENT: &str = "Instrument";

impl EffectBrowser {
    pub fn open(&mut self, target: Target) {
        self.target = Some(target);
        self.filter.clear();
        self.focus_search = true;
        self.chains = crate::chains::list(crate::chains::root().as_deref());
        self.plugin_families = crate::plugin_categories::load();
    }

    /// Opens the browser on the instruments, to put one at the head of a track.
    pub fn open_instruments(&mut self, target: Target) {
        self.open(target);
        self.family = Some(INSTRUMENT.to_owned());
    }

    /// Draws the browser in its window: on a choice or when closed, leaves the result for
    /// the window that opened it, hides at once and has that window draw again.
    fn window(&mut self, ui: &mut Ui, target: Target, parent: egui::ViewportId) {
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
        let mut chosen = None;
        let mut open = true;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme_title(tr("Add an effect")));
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text(tr("Search effects and plugins"))
                        .desired_width(280.0),
                );
                if self.focus_search {
                    search.request_focus();
                    self.focus_search = false;
                }
                filter_combo(
                    ui,
                    "browser-vendor",
                    tr("Vendor"),
                    &vendors,
                    &mut self.vendor,
                );
                filter_combo(
                    ui,
                    "browser-format",
                    tr("Format"),
                    &formats,
                    &mut self.format,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Close")).clicked() {
                        open = false;
                    }
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                self.families(ui, &entries);
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    chosen = self.cards(ui, &entries);
                });
            });
        });
        if ui
            .ctx()
            .input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
        {
            open = false;
        }
        if chosen.is_some() || !open {
            self.target = None;
            self.result = chosen.map(|(key, name)| (target, key, name));
            self.hidden = true;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Chains and families changed here show in the entries the opener gathers.
        ui.ctx().request_repaint_of(parent);
    }

    fn families(&mut self, ui: &mut Ui, entries: &[Entry]) {
        ui.vertical(|ui| {
            ui.set_width(140.0);
            let count =
                |family: Option<&str>| entries.iter().filter(|e| in_family(e, family)).count();
            let mut row = |ui: &mut Ui, family: Option<&str>, label: &str| {
                let n = count(family);
                if n == 0 && family.is_some() {
                    return;
                }
                let on = self.family.as_deref() == family;
                if ui.selectable_label(on, format!("{label}  {n}")).clicked() {
                    self.family = family.map(str::to_owned);
                }
            };
            row(ui, None, tr("All"));
            row(ui, Some(CHAINS), tr(CHAINS));
            ui.add_space(4.0);
            for family in audiostrada_dsp::CATEGORIES {
                row(ui, Some(family), tr(family));
            }
            row(
                ui,
                Some(crate::plugin_categories::OTHER),
                tr(crate::plugin_categories::OTHER),
            );
            ui.add_space(4.0);
            row(ui, Some(PLUGINS), tr(PLUGINS));
        });
    }

    /// The entries of the chosen family matching the search, as cards.
    fn cards(&mut self, ui: &mut Ui, entries: &[Entry]) -> Option<(String, String)> {
        let filter = self.filter.to_lowercase();
        let shown: Vec<&Entry> = entries
            .iter()
            .filter(|e| in_family(e, self.family.as_deref()))
            .filter(|e| {
                filter.is_empty()
                    || e.name.to_lowercase().contains(&filter)
                    || e.detail.to_lowercase().contains(&filter)
                    || tr(e.family).to_lowercase().contains(&filter)
            })
            .collect();
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.weak(tr(
                        "Nothing matches. Plugin folders are set in Preferences › Plugins.",
                    ));
                }
                let width = ui.available_width();
                let columns = ((width / 300.0).floor() as usize).max(1);
                let card_width = width / columns as f32 - 8.0;
                let mut removed = None;
                let mut refiled = None;
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        for entry in row {
                            let (response, vendor) = card(ui, entry, card_width);
                            if vendor {
                                self.vendor = Some(entry.vendor.clone());
                            } else if response.clicked() {
                                chosen = Some((entry.key.clone(), entry.name.clone()));
                            }
                            if entry.removable {
                                response.context_menu(|ui| {
                                    if ui.button(tr("Delete chain")).clicked() {
                                        removed = Some(entry.key.clone());
                                        ui.close();
                                    }
                                });
                            } else if entry.plugin && entry.family != INSTRUMENT {
                                response.context_menu(|ui| {
                                    if let Some(family) = family_menu(ui, entry) {
                                        refiled = Some((entry.key.clone(), family));
                                    }
                                });
                            }
                        }
                    });
                    ui.add_space(4.0);
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
}

/// A clickable card: name, family and what the effect does, or a plugin's vendor (clickable,
/// to show only its plugins), format and version. Also returns whether the vendor was clicked.
fn card(ui: &mut Ui, entry: &Entry, width: f32) -> (egui::Response, bool) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 66.0), Sense::click());
    let fill = if response.hovered() {
        p.accent.gamma_multiply(0.18)
    } else {
        p.base
    };
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, vec2(4.0, rect.height())),
        egui::CornerRadius {
            nw: 6,
            sw: 6,
            ne: 0,
            se: 0,
        },
        family_color(entry.family),
    );
    painter.text(
        rect.min + vec2(14.0, 8.0),
        egui::Align2::LEFT_TOP,
        &entry.name,
        theme::strong(13.0),
        p.text,
    );
    painter.text(
        rect.right_top() + vec2(-10.0, 9.0),
        egui::Align2::RIGHT_TOP,
        tr(entry.family),
        egui::FontId::proportional(9.5),
        family_color(entry.family),
    );
    let mut vendor_clicked = false;
    let mut detail_left = rect.min + vec2(14.0, 28.0);
    if entry.plugin {
        let vendor = painter.layout_no_wrap(
            entry.vendor.clone(),
            egui::FontId::proportional(10.5),
            p.accent,
        );
        let vendor_rect = egui::Rect::from_min_size(detail_left, vendor.size());
        let link = ui
            .interact(
                vendor_rect.expand(2.0),
                ui.scope_id().with(("vendor", &entry.key)),
                Sense::click(),
            )
            .on_hover_text(tr("Show only this vendor's plugins"))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if link.hovered() {
            painter.line_segment(
                [vendor_rect.left_bottom(), vendor_rect.right_bottom()],
                egui::Stroke::new(1.0, p.accent),
            );
        }
        vendor_clicked = link.clicked();
        painter.galley(detail_left, vendor, p.accent);
        detail_left.x = vendor_rect.right();
    }
    let galley = painter.layout(
        entry.detail.clone(),
        egui::FontId::proportional(10.5),
        p.weak,
        rect.right() - 10.0 - detail_left.x,
    );
    painter.galley(detail_left, galley, p.weak);
    (
        response.on_hover_cursor(egui::CursorIcon::PointingHand),
        vendor_clicked,
    )
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
        .on_hover_text(tr("The family the plugin declares"))
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

/// A filter menu: "All" or one of `values`.
fn filter_combo(
    ui: &mut Ui,
    id: &str,
    label: &str,
    values: &[String],
    chosen: &mut Option<String>,
) {
    let shown = chosen.as_deref().unwrap_or(tr("All")).to_owned();
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("{label} : {shown}"))
        .width(170.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(chosen.is_none(), tr("All")).clicked() {
                *chosen = None;
            }
            for value in values {
                if ui
                    .selectable_label(chosen.as_ref() == Some(value), value)
                    .clicked()
                {
                    *chosen = Some(value.clone());
                }
            }
        });
}

fn theme_title(text: &str) -> egui::RichText {
    egui::RichText::new(text).font(theme::strong(15.0))
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
    let chains = chains.iter().map(|c| Entry {
        key: format!("{CHAIN_KEY}{}", c.name),
        name: c.label(),
        family: CHAINS,
        detail: c.summary(name_of),
        removable: !c.factory,
        plugin: false,
        vendor: String::new(),
        format: "",
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
                        .unwrap_or_else(|| crate::plugin_categories::guess(&p.tags))
                };
                Entry {
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
}
