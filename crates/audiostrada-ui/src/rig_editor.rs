//! Editor of the Amp rig: a pedalboard of drawn stompboxes, a drawn amp head and its cabinet,
//! with pickers for the amp and pedal captures of the library. The faces are generic, drawn in
//! classic styles without any brand.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use audiostrada_core::routing;
use audiostrada_dsp::amp::MODELS;
use audiostrada_dsp::rig::{
    CABS, PEDAL_GROUPS, PEDALS, SLOTS, in_loop, knob_names, pedal_based_on,
};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, pos2, vec2,
};

use crate::captures::{self, Category, Pack, Place};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;
use crate::tone3000::{List, Page, Pending, Session, Status, Tone, User};
use crate::trf;

/// Look of an amp face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Black panel, silver grille: clean American combo.
    Blackface,
    /// Gold panel, wheat grille: British crunch.
    Plexi,
    /// Copper panel, diamond grille: chiming British combo.
    Voxy,
    /// Smoked panel, cane grille, gold lettering: boutique overdrive.
    Boutique,
    /// Metal plate, black grille: modern high gain.
    Modern,
    /// Tweed covering, brown panel.
    Tweed,
    /// Silver panel with a blue line: bass head.
    Bass,
}

impl Style {
    /// The look named by a built-in amp.
    fn from_face(face: &str) -> Style {
        match face {
            "tweed" => Style::Tweed,
            "voxy" => Style::Voxy,
            "plexi" => Style::Plexi,
            "boutique" => Style::Boutique,
            "modern" => Style::Modern,
            "bass" => Style::Bass,
            _ => Style::Blackface,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Style::Blackface => "Blackface",
            Style::Plexi => "Plexi",
            Style::Voxy => "Voxy",
            Style::Boutique => "Boutique",
            Style::Modern => "Modern",
            Style::Tweed => "Tweed",
            Style::Bass => "Bass",
        }
    }

    /// Covering, panel, grille, knob and lettering colors.
    fn colors(self) -> [Color32; 5] {
        let c = Color32::from_rgb;
        match self {
            Style::Blackface => [
                c(24, 24, 26),
                c(28, 28, 30),
                c(178, 180, 182),
                c(20, 20, 22),
                c(230, 230, 230),
            ],
            Style::Plexi => [
                c(22, 20, 18),
                c(196, 160, 82),
                c(186, 168, 128),
                c(26, 22, 18),
                c(30, 24, 16),
            ],
            Style::Boutique => [
                c(18, 18, 20),
                c(46, 40, 36),
                c(150, 130, 100),
                c(30, 30, 32),
                c(220, 190, 120),
            ],
            Style::Voxy => [
                c(30, 26, 24),
                c(172, 102, 58),
                c(96, 64, 50),
                c(36, 30, 26),
                c(240, 228, 210),
            ],
            Style::Modern => [
                c(16, 16, 18),
                c(150, 154, 160),
                c(28, 28, 30),
                c(40, 42, 46),
                c(18, 18, 20),
            ],
            Style::Tweed => [
                c(186, 156, 104),
                c(120, 82, 46),
                c(110, 86, 60),
                c(30, 22, 16),
                c(240, 228, 200),
            ],
            Style::Bass => [
                c(20, 20, 24),
                c(196, 200, 206),
                c(40, 40, 44),
                c(24, 24, 28),
                c(30, 60, 140),
            ],
        }
    }

    /// The materials of an amp or a cabinet of this style.
    fn look(self) -> crate::gear::Look {
        use crate::gear::{KnobStyle, Look, Texture};
        let c = Color32::from_rgb;
        let red = c(225, 40, 28);
        match self {
            Style::Blackface => Look {
                covering: (Texture::Tolex, c(34, 34, 36)),
                cloth: (Texture::Sparkle, c(196, 198, 202)),
                panel: c(44, 44, 48),
                piping: Some(c(220, 220, 220)),
                corners: false,
                knob: KnobStyle::Skirted,
                letters: c(236, 236, 236),
                lamp: red,
            },
            Style::Plexi => Look {
                covering: (Texture::Tolex, c(30, 28, 26)),
                cloth: (Texture::Basket, c(196, 178, 136)),
                panel: c(218, 180, 92),
                piping: Some(c(232, 224, 204)),
                corners: true,
                knob: KnobStyle::GoldCap,
                letters: c(32, 26, 18),
                lamp: red,
            },
            Style::Voxy => Look {
                covering: (Texture::Tolex, c(38, 34, 32)),
                cloth: (Texture::Diamond, c(132, 88, 64)),
                panel: c(196, 126, 74),
                piping: Some(c(232, 224, 204)),
                corners: true,
                knob: KnobStyle::ChickenHead,
                letters: c(34, 24, 16),
                lamp: red,
            },
            Style::Boutique => Look {
                covering: (Texture::Tolex, c(26, 26, 28)),
                cloth: (Texture::Cane, c(178, 156, 118)),
                panel: c(56, 52, 48),
                piping: Some(c(204, 174, 104)),
                corners: false,
                knob: KnobStyle::Knurled,
                letters: c(224, 194, 124),
                lamp: c(255, 166, 50),
            },
            Style::Modern => Look {
                covering: (Texture::Tolex, c(22, 22, 24)),
                cloth: (Texture::Basket, c(70, 70, 76)),
                panel: c(178, 182, 188),
                piping: None,
                corners: true,
                knob: KnobStyle::Knurled,
                letters: c(20, 20, 22),
                lamp: c(70, 190, 255),
            },
            Style::Tweed => Look {
                covering: (Texture::Tweed, c(206, 170, 112)),
                cloth: (Texture::Sparkle, c(128, 78, 56)),
                panel: c(200, 202, 206),
                piping: None,
                corners: false,
                knob: KnobStyle::ChickenHead,
                letters: c(24, 22, 20),
                lamp: red,
            },
            Style::Bass => Look {
                covering: (Texture::Tolex, c(26, 26, 30)),
                cloth: (Texture::Perforated, c(168, 172, 178)),
                panel: c(204, 208, 214),
                piping: None,
                corners: true,
                knob: KnobStyle::Skirted,
                letters: c(30, 60, 140),
                lamp: red,
            },
        }
    }

    /// The look of a cabinet, from what it is.
    fn of_cab(cab: usize) -> Style {
        match cab {
            0 => Style::Modern,
            1 => Style::Plexi,
            2 => Style::Voxy,
            3..=5 => Style::Blackface,
            _ => Style::Bass,
        }
    }

    /// A style for gear from its description and gain (0 to 1), when the files say little.
    pub fn guess(text: &str, gain: Option<f32>) -> Style {
        let t = text.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
        if has(&["tweed", "bassman", "champ", "5e3"]) {
            Style::Tweed
        } else if has(&["bass", "svt", "ampeg", "b15", "darkglass"]) {
            Style::Bass
        } else if has(&["vox", "ac30", "ac15", "ac-30", "matchless"]) {
            Style::Voxy
        } else if has(&["dumble", "two-rock", "two rock", "ods", "tone king"]) {
            Style::Boutique
        } else if has(&[
            "marshall",
            "plexi",
            "jcm",
            "jtm",
            "800",
            "super lead",
            "orange",
        ]) {
            Style::Plexi
        } else if has(&[
            "mesa", "recti", "5150", "6505", "evh", "peavey", "diezel", "engl", "bogner",
            "friedman",
        ]) {
            Style::Modern
        } else if has(&[
            "fender",
            "twin",
            "deluxe",
            "princeton",
            "blackface",
            "clean",
        ]) {
            Style::Blackface
        } else {
            match gain {
                Some(g) if g > 0.7 => Style::Modern,
                Some(g) if g > 0.35 => Style::Plexi,
                _ => Style::Blackface,
            }
        }
    }
}

/// What the rig editor remembers between frames.
#[derive(Debug, Default)]
pub struct RigState {
    picker: Option<Picker>,
    filter: String,
    /// What the TONE3000 browser's list is narrowed to.
    search: String,
    tab: Tab,
    /// The TONE3000 browser, when open, and the lists it loaded.
    browser: Option<Browse>,
    feeds: HashMap<(Kind, List), Feed>,
    user: Option<Pending<User>>,
    /// A TONE3000 sign-in or download, where its captures go (found from them when `None`)
    /// and their category.
    session: Option<(Session, Option<Picker>, Option<Category>)>,
    message: Option<String>,
    info: Option<String>,
    /// Library filter: a category, or favourites when `None` and `favorites` is set.
    category: Option<Category>,
    favorites: bool,
    renaming: Option<(PathBuf, String)>,
    /// Pedal being dragged to another place.
    dragging: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    BuiltIn,
    Library,
}

/// Where chosen captures go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picker {
    Amp,
    Pedal(usize),
    Cab,
}

impl Picker {
    fn place(self) -> Place {
        match self {
            Picker::Amp => Place::Amp,
            Picker::Pedal(_) => Place::Pedalboard,
            Picker::Cab => Place::Cab,
        }
    }
}

