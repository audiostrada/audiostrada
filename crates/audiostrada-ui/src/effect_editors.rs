//! Dedicated editors for built-in effects: a visual main view with a control for every
//! parameter (some only where they apply, as the note of a synced delay).

use std::collections::VecDeque;

use audiostrada_core::InsertId;
use audiostrada_core::automation::AutomationTarget;
use audiostrada_core::routing::{self, Channel};
use audiostrada_dsp::{EffectInfo, ParamInfo};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, pos2,
    vec2,
};

use crate::document::Document;
use crate::i18n::tr;
use crate::mixer::{automated_control, learn_menu};
use crate::theme;
use crate::trf;
use crate::widgets;

/// Reads and writes the parameters of one insert, through automation when it applies.
pub struct Params<'a> {
    pub doc: &'a mut Document,
    pub channel: Channel,
    pub insert: InsertId,
    pub info: &'static EffectInfo,
    pub values: Vec<f32>,
}

impl Params<'_> {
    fn index(&self, id: &str) -> Option<usize> {
        self.info.params.iter().position(|p| p.id == id)
    }

    pub fn get(&self, id: &str) -> f32 {
        self.index(id).map_or(0.0, |i| self.values[i])
    }

    pub(crate) fn param(&self, id: &str) -> Option<&'static ParamInfo> {
        self.info.params.iter().find(|p| p.id == id)
    }

    /// Applies a control's change and ends the gesture when it is released, without the
    /// MIDI learn menu (for areas that have their own).
    pub fn change(&mut self, id: &str, changed: Option<f32>, response: &Response) {
        let Some(param) = self.param(id) else {
            return;
        };
        #[cfg(test)]
        tests::SHOWN.with_borrow_mut(|shown| shown.push(id.to_owned()));
        let (channel, insert, name) = (self.channel, self.insert, self.info.name);
        let lane = AutomationTarget::Param {
            insert,
            param: id.to_owned(),
        };
        let changed = changed.map(|v| v.clamp(param.min, param.max));
        if let (Some(v), Some(i)) = (changed, self.index(id)) {
            self.values[i] = v;
        }
        automated_control(self.doc, channel, &lane, changed, response, |doc, value| {
            doc.history_mut().edit_merged(
                &format!("{} {}", tr(name), tr(param.name)),
                &format!("insert:{}:{id}", insert.0),
                |p| routing::set_insert_param(p, channel, insert, id, value),
            );
        });
    }

    /// Applies a control's change and ends the gesture when it is released.
    pub fn set(&mut self, id: &str, changed: Option<f32>, response: &Response) {
        let Some(param) = self.param(id) else {
            return;
        };
        self.change(id, changed, response);
        let lane = AutomationTarget::Param {
            insert: self.insert,
            param: id.to_owned(),
        };
        learn_menu(
            response,
            self.doc,
            self.channel,
            &lane,
            (param.min, param.max),
        );
    }

    /// Applies an on/off control drawn by `add`, which gets whether the switch is on.
    pub(crate) fn toggle(
        &mut self,
        ui: &mut Ui,
        id: &str,
        add: impl FnOnce(&mut Ui, &mut bool) -> Response,
    ) {
        let mut on = self.get(id) >= 0.5;
        let response = add(ui, &mut on);
        let changed = response.changed().then_some(if on { 1.0 } else { 0.0 });
        self.set(id, changed, &response);
    }
}

/// Per-editor memory between frames.
#[derive(Debug, Default)]
pub struct EditorState {
    /// Settings kept in the A and B slots, and which one is being edited.
    ab: [Option<Settings>; 2],
    ab_current: usize,
    /// Last preset loaded, and the name typed while saving one.
    preset: Option<String>,
    saving: Option<String>,
    selected: usize,
    /// Gain reduction of the last frames, newest last.
    history: VecDeque<(f32, f32)>,
    /// Gain reduction, input and output of the last frames (dB), newest last.
    dynamics: VecDeque<[f32; 3]>,
    /// Position of an analog meter's needle, with its ballistics.
    needle: f32,
    rig: crate::rig_editor::RigState,
    limiter: LimiterState,
    spectrum: SpectrumState,
    /// Center of the Pitch Correct view, in semitones (0 before any pitch).
    pitch_view: f32,
    pub synth: crate::synth_editor::SynthState,
    pub drums: crate::drums_editor::DrumsState,
    /// Match is on (listening only; switched off when the window closes).
    pub matching: bool,
    /// EQ band heard alone (listening only; stopped when the window closes).
    pub eq_solo: Option<usize>,
    eq_view: EqView,
}

/// How the EQ display looks: its range and its spectra.
#[derive(Debug)]
struct EqView {
    /// Index in [`EQ_RANGES`].
    range: usize,
    /// Index in [`ANALYZER_MODES`].
    analyzer: usize,
    /// Highest level of each band of the output spectrum while holding.
    hold: Option<Vec<f32>>,
}

impl Default for EqView {
    fn default() -> Self {
        Self {
            range: 2,
            analyzer: 2,
            hold: None,
        }
    }
}

/// Parameters and plugin state of an insert, as A/B slots and presets hold them.
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    params: std::collections::BTreeMap<String, f32>,
    state: Option<Vec<u8>>,
}

fn current_settings(
    doc: &mut Document,
    channel: Channel,
    insert: &audiostrada_core::Insert,
) -> Settings {
    let state = doc
        .player_mut()
        .plugin(channel, insert.id)
        .and_then(|p| p.save_state());
    Settings {
        params: insert.params.clone(),
        state,
    }
}

fn apply_settings(
    doc: &mut Document,
    channel: Channel,
    insert: InsertId,
    label: &str,
    settings: &Settings,
) {
    let encoded = settings
        .state
        .as_deref()
        .map(audiostrada_core::base64::encode);
    doc.history_mut().edit(label, |p| {
        let inserts = match channel {
            Channel::Master => Some(&mut p.master.inserts),
            Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
        };
        if let Some(i) = inserts.and_then(|list| list.iter_mut().find(|i| i.id == insert)) {
            i.params = settings.params.clone();
            if encoded.is_some() {
                i.state = encoded.clone();
            }
        }
    });
    if let (Some(state), Some(plugin)) = (&settings.state, doc.player_mut().plugin(channel, insert))
    {
        plugin.load_state(state);
    }
}

/// The band at the top of a built-in effect: power (bypass) button, name and description in
/// the family's color, and the presets on the right. Returns the new bypass state if clicked.
pub fn header(
    ui: &mut Ui,
    info: &EffectInfo,
    bypass: bool,
    presets: impl FnOnce(&mut Ui),
) -> Option<bool> {
    let p = theme::current(ui);
    let color = crate::effect_browser::family_color(info.category);
    let width = ui.available_width();
    let (band, _) = ui.allocate_exact_size(vec2(width, 58.0), Sense::hover());
    let painter = ui.painter_at(band);
    // A soft wash of the family color fading to the panel.
    let mut mesh = egui::Mesh::default();
    let tint = color.gamma_multiply(if bypass { 0.08 } else { 0.22 });
    for (pos, c) in [
        (band.left_top(), tint),
        (band.right_top(), Color32::TRANSPARENT),
        (band.right_bottom(), Color32::TRANSPARENT),
        (band.left_bottom(), tint),
    ] {
        mesh.colored_vertex(pos, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
    painter.rect_filled(
        Rect::from_min_size(band.left_top(), vec2(4.0, band.height())),
        2.0,
        color,
    );
    // Power button.
    let center = pos2(band.left() + 30.0, band.center().y);
    let power = ui.interact(
        Rect::from_center_size(center, vec2(30.0, 30.0)),
        ui.scope_id().with(("power", info.id)),
        Sense::click(),
    );
    let lit = theme::power_ink(!bypass, &p);
    painter.circle_filled(center, 15.0, p.base);
    painter.circle_stroke(center, 15.0, Stroke::new(1.5, lit.gamma_multiply(0.7)));
    let arc: Vec<Pos2> = (0..=24)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2
                + 0.6
                + i as f32 / 24.0 * (std::f32::consts::TAU - 1.2);
            center + vec2(a.cos(), a.sin()) * 7.0
        })
        .collect();
    painter.add(Shape::line(arc, Stroke::new(2.0, lit)));
    painter.line_segment(
        [center - vec2(0.0, 9.0), center - vec2(0.0, 2.0)],
        Stroke::new(2.0, lit),
    );
    let toggled = power
        .on_hover_text(if bypass { tr("Turn on") } else { tr("Bypass") })
        .clicked()
        .then_some(!bypass);
    brand(ui, &painter, band);
    painter.text(
        pos2(band.left() + 56.0, band.top() + 10.0),
        Align2::LEFT_TOP,
        tr(info.name),
        theme::strong(19.0),
        if bypass { p.weak } else { p.text },
    );
    painter.text(
        pos2(band.left() + 57.0, band.top() + 35.0),
        Align2::LEFT_TOP,
        tr(info.description),
        FontId::proportional(11.0),
        p.weak,
    );
    ui.add_space(4.0);
    presets(ui);
    ui.add_space(4.0);
    toggled
}

/// With the full mark: the logo and "AudioStrada Effects"; else the logo alone.
const BRAND_FULL: bool = true;

/// AudioStrada's mark at the right of an effect's band: one of its own effects.
fn brand(ui: &Ui, painter: &egui::Painter, band: Rect) {
    let p = theme::current(ui);
    let Some(logo) = crate::logo(ui.ctx()) else {
        return;
    };
    let height = if BRAND_FULL { 22.0 } else { 26.0 };
    let size = vec2(height * 105.0 / 96.0, height);
    let right = band.right() - 16.0;
    let mut x = right - size.x;
    if BRAND_FULL {
        let name = painter.layout_no_wrap("AudioStrada".to_owned(), theme::strong(13.0), p.text);
        let line = painter.layout_no_wrap("Effects".to_owned(), FontId::proportional(10.0), p.weak);
        let text = name.size().x.max(line.size().x);
        let left = right - text;
        painter.galley(pos2(left, band.center().y - 12.0), name, p.text);
        painter.galley(pos2(left + 1.0, band.center().y + 3.0), line, p.weak);
        x = left - 8.0 - size.x;
    }
    painter.image(
        logo.id(),
        Rect::from_min_size(pos2(x, band.center().y - size.y / 2.0), size),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// Presets (factory and user) and A/B comparison, above an effect's editor.
pub fn preset_bar(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &audiostrada_core::Insert,
    state: &mut EditorState,
) {
    let root = crate::presets::root();
    let key = insert.effect.as_str();
    ui.horizontal(|ui| {
        let list = crate::presets::list(root.as_deref(), key);
        let shown = state
            .preset
            .clone()
            .unwrap_or_else(|| tr("Presets").to_owned());
        egui::ComboBox::from_id_salt(("presets", insert.id.0))
            .width(170.0)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if list.is_empty() {
                    ui.weak(tr("No presets yet"));
                }
                let mut factory = true;
                for preset in &list {
                    if factory && !preset.factory {
                        ui.separator();
                        factory = false;
                    }
                    if ui.selectable_label(false, &preset.name).clicked() {
                        let settings = Settings {
                            params: preset.params.clone(),
                            state: preset
                                .state
                                .as_deref()
                                .and_then(audiostrada_core::base64::decode),
                        };
                        apply_settings(
                            doc,
                            channel,
                            insert.id,
                            &crate::trf!("Preset {name}", name = preset.name),
                            &settings,
                        );
                        state.preset = Some(preset.name.clone());
                    }
                }
            });
        if ui
            .button(tr("Save…"))
            .on_hover_text(tr("Save the current settings as a preset"))
            .clicked()
        {
            state.saving = Some(state.preset.clone().unwrap_or_default());
        }
        if let Some(name) = state.preset.clone()
            && !list.iter().any(|p| p.name == name && p.factory)
            && list.iter().any(|p| p.name == name)
            && ui.button(tr("Delete")).clicked()
            && let Some(root) = &root
        {
            if let Err(e) = crate::presets::delete(root, key, &name) {
                doc.notify(crate::trf!("Cannot delete the preset: {error}", error = e));
            }
            state.preset = None;
        }
        ui.separator();
        for (slot, label) in ["A", "B"].into_iter().enumerate() {
            if ui
                .selectable_label(state.ab_current == slot, label)
                .on_hover_text(tr("Compare two settings: each slot keeps its own"))
                .clicked()
                && state.ab_current != slot
            {
                let now = current_settings(doc, channel, insert);
                let other = state.ab[slot].clone().unwrap_or_else(|| now.clone());
                state.ab[state.ab_current] = Some(now);
                apply_settings(
                    doc,
                    channel,
                    insert.id,
                    &crate::trf!("Compare {slot}", slot = label),
                    &other,
                );
                state.ab_current = slot;
            }
        }
        if ui
            .small_button(if state.ab_current == 0 {
                "A → B"
            } else {
                "B → A"
            })
            .on_hover_text(tr("Copy the settings being edited to the other slot"))
            .clicked()
        {
            let now = current_settings(doc, channel, insert);
            state.ab[1 - state.ab_current] = Some(now);
        }
    });
    if let Some(mut name) = state.saving.take() {
        let mut keep = true;
        ui.horizontal(|ui| {
            ui.label(tr("Name"));
            let response = ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0));
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(tr("Save")).clicked() || enter {
                let settings = current_settings(doc, channel, insert);
                let preset = crate::presets::Preset {
                    name: name.trim().to_owned(),
                    params: settings.params,
                    state: settings
                        .state
                        .as_deref()
                        .map(audiostrada_core::base64::encode),
                    factory: false,
                };
                match root
                    .as_deref()
                    .map(|r| crate::presets::save(r, key, &preset))
                {
                    Some(Ok(())) => {
                        state.preset = Some(preset.name);
                        keep = false;
                    }
                    Some(Err(e)) => {
                        doc.notify(crate::trf!("Cannot save the preset: {error}", error = e))
                    }
                    None => doc.notify(tr("No configuration folder for presets").into()),
                }
            }
            if ui.button(tr("Cancel")).clicked() {
                keep = false;
            }
        });
        if keep {
            state.saving = Some(name);
        }
    }
}

/// Effects that only measure: they have no presets nor Match.
pub fn is_meter(id: &str) -> bool {
    matches!(id, "analyzer" | "tuner")
}

/// Which track drives the detection of a compressor or gate.
fn sidechain_choice(ui: &mut Ui, params: &mut Params) {
    let project = params.doc.project();
    let (channel, insert) = (params.channel, params.insert);
    let current = routing::find_insert(project, channel, insert).and_then(|i| i.sidechain);
    let own = match channel {
        Channel::Track(t) => Some(t),
        Channel::Master => None,
    };
    let tracks: Vec<(audiostrada_core::TrackId, String)> = project
        .tracks
        .iter()
        .filter(|t| Some(t.id) != own && t.kind == audiostrada_core::TrackKind::Audio)
        .map(|t| (t.id, t.name.clone()))
        .collect();
    let name = |id: Option<audiostrada_core::TrackId>| {
        id.and_then(|id| tracks.iter().find(|t| t.0 == id).map(|t| t.1.clone()))
            .unwrap_or_else(|| tr("Off (own signal)").to_owned())
    };
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr("Sidechain"))
                .size(10.5)
                .color(theme::current(ui).weak),
        );
        egui::ComboBox::from_id_salt(("sidechain", insert.0))
            .selected_text(name(current))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_none(), tr("Off (own signal)"))
                    .clicked()
                {
                    chosen = Some(None);
                }
                for (id, track) in &tracks {
                    if ui.selectable_label(current == Some(*id), track).clicked() {
                        chosen = Some(Some(*id));
                    }
                }
            })
            .response
            .on_hover_text(tr(
                "Detect on another track's output, e.g. duck the bass with the kick",
            ));
    });
    if let Some(source) = chosen {
        params.doc.history_mut().edit(tr("Sidechain"), |p| {
            let inserts = match channel {
                Channel::Master => Some(&mut p.master.inserts),
                Channel::Track(t) => p.track_mut(t).map(|t| &mut t.inserts),
            };
            if let Some(i) = inserts.and_then(|l| l.iter_mut().find(|i| i.id == insert)) {
                i.sidechain = source;
            }
        });
    }
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    if audiostrada_dsp::supports_sidechain(params.info.id) {
        sidechain_choice(ui, params);
        ui.add_space(4.0);
    }
    match params.info.id {
        "eq" => eq(ui, params, meters, state),
        "compressor" => compressor(ui, params, meters, state),
        "tuner" => tuner(ui, params, meters),
        "pitch_correct" => pitch_correct(ui, params, meters, state),
        "analyzer" => analyzer(ui, params, meters, &mut state.spectrum),
        "delay" => delay(ui, params),
        "reverb" => reverb(ui, params),
        "maximizer" => maximizer(ui, params, meters, state),
        "tape" => tape(ui, params),
        "flanger" | "phaser" | "chorus" => modulation(ui, params),
        "imager" => imager(ui, params, meters),
        "utility" => utility(ui, params, meters),
        "channel" => channel_strip(ui, params, meters),
        "transient" => transient(ui, params, meters, state),
        "deesser" => deesser(ui, params, meters, state),
        "vintage_eq" => vintage_eq(ui, params),
        "filter" => filter(ui, params, meters),
        "drive" => drive(ui, params),
        "gate" => gate(ui, params, meters, state),
        "limiter" => limiter(ui, params, meters, state),
        "amp_rig" => crate::rig_editor::show(ui, params, meters, &mut state.rig),
        "synth" => crate::synth_editor::show(ui, params, &mut state.synth),
        "drums" => crate::drums_editor::show(ui, params, meters, &mut state.drums),
        _ => knob_panel(ui, params, meters),
    }
}

/// Knobs for continuous parameters, switches above them, meters as bars.
fn knob_panel(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    let p = theme::current(ui);
    if params.info.params.iter().any(|q| q.toggle) {
        ui.horizontal_wrapped(|ui| toggles(ui, params));
        ui.add_space(6.0);
    }
    for (name, value) in params.info.meters.iter().zip(meters) {
        if name.is_empty() {
            continue;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(name)).size(10.5).color(p.weak));
            let (rect, _) = ui.allocate_exact_size(vec2(220.0, 8.0), Sense::hover());
            ui.painter().rect_filled(rect, 3.0, p.base);
            let f = (-value / 24.0).clamp(0.0, 1.0);
            let bar = Rect::from_min_max(
                rect.min,
                pos2(rect.left() + rect.width() * f, rect.bottom()),
            );
            ui.painter().rect_filled(bar, 3.0, theme::RECORD);
            ui.label(
                RichText::new(format!("{value:.1} dB"))
                    .monospace()
                    .size(11.0),
            );
        });
    }
    let ids: Vec<&'static str> = params
        .info
        .params
        .iter()
        .filter(|q| !q.toggle)
        .map(|q| q.id)
        .collect();
    for row in ids.chunks(6) {
        ui.horizontal(|ui| knobs(ui, params, row, 50.0, p.accent));
        ui.add_space(4.0);
    }
}

fn is_log(p: &ParamInfo) -> bool {
    matches!(p.unit, "Hz" | "ms") && p.min > 0.0
}

pub(crate) fn to_unit(p: &ParamInfo, v: f32) -> f32 {
    if is_log(p) {
        (v / p.min).ln() / (p.max / p.min).ln()
    } else {
        (v - p.min) / (p.max - p.min)
    }
}

pub(crate) fn from_unit(p: &ParamInfo, t: f32) -> f32 {
    if is_log(p) {
        p.min * (p.max / p.min).powf(t)
    } else {
        p.min + t * (p.max - p.min)
    }
}

pub fn format_value(p: &ParamInfo, v: f32) -> String {
    if let Some(name) = choice_name(p, v) {
        return name.to_owned();
    }
    match p.unit {
        "Hz" if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
        "Hz" if v < 10.0 => format!("{v:.2} Hz"),
        "Hz" if v < 100.0 => format!("{v:.1} Hz"),
        "Hz" => format!("{v:.0} Hz"),
        "ms" if v < 10.0 => format!("{v:.1} ms"),
        "ms" => format!("{v:.0} ms"),
        "dB" => format!("{v:+.1} dB"),
        ":1" => format!("{v:.1}:1"),
        "%" => format!("{v:.0} %"),
        "" => format!("{v:.2}"),
        unit => format!("{v:.1} {unit}"),
    }
}

fn choice_name(p: &ParamInfo, v: f32) -> Option<&'static str> {
    let i = (v - p.min).round();
    (!p.choices.is_empty() && i >= 0.0)
        .then(|| p.choices.get(i as usize).copied())
        .flatten()
        .map(tr)
}

/// Buttons side by side for a parameter with a few named values, a drop-down list for more.
pub fn choice_box(ui: &mut Ui, params: &mut Params, id: &str) {
    let Some(p) = params.param(id) else {
        return;
    };
    if p.choices.len() <= 4 {
        ui.horizontal(|ui| segmented(ui, params, id, &[]));
        return;
    }
    let value = params.get(id);
    let mut chosen = None;
    let response = egui::ComboBox::from_id_salt(("choice", params.insert.0, id))
        .selected_text(choice_name(p, value).unwrap_or("?"))
        .show_ui(ui, |ui| {
            for (i, name) in p.choices.iter().enumerate() {
                let v = p.min + i as f32;
                if ui
                    .selectable_label((value - v).abs() < 0.5, tr(name))
                    .clicked()
                {
                    chosen = Some(v);
                }
            }
        })
        .response;
    params.set(id, chosen, &response);
    if chosen.is_some() {
        params.set(id, None, &response);
        params.doc.history_mut().end_gesture();
    }
}

/// A knob with its name above and value below; a list for named values.
pub(crate) fn param_knob(ui: &mut Ui, params: &mut Params, id: &str, size: f32, tint: Color32) {
    let Some(p) = params.param(id) else {
        return;
    };
    named_knob(ui, params, id, tr(p.name), size, tint);
}

/// A knob labelled `label` instead of its parameter's name.
pub(crate) fn named_knob(
    ui: &mut Ui,
    params: &mut Params,
    id: &str,
    label: &str,
    size: f32,
    tint: Color32,
) {
    knob_with(ui, params, (id, label), size, tint, false);
}

/// A knob as wide as its label and its longest value, which stay on one line.
pub(crate) fn wide_knob(
    ui: &mut Ui,
    params: &mut Params,
    id: &str,
    label: &str,
    size: f32,
    tint: Color32,
) {
    knob_with(ui, params, (id, label), size, tint, true);
}

fn knob_with(
    ui: &mut Ui,
    params: &mut Params,
    (id, label): (&str, &str),
    size: f32,
    tint: Color32,
    wide: bool,
) {
    let Some(p) = params.param(id) else {
        return;
    };
    if !p.choices.is_empty() {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(label)
                    .size(10.5)
                    .color(theme::current(ui).weak),
            );
            choice_box(ui, params, id);
        });
        return;
    }
    let value = params.get(id);
    let width = if wide {
        let text = |t: String, font: FontId| {
            ui.painter()
                .layout_no_wrap(t, font, Color32::WHITE)
                .size()
                .x
        };
        let values = [p.min, p.max, value]
            .into_iter()
            .map(|v| text(format_value(p, v), FontId::monospace(11.0)))
            .fold(0.0, f32::max);
        (size + 18.0)
            .max(text(label.to_owned(), FontId::proportional(10.5)) + 6.0)
            .max(values + 6.0)
    } else {
        size + 18.0
    };
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.vertical_centered(|ui| {
            let mut name = egui::Label::new(
                RichText::new(label)
                    .size(10.5)
                    .color(theme::current(ui).weak),
            );
            if wide {
                name = name.extend();
            }
            ui.add(name);
            let bipolar = p.min < 0.0 && p.max > 0.0;
            let (changed, response) = widgets::knob(
                ui,
                to_unit(p, value),
                (0.0, 1.0),
                to_unit(p, p.default),
                size,
                bipolar,
                tint,
            );
            let changed = changed.map(|t| from_unit(p, t));
            let shown = changed.unwrap_or(value);
            let response = response.on_hover_text(crate::trf!(
                "{name}: {value}. Drag, Shift for fine, double-click to reset",
                name = tr(p.name),
                value = format_value(p, shown)
            ));
            params.set(id, changed, &response);
            let mut text =
                egui::Label::new(RichText::new(format_value(p, shown)).monospace().size(11.0));
            if wide {
                text = text.extend();
            }
            ui.add(text);
        });
    });
}

