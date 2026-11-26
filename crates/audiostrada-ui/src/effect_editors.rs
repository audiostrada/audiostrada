//! Dedicated editors for built-in effects: a visual main view with a control for every
//! parameter (some only where they apply, as the note of a synced delay).

use audiostrada_core::InsertId;
use audiostrada_core::automation::AutomationTarget;
use audiostrada_core::routing::{self, Channel};
use audiostrada_dsp::{EffectInfo, ParamInfo};
#[cfg(test)]
use eframe::egui::{self, Pos2, Rect, Sense, pos2, vec2};
use eframe::egui::{Response, Ui};

pub(crate) use audiostrada_editors::controls::{
    EditorParams, choice_box, choice_list, format_value, from_unit, named_knob, pill_switch,
    segmented, to_unit, wide_knob, wide_knob_with,
};
#[cfg(test)]
use audiostrada_editors::presets::menu_order;
pub(crate) use audiostrada_editors::presets::preset_category;

use crate::document::Document;
use crate::i18n::tr;
use crate::mixer::{automated_control, learn_menu_with};

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
        // One undo step for a whole gesture, even one moving several parameters (a point of
        // the EQ dragged): releasing the control ends it.
        automated_control(self.doc, channel, &lane, changed, response, |doc, value| {
            doc.history_mut().edit_merged(
                &format!("{} {}", tr(name), tr(param.name)),
                &format!("insert:{}", insert.0),
                |p| routing::set_insert_param(p, channel, insert, id, value),
            );
        });
    }

    /// Applies a control's change and ends the gesture when it is released.
    pub fn set(&mut self, id: &str, changed: Option<f32>, response: &Response) {
        self.set_with(id, changed, response, |_| {});
    }

    /// `set`, with `more` items in the control's right-click menu.
    pub(crate) fn set_with(
        &mut self,
        id: &str,
        changed: Option<f32>,
        response: &Response,
        more: impl FnOnce(&mut Ui),
    ) {
        let Some(param) = self.param(id) else {
            return;
        };
        self.change(id, changed, response);
        let lane = AutomationTarget::Param {
            insert: self.insert,
            param: id.to_owned(),
        };
        learn_menu_with(
            response,
            self.doc,
            self.channel,
            &lane,
            (param.min, param.max),
            more,
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

impl EditorParams for Params<'_> {
    fn info(&self) -> &'static EffectInfo {
        self.info
    }

    fn get(&self, id: &str) -> f32 {
        Params::get(self, id)
    }

    fn change(&mut self, id: &str, changed: Option<f32>, response: &Response) {
        Params::change(self, id, changed, response);
    }

    fn set_with(
        &mut self,
        id: &str,
        changed: Option<f32>,
        response: &Response,
        more: &mut dyn FnMut(&mut Ui),
    ) {
        Params::set_with(self, id, changed, response, more);
    }

    fn end_gesture(&mut self) {
        self.doc.history_mut().end_gesture();
    }

    fn salt(&self) -> u64 {
        self.insert.0
    }

    fn edit_many(&mut self, label: &str, changes: &[(String, f32)]) {
        let (channel, insert) = (self.channel, self.insert);
        for (id, v) in changes {
            if let Some(i) = self.index(id) {
                self.values[i] = *v;
            }
        }
        self.doc.history_mut().edit(label, |p| {
            for (id, v) in changes {
                routing::set_insert_param(p, channel, insert, id, *v);
            }
        });
    }

    fn hear(&mut self, id: &str, value: Option<f32>) {
        let project = self.doc.project().clone();
        let target = AutomationTarget::Param {
            insert: self.insert,
            param: id.into(),
        };
        self.doc
            .player_mut()
            .set_override(&project, self.channel, &target, value);
    }

    fn sample_rate(&self) -> u32 {
        self.doc.player().engine().sample_rate()
    }

    fn panic(&mut self) {
        self.doc.player_mut().panic();
    }

    fn file_name(&self, slot: &str) -> Option<String> {
        let insert = routing::find_insert(self.doc.project(), self.channel, self.insert)?;
        let path = insert.files.get(slot)?;
        Some(path.file_stem()?.to_string_lossy().into_owned())
    }

    fn load_file(
        &mut self,
        ui: &Ui,
        slot: &str,
        ask: bool,
        path: Option<std::path::PathBuf>,
    ) -> bool {
        let Some(slot) = self.info.files.iter().find(|f| f.id == slot) else {
            return false;
        };
        let (name, extensions) = (slot.name, slot.extensions);
        let id = ("effect-file", self.channel, self.insert, slot.id);
        let chosen = crate::dialogs::ask(ui.ctx(), id, ask && path.is_none(), move || {
            let dialog = rfd::FileDialog::new()
                .set_title(tr(name))
                .add_filter(tr(name), extensions);
            crate::dialogs::remembered(
                "effect-file",
                crate::dialogs::in_last_folder(dialog, "effect-file").pick_file(),
            )
        });
        let Some(file) = path.or(chosen) else {
            return false;
        };
        match self
            .doc
            .set_effect_file(self.channel, self.insert, slot.id, Some(&file))
        {
            Ok(()) => true,
            Err(e) => {
                self.doc.notify(crate::trf!(
                    "Cannot load {file}: {e}",
                    file = file.display(),
                    e = e
                ));
                false
            }
        }
    }

    fn sidechain_name(&self) -> Option<String> {
        if !audiostrada_dsp::supports_sidechain(self.info.id) {
            return None;
        }
        let insert = routing::find_insert(self.doc.project(), self.channel, self.insert)?;
        let source = insert.sidechain.and_then(|id| {
            crate::effect_frame::key_tracks(self.doc, self.channel)
                .into_iter()
                .find(|t| t.0 == id)
                .map(|t| t.1)
        });
        Some(source.unwrap_or_else(|| tr("This track's input").to_owned()))
    }

    fn can_keep_recording(&self) -> bool {
        self.doc.player().effect_recording(self.insert).is_some()
    }

    fn keep_recording(&mut self, ui: &Ui, slot: &str, ask: bool) -> bool {
        let id = ("effect-recording", self.channel, self.insert);
        let (title, name) = (
            tr("Save the memory as a sound").to_owned(),
            format!("{}.wav", tr("Grain memory")),
        );
        let chosen = crate::dialogs::ask(ui.ctx(), id, ask, move || {
            let dialog = rfd::FileDialog::new()
                .set_title(title)
                .set_file_name(name)
                .add_filter("WAV", &["wav"]);
            crate::dialogs::remembered(
                "export",
                crate::dialogs::in_last_folder(dialog, "export").save_file(),
            )
        });
        let Some(path) = chosen else {
            return false;
        };
        let Some(recording) = self.doc.player().effect_recording(self.insert) else {
            return false;
        };
        let (samples, rate) = recording.read();
        if samples.is_empty() {
            self.doc
                .notify(tr("The memory is empty: nothing was heard yet").to_owned());
            return false;
        }
        let audio = audiostrada_media::AudioData::new(vec![samples], rate);
        if let Err(e) = audiostrada_media::write_wav(&path, &audio) {
            self.doc.notify(crate::trf!(
                "Cannot save {file}: {e}",
                file = path.display(),
                e = e
            ));
            return false;
        }
        self.load_file(ui, slot, false, Some(path))
    }

    fn sidechain_menu(&mut self, ui: &mut Ui) {
        if let Some(insert) =
            routing::find_insert(self.doc.project(), self.channel, self.insert).cloned()
        {
            crate::effect_frame::sidechain_menu(ui, self.doc, self.channel, &insert);
        }
    }

    fn play_key(&mut self, data: [u8; 3]) {
        if let Channel::Track(track) = self.channel {
            self.doc.player().play_key(track, data);
        }
    }

    fn tempo(&self) -> f64 {
        self.doc.project().tempo.bpm_at_beat(0.0)
    }

    fn notify(&mut self, message: String) {
        self.doc.notify(message);
    }

    fn shown(&self, _id: &str) {
        #[cfg(test)]
        tests::SHOWN.with_borrow_mut(|shown| shown.push(_id.to_owned()));
    }
}