/// The TONE3000 browser's sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Amps,
    Effects,
    Cabs,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Amps, Kind::Effects, Kind::Cabs];

    fn name(self) -> &'static str {
        match self {
            Kind::Amps => tr("Amps"),
            Kind::Effects => tr("Effects"),
            Kind::Cabs => tr("Cabs"),
        }
    }

    fn of(place: Place) -> Kind {
        match place {
            Place::Amp => Kind::Amps,
            Place::Pedalboard => Kind::Effects,
            Place::Cab => Kind::Cabs,
        }
    }

    fn place(self) -> Place {
        match self {
            Kind::Amps => Place::Amp,
            Kind::Effects => Place::Pedalboard,
            Kind::Cabs => Place::Cab,
        }
    }

    /// TONE3000 gear types asked for, one after the other.
    fn gears(self) -> &'static [&'static str] {
        match self {
            Kind::Amps => &["amp", "amp-cab"],
            Kind::Effects => &["pedal", "outboard"],
            Kind::Cabs => &["cab"],
        }
    }

    fn fits(self, tone: &Tone) -> bool {
        let format = if self == Kind::Cabs { "ir" } else { "nam" };
        tone.format == format && Category::from_gear(&tone.gear).place() == self.place()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Browse {
    kind: Kind,
    list: List,
}

/// A list of the browser, loaded page by page as it is scrolled.
#[derive(Debug, Default)]
struct Feed {
    tones: Vec<Tone>,
    gear: usize,
    page: u32,
    done: bool,
    pending: Option<Pending<Page>>,
    error: Option<String>,
}

impl Feed {
    fn poll(&mut self, ctx: &egui::Context, kind: Kind, list: List) {
        let Some(answer) = self.pending.as_ref().and_then(|p| ready(ctx, p)) else {
            return;
        };
        self.pending = None;
        match answer {
            Ok((tones, more)) => {
                for tone in tones {
                    if kind.fits(&tone) && !self.tones.iter().any(|t| t.id == tone.id) {
                        self.tones.push(tone);
                    }
                }
                if !more {
                    if list != List::Latest && self.gear + 1 < kind.gears().len() {
                        self.gear += 1;
                        self.page = 0;
                    } else {
                        self.done = true;
                    }
                }
            }
            Err(e) => {
                self.error = Some(e);
                self.done = true;
            }
        }
    }

    fn more(&mut self, kind: Kind, list: List) {
        if self.pending.is_none() && !self.done {
            self.page += 1;
            let gear = kind.gears()[self.gear];
            self.pending = Some(crate::tone3000::list(list, Some(gear), self.page));
        }
    }
}

fn slot_id(slot: usize, what: &str) -> String {
    format!("p{}_{what}", slot + 1)
}

/// Sets a parameter from a click (a menu entry, a switch) as one undo step.
fn set_now(params: &mut Params, id: &str, value: f32, response: &Response) {
    params.set(id, Some(value), response);
    params.set(id, None, response);
    params.doc.history_mut().end_gesture();
}

/// A drawn knob turning from 0 to 10.
fn face_knob(
    ui: &mut Ui,
    center: Pos2,
    diameter: f32,
    value: f32,
    look: &crate::gear::Look,
    key: impl std::hash::Hash + std::fmt::Debug,
) -> (Option<f32>, Response) {
    let rect = Rect::from_center_size(center, vec2(diameter, diameter));
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click_and_drag());
    let (min, max, span) = (0.0, 10.0, 10.0);
    let mut v = value;
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        v = (v - response.drag_delta().y / 150.0 * span * if fine { 0.15 } else { 1.0 })
            .clamp(min, max);
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            v = (v + wheel.signum() * span * 0.02).clamp(min, max);
        }
    }
    crate::gear::knob(
        ui.painter(),
        center,
        diameter,
        (v - min) / span,
        look.knob,
        look.letters,
    );
    ((v != value).then_some(v), response)
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut RigState) {
    let p = theme::current(ui);
    let input = meters.first().copied().unwrap_or(-120.0);
    ui.horizontal(|ui| {
        // The guitar's level into the rig: red when it would clip the first stage.
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        let lit = if input > -3.0 {
            theme::RECORD
        } else if input > -40.0 {
            theme::PLAY
        } else {
            p.base
        };
        ui.painter().circle_filled(dot.center(), 4.0, lit);
        small_param(ui, params, "input", tr("Input"));
        small_param(ui, params, "gate", tr("Gate"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button =
                egui::Button::new(RichText::new(tr("Browse TONE3000…")).font(theme::strong(13.0)))
                    .fill(p.accent.gamma_multiply(0.35));
            if ui
                .add(button)
                .on_hover_text(tr(
                    "Free amp, effect and cabinet captures, downloaded and loaded in one click",
                ))
                .clicked()
            {
                state.browser = Some(Browse {
                    kind: Kind::Amps,
                    list: List::Trending,
                });
            }
        });
    });
    ui.add_space(6.0);
    chain(ui, params, state);
    let open_browser = |state: &mut RigState, kind: Kind| {
        state.browser = Some(Browse {
            kind,
            list: List::Trending,
        });
    };
    match section(
        ui,
        tr("PEDALBOARD"),
        &[tr("Effect captures…"), tr("Effects on TONE3000…")],
    ) {
        Some(0) => match free_slot(params) {
            Some(slot) => open_picker(state, Picker::Pedal(slot)),
            None => params
                .doc
                .notify(tr("The pedalboard is full: remove a pedal first.").into()),
        },
        Some(_) => open_browser(state, Kind::Effects),
        None => {}
    }
    pedalboard(ui, params, state);
    match section(
        ui,
        tr("AMP AND CABINET"),
        &[
            tr("All amps…"),
            tr("Amps on TONE3000…"),
            tr("Impulses…"),
            tr("Cabs on TONE3000…"),
        ],
    ) {
        Some(0) => open_picker(state, Picker::Amp),
        Some(1) => open_browser(state, Kind::Amps),
        Some(2) => open_picker(state, Picker::Cab),
        Some(_) => open_browser(state, Kind::Cabs),
        None => {}
    }
    // The head sits on its cabinet, as on stage.
    let width = ui.available_width().max(560.0);
    let cab = (params.get("cab").round().max(0.0) as usize).min(CABS.len() - 1);
    let size = cab_size(cab, width) * CAB_SCALE;
    // The head is the rig's face: as wide as the window allows, in its proportions.
    let head_width = (width - 120.0).clamp(520.0, 760.0);
    let head_height = head_width * 0.29;
    ui.add_space(10.0);
    let (row, _) = ui.allocate_exact_size(vec2(width, head_height + 30.0), Sense::hover());
    let head = Rect::from_center_size(
        pos2(row.center().x, row.top() + 12.0 + head_height / 2.0),
        vec2(head_width, head_height),
    );
    amp(ui, params, state, head.expand2(vec2(52.0, 0.0)));
    let (row, _) = ui.allocate_exact_size(vec2(width, size.y + 36.0), Sense::hover());
    cabinet(ui, params, row, size);
    poll_session(ui, params, state);
    picker(ui, params, state);
    browser(ui, params, state);
}

/// The cabinet under the head, smaller than life: the head is what is played with.
const CAB_SCALE: f32 = 0.62;

/// One block of the chain overview.
struct Block {
    name: String,
    color: Color32,
    on: bool,
    /// What a click on it does.
    action: BlockAction,
}

#[derive(Clone, Copy)]
enum BlockAction {
    TogglePedal(usize),
    PickAmp,
    PickCab,
}

/// The whole rig as the signal goes through it, on one line: pedals, amp, cabinet, then the
/// effects loop. A pedal's block switches it, the amp's and the cabinet's choose them.
fn chain(ui: &mut Ui, params: &mut Params, state: &mut RigState) {
    let p = theme::current(ui);
    let pedal = |params: &Params, slot: usize| {
        let kind = pedal_kind(params, slot);
        let name = if kind == 12 {
            capture_name(params, &format!("pedal{}", slot + 1))
                .unwrap_or_else(|| tr(PEDALS[kind]).into())
        } else {
            tr(PEDALS[kind]).to_owned()
        };
        Block {
            name,
            color: pedal_color(kind),
            on: params.get(&slot_id(slot, "on")) >= 0.5,
            action: BlockAction::TogglePedal(slot),
        }
    };
    let used: Vec<usize> = (0..SLOTS).filter(|&s| pedal_kind(params, s) != 0).collect();
    let mut blocks: Vec<Block> = used
        .iter()
        .filter(|&&s| !in_loop(pedal_kind(params, s)))
        .map(|&s| pedal(params, s))
        .collect();
    let style = amp_style(params);
    let amp_name = match builtin(params) {
        Some(model) => model.name.to_owned(),
        None => capture_name(params, "amp").unwrap_or_else(|| tr("Choose an amp").into()),
    };
    blocks.push(Block {
        name: amp_name,
        color: style.look().panel,
        on: true,
        action: BlockAction::PickAmp,
    });
    let cab = (params.get("cab").round().max(0.0) as usize).min(CABS.len() - 1);
    blocks.push(Block {
        name: tr(CABS[cab]).to_owned(),
        color: Color32::from_rgb(52, 52, 56),
        on: true,
        action: BlockAction::PickCab,
    });
    blocks.extend(
        used.iter()
            .filter(|&&s| in_loop(pedal_kind(params, s)))
            .map(|&s| pedal(params, s)),
    );
    let height = 34.0;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height + 8.0), Sense::hover());
    let painter = ui.painter_at(row);
    let mid = row.center().y;
    let font = theme::strong(11.0);
    let widths: Vec<f32> = blocks
        .iter()
        .map(|b| {
            painter
                .layout_no_wrap(b.name.clone(), font.clone(), Color32::WHITE)
                .size()
                .x
                .clamp(40.0, 150.0)
                + 26.0
        })
        .collect();
    let gap = 22.0;
    let total: f32 = widths.iter().sum::<f32>() + gap * (blocks.len() + 1) as f32;
    // Squeezed to the row when it is too long.
    let squeeze = ((row.width() - 40.0) / total).min(1.0);
    let line = Stroke::new(1.5, p.weak.gamma_multiply(0.6));
    painter.line_segment(
        [pos2(row.left() + 6.0, mid), pos2(row.right() - 14.0, mid)],
        line,
    );
    painter.circle_stroke(pos2(row.left() + 6.0, mid), 3.0, line);
    let tip = pos2(row.right() - 6.0, mid);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, tip - vec2(9.0, 5.0), tip - vec2(9.0, -5.0)],
        p.weak.gamma_multiply(0.6),
        Stroke::NONE,
    ));
    let mut x = row.left() + 20.0 + gap * squeeze;
    let mut clicked = None;
    for (i, (block, w)) in blocks.iter().zip(&widths).enumerate() {
        let w = w * squeeze;
        let rect = Rect::from_min_size(pos2(x, mid - height / 2.0), vec2(w, height));
        let response = ui.interact(rect, ui.scope_id().with(("chain", i)), Sense::click());
        let fill = if block.on {
            block.color
        } else {
            block.color.gamma_multiply(0.25)
        };
        painter.rect_filled(rect, 6.0, p.base);
        painter.rect_filled(rect.shrink(1.0), 5.0, fill);
        if response.hovered() {
            painter.rect_stroke(
                rect.expand(1.5),
                7.0,
                Stroke::new(1.5, p.text),
                egui::StrokeKind::Outside,
            );
        }
        let ink = if block.on {
            theme::text_on(fill)
        } else {
            p.weak
        };
        let shown = fit(&painter, &block.name, &font, w - 10.0);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            shown,
            font.clone(),
            ink,
        );
        let hint = match block.action {
            BlockAction::TogglePedal(_) if block.on => {
                trf!("{name}: click to switch it off", name = block.name)
            }
            BlockAction::TogglePedal(_) => {
                trf!("{name} (off): click to switch it on", name = block.name)
            }
            BlockAction::PickAmp => trf!("{name}: click to choose the amp", name = block.name),
            BlockAction::PickCab => {
                trf!("{name}: click to choose the cabinet", name = block.name)
            }
        };
        let response = response.on_hover_text(hint);
        if let BlockAction::PickCab = block.action {
            egui::Popup::menu(&response).show(|ui| {
                for (k, name) in CABS.iter().enumerate() {
                    if ui.selectable_label(k == cab, tr(name)).clicked() {
                        set_now(params, "cab", k as f32, &response);
                        ui.close();
                    }
                }
            });
        } else if response.clicked() {
            clicked = Some((block.action, block.on, response));
        }
        x += w + gap * squeeze;
    }
    match clicked {
        Some((BlockAction::TogglePedal(slot), on, response)) => {
            set_now(
                params,
                &slot_id(slot, "on"),
                if on { 0.0 } else { 1.0 },
                &response,
            );
        }
        Some((BlockAction::PickAmp, _, _)) => {
            state.picker = Some(Picker::Amp);
            state.filter.clear();
        }
        _ => {}
    }
}

/// A section title with buttons on the right; returns the button clicked.
fn section(ui: &mut Ui, title: &str, buttons: &[&str]) -> Option<usize> {
    let p = theme::current(ui);
    let mut clicked = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(theme::strong(12.0)).color(p.weak));
        let (line, _) = ui.allocate_exact_size(vec2(24.0, 12.0), Sense::hover());
        ui.painter().line_segment(
            [line.left_center(), line.right_center()],
            Stroke::new(1.0, p.border),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, text) in buttons.iter().enumerate().rev() {
                if ui.small_button(*text).clicked() {
                    clicked = Some(i);
                }
            }
        });
    });
    ui.add_space(2.0);
    clicked
}

fn open_picker(state: &mut RigState, target: Picker) {
    state.picker = Some(target);
    state.filter.clear();
}

fn small_param(ui: &mut Ui, params: &mut Params, id: &str, label: &str) {
    let Some(info) = params.info.params.iter().find(|q| q.id == id) else {
        return;
    };
    let mut v = params.get(id);
    ui.label(
        RichText::new(label)
            .size(10.0)
            .color(theme::current(ui).weak),
    );
    let response = ui.add(
        egui::DragValue::new(&mut v)
            .range(info.min..=info.max)
            .speed(0.2)
            .custom_formatter(|v, _| {
                if v <= -100.0 {
                    "Off".into()
                } else {
                    format!("{v:.0} dB")
                }
            })
            .custom_parser(|t| {
                let t = t.trim().trim_end_matches("dB").trim();
                if t.eq_ignore_ascii_case("off") {
                    Some(-100.0)
                } else {
                    t.parse().ok()
                }
            }),
    );
    let changed = response.changed().then_some(v);
    params.set(id, changed, &response);
}