// ---------------------------------------------------------------- EQ

use audiostrada_dsp::eq::{BANDS as EQ_BANDS, BandType, Place};

/// Placements of a band as on its handle, and what they mean.
const PLACE_SHORT: [&str; 5] = ["St", "L", "R", "M", "S"];
const PLACE_HINTS: [&str; 5] = [
    "Both channels",
    "Left channel only",
    "Right channel only",
    "Mid only: what both channels share",
    "Sides only: what differs between the channels",
];

/// Small buttons in a row, the current one lit; the index of the one clicked.
fn chips(
    ui: &mut Ui,
    labels: &[&str],
    hints: &[&'static str],
    current: Option<usize>,
) -> Option<usize> {
    let p = theme::current(ui);
    let mut clicked = None;
    ui.spacing_mut().item_spacing.x = 3.0;
    for (i, label) in labels.iter().enumerate() {
        let galley =
            ui.painter()
                .layout_no_wrap((*label).to_owned(), FontId::proportional(11.0), p.text);
        let size = vec2((galley.size().x + 16.0).max(30.0), 22.0);
        let (r, mut response) = ui.allocate_exact_size(size, Sense::click());
        let on = current == Some(i);
        let fill = match (on, response.hovered()) {
            (true, _) => p.accent.gamma_multiply(0.3),
            (false, true) => p.hover,
            (false, false) => p.surface,
        };
        let painter = ui.painter();
        painter.rect_filled(r, 6.0, fill);
        painter.rect_stroke(
            r,
            6.0,
            Stroke::new(1.0, if on { p.accent } else { p.border }),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            r.center() - galley.size() / 2.0,
            galley,
            if on { p.text } else { p.weak },
        );
        if let Some(hint) = hints.get(i) {
            response = response.on_hover_text(tr(hint));
        }
        if response.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// Above the EQ display: what the analyzer shows, and the range in dB.
fn eq_toolbar(ui: &mut Ui, state: &mut EditorState) {
    let view = &mut state.eq_view;
    let weak = theme::current(ui).weak;
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Analyzer")).size(11.0).color(weak));
        let modes = ANALYZER_MODES.map(tr);
        let hints = [
            "No spectrum",
            "The spectrum of the output",
            "The input as a line, the output filled",
        ];
        if let Some(m) = chips(ui, &modes, &hints, Some(view.analyzer)) {
            view.analyzer = m;
        }
        ui.add_space(6.0);
        let hold = [tr("Hold")];
        let lit = view.hold.is_some().then_some(0);
        let hint = ["Keep the highest level of the output spectrum; click again to clear"];
        if chips(ui, &hold, &hint, lit).is_some() {
            view.hold = match view.hold {
                Some(_) => None,
                None => Some(Vec::new()),
            };
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let labels: Vec<String> = EQ_RANGES
                .iter()
                .rev()
                .map(|(r, _)| format!("±{r}"))
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let n = EQ_RANGES.len();
            let current = Some(n - 1 - view.range.min(n - 1));
            if let Some(i) = chips(ui, &labels, &[], current) {
                view.range = n - 1 - i;
            }
            ui.label(RichText::new(tr("Range")).size(11.0).color(weak));
        });
    });
}

/// Heights of the EQ display in dB above and below 0, and the step of their grid.
const EQ_RANGES: [(f32, f32); 4] = [(3.0, 1.0), (6.0, 2.0), (12.0, 3.0), (30.0, 6.0)];
const ANALYZER_MODES: [&str; 3] = ["Off", "Output", "In + out"];

/// Color of an EQ band, spread around the wheel.
fn band_color(band: usize) -> Color32 {
    egui::ecolor::Hsva::new((band as f32 * 0.618_034 + 0.58).fract(), 0.55, 0.95, 1.0).into()
}

fn band_type_name(kind: BandType) -> &'static str {
    tr(audiostrada_dsp::eq::TYPES[kind.index()])
}

/// Whether a parameter of an effect is offered for automation: for the EQ, the settings of
/// the bands in use.
pub(crate) fn automatable(
    effect: &str,
    params: &std::collections::BTreeMap<String, f32>,
    id: &str,
) -> bool {
    if effect != "eq" {
        return true;
    }
    if id == "solo" {
        return false;
    }
    match id.strip_prefix('b').and_then(|rest| rest.split_once('_')) {
        Some((band, field)) => {
            params
                .get(&format!("b{band}_used"))
                .is_some_and(|&u| u >= 0.5)
                && matches!(
                    field,
                    "freq" | "gain" | "q" | "thresh" | "range" | "bypass" | "attack" | "release"
                )
        }
        None => true,
    }
}

/// Sets several parameters of the insert as one undo step.
fn edit_params(params: &mut Params, label: &str, changes: &[(String, f32)]) {
    let (channel, insert) = (params.channel, params.insert);
    for (id, v) in changes {
        if let Some(i) = params.index(id) {
            params.values[i] = *v;
        }
    }
    params.doc.history_mut().edit(label, |p| {
        for (id, v) in changes {
            routing::set_insert_param(p, channel, insert, id, *v);
        }
    });
}

/// Hears band `band` alone (its range), or the whole EQ again; never saved.
fn solo_band(params: &mut Params, band: Option<usize>) {
    let project = params.doc.project().clone();
    let target = AutomationTarget::Param {
        insert: params.insert,
        param: "solo".into(),
    };
    let value = band.map(|b| (b + 1) as f32);
    params
        .doc
        .player_mut()
        .set_override(&project, params.channel, &target, value);
}

/// Equalizer in the style of the big studio EQs: double-click the curve to add a band,
/// drag a band to move it, the wheel for its Q, double-click it to remove it, right-click
/// for its type, dynamics and solo.
fn eq(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    let p = theme::current(ui);
    let rate = f64::from(params.doc.player().engine().sample_rate().max(1));
    let width = ui.available_width().max(600.0);
    eq_toolbar(ui, state);
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 340.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    let x_of = |f: f32| rect.left() + (f / 20.0).log10() / 3.0 * rect.width();
    let f_of = |x: f32| 20.0 * 1000f32.powf(((x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    let half = rect.height() / 2.0 - 14.0;
    let (db_range, db_step) = EQ_RANGES[state.eq_view.range.min(EQ_RANGES.len() - 1)];
    let y_of = |db: f32| rect.center().y - db / db_range * half;
    let db_of = |y: f32| ((rect.center().y - y) / half * db_range).clamp(-30.0, 30.0);
    let grid = p.weak.gamma_multiply(0.16);
    for f in [
        30.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0,
    ] {
        let x = x_of(f);
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, grid),
        );
        let label = if f >= 1000.0 {
            format!("{}k", f / 1000.0)
        } else {
            format!("{f}")
        };
        painter.text(
            pos2(x + 3.0, rect.bottom() - 3.0),
            Align2::LEFT_BOTTOM,
            label,
            FontId::proportional(9.5),
            p.weak,
        );
    }
    let lines = (db_range / db_step).round() as i32;
    for k in -lines..=lines {
        let db = k as f32 * db_step;
        let y = y_of(db).clamp(rect.top() + 12.0, rect.bottom() - 2.0);
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(
                1.0,
                if db == 0.0 {
                    grid.gamma_multiply(2.2)
                } else {
                    grid
                },
            ),
        );
        painter.text(
            pos2(rect.right() - 4.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            format!("{db:+}"),
            FontId::proportional(9.5),
            p.weak,
        );
    }

    // Spectra, -90 to 0 dBFS over the full height: the input as a line, the output filled.
    let spectrum = |levels: &[f32]| -> Vec<Pos2> {
        levels
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let f = audiostrada_dsp::analyzer::band_frequency(i);
                let level = ((m + 90.0) / 90.0).clamp(0.0, 1.0);
                pos2(x_of(f), rect.bottom() - level * rect.height())
            })
            .collect()
    };
    let bins = audiostrada_dsp::analyzer::BANDS;
    let analyzer = &meters[..meters.len().min(bins)];
    let mode = state.eq_view.analyzer;
    if mode == 2
        && let Some(input) = meters.get(audiostrada_dsp::eq::INPUT_SPECTRUM..)
        && input.len() >= bins
    {
        let mut points = spectrum(&input[..bins]);
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        painter.add(Shape::line(
            points,
            Stroke::new(1.0, p.weak.gamma_multiply(0.45)),
        ));
    }
    if let Some(hold) = &mut state.eq_view.hold {
        hold.resize(analyzer.len(), -120.0);
        for (h, m) in hold.iter_mut().zip(analyzer) {
            *h = h.max(*m);
        }
        let mut points = spectrum(hold);
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        painter.add(Shape::line(
            points,
            Stroke::new(1.0, theme::SOLO.gamma_multiply(0.7)),
        ));
    }
    if mode > 0 && !analyzer.is_empty() {
        let mut points = spectrum(analyzer);
        // Filled from the spectrum down, fading toward the bottom.
        let mut mesh = egui::Mesh::default();
        let (top, bottom) = (p.accent.gamma_multiply(0.18), p.accent.gamma_multiply(0.02));
        for pair in points.windows(2) {
            let i = mesh.vertices.len() as u32;
            for (q, c) in [
                (pair[0], top),
                (pair[1], top),
                (pos2(pair[1].x, rect.bottom()), bottom),
                (pos2(pair[0].x, rect.bottom()), bottom),
            ] {
                mesh.colored_vertex(q, c);
            }
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        painter.add(Shape::mesh(mesh));
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5);
        painter.add(Shape::line(
            points,
            Stroke::new(1.0, p.accent.gamma_multiply(0.45)),
        ));
    }

    let values = params.values.clone();
    let offsets: Vec<f32> = (0..EQ_BANDS)
        .map(|b| {
            meters
                .get(audiostrada_dsp::analyzer::BANDS + b)
                .copied()
                .unwrap_or(0.0)
        })
        .collect();
    let bands: Vec<(usize, audiostrada_dsp::eq::Band)> = (0..EQ_BANDS)
        .filter_map(|b| audiostrada_dsp::eq::band(&values, b).map(|band| (b, band)))
        .collect();
    if state.selected >= EQ_BANDS || !bands.iter().any(|(b, _)| *b == state.selected) {
        state.selected = bands.first().map_or(0, |(b, _)| *b);
    }
    let steps = 240;
    let freq_at = |i: usize| 20.0 * 1000f32.powf(i as f32 / steps as f32);
    let zero = y_of(0.0);
    // Each band's shape, filled from 0 dB; the selected one stronger.
    for &(b, band) in &bands {
        let curve: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let f = freq_at(i);
                let db = audiostrada_dsp::eq::band_db(&values, b, offsets[b], f64::from(f), rate)
                    .unwrap_or(0.0);
                pos2(
                    x_of(f),
                    y_of((db as f32).clamp(-30.0, 30.0)).clamp(rect.top(), rect.bottom()),
                )
            })
            .collect();
        let selected = b == state.selected;
        let color = band_color(b);
        let alpha = if band.bypass {
            0.0
        } else if selected {
            0.22
        } else {
            0.08
        };
        if alpha > 0.0 {
            let mut mesh = egui::Mesh::default();
            let fill = color.gamma_multiply(alpha);
            for pair in curve.windows(2) {
                let i = mesh.vertices.len() as u32;
                for q in [
                    pair[0],
                    pair[1],
                    pos2(pair[1].x, zero),
                    pos2(pair[0].x, zero),
                ] {
                    mesh.colored_vertex(q, fill);
                }
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
            painter.add(Shape::mesh(mesh));
        }
        if selected {
            painter.add(Shape::line(
                curve,
                Stroke::new(1.2, color.gamma_multiply(0.8)),
            ));
        }
    }
    let total: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let f = freq_at(i);
            let db = audiostrada_dsp::eq::response_db(&values, &offsets, f64::from(f), rate);
            pos2(x_of(f), y_of((db as f32).clamp(-30.0, 30.0)))
        })
        .collect();
    painter.add(Shape::line(total, Stroke::new(2.2, p.text)));

    // Band handles: on the curve for cuts, notches and band passes, at their gain otherwise.
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let handle_of = |b: usize, band: &audiostrada_dsp::eq::Band| {
        let db = if band.kind.has_gain() {
            band.gain
        } else {
            audiostrada_dsp::eq::band_db(&values, b, 0.0, f64::from(band.freq), rate).unwrap_or(0.0)
                as f32
        };
        pos2(
            x_of(band.freq),
            y_of(db.clamp(-db_range, db_range)).clamp(rect.top() + 9.0, rect.bottom() - 9.0),
        )
    };
    let handles: Vec<(usize, Pos2)> = bands
        .iter()
        .map(|(b, band)| (*b, handle_of(*b, band)))
        .collect();
    // The nearest handle within reach of a point: generous, the dots are small.
    let pick = |q: Pos2| {
        handles
            .iter()
            .filter(|(_, c)| c.distance(q) < 18.0)
            .min_by(|a, b| a.1.distance(q).total_cmp(&b.1.distance(q)))
            .map(|h| h.0)
    };
    let hovered = pointer.and_then(pick);
    for (&(b, band), &(_, center)) in bands.iter().zip(&handles) {
        let color = band_color(b);
        // A dynamic band: its range as a line, the gain it has now as a ring.
        if let Some((_, range)) = band.dynamic {
            let reach = y_of((band.gain + range).clamp(-db_range, db_range));
            painter.line_segment(
                [center, pos2(center.x, reach)],
                Stroke::new(3.0, color.gamma_multiply(0.35)),
            );
            let now = pos2(
                center.x,
                y_of((band.gain + offsets[b]).clamp(-db_range, db_range)),
            );
            painter.circle_stroke(now, 6.0, Stroke::new(2.0, color));
        }
        let r = if Some(b) == hovered || b == state.selected {
            9.0
        } else {
            7.5
        };
        if band.bypass {
            painter.circle_stroke(center, r, Stroke::new(1.5, color.gamma_multiply(0.6)));
        } else {
            painter.circle_filled(center, r, color);
            painter.circle_stroke(
                center,
                r,
                Stroke::new(if b == state.selected { 2.0 } else { 1.0 }, p.base),
            );
        }
        painter.text(
            center,
            Align2::CENTER_CENTER,
            (b + 1).to_string(),
            theme::strong(9.5),
            if band.bypass {
                color
            } else {
                Color32::from_rgb(20, 20, 24)
            },
        );
        if band.place != Place::Stereo {
            painter.text(
                center + vec2(0.0, r + 7.0),
                Align2::CENTER_CENTER,
                PLACE_SHORT[band.place as usize],
                theme::strong(9.0),
                color,
            );
        }
    }
    if bands.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr("Double-click to add a band"),
            theme::strong(14.0),
            p.weak,
        );
    }
    // Where the pointer is: its frequency, note and gain, in the corner.
    if let Some(at) = pointer.filter(|q| rect.contains(*q)) {
        let f = f_of(at.x);
        let note = {
            const NAMES: [&str; 12] = [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ];
            let n = (69.0 + 12.0 * (f / 440.0).log2()).round() as i32;
            format!(
                "{}{}",
                NAMES[n.rem_euclid(12) as usize],
                n.div_euclid(12) - 1
            )
        };
        let freq = if f >= 1000.0 {
            format!("{:.2} kHz", f / 1000.0)
        } else {
            format!("{f:.0} Hz")
        };
        painter.text(
            rect.left_top() + vec2(10.0, 8.0),
            Align2::LEFT_TOP,
            format!("{freq}  {note}  {:+.1} dB", db_of(at.y)),
            FontId::monospace(11.0),
            p.weak,
        );
        painter.line_segment(
            [pos2(at.x, rect.top()), pos2(at.x, rect.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.18)),
        );
    }

    // Mouse: drag a band (frequency and gain), wheel for Q, double-click to add or remove.
    let id = ui.scope_id().with(("eq-drag", params.insert.0));
    let drag = ui.interact(rect, id, Sense::click_and_drag());
    // Picked where the button went down: the drag starts once the pointer has moved off it.
    if drag.drag_started()
        && let Some(b) = ui
            .input(|i| i.pointer.press_origin())
            .and_then(pick)
            .or(hovered)
    {
        state.selected = b;
        ui.data_mut(|d| d.insert_temp(id, b));
    }
    let grabbed: Option<usize> = ui.data(|d| d.get_temp(id));
    if let Some(b) = grabbed
        && let Some(band) = audiostrada_dsp::eq::band(&values, b)
    {
        // Ctrl: up narrows the band (higher Q), down widens it; else frequency and gain.
        let ctrl = ui.input(|i| i.modifiers.command);
        let at = drag
            .interact_pointer_pos()
            .filter(|_| drag.dragged() && !ctrl);
        let freq = audiostrada_dsp::eq::param_id(b, "freq");
        params.change(&freq, at.map(|q| f_of(q.x)), &drag);
        if band.kind.has_gain() {
            let gain = audiostrada_dsp::eq::param_id(b, "gain");
            params.change(&gain, at.map(|q| db_of(q.y)), &drag);
        }
        let q = audiostrada_dsp::eq::param_id(b, "q");
        let dy = drag.drag_delta().y;
        let narrowed =
            (ctrl && drag.dragged() && dy != 0.0).then(|| params.get(&q) * 2f32.powf(-dy / 80.0));
        params.change(&q, narrowed, &drag);
        if drag.drag_stopped() {
            ui.data_mut(|d| d.remove::<usize>(id));
        }
    }
    if hovered.is_some() || grabbed.is_some() {
        let ctrl = ui.input(|i| i.modifiers.command);
        ui.ctx().set_cursor_icon(if ctrl {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::Grab
        });
    }
    if let Some(b) = hovered {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            let q = audiostrada_dsp::eq::param_id(b, "q");
            let factor = if wheel > 0.0 { 1.06 } else { 1.0 / 1.06 };
            params.change(&q, Some(params.get(&q) * factor), &drag);
            params.doc.history_mut().end_gesture();
        }
        if drag.clicked() {
            state.selected = b;
        }
    }
    if drag.double_clicked()
        && let Some(at) = drag.interact_pointer_pos()
    {
        match hovered {
            Some(b) => remove_band(params, state, b),
            None => add_band(params, state, f_of(at.x), db_of(at.y)),
        }
    }
    if drag.secondary_clicked()
        && let Some(b) = hovered
    {
        state.selected = b;
        ui.data_mut(|d| d.insert_temp(id.with("menu"), b));
    }
    let menu_band: Option<usize> = ui.data(|d| d.get_temp(id.with("menu")));
    egui::Popup::context_menu(&drag)
        .id(id.with("band-menu"))
        .show(|ui| {
            match menu_band.and_then(|b| audiostrada_dsp::eq::band(&values, b).map(|x| (b, x))) {
                Some((b, band)) => band_menu(ui, params, state, b, &band),
                None => {
                    ui.weak(tr("Double-click the curve to add a band"));
                }
            }
        });
    let hint = match hovered.and_then(|b| audiostrada_dsp::eq::band(&values, b).map(|x| (b, x))) {
        Some((b, band)) => {
            let mut text = format!(
                "{} {}: {}, {}",
                tr("Band"),
                b + 1,
                band_type_name(band.kind),
                format_value(&audiostrada_dsp::eq::INFO.params[3], band.freq)
            );
            if band.kind.has_gain() {
                text.push_str(&format!(", {:+.1} dB", band.gain));
            }
            text.push_str(&format!(", Q {:.2}", band.q));
            format!(
                "{text}\n{}",
                tr(
                    "Drag to move, Ctrl+drag or wheel for Q, double-click to remove, \
                     right-click for more"
                )
            )
        }
        None => tr("Double-click to add a band, drag a band to move it").to_owned(),
    };
    drag.on_hover_text(hint);

    ui.add_space(6.0);
    band_panel(ui, params, state, &bands);
}

/// Puts a band in the first free slot, at `freq` and `gain`.
fn add_band(params: &mut Params, state: &mut EditorState, freq: f32, gain: f32) {
    let Some(b) = (0..EQ_BANDS).find(|&b| audiostrada_dsp::eq::band(&params.values, b).is_none())
    else {
        params.doc.notify(tr("The EQ has 16 bands at most").into());
        return;
    };
    let kind = audiostrada_dsp::eq::new_band_type(freq);
    let id = |field: &str| audiostrada_dsp::eq::param_id(b, field);
    let changes = vec![
        (id("used"), 1.0),
        (id("bypass"), 0.0),
        (id("type"), kind.index() as f32),
        (id("freq"), freq),
        (id("gain"), if kind.has_gain() { gain } else { 0.0 }),
        (id("q"), if kind == BandType::Bell { 1.0 } else { 0.707 }),
        (id("slope"), 1.0),
        (id("dyn"), 0.0),
    ];
    edit_params(params, tr("Add EQ band"), &changes);
    state.selected = b;
}

fn remove_band(params: &mut Params, state: &mut EditorState, b: usize) {
    if state.eq_solo == Some(b) {
        state.eq_solo = None;
        solo_band(params, None);
    }
    edit_params(
        params,
        tr("Remove EQ band"),
        &[(audiostrada_dsp::eq::param_id(b, "used"), 0.0)],
    );
}

/// Right-click menu of a band: its type, dynamics, solo, bypass, removal.
fn band_menu(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut EditorState,
    b: usize,
    band: &audiostrada_dsp::eq::Band,
) {
    let id = |field: &str| audiostrada_dsp::eq::param_id(b, field);
    for kind in BandType::ALL {
        if ui
            .selectable_label(band.kind == kind, band_type_name(kind))
            .clicked()
        {
            edit_params(
                params,
                tr("EQ band type"),
                &[(id("type"), kind.index() as f32)],
            );
            ui.close();
        }
    }
    ui.separator();
    if band.kind.can_be_dynamic() {
        let label = if band.dynamic.is_some() {
            tr("Static band")
        } else {
            tr("Dynamic band")
        };
        if ui.button(label).clicked() {
            let on = if band.dynamic.is_some() { 0.0 } else { 1.0 };
            edit_params(params, tr("Dynamic EQ band"), &[(id("dyn"), on)]);
            ui.close();
        }
    }
    let soloed = state.eq_solo == Some(b);
    if ui
        .button(if soloed { tr("Stop solo") } else { tr("Solo") })
        .clicked()
    {
        state.eq_solo = (!soloed).then_some(b);
        solo_band(params, state.eq_solo);
        ui.close();
    }
    if ui
        .button(if band.bypass {
            tr("Enable")
        } else {
            tr("Bypass")
        })
        .clicked()
    {
        let off = if band.bypass { 0.0 } else { 1.0 };
        edit_params(params, tr("Bypass EQ band"), &[(id("bypass"), off)]);
        ui.close();
    }
    if ui.button(tr("Remove")).clicked() {
        remove_band(params, state, b);
        ui.close();
    }
}