/// Per-editor memory between frames: the built-in effects' in `effects`, reached through it.
#[derive(Debug, Default)]
pub struct EditorState {
    /// Presets, A/B and the bars around the editor.
    pub frame: crate::effect_frame::FrameState,
    pub effects: audiostrada_editors::effects::EffectsState,
    pub rig: crate::rig_editor::RigState,
    pub synth: crate::synth_editor::SynthState,
    pub drums: crate::drums_editor::DrumsState,
    pub trigger: crate::trigger_editor::TriggerState,
    /// Match is on (listening only; switched off when the window closes).
    pub matching: bool,
}

impl std::ops::Deref for EditorState {
    type Target = audiostrada_editors::effects::EffectsState;

    fn deref(&self) -> &Self::Target {
        &self.effects
    }
}

impl std::ops::DerefMut for EditorState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.effects
    }
}

pub use audiostrada_editors::is_meter;

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut EditorState) {
    match params.info.id {
        "channel" => crate::channel_editor::show(ui, params, meters),
        "amp_rig" => crate::rig_editor::show(ui, params, meters, &mut state.rig),
        "synth" => crate::synth_editor::show(ui, params, meters, &mut state.synth),
        "drums" => crate::drums_editor::show(ui, params, meters, &mut state.drums),
        "trigger" => crate::trigger_editor::show(ui, params, meters, &mut state.trigger),
        _ => audiostrada_editors::effects::show(ui, params, meters, &mut state.effects),
    }
}