fn pedal_color(kind: usize) -> Color32 {
    let c = Color32::from_rgb;
    match kind {
        1 => c(220, 220, 214),
        2 => c(60, 170, 90),
        3 => c(250, 120, 30),
        4 => c(170, 50, 60),
        5 => c(200, 40, 40),
        6 => c(90, 190, 220),
        7 => c(240, 160, 40),
        8 => c(140, 90, 200),
        9 => c(40, 160, 150),
        10 => c(225, 215, 190),
        11 => c(40, 60, 120),
        12 => c(20, 70, 80),
        13 => c(212, 175, 55),
        14 => c(40, 90, 200),
        15 => c(32, 32, 34),
        16 => c(200, 200, 205),
        17 => c(55, 55, 60),
        18 => c(150, 150, 150),
        19 => c(22, 22, 22),
        20 => c(30, 110, 60),
        21 => c(240, 210, 40),
        _ => c(60, 60, 64),
    }
}

fn pedal_kind(params: &Params, slot: usize) -> usize {
    params.get(&slot_id(slot, "type")).round().max(0.0) as usize
}

/// Puts the pedals of the slots in `order` in the first slots, empties the others, in one
/// undo step (parameters and captures move together).
fn rearrange(params: &mut Params, order: &[usize]) {
    let (channel, insert) = (params.channel, params.insert);
    let mut values = Vec::new();
    for new in 0..SLOTS {
        for (field, empty) in [
            ("type", 0.0),
            ("on", 1.0),
            ("k1", 5.0),
            ("k2", 5.0),
            ("k3", 5.0),
        ] {
            let v = order
                .get(new)
                .map_or(empty, |&old| params.get(&slot_id(old, field)));
            values.push((slot_id(new, field), v));
        }
    }
    let stored = routing::find_insert(params.doc.project(), channel, insert)
        .map(|i| i.files.clone())
        .unwrap_or_default();
    let files: Vec<(String, Option<PathBuf>)> = (0..SLOTS)
        .map(|new| {
            let file = order
                .get(new)
                .and_then(|old| stored.get(&format!("pedal{}", old + 1)).cloned());
            (format!("pedal{}", new + 1), file)
        })
        .collect();
    params.doc.history_mut().edit(tr("Move pedals"), |p| {
        for (id, v) in &values {
            routing::set_insert_param(p, channel, insert, id, *v);
        }
        for (slot, file) in &files {
            routing::set_insert_file(p, channel, insert, slot, file.clone());
        }
    });
    for (id, v) in values {
        if let Some(i) = params.info.params.iter().position(|q| q.id == id) {
            params.values[i] = v;
        }
    }
}

/// The slot a new pedal goes in: after the last pedal, else any empty one.
fn free_slot(params: &Params) -> Option<usize> {
    let last = (0..SLOTS).rev().find(|&s| pedal_kind(params, s) != 0);
    (0..SLOTS)
        .find(|&s| pedal_kind(params, s) == 0 && last.is_none_or(|l| s > l))
        .or_else(|| (0..SLOTS).find(|&s| pedal_kind(params, s) == 0))
}

fn pedalboard(ui: &mut Ui, params: &mut Params, state: &mut RigState) {
    let shown: Vec<usize> = (0..SLOTS).filter(|&s| pedal_kind(params, s) != 0).collect();
    let free = free_slot(params);
    let width = ui.available_width().max(520.0);
    let (gap, w, h, add_w) = (12.0, 124.0, 150.0, 124.0);
    let per_row = (((width - gap) / (w + gap)).floor() as usize).max(1);
    // The narrow "+" tile follows the last pedal, on a new row only when there is no room.
    let n = shown.len();
    let fits_after = |pedals: usize| width - gap - pedals as f32 * (w + gap) >= add_w + gap;
    let (add_row, add_col) = if n > 0 && n.is_multiple_of(per_row) && fits_after(per_row) {
        (n / per_row - 1, per_row)
    } else if fits_after(n % per_row) {
        (n / per_row, n % per_row)
    } else {
        (n / per_row + 1, 0)
    };
    let rows = if free.is_some() {
        (add_row + 1).max(n.div_ceil(per_row))
    } else {
        n.div_ceil(per_row).max(1)
    };
    let (board, _) =
        ui.allocate_exact_size(vec2(width, rows as f32 * (h + gap) + gap), Sense::hover());
    let painter = ui.painter_at(board.expand(12.0));
    let places: Vec<Rect> = (0..per_row * rows)
        .map(|i| {
            let (row, col) = (i / per_row, i % per_row);
            Rect::from_min_size(
                pos2(
                    board.left() + gap + col as f32 * (w + gap),
                    board.top() + gap + row as f32 * (h + gap),
                ),
                vec2(w, h),
            )
        })
        .filter(|r| r.right() <= board.right() - gap * 0.5)
        .collect();
    crate::gear::board(&painter, board, &places);
    if shown.is_empty() {
        let galley = painter.layout_no_wrap(
            tr("Add a pedal: an effect capture, or one from TONE3000").to_owned(),
            FontId::proportional(13.0),
            Color32::from_white_alpha(190),
        );
        let at = board.left_center() + vec2(gap + add_w + 24.0, -galley.size().y / 2.0);
        painter.rect_filled(
            Rect::from_min_size(at, galley.size()).expand2(vec2(10.0, 5.0)),
            6.0,
            Color32::from_black_alpha(170),
        );
        painter.galley(at, galley, Color32::WHITE);
    }
    let place = |i: usize| {
        let (row, col) = (i / per_row, i % per_row);
        Rect::from_min_size(
            pos2(
                board.left() + gap + col as f32 * (w + gap),
                board.top() + gap + row as f32 * (h + gap),
            ),
            vec2(w, h),
        )
    };
    let mut rects = Vec::new();
    for (i, &slot) in shown.iter().enumerate() {
        let body = stompbox(ui, params, state, slot, place(i), &shown);
        if body.drag_started() {
            state.dragging = Some(slot);
        }
        rects.push(place(i));
    }
    // Dragging a pedal: a bar shows where it lands, releasing moves it there.
    if let Some(dragged) = state.dragging {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let target = pointer.map(|p| {
            rects
                .iter()
                .filter(|r| r.bottom() < p.y || (r.top() <= p.y && r.center().x < p.x))
                .count()
        });
        if let (Some(target), Some(from)) = (target, shown.iter().position(|&s| s == dragged)) {
            painter.rect_filled(rects[from], 9.0, Color32::from_black_alpha(120));
            let bar = match rects.get(target) {
                Some(r) => Rect::from_min_size(r.min - vec2(gap * 0.5 + 2.0, 0.0), vec2(4.0, h)),
                None => {
                    let last = rects[rects.len() - 1];
                    Rect::from_min_size(
                        pos2(last.right() + gap * 0.5 - 2.0, last.top()),
                        vec2(4.0, h),
                    )
                }
            };
            painter.rect_filled(bar, 2.0, theme::current(ui).accent);
            if ui.input(|i| i.pointer.any_released()) {
                let mut order = shown.clone();
                let item = order.remove(from);
                order.insert(if target > from { target - 1 } else { target }, item);
                if order != shown {
                    rearrange(params, &order);
                }
                state.dragging = None;
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            state.dragging = None;
        }
    }
    if let Some(slot) = free {
        let col = add_col;
        let rect = Rect::from_min_size(
            pos2(
                board.left() + gap + col as f32 * (w + gap),
                board.top() + gap + add_row as f32 * (h + gap),
            ),
            vec2(add_w, h),
        );
        let add = ui.interact(
            rect,
            ui.scope_id().with(("pedal-add", slot)),
            Sense::click(),
        );
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(
                1.5,
                Color32::from_white_alpha(if add.hovered() { 90 } else { 40 }),
            ),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "+",
            theme::strong(24.0),
            Color32::from_white_alpha(if add.hovered() { 220 } else { 120 }),
        );
        let add = add.on_hover_text(tr("Add a pedal"));
        pedal_menu(params, slot, state, &add, &shown);
    }
}

fn stompbox(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut RigState,
    slot: usize,
    rect: Rect,
    shown: &[usize],
) -> Response {
    let kind = pedal_kind(params, slot);
    let on = params.get(&slot_id(slot, "on")) >= 0.5;
    let painter = ui.painter().clone();
    // The body drags the pedal; knobs, switch and icons are above it.
    let body = ui.interact(
        rect,
        ui.scope_id().with(("pedal-body", slot)),
        Sense::click_and_drag(),
    );
    let color = pedal_color(kind);
    painter.rect_filled(
        rect.translate(vec2(2.0, 3.0)),
        9.0,
        Color32::from_black_alpha(110),
    );
    painter.rect_filled(rect, 9.0, color);
    // Painted metal: lighter at the top, a soft edge.
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() * 0.45)),
        egui::CornerRadius {
            nw: 9,
            ne: 9,
            sw: 0,
            se: 0,
        },
        Color32::from_white_alpha(18),
    );
    painter.rect_stroke(
        rect,
        9.0,
        Stroke::new(1.0, Color32::from_white_alpha(50)),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            rect.left_bottom() + vec2(9.0, -1.0),
            rect.right_bottom() + vec2(-9.0, -1.0),
        ],
        Stroke::new(2.0, Color32::from_black_alpha(70)),
    );
    let dark = color.r() as u16 + color.g() as u16 + color.b() as u16 > 420;
    let ink = if dark {
        Color32::from_rgb(24, 24, 24)
    } else {
        Color32::from_rgb(240, 240, 236)
    };
    let name = if kind == 12 {
        capture_name(params, &format!("pedal{}", slot + 1)).unwrap_or_else(|| tr("Capture").into())
    } else {
        tr(PEDALS[kind]).to_owned()
    };
    // LED.
    let led = pos2(rect.center().x, rect.top() + 14.0);
    painter.circle_filled(
        led,
        5.0,
        if on {
            Color32::from_rgb(255, 50, 40)
        } else {
            Color32::from_rgb(70, 20, 20)
        },
    );
    if on {
        painter.circle_filled(led, 9.0, Color32::from_rgba_unmultiplied(255, 60, 40, 40));
    }
    // Knobs.
    let names = knob_names(kind);
    let used: Vec<usize> = (0..3).filter(|&k| !names[k].is_empty()).collect();
    let spacing = rect.width() / (used.len() as f32 + 1.0);
    // Silver knobs on black pedals, black ones elsewhere.
    let black = (color.r() as u16 + color.g() as u16 + color.b() as u16) < 200;
    let look = crate::gear::Look {
        knob: if black {
            crate::gear::KnobStyle::Skirted
        } else {
            crate::gear::KnobStyle::Knurled
        },
        letters: if black {
            Color32::from_rgb(230, 230, 230)
        } else {
            Color32::from_rgb(24, 24, 26)
        },
        ..Style::Blackface.look()
    };
    for (n, &k) in used.iter().enumerate() {
        let id = slot_id(slot, &format!("k{}", k + 1));
        let center = pos2(rect.left() + spacing * (n as f32 + 1.0), rect.top() + 44.0);
        let (changed, response) = face_knob(
            ui,
            center,
            (spacing * 0.6).min(26.0),
            params.get(&id),
            &look,
            ("pedal-knob", slot, k),
        );
        let response = response.on_hover_text(format!(
            "{}: {:.1}",
            names[k],
            changed.unwrap_or(params.get(&id))
        ));
        params.set(&id, changed, &response);
        painter.text(
            center + vec2(0.0, 17.0),
            Align2::CENTER_TOP,
            names[k],
            FontId::proportional(if names[k].len() > 6 { 8.0 } else { 9.0 }),
            ink,
        );
    }
    // Name plate and footswitch.
    let font = theme::strong(12.5);
    painter.text(
        pos2(rect.center().x, rect.top() + 90.0),
        Align2::CENTER_CENTER,
        fit(&painter, &name, &font, rect.width() - 10.0),
        font,
        ink,
    );
    let foot = pos2(rect.center().x, rect.bottom() - 24.0);
    let switch = ui.interact(
        Rect::from_center_size(foot, vec2(34.0, 34.0)),
        ui.scope_id().with(("pedal-switch", slot)),
        Sense::click(),
    );
    painter.circle_filled(foot, 15.0, Color32::from_rgb(170, 172, 176));
    painter.circle_filled(
        foot,
        11.0,
        if switch.hovered() {
            Color32::from_rgb(220, 222, 226)
        } else {
            Color32::from_rgb(196, 198, 202)
        },
    );
    let switch = switch.on_hover_text(if on { tr("Bypass") } else { tr("Turn on") });
    if switch.clicked() {
        set_now(
            params,
            &slot_id(slot, "on"),
            if on { 0.0 } else { 1.0 },
            &switch,
        );
    }
    // Change and remove icons, shown while the pointer is over the pedal.
    let icon = |dx: f32| {
        Rect::from_center_size(pos2(rect.right() - dx, rect.top() + 14.0), vec2(20.0, 20.0))
    };
    let edit = ui.interact(
        icon(32.0),
        ui.scope_id().with(("pedal-edit", slot)),
        Sense::click(),
    );
    let remove = ui.interact(
        icon(12.0),
        ui.scope_id().with(("pedal-remove", slot)),
        Sense::click(),
    );
    let menu_open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&edit));
    if (ui.rect_contains_pointer(rect) && state.dragging.is_none()) || menu_open {
        for (r, hovered) in [(edit.rect, edit.hovered()), (remove.rect, remove.hovered())] {
            let fill = if hovered {
                Color32::from_black_alpha(200)
            } else {
                Color32::from_black_alpha(120)
            };
            painter.circle_filled(r.center(), 10.0, fill);
        }
        let stroke = Stroke::new(1.6, Color32::WHITE);
        // A pencil.
        let c = edit.rect.center();
        painter.line_segment([c + vec2(-4.0, 4.0), c + vec2(3.0, -3.0)], stroke);
        painter.line_segment([c + vec2(-4.0, 4.0), c + vec2(-4.5, 1.5)], stroke);
        painter.line_segment([c + vec2(-4.0, 4.0), c + vec2(-1.5, 4.5)], stroke);
        // A cross.
        let c = remove.rect.center();
        painter.line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], stroke);
        painter.line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], stroke);
    }
    let edit = edit.on_hover_text(tr("Change this pedal"));
    pedal_menu(params, slot, state, &edit, shown);
    if remove.on_hover_text(tr("Remove this pedal")).clicked() {
        let order: Vec<usize> = shown.iter().copied().filter(|&s| s != slot).collect();
        rearrange(params, &order);
    }
    body.on_hover_text(trf!(
        "Based on the {gear}\nDrag to move it on the pedalboard",
        gear = tr(pedal_based_on(kind))
    ))
}