/// The bands as colored chips, and the controls of the selected one.
fn band_panel(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut EditorState,
    bands: &[(usize, audiostrada_dsp::eq::Band)],
) {
    let p = theme::current(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for &(b, band) in bands {
            let color = band_color(b);
            let selected = b == state.selected;
            let (r, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            if band.bypass {
                ui.painter()
                    .circle_stroke(r.center(), 9.0, Stroke::new(1.5, color));
            } else {
                ui.painter().circle_filled(r.center(), 9.0, color);
            }
            if selected {
                ui.painter()
                    .circle_stroke(r.center(), 11.0, Stroke::new(1.5, p.text));
            }
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                (b + 1).to_string(),
                theme::strong(10.0),
                if band.bypass {
                    color
                } else {
                    Color32::from_rgb(20, 20, 24)
                },
            );
            if response
                .on_hover_text(format!(
                    "{} {}: {}",
                    tr("Band"),
                    b + 1,
                    band_type_name(band.kind)
                ))
                .clicked()
            {
                state.selected = b;
            }
        }
        if bands.is_empty() {
            ui.weak(tr("Double-click the curve to add a band"));
        }
        if let Some(b) = state.eq_solo {
            ui.add_space(8.0);
            ui.colored_label(theme::SOLO, crate::trf!("Band {band} alone", band = b + 1));
        }
    });
    ui.add_space(6.0);
    let accent = p.accent;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(&(b, band)) = bands.iter().find(|(b, _)| *b == state.selected) {
            let color = band_color(b);
            let id = |field: &str| audiostrada_dsp::eq::param_id(b, field);
            let title = format!("{} {}", tr("Band"), b + 1);
            eq_section(ui, &title, color, |ui| {
                ui.horizontal(|ui| {
                    choice_box(ui, params, &id("type"));
                    if band.kind.has_slope() {
                        choice_box(ui, params, &id("slope"));
                    }
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let current = Some(band.place as usize);
                    if let Some(i) = chips(ui, &PLACE_SHORT, &PLACE_HINTS, current) {
                        edit_params(params, tr("EQ band placement"), &[(id("place"), i as f32)]);
                    }
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let mut on = !band.bypass;
                    if pill_switch(ui, on, tr("On")).clicked() {
                        on = !on;
                        edit_params(
                            params,
                            tr("Bypass EQ band"),
                            &[(id("bypass"), if on { 0.0 } else { 1.0 })],
                        );
                    }
                    let soloed = state.eq_solo == Some(b);
                    let response = pill_switch(ui, soloed, tr("Solo"))
                        .on_hover_text(tr("Hear only the frequencies this band works on"));
                    if response.clicked() {
                        state.eq_solo = (!soloed).then_some(b);
                        solo_band(params, state.eq_solo);
                    }
                    if ui.button("✕").on_hover_text(tr("Remove")).clicked() {
                        remove_band(params, state, b);
                    }
                });
            });
            eq_section(ui, tr("Shape"), p.weak, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    wide_knob(ui, params, &id("freq"), tr("Frequency"), 44.0, color);
                    if band.kind.has_gain() {
                        wide_knob(ui, params, &id("gain"), tr("Gain"), 44.0, color);
                    }
                    wide_knob(ui, params, &id("q"), "Q", 44.0, color);
                });
            });
            eq_section(ui, tr("Dynamics"), p.weak, |ui| {
                if !band.kind.can_be_dynamic() {
                    ui.weak(tr("Bells and shelves can be dynamic"));
                    return;
                }
                let dynamic = band.dynamic.is_some();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_enabled_ui(dynamic, |ui| {
                        for (field, name) in [
                            ("thresh", tr("Threshold")),
                            ("range", tr("Range")),
                            ("attack", tr("Attack")),
                            ("release", tr("Release")),
                        ] {
                            wide_knob(ui, params, &id(field), name, 44.0, theme::SOLO);
                        }
                    });
                });
                ui.add_space(4.0);
                let response = pill_switch(ui, dynamic, tr("Dynamic")).on_hover_text(tr(
                    "The gain moves by up to the range while these frequencies are above the \
                     threshold",
                ));
                if response.clicked() {
                    edit_params(
                        params,
                        tr("Dynamic EQ band"),
                        &[(id("dyn"), if dynamic { 0.0 } else { 1.0 })],
                    );
                }
            });
        }
        // The output at the right end.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            eq_section(ui, tr("Output"), p.weak, |ui| {
                ui.horizontal(|ui| {
                    wide_knob(ui, params, "output", tr("Gain"), 44.0, accent);
                });
                ui.add_space(4.0);
                let values = params.values.clone();
                let rate = f64::from(params.doc.player().engine().sample_rate().max(1));
                params.toggle(ui, "auto_gain", |ui, on| {
                    let auto = audiostrada_dsp::eq::auto_gain_db(&values, rate);
                    let label = if *on {
                        format!("{} {auto:+.1} dB", tr("Auto"))
                    } else {
                        tr("Auto gain").to_owned()
                    };
                    let response = pill_switch(ui, *on, &label).on_hover_text(tr(
                        "Makes up for the boosts and cuts, so the level stays about the same",
                    ));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            });
        });
    });
}

/// A titled panel of the EQ's controls, all of one height.
fn eq_section(ui: &mut Ui, title: &str, color: Color32, add: impl FnOnce(&mut Ui)) {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_height(112.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(title).strong().size(11.5).color(color));
                ui.add_space(2.0);
                add(ui);
            });
        });
}

// ---------------------------------------------------------------- Tuner

fn tuner(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let p = theme::current(ui);
    let frequency = meters.first().copied().unwrap_or(0.0);
    let clarity = meters.get(1).copied().unwrap_or(0.0);
    let reference = params.get("reference");
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(360.0), 170.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, p.base);
    let center = pos2(rect.center().x, rect.top() + 70.0);
    let width = rect.width() * 0.42;
    for cents in (-50..=50).step_by(10) {
        let x = center.x + cents as f32 / 50.0 * width;
        let tall = cents % 50 == 0 || cents == 0;
        painter.line_segment(
            [
                pos2(x, center.y - if tall { 26.0 } else { 14.0 }),
                pos2(x, center.y),
            ],
            Stroke::new(if cents == 0 { 2.0 } else { 1.0 }, p.weak),
        );
    }
    if clarity > 0.5 && frequency > 0.0 {
        let (number, cents) = audiostrada_dsp::tuner::note(frequency, reference);
        let name = NAMES[number.rem_euclid(12) as usize];
        let octave = number.div_euclid(12) - 1;
        let close = cents.abs() < 3.0;
        let color = if close {
            theme::PLAY
        } else if cents.abs() < 15.0 {
            theme::SOLO
        } else {
            theme::RECORD
        };
        let x = center.x + cents.clamp(-50.0, 50.0) / 50.0 * width;
        painter.line_segment(
            [pos2(x, center.y - 34.0), pos2(x, center.y + 4.0)],
            Stroke::new(3.0, color),
        );
        painter.text(
            pos2(center.x, center.y + 46.0),
            Align2::CENTER_CENTER,
            format!("{name}{octave}"),
            theme::strong(40.0),
            color,
        );
        painter.text(
            pos2(rect.right() - 10.0, rect.bottom() - 8.0),
            Align2::RIGHT_BOTTOM,
            format!("{frequency:.1} Hz  {cents:+.0} cents"),
            FontId::monospace(12.0),
            p.weak,
        );
    } else {
        painter.text(
            pos2(center.x, center.y + 46.0),
            Align2::CENTER_CENTER,
            tr("Play a note"),
            theme::strong(18.0),
            p.weak,
        );
    }
    ui.ctx().request_repaint();
    ui.add_space(6.0);
    knob_panel(ui, params, &[]);
}

/// Pitch Correct: the sung pitch and the corrected one scrolling over the notes of the scale.
fn pitch_correct(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    use audiostrada_dsp::pitch_correct::{NOTE_NAMES, in_scale};
    const SPAN: f32 = 14.0;
    let p = theme::current(ui);
    let meter = |i: usize| meters.get(i).copied().unwrap_or(0.0);
    let (input, output, target) = (meter(0), meter(1), meter(2));
    push_history(state, (input, output));
    if output > 0.0 {
        state.pitch_view = if state.pitch_view > 0.0 {
            state.pitch_view + 0.03 * (output - state.pitch_view)
        } else {
            output
        };
    }
    let view = if state.pitch_view > 0.0 {
        state.pitch_view
    } else {
        60.0
    };
    let key = params.get("key").round() as i32;
    let scale = params.get("scale").round().max(0.0) as usize;
    let (rect, painter) = display(ui, 210.0);
    let graph = Rect::from_min_max(
        rect.min + vec2(40.0, 8.0),
        pos2(rect.right() - 160.0, rect.bottom() - 8.0),
    );
    let low = view - SPAN / 2.0;
    let to_y = |n: f32| graph.bottom() - (n - low) / SPAN * graph.height();
    let row = graph.height() / SPAN;
    let name = |n: i32| {
        format!(
            "{}{}",
            NOTE_NAMES[n.rem_euclid(12) as usize],
            n.div_euclid(12) - 1
        )
    };
    for n in low.floor() as i32..=(low + SPAN).ceil() as i32 {
        let y = to_y(n as f32);
        let band = Rect::from_min_max(
            pos2(graph.left(), y - row / 2.0),
            pos2(graph.right(), y + row / 2.0),
        );
        let used = in_scale(n, key, scale);
        if target > 0.0 && n == target as i32 {
            painter.rect_filled(band, 0.0, p.accent.gamma_multiply(0.18));
        } else if used && scale > 0 {
            painter.rect_filled(band, 0.0, p.panel.gamma_multiply(0.6));
        }
        painter.line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(1.0, p.weak.gamma_multiply(if used { 0.25 } else { 0.08 })),
        );
        if used {
            painter.text(
                pos2(rect.left() + 8.0, y),
                Align2::LEFT_CENTER,
                name(n),
                FontId::monospace(10.0),
                p.weak,
            );
        }
    }
    let step = graph.width() / HISTORY as f32;
    let x0 = graph.right() - state.history.len() as f32 * step;
    for (pick, color, width) in [(0, p.weak.gamma_multiply(0.7), 1.5), (1, p.accent, 2.5)] {
        let value = |v: (f32, f32)| if pick == 0 { v.0 } else { v.1 };
        let mut line = Vec::new();
        for (i, &v) in state.history.iter().enumerate() {
            if value(v) > 0.0 {
                line.push(pos2(x0 + i as f32 * step, to_y(value(v))));
            } else if !line.is_empty() {
                painter.add(Shape::line(
                    std::mem::take(&mut line),
                    Stroke::new(width, color),
                ));
            }
        }
        painter.add(Shape::line(line, Stroke::new(width, color)));
    }
    for (i, (text, color)) in [(tr("Input pitch"), p.weak), (tr("Output pitch"), p.accent)]
        .into_iter()
        .enumerate()
    {
        painter.text(
            graph.left_top() + vec2(8.0, 4.0 + i as f32 * 14.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(10.5),
            color,
        );
    }
    let readout = pos2(rect.right() - 80.0, rect.center().y);
    if target > 0.0 {
        painter.text(
            readout - vec2(0.0, 14.0),
            Align2::CENTER_CENTER,
            name(target as i32),
            theme::strong(38.0),
            p.accent,
        );
        painter.text(
            readout + vec2(0.0, 26.0),
            Align2::CENTER_CENTER,
            format!("{:+.0} cents", (output - input) * 100.0),
            FontId::monospace(12.0),
            p.weak,
        );
    } else {
        painter.text(
            readout,
            Align2::CENTER_CENTER,
            tr("Play a note"),
            theme::strong(15.0),
            p.weak,
        );
    }
    ui.ctx().request_repaint();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for id in ["key", "scale"] {
            if let Some(q) = params.param(id) {
                ui.label(RichText::new(tr(q.name)).size(10.5).color(p.weak));
            }
            choice_box(ui, params, id);
            ui.add_space(8.0);
        }
    });
    knob_row(
        ui,
        params,
        &["speed", "humanize", "amount", "transpose"],
        p.accent,
    );
}

// ---------------------------------------------------------------- Shared drawing

/// The area between a polyline and the horizontal line at `bottom`.
fn fill_under(points: &[Pos2], bottom: f32, color: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    for pair in points.windows(2) {
        let i = mesh.vertices.len() as u32;
        for q in [
            pair[0],
            pair[1],
            pos2(pair[1].x, bottom),
            pos2(pair[0].x, bottom),
        ] {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    Shape::mesh(mesh)
}

/// Remembers one frame of two meter values for the scrolling displays.
fn push_history(state: &mut EditorState, values: (f32, f32)) {
    state.history.push_back(values);
    while state.history.len() > HISTORY {
        state.history.pop_front();
    }
}

/// The on/off parameters of the effect as a row of buttons.
fn switches(ui: &mut Ui, params: &mut Params) {
    ui.horizontal(|ui| toggles(ui, params));
}

/// A button for each on/off parameter of the effect.
fn toggles(ui: &mut Ui, params: &mut Params) {
    for q in params.info.params.iter().filter(|q| q.toggle) {
        params.toggle(ui, q.id, |ui, on| ui.toggle_value(on, tr(q.name)));
    }
}

/// Vertical lines every round number of seconds across a time display.
fn grid_seconds(
    painter: &egui::Painter,
    rect: Rect,
    span: f32,
    to_x: &dyn Fn(f32) -> f32,
    color: Color32,
) {
    let step = match span {
        s if s > 4.0 => 1.0,
        s if s > 1.0 => 0.25,
        s if s > 0.4 => 0.1,
        _ => 0.025,
    };
    let mut t = step;
    while t < span {
        let x = to_x(t);
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, color),
        );
        t += step;
    }
}

// ---------------------------------------------------------------- Delay

fn delay(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    let synced = params.get("sync") >= 0.5;
    let bpm = params.doc.project().tempo.bpm_at_beat(0.0);
    let seconds = if synced {
        audiostrada_dsp::delay::synced_seconds(params.get("note"), bpm)
    } else {
        params.get("time") * 0.001
    };
    let (feedback, mix) = (params.get("feedback") / 100.0, params.get("mix") / 100.0);
    let pingpong = params.get("pingpong") >= 0.5;
    let (low, high) = (params.get("low_cut"), params.get("high_cut"));
    // Each pass through the filters loses some of the band: echoes darken as they repeat.
    let band = ((high / low).log2() / (20_000.0f32 / 20.0).log2()).clamp(0.1, 1.0);

    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(420.0), 150.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, p.base);
    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 18.0;
    // Echoes are drawn on a 48 dB scale, as far as they stay above it.
    let height = |a: f32| (1.0 + 20.0 * a.max(1e-6).log10() / 48.0).clamp(0.0, 1.0);
    let mut taps = 1;
    while taps < 16 && mix * feedback.powi(taps) > 0.004 {
        taps += 1;
    }
    let span = (seconds * (taps as f32 + 1.0))
        .max(seconds * 3.0)
        .clamp(0.1, 10.0);
    let to_x = |t: f32| rect.left() + 14.0 + t / span * (rect.width() - 28.0);
    let grid = p.weak.gamma_multiply(0.18);
    painter.line_segment(
        [pos2(rect.left(), mid), pos2(rect.right(), mid)],
        Stroke::new(1.0, grid),
    );
    grid_seconds(&painter, rect, span, &to_x, grid);
    let dry = height(1.0 - mix);
    for side in [-1.0, 1.0] {
        painter.line_segment(
            [
                pos2(to_x(0.0), mid),
                pos2(to_x(0.0), mid + side * half * dry),
            ],
            Stroke::new(3.0, p.weak),
        );
    }
    let mut level = mix;
    for n in 1..=taps {
        let x = to_x(seconds * n as f32);
        if x > rect.right() {
            break;
        }
        let color = p.accent.gamma_multiply(band.powi(n - 1).max(0.25));
        let sides: &[f32] = match (pingpong, n % 2) {
            (false, _) => &[-1.0, 1.0],
            (true, 1) => &[-1.0],
            (true, _) => &[1.0],
        };
        for side in sides {
            painter.line_segment(
                [pos2(x, mid), pos2(x, mid + side * half * height(level))],
                Stroke::new(3.0, color),
            );
        }
        level *= feedback;
    }
    painter.text(
        rect.left_top() + vec2(8.0, 4.0),
        Align2::LEFT_TOP,
        "L",
        FontId::proportional(9.0),
        p.weak,
    );
    painter.text(
        rect.left_bottom() + vec2(8.0, -4.0),
        Align2::LEFT_BOTTOM,
        "R",
        FontId::proportional(9.0),
        p.weak,
    );
    let time = if synced {
        let note = params
            .param("note")
            .and_then(|q| choice_name(q, params.get("note")))
            .unwrap_or("?");
        crate::trf!(
            "{note} at {bpm} BPM = {ms} ms",
            note = note,
            bpm = format!("{bpm:.0}"),
            ms = format!("{:.0}", seconds * 1000.0)
        )
    } else {
        format!("{:.0} ms", seconds * 1000.0)
    };
    painter.text(
        rect.right_top() + vec2(-8.0, 4.0),
        Align2::RIGHT_TOP,
        time,
        theme::strong(13.0),
        p.accent,
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Model")).size(10.5).color(p.weak));
        choice_box(ui, params, "model");
        ui.add_space(8.0);
        switches(ui, params);
    });
    ui.add_space(4.0);
    let analog = params.get("model") >= 0.5;
    ui.horizontal(|ui| {
        param_knob(
            ui,
            params,
            if synced { "note" } else { "time" },
            56.0,
            p.accent,
        );
        knobs(
            ui,
            params,
            &["feedback", "low_cut", "high_cut", "mix"],
            50.0,
            p.accent,
        );
        if analog {
            param_knob(ui, params, "wow", 50.0, theme::SOLO);
            param_knob(ui, params, "drive", 50.0, theme::SOLO);
        }
    });
}

// ---------------------------------------------------------------- Reverb

fn reverb(ui: &mut Ui, params: &mut Params) {
    use audiostrada_dsp::reverb::{SHIMMER, TYPE_HINTS, decay_seconds, uses};
    let kind = (params.get("type").round().max(0.0) as usize).min(SHIMMER);
    let (lows, highs) = decay_seconds(
        kind,
        params.get("decay"),
        params.get("bass"),
        params.get("treble"),
    );
    space_view(ui, params, lows, highs);
    ui.horizontal(|ui| {
        mode_buttons(ui, params, "type", &TYPE_HINTS);
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.add_space(14.0);
            switches(ui, params);
        });
    });
    ui.add_space(6.0);
    let accent = theme::current(ui).accent;
    ui.horizontal(|ui| {
        param_knob(ui, params, "decay", 60.0, accent);
        let ids = ["predelay", "high_cut", "low_cut", "width", "duck", "mix"];
        knobs(ui, params, &ids, 50.0, accent);
        if kind == SHIMMER {
            param_knob(ui, params, "shimmer", 50.0, theme::SOLO);
        }
    });
    ui.horizontal(|ui| {
        for id in ["size", "early", "treble", "bass", "diffusion", "modulation"] {
            ui.add_enabled_ui(uses(kind, id), |ui| {
                param_knob(ui, params, id, 44.0, accent)
            });
        }
    });
}

/// Decay envelopes of the lows and highs after the pre-delay on a 60 dB scale, with the
/// stereo width; shared by the reverbs.
fn space_view(ui: &mut Ui, params: &mut Params, lows: f32, highs: f32) {
    let p = theme::current(ui);
    let predelay = params.get("predelay") * 0.001;
    let (width, mix) = (params.get("width") / 100.0, params.get("mix") / 100.0);

    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(420.0), 150.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, p.base);
    let span = (predelay + lows * 1.1).clamp(0.5, 12.0);
    let inner = rect.shrink2(vec2(12.0, 16.0));
    let to_x = |t: f32| inner.left() + t / span * inner.width();
    // Levels on a 60 dB scale: each envelope falls 60 dB over its decay time.
    let to_y = |db: f32| inner.bottom() - (1.0 + db / 60.0).clamp(0.0, 1.0) * inner.height();
    let db = |a: f32| 20.0 * a.max(1e-6).log10();
    let grid = p.weak.gamma_multiply(0.18);
    grid_seconds(&painter, rect, span, &to_x, grid);
    painter.line_segment(
        [
            pos2(to_x(0.0), to_y(-60.0)),
            pos2(to_x(0.0), to_y(db(1.0 - mix))),
        ],
        Stroke::new(3.0, p.weak),
    );
    let envelope = |rt: f32| -> Vec<Pos2> {
        (0..=160)
            .map(|i| {
                let t = predelay + i as f32 / 160.0 * (span - predelay);
                let level = db(mix) - 60.0 * (t - predelay) / rt.max(0.01);
                pos2(to_x(t), to_y(level))
            })
            .collect()
    };
    let low_curve = envelope(lows);
    painter.add(fill_under(
        &low_curve,
        to_y(-60.0),
        p.accent.gamma_multiply(0.3),
    ));
    painter.add(Shape::line(low_curve, Stroke::new(2.0, p.accent)));
    painter.add(Shape::line(
        envelope(highs),
        Stroke::new(1.5, theme::SOLO.gamma_multiply(0.8)),
    ));
    if predelay > 0.0 {
        painter.line_segment(
            [
                pos2(to_x(predelay), inner.top()),
                pos2(to_x(predelay), inner.bottom()),
            ],
            Stroke::new(1.0, p.weak.gamma_multiply(0.5)),
        );
    }
    painter.text(
        rect.right_top() + vec2(-8.0, 4.0),
        Align2::RIGHT_TOP,
        format!("{} {lows:.1} s", tr("Decay")),
        theme::strong(13.0),
        p.accent,
    );
    painter.text(
        rect.right_top() + vec2(-8.0, 22.0),
        Align2::RIGHT_TOP,
        format!("{} {highs:.1} s", tr("Highs")),
        FontId::proportional(10.5),
        theme::SOLO,
    );
    // Stereo width as the spread of two dots.
    let center = pos2(rect.right() - 48.0, rect.top() + 48.0);
    let spread = 6.0 + width * 26.0;
    painter.line_segment(
        [center - vec2(32.0, 0.0), center + vec2(32.0, 0.0)],
        Stroke::new(1.0, grid.gamma_multiply(2.0)),
    );
    for side in [-1.0, 1.0] {
        painter.circle_filled(center + vec2(side * spread, 0.0), 4.0, p.accent);
    }
    ui.add_space(6.0);
}

// ---------------------------------------------------------------- Gate

fn gate(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    use audiostrada_dsp::gate::{DUCKER, EXPANDER, MODE_HINTS, MODES, expander_db};
    let mode = (params.get("mode").round().max(0.0) as usize).min(MODES.len() - 1);
    let gain = meters.first().copied().unwrap_or(0.0);
    let input = meters.get(1).copied().unwrap_or(-120.0);
    state.dynamics.push_back([gain, input, input + gain]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    cards(ui, params, "mode", &MODES, &MODE_HINTS);
    ui.add_space(8.0);
    let threshold = params.get("threshold");
    let close = threshold - params.get("hysteresis");
    let (range, ratio) = (params.get("range"), params.get("ratio"));
    let height = 230.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let width = (ui.available_width() - height - 60.0).max(260.0);
        dynamics_history(
            ui,
            params,
            state,
            vec2(width, height),
            (threshold, Some(close)),
            (gain, range.abs().max(6.0)),
        );
        // Output level for an input level, the gate open above the threshold.
        let out_of = |level: f32| match mode {
            DUCKER if level >= threshold => level + range,
            DUCKER => level,
            _ if level >= threshold => level,
            EXPANDER => level + expander_db(level, close, ratio, range),
            _ => level + range,
        };
        transfer_curve(ui, params, &out_of, threshold, input, height);
        gate_light(ui, gain, range, mode, height);
    });
    ui.add_space(8.0);
    knob_sections(
        ui,
        params,
        &[
            (
                "Detection",
                &["threshold", "hysteresis", "range", "ratio"],
                None,
            ),
            ("Time", &["attack", "hold", "release", "lookahead"], None),
            (
                "Sidechain",
                &["sc_hpf", "sc_lpf"],
                Some((
                    "listen",
                    "Listen",
                    "Hear what the detector hears: the key through its filters",
                )),
            ),
        ],
        &|id| id != "ratio" || mode == EXPANDER,
    );
}