/// Whether a parameter of an effect is offered for automation: for the EQ, the settings of
/// the bands in use.
pub(crate) fn automatable(
    effect: &str,
    params: &std::collections::BTreeMap<String, f32>,
    id: &str,
) -> bool {
    if effect == "mb_comp" {
        return id != "solo";
    }
    // A chain's knobs; its stages' settings are the base they act around.
    if matches!(effect, "mastering" | "vocals") {
        return !id.contains('.') && !audiostrada_dsp::chain::HIDDEN.contains(&id);
    }
    if matches!(effect, "maximizer" | "limiter") && id == "compare" {
        return false;
    }
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

// ---------------------------------------------------------------- Input and output levels

/// The level going into an insert and coming out of it, the gain after the effect, and Match:
/// a listening aid bringing the output to the input's loudness, to hear what the effect does
/// at equal level. Match is never saved nor exported; Keep moves its correction into the gain.
/// Draws the editor of effect `id` for `frames` frames, headless, on a track with a MIDI clip
/// (the instruments have something to show); `set` overrides parameters, `before` changes the
/// editor's state before each frame.
#[cfg(test)]
pub(crate) fn draw_for_test(
    id: &str,
    set: &[(&str, f32)],
    frames: usize,
    before: impl FnMut(usize, &mut EditorState),
) {
    drive_for_test(id, set, frames, before, |_| Vec::new());
}

/// Clicks every `step` pixels over the editor of `id`, and drags up and down from every other
/// point (knobs, faders, curves), as a user trying everything would.
#[cfg(test)]
pub(crate) fn sweep_for_test(id: &str, set: &[(&str, f32)], step: f32) {
    use eframe::egui::{Event, Modifiers, PointerButton, pos2};
    let step = crate::test_sweep::step(step);
    let mut script: Vec<Vec<Event>> = Vec::new();
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let mut y = step / 2.0;
    let mut n = 0;
    while y < 1100.0 {
        let mut x = step / 2.0;
        while x < 1300.0 {
            let at = pos2(x, y);
            script.push(vec![Event::PointerMoved(at)]);
            script.push(vec![button(at, true)]);
            if n % 2 == 1 {
                for dy in [-20.0, 25.0] {
                    script.push(vec![Event::PointerMoved(at + vec2(4.0, dy))]);
                }
            }
            script.push(vec![button(at, false)]);
            n += 1;
            x += step;
        }
        y += step;
    }
    script.push(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]);
    let frames = script.len();
    drive_for_test(
        id,
        set,
        frames,
        |_, _| {},
        |i| std::mem::take(&mut script[i]),
    );
}