fn pedal_menu(
    params: &mut Params,
    slot: usize,
    state: &mut RigState,
    response: &Response,
    shown: &[usize],
) {
    egui::Popup::menu(response).show(|ui| {
        ui.set_min_width(200.0);
        for (family, kinds) in PEDAL_GROUPS {
            ui.label(RichText::new(tr(family)).size(10.0).weak());
            for &kind in kinds {
                let based = trf!("Based on the {gear}", gear = tr(pedal_based_on(kind)));
                if ui.button(tr(PEDALS[kind])).on_hover_text(based).clicked() {
                    set_now(params, &slot_id(slot, "type"), kind as f32, response);
                    ui.close();
                }
            }
            ui.separator();
        }
        if ui.button(tr("Capture from the library…")).clicked() {
            state.picker = Some(Picker::Pedal(slot));
            state.filter.clear();
            ui.close();
        }
        let Some(at) = shown.iter().position(|&s| s == slot) else {
            return;
        };
        ui.separator();
        let mut order = shown.to_vec();
        if at > 0 && ui.button(tr("Move left")).clicked() {
            order.swap(at, at - 1);
            rearrange(params, &order);
            ui.close();
        }
        if at + 1 < shown.len() && ui.button(tr("Move right")).clicked() {
            order.swap(at, at + 1);
            rearrange(params, &order);
            ui.close();
        }
        if ui.button(tr("Remove pedal")).clicked() {
            order.remove(at);
            rearrange(params, &order);
            ui.close();
        }
    });
}

/// `text` cut with an ellipsis so that it fits `width` in `font`.
fn fit(painter: &egui::Painter, text: &str, font: &FontId, width: f32) -> String {
    let wide = |s: String| {
        painter
            .layout_no_wrap(s, font.clone(), Color32::WHITE)
            .size()
            .x
    };
    if wide(text.to_owned()) <= width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |n: usize| chars[..n].iter().collect::<String>().trim_end().to_owned() + "…";
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if wide(cut(mid)) <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    cut(low)
}

/// The name of the captures loaded in `slot`: their library title, else their folder (a
/// download is named after its tone), else what the captures say.
fn capture_name(params: &Params, slot: &str) -> Option<String> {
    let stored = routing::find_insert(params.doc.project(), params.channel, params.insert)
        .and_then(|i| i.files.get(slot))
        .cloned()?;
    let name = stored.file_name()?.to_owned();
    if let Some(packs) = captures::cached()
        && let Some(pack) = packs.iter().find(|p| p.path.file_name() == Some(&name))
    {
        return Some(pack.title.clone());
    }
    if stored.extension().is_none() {
        return Some(name.to_string_lossy().into_owned());
    }
    match params.doc.player().effect_file(params.insert, slot) {
        Some(Ok(text)) if !text.is_empty() && !text.starts_with(|c: char| c.is_ascii_digit()) => {
            Some(text.split(" · ").next().unwrap_or(text).to_owned())
        }
        _ => stored.file_stem().map(|s| s.to_string_lossy().into_owned()),
    }
}

/// The style of the loaded amp, from its name.
/// The built-in amp in use, if not the captures.
fn builtin(params: &Params) -> Option<&'static audiostrada_dsp::amp::Model> {
    let model = params.get("model").round() as usize;
    model.checked_sub(1).and_then(|i| MODELS.get(i))
}

fn amp_style(params: &Params) -> Style {
    builtin(params)
        .map(|m| Style::from_face(m.face))
        .unwrap_or_else(|| {
            capture_name(params, "amp").map_or(Style::Blackface, |name| Style::guess(&name, None))
        })
}

/// What the amp arrows step through: the built-in amps, then the favourite captures.
#[derive(Clone, Debug, PartialEq)]
enum AmpChoice {
    Model(usize),
    Pack(PathBuf, Category),
}

fn amp_choices(ctx: &egui::Context) -> Vec<AmpChoice> {
    let mut choices: Vec<AmpChoice> = FAMILIES
        .iter()
        .flat_map(|(_, models)| models.iter().map(|&m| AmpChoice::Model(m)))
        .collect();
    if let Some(packs) = captures::packs(ctx, false) {
        choices.extend(
            packs
                .iter()
                .filter(|p| p.favorite && p.category.place() == Place::Amp)
                .map(|p| AmpChoice::Pack(p.path.clone(), p.category)),
        );
    }
    choices
}

/// Where the amp in use is among `choices`.
fn current_choice(params: &Params, choices: &[AmpChoice]) -> Option<usize> {
    let model = params.get("model").round() as usize;
    if model > 0 {
        return choices.iter().position(|c| *c == AmpChoice::Model(model));
    }
    let stored = routing::find_insert(params.doc.project(), params.channel, params.insert)
        .and_then(|i| i.files.get("amp"))
        .and_then(|p| p.file_name().map(|n| n.to_owned()))?;
    choices.iter().position(|c| match c {
        AmpChoice::Pack(path, _) => path.file_name() == Some(stored.as_os_str()),
        AmpChoice::Model(_) => false,
    })
}

/// Switches to a built-in amp and its cabinet (unless an impulse response is in use).
fn set_model(ui: &Ui, params: &mut Params, model: usize) {
    let response = ui.interact(
        Rect::NOTHING,
        ui.scope_id().with("rig-model"),
        Sense::hover(),
    );
    set_now(params, "model", model as f32, &response);
    let impulse = params.get("cab").round() as usize == CABS.len() - 1;
    if let (false, Some(m)) = (impulse, MODELS.get(model.wrapping_sub(1))) {
        set_now(params, "cab", m.cab as f32, &response);
    }
}

fn apply_choice(ui: &Ui, params: &mut Params, choice: &AmpChoice) {
    match choice {
        AmpChoice::Model(m) => set_model(ui, params, *m),
        AmpChoice::Pack(path, category) => load_into(
            ui,
            params,
            Picker::Amp,
            path,
            *category == Category::FullRig,
        ),
    }
}

/// A round arrow button of a carousel; true when clicked.
fn arrow(ui: &mut Ui, center: Pos2, left: bool, key: &str, hint: &str) -> bool {
    let rect = Rect::from_center_size(center, vec2(30.0, 30.0));
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    let p = theme::current(ui);
    let painter = ui.painter();
    let fill = if response.hovered() {
        p.accent
    } else {
        p.hover
    };
    painter.circle_filled(center, 15.0, fill);
    let d = if left { -1.0 } else { 1.0 };
    let stroke = Stroke::new(2.5, p.text);
    painter.line_segment(
        [center + vec2(-3.0 * d, -7.0), center + vec2(4.0 * d, 0.0)],
        stroke,
    );
    painter.line_segment(
        [center + vec2(4.0 * d, 0.0), center + vec2(-3.0 * d, 7.0)],
        stroke,
    );
    response.on_hover_text(hint).clicked()
}