/// Whether the gate lets the sound through, as a lamp, and its gain as a bar.
fn gate_light(ui: &mut Ui, gain: f32, range: f32, mode: usize, height: f32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let floor = range.min(-1.0);
    let depth = (gain / floor).clamp(0.0, 1.0);
    // A ducker is "open" while it lowers the sound: the lamp shows it working.
    let lit = if mode == audiostrada_dsp::gate::DUCKER {
        depth > 0.5
    } else {
        depth < 0.5
    };
    let lamp = pos2(rect.center().x, rect.top() + 12.0);
    let color = if lit {
        theme::PLAY
    } else {
        p.weak.gamma_multiply(0.4)
    };
    if lit {
        painter.circle_filled(lamp, 11.0, color.gamma_multiply(0.18));
    }
    painter.circle_filled(lamp, 6.5, color);
    let bar = Rect::from_min_max(
        pos2(rect.center().x - 6.0, rect.top() + 30.0),
        pos2(rect.center().x + 6.0, rect.bottom() - 16.0),
    );
    painter.rect_filled(bar, 4.0, p.base);
    let open = 1.0 - depth;
    painter.rect_filled(
        Rect::from_min_max(
            pos2(bar.left(), bar.bottom() - open * bar.height()),
            bar.max,
        ),
        4.0,
        theme::PLAY.gamma_multiply(0.8),
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 2.0),
        Align2::CENTER_BOTTOM,
        format!("{:.0}", gain.round() + 0.0),
        FontId::monospace(10.0),
        p.weak,
    );
}

// ---------------------------------------------------------------- Vintage compressor

/// An analog VU meter showing gain reduction: cream face, needle with VU ballistics.
fn vu_meter(ui: &mut Ui, reduction: f32, needle: &mut f32, size: egui::Vec2, amber: bool) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    // VU ballistics: about 300 ms to settle, from the frame time.
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let target = reduction.clamp(-20.0, 0.0);
    *needle += (target - *needle) * (1.0 - (-dt / 0.12).exp());
    // Face, ink and needle: backlit amber, or the theme's dark or cream.
    let (face_top, face_bottom, ink, red, hand) = if amber {
        (
            Color32::from_rgb(250, 206, 112),
            Color32::from_rgb(222, 152, 52),
            Color32::from_rgb(52, 34, 18),
            Color32::from_rgb(214, 46, 24),
            Color32::from_rgb(30, 22, 14),
        )
    } else if p.dark {
        (
            Color32::from_rgb(70, 74, 79),
            Color32::from_rgb(46, 49, 53),
            Color32::from_rgb(228, 230, 232),
            Color32::from_rgb(236, 92, 72),
            Color32::from_rgb(244, 244, 240),
        )
    } else {
        (
            Color32::from_rgb(246, 244, 238),
            Color32::from_rgb(228, 225, 216),
            Color32::from_rgb(30, 30, 30),
            Color32::from_rgb(210, 58, 40),
            Color32::from_rgb(24, 24, 24),
        )
    };
    // The bezel, then the face behind its glass.
    let (bezel_top, bezel_bottom) = if p.dark {
        (Color32::from_gray(58), Color32::from_gray(26))
    } else {
        (Color32::from_gray(212), Color32::from_gray(170))
    };
    theme::gradient_rect(&painter, rect, bezel_top, bezel_bottom);
    let face = rect.shrink(9.0);
    theme::gradient_rect(&painter, face, face_top, face_bottom);
    painter.rect_stroke(
        face,
        3.0,
        Stroke::new(1.5, Color32::from_black_alpha(140)),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter_at(face);
    // The scale as on a real VU: positions follow the voltage, -20 to +3 dB.
    let span = (10f32.powf(3.0 / 20.0) - 0.1).max(1e-3);
    let position = |db: f32| (10f32.powf(db / 20.0) - 0.1) / span;
    let radius = face.height() * 0.95;
    let pivot = pos2(face.center().x, face.top() + 20.0 + radius);
    let sweep = 0.82;
    let at = |db: f32, r: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + (position(db) - 0.5) * 2.0 * sweep;
        pivot + vec2(a.cos(), a.sin()) * r
    };
    let arc_r = radius * 0.8;
    let arc = |from: f32, to: f32, r: f32| -> Vec<Pos2> {
        (0..=48)
            .map(|i| at(from + (to - from) * i as f32 / 48.0, r))
            .collect()
    };
    painter.add(Shape::line(arc(-20.0, 0.0, arc_r), Stroke::new(1.4, ink)));
    painter.add(Shape::line(arc(0.0, 3.0, arc_r), Stroke::new(1.4, red)));
    // The red zone over 0, as a band.
    painter.add(Shape::line(
        arc(0.0, 3.0, arc_r + 4.0),
        Stroke::new(6.0, red),
    ));
    let big = FontId::proportional((face.height() * 0.075).clamp(9.0, 14.0));
    let small = FontId::proportional((face.height() * 0.058).clamp(8.0, 11.0));
    for (db, label) in [
        (-20.0, "-20"),
        (-10.0, "-10"),
        (-7.0, "-7"),
        (-5.0, "-5"),
        (-3.0, "-3"),
        (-2.0, "-2"),
        (-1.0, "-1"),
        (0.0, "0"),
        (1.0, "+1"),
        (2.0, "+2"),
        (3.0, "+3"),
    ] {
        let color = if db > 0.0 { red } else { ink };
        painter.line_segment(
            [at(db, arc_r), at(db, arc_r + 9.0)],
            Stroke::new(1.3, color),
        );
        painter.text(
            at(db, arc_r + 20.0),
            Align2::CENTER_CENTER,
            label,
            big.clone(),
            color,
        );
    }
    for db in [-15.0f32, -0.5, 0.5, 1.5, 2.5] {
        let color = if db > 0.0 { red } else { ink };
        painter.line_segment(
            [at(db, arc_r), at(db, arc_r + 5.0)],
            Stroke::new(1.0, color),
        );
    }
    // Percent of the reference level, inside the arc.
    for pct in [0.0f32, 20.0, 40.0, 60.0, 80.0, 100.0] {
        let db = 20.0 * (pct.max(10.0) / 100.0).log10();
        let db = if pct == 0.0 { -20.0 } else { db };
        painter.line_segment([at(db, arc_r - 5.0), at(db, arc_r)], Stroke::new(1.0, ink));
        painter.text(
            at(db, arc_r - 14.0),
            Align2::CENTER_CENTER,
            format!("{pct:.0}"),
            small.clone(),
            ink.gamma_multiply(0.85),
        );
    }
    for (x, align) in [
        (face.left() + 10.0, Align2::LEFT_BOTTOM),
        (face.right() - 10.0, Align2::RIGHT_BOTTOM),
    ] {
        painter.text(pos2(x, face.bottom() - 8.0), align, "VU", big.clone(), ink);
    }
    // The maker's mark and what it reads, in the middle.
    let mark = pos2(face.center().x, face.top() + face.height() * 0.6);
    if let Some(logo) = crate::logo(ui.ctx()) {
        let h = (face.height() * 0.09).clamp(10.0, 18.0);
        painter.image(
            logo.id(),
            Rect::from_center_size(mark - vec2(0.0, h * 0.7), vec2(h * 105.0 / 96.0, h)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    painter.text(
        mark + vec2(0.0, 6.0),
        Align2::CENTER_CENTER,
        tr("GAIN REDUCTION"),
        small,
        ink.gamma_multiply(0.75),
    );
    // The needle from its pivot, under a cap at the bottom.
    let tip = at(*needle, arc_r + 12.0);
    painter.line_segment(
        [pivot, tip],
        Stroke::new(2.0, Color32::from_black_alpha(50)),
    );
    painter.line_segment([pivot, tip], Stroke::new(1.4, hand));
    let cap = pos2(face.center().x, face.bottom());
    painter.circle_filled(cap, face.height() * 0.11, bezel_bottom);
    painter.circle_stroke(
        cap,
        face.height() * 0.11,
        Stroke::new(1.0, Color32::from_black_alpha(120)),
    );
    // A light reflection on the glass.
    let mut glass = egui::Mesh::default();
    for (pos, alpha) in [
        (face.left_top(), 30u8),
        (face.right_top(), 6),
        (pos2(face.right(), face.top() + face.height() * 0.45), 0),
        (pos2(face.left(), face.top() + face.height() * 0.45), 8),
    ] {
        glass.colored_vertex(pos, Color32::from_white_alpha(alpha));
    }
    glass.add_triangle(0, 1, 2);
    glass.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(glass));
    ui.ctx().request_repaint();
}

// ---------------------------------------------------------------- New effects

/// A row of knobs for parameter ids, the first one larger.
pub(crate) fn knob_row(ui: &mut Ui, params: &mut Params, ids: &[&str], tint: Color32) {
    ui.horizontal(|ui| {
        param_knob(ui, params, ids[0], 58.0, tint);
        knobs(ui, params, &ids[1..], 50.0, tint);
    });
}

/// Knobs of one size for parameter ids, in the current layout.
fn knobs(ui: &mut Ui, params: &mut Params, ids: &[&str], size: f32, tint: Color32) {
    for id in ids {
        param_knob(ui, params, id, size, tint);
    }
}

/// The channel strip: its curve with handles to drag (EQ bands, high and low pass), the gate
/// and the compressor's meters, then each module with its switch, in the order they play.
fn channel_strip(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    use audiostrada_dsp::Effect;
    let p = theme::current(ui);
    let (rect, painter) = display(ui, 170.0);
    let graph = Rect::from_min_max(rect.min + vec2(40.0, 12.0), rect.max - vec2(70.0, 18.0));
    let x_of = |f: f32| graph.left() + graph.width() * ((f / 20.0).max(1e-3).log10() / 3.0);
    let f_of =
        |x: f32| 20.0 * 10f32.powf(((x - graph.left()) / graph.width()).clamp(0.0, 1.0) * 3.0);
    let y_of = |db: f32| graph.center().y - db / 18.0 * graph.height() * 0.5;
    let db_of =
        |y: f32| ((graph.center().y - y) / (graph.height() * 0.5) * 18.0).clamp(-15.0, 15.0);
    let small = FontId::proportional(9.0);
    for db in [-12.0f32, -6.0, 0.0, 6.0, 12.0] {
        let y = y_of(db);
        painter.line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(
                1.0,
                p.weak.gamma_multiply(if db == 0.0 { 0.35 } else { 0.12 }),
            ),
        );
        painter.text(
            pos2(graph.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{db:+.0}"),
            small.clone(),
            p.weak,
        );
    }
    for (f, label) in [
        (50.0, "50"),
        (100.0, "100"),
        (500.0, "500"),
        (1000.0, "1k"),
        (5000.0, "5k"),
        (10_000.0, "10k"),
    ] {
        let x = x_of(f);
        painter.line_segment(
            [pos2(x, graph.top()), pos2(x, graph.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.1)),
        );
        painter.text(
            pos2(x, graph.bottom() + 3.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            p.weak,
        );
    }
    // The response of what is on.
    let mut dsp = audiostrada_dsp::channel::Channel::default();
    dsp.prepare(48_000, 64);
    let character = crate::console_ui::character(params.doc, params.channel);
    for (i, (v, q)) in params.values.iter().zip(params.info.params).enumerate() {
        dsp.set_param(
            i,
            if q.id == "character" {
                character as f32
            } else {
                *v
            },
        );
    }
    let points: Vec<Pos2> = (0..=160)
        .map(|i| {
            let x = graph.left() + graph.width() * i as f32 / 160.0;
            let db = dsp.eq_response_db(f64::from(f_of(x))).clamp(-18.0, 18.0) as f32;
            pos2(x, y_of(db))
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, p.accent)));
    // Handles: each band's frequency across and gain up and down; the filters across only.
    let eq_on = params.get("eq") >= 0.5;
    let filters_on = params.get("filters") >= 0.5;
    let handles: [(&str, Option<&str>, &str, Color32); 6] = [
        ("hpf", None, "filters", theme::family_color("EQ & Filter")),
        (
            "low_freq",
            Some("low_gain"),
            "eq",
            Color32::from_rgb(236, 120, 90),
        ),
        (
            "lmf_freq",
            Some("lmf_gain"),
            "eq",
            Color32::from_rgb(230, 190, 80),
        ),
        (
            "hmf_freq",
            Some("hmf_gain"),
            "eq",
            Color32::from_rgb(110, 200, 120),
        ),
        (
            "high_freq",
            Some("high_gain"),
            "eq",
            Color32::from_rgb(110, 170, 240),
        ),
        ("lpf", None, "filters", theme::family_color("EQ & Filter")),
    ];
    for (freq, gain, module, color) in handles {
        let on = if module == "eq" { eq_on } else { filters_on };
        let center = pos2(
            x_of(params.get(freq)),
            gain.map_or(y_of(0.0), |g| y_of(params.get(g))),
        );
        let response = ui.interact(
            Rect::from_center_size(center, vec2(16.0, 16.0)),
            ui.scope_id().with(("strip-handle", freq)),
            Sense::drag(),
        );
        let hot = response.hovered() || response.dragged();
        painter.circle_filled(
            center,
            if hot { 7.0 } else { 5.5 },
            if on { color } else { color.gamma_multiply(0.3) },
        );
        painter.circle_stroke(
            center,
            if hot { 7.0 } else { 5.5 },
            Stroke::new(1.0, Color32::BLACK.gamma_multiply(0.5)),
        );
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.drag_started() && !on {
            // Grabbing a handle switches its module on.
            params.set(module, Some(1.0), &response);
        }
        if let Some(pos) = response
            .interact_pointer_pos()
            .filter(|_| response.dragged())
        {
            let range = params
                .param(freq)
                .map_or((20.0, 20_000.0), |p| (p.min, p.max));
            params.set(freq, Some(f_of(pos.x).clamp(range.0, range.1)), &response);
            if let Some(g) = gain {
                params.set(g, Some(db_of(pos.y)), &response);
            }
        } else {
            params.set(freq, None, &response);
        }
    }
    // Gate and gain reduction meters at the right.
    let gate = meters.first().copied().unwrap_or(0.0).clamp(-40.0, 0.0);
    let reduction = meters.get(1).copied().unwrap_or(0.0).abs().min(24.0);
    for (k, (value, full, label, color)) in [
        (-gate, 40.0, tr("Gate"), theme::PLAY),
        (reduction, 24.0, tr("GR"), theme::family_color("Dynamics")),
    ]
    .into_iter()
    .enumerate()
    {
        let x = rect.right() - 50.0 + k as f32 * 24.0;
        let bar = Rect::from_min_max(pos2(x, graph.top()), pos2(x + 10.0, graph.bottom()));
        painter.rect_filled(bar, 3.0, p.panel);
        let h = bar.height() * (value / full).clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + h)),
            3.0,
            color,
        );
        painter.text(
            pos2(bar.center().x, bar.bottom() + 3.0),
            Align2::CENTER_TOP,
            label,
            small.clone(),
            p.weak,
        );
    }
    ui.add_space(6.0);
    // The console it plays through, which gives the EQ and the drive their character.
    ui.horizontal(|ui| {
        let console = crate::console_ui::of_channel(params.doc, params.channel);
        ui.label(RichText::new(tr("Console")).color(p.weak));
        let mut text = RichText::new(crate::console_ui::name(console.model)).strong();
        if console.colors() {
            text = text.color(crate::console_ui::color(console.model));
        }
        let channel = params.channel;
        let button = ui.button(text).on_hover_text(tr(
            "The console this channel plays through: its preamp colors the track, and the EQ \
             and the drive take its character",
        ));
        // Stays open while its settings change.
        egui::Popup::menu(&button)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| match channel {
                Channel::Track(t) => crate::console_ui::track_menu(ui, params.doc, t),
                Channel::Master => crate::console_ui::console_menu(ui, params.doc),
            });
    });
    ui.add_space(6.0);
    // The modules, in the order they play; dragged by their grip to reorder them.
    use audiostrada_dsp::channel::{MODULES, module_order, order_code};
    let order = module_order(params.get("order"));
    let drag_id = ui.scope_id().with("strip-module-drag");
    let dragged: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let mut frames = [Rect::NOTHING; 5];
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (slot, &m) in order.iter().enumerate() {
            let module = MODULES[m];
            let (title, family) = match module {
                "filters" => ("Filters", "EQ & Filter"),
                "gate" => ("Gate", "Dynamics"),
                "comp" => ("Compressor", "Dynamics"),
                "eq" => ("EQ", "EQ & Filter"),
                _ => ("Drive", "Color"),
            };
            let tint = theme::family_color(family);
            let border = if dragged == Some(m) {
                p.accent
            } else {
                Color32::TRANSPARENT
            };
            let frame = egui::Frame::new()
                .fill(p.base)
                .stroke(Stroke::new(1.0, border))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (grip, response) =
                                ui.allocate_exact_size(vec2(10.0, 22.0), Sense::drag());
                            let hot = response.hovered() || response.dragged();
                            let ink = if hot {
                                p.text
                            } else {
                                p.weak.gamma_multiply(0.6)
                            };
                            for k in 0..6 {
                                let dot = pos2(
                                    grip.left() + 3.0 + (k % 2) as f32 * 4.0,
                                    grip.center().y - 4.0 + (k / 2) as f32 * 4.0,
                                );
                                ui.painter().circle_filled(dot, 1.1, ink);
                            }
                            if hot {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }
                            if response.drag_started() {
                                ui.data_mut(|d| d.insert_temp(drag_id, m));
                            }
                            response.on_hover_text(tr("Drag to change the order of the modules"));
                            module_switch(ui, params, module, tr(title), tint);
                        });
                        let on = params.get(module) >= 0.5;
                        let tint = if on { tint } else { p.weak };
                        let row =
                            |ui: &mut Ui, params: &mut Params, list: &[(&str, &'static str)]| {
                                ui.horizontal(|ui| {
                                    for (id, label) in list {
                                        named_knob(ui, params, id, tr(label), 40.0, tint);
                                    }
                                });
                            };
                        match module {
                            "filters" => {
                                row(ui, params, &[("hpf", "High pass"), ("lpf", "Low pass")]);
                                ui.horizontal(|ui| segmented(ui, params, "slope", &[]));
                            }
                            "gate" => {
                                row(
                                    ui,
                                    params,
                                    &[("gate_threshold", "Threshold"), ("gate_range", "Range")],
                                );
                                row(
                                    ui,
                                    params,
                                    &[("gate_attack", "Attack"), ("gate_release", "Release")],
                                );
                            }
                            "comp" => {
                                ui.horizontal(|ui| {
                                    segmented(
                                        ui,
                                        params,
                                        "comp_type",
                                        &audiostrada_dsp::compressor::TYPE_HINTS,
                                    )
                                });
                                row(
                                    ui,
                                    params,
                                    &[
                                        ("comp_threshold", "Threshold"),
                                        ("comp_ratio", "Ratio"),
                                        ("comp_makeup", "Makeup"),
                                    ],
                                );
                                row(
                                    ui,
                                    params,
                                    &[
                                        ("comp_attack", "Attack"),
                                        ("comp_release", "Release"),
                                        ("comp_mix", "Mix"),
                                    ],
                                );
                            }
                            "eq" => {
                                row(
                                    ui,
                                    params,
                                    &[
                                        ("low_freq", "Low"),
                                        ("lmf_freq", "Low mid"),
                                        ("hmf_freq", "High mid"),
                                        ("high_freq", "High"),
                                    ],
                                );
                                row(
                                    ui,
                                    params,
                                    &[
                                        ("low_gain", "Gain"),
                                        ("lmf_gain", "Gain"),
                                        ("hmf_gain", "Gain"),
                                        ("high_gain", "Gain"),
                                    ],
                                );
                                ui.horizontal(|ui| {
                                    ui.add_space(58.0 + ui.spacing().item_spacing.x);
                                    named_knob(ui, params, "lmf_q", "Q", 40.0, tint);
                                    named_knob(ui, params, "hmf_q", "Q", 40.0, tint);
                                });
                            }
                            _ => {
                                row(
                                    ui,
                                    params,
                                    &[("drive_amount", "Amount"), ("output", "Output")],
                                );
                                let character =
                                    audiostrada_dsp::channel::CHARACTERS[character.min(4)];
                                ui.label(
                                    RichText::new(trf!("Character: {name}", name = tr(character)))
                                        .size(10.0)
                                        .color(p.weak),
                                );
                            }
                        }
                    });
                });
            frames[slot] = frame.response.rect;
        }
    });
    let Some(m) = dragged else {
        return;
    };
    // Where the dragged module would go: among the others, by the pointer's x.
    let others: Vec<usize> = order.iter().copied().filter(|&o| o != m).collect();
    let rect_of = |module: usize| frames[order.iter().position(|&o| o == module).unwrap_or(0)];
    let x = ui.input(|i| i.pointer.hover_pos()).map_or(0.0, |pos| pos.x);
    let target = others
        .iter()
        .filter(|&&o| rect_of(o).center().x < x)
        .count();
    let line_x = match target {
        0 => rect_of(others[0]).left() - 4.0,
        t => rect_of(others[t - 1]).right() + 4.0,
    };
    let top = frames.iter().map(|r| r.top()).fold(f32::MAX, f32::min);
    let bottom = frames.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
    ui.painter().line_segment(
        [pos2(line_x, top), pos2(line_x, bottom)],
        Stroke::new(2.0, p.accent),
    );
    if ui.input(|i| !i.pointer.any_down()) {
        ui.data_mut(|d| d.remove::<usize>(drag_id));
        let mut new = [m; 5];
        let mut rest = others.iter();
        for (k, slot) in new.iter_mut().enumerate() {
            if k != target {
                *slot = *rest.next().unwrap_or(&m);
            }
        }
        if new != order {
            let code = order_code(new);
            if let Some(i) = params.info.params.iter().position(|q| q.id == "order") {
                params.values[i] = code;
            }
            let (channel, insert) = (params.channel, params.insert);
            params
                .doc
                .history_mut()
                .edit(tr("Reorder the channel strip"), |p| {
                    routing::set_insert_param(p, channel, insert, "order", code)
                });
        }
    }
}

/// A module's on/off switch: the power sign and its name, lit in its color while on.
fn module_switch(ui: &mut Ui, params: &mut Params, id: &str, title: &str, tint: Color32) {
    let p = theme::current(ui);
    let on = params.get(id) >= 0.5;
    let galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), FontId::proportional(13.0), Color32::WHITE);
    let size = vec2(galley.size().x + 32.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = match (on, response.hovered()) {
        (true, _) => tint.gamma_multiply(0.3),
        (false, true) => p.hover,
        (false, false) => p.surface,
    };
    let ink = if on { p.text } else { p.weak };
    ui.painter().rect_filled(rect, 5.0, fill);
    theme::paint_power(
        ui.painter(),
        Rect::from_min_size(rect.min + vec2(3.0, 2.0), vec2(18.0, 18.0)),
        theme::power_ink(on, &p),
    );
    ui.painter().galley_with_override_text_color(
        pos2(rect.left() + 23.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    let response = response.on_hover_text(if on {
        tr("On: click to switch the module off")
    } else {
        tr("Off: click to switch the module on")
    });
    let changed = response.clicked().then_some(if on { 0.0 } else { 1.0 });
    params.set(id, changed, &response);
}

/// A dark display area with a subtle frame.
fn display(ui: &mut Ui, height: f32) -> (Rect, egui::Painter) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width().max(420.0), height),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, p.weak.gamma_multiply(0.15)),
        egui::StrokeKind::Inside,
    );
    (rect, painter)
}