/// Draws the editor of `id`, with the events `events` gives for each frame.
#[cfg(test)]
pub(crate) fn drive_for_test(
    id: &str,
    set: &[(&str, f32)],
    frames: usize,
    mut before: impl FnMut(usize, &mut EditorState),
    mut events: impl FnMut(usize) -> Vec<egui::Event>,
) {
    use audiostrada_core::model::Note;
    use eframe::egui::{Pos2, RawInput};
    let info = audiostrada_dsp::info(id).expect("an effect");
    let mut doc = Document::new(crate::document::AppDirs::default());
    let mut insert = None;
    doc.history_mut().edit("Setup", |p| {
        let t = p.add_track("A");
        insert = routing::add_insert(p, Channel::Track(t), id, Default::default());
        let notes = (0..16u64)
            .map(|i| Note {
                start: i * 6_000,
                length: 3_000,
                key: [36, 38, 42, 46][i as usize % 4],
                velocity: 100,
                channel: 9,
                probability: 100,
                muted: false,
            })
            .collect();
        p.add_midi_clip(t, 0, 96_000, notes);
    });
    let channel = Channel::Track(doc.project().tracks[0].id);
    let mut values: Vec<f32> = info.params.iter().map(|p| p.default).collect();
    for (name, value) in set {
        let i = info
            .params
            .iter()
            .position(|p| p.id == *name)
            .expect("a parameter");
        values[i] = *value;
    }
    let ctx = egui::Context::default();
    let mut state = EditorState::default();
    for frame in 0..frames {
        before(frame, &mut state);
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 1200.0))),
            time: Some(frame as f64 * 0.05),
            events: events(frame),
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let mut params = Params {
                values: values.clone(),
                doc: &mut doc,
                channel,
                insert: insert.unwrap(),
                info,
            };
            show(ui, &mut params, &[0.5; 64], &mut state);
            values = params.values;
        })
        .drop_without_applying_deltas();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_band_count_band_and_solo_draws() {
        for count in 0..3 {
            for band in 0..4 {
                draw_for_test("mb_comp", &[("count", count as f32)], 2, |frame, state| {
                    state.mb.band = band;
                    state.mb.solo = (frame == 1).then_some(band);
                });
            }
        }
    }

    #[test]
    fn a_drag_moving_two_parameters_is_one_undo_step() {
        use crate::document::{AppDirs, Document};
        use egui::{Event, Modifiers, PointerButton, RawInput};
        let mut doc = Document::new(AppDirs::default());
        let mut insert = None;
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("A");
            insert = routing::add_insert(p, Channel::Track(t), "synth", Default::default());
        });
        let channel = Channel::Track(doc.project().tracks[0].id);
        let info = audiostrada_dsp::info("synth").unwrap();
        let ctx = egui::Context::default();
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let at = |x: f32| pos2(50.0 + x, 50.0);
        let mut frames = vec![vec![Event::PointerMoved(at(0.0)), button(at(0.0), true)]];
        frames.extend((1..6).map(|k| vec![Event::PointerMoved(at(10.0 * k as f32))]));
        frames.push(vec![button(at(50.0), false)]);
        for (k, events) in frames.into_iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let rect = Rect::from_min_size(Pos2::ZERO, vec2(200.0, 200.0));
                let response = ui.allocate_rect(rect, Sense::drag());
                let mut params = Params {
                    values: info.params.iter().map(|p| p.default).collect(),
                    doc: &mut doc,
                    channel,
                    insert: insert.unwrap(),
                    info,
                };
                // A point of a curve: its frequency and its gain move together.
                params.change("cutoff", Some(1000.0 + 100.0 * k as f32), &response);
                params.change("resonance", Some(10.0 + k as f32), &response);
            })
            .drop_without_applying_deltas();
        }
        doc.history_mut().undo();
        assert!(
            doc.project().tracks[0].inserts[0].params.is_empty(),
            "{:?}",
            doc.project().tracks[0].inserts[0].params
        );
        assert_eq!(doc.history().undo_label(), Some("Setup"));
    }

    #[test]
    fn knob_positions_round_trip_every_parameter() {
        for info in audiostrada_dsp::EFFECTS.iter() {
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

    /// Every fourth effect from `first`, swept (four tests, so that they run side by side).
    fn sweep_quarter(first: usize) {
        for info in audiostrada_dsp::EFFECTS.iter().skip(first).step_by(4) {
            sweep_for_test(info.id, &[], 40.0);
        }
    }

    #[test]
    fn every_editor_answers_a_user_trying_everything_0() {
        sweep_quarter(0);
    }

    #[test]
    fn every_editor_answers_a_user_trying_everything_1() {
        sweep_quarter(1);
    }

    #[test]
    fn every_editor_answers_a_user_trying_everything_2() {
        sweep_quarter(2);
    }

    #[test]
    fn every_editor_answers_a_user_trying_everything_3() {
        sweep_quarter(3);
    }

    /// Every fourth drawing from `first` of every choice of every editor (four tests, side by
    /// side: one editor can have far more choices than the others).
    fn draw_choices_quarter(first: usize) {
        let mut drawings = Vec::new();
        for info in audiostrada_dsp::EFFECTS.iter() {
            for param in info.params.iter().filter(|p| !p.choices.is_empty()) {
                for value in 0..param.choices.len() {
                    drawings.push((info.id, vec![(param.id, value as f32)], 1));
                }
            }
            // Switches on, together.
            let on: Vec<(&str, f32)> = info
                .params
                .iter()
                .filter(|p| p.choices.is_empty() && p.min == 0.0 && p.max == 1.0)
                .map(|p| (p.id, 1.0))
                .collect();
            drawings.push((info.id, on, 2));
        }
        for (id, set, frames) in drawings.into_iter().skip(first).step_by(4) {
            draw_for_test(id, &set, frames, |_, _| {});
        }
    }

    #[test]
    fn every_choice_of_every_editor_draws_0() {
        draw_choices_quarter(0);
    }

    #[test]
    fn every_choice_of_every_editor_draws_1() {
        draw_choices_quarter(1);
    }

    #[test]
    fn every_choice_of_every_editor_draws_2() {
        draw_choices_quarter(2);
    }

    #[test]
    fn every_choice_of_every_editor_draws_3() {
        draw_choices_quarter(3);
    }

    #[test]
    fn the_large_editors_draw() {
        draw_for_test(
            "amp_rig",
            &[("p1_type", 2.0), ("p2_type", 10.0), ("p3_type", 19.0)],
            3,
            |_, _| {},
        );
        draw_for_test("mb_comp", &[("count", 2.0)], 3, |_, _| {});
        draw_for_test("drums", &[], 3, |_, _| {});
        // Every type of band, dynamic or not.
        let mut set = Vec::new();
        let names: Vec<String> = (1..=10)
            .flat_map(|b| ["used", "type", "dyn"].map(|k| format!("b{b}_{k}")))
            .collect();
        let info = audiostrada_dsp::info("eq").unwrap();
        for (i, name) in names.iter().enumerate() {
            if info.params.iter().any(|p| p.id == name) {
                let value = match i % 3 {
                    0 => 1.0,
                    1 => (i / 3) as f32,
                    _ => (i % 2) as f32,
                };
                set.push((name.as_str(), value));
            }
        }
        draw_for_test("eq", &set, 3, |_, _| {});
    }

    #[test]
    fn values_show_with_their_unit() {
        let eq = audiostrada_dsp::info("eq").unwrap();
        let param = |id: &str| eq.params.iter().find(|p| p.id == id).unwrap();
        assert_eq!(format_value(param("b1_freq"), 1500.0), "1.50 kHz");
        assert_eq!(format_value(param("b1_freq"), 50.0), "50.0 Hz");
        assert_eq!(format_value(param("b1_gain"), 3.0), "+3.0 dB");
        assert_eq!(format_value(param("b1_type"), 0.0), "Bell");
        let choice_name = audiostrada_editors::controls::choice_name;
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
    fn next_preset_follows_the_menu() {
        let names = [
            "Init",
            "Keys: Soft",
            "Pad: Warm",
            "Keys: Organ",
            "Bell",
            "Pad: Glass",
        ];
        let list: Vec<_> = names
            .iter()
            .map(|n| crate::presets::preset(n, &[]))
            .collect();
        let order: Vec<&str> = menu_order(&list).into_iter().map(|i| names[i]).collect();
        assert_eq!(
            order,
            [
                "Init",
                "Bell",
                "Keys: Soft",
                "Keys: Organ",
                "Pad: Warm",
                "Pad: Glass"
            ]
        );
    }

    #[test]
    fn every_editor_shows_every_parameter() {
        let mut missing = Vec::new();
        for info in audiostrada_dsp::EFFECTS.iter() {
            // The drum machine shows the settings of one voice at a time.
            if matches!(info.id, "amp_rig" | "eq" | "drums" | "mb_comp") {
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
            // The synth shows its controls over its views, with the expert's.
            ctx.data_mut(|d| d.insert_temp(crate::synth_editor::View::expert_id(), true));
            let mut state = EditorState::default();
            // The electric pianos show every setting in their advanced view.
            state.effects.keys.advanced = true;
            SHOWN.with_borrow_mut(Vec::clear);
            for frame in 0..12usize {
                ctx.data_mut(|d| d.insert_temp(crate::synth_editor::View::id(), frame / 2));
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
                ("reverb", "predelay_note"),
                ("chorus", "note"),
                ("flanger", "note"),
                ("phaser", "note"),
                // Set by dragging the modules; from the console.
                ("channel", "order"),
                ("channel", "character"),
                // The cards of the drum chosen.
                ("trigger", "snare"),
            ];
            for p in info.params {
                // A chain's stages show in their own editors, opened from its knobs.
                if matches!(info.id, "mastering" | "vocals") && p.id.contains('.') {
                    continue;
                }
                // The synth's routings show once used, its LFO notes once synced.
                // The modulator shows what its mode uses: here the synced tremolo.
                let modal = info.id == "modulator"
                    && (matches!(p.id, "rate" | "steps" | "gate" | "freq")
                        || p.id
                            .strip_prefix('s')
                            .is_some_and(|n| n.parse::<u8>().is_ok()));
                let routed = info.id == "synth"
                    && (p.id.starts_with("mod")
                        || p.id.ends_with("_note")
                        || p.id.contains("_table")
                        || p.id.ends_with("_position"));
                if !shown.iter().any(|s| s == p.id)
                    && !conditional.contains(&(info.id, p.id))
                    && !routed
                    && !modal
                {
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
                audiostrada_editors::eq_editor::show(ui, &mut params, &[], &mut state.eq);
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

    #[test]
    fn eq_window_keeps_its_width_as_bands_come_and_go() {
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
        let values = |doc: &Document| -> Vec<f32> {
            info.params
                .iter()
                .map(|q| {
                    doc.project().tracks[0].inserts[0]
                        .params
                        .get(q.id)
                        .copied()
                        .unwrap_or(q.default)
                })
                .collect()
        };
        for n in 0..60 {
            let values = values(&doc);
            let mut params = Params {
                doc: &mut doc,
                channel,
                insert,
                info,
                values,
            };
            match n {
                10 => {
                    audiostrada_editors::eq_editor::add_band(&mut params, &mut state.eq, 600.0, 3.0)
                }
                20 => audiostrada_editors::eq_editor::add_band(
                    &mut params,
                    &mut state.eq,
                    4000.0,
                    -3.0,
                ),
                30 => state.eq.selected = 0,
                40 => audiostrada_editors::eq_editor::remove_band(&mut params, &mut state.eq, 0),
                _ => {}
            }
            // As in the editor's window, 1040 wide: what the content needs past its room
            // widens the window.
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1040.0, 800.0))),
                ..Default::default()
            };
            let mut out = (egui::Vec2::ZERO, egui::Vec2::ZERO);
            ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let o = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            audiostrada_editors::eq_editor::show(
                                ui,
                                &mut params,
                                &[],
                                &mut state.eq,
                            )
                        });
                    out = (o.content_size, o.inner_rect.size());
                });
            })
            .drop_without_applying_deltas();
            assert!(
                out.0.x <= out.1.x + 1.0,
                "frame {n}: {} wide in {}",
                out.0.x,
                out.1.x
            );
        }
    }
}