fn amp(ui: &mut Ui, params: &mut Params, state: &mut RigState, area: Rect) {
    let rect = area.shrink2(vec2(36.0, 0.0));
    let choices = amp_choices(ui.ctx());
    let at = current_choice(params, &choices);
    let step = |delta: isize| {
        let n = choices.len() as isize;
        let from = at.map_or(if delta > 0 { -1 } else { 0 }, |a| a as isize);
        (from + delta).rem_euclid(n) as usize
    };
    if arrow(
        ui,
        pos2(area.left() + 16.0, area.center().y),
        true,
        "amp-prev",
        tr("Previous amp"),
    ) {
        let choice = choices[step(-1)].clone();
        apply_choice(ui, params, &choice);
    }
    if arrow(
        ui,
        pos2(area.right() - 16.0, area.center().y),
        false,
        "amp-next",
        tr("Next amp"),
    ) {
        let choice = choices[step(1)].clone();
        apply_choice(ui, params, &choice);
    }
    let style = amp_style(params);
    let look = style.look();
    let loaded = params
        .doc
        .player()
        .effect_file(params.insert, "amp")
        .cloned();
    let captures = params
        .doc
        .player()
        .effect_file(params.insert, "amp")
        .and_then(|r| r.as_ref().ok())
        .and_then(|t| {
            t.rsplit(" · ")
                .next()
                .and_then(|c| c.split(' ').next())
                .and_then(|n| n.parse::<usize>().ok())
        });
    let built_in = builtin(params);
    let name = match built_in {
        Some(model) => model.name.to_owned(),
        None => capture_name(params, "amp").unwrap_or_else(|| tr("Choose an amp").into()),
    };
    let painter = ui.painter().clone();
    let shown = fit(&painter, &name, &theme::strong(18.0), rect.width() * 0.6);
    let head = crate::gear::head(&painter, rect, &look, &shown);
    // The name plate opens the amp picker.
    let plate = ui.interact(head.plate, ui.scope_id().with("amp-plate"), Sense::click());
    if plate.hovered() {
        painter.rect_stroke(
            head.plate.expand(2.0),
            5.0,
            Stroke::new(1.5, Color32::from_white_alpha(120)),
            egui::StrokeKind::Outside,
        );
    }
    let details = match &loaded {
        _ if built_in.is_some() => match at {
            Some(i) => trf!(
                "Built-in · {n} of {count} · click for the list",
                n = i + 1,
                count = choices.len()
            ),
            None => tr("Built-in · click for the list").to_owned(),
        },
        None => tr("No amp loaded · click to choose one").to_owned(),
        Some(Err(e)) => trf!("Cannot load: {error}", error = e),
        Some(Ok(_)) => trf!(
            "{style} style · {captures} · click to change",
            style = style.name(),
            captures = match captures {
                Some(n) if n > 1 => trf!("{n} captures", n = n),
                _ => tr("1 capture").to_owned(),
            }
        ),
    };
    let hint = match built_in {
        Some(model) => trf!(
            "Based on the {gear}\nClick to choose the amp",
            gear = model.based_on
        ),
        None if shown != name => trf!("{name}\nClick to choose the amp", name = name),
        None => tr("Choose the amp").to_owned(),
    };
    if plate.on_hover_text(format!("{hint}\n{details}")).clicked() {
        state.picker = Some(Picker::Amp);
        state.filter.clear();
    }
    let p = theme::current(ui);
    painter.text(
        pos2(area.center().x, area.bottom() + 2.0),
        Align2::CENTER_TOP,
        &details,
        FontId::proportional(10.0),
        p.weak,
    );
    crate::gear::lamp(&painter, head.lamp, head.panel.height() * 0.13, look.lamp);
    painter.text(
        head.lamp + vec2(0.0, head.panel.height() * 0.22),
        Align2::CENTER_TOP,
        tr("ON"),
        FontId::proportional(8.5),
        look.letters,
    );
    // Knobs: Gain steps through the captures, the others shape the tone after the amp.
    let knobs = [
        ("gain", tr("GAIN")),
        ("bass", tr("BASS")),
        ("mid", tr("MIDDLE")),
        ("treble", tr("TREBLE")),
        ("presence", tr("PRESENCE")),
        ("master", tr("MASTER")),
    ];
    let start = head.panel.left() + head.panel.width() * 0.04;
    let end = head.lamp.x - head.panel.height() * 0.5;
    let step = (end - start) / knobs.len() as f32;
    let diameter = (step * 0.5).min(head.panel.height() * 0.46);
    for (i, (id, label)) in knobs.iter().enumerate() {
        let center = pos2(
            start + step * (i as f32 + 0.5),
            head.panel.center().y + head.panel.height() * 0.06,
        );
        // Master is in dB; the face shows it from 0 to 10.
        let value = if *id == "master" {
            (params.get(id) + 24.0) / 3.6
        } else {
            params.get(id)
        };
        let (changed, response) = face_knob(ui, center, diameter, value, &look, ("amp-knob", i));
        let hint = match *id {
            "gain" if built_in.is_some() => tr("Gain").to_owned(),
            "gain" => match captures {
                Some(n) if n > 1 => {
                    trf!("Gain: steps through the {n} captures of this amp", n = n)
                }
                _ => tr("Gain: this amp has a single capture (load a pack with several gains)")
                    .into(),
            },
            "master" => trf!(
                "Master {value} dB",
                value = format!("{:+.1}", params.get(id))
            ),
            _ => trf!(
                "{label}: {value} (applied after the amp)",
                label = label,
                value = format!("{value:.1}")
            ),
        };
        let response = response.on_hover_text(hint);
        let changed = changed.map(|v| if *id == "master" { v * 3.6 - 24.0 } else { v });
        params.set(id, changed, &response);
        painter.text(
            center - vec2(0.0, diameter / 2.0 + 10.0),
            Align2::CENTER_BOTTOM,
            *label,
            FontId::proportional(9.0),
            look.letters,
        );
    }
}

/// A DI box, or an impulse response, drawn in `area`.
fn draw_cab(painter: &egui::Painter, area: Rect, cab: usize) {
    let c = Color32::from_rgb;
    // Direct: a DI box; impulse response: a box with a waveform.
    let rect = Rect::from_center_size(area.center(), vec2(110.0, 64.0));
    painter.rect_filled(rect, 6.0, c(70, 74, 82));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_white_alpha(40)),
        egui::StrokeKind::Inside,
    );
    if cab == CABS.len() - 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "DI",
            theme::strong(20.0),
            c(230, 230, 230),
        );
    } else {
        let points: Vec<Pos2> = (0..60)
            .map(|i| {
                let t = i as f32 / 59.0;
                let decay = (-t * 5.0).exp();
                pos2(
                    rect.left() + 10.0 + t * (rect.width() - 20.0),
                    rect.center().y + (t * 60.0).sin() * 22.0 * decay,
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(1.5, c(120, 200, 255)),
        ));
    }
}

/// The size a cabinet is drawn at in `width`: its own proportions, big.
fn cab_size(cab: usize, width: f32) -> egui::Vec2 {
    match crate::gear::cab_ratio(cab) {
        Some(ratio) => {
            let height = if ratio < 0.8 { 400.0 } else { 300.0 };
            let size = vec2(height * ratio, height);
            size * ((width - 140.0) / size.x).min(1.0)
        }
        None => vec2(240.0, 140.0),
    }
}

fn cabinet(ui: &mut Ui, params: &mut Params, area: Rect, size: egui::Vec2) {
    let p = theme::current(ui);
    let cab = (params.get("cab").round().max(0.0) as usize).min(CABS.len() - 1);
    let n = CABS.len();
    let response = ui.interact(Rect::NOTHING, ui.scope_id().with("cab-set"), Sense::hover());
    let drawing = Rect::from_center_size(pos2(area.center().x, area.top() + size.y / 2.0), size);
    let middle = drawing.center().y;
    if arrow(
        ui,
        pos2(drawing.left() - 30.0, middle),
        true,
        "cab-prev",
        tr("Previous cabinet"),
    ) {
        set_now(params, "cab", ((cab + n - 1) % n) as f32, &response);
    }
    if arrow(
        ui,
        pos2(drawing.right() + 30.0, middle),
        false,
        "cab-next",
        tr("Next cabinet"),
    ) {
        set_now(params, "cab", ((cab + 1) % n) as f32, &response);
    }
    let pick = ui.interact(drawing, ui.scope_id().with("cab-pick"), Sense::click());
    if crate::gear::cab_ratio(cab).is_some() {
        crate::gear::cabinet(ui.painter(), drawing, cab, &Style::of_cab(cab).look());
    } else {
        draw_cab(ui.painter(), drawing, cab);
    }
    let pick = pick.on_hover_text(tr("Click to choose the cabinet"));
    egui::Popup::menu(&pick).show(|ui| {
        for (i, name) in CABS.iter().enumerate() {
            if ui.selectable_label(i == cab, tr(name)).clicked() {
                set_now(params, "cab", i as f32, &response);
                ui.close();
            }
        }
    });
    let label = pos2(area.center().x, drawing.bottom() + 10.0);
    if cab == n - 1 {
        let loaded = capture_name(params, "ir");
        let text = loaded
            .as_deref()
            .map_or(tr("Load impulse…").to_owned(), |name| {
                format!("IR: {name}")
            });
        let button = Rect::from_center_size(label + vec2(0.0, 10.0), vec2(220.0, 22.0));
        let open = ui.put(button, egui::Button::new(text)).clicked();
        let id = ("rig-ir", params.channel, params.insert);
        if let Some(file) = crate::dialogs::ask(ui.ctx(), id, open, || {
            rfd::FileDialog::new()
                .add_filter(tr("Impulse response"), &["wav", "flac", "aif", "aiff"])
                .pick_file()
        }) && let Err(e) =
            params
                .doc
                .set_effect_file(params.channel, params.insert, "ir", Some(&file))
        {
            params.doc.notify(trf!(
                "Cannot load {file}: {error}",
                file = file.display(),
                error = e
            ));
        }
    } else {
        ui.painter().text(
            label,
            Align2::CENTER_TOP,
            tr(CABS[cab]),
            theme::strong(13.0),
            p.text,
        );
    }
}

/// Loads captures into the slot the picker was opened for.
fn load_into(ui: &Ui, params: &mut Params, target: Picker, path: &std::path::Path, full_rig: bool) {
    let slot = match target {
        Picker::Amp => "amp".to_owned(),
        Picker::Pedal(i) => format!("pedal{}", i + 1),
        Picker::Cab => "ir".to_owned(),
    };
    // A downloaded cabinet is a folder of impulses: the first one.
    let first_wav = (target == Picker::Cab && path.is_dir())
        .then(|| {
            let mut files: Vec<PathBuf> = std::fs::read_dir(path)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
                .collect();
            files.sort();
            files.into_iter().next()
        })
        .flatten();
    let path = first_wav.as_deref().unwrap_or(path);
    if let Err(e) = params
        .doc
        .set_effect_file(params.channel, params.insert, &slot, Some(path))
    {
        params.doc.notify(trf!(
            "Cannot load {file}: {error}",
            file = path.display(),
            error = e
        ));
        return;
    }
    let response = ui.interact(
        Rect::NOTHING,
        ui.scope_id().with("rig-load"),
        Sense::hover(),
    );
    match target {
        Picker::Pedal(i) => {
            set_now(params, &slot_id(i, "type"), 12.0, &response);
            set_now(params, &slot_id(i, "on"), 1.0, &response);
        }
        Picker::Cab => set_now(params, "cab", (CABS.len() - 1) as f32, &response),
        // A full rig already has its cabinet.
        Picker::Amp => {
            set_now(params, "model", 0.0, &response);
            if full_rig {
                set_now(params, "cab", (CABS.len() - 2) as f32, &response);
            }
        }
    }
}

/// Follows a TONE3000 sign-in or download, even with the picker closed.
fn poll_session(ui: &Ui, params: &mut Params, state: &mut RigState) {
    let Some((session, target, category)) = &state.session else {
        return;
    };
    let (target, category) = (*target, *category);
    match session.status() {
        Status::Done { folder, .. } => {
            state.session = None;
            let category = category.or_else(|| captures::category_of(&folder));
            if let Some(category) = category {
                captures::note(&folder, |n| n.category = Some(category));
            }
            captures::invalidate();
            match target.or_else(|| category.and_then(|c| destination(params, c))) {
                Some(target) => load_into(
                    ui,
                    params,
                    target,
                    &folder,
                    category == Some(Category::FullRig),
                ),
                None => {
                    state.info =
                        Some(tr("Downloaded into the library: the pedalboard is full.").into())
                }
            }
            state.picker = None;
        }
        Status::SignedIn => {
            state.session = None;
            state.feeds.clear();
            state.user = None;
        }
        Status::Failed(e) => {
            state.session = None;
            state.message = Some(e);
        }
        _ => ui
            .ctx()
            .request_repaint_after(std::time::Duration::from_millis(200)),
    }
}

/// The answer of a background request, asking for frames until it comes.
fn ready<T: Clone>(ctx: &egui::Context, pending: &Pending<T>) -> Option<Result<T, String>> {
    let value = pending.lock().ok().and_then(|r| r.clone());
    if value.is_none() {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
    value
}

enum Picture {
    Loading(Pending<([usize; 2], Vec<u8>)>),
    Ready(egui::TextureHandle),
    Failed,
}

static PICTURES: LazyLock<Mutex<HashMap<String, Picture>>> = LazyLock::new(Default::default);

/// A downloaded picture (tone photo, avatar), once it has arrived.
fn picture(ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
    let mut all = PICTURES.lock().ok()?;
    let entry = all
        .entry(url.to_owned())
        .or_insert_with(|| Picture::Loading(crate::tone3000::image(url.to_owned(), 160)));
    if let Picture::Loading(pending) = entry {
        match ready(ctx, pending) {
            Some(Ok((size, rgba))) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                *entry = Picture::Ready(ctx.load_texture(url, image, egui::TextureOptions::LINEAR));
            }
            Some(Err(_)) => *entry = Picture::Failed,
            None => {}
        }
    }
    match entry {
        Picture::Ready(texture) => Some(texture.clone()),
        _ => None,
    }
}