fn tape(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    let (rect, painter) = display(ui, 160.0);
    let speed = [0.6, 1.0, 1.6][(params.get("speed").round() as usize).min(2)];
    let wow = params.get("wow") / 100.0;
    let time = ui.input(|i| i.time) as f32;
    let angle = time * speed * 2.2 + wow * 0.15 * (time * 3.7).sin();
    let reel = |center: Pos2, radius: f32, full: f32, angle: f32| {
        painter.circle_filled(center, radius, Color32::from_rgb(52, 46, 40));
        painter.circle_filled(
            center,
            radius * (0.35 + 0.6 * full),
            Color32::from_rgb(70, 52, 38),
        );
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(2.0, Color32::from_rgb(150, 140, 125)),
        );
        for k in 0..3 {
            let a = angle + k as f32 * std::f32::consts::TAU / 3.0;
            let hole = center + vec2(a.cos(), a.sin()) * radius * 0.55;
            painter.circle_filled(hole, radius * 0.18, p.base);
        }
        painter.circle_filled(center, radius * 0.12, Color32::from_rgb(190, 180, 160));
    };
    let r = rect.height() * 0.36;
    let left = pos2(rect.left() + r + 24.0, rect.center().y - 6.0);
    let right = pos2(rect.left() + r * 3.0 + 70.0, rect.center().y - 6.0);
    reel(left, r, 0.75, angle);
    reel(right, r, 0.45, angle * 1.3);
    let head = pos2((left.x + right.x) / 2.0, rect.bottom() - 18.0);
    painter.line_segment(
        [left + vec2(0.0, r * 0.95), head],
        Stroke::new(2.0, Color32::from_rgb(90, 66, 44)),
    );
    painter.line_segment(
        [head, right + vec2(0.0, r * 0.75)],
        Stroke::new(2.0, Color32::from_rgb(90, 66, 44)),
    );
    painter.rect_filled(
        Rect::from_center_size(head + vec2(0.0, 6.0), vec2(30.0, 10.0)),
        2.0,
        Color32::from_rgb(170, 170, 175),
    );
    let info_x = right.x + r + 30.0;
    let drive = params.get("drive");
    painter.text(
        pos2(info_x, rect.top() + 22.0),
        Align2::LEFT_TOP,
        SPEED_NAMES[(params.get("speed").round() as usize).min(2)],
        theme::strong(20.0),
        theme::SOLO,
    );
    painter.text(
        pos2(info_x, rect.top() + 52.0),
        Align2::LEFT_TOP,
        format!("{} {drive:+.1} dB", tr("Drive")),
        FontId::proportional(12.0),
        p.text,
    );
    painter.text(
        pos2(info_x, rect.top() + 72.0),
        Align2::LEFT_TOP,
        format!("{} {:.1} dB", tr("Head bump"), params.get("bump")),
        FontId::proportional(12.0),
        p.weak,
    );
    painter.text(
        pos2(info_x, rect.top() + 92.0),
        Align2::LEFT_TOP,
        format!("{} {:.0} %", tr("Wow & flutter"), params.get("wow")),
        FontId::proportional(12.0),
        p.weak,
    );
    ui.ctx().request_repaint();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Speed")).size(10.5).color(p.weak));
        choice_box(ui, params, "speed");
    });
    knob_row(
        ui,
        params,
        &["drive", "bump", "wow", "hiss", "bias", "output", "mix"],
        theme::SOLO,
    );
}

const SPEED_NAMES: [&str; 3] = ["7.5 ips", "15 ips", "30 ips"];

/// The LFO of a modulation effect drawn over two cycles, with a dot running along it.
fn modulation(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    let id = params.info.id;
    let rate = params.get("rate");
    let depth = params.get("depth") / 100.0;
    let stereo = match id {
        "chorus" => 0.25,
        _ => params.get("stereo") / 360.0,
    };
    let (rect, painter) = display(ui, 130.0);
    let inner = rect.shrink2(vec2(14.0, 18.0));
    let wave = |phase: f32| -> f32 {
        match id {
            "flanger" => {
                let t = phase.rem_euclid(1.0);
                if t < 0.5 { t * 2.0 } else { 2.0 - t * 2.0 }
            }
            "phaser" => 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos(),
            _ => 0.5 + 0.5 * (phase * std::f32::consts::TAU).sin(),
        }
    };
    let point = |phase: f32, x: f32| {
        pos2(
            x,
            inner.bottom() - (0.5 + (wave(phase) - 0.5) * depth) * inner.height(),
        )
    };
    for (offset, color) in [(0.0, p.accent), (stereo, theme::SOLO)] {
        let line: Vec<Pos2> = (0..=200)
            .map(|i| {
                let t = i as f32 / 200.0;
                point(t * 2.0 + offset, inner.left() + t * inner.width())
            })
            .collect();
        painter.add(Shape::line(line, Stroke::new(2.0, color)));
    }
    let time = ui.input(|i| i.time) as f32;
    let phase = (time * rate).fract();
    for (offset, color) in [(0.0, p.accent), (stereo, theme::SOLO)] {
        let x = inner.left() + phase * 0.5 * inner.width();
        painter.circle_filled(point(phase + offset, x), 5.0, color);
        let x2 = x + 0.5 * inner.width();
        painter.circle_filled(point(phase + offset, x2), 5.0, color.gamma_multiply(0.5));
    }
    painter.text(
        rect.right_top() + vec2(-10.0, 8.0),
        Align2::RIGHT_TOP,
        format!("{rate:.2} Hz"),
        theme::strong(14.0),
        p.accent,
    );
    painter.text(
        rect.left_top() + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        "L",
        FontId::proportional(10.0),
        p.accent,
    );
    painter.text(
        rect.left_top() + vec2(22.0, 8.0),
        Align2::LEFT_TOP,
        "R",
        FontId::proportional(10.0),
        theme::SOLO,
    );
    ui.ctx().request_repaint();
    ui.add_space(8.0);
    match id {
        "flanger" => knob_row(
            ui,
            params,
            &[
                "rate", "depth", "delay", "feedback", "stereo", "mix", "output",
            ],
            p.accent,
        ),
        "phaser" => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Stages")).size(10.5).color(p.weak));
                choice_box(ui, params, "stages");
            });
            knob_row(
                ui,
                params,
                &["rate", "depth", "low", "high", "feedback", "stereo", "mix"],
                p.accent,
            );
        }
        _ => knob_row(
            ui,
            params,
            &["rate", "depth", "delay", "feedback", "mix"],
            p.accent,
        ),
    }
}

/// Utility: the phase correlation and the left/right balance read live, then the level and
/// stereo controls, then the phase tools.
fn utility(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    let p = theme::current(ui);
    let correlation = meters.first().copied().unwrap_or(1.0);
    let lean = meters.get(1).copied().unwrap_or(0.0);
    let (rect, painter) = display(ui, 86.0);
    // Two scales: correlation from -1 to +1, balance from left to right (12 dB each way).
    let bar = |row: usize, at: f32, left: &str, right: &str, title: String, warn: bool| {
        let y = rect.top() + 30.0 + row as f32 * 38.0;
        let line = Rect::from_min_max(
            pos2(rect.left() + 60.0, y - 3.0),
            pos2(rect.right() - 60.0, y + 3.0),
        );
        painter.rect_filled(line, 3.0, p.panel);
        painter.line_segment(
            [
                pos2(line.center().x, line.top() - 4.0),
                pos2(line.center().x, line.bottom() + 4.0),
            ],
            Stroke::new(1.0, p.weak.gamma_multiply(0.5)),
        );
        let x = line.left() + at.clamp(0.0, 1.0) * line.width();
        painter.circle_filled(
            pos2(x, line.center().y),
            6.0,
            if warn { theme::RECORD } else { theme::PLAY },
        );
        let small = FontId::proportional(10.0);
        painter.text(
            pos2(line.left() - 10.0, y),
            Align2::RIGHT_CENTER,
            left,
            small.clone(),
            p.weak,
        );
        painter.text(
            pos2(line.right() + 10.0, y),
            Align2::LEFT_CENTER,
            right,
            small,
            p.weak,
        );
        painter.text(
            pos2(line.center().x, line.top() - 6.0),
            Align2::CENTER_BOTTOM,
            title,
            FontId::proportional(11.0),
            p.text,
        );
    };
    bar(
        0,
        (correlation + 1.0) / 2.0,
        "-1",
        "+1",
        format!("{} {correlation:+.2}", tr("Correlation")),
        correlation < 0.0,
    );
    let side = match lean {
        l if l > 0.5 => trf!("{db} dB to the right", db = format!("{l:.1}")),
        l if l < -0.5 => trf!("{db} dB to the left", db = format!("{:.1}", -l)),
        _ => tr("Centered").to_owned(),
    };
    bar(
        1,
        (lean / 12.0 + 1.0) / 2.0,
        tr("L"),
        tr("R"),
        format!("{} {side}", tr("Balance")),
        lean.abs() > 3.0,
    );
    ui.add_space(8.0);
    let heading = |ui: &mut Ui, text: &'static str| {
        ui.label(RichText::new(tr(text)).size(10.5).color(p.weak));
    };
    heading(ui, "Level and stereo");
    ui.horizontal(|ui| {
        for (id, name) in [("mono", "Mono"), ("swap", "Swap channels")] {
            params.toggle(ui, id, |ui, on| ui.toggle_value(on, tr(name)));
        }
    });
    ui.horizontal(|ui| knobs(ui, params, &["gain", "balance", "width"], 50.0, p.accent));
    ui.add_space(6.0);
    heading(ui, "Phase");
    ui.horizontal(|ui| {
        for (id, name) in [
            ("invert_left", "Invert left"),
            ("invert_right", "Invert right"),
        ] {
            params.toggle(ui, id, |ui, on| ui.toggle_value(on, tr(name)));
        }
    });
    ui.horizontal(|ui| {
        knobs(
            ui,
            params,
            &["rotation", "rotation_freq", "delay_left", "delay_right"],
            50.0,
            p.accent,
        )
    });
    ui.label(
        RichText::new(tr(
            "Delay a side to line up two microphones; turn the phase to match the lows of two \
             sources (DI and amp, two kick mics) where polarity alone is too much.",
        ))
        .size(10.5)
        .color(p.weak),
    );
}

fn imager(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    let p = theme::current(ui);
    let correlation = meters.first().copied().unwrap_or(1.0);
    let (rect, painter) = display(ui, 210.0);
    // Goniometer: mid up, side across, rotated 45 degrees as on the classic scopes.
    let size = rect.height() - 24.0;
    let scope = Rect::from_center_size(
        pos2(rect.left() + size / 2.0 + 16.0, rect.center().y),
        vec2(size, size),
    );
    let c = scope.center();
    let grid = p.weak.gamma_multiply(0.2);
    painter.circle_stroke(c, size / 2.0, Stroke::new(1.0, grid));
    painter.line_segment(
        [pos2(c.x, scope.top()), pos2(c.x, scope.bottom())],
        Stroke::new(1.0, grid),
    );
    painter.line_segment(
        [pos2(scope.left(), c.y), pos2(scope.right(), c.y)],
        Stroke::new(1.0, grid),
    );
    painter.text(
        scope.left_top(),
        Align2::LEFT_TOP,
        "L",
        FontId::proportional(10.0),
        p.weak,
    );
    painter.text(
        scope.right_top(),
        Align2::RIGHT_TOP,
        "R",
        FontId::proportional(10.0),
        p.weak,
    );
    let points: Vec<(f32, f32)> = meters
        .get(2..)
        .unwrap_or(&[])
        .chunks(2)
        .map(|v| (v[0], v.get(1).copied().unwrap_or(0.0)))
        .collect();
    let peak = points
        .iter()
        .fold(0.05f32, |m, (a, b)| m.max(a.abs()).max(b.abs()));
    let scale = size / 2.0 * 0.9 / peak;
    for (i, (m, s)) in points.iter().enumerate() {
        let alpha = (i as f32 / points.len().max(1) as f32).max(0.15);
        painter.circle_filled(
            pos2(c.x + s * scale, c.y - m * scale),
            2.0,
            p.accent.gamma_multiply(alpha),
        );
    }
    // Correlation bar.
    let bar = Rect::from_min_size(
        pos2(scope.right() + 30.0, rect.bottom() - 34.0),
        vec2(rect.right() - scope.right() - 50.0, 10.0),
    );
    painter.rect_filled(bar, 4.0, p.panel);
    let x = bar.left() + (correlation + 1.0) / 2.0 * bar.width();
    painter.circle_filled(
        pos2(x, bar.center().y),
        6.0,
        if correlation < 0.0 {
            theme::RECORD
        } else {
            theme::PLAY
        },
    );
    painter.text(
        bar.left_top() - vec2(0.0, 4.0),
        Align2::LEFT_BOTTOM,
        "-1",
        FontId::proportional(9.0),
        p.weak,
    );
    painter.text(
        bar.right_top() - vec2(0.0, 4.0),
        Align2::RIGHT_BOTTOM,
        "+1",
        FontId::proportional(9.0),
        p.weak,
    );
    painter.text(
        pos2(bar.center().x, bar.top() - 4.0),
        Align2::CENTER_BOTTOM,
        format!("{} {correlation:+.2}", tr("Correlation")),
        FontId::proportional(11.0),
        p.text,
    );
    // Band widths as bars.
    let bands = [
        (tr("LOW"), params.get("low_width")),
        (tr("MID"), params.get("mid_width")),
        (tr("HIGH"), params.get("high_width")),
    ];
    let area = Rect::from_min_max(
        pos2(scope.right() + 30.0, rect.top() + 16.0),
        pos2(rect.right() - 20.0, bar.top() - 30.0),
    );
    let w = area.width() / 3.0;
    for (i, (name, width)) in bands.iter().enumerate() {
        let x = area.left() + w * (i as f32 + 0.5);
        let half = (width / 200.0) * (w * 0.45);
        let r = Rect::from_center_size(
            pos2(x, area.center().y),
            vec2(half * 2.0, area.height() * 0.5),
        );
        painter.rect_filled(r, 4.0, p.accent.gamma_multiply(0.35 + width / 400.0));
        painter.text(
            pos2(x, area.bottom()),
            Align2::CENTER_BOTTOM,
            format!("{name} {width:.0}%"),
            FontId::proportional(10.0),
            p.weak,
        );
    }
    ui.ctx().request_repaint();
    ui.add_space(8.0);
    knob_row(
        ui,
        params,
        &[
            "mid_width",
            "low_width",
            "high_width",
            "low_split",
            "high_split",
            "mono_bass",
            "output",
        ],
        p.accent,
    );
}

/// A bipolar bar from the middle for a gain in dB (-12 to +12).
fn bipolar(painter: &egui::Painter, rect: Rect, db: f32, color: Color32, base: Color32) {
    painter.rect_filled(rect, 4.0, base);
    let mid = rect.center().x;
    let x = mid + (db / 12.0).clamp(-1.0, 1.0) * rect.width() / 2.0;
    painter.rect_filled(
        Rect::from_x_y_ranges(mid.min(x)..=mid.max(x), rect.y_range()),
        4.0,
        color,
    );
    painter.line_segment(
        [pos2(mid, rect.top() - 2.0), pos2(mid, rect.bottom() + 2.0)],
        Stroke::new(1.0, base.gamma_multiply(3.0)),
    );
}

fn transient(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    let p = theme::current(ui);
    let at = |i: usize, floor: f32| meters.get(i).copied().unwrap_or(floor);
    state
        .dynamics
        .push_back([at(2, 0.0), at(3, -120.0), at(4, -120.0)]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    let height = 220.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let width = (ui.available_width() - 190.0).max(300.0);
        transient_view(ui, state, vec2(width, height));
        // What the shaper does now, attack and sustain apart.
        let (rect, _) = ui.allocate_exact_size(vec2(182.0, height), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, p.base);
        for (i, (name, db, color)) in [
            (tr("Attack"), at(0, 0.0), p.accent),
            (tr("Sustain"), at(1, 0.0), theme::SOLO),
        ]
        .into_iter()
        .enumerate()
        {
            let y = rect.top() + 46.0 + i as f32 * 74.0;
            painter.text(
                pos2(rect.left() + 12.0, y - 24.0),
                Align2::LEFT_TOP,
                name,
                FontId::proportional(11.0),
                p.weak,
            );
            painter.text(
                pos2(rect.right() - 12.0, y - 26.0),
                Align2::RIGHT_TOP,
                format!("{:+.1} dB", db + 0.0),
                theme::strong(14.0),
                color,
            );
            bipolar(
                &painter,
                Rect::from_min_size(pos2(rect.left() + 12.0, y), vec2(158.0, 12.0)),
                db,
                color,
                p.panel,
            );
        }
        painter.text(
            pos2(rect.center().x, rect.bottom() - 12.0),
            Align2::CENTER_BOTTOM,
            format!("{} {:.1} dBFS", tr("Out"), at(4, -120.0).max(-99.0)),
            FontId::monospace(10.5),
            p.weak,
        );
    });
    ui.ctx().request_repaint();
    ui.add_space(8.0);
    knob_sections(
        ui,
        params,
        &[
            (
                "Shape",
                &["attack", "sustain", "speed", "sensitivity"],
                None,
            ),
            ("Detection", &["sc_hpf"], None),
            ("Output", &["mix", "output", "clip"], None),
        ],
        &|_| true,
    );
}

/// The sound over time, mirrored like a waveform: the input in grey, the output over it, and
/// the gain the shaper applies as a line around 0 dB.
fn transient_view(ui: &mut Ui, state: &EditorState, size: egui::Vec2) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 10.0;
    // Levels from -48 dBFS (the middle) to 0 (the edges).
    let amp = |db: f32| ((db + 48.0) / 48.0).clamp(0.0, 1.0) * half;
    let step = rect.width() / HISTORY as f32;
    let x0 = rect.right() - state.dynamics.len() as f32 * step;
    let points = |k: usize| -> Vec<(f32, f32)> {
        state
            .dynamics
            .iter()
            .enumerate()
            .map(|(i, v)| (x0 + i as f32 * step, amp(v[k])))
            .collect()
    };
    painter.add(mirrored(&points(1), mid, p.weak.gamma_multiply(0.3)));
    painter.add(mirrored(&points(2), mid, p.accent.gamma_multiply(0.45)));
    // Gain: +12 dB at the top, -12 at the bottom.
    let gain_y = |db: f32| mid - (db / 12.0).clamp(-1.0, 1.0) * half;
    painter.line_segment(
        [pos2(rect.left(), mid), pos2(rect.right(), mid)],
        Stroke::new(1.0, p.weak.gamma_multiply(0.25)),
    );
    let gain: Vec<Pos2> = state
        .dynamics
        .iter()
        .enumerate()
        .map(|(i, v)| pos2(x0 + i as f32 * step, gain_y(v[0])))
        .collect();
    painter.add(Shape::line(gain, Stroke::new(1.8, theme::SOLO)));
    let legend = [
        (tr("Input"), p.weak),
        (tr("Output"), p.accent),
        (tr("Gain"), theme::SOLO),
    ];
    let mut x = rect.left() + 10.0;
    for (name, color) in legend {
        painter.rect_filled(
            Rect::from_min_size(pos2(x, rect.bottom() - 14.0), vec2(8.0, 3.0)),
            1.0,
            color,
        );
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(9.5), p.weak);
        let w = galley.size().x;
        painter.galley(pos2(x + 12.0, rect.bottom() - 19.0), galley, p.weak);
        x += w + 26.0;
    }
    for (db, label) in [(12.0, "+12"), (-12.0, "-12")] {
        painter.text(
            pos2(
                rect.right() - 6.0,
                gain_y(db) + if db > 0.0 { 2.0 } else { -2.0 },
            ),
            if db > 0.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::RIGHT_BOTTOM
            },
            label,
            FontId::proportional(8.5),
            theme::SOLO.gamma_multiply(0.7),
        );
    }
}

fn deesser(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    use audiostrada_dsp::deesser::{MODE_HINTS, MODES};
    let reduction = meters.first().copied().unwrap_or(0.0);
    let level = meters.get(1).copied().unwrap_or(-120.0);
    state
        .dynamics
        .push_back([reduction, level, level + reduction]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    cards(ui, params, "mode", &MODES, &MODE_HINTS);
    ui.add_space(8.0);
    let threshold = params.get("threshold");
    let height = 210.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let side = 300.0;
        let width = (ui.available_width() - side - 52.0).max(260.0);
        dynamics_history(
            ui,
            params,
            state,
            vec2(width, height),
            (threshold, None),
            (reduction, 24.0),
        );
        detection_curve(ui, params, vec2(side, height));
        reduction_meter(ui, reduction, height);
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let weak = theme::current(ui).weak;
        ui.label(RichText::new(tr("Detector")).size(11.0).color(weak));
        segmented(
            ui,
            params,
            "detector",
            &[
                "Everything above the frequency: catches every S",
                "Only around the frequency: leaves the air above alone",
            ],
        );
        ui.add_space(16.0);
        ui.label(RichText::new(tr("Listen")).size(11.0).color(weak));
        segmented(
            ui,
            params,
            "listen",
            &[
                "The de-essed sound",
                "What the detector hears",
                "Only what is removed",
            ],
        );
    });
    ui.add_space(6.0);
    knob_sections(
        ui,
        params,
        &[
            (
                "Detection",
                &["frequency", "threshold", "range"],
                Some((
                    "relative",
                    "Relative",
                    "Compares the sibilance with the whole voice, so the threshold holds \
                     whether the singer is loud or soft",
                )),
            ),
            ("Time", &["attack", "release", "lookahead"], None),
        ],
        &|_| true,
    );
}