/// Paints a picture inside `rect`, keeping its proportions.
fn paint_picture(painter: &egui::Painter, rect: Rect, texture: &egui::TextureHandle) {
    let [w, h] = texture.size().map(|v| v as f32);
    let scale = (rect.width() / w).min(rect.height() / h);
    let fit = Rect::from_center_size(rect.center(), vec2(w, h) * scale);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(texture.id(), fit, uv, Color32::WHITE);
}

/// Where captures of a category go: the amp, the next free pedal slot, the cabinet.
fn destination(params: &Params, category: Category) -> Option<Picker> {
    match category.place() {
        Place::Amp => Some(Picker::Amp),
        Place::Cab => Some(Picker::Cab),
        Place::Pedalboard => free_slot(params).map(Picker::Pedal),
    }
}

fn start_tone3000(ctx: &egui::Context, state: &mut RigState, target: Option<Picker>) {
    let Some(root) = crate::nam_library::library_root() else {
        return;
    };
    state.message = None;
    let open = |url: &str| ctx.open_url(egui::OpenUrl::new_tab(url));
    let prompt = if crate::tone3000::signed_in() {
        crate::tone3000::Prompt::Select
    } else {
        crate::tone3000::Prompt::SignIn
    };
    match crate::tone3000::start(root, prompt, open) {
        Ok(session) => state.session = Some((session, target, None)),
        Err(e) => state.message = Some(e),
    }
}

fn picker(ui: &mut Ui, params: &mut Params, state: &mut RigState) {
    let Some(target) = state.picker else {
        return;
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    let mut chosen: Option<(PathBuf, bool)> = None;
    let mut picked_model = None;
    let place = target.place();
    let modal =
        egui::Modal::new(egui::Id::unique(("rig-picker", params.insert.0))).show(&ctx, |ui| {
            ui.set_width(860.0);
            ui.set_height(540.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(match place {
                        Place::Amp => tr("Choose an amp"),
                        Place::Pedalboard => tr("Choose an effect capture"),
                        Place::Cab => tr("Choose a cabinet impulse"),
                    })
                    .font(theme::strong(15.0)),
                );
                ui.add_space(12.0);
                if place != Place::Amp {
                    state.tab = Tab::Library;
                } else {
                    ui.selectable_value(&mut state.tab, Tab::BuiltIn, tr("Built-in"));
                }
                ui.selectable_value(&mut state.tab, Tab::Library, tr("My library"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Close")).clicked() {
                        open = false;
                    }
                    if ui.button(tr("Browse TONE3000…")).clicked() {
                        state.browser = Some(Browse {
                            kind: Kind::of(place),
                            list: List::Trending,
                        });
                        open = false;
                    }
                });
            });
            if let Some((session, _, _)) = &state.session {
                let text = match session.status() {
                    Status::Waiting => tr("TONE3000: finish in your browser…").to_owned(),
                    Status::Downloading(what) => what,
                    _ => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text);
                });
            }
            if let Some(message) = &state.message {
                ui.colored_label(theme::RECORD, message);
            }
            if let Some(info) = &state.info {
                ui.colored_label(theme::current(ui).accent, info);
            }
            ui.separator();
            if state.tab == Tab::BuiltIn {
                let current = params.get("model").round() as usize;
                if let Some(model) = builtin_list(ui, current) {
                    picked_model = Some(model);
                }
                return;
            }
            chosen = library(ui, state, place);
        });
    if modal.should_close() {
        open = false;
    }
    if let Some((path, full_rig)) = chosen {
        load_into(ui, params, target, &path, full_rig);
        open = false;
    }
    if let Some(model) = picked_model {
        set_model(ui, params, model);
        open = false;
    }
    if !open {
        state.picker = None;
    }
}

/// The "My library" tab; returns the pack clicked, and whether it is a full rig.
fn library(ui: &mut Ui, state: &mut RigState, place: Place) -> Option<(PathBuf, bool)> {
    let ctx = ui.ctx().clone();
    let mut rescan = false;
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text(tr("Search"))
                .desired_width(200.0),
        );
        let open = ui
            .button(tr("Add captures…"))
            .on_hover_text(tr(
                "Copy .nam files into AudioStrada's library, sorted by gear",
            ))
            .clicked();
        if let Some(files) = crate::dialogs::ask(ui.ctx(), "nam-captures", open, || {
            rfd::FileDialog::new()
                .add_filter(tr("NAM captures"), &["nam"])
                .pick_files()
        }) {
            match captures::import(&files) {
                Ok(added) => {
                    let names: Vec<String> = added
                        .iter()
                        .map(|(name, category)| format!("{name} ({})", category.name()))
                        .collect();
                    state.message = None;
                    state.info = Some(trf!(
                        "Added: {names}. Right-click a card to rename it or change its category.",
                        names = names.join(", ")
                    ));
                    rescan = true;
                }
                Err(e) => state.message = Some(e),
            }
        }
        let open = ui
            .button(tr("Add folder…"))
            .on_hover_text(tr("Use the captures of a folder where they are"))
            .clicked();
        if let Some(dir) = crate::dialogs::ask(ui.ctx(), "nam-folder", open, || {
            rfd::FileDialog::new().pick_folder()
        }) {
            crate::nam_library::add_dir(dir);
            rescan = true;
        }
        if ui.button(tr("Rescan")).clicked() {
            rescan = true;
        }
    });
    if let Some((path, name)) = &mut state.renaming {
        let mut done = None;
        ui.horizontal(|ui| {
            ui.label(tr("Rename:"));
            let edit = ui.add(egui::TextEdit::singleline(name).desired_width(260.0));
            edit.request_focus();
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(tr("OK")).clicked() || enter {
                done = Some(true);
            }
            if ui.button(tr("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        });
        match done {
            Some(true) => {
                let name = name.trim().to_owned();
                captures::note(path, |n| n.name = (!name.is_empty()).then_some(name));
                state.renaming = None;
            }
            Some(false) => state.renaming = None,
            None => {}
        }
    }
    let Some(all) = captures::packs(&ctx, rescan) else {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(tr("Looking for captures…"));
        });
        return None;
    };
    let fits = |pack: &Pack| pack.category.place() == place;
    let filter = state.filter.to_lowercase();
    let shown: Vec<&Pack> = all
        .iter()
        .filter(|p| fits(p))
        .filter(|p| !state.favorites || p.favorite)
        .filter(|p| state.category.is_none_or(|c| p.category == c))
        .filter(|p| {
            filter.is_empty()
                || p.title.to_lowercase().contains(&filter)
                || p.detail.to_lowercase().contains(&filter)
        })
        .collect();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(140.0);
            let count = |c: Option<Category>, fav: bool| {
                all.iter()
                    .filter(|p| fits(p))
                    .filter(|p| !fav || p.favorite)
                    .filter(|p| c.is_none_or(|c| p.category == c))
                    .count()
            };
            let mut entry = |ui: &mut Ui, label: String, c: Option<Category>, fav: bool| {
                let selected = state.category == c && state.favorites == fav;
                if ui.selectable_label(selected, label).clicked() {
                    state.category = c;
                    state.favorites = fav;
                }
            };
            entry(ui, trf!("All  {n}", n = count(None, false)), None, false);
            entry(
                ui,
                trf!("★ Favorites  {n}", n = count(None, true)),
                None,
                true,
            );
            for c in Category::ALL.into_iter().filter(|c| c.place() == place) {
                entry(
                    ui,
                    format!("{}  {}", c.name(), count(Some(c), false)),
                    Some(c),
                    false,
                );
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.weak(tr(
                        "Nothing here yet: Add captures… copies .nam files into the library, \
                         the TONE3000 tab downloads tones.",
                    ));
                }
                let width = ui.available_width();
                let columns = ((width / 250.0).floor() as usize).max(1);
                let card_width = width / columns as f32 - 8.0;
                for row in shown.chunks(columns) {
                    ui.horizontal(|ui| {
                        for pack in row {
                            match card(ui, pack, card_width) {
                                CardAction::Pick => {
                                    chosen = Some((
                                        pack.path.clone(),
                                        pack.category == Category::FullRig,
                                    ))
                                }
                                CardAction::Star => {
                                    let favorite = !pack.favorite;
                                    captures::note(&pack.path, |n| n.favorite = favorite);
                                }
                                CardAction::Rename => {
                                    state.renaming = Some((pack.path.clone(), pack.title.clone()))
                                }
                                CardAction::Category(c) => {
                                    captures::note(&pack.path, |n| n.category = Some(c))
                                }
                                CardAction::Remove => {
                                    if let Err(e) = captures::remove(pack) {
                                        state.message = Some(e);
                                    }
                                }
                                CardAction::None => {}
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
            });
    });
    chosen
}

enum CardAction {
    None,
    Pick,
    Star,
    Rename,
    Category(Category),
    Remove,
}

/// Built-in amps by family (indices in the rig's amp models).
const FAMILIES: [(&str, &[usize]); 6] = [
    ("American clean", &[1, 2, 3]),
    ("Tweed", &[4, 5]),
    ("British", &[6, 7, 8, 9, 10]),
    ("Boutique", &[11]),
    ("High gain", &[12, 13, 14]),
    ("Bass amps", &[15, 16]),
];

/// The built-in amps; returns the model clicked (index in the rig's amp models).
fn builtin_list(ui: &mut Ui, current: usize) -> Option<usize> {
    let p = theme::current(ui);
    ui.weak(tr(
        "Amps modeled by AudioStrada after the most played ones: preamp, tone stack and power \
         stage. For the exact sound of one amp, load its NAM captures (My library, TONE3000).",
    ));
    ui.add_space(4.0);
    let mut picked = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let columns = 2;
            let card_width = width / columns as f32 - 8.0;
            for (family, models) in FAMILIES {
                ui.label(
                    RichText::new(tr(family))
                        .font(theme::strong(12.0))
                        .color(p.weak),
                );
                for row in models.chunks(columns) {
                    ui.horizontal(|ui| {
                        for &index in row {
                            let Some(model) = MODELS.get(index - 1) else {
                                continue;
                            };
                            if amp_card(ui, model, card_width, index == current) {
                                picked = Some(index);
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(4.0);
            }
        });
    picked
}

/// A built-in amp's card; true when clicked.
fn amp_card(ui: &mut Ui, model: &audiostrada_dsp::amp::Model, width: f32, selected: bool) -> bool {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 92.0), Sense::click());
    let fill = if selected || response.hovered() {
        p.accent.gamma_multiply(if selected { 0.3 } else { 0.18 })
    } else {
        p.base
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, fill);
    let thumb = Rect::from_min_size(rect.min + vec2(8.0, 14.0), vec2(80.0, 64.0));
    face_thumb(painter, thumb, false, Style::from_face(model.face));
    let left = rect.left() + 98.0;
    painter.text(
        pos2(left, rect.top() + 10.0),
        Align2::LEFT_TOP,
        model.name,
        theme::strong(14.0),
        p.text,
    );
    painter.text(
        pos2(left, rect.top() + 30.0),
        Align2::LEFT_TOP,
        trf!("Based on the {gear}", gear = model.based_on),
        FontId::proportional(10.0),
        p.accent,
    );
    let galley = painter.layout(
        tr(model.blurb).to_owned(),
        FontId::proportional(11.0),
        p.weak,
        rect.right() - left - 8.0,
    );
    painter.galley(pos2(left, rect.top() + 48.0), galley, p.weak);
    response.clicked()
}

/// The TONE3000 browser: amps, effects and cabinets, trending, latest and the user's lists
/// loaded as they are scrolled; a click downloads a tone and loads it where it belongs.
fn browser(ui: &mut Ui, params: &mut Params, state: &mut RigState) {
    let Some(mut browse) = state.browser else {
        return;
    };
    let ctx = ui.ctx().clone();
    let p = theme::current(ui);
    let mut open = true;
    let mut picked: Option<Tone> = None;
    let signed_in = crate::tone3000::signed_in();
    let modal =
        egui::Modal::new(egui::Id::unique(("tone3000-browser", params.insert.0))).show(&ctx, |ui| {
            ui.set_width(980.0);
            ui.set_height(640.0);
            // The header: the service, the account, the search on the site, closing.
            ui.horizontal(|ui| {
                ui.label(RichText::new("TONE3000").font(theme::strong(20.0)).color(p.accent));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(tr("Free amp, effect and cabinet captures"))
                        .size(12.0)
                        .color(p.weak),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("✕").size(14.0)).frame(false))
                        .on_hover_text(tr("Close"))
                        .clicked()
                    {
                        open = false;
                    }
                    ui.add_space(6.0);
                    if ui
                        .button(tr("Search all of TONE3000…"))
                        .on_hover_text(tr("Search the whole catalog in your browser; the tone you pick is downloaded and loaded here"))
                        .clicked()
                    {
                        start_tone3000(&ctx, state, None);
                    }
                    if signed_in {
                        if ui.small_button(tr("Sign out")).clicked() {
                            crate::tone3000::sign_out();
                            state.feeds.clear();
                            state.user = None;
                        }
                        let user = state.user.get_or_insert_with(crate::tone3000::user);
                        if let Some(Ok(user)) = ready(&ctx, user) {
                            ui.label(&user.name);
                            avatar(ui, user.avatar.as_deref(), 22.0);
                        }
                    }
                });
            });
            if let Some((session, _, _)) = &state.session {
                let text = match session.status() {
                    Status::Waiting => tr("TONE3000: finish in your browser…").to_owned(),
                    Status::Downloading(what) => what,
                    _ => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(text);
                });
            }
            if let Some(message) = &state.message {
                ui.colored_label(theme::RECORD, message);
            }
            if let Some(info) = &state.info {
                ui.colored_label(p.accent, info);
            }
            ui.add_space(4.0);
            if !signed_in {
                sign_in_card(ui, &ctx, state);
                return;
            }
            ui.horizontal_top(|ui| {
                // The sidebar: what to look for, and in which list.
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    ui.label(RichText::new(tr("Gear")).size(11.0).color(p.weak));
                    for kind in Kind::ALL {
                        if side_item(ui, browse.kind == kind, kind.name()) {
                            browse.kind = kind;
                        }
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new(tr("Lists")).size(11.0).color(p.weak));
                    for list in List::ALL {
                        if side_item(ui, browse.list == list, list.name()) {
                            browse.list = list;
                        }
                    }
                    ui.add_space(10.0);
                    if ui.small_button(tr("Refresh")).clicked() {
                        state.feeds.remove(&(browse.kind, browse.list));
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.search)
                            .hint_text(tr("Filter this list by name or creator…"))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    let filter = state.search.to_lowercase();
                    let feed = state.feeds.entry((browse.kind, browse.list)).or_default();
                    feed.poll(&ctx, browse.kind, browse.list);
                    if feed.tones.is_empty() && !feed.done {
                        feed.more(browse.kind, browse.list);
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let width = ui.available_width();
                            let columns = ((width / 230.0).floor() as usize).max(1);
                            let card_width = (width - 10.0 * (columns - 1) as f32) / columns as f32;
                            let shown: Vec<&Tone> = feed
                                .tones
                                .iter()
                                .filter(|t| {
                                    filter.is_empty()
                                        || t.title.to_lowercase().contains(&filter)
                                        || t.creator.to_lowercase().contains(&filter)
                                })
                                .collect();
                            for row in shown.chunks(columns) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 10.0;
                                    for tone in row {
                                        if tone_card(ui, tone, card_width, browse.kind) {
                                            picked = Some((*tone).clone());
                                        }
                                    }
                                });
                                ui.add_space(10.0);
                            }
                            if let Some(e) = &feed.error {
                                ui.colored_label(theme::RECORD, e);
                            } else if feed.pending.is_some() {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(tr("Loading…"));
                                });
                            } else if feed.done && shown.is_empty() {
                                ui.weak(tr("Nothing in this list."));
                            } else if !feed.done {
                                // Reaching the end of the list loads the next page.
                                let end = ui.allocate_response(vec2(width, 20.0), Sense::hover());
                                if ui.is_rect_visible(end.rect) {
                                    feed.more(browse.kind, browse.list);
                                }
                            }
                        });
                });
            });
        });
    if modal.should_close() {
        open = false;
    }
    if let Some(tone) = picked {
        let category = Category::from_gear(&tone.gear);
        let target = destination(params, category);
        match (target, crate::nam_library::library_root()) {
            (None, _) => {
                state.message = Some(tr("The pedalboard is full: remove a pedal first.").into())
            }
            (Some(target), Some(root)) => {
                state.message = None;
                state.info = None;
                let session = crate::tone3000::download(root, tone.id, tone.title.clone());
                state.session = Some((session, Some(target), Some(category)));
            }
            (Some(_), None) => {}
        }
    }
    state.browser = open.then_some(browse);
}