/// What the detector hears, from 1 to 20 kHz, with the frequency to drag sideways; in split
/// mode, the highs that are turned down are shaded.
fn detection_curve(ui: &mut Ui, params: &mut Params, size: egui::Vec2) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    let inner = rect.shrink2(vec2(10.0, 14.0));
    let (low, high) = (1000.0f32, 20_000.0f32);
    let to_x = |f: f32| inner.left() + (f / low).ln() / (high / low).ln() * inner.width();
    let of_x =
        |x: f32| low * (high / low).powf(((x - inner.left()) / inner.width()).clamp(0.0, 1.0));
    let to_y = |db: f32| inner.top() + ((6.0 - db) / 36.0).clamp(0.0, 1.0) * inner.height();
    let grid = p.weak.gamma_multiply(0.14);
    for f in [2000.0, 5000.0, 10_000.0] {
        painter.line_segment(
            [pos2(to_x(f), inner.top()), pos2(to_x(f), inner.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(to_x(f) + 3.0, rect.bottom() - 3.0),
            Align2::LEFT_BOTTOM,
            format!("{}k", f / 1000.0),
            FontId::proportional(9.0),
            p.weak,
        );
    }
    let rate = f64::from(params.doc.player().engine().sample_rate().max(1));
    let frequency = params.get("frequency");
    if params.get("mode") < 0.5 {
        let x = to_x(frequency * audiostrada_dsp::deesser::SPLIT as f32);
        painter.rect_filled(
            Rect::from_min_max(pos2(x, rect.top()), rect.max),
            0.0,
            theme::RECORD.gamma_multiply(0.08),
        );
    }
    let filter = audiostrada_dsp::deesser::detector(&params.values, rate);
    let points: Vec<Pos2> = (0..=160)
        .map(|i| {
            let f = low * (high / low).powf(i as f32 / 160.0);
            pos2(
                to_x(f),
                to_y(filter.magnitude_db(f64::from(f), rate) as f32),
            )
        })
        .collect();
    painter.add(fill_under(
        &points,
        inner.bottom(),
        p.accent.gamma_multiply(0.16),
    ));
    painter.add(Shape::line(points, Stroke::new(2.0, p.accent)));
    let hot = response.hovered() || response.dragged();
    let x = to_x(frequency);
    painter.line_segment(
        [pos2(x, rect.top() + 4.0), pos2(x, rect.bottom() - 4.0)],
        Stroke::new(if hot { 2.0 } else { 1.2 }, theme::SOLO),
    );
    painter.text(
        pos2(x + 5.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
        format_hz(frequency),
        theme::strong(11.0),
        theme::SOLO,
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let response = response.on_hover_text(tr("Drag sideways to move the frequency"));
    let changed = response
        .dragged()
        .then(|| of_x(x + response.drag_delta().x));
    params.set("frequency", changed, &response);
}

/// A frequency response from 20 Hz to 20 kHz on a dB range, with labeled grid lines.
fn response_plot(
    ui: &mut Ui,
    height: f32,
    range: f32,
    curve: impl Fn(f32) -> f32,
    color: Color32,
    marker: Option<f32>,
) {
    let p = theme::current(ui);
    let (rect, painter) = display(ui, height);
    let inner = rect.shrink2(vec2(10.0, 12.0));
    let to_x = |f: f32| inner.left() + (f / 20.0).log10() / 3.0 * inner.width();
    let to_y = |db: f32| inner.center().y - (db / range).clamp(-1.0, 1.0) * inner.height() / 2.0;
    let grid = p.weak.gamma_multiply(0.15);
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        painter.line_segment(
            [pos2(to_x(f), inner.top()), pos2(to_x(f), inner.bottom())],
            Stroke::new(1.0, grid),
        );
        let label = if f >= 1000.0 {
            format!("{}k", f / 1000.0)
        } else {
            format!("{f}")
        };
        painter.text(
            pos2(to_x(f), inner.bottom()),
            Align2::CENTER_BOTTOM,
            label,
            FontId::proportional(9.0),
            p.weak,
        );
    }
    for db in [-range / 2.0, 0.0, range / 2.0] {
        painter.line_segment(
            [pos2(inner.left(), to_y(db)), pos2(inner.right(), to_y(db))],
            Stroke::new(1.0, grid),
        );
    }
    let points: Vec<Pos2> = (0..=240)
        .map(|i| {
            let f = 20.0 * 1000f32.powf(i as f32 / 240.0);
            pos2(to_x(f), to_y(curve(f)))
        })
        .collect();
    painter.add(fill_under(&points, to_y(0.0), color.gamma_multiply(0.18)));
    painter.add(Shape::line(points, Stroke::new(2.4, color)));
    if let Some(f) = marker {
        painter.circle_filled(pos2(to_x(f), to_y(curve(f))), 5.0, theme::SOLO);
    }
}

fn vintage_eq(ui: &mut Ui, params: &mut Params) {
    use audiostrada_dsp::vintage_eq::{CUT_FREQS, HIGH_FREQS, LOW_FREQS};
    let values = params.values.clone();
    let rate = params.doc.player().engine().sample_rate().max(1) as f32;
    let amber = Color32::from_rgb(230, 170, 90);
    response_plot(
        ui,
        150.0,
        18.0,
        |f| audiostrada_dsp::vintage_eq::response_db(&values, rate, f),
        amber,
        None,
    );
    ui.add_space(8.0);
    // The faceplate: knobs above, frequency selectors below, by section.
    let plate = Plate::of(&theme::current(ui));
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 268.0), Sense::hover());
    theme::gradient_rect(ui.painter(), rect, plate.top, plate.bottom);
    ui.painter().rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, plate.ink.gamma_multiply(0.25)),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(14.0, 10.0));
    let unit = inner.width() / 6.0;
    let strip = |from: f32, cells: f32| {
        Rect::from_min_size(
            pos2(inner.left() + from * unit, inner.top()),
            vec2(cells * unit, inner.height()),
        )
    };
    let short = |names: &[&'static str]| -> Vec<String> {
        names
            .iter()
            .map(|n| {
                n.trim_end_matches(" Hz")
                    .trim_end_matches(" kHz")
                    .to_owned()
            })
            .collect()
    };
    let sections = [
        (strip(0.0, 2.0), tr("Low frequency"), "CPS"),
        (strip(2.0, 2.0), tr("High frequency"), "KCS"),
        (strip(4.0, 1.0), tr("High atten"), "KCS"),
        (strip(5.0, 1.0), tr("Tube"), ""),
    ];
    for (i, (column, title, _)) in sections.iter().enumerate() {
        ui.painter().text(
            pos2(column.center().x, column.top() + 8.0),
            Align2::CENTER_CENTER,
            title.to_uppercase(),
            theme::strong(10.5),
            plate.ink,
        );
        if i > 0 {
            // Engraved: a dark line and its light edge.
            let x = column.left();
            let y = column.top() + 2.0..=column.bottom() - 2.0;
            ui.painter().vline(
                x,
                y.clone(),
                Stroke::new(1.0, Color32::from_black_alpha(70)),
            );
            ui.painter()
                .vline(x + 1.0, y, Stroke::new(1.0, Color32::from_white_alpha(28)));
        }
    }
    let row = |column: Rect, cell: usize, cells: usize, second: bool| {
        let w = column.width() / cells as f32;
        let top = column.top() + 20.0 + if second { 116.0 } else { 0.0 };
        Rect::from_min_size(pos2(column.left() + w * cell as f32, top), vec2(w, 112.0))
    };
    let (low, high, cut, tube) = (sections[0].0, sections[1].0, sections[2].0, sections[3].0);
    plate_knob(
        ui,
        params,
        "low_boost",
        tr("Boost"),
        row(low, 0, 2, false),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "low_cut",
        tr("Atten"),
        row(low, 1, 2, false),
        &plate,
    );
    plate_switch(
        ui,
        params,
        "low_freq",
        "CPS",
        &short(&LOW_FREQS),
        row(low, 0, 1, true),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "high_boost",
        tr("Boost"),
        row(high, 0, 2, false),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "bandwidth",
        tr("Bandwidth"),
        row(high, 1, 2, false),
        &plate,
    );
    plate_switch(
        ui,
        params,
        "high_freq",
        "KCS",
        &short(&HIGH_FREQS),
        row(high, 0, 1, true),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "high_cut",
        tr("Atten"),
        row(cut, 0, 1, false),
        &plate,
    );
    plate_switch(
        ui,
        params,
        "cut_freq",
        "KCS",
        &short(&CUT_FREQS),
        row(cut, 0, 1, true),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "warmth",
        tr("Warmth"),
        row(tube, 0, 1, false),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "output",
        tr("Output"),
        row(tube, 0, 1, true),
        &plate,
    );
    // The pilot lamp glows with the warmth.
    let glow = params.get("warmth") / 100.0;
    let lamp = pos2(tube.right() - 12.0, tube.top() + 8.0);
    let lit = Color32::from_rgb(255, 176, 64);
    for (radius, alpha) in [(14.0, 0.10), (10.0, 0.18)] {
        ui.painter()
            .circle_filled(lamp, radius, lit.gamma_multiply(alpha * glow));
    }
    ui.painter().circle_filled(
        lamp,
        6.0,
        Color32::from_rgb(90, 40, 20).lerp_to_gamma(lit, 0.25 + 0.75 * glow),
    );
    ui.painter()
        .circle_stroke(lamp, 6.0, Stroke::new(1.5, Color32::from_black_alpha(120)));
    ui.painter()
        .circle_filled(lamp + vec2(-2.0, -2.0), 1.6, Color32::from_white_alpha(140));
}

/// Colors of the tube EQ's faceplate, with the theme: slate with cream engraving in the
/// dark, pale steel with dark engraving in the light.
struct Plate {
    top: Color32,
    bottom: Color32,
    ink: Color32,
    knob: Color32,
    head: Color32,
    edge: Color32,
}

impl Plate {
    fn of(p: &theme::Palette) -> Self {
        if p.dark {
            Self {
                top: Color32::from_rgb(54, 64, 76),
                bottom: Color32::from_rgb(38, 45, 54),
                ink: Color32::from_rgb(234, 226, 206),
                knob: Color32::from_rgb(14, 14, 16),
                head: Color32::from_rgb(36, 36, 40),
                edge: Color32::from_rgb(78, 78, 84),
            }
        } else {
            Self {
                top: Color32::from_rgb(202, 212, 221),
                bottom: Color32::from_rgb(176, 188, 199),
                ink: Color32::from_rgb(26, 34, 46),
                knob: Color32::from_rgb(18, 18, 20),
                head: Color32::from_rgb(40, 40, 44),
                edge: Color32::from_rgb(96, 96, 102),
            }
        }
    }
}

/// Where a hardware control points: 0 straight up, clockwise, in radians.
fn plate_point(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    center + radius * vec2(angle.sin(), -angle.cos())
}

/// A pointer knob as on the old hardware: a round skirt, a pointed head with its line.
fn paint_pointer_knob(painter: &egui::Painter, center: Pos2, r: f32, angle: f32, plate: &Plate) {
    painter.circle_filled(
        center + vec2(0.0, 2.5),
        r * 1.04,
        Color32::from_black_alpha(80),
    );
    painter.circle_filled(center, r, plate.knob);
    painter.circle_stroke(center, r, Stroke::new(1.0, plate.edge));
    let dir = vec2(angle.sin(), -angle.cos());
    let side = vec2(-dir.y, dir.x);
    let head = vec![
        center + dir * r * 1.3,
        center + side * r * 0.5,
        center - dir * r * 0.75,
        center - side * r * 0.5,
    ];
    painter.add(Shape::convex_polygon(
        head,
        plate.head,
        Stroke::new(1.0, plate.edge),
    ));
    painter.circle_filled(
        center - vec2(r * 0.25, r * 0.3),
        r * 0.32,
        Color32::from_white_alpha(14),
    );
    painter.line_segment(
        [center + dir * r * 0.2, center + dir * r * 1.15],
        Stroke::new(2.0, Color32::from_rgb(240, 234, 220)),
    );
}

/// A knob of the faceplate in `cell`: its name, a scale from 0 to 10 (or -12 to +12), the
/// value under it.
fn plate_knob(ui: &mut Ui, params: &mut Params, id: &str, label: &str, cell: Rect, plate: &Plate) {
    let Some(p) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let r = 21.0;
    let center = pos2(cell.center().x, cell.top() + 58.0);
    let response = ui
        .interact(
            Rect::from_center_size(center, vec2(2.0 * r + 30.0, 2.0 * r + 30.0)),
            ui.scope_id().with(("plate", params.insert.0, id)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let mut t = to_unit(p, value);
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        t -= response.drag_delta().y / 150.0 * if fine { 0.15 } else { 1.0 };
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            t += wheel.signum() * if fine { 0.002 } else { 0.02 };
        }
    }
    if response.double_clicked() {
        t = to_unit(p, p.default);
    }
    let t = t.clamp(0.0, 1.0);
    let changed = ((t - to_unit(p, value)).abs() > 1e-6).then(|| from_unit(p, t));
    let shown = changed.unwrap_or(value);
    let painter = ui.painter();
    painter.text(
        pos2(cell.center().x, cell.top() + 12.0),
        Align2::CENTER_CENTER,
        label.to_uppercase(),
        FontId::proportional(10.0),
        plate.ink.gamma_multiply(0.85),
    );
    let sweep = 0.75 * std::f32::consts::PI;
    let bipolar = p.min < 0.0;
    let marks: Vec<(f32, String)> = if bipolar {
        [-12.0f32, -6.0, 0.0, 6.0, 12.0]
            .iter()
            .map(|v| (to_unit(p, *v), format!("{v:+}").replace("+0", "0")))
            .collect()
    } else {
        (0..=10)
            .map(|k| {
                (
                    k as f32 / 10.0,
                    if k % 2 == 0 {
                        (k as f32 * p.max / 10.0).to_string()
                    } else {
                        String::new()
                    },
                )
            })
            .collect()
    };
    for (u, text) in &marks {
        let a = -sweep + 2.0 * sweep * u;
        painter.line_segment(
            [
                plate_point(center, r + 4.0, a),
                plate_point(center, r + 8.0, a),
            ],
            Stroke::new(1.2, plate.ink.gamma_multiply(0.8)),
        );
        if !text.is_empty() {
            painter.text(
                plate_point(center, r + 16.0, a),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(9.0),
                plate.ink.gamma_multiply(0.75),
            );
        }
    }
    paint_pointer_knob(painter, center, r, -sweep + 2.0 * sweep * t, plate);
    painter.text(
        pos2(cell.center().x, cell.bottom() - 8.0),
        Align2::CENTER_CENTER,
        format_value(p, shown),
        FontId::monospace(10.0),
        plate.ink.gamma_multiply(0.7),
    );
    let response = response.on_hover_text(crate::trf!(
        "{name}: {value}. Drag, Shift for fine, double-click to reset",
        name = tr(p.name),
        value = format_value(p, shown)
    ));
    params.set(id, changed, &response);
}

/// A rotary selector of the faceplate in `cell`, its positions written around it: click a
/// position, or the knob for the next one; drag or wheel to step.
fn plate_switch(
    ui: &mut Ui,
    params: &mut Params,
    id: &str,
    unit: &str,
    labels: &[String],
    cell: Rect,
    plate: &Plate,
) {
    let Some(p) = params.param(id) else {
        return;
    };
    let n = labels.len().max(1);
    let current = ((params.get(id) - p.min).round().max(0.0) as usize).min(n - 1);
    let r = 16.0;
    let center = pos2(cell.center().x, cell.top() + 62.0);
    let step = (std::f32::consts::PI * 1.5 / (n - 1).max(1) as f32).min(0.8);
    let angle = |i: usize| (i as f32 - (n - 1) as f32 / 2.0) * step;
    let key = ui.scope_id().with(("plate", params.insert.0, id));
    let response = ui.interact(
        Rect::from_center_size(center, vec2(112.0, 96.0)),
        key,
        Sense::click_and_drag(),
    );
    let mut chosen = None;
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let near = (0..n)
            .map(|i| (i, plate_point(center, r + 22.0, angle(i)).distance(at)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, d)| *d < 13.0);
        chosen = Some(near.map_or((current + 1) % n, |(i, _)| i));
    }
    let wheel = if response.hovered() {
        ui.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };
    let travel: f32 = ui.data(|d| d.get_temp(key).unwrap_or(0.0));
    let travel = if response.dragged() {
        travel - response.drag_delta().y
    } else {
        0.0
    } + wheel;
    if travel.abs() >= 24.0 {
        let next = current as i32 + travel.signum() as i32;
        chosen = Some(next.clamp(0, n as i32 - 1) as usize);
        ui.data_mut(|d| d.insert_temp(key, 0.0f32));
    } else {
        ui.data_mut(|d| d.insert_temp(key, travel));
    }
    let shown = chosen.unwrap_or(current);
    let painter = ui.painter();
    painter.text(
        pos2(cell.center().x, cell.top() + 12.0),
        Align2::CENTER_CENTER,
        unit,
        FontId::proportional(10.0),
        plate.ink.gamma_multiply(0.85),
    );
    for (i, label) in labels.iter().enumerate() {
        let a = angle(i);
        let on = i == shown;
        painter.line_segment(
            [
                plate_point(center, r + 4.0, a),
                plate_point(center, r + 9.0, a),
            ],
            Stroke::new(1.2, plate.ink.gamma_multiply(0.8)),
        );
        painter.text(
            plate_point(center, r + 22.0, a),
            Align2::CENTER_CENTER,
            label,
            if on {
                theme::strong(11.0)
            } else {
                FontId::proportional(10.0)
            },
            if on {
                plate.ink
            } else {
                plate.ink.gamma_multiply(0.6)
            },
        );
    }
    paint_pointer_knob(painter, center, r, angle(shown), plate);
    let response = response.on_hover_text(format!(
        "{}: {}",
        tr(p.name),
        p.choices.get(shown).map_or("", |c| tr(c))
    ));
    let changed = chosen.filter(|&c| c != current).map(|c| p.min + c as f32);
    params.set(id, changed, &response);
    if changed.is_some() {
        params.doc.history_mut().end_gesture();
    }
}

fn filter(ui: &mut Ui, params: &mut Params, meters: &[f32]) {
    let p = theme::current(ui);
    let kind = params.get("type").round() as usize;
    let k = 2.0 - 1.95 * params.get("resonance") / 100.0;
    let cutoff = meters
        .first()
        .copied()
        .filter(|c| *c > 0.0)
        .unwrap_or(params.get("cutoff"));
    let curve = |f: f32| -> f32 {
        let w = f / cutoff;
        let den = ((1.0 - w * w).powi(2) + (k * w).powi(2)).sqrt();
        let num = match kind {
            0 => 1.0,
            1 => w * w,
            2 => k * w,
            _ => (1.0 - w * w).abs(),
        };
        20.0 * (num / den.max(1e-6)).max(1e-6).log10()
    };
    response_plot(ui, 170.0, 36.0, curve, p.accent, Some(cutoff));
    ui.ctx().request_repaint();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Type")).size(10.5).color(p.weak));
        choice_box(ui, params, "type");
        switches(ui, params);
    });
    let lfo = if params.get("sync") >= 0.5 {
        "note"
    } else {
        "lfo_rate"
    };
    knob_row(
        ui,
        params,
        &[
            "cutoff",
            "resonance",
            "drive",
            lfo,
            "lfo_depth",
            "envelope",
            "mix",
            "output",
        ],
        p.accent,
    );
}

fn drive(ui: &mut Ui, params: &mut Params) {
    use audiostrada_dsp::drive::{TYPE_HINTS, TYPES, transfer};
    let p = theme::current(ui);
    let mode = (params.get("mode").round() as usize).min(TYPES.len() - 1);
    let bias = params.get("bias");
    let gain = 10f32.powf(params.get("drive") / 20.0);
    let (rect, painter) = display(ui, 160.0);
    let size = rect.height() - 20.0;
    let plot = Rect::from_center_size(
        pos2(rect.left() + size / 2.0 + 16.0, rect.center().y),
        vec2(size, size),
    );
    let grid = p.weak.gamma_multiply(0.2);
    painter.line_segment(
        [
            pos2(plot.left(), plot.center().y),
            pos2(plot.right(), plot.center().y),
        ],
        Stroke::new(1.0, grid),
    );
    painter.line_segment(
        [
            pos2(plot.center().x, plot.top()),
            pos2(plot.center().x, plot.bottom()),
        ],
        Stroke::new(1.0, grid),
    );
    let line: Vec<Pos2> = (0..=200)
        .map(|i| {
            let x = -1.0 + i as f32 / 100.0;
            let y = transfer(mode as u32, bias, x * gain).clamp(-1.2, 1.2);
            pos2(
                plot.center().x + x * size / 2.0,
                plot.center().y - y / 1.2 * size / 2.0,
            )
        })
        .collect();
    let color = Color32::from_rgb(230, 90, 150);
    painter.add(Shape::line(line, Stroke::new(2.4, color)));
    let info_x = plot.right() + 30.0;
    painter.text(
        pos2(info_x, rect.top() + 24.0),
        Align2::LEFT_TOP,
        tr(TYPES[mode]),
        theme::strong(20.0),
        color,
    );
    painter.text(
        pos2(info_x, rect.top() + 54.0),
        Align2::LEFT_TOP,
        format!("{} {:+.1} dB", tr("Drive"), params.get("drive")),
        FontId::proportional(12.0),
        p.text,
    );
    let hint = painter.layout(
        tr(TYPE_HINTS[mode]).to_owned(),
        FontId::proportional(11.5),
        p.weak,
        (rect.right() - info_x - 16.0).max(80.0),
    );
    painter.galley(pos2(info_x, rect.top() + 76.0), hint, p.weak);
    ui.add_space(8.0);
    mode_buttons(ui, params, "mode", &TYPE_HINTS);
    ui.add_space(6.0);
    knob_row(ui, params, &["drive", "tone", "level", "mix"], color);
    ui.horizontal(|ui| knobs(ui, params, &["low_cut", "bias", "bass", "mid"], 44.0, color));
}

// ---------------------------------------------------------------- Compressor

const HISTORY: usize = 240;

/// The compressor: its character, the clean curve or the vintage VU meter, and its knobs.
fn compressor(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    use audiostrada_dsp::compressor::{TYPE_HINTS, TYPES, uses};
    let kind = (params.get("type").round().max(0.0) as usize).min(TYPES.len() - 1);
    let reduction = meters.first().copied().unwrap_or(0.0);
    let input = meters.get(1).copied().unwrap_or(-120.0);
    let output = meters.get(2).copied().unwrap_or(-120.0);
    state.dynamics.push_back([reduction, input, output]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    // The characters, as cards: their name and what they do.
    cards(ui, params, "type", &TYPES, &TYPE_HINTS);
    ui.add_space(8.0);
    // The display: levels and reduction over time, the curve, the reduction meter.
    let threshold = params.get("threshold");
    let curve = (threshold, params.get("ratio"), params.get("knee"));
    let range = params.get("range");
    let makeup = params.get("makeup")
        + if params.get("auto_gain") >= 0.5 {
            audiostrada_dsp::compressor::auto_makeup(curve.0, curve.1)
        } else {
            0.0
        };
    let height = 230.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let side = if kind == audiostrada_dsp::compressor::CLEAN {
            height
        } else {
            height * 1.45
        };
        let width = (ui.available_width() - side - 52.0).max(260.0);
        dynamics_history(
            ui,
            params,
            state,
            vec2(width, height),
            (threshold, None),
            (reduction, 24.0),
        );
        // Modern: the curve; vintage: a needle, as on the hardware.
        if kind == audiostrada_dsp::compressor::CLEAN {
            let out_of = |level: f32| {
                level
                    + audiostrada_dsp::compressor::reduction_at(kind, level, curve, range)
                    + makeup
            };
            transfer_curve(ui, params, &out_of, threshold, input, height);
        } else {
            let amber = kind == 3;
            vu_meter(
                ui,
                reduction,
                &mut state.needle,
                vec2(height * 1.45, height),
                amber,
            );
        }
        reduction_meter(ui, reduction, height);
    });
    ui.add_space(8.0);
    // The controls, by what they act on, in sections of one height.
    knob_sections(
        ui,
        params,
        &[
            ("Dynamics", &["threshold", "ratio", "knee", "range"], None),
            (
                "Time",
                &["attack", "release", "hold", "lookahead"],
                Some((
                    "auto",
                    "Auto release",
                    "Fast recovery after peaks, slow after sustained compression",
                )),
            ),
            (
                "Sidechain",
                &["sc_hpf", "link"],
                Some((
                    "listen",
                    "Listen",
                    "Hear what the detector hears: the key through its high-pass",
                )),
            ),
            (
                "Output",
                &["makeup", "mix", "color"],
                Some((
                    "auto_gain",
                    "Auto gain",
                    "Makeup from the threshold and ratio, so the level stays about the same",
                )),
            ),
        ],
        &|id| uses(kind, id),
    );
}

/// The values of a choice as cards side by side: their name and two lines of what they do.
fn cards(
    ui: &mut Ui,
    params: &mut Params,
    id: &str,
    names: &[&'static str],
    hints: &[&'static str],
) {
    #[cfg(test)]
    tests::SHOWN.with_borrow_mut(|shown| shown.push(id.to_owned()));
    let p = theme::current(ui);
    let kind = (params.get(id).round().max(0.0) as usize).min(names.len() - 1);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let n = names.len() as f32;
        let width = ((ui.available_width() - 6.0 * (n - 1.0)) / n).max(120.0);
        for (k, name) in names.iter().enumerate() {
            let on = k == kind;
            let (rect, response) = ui.allocate_exact_size(vec2(width, 58.0), Sense::click());
            let fill = if on {
                p.accent.gamma_multiply(0.22)
            } else if response.hovered() {
                p.hover
            } else {
                p.base
            };
            let painter = ui.painter();
            painter.rect_filled(rect, 8.0, fill);
            painter.rect_stroke(
                rect,
                8.0,
                Stroke::new(
                    if on { 1.5 } else { 1.0 },
                    if on { p.accent } else { p.border },
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.left_top() + vec2(12.0, 7.0),
                Align2::LEFT_TOP,
                tr(name),
                theme::strong(13.5),
                if on {
                    p.text
                } else {
                    p.text.gamma_multiply(0.8)
                },
            );
            // Two lines at most of what it does.
            let mut job = egui::text::LayoutJob::simple(
                tr(hints[k]).to_owned(),
                FontId::proportional(9.5),
                p.weak,
                rect.width() - 24.0,
            );
            job.wrap.max_rows = 2;
            job.wrap.overflow_character = Some('…');
            let hint = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter()
                .galley(rect.left_top() + vec2(12.0, 26.0), hint, p.weak);
            let response = response.on_hover_text(tr(hints[k]));
            if response.clicked() && !on {
                params.set(id, Some(k as f32), &response);
            }
        }
    });
}

/// Switch under a section's knobs: its parameter, name and what it does.
type SectionSwitch<'a> = Option<(&'a str, &'static str, &'static str)>;

/// Titled panels of knobs side by side, all of one height, each with its switch under the
/// knobs; the controls `enabled` refuses are greyed.
fn knob_sections(
    ui: &mut Ui,
    params: &mut Params,
    sections: &[(&'static str, &[&str], SectionSwitch)],
    enabled: &dyn Fn(&str) -> bool,
) {
    let p = theme::current(ui);
    let accent = p.accent;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for &(title, ids, switch) in sections {
            egui::Frame::new()
                .fill(p.base)
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_min_height(118.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(tr(title)).strong().size(11.5).color(p.weak));
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for &id in ids {
                                let tint = if id == "color" { theme::SOLO } else { accent };
                                let name = match id {
                                    "sc_hpf" => tr("High pass"),
                                    "sc_lpf" => tr("Low pass"),
                                    _ => params.param(id).map_or("", |q| tr(q.name)),
                                };
                                ui.add_enabled_ui(enabled(id), |ui| {
                                    wide_knob(ui, params, id, name, 44.0, tint)
                                });
                            }
                        });
                        if let Some((id, label, hint)) = switch {
                            ui.add_space(4.0);
                            ui.add_enabled_ui(enabled(id), |ui| {
                                params.toggle(ui, id, |ui, on| {
                                    let response =
                                        pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
                                    if response.clicked() {
                                        *on = !*on;
                                    }
                                    response
                                });
                            });
                        }
                    });
                });
        }
    });
}

/// A switch as a pill: its light and its name, lit in the accent while on.
fn pill_switch(ui: &mut Ui, on: bool, label: &str) -> Response {
    let p = theme::current(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(11.5), p.text);
    let size = vec2(galley.size().x + 34.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    let fill = match (on, response.hovered()) {
        (true, _) => p.accent.gamma_multiply(0.3),
        (false, true) => p.hover,
        (false, false) => p.surface,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 11.0, fill);
    painter.rect_stroke(
        rect,
        11.0,
        Stroke::new(1.0, if on { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    let light = pos2(rect.left() + 13.0, rect.center().y);
    painter.circle_filled(light, 4.0, theme::power_ink(on, &p));
    painter.galley(
        pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        if on {
            p.text
        } else {
            p.text.gamma_multiply(0.8)
        },
    );
    if response.clicked() {
        response.mark_changed();
    }
    response
}

/// Input (filled), output (line) and gain reduction (from the top) over time, with the
/// threshold as a line to drag.
fn dynamics_history(
    ui: &mut Ui,
    params: &mut Params,
    state: &EditorState,
    size: egui::Vec2,
    (threshold, close): (f32, Option<f32>),
    (reduction, floor): (f32, f32),
) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    // Levels from -60 to 0 dBFS; the reduction from 0 down to -24 dB.
    let level_y = |db: f32| rect.bottom() - ((db + 60.0) / 60.0).clamp(0.0, 1.0) * rect.height();
    let gr_y = |db: f32| rect.top() + (-db / floor).clamp(0.0, 1.0) * rect.height();
    let grid = p.weak.gamma_multiply(0.14);
    for db in [-6.0f32, -12.0, -18.0, -24.0, -36.0, -48.0] {
        let y = level_y(db);
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(rect.right() - 6.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            format!("{db:.0}"),
            FontId::proportional(8.5),
            p.weak.gamma_multiply(0.7),
        );
    }
    let step = rect.width() / HISTORY as f32;
    let x0 = rect.right() - state.dynamics.len() as f32 * step;
    let line = |pick: &dyn Fn(&[f32; 3]) -> f32| -> Vec<Pos2> {
        state
            .dynamics
            .iter()
            .enumerate()
            .map(|(i, v)| pos2(x0 + i as f32 * step, pick(v)))
            .collect()
    };
    let input = line(&|v| level_y(v[1]));
    painter.add(fill_under(
        &input,
        rect.bottom(),
        p.weak.gamma_multiply(0.22),
    ));
    painter.add(Shape::line(
        line(&|v| level_y(v[2])),
        Stroke::new(1.4, p.text.gamma_multiply(0.75)),
    ));
    let gr = line(&|v| gr_y(v[0]));
    // The reduction hangs from the top, filled.
    if gr.len() > 1 {
        let mut mesh = egui::Mesh::default();
        let color = theme::RECORD.gamma_multiply(0.22);
        for (k, point) in gr.iter().enumerate() {
            mesh.colored_vertex(pos2(point.x, rect.top()), color);
            mesh.colored_vertex(*point, color);
            if k > 0 {
                let i = (k * 2) as u32;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i, i + 1);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    painter.add(Shape::line(gr, Stroke::new(2.0, theme::RECORD)));
    // The threshold, to drag up and down; darker on a light theme, to read.
    let ty = level_y(threshold);
    let hot = response.hovered() || response.dragged();
    let solo = if p.dark {
        theme::SOLO
    } else {
        Color32::from_rgb(170, 112, 0)
    };
    painter.add(Shape::dashed_line(
        &[pos2(rect.left(), ty), pos2(rect.right(), ty)],
        Stroke::new(if hot { 2.0 } else { 1.2 }, solo),
        6.0,
        4.0,
    ));
    painter.text(
        pos2(rect.left() + 10.0, ty + 3.0),
        Align2::LEFT_TOP,
        format!("{} {threshold:.1} dB", tr("Threshold")),
        FontId::proportional(10.0),
        solo,
    );
    // Where it closes again, below by the hysteresis.
    if let Some(close) = close.filter(|c| *c < threshold - 0.05) {
        let cy = level_y(close);
        painter.add(Shape::dashed_line(
            &[pos2(rect.left(), cy), pos2(rect.right(), cy)],
            Stroke::new(1.0, solo.gamma_multiply(0.5)),
            2.0,
            4.0,
        ));
        painter.text(
            pos2(rect.right() - 40.0, cy + 3.0),
            Align2::RIGHT_TOP,
            format!("{} {close:.1} dB", tr("Closes at")),
            FontId::proportional(9.5),
            solo.gamma_multiply(0.75),
        );
    }
    painter.text(
        rect.left_top() + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        format!(
            "{} {:.1} dB",
            tr("GR"),
            if reduction > -0.05 { 0.0 } else { reduction }
        ),
        theme::strong(15.0),
        theme::RECORD,
    );
    // Legend.
    let legend = [
        (tr("Input"), p.weak),
        (tr("Output"), p.text),
        (tr("Reduction"), theme::RECORD),
    ];
    let mut x = rect.left() + 10.0;
    for (name, color) in legend {
        painter.rect_filled(
            Rect::from_min_size(pos2(x, rect.bottom() - 14.0), vec2(8.0, 3.0)),
            1.0,
            color,
        );
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(9.5), p.weak);
        let w = galley.size().x;
        painter.galley(pos2(x + 12.0, rect.bottom() - 19.0), galley, p.weak);
        x += w + 26.0;
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let response = response.on_hover_text(tr("Drag up and down to move the threshold"));
    let changed = response
        .dragged()
        .then(|| threshold - response.drag_delta().y / rect.height() * 60.0);
    params.set("threshold", changed, &response);
}

/// The static curve of the character, with where the input is now.
fn transfer_curve(
    ui: &mut Ui,
    params: &mut Params,
    out_of: &dyn Fn(f32) -> f32,
    threshold: f32,
    input: f32,
    size: f32,
) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    let to_x = |db: f32| rect.left() + (db + 60.0) / 60.0 * rect.width();
    let to_y = |db: f32| rect.bottom() - (db + 60.0) / 60.0 * rect.height();
    let grid = p.weak.gamma_multiply(0.14);
    for db in [-48.0, -36.0, -24.0, -12.0] {
        painter.line_segment(
            [pos2(to_x(db), rect.top()), pos2(to_x(db), rect.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.line_segment(
            [pos2(rect.left(), to_y(db)), pos2(rect.right(), to_y(db))],
            Stroke::new(1.0, grid),
        );
    }
    painter.line_segment(
        [pos2(to_x(-60.0), to_y(-60.0)), pos2(to_x(0.0), to_y(0.0))],
        Stroke::new(1.0, grid.gamma_multiply(2.0)),
    );
    let at = |level: f32| pos2(to_x(level), to_y(out_of(level).clamp(-60.0, 0.0)));
    let points: Vec<Pos2> = (0..=120).map(|i| at(-60.0 + i as f32 * 0.5)).collect();
    painter.add(Shape::line(points, Stroke::new(2.2, p.accent)));
    let tx = to_x(threshold);
    painter.line_segment(
        [pos2(tx, rect.top()), pos2(tx, rect.bottom())],
        Stroke::new(1.0, theme::SOLO.gamma_multiply(0.6)),
    );
    if input > -60.0 {
        let dot = at(input.min(0.0));
        painter.circle_filled(dot, 7.0, theme::SOLO.gamma_multiply(0.25));
        painter.circle_filled(dot, 4.5, theme::SOLO);
    }
    for (text, anchor, align) in [
        (
            tr("OUT"),
            rect.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
        ),
        (
            tr("IN"),
            rect.right_bottom() - vec2(8.0, 6.0),
            Align2::RIGHT_BOTTOM,
        ),
    ] {
        painter.text(anchor, align, text, FontId::proportional(9.0), p.weak);
    }
    let response = response.on_hover_text(tr("Drag sideways to move the threshold"));
    let changed = response
        .dragged()
        .then(|| threshold + response.drag_delta().x / rect.width() * 60.0);
    params.set("threshold", changed, &response);
}

/// The gain reduction as a bar from the top, with its scale.
fn reduction_meter(ui: &mut Ui, reduction: f32, height: f32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let bar = Rect::from_min_max(
        rect.left_top() + vec2(4.0, 0.0),
        pos2(rect.left() + 14.0, rect.bottom()),
    );
    painter.rect_filled(bar, 4.0, p.base);
    let depth = (-reduction / 24.0).clamp(0.0, 1.0);
    painter.rect_filled(
        Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + depth * bar.height())),
        4.0,
        theme::RECORD,
    );
    for db in [0.0f32, -3.0, -6.0, -12.0, -18.0, -24.0] {
        let y = bar.top() + (-db / 24.0) * bar.height();
        painter.text(
            pos2(rect.right(), y.clamp(rect.top() + 5.0, rect.bottom() - 5.0)),
            Align2::RIGHT_CENTER,
            format!("{:.0}", db.abs()),
            FontId::proportional(8.5),
            p.weak,
        );
    }
}

// ---------------------------------------------------------------- Limiter and Maximizer

/// Bottom of the limiter meters and display, and top of the display, in dBFS.
const METER_FLOOR: f32 = -36.0;
const DISPLAY_TOP: f32 = 6.0;
/// Points per second of the scrolling display, and how many it shows.
const TRACE_RATE: f64 = 30.0;
const TRACE_POINTS: usize = 180;
/// Loudness targets for Learn, in LUFS.
const TARGETS: [(f32, &str); 5] = [
    (-9.0, "-9 LUFS (loud)"),
    (-11.0, "-11 LUFS"),
    (-14.0, "-14 LUFS (streaming)"),
    (-16.0, "-16 LUFS (Apple Music)"),
    (-23.0, "-23 LUFS (broadcast)"),
];
const MAXIMIZER_MODES: [&str; 3] = [
    "Cleanest: no clipping, slower release",
    "Shaves the tallest peaks first, faster release",
    "Loudest: more clipping, fastest release",
];

/// What the limiter display remembers between frames.
#[derive(Debug, Default)]
pub struct LimiterState {
    /// Driven input and output peaks (dBFS), oldest first.
    trace: VecDeque<[f32; 2]>,
    trace_time: f64,
    target: Option<f32>,
    /// When Learn started and when it last moved the threshold.
    learn: Option<(f64, f64)>,
}

fn limiter(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    limiter_view(ui, params, meters, &mut state.limiter, "input");
    ui.horizontal_wrapped(|ui| {
        param_slider(ui, params, "release", 200.0);
    });
}

fn maximizer(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    limiter_view(ui, params, meters, &mut state.limiter, "gain");
    ui.horizontal_wrapped(|ui| {
        mode_buttons(ui, params, "style", &MAXIMIZER_MODES);
        ui.add_space(16.0);
        param_slider(ui, params, "release", 150.0);
        ui.add_space(8.0);
        param_slider(ui, params, "link", 110.0);
    });
}

/// A mastering limiter's face: the input meter with the threshold, what the limiter takes
/// away scrolling in the middle, the output meter with the ceiling, then the readouts.
fn limiter_view(
    ui: &mut Ui,
    params: &mut Params,
    meters: &[f32],
    state: &mut LimiterState,
    gain_id: &str,
) {
    let p = theme::current(ui);
    let meter = |i: usize, default: f32| meters.get(i).copied().unwrap_or(default);
    let reduction = meter(0, 0.0);
    let inputs = [meter(2, -120.0), meter(3, -120.0)];
    let outputs = [meter(4, -120.0), meter(5, -120.0)];
    let (in_lufs, out_lufs) = (meter(6, -120.0), meter(7, -120.0));
    let gain = params.get(gain_id);
    let ceiling = params.get("ceiling");
    record_trace(
        ui,
        state,
        inputs[0].max(inputs[1]) + gain,
        outputs[0].max(outputs[1]),
    );

    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(560.0), 230.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.panel);
    let side = 78.0;
    let left = Rect::from_min_max(rect.min, pos2(rect.left() + side, rect.bottom()));
    let right = Rect::from_min_max(pos2(rect.right() - side, rect.top()), rect.max);
    let display = Rect::from_min_max(
        pos2(left.right(), rect.top() + 8.0),
        pos2(right.left(), rect.bottom() - 8.0),
    );
    draw_trace(&painter, display, &state.trace, ceiling, &p);

    // Input peaks above the threshold are pushed into the ceiling.
    let threshold = ceiling - gain;
    let (response, moved) = level_column(
        ui,
        &painter,
        left,
        (
            tr("THRESHOLD"),
            tr("Drag down to push the input harder into the ceiling"),
        ),
        (inputs, threshold),
        p.accent,
        true,
    );
    let default = params.param(gain_id).map_or(0.0, |p| p.default);
    let moved = if response.double_clicked() {
        Some(default)
    } else {
        moved.map(|db| ceiling - db)
    };
    params.set(gain_id, moved, &response);
    let (response, moved) = level_column(
        ui,
        &painter,
        right,
        (
            tr("CEILING"),
            tr("Drag to set the highest level the output reaches"),
        ),
        (outputs, ceiling),
        theme::SOLO,
        false,
    );
    let default = params.param("ceiling").map_or(0.0, |p| p.default);
    let moved = if response.double_clicked() {
        Some(default)
    } else {
        moved
    };
    params.set("ceiling", moved, &response);

    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        let text = |v: f32, unit: &str| {
            if v > -100.0 {
                format!("{v:.1} {unit}")
            } else {
                "-".into()
            }
        };
        let reduced = if reduction < -0.1 {
            theme::RECORD
        } else {
            p.text
        };
        let shown = if reduction > -0.05 { 0.0 } else { reduction };
        readout(ui, tr("GAIN REDUCTION"), format!("{shown:.1} dB"), reduced);
        let peak = outputs[0].max(outputs[1]);
        readout(ui, tr("OUTPUT PEAK"), text(peak, "dB"), p.text);
        readout(ui, tr("SHORT-TERM"), text(out_lufs, "LUFS"), p.text);
        let louder =
            (in_lufs > -70.0 && out_lufs > -70.0).then(|| format!("{:+.1} dB", out_lufs - in_lufs));
        readout(
            ui,
            tr("LOUDER BY"),
            louder.unwrap_or_else(|| "-".into()),
            p.accent,
        );
        ui.add_space(10.0);
        learn(ui, params, state, gain_id, out_lufs);
    });
    ui.add_space(6.0);
}

/// Adds the levels of the frames since the last call to the scrolling display.
fn record_trace(ui: &Ui, state: &mut LimiterState, input: f32, output: f32) {
    let now = ui.input(|i| i.time);
    let step = 1.0 / TRACE_RATE;
    if now - state.trace_time > 1.0 {
        state.trace_time = now - step;
    }
    while now - state.trace_time >= step {
        state.trace_time += step;
        state.trace.push_back([input, output]);
    }
    while state.trace.len() > TRACE_POINTS {
        state.trace.pop_front();
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f64(step));
}

/// The input pushed into the limiter (red where it is taken away) and the output over it,
/// mirrored around the middle like a waveform, with the ceiling.
fn draw_trace(
    painter: &egui::Painter,
    rect: Rect,
    trace: &VecDeque<[f32; 2]>,
    ceiling: f32,
    p: &theme::Palette,
) {
    painter.rect_filled(rect, 6.0, p.base);
    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 6.0;
    let reach = |db: f32| ((db - METER_FLOOR) / (DISPLAY_TOP - METER_FLOOR)).clamp(0.0, 1.0) * half;
    let grid = p.weak.gamma_multiply(0.15);
    for db in [-24.0, -12.0, -6.0, 0.0] {
        for y in [mid - reach(db), mid + reach(db)] {
            painter.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(1.0, grid),
            );
        }
    }
    let step = rect.width() / TRACE_POINTS as f32;
    let x0 = rect.right() - trace.len() as f32 * step;
    let points = |k: usize| -> Vec<(f32, f32)> {
        trace
            .iter()
            .enumerate()
            .map(|(i, v)| (x0 + i as f32 * step, reach(v[k])))
            .collect()
    };
    painter.add(mirrored(
        &points(0),
        mid,
        theme::RECORD.gamma_multiply(0.75),
    ));
    let output = points(1);
    painter.add(mirrored(&output, mid, p.base.lerp_to_gamma(p.accent, 0.5)));
    for sign in [-1.0, 1.0] {
        let edge = output
            .iter()
            .map(|&(x, a)| pos2(x, mid - sign * a))
            .collect();
        painter.add(Shape::line(edge, Stroke::new(1.2, p.accent)));
    }
    for y in [mid - reach(ceiling), mid + reach(ceiling)] {
        painter.extend(Shape::dashed_line(
            &[pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.2, theme::SOLO),
            6.0,
            4.0,
        ));
    }
}

/// A band between `mid - a` and `mid + a` along the points `(x, a)`.
fn mirrored(points: &[(f32, f32)], mid: f32, color: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    for pair in points.windows(2) {
        let i = mesh.vertices.len() as u32;
        let [(x0, a0), (x1, a1)] = [pair[0], pair[1]];
        for q in [
            pos2(x0, mid - a0),
            pos2(x1, mid - a1),
            pos2(x1, mid + a1),
            pos2(x0, mid + a0),
        ] {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    Shape::mesh(mesh)
}

/// A stereo peak meter with a level marker (threshold or ceiling) dragged up and down;
/// returns the column's response and where the marker was dragged to.
fn level_column(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    (label, hint): (&str, &str),
    (levels, marker): ([f32; 2], f32),
    color: Color32,
    scale_left: bool,
) -> (Response, Option<f32>) {
    let p = theme::current(ui);
    let (top, bottom) = (rect.top() + 24.0, rect.bottom() - 26.0);
    let height = bottom - top;
    let y = |db: f32| bottom - ((db - METER_FLOOR) / -METER_FLOOR).clamp(0.0, 1.0) * height;
    let cx = rect.center().x + if scale_left { 8.0 } else { -8.0 };
    for (i, level) in levels.into_iter().enumerate() {
        let x = cx - 10.5 + i as f32 * 12.0;
        let bar = Rect::from_min_max(pos2(x, top), pos2(x + 9.0, bottom));
        painter.rect_filled(bar, 2.0, p.base);
        for (low, high, zone) in [
            (METER_FLOOR, -12.0, theme::PLAY),
            (-12.0, -3.0, theme::SOLO),
            (-3.0, 0.0, theme::RECORD),
        ] {
            if level > low {
                painter.rect_filled(
                    Rect::from_min_max(pos2(x, y(level.min(high))), pos2(x + 9.0, y(low))),
                    0.0,
                    zone,
                );
            }
        }
    }
    let (inner, outer) = if scale_left {
        (cx + 16.0, cx - 16.0)
    } else {
        (cx - 16.0, cx + 16.0)
    };
    for db in [0.0, -6.0, -12.0, -24.0, -36.0] {
        painter.text(
            pos2(outer, y(db)),
            if scale_left {
                Align2::RIGHT_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            format!("{db}"),
            FontId::proportional(8.5),
            p.weak,
        );
    }
    let my = y(marker);
    painter.line_segment(
        [pos2(cx - 15.0, my), pos2(cx + 15.0, my)],
        Stroke::new(2.0, color),
    );
    let tip = if scale_left { -1.0 } else { 1.0 };
    painter.add(Shape::convex_polygon(
        vec![
            pos2(inner, my),
            pos2(inner - tip * 9.0, my - 6.0),
            pos2(inner - tip * 9.0, my + 6.0),
        ],
        color,
        Stroke::NONE,
    ));
    painter.text(
        pos2(rect.center().x, rect.top() + 7.0),
        Align2::CENTER_TOP,
        label,
        FontId::proportional(9.0),
        p.weak,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 7.0),
        Align2::CENTER_BOTTOM,
        format!("{marker:.1} dB"),
        FontId::monospace(11.0),
        color,
    );
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("limiter", label)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text(format!(
            "{hint}. {}",
            tr("Shift for fine, double-click to reset")
        ));
    let fine = if ui.input(|i| i.modifiers.shift) {
        0.2
    } else {
        1.0
    };
    let moved = response
        .dragged()
        .then(|| marker + response.drag_delta().y / height * METER_FLOOR * fine);
    (response, moved)
}

fn readout(ui: &mut Ui, label: &str, value: String, color: Color32) {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_min_width(96.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new(label).size(9.0).color(p.weak));
                ui.label(RichText::new(value).monospace().size(15.0).color(color));
            });
        });
}