/// An entry of the browser's sidebar; true when clicked.
fn side_item(ui: &mut Ui, on: bool, text: &str) -> bool {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let fill = if on {
        p.accent.gamma_multiply(0.25)
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    if on {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(3.0, rect.height() - 12.0)),
            2.0,
            p.accent,
        );
    }
    ui.painter().text(
        rect.left_center() + vec2(12.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        if on {
            theme::strong(13.0)
        } else {
            FontId::proportional(13.0)
        },
        if on {
            p.text
        } else {
            p.text.gamma_multiply(0.85)
        },
    );
    response.clicked()
}

/// Before signing in: what TONE3000 is, a rig drawn, and the button.
fn sign_in_card(ui: &mut Ui, ctx: &egui::Context, state: &mut RigState) {
    let p = theme::current(ui);
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(360.0, 220.0), Sense::hover());
        let painter = ui.painter();
        let look = Style::Plexi.look();
        let head = Rect::from_min_size(rect.min + vec2(40.0, 14.0), vec2(280.0, 80.0));
        let shown = crate::gear::head(painter, head, &look, "TONE3000");
        for k in 0..5 {
            let x = shown.panel.left() + 22.0 + k as f32 * 38.0;
            crate::gear::knob(
                painter,
                pos2(x, shown.panel.center().y + 2.0),
                16.0,
                0.3 + 0.1 * k as f32,
                look.knob,
                look.letters,
            );
        }
        let cab = Rect::from_min_size(
            pos2(head.left() + 40.0, head.bottom() + 4.0),
            vec2(200.0, 110.0),
        );
        crate::gear::cabinet(painter, cab, 2, &Style::Voxy.look());
        ui.add_space(14.0);
        ui.label(
            RichText::new(tr(
                "TONE3000 shares thousands of free amp, effect and cabinet captures. \
                 Sign in (free account) to browse them here.",
            ))
            .size(14.0),
        );
        ui.add_space(12.0);
        if theme::primary_button(ui, tr("Sign in to TONE3000…")).clicked() {
            start_tone3000(ctx, state, None);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(tr("Your browser opens; come back here once signed in."))
                .size(11.0)
                .color(p.weak),
        );
    });
}

fn avatar(ui: &mut Ui, url: Option<&str>, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_avatar(ui, rect, url);
}

fn paint_avatar(ui: &Ui, rect: Rect, url: Option<&str>) {
    let painter = ui.painter();
    match url.and_then(|u| picture(ui.ctx(), u)) {
        Some(texture) => paint_picture(painter, rect, &texture),
        None => {
            painter.circle_filled(rect.center(), rect.width() / 2.0, theme::current(ui).hover);
        }
    }
}

fn short(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..1_000_000 => format!("{:.1}k", n as f64 / 1000.0),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// A TONE3000 tone: its photo, title, gear, format, license and creator; true when clicked.
fn tone_card(ui: &mut Ui, tone: &Tone, width: f32, kind: Kind) -> bool {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 236.0), Sense::click());
    let hot = response.hovered();
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, 8.0, p.base);
    if hot {
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.5, p.accent),
            egui::StrokeKind::Inside,
        );
    }
    // The picture: the creator's photo, or the gear drawn.
    let thumb = Rect::from_min_size(rect.min, vec2(width, 128.0)).shrink(1.0);
    crate::gear::sheen(
        &painter,
        thumb,
        Color32::from_rgb(44, 46, 52),
        Color32::from_rgb(22, 23, 26),
    );
    match tone.image.as_deref().and_then(|u| picture(ui.ctx(), u)) {
        Some(texture) => cover_picture(&painter, thumb, &texture),
        None => gear_thumb(&painter, thumb.shrink(14.0), kind, &tone.title),
    }
    if hot {
        painter.rect_filled(thumb, 0.0, Color32::from_black_alpha(90));
        let button = Rect::from_center_size(thumb.center(), vec2(110.0, 30.0));
        painter.rect_filled(button, 15.0, p.accent);
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            tr("Load"),
            theme::strong(13.0),
            Color32::WHITE,
        );
    }
    let left = rect.left() + 10.0;
    let mut job = egui::text::LayoutJob::simple(
        tone.title.clone(),
        theme::strong(13.0),
        p.text,
        width - 20.0,
    );
    job.wrap.max_rows = 2;
    job.wrap.overflow_character = Some('…');
    let title = painter.layout_job(job);
    painter.galley(pos2(left, thumb.bottom() + 8.0), title, p.text);
    // What it is, as small chips.
    let gear = match tone.gear.as_str() {
        "amp-cab" | "full-rig" => tr("Full rig"),
        "amp" => tr("Amp"),
        "pedal" => tr("Pedal"),
        "outboard" => tr("Outboard"),
        "cab" => tr("Cabinet"),
        other => other,
    };
    let mut x = left;
    let chip_y = rect.bottom() - 56.0;
    for text in [
        gear.to_owned(),
        if tone.models > 1 {
            trf!("{n} captures", n = tone.models)
        } else {
            tr("1 capture").to_owned()
        },
        tone.license.to_uppercase(),
    ] {
        if text.is_empty() {
            continue;
        }
        let galley = painter.layout_no_wrap(text, FontId::proportional(10.0), p.text);
        let chip = Rect::from_min_size(pos2(x, chip_y), galley.size() + vec2(12.0, 6.0));
        if chip.right() > rect.right() - 8.0 {
            break;
        }
        painter.rect_filled(chip, 9.0, p.panel);
        painter.galley(chip.min + vec2(6.0, 3.0), galley, p.text);
        x = chip.right() + 5.0;
    }
    let face = Rect::from_min_size(pos2(left, rect.bottom() - 26.0), vec2(18.0, 18.0));
    paint_avatar(ui, face, tone.avatar.as_deref());
    let painter = ui.painter_at(rect);
    let downloads = painter.text(
        rect.right_bottom() - vec2(10.0, 12.0),
        Align2::RIGHT_CENTER,
        format!("↓ {}", short(tone.downloads)),
        FontId::proportional(10.5),
        p.weak,
    );
    painter.text(
        pos2(left + 24.0, face.center().y),
        Align2::LEFT_CENTER,
        fit(
            &painter,
            &tone.creator,
            &FontId::proportional(11.0),
            downloads.left() - left - 32.0,
        ),
        FontId::proportional(11.0),
        p.text,
    );
    let hint = match &tone.url {
        Some(url) => trf!("Download and load\n{url}", url = url),
        None => tr("Download and load").to_owned(),
    };
    response.on_hover_text(hint).clicked()
}

/// A picture filling `rect`, cropped to keep its proportions.
fn cover_picture(painter: &egui::Painter, rect: Rect, texture: &egui::TextureHandle) {
    let [w, h] = texture.size().map(|v| v as f32);
    let (rw, rh) = (rect.width() / w, rect.height() / h);
    let uv = if rw > rh {
        let keep = rh / rw;
        Rect::from_min_max(pos2(0.0, (1.0 - keep) / 2.0), pos2(1.0, (1.0 + keep) / 2.0))
    } else {
        let keep = rw / rh;
        Rect::from_min_max(pos2((1.0 - keep) / 2.0, 0.0), pos2((1.0 + keep) / 2.0, 1.0))
    };
    painter.image(texture.id(), rect, uv, Color32::WHITE);
}

/// The gear of a tone drawn in its likely style, when it has no picture.
fn gear_thumb(painter: &egui::Painter, area: Rect, kind: Kind, title: &str) {
    let style = Style::guess(title, None);
    match kind {
        Kind::Effects => face_thumb(painter, area, true, style),
        Kind::Cabs => {
            let size = vec2(area.height() * 1.1, area.height());
            let rect = Rect::from_center_size(area.center(), size);
            crate::gear::cabinet(painter, rect, 1, &style.look());
        }
        Kind::Amps => {
            let size = vec2(area.width().min(200.0), area.height() * 0.62);
            let rect = Rect::from_center_size(area.center() + vec2(0.0, 6.0), size);
            let look = style.look();
            let head = crate::gear::head(painter, rect, &look, style.name());
            for k in 0..5 {
                let x = head.panel.left() + head.panel.width() * (0.1 + 0.16 * k as f32);
                crate::gear::knob(
                    painter,
                    pos2(x, head.panel.center().y + 2.0),
                    12.0,
                    0.5,
                    look.knob,
                    look.letters,
                );
            }
        }
    }
}

/// A miniature amp or pedal face.
fn face_thumb(painter: &egui::Painter, thumb: Rect, pedal: bool, style: Style) {
    if pedal {
        let body = Rect::from_center_size(thumb.center(), vec2(42.0, 60.0));
        painter.rect_filled(body, 5.0, pedal_color(12));
        painter.circle_filled(
            body.center() + vec2(0.0, 16.0),
            7.0,
            Color32::from_rgb(180, 182, 186),
        );
        painter.circle_filled(
            body.center() - vec2(0.0, 12.0),
            5.0,
            Color32::from_rgb(40, 40, 44),
        );
    } else {
        let [covering, panel, grille, knob, _] = style.colors();
        painter.rect_filled(thumb, 5.0, covering);
        let face = Rect::from_min_max(
            thumb.min + vec2(4.0, 6.0),
            pos2(thumb.right() - 4.0, thumb.top() + 28.0),
        );
        painter.rect_filled(face, 2.0, panel);
        for i in 0..5 {
            painter.circle_filled(
                pos2(face.left() + 9.0 + i as f32 * 14.0, face.center().y),
                3.5,
                knob,
            );
        }
        painter.rect_filled(
            Rect::from_min_max(
                pos2(thumb.left() + 4.0, face.bottom() + 4.0),
                thumb.max - vec2(4.0, 4.0),
            ),
            2.0,
            grille,
        );
    }
}

/// A library card: a miniature face, the gear, its captures, a star; right-click for more.
fn card(ui: &mut Ui, pack: &Pack, width: f32) -> CardAction {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 84.0), Sense::click());
    let star_rect = Rect::from_min_size(
        pos2(rect.right() - 26.0, rect.top() + 4.0),
        vec2(22.0, 22.0),
    );
    let star = ui.interact(
        star_rect,
        ui.scope_id().with(("star", &pack.path)),
        Sense::click(),
    );
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        6.0,
        if response.hovered() {
            p.accent.gamma_multiply(0.18)
        } else {
            p.base
        },
    );
    let thumb = Rect::from_min_size(rect.min + vec2(8.0, 10.0), vec2(80.0, 64.0));
    let style = Style::guess(&format!("{} {}", pack.title, pack.detail), pack.gain);
    match pack.category {
        Category::Cab => draw_cab(painter, thumb, 0),
        c => face_thumb(painter, thumb, c == Category::Effect, style),
    }
    let left = rect.left() + 98.0;
    let mut job = egui::text::LayoutJob::simple(
        pack.title.clone(),
        theme::strong(13.0),
        p.text,
        rect.right() - left - 30.0,
    );
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    let title = painter.layout_job(job);
    let title_height = title.size().y;
    painter.galley(pos2(left, rect.top() + 10.0), title, p.text);
    let mut job = egui::text::LayoutJob::simple(
        pack.detail.clone(),
        FontId::proportional(10.5),
        p.weak,
        rect.right() - left - 8.0,
    );
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    painter.galley(
        pos2(left, rect.top() + 14.0 + title_height),
        painter.layout_job(job),
        p.weak,
    );
    painter.text(
        rect.right_bottom() - vec2(10.0, 8.0),
        Align2::RIGHT_BOTTOM,
        format!(
            "{} · {}",
            pack.category.name(),
            match pack.count {
                1 => tr("1 capture").to_owned(),
                n => trf!("{n} captures", n = n),
            }
        ),
        FontId::proportional(9.5),
        p.accent,
    );
    let gold = Color32::from_rgb(240, 196, 60);
    painter.text(
        star_rect.center(),
        Align2::CENTER_CENTER,
        if pack.favorite { "★" } else { "☆" },
        FontId::proportional(17.0),
        if pack.favorite || star.hovered() {
            gold
        } else {
            p.weak
        },
    );
    if star
        .on_hover_text(if pack.favorite {
            tr("Remove from favorites")
        } else {
            tr("Add to favorites: favourite amps come after the built-in ones under the amp arrows")
        })
        .clicked()
    {
        return CardAction::Star;
    }
    let mut action = CardAction::None;
    response.context_menu(|ui| {
        if ui
            .button(if pack.favorite {
                tr("Remove from favorites")
            } else {
                tr("Add to favorites")
            })
            .clicked()
        {
            action = CardAction::Star;
        }
        if ui.button(tr("Rename…")).clicked() {
            action = CardAction::Rename;
        }
        ui.menu_button(tr("Category"), |ui| {
            for c in Category::ALL {
                if ui.selectable_label(pack.category == c, c.name()).clicked() {
                    action = CardAction::Category(c);
                }
            }
        });
        ui.separator();
        let remove = if pack.owned {
            tr("Delete from the library")
        } else {
            tr("Hide (the files stay in their folder)")
        };
        if ui.button(remove).clicked() {
            action = CardAction::Remove;
        }
    });
    if !matches!(action, CardAction::None) {
        return action;
    }
    if response
        .on_hover_text(trf!(
            "{path}\nRight-click to rename, recategorize or remove",
            path = pack.path.display()
        ))
        .clicked()
    {
        return CardAction::Pick;
    }
    CardAction::None
}

#[cfg(test)]
mod tests {
    /// A few captures in the tests' library: amps, a pedal, a full rig, and an amp in a
    /// folder of its own with captures at several gains.
    fn fake_library() {
        let root = crate::nam_library::library_root().expect("a library folder");
        let capture = |dir: &std::path::Path, file: &str, kind: &str, model: &str, gain: f32| {
            std::fs::create_dir_all(dir).expect("a folder");
            let text = format!(
                r#"{{"version": "0.5.4", "metadata": {{"name": "{file}", "modeled_by": "Test",
                "gear_make": "Brand", "gear_model": "{model}", "gear_type": "{kind}", "gain": {gain}}},
                "architecture": "WaveNet", "config": {{}}, "weights": []}}"#
            );
            std::fs::write(dir.join(format!("{file}.nam")), text).expect("a capture");
        };
        capture(&root, "clean", "amp", "Twin", 0.2);
        capture(&root, "fuzz", "pedal", "Big Muff", 0.7);
        capture(&root, "rig", "amp-cab", "Plexi rig", 0.5);
        for (i, gain) in [0.2, 0.5, 0.9].into_iter().enumerate() {
            capture(
                &root.join("JCM800"),
                &format!("gain {i}"),
                "amp",
                "JCM800",
                gain,
            );
        }
        crate::captures::invalidate();
    }

    #[test]
    fn the_library_answers_a_user_trying_everything() {
        fake_library();
        for picker in [None, Some(Picker::Amp), Some(Picker::Pedal(0))] {
            let script = crate::test_sweep::script(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1300.0, 1100.0)),
                40.0,
                true,
            );
            let frames = script.len();
            let mut script = script.into_iter();
            crate::effect_editors::drive_for_test(
                "amp_rig",
                &[("model", 0.0)],
                frames,
                |frame, state| {
                    if frame == 0 {
                        state.rig.tab = Tab::Library;
                        state.rig.picker = picker;
                    }
                },
                |_| script.next().unwrap_or_default(),
            );
        }
    }

    #[test]
    fn the_rig_answers_a_user_trying_everything() {
        let pickers = [
            None,
            Some(Picker::Amp),
            Some(Picker::Pedal(0)),
            Some(Picker::Cab),
        ];
        for tab in [Tab::BuiltIn, Tab::Library] {
            for picker in pickers {
                let script = crate::test_sweep::script(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1300.0, 1100.0)),
                    48.0,
                    true,
                );
                let frames = script.len();
                let mut script = script.into_iter();
                crate::effect_editors::drive_for_test(
                    "amp_rig",
                    &[("p1_type", 2.0), ("p2_type", 20.0), ("p3_type", 19.0)],
                    frames,
                    |frame, state| {
                        if frame == 0 {
                            state.rig.tab = tab;
                            state.rig.picker = picker;
                        }
                    },
                    |_| script.next().unwrap_or_default(),
                );
            }
        }
    }

    #[test]
    fn the_rig_draws_its_tabs_and_pickers() {
        let pickers = [
            None,
            Some(Picker::Amp),
            Some(Picker::Pedal(0)),
            Some(Picker::Cab),
        ];
        for tab in [Tab::BuiltIn, Tab::Library] {
            for picker in pickers {
                crate::effect_editors::draw_for_test(
                    "amp_rig",
                    &[("p1_type", 2.0), ("p2_type", 20.0), ("cab", 10.0)],
                    2,
                    |_, state| {
                        state.rig.tab = tab;
                        state.rig.picker = picker;
                    },
                );
            }
        }
    }

    use super::*;

    #[test]
    fn styles_follow_names_then_gain() {
        assert_eq!(Style::guess("Ampeg SVT", None), Style::Bass);
        assert_eq!(Style::guess("Vox AC30", None), Style::Voxy);
        assert_eq!(Style::guess("Fender Bassman", None), Style::Tweed);
        for m in &MODELS {
            assert!(m.face == "blackface" || Style::from_face(m.face) != Style::Blackface);
        }
        assert_eq!(Style::guess("Mystery amp", Some(0.9)), Style::Modern);
        assert_eq!(Style::guess("Mystery amp", None), Style::Blackface);
    }

    #[test]
    fn counts_and_browser_sections() {
        assert_eq!(short(999), "999");
        assert_eq!(short(1500), "1.5k");
        assert_eq!(short(2_500_000), "2.5M");
        for kind in Kind::ALL {
            assert_eq!(Kind::of(kind.place()), kind);
        }
        assert_eq!(slot_id(0, "type"), "p1_type");
    }
}