/// Learn: while the music plays, moves the threshold until the output reaches the target
/// loudness, waiting for the short-term loudness (3 s) to follow each move.
fn learn(ui: &mut Ui, params: &mut Params, state: &mut LimiterState, gain_id: &str, out_lufs: f32) {
    let p = theme::current(ui);
    let target = state.target.unwrap_or(-14.0);
    ui.vertical(|ui| {
        ui.label(RichText::new(tr("TARGET")).size(9.0).color(p.weak));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("limiter-target", params.insert.0))
                .selected_text(format!("{target:.0} LUFS"))
                .show_ui(ui, |ui| {
                    for (value, name) in TARGETS {
                        if ui.selectable_label(target == value, tr(name)).clicked() {
                            state.target = Some(value);
                        }
                    }
                });
            let now = ui.input(|i| i.time);
            let learning = state.learn.is_some();
            let response = ui
                .add(
                    egui::Button::new(if learning {
                        tr("Learning")
                    } else {
                        tr("Learn")
                    })
                    .selected(learning),
                )
                .on_hover_text(tr(
                    "Play the loudest part: the threshold moves until the output reaches \
                     the target loudness",
                ));
            if response.clicked() {
                state.learn = (!learning).then_some((now, now - 3.0));
            }
            let Some((started, last)) = state.learn else {
                return;
            };
            ui.spinner();
            if now - started > 30.0 {
                state.learn = None;
                return;
            }
            if out_lufs < -70.0 || now - last < 3.0 {
                return;
            }
            let difference = target - out_lufs;
            let Some(range) = params.param(gain_id).map(|p| p.min..=p.max) else {
                return;
            };
            let gain = params.get(gain_id);
            let next = (gain + difference).clamp(*range.start(), *range.end());
            if difference.abs() < 0.3 || (next - gain).abs() < 0.05 {
                state.learn = None;
                return;
            }
            params.set(gain_id, Some(next), &response);
            params.doc.history_mut().end_gesture();
            state.learn = Some((started, now));
        });
    });
}

/// Buttons side by side for a parameter with named values.
pub(crate) fn mode_buttons(ui: &mut Ui, params: &mut Params, id: &str, hints: &[&'static str]) {
    let Some(p) = params.param(id) else {
        return;
    };
    ui.vertical(|ui| {
        ui.label(
            RichText::new(tr(p.name).to_uppercase())
                .size(9.0)
                .color(theme::current(ui).weak),
        );
        ui.horizontal(|ui| segmented(ui, params, id, hints));
    });
}

/// One button per named value of a parameter, the current one lit.
fn segmented(ui: &mut Ui, params: &mut Params, id: &str, hints: &[&'static str]) {
    let Some(p) = params.param(id) else {
        return;
    };
    #[cfg(test)]
    tests::SHOWN.with_borrow_mut(|shown| shown.push(id.to_owned()));
    let value = params.get(id);
    ui.spacing_mut().item_spacing.x = 2.0;
    for (i, name) in p.choices.iter().enumerate() {
        let v = p.min + i as f32;
        let mut response = ui.selectable_label((value - v).abs() < 0.5, tr(name));
        if let Some(hint) = hints.get(i) {
            response = response.on_hover_text(tr(hint));
        }
        if response.clicked() {
            params.set(id, Some(v), &response);
            params.doc.history_mut().end_gesture();
        }
    }
}

/// A horizontal slider with the parameter's name above and its value beside it.
fn param_slider(ui: &mut Ui, params: &mut Params, id: &str, width: f32) {
    let Some(p) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let mut t = to_unit(p, value);
    ui.vertical(|ui| {
        ui.label(
            RichText::new(tr(p.name).to_uppercase())
                .size(9.0)
                .color(theme::current(ui).weak),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = width;
            let response = ui.add(egui::Slider::new(&mut t, 0.0..=1.0).show_value(false));
            let changed = response.changed().then(|| from_unit(p, t));
            ui.label(
                RichText::new(format_value(p, changed.unwrap_or(value)))
                    .monospace()
                    .size(11.0),
            );
            params.set(id, changed, &response);
        });
    });
}

// ---------------------------------------------------------------- Input and output levels

/// The level going into an insert and coming out of it, the gain after the effect, and Match:
/// a listening aid bringing the output to the input's loudness, to hear what the effect does
/// at equal level. Match is never saved nor exported; Keep moves its correction into the gain.
pub fn levels_bar(
    ui: &mut Ui,
    doc: &mut Document,
    channel: Channel,
    insert: &audiostrada_core::Insert,
    state: &mut EditorState,
) {
    let p = theme::current(ui);
    let id = insert.id;
    doc.player_mut().set_insert_watched(channel, id, true);
    doc.player_mut()
        .set_insert_match(channel, id, state.matching);
    let levels = doc.player_mut().insert_levels(channel, id);
    let [in_l, in_r, out_l, out_r, in_lufs, out_lufs, matched] =
        levels.unwrap_or([-120.0, -120.0, -120.0, -120.0, -120.0, -120.0, 0.0]);
    let loud = |v: f32| v > -70.0;
    // Wraps rather than cutting Match and Keep off in a narrow window.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (label, peaks, lufs) in [
            (tr("IN"), [in_l, in_r], in_lufs),
            (tr("OUT"), [out_l, out_r], out_lufs),
        ] {
            ui.label(RichText::new(label).size(9.0).color(p.weak));
            let (rect, _) = ui.allocate_exact_size(vec2(96.0, 12.0), Sense::hover());
            let painter = ui.painter_at(rect);
            for (i, db) in peaks.into_iter().enumerate() {
                let bar = Rect::from_min_size(
                    rect.min + vec2(0.0, i as f32 * 6.5),
                    vec2(rect.width(), 5.5),
                );
                painter.rect_filled(bar, 1.0, p.base);
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
                            0.0,
                            color,
                        );
                    }
                }
            }
            let text = if loud(lufs) {
                format!("{lufs:5.1} LUFS")
            } else {
                "   - LUFS".to_owned()
            };
            ui.label(RichText::new(text).monospace().size(10.5));
        }
        ui.separator();
        ui.label(RichText::new(tr("Output")).size(10.0).color(p.weak));
        let mut gain = insert.gain_db;
        let response = ui
            .add(
                egui::DragValue::new(&mut gain)
                    .range(-24.0..=24.0)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .suffix(" dB"),
            )
            .on_hover_text(tr(
                "Gain after the effect, kept in the mix and the export. Double-click to type",
            ));
        if gain != insert.gain_db {
            doc.history_mut().edit_merged(
                tr("Insert gain"),
                &format!("insert-gain:{}", id.0),
                |p| routing::set_insert_gain(p, channel, id, gain),
            );
        }
        if response.drag_stopped() || response.lost_focus() {
            doc.history_mut().end_gesture();
        }
        if ui
            .add(egui::Button::new(RichText::new(tr("Match")).strong()).selected(state.matching))
            .on_hover_text(tr(
                "Listening aid: while on, the output is brought to the input's loudness, so \
                 that bypassing compares the effect at equal level. Not saved, not exported; \
                 it turns off when this window closes",
            ))
            .clicked()
        {
            state.matching = !state.matching;
        }
        if state.matching {
            ui.label(
                RichText::new(format!("{matched:+.1} dB"))
                    .monospace()
                    .size(10.5)
                    .color(p.accent),
            );
            if ui
                .add_enabled(matched.abs() >= 0.05, egui::Button::new(tr("Keep")))
                .on_hover_text(tr("Put this correction into Output, to keep it in the mix"))
                .clicked()
            {
                let kept = insert.gain_db + matched;
                doc.history_mut().edit(tr("Keep matched gain"), |p| {
                    routing::set_insert_gain(p, channel, id, kept)
                });
                state.matching = false;
            }
        }
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_positions_round_trip_every_parameter() {
        for info in audiostrada_dsp::EFFECTS {
            for p in info.params.iter().filter(|p| !p.toggle && p.max > p.min) {
                for t in [0.0, 0.3, 1.0] {
                    let back = to_unit(p, from_unit(p, t));
                    assert!(
                        (back - t).abs() < 1e-3,
                        "{}.{}: {t} -> {back}",
                        info.id,
                        p.id
                    );
                }
            }
        }
    }

    #[test]
    fn values_show_with_their_unit() {
        let eq = audiostrada_dsp::info("eq").unwrap();
        let param = |id: &str| eq.params.iter().find(|p| p.id == id).unwrap();
        assert_eq!(format_value(param("b1_freq"), 1500.0), "1.50 kHz");
        assert_eq!(format_value(param("b1_freq"), 50.0), "50.0 Hz");
        assert_eq!(format_value(param("b1_gain"), 3.0), "+3.0 dB");
        assert_eq!(format_value(param("b1_type"), 0.0), "Bell");
        assert_eq!(choice_name(param("b1_type"), 99.0), None);
    }

    #[test]
    fn only_the_eq_bands_in_use_are_automatable() {
        let params: std::collections::BTreeMap<String, f32> =
            [("b1_used".to_owned(), 1.0)].into_iter().collect();
        assert!(automatable("eq", &params, "b1_freq"));
        assert!(!automatable("eq", &params, "b1_type"));
        assert!(!automatable("eq", &params, "b2_freq"));
        assert!(!automatable("eq", &params, "solo"));
        assert!(automatable("eq", &params, "output"));
        assert!(automatable("compressor", &params, "anything"));
    }
    use crate::document::{AppDirs, Document};
    use eframe::egui::{Event, Modifiers, PointerButton, RawInput};

    thread_local! {
        /// Parameters that got a control while drawing an editor.
        pub(super) static SHOWN: std::cell::RefCell<Vec<String>> = const {
            std::cell::RefCell::new(Vec::new())
        };
    }

    #[test]
    fn every_editor_shows_every_parameter() {
        let mut missing = Vec::new();
        for info in audiostrada_dsp::EFFECTS {
            // The drum machine shows the settings of one voice at a time.
            if matches!(info.id, "amp_rig" | "eq" | "drums") {
                continue;
            }
            let mut doc = Document::new(AppDirs::default());
            let mut insert = None;
            doc.history_mut().edit("Setup", |p| {
                let t = p.add_track("A");
                insert = routing::add_insert(p, Channel::Track(t), info.id, Default::default());
            });
            let channel = Channel::Track(doc.project().tracks[0].id);
            let ctx = egui::Context::default();
            let mut state = EditorState::default();
            SHOWN.with_borrow_mut(Vec::clear);
            for _ in 0..2 {
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 1600.0))),
                    ..Default::default()
                };
                ctx.run_ui(input, |ui| {
                    let mut params = Params {
                        values: info.params.iter().map(|p| p.default).collect(),
                        doc: &mut doc,
                        channel,
                        insert: insert.unwrap(),
                        info,
                    };
                    show(ui, &mut params, &[0.0; 16], &mut state);
                })
                .drop_without_applying_deltas();
            }
            let shown = SHOWN.with_borrow(Clone::clone);
            // Shown only where they apply: a synced note, the analog delay's tape, the shimmer.
            let conditional = [
                ("filter", "note"),
                ("delay", "note"),
                ("delay", "wow"),
                ("delay", "drive"),
                ("reverb", "shimmer"),
                // Set by dragging the modules; from the console.
                ("channel", "order"),
                ("channel", "character"),
            ];
            for p in info.params {
                if !shown.iter().any(|s| s == p.id) && !conditional.contains(&(info.id, p.id)) {
                    missing.push(format!("{}: {}", info.id, p.id));
                }
            }
        }
        assert!(missing.is_empty(), "no control for {missing:?}");
    }

    #[test]
    fn double_clicks_add_and_remove_eq_bands() {
        let mut doc = Document::new(AppDirs::default());
        let (mut insert, mut track) = (None, None);
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("A");
            insert = routing::add_insert(p, Channel::Track(t), "eq", Default::default());
            track = Some(t);
        });
        let (channel, insert) = (Channel::Track(track.unwrap()), insert.unwrap());
        let info = audiostrada_dsp::info("eq").unwrap();
        let ctx = egui::Context::default();
        let mut state = EditorState::default();
        let mut time = 0.0;
        let mut frame = |doc: &mut Document, events: Vec<Event>| {
            time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let values = info
                    .params
                    .iter()
                    .map(|q| {
                        doc.project().tracks[0].inserts[0]
                            .params
                            .get(q.id)
                            .copied()
                            .unwrap_or(q.default)
                    })
                    .collect();
                let mut params = Params {
                    doc: &mut *doc,
                    channel,
                    insert,
                    info,
                    values,
                };
                eq(ui, &mut params, &[], &mut state);
            })
            .drop_without_applying_deltas();
        };
        let double_click =
            |doc: &mut Document, frame: &mut dyn FnMut(&mut Document, Vec<Event>), at: Pos2| {
                let button = |pressed| Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                };
                frame(doc, vec![Event::PointerMoved(at)]);
                for _ in 0..2 {
                    frame(doc, vec![button(true)]);
                    frame(doc, vec![button(false)]);
                }
                frame(doc, vec![]);
            };
        frame(&mut doc, vec![]);
        // The display spans the width, 300 high: its middle is 1 kHz-ish at 0 dB.
        let at = pos2(450.0, 150.0);
        double_click(&mut doc, &mut frame, at);
        let params = |doc: &Document| doc.project().tracks[0].inserts[0].params.clone();
        let added = params(&doc);
        assert_eq!(added.get("b1_used"), Some(&1.0));
        assert_eq!(added.get("b1_type"), Some(&0.0), "a bell");
        assert!(
            (added["b1_freq"] - 632.0).abs() < 10.0,
            "{}",
            added["b1_freq"]
        );
        // A pause, or the next clicks would count as a triple click.
        for _ in 0..20 {
            frame(&mut doc, vec![]);
        }
        // Ctrl+drag up 80 points: twice the Q, same frequency.
        let ctrl = Modifiers::COMMAND;
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: ctrl,
        };
        frame(
            &mut doc,
            vec![Event::ModifiersChanged(ctrl), Event::PointerMoved(at)],
        );
        frame(&mut doc, vec![press(at, true)]);
        for k in 1..=8 {
            frame(
                &mut doc,
                vec![Event::PointerMoved(at - vec2(0.0, 10.0 * k as f32))],
            );
        }
        frame(&mut doc, vec![press(at - vec2(0.0, 80.0), false)]);
        frame(&mut doc, vec![Event::ModifiersChanged(Modifiers::NONE)]);
        let narrowed = params(&doc);
        assert!((narrowed["b1_q"] - 2.0).abs() < 0.2, "{}", narrowed["b1_q"]);
        assert_eq!(narrowed["b1_freq"], added["b1_freq"]);
        for _ in 0..20 {
            frame(&mut doc, vec![]);
        }
        double_click(&mut doc, &mut frame, at);
        assert_eq!(params(&doc).get("b1_used"), Some(&0.0), "removed");
    }
}

// ---------------------------------------------------------------- Spectrum analyzer

/// Rows of the spectrogram, one per frame drawn.
const SPECTROGRAM_ROWS: usize = 300;

/// What the analyzer display remembers between frames.
#[derive(Default)]
pub struct SpectrumState {
    /// Highest level of each band since the hold was reset.
    peaks: Vec<f32>,
    /// Spectrogram rows, newest last.
    rows: VecDeque<Vec<f32>>,
    texture: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for SpectrumState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpectrumState")
            .field("rows", &self.rows.len())
            .finish()
    }
}

/// Position of a frequency across the display, 0 at 20 Hz and 1 at 20 kHz.
fn frequency_fraction(hz: f32) -> f32 {
    (hz / 20.0).log10() / 3.0
}

/// Name of the note nearest a frequency, as A4 or C#3.
fn note_name(hz: f32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let number = (69.0 + 12.0 * (hz / 440.0).log2()).round() as i32;
    format!(
        "{}{}",
        NAMES[number.rem_euclid(12) as usize],
        number.div_euclid(12) - 1
    )
}

fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// Color of a level in the spectrogram, from the floor (dark) to 0 dB (white hot).
fn heat(t: f32, accent: Color32, base: Color32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        base.lerp_to_gamma(accent, t * 2.0)
    } else if t < 0.8 {
        accent.lerp_to_gamma(theme::SOLO, (t - 0.5) / 0.3)
    } else {
        theme::SOLO.lerp_to_gamma(Color32::WHITE, (t - 0.8) / 0.2)
    }
}

fn analyzer(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut SpectrumState) {
    let p = theme::current(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.horizontal(|ui| segmented(ui, params, "view", &[]));
        ui.separator();
        ui.horizontal(|ui| {
            segmented(
                ui,
                params,
                "source",
                &[
                    "Both channels together",
                    "Left channel",
                    "Right channel",
                    "What differs between the channels",
                ],
            )
        });
        ui.separator();
        ui.horizontal(|ui| segmented(ui, params, "range", &[]));
        ui.separator();
        params.toggle(ui, "hold", |ui, on| {
            ui.toggle_value(on, tr("Peak hold")).on_hover_text(tr(
                "Keeps the highest level of each band; click the display to clear it",
            ))
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        param_knob(ui, params, "decay", 34.0, p.accent);
        ui.weak(tr("How fast the display falls back after a peak"))
            .on_hover_text(tr("Slow speeds show the average tone, fast ones every hit"));
        ui.add_space(12.0);
        param_knob(ui, params, "tilt", 34.0, p.accent);
        ui.weak(tr("Raises the highs, so that a balanced mix looks flat"));
    });
    ui.add_space(6.0);

    let floor = audiostrada_dsp::analyzer::range_floor(params.get("range"));
    let tilt = params.get("tilt");
    let view = params.get("view").round() as usize;
    let hold = params.get("hold") > 0.5;
    let bands = audiostrada_dsp::analyzer::BANDS;
    let levels: Vec<f32> = (0..bands)
        .map(|b| {
            let db = meters.get(b).copied().unwrap_or(-120.0);
            let hz = audiostrada_dsp::analyzer::band_frequency(b);
            db + tilt * (hz / 1000.0).log2()
        })
        .collect();
    if state.peaks.len() != bands || !hold {
        state.peaks = levels.clone();
    } else {
        for (peak, &db) in state.peaks.iter_mut().zip(&levels) {
            *peak = peak.max(db);
        }
    }
    state.rows.push_back(levels.clone());
    while state.rows.len() > SPECTROGRAM_ROWS {
        state.rows.pop_front();
    }

    let size = vec2(
        ui.available_width().max(420.0),
        (ui.available_height() - 8.0).max(220.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        state.peaks = levels.clone();
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    // Room for the scales.
    let plot = Rect::from_min_max(rect.min + vec2(36.0, 8.0), rect.max - vec2(8.0, 20.0));
    let x_of = |hz: f32| plot.left() + frequency_fraction(hz) * plot.width();
    let y_of = |db: f32| plot.top() + (db / floor).clamp(0.0, 1.0) * plot.height();
    let grid = p.weak.gamma_multiply(0.18);
    let label = |pos: Pos2, align: Align2, text: String| {
        painter.text(pos, align, text, FontId::proportional(10.0), p.weak);
    };
    for hz in [
        30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0,
        800.0, 900.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0, 8000.0, 9000.0,
        10000.0, 20000.0,
    ] {
        let x = x_of(hz);
        let major = [100.0, 1000.0, 10000.0].contains(&hz);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(
                1.0,
                if major {
                    grid.gamma_multiply(2.0)
                } else {
                    grid
                },
            ),
        );
        if [
            50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
        ]
        .contains(&hz)
        {
            let text = if hz >= 1000.0 {
                format!("{}k", hz / 1000.0)
            } else {
                format!("{hz}")
            };
            label(pos2(x, plot.bottom() + 4.0), Align2::CENTER_TOP, text);
        }
    }
    if view == 2 {
        draw_spectrogram(ui, &painter, plot, state, floor, &p);
    } else {
        let step = match floor as i32 {
            -60 => 10,
            -90 => 15,
            _ => 20,
        };
        for db in (floor as i32..=0).step_by(step) {
            let y = y_of(db as f32);
            painter.line_segment(
                [pos2(plot.left(), y), pos2(plot.right(), y)],
                Stroke::new(1.0, grid),
            );
            label(
                pos2(plot.left() - 4.0, y),
                Align2::RIGHT_CENTER,
                format!("{db}"),
            );
        }
        let points: Vec<Pos2> = levels
            .iter()
            .enumerate()
            .map(|(b, &db)| pos2(x_of(audiostrada_dsp::analyzer::band_frequency(b)), y_of(db)))
            .collect();
        if view == 1 {
            let width = plot.width() / bands as f32;
            for point in &points {
                let bar = Rect::from_min_max(
                    pos2(point.x - width / 2.0 + 0.5, point.y),
                    pos2(point.x + width / 2.0 - 0.5, plot.bottom()),
                );
                painter.rect_filled(bar, 0.0, p.accent.gamma_multiply(0.8));
            }
        } else {
            let mut mesh = egui::Mesh::default();
            let fill = p.accent.gamma_multiply(0.25);
            for point in &points {
                mesh.colored_vertex(*point, fill);
                mesh.colored_vertex(pos2(point.x, plot.bottom()), fill.gamma_multiply(0.2));
            }
            for i in 0..points.len().saturating_sub(1) as u32 {
                mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
                mesh.add_triangle(2 * i + 1, 2 * i + 2, 2 * i + 3);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::line(points, Stroke::new(1.5, p.accent)));
        }
        if hold {
            let peaks = state
                .peaks
                .iter()
                .enumerate()
                .map(|(b, &db)| pos2(x_of(audiostrada_dsp::analyzer::band_frequency(b)), y_of(db)))
                .collect();
            painter.add(Shape::line(
                peaks,
                Stroke::new(1.0, theme::SOLO.gamma_multiply(0.8)),
            ));
        }
    }
    if let Some(pointer) = response.hover_pos().filter(|pos| plot.contains(*pos)) {
        let hz = 20.0 * 1000f32.powf((pointer.x - plot.left()) / plot.width());
        let band = ((frequency_fraction(hz) * bands as f32) as usize).min(bands - 1);
        painter.line_segment(
            [pos2(pointer.x, plot.top()), pos2(pointer.x, plot.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.5)),
        );
        let text = format!(
            "{}  {}  {:.1} dB",
            format_hz(hz),
            note_name(hz),
            levels[band]
        );
        let align = if pointer.x > plot.center().x {
            Align2::RIGHT_TOP
        } else {
            Align2::LEFT_TOP
        };
        let offset = if pointer.x > plot.center().x {
            -8.0
        } else {
            8.0
        };
        painter.text(
            pos2(pointer.x + offset, plot.top() + 4.0),
            align,
            text,
            FontId::monospace(11.0),
            p.text,
        );
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

/// Frequencies across, time downwards (newest at the top), level as heat.
fn draw_spectrogram(
    ui: &Ui,
    painter: &egui::Painter,
    plot: Rect,
    state: &mut SpectrumState,
    floor: f32,
    p: &theme::Palette,
) {
    let bands = audiostrada_dsp::analyzer::BANDS;
    let mut image = egui::ColorImage::filled([bands, SPECTROGRAM_ROWS], p.base);
    for (row, levels) in state.rows.iter().rev().enumerate() {
        for (band, &db) in levels.iter().enumerate() {
            image.pixels[row * bands + band] = heat(1.0 - db / floor, p.accent, p.base);
        }
    }
    let texture = match &mut state.texture {
        Some(texture) => {
            texture.set(image, egui::TextureOptions::LINEAR);
            texture
        }
        None => state.texture.insert(ui.ctx().load_texture(
            "spectrogram",
            image,
            egui::TextureOptions::LINEAR,
        )),
    };
    // Bands are log-spaced like the frequency scale, so the image maps straight onto it.
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(texture.id(), plot, uv, Color32::WHITE);
}
