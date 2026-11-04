//! The built-in synth's editor: ready-made sounds, the two oscillators with their sub and
//! noise, then the filter, the voice and the output, the envelope, and a keyboard played with
//! the mouse or the computer keyboard.

use std::collections::BTreeSet;

use audiostrada_core::routing::Channel;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::effect_editors::{
    Params, choice_box, from_unit, pill_switch, section_frame, segmented, to_unit, wide_knob,
};
use crate::i18n::tr;
use crate::theme;

/// Octaves shown on the keyboard.
const OCTAVES: i32 = 3;
const KEYBOARD_HEIGHT: f32 = 84.0;

/// What the synth's editor remembers between frames.
#[derive(Debug)]
pub struct SynthState {
    /// MIDI note of the keyboard's first C.
    low: i32,
    /// The key held with the mouse.
    mouse: Option<u8>,
    /// Keys held on the computer keyboard.
    computer: BTreeSet<u8>,
    /// The computer keyboard plays notes.
    typing: bool,
    /// Where the notes went, to release them there.
    track: Option<audiostrada_core::TrackId>,
    /// The modulation source being dragged onto a knob.
    dragging: Option<usize>,
    /// Knobs a source can be dropped on this frame: where they are and their target.
    targets: Vec<(Rect, usize)>,
}

impl Default for SynthState {
    fn default() -> Self {
        Self {
            low: 48,
            mouse: None,
            computer: BTreeSet::new(),
            typing: false,
            track: None,
            dragging: None,
            targets: Vec::new(),
        }
    }
}

impl SynthState {
    /// Releases every held note (the window closes or loses the focus).
    pub fn release(&mut self, doc: &crate::document::Document) {
        let Some(track) = self.track else {
            return;
        };
        for key in self
            .mouse
            .take()
            .into_iter()
            .chain(std::mem::take(&mut self.computer))
        {
            doc.player().play_key(track, [0x80, key, 0]);
        }
    }
}

/// A key of the computer keyboard: a note from the keyboard's first C, or an octave change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ComputerKey {
    Note(i32),
    OctaveDown,
    OctaveUp,
}

/// The piano layout of DAWs on a QWERTY keyboard (by position, so AZERTY works the same):
/// the home row plays white keys from C, the row above the black keys, Z and X move an octave.
pub(crate) fn computer_key(key: egui::Key) -> Option<ComputerKey> {
    use egui::Key::*;
    let note = match key {
        A => 0,
        W => 1,
        S => 2,
        E => 3,
        D => 4,
        F => 5,
        T => 6,
        G => 7,
        Y => 8,
        H => 9,
        U => 10,
        J => 11,
        K => 12,
        O => 13,
        L => 14,
        P => 15,
        Z => return Some(ComputerKey::OctaveDown),
        X => return Some(ComputerKey::OctaveUp),
        _ => return None,
    };
    Some(ComputerKey::Note(note))
}

/// Where the editor of a viewport asks for the piano keys, and gets them.
pub(crate) fn claim_id(viewport: egui::ViewportId) -> egui::Id {
    egui::Id::unique(("computer-piano", viewport))
}

pub(crate) fn keys_id(viewport: egui::ViewportId) -> egui::Id {
    egui::Id::unique(("computer-piano-keys", viewport))
}

/// Ready-made sounds, by role; settings not listed keep their defaults (one saw with two
/// unison voices, the filter at 2.5 kHz).
pub(crate) fn factory() -> Vec<crate::presets::Preset> {
    use crate::presets::preset;
    let adsr = |a: f32, d: f32, s: f32, r: f32| {
        [("attack", a), ("decay", d), ("sustain", s), ("release", r)]
    };
    let filter = |cutoff: f32, resonance: f32, env: f32| {
        [
            ("cutoff", cutoff),
            ("resonance", resonance),
            ("env_amount", env),
        ]
    };
    let sound =
        |name: &str, shape: &[(&str, f32)], envelope: [(&str, f32); 4], tone: [(&str, f32); 3]| {
            let all: Vec<(&str, f32)> = shape.iter().copied().chain(envelope).chain(tone).collect();
            preset(name, &all)
        };
    // Waves: 0 saw, 1 square, 2 sine, 3 triangle. Modes: 0 poly, 1 mono, 2 legato.
    vec![
        preset(tr("Init"), &[]),
        sound(
            tr("Keys: Soft"),
            &[
                ("key_track", 40.0),
                ("wave", 3.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 40.0),
                ("osc2_octave", 1.0),
                ("drift", 15.0),
                ("velocity", 80.0),
            ],
            adsr(3.0, 900.0, 35.0, 500.0),
            filter(3500.0, 10.0, 35.0),
        ),
        sound(
            tr("Keys: Electric"),
            &[
                ("lfo1_rate", 4.5),
                ("lfo1_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 11.0),
                ("mod1_amount", 15.0),
                ("wave", 2.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 3.0),
                ("osc2_level", 30.0),
                ("osc2_octave", 1.0),
                ("velocity", 90.0),
                ("volume", -14.0),
            ],
            adsr(1.0, 1500.0, 20.0, 600.0),
            filter(6000.0, 0.0, 30.0),
        ),
        sound(
            tr("Pad: Warm"),
            &[
                ("lfo1_rate", 0.2),
                ("lfo1_shape", 1.0),
                ("lfo1_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 8.0),
                ("mod1_amount", 15.0),
                ("mod2_source", 7.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 40.0),
                ("osc1_voices", 4.0),
                ("detune", 15.0),
                ("osc2_level", 60.0),
                ("osc2_octave", -1.0),
                ("osc2_voices", 3.0),
                ("osc2_detune", 12.0),
                ("spread", 80.0),
                ("drift", 30.0),
                ("velocity", 30.0),
                ("volume", -16.0),
            ],
            adsr(600.0, 1500.0, 80.0, 2500.0),
            filter(1200.0, 15.0, 20.0),
        ),
        sound(
            tr("Pad: Glass"),
            &[
                ("lfo1_rate", 0.15),
                ("lfo1_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 5.0),
                ("mod1_amount", 40.0),
                ("mod2_source", 7.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 40.0),
                ("wave", 3.0),
                ("osc1_voices", 3.0),
                ("detune", 10.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 50.0),
                ("osc2_octave", 1.0),
                ("spread", 90.0),
                ("drift", 20.0),
                ("velocity", 30.0),
                ("volume", -17.0),
            ],
            adsr(900.0, 2000.0, 85.0, 3500.0),
            filter(5000.0, 25.0, 15.0),
        ),
        sound(
            tr("Pad: Airy"),
            &[
                ("osc1_voices", 5.0),
                ("detune", 18.0),
                ("osc2_wave", 3.0),
                ("osc2_level", 50.0),
                ("osc2_octave", 1.0),
                ("filter_type", 3.0),
                ("spread", 100.0),
                ("drift", 25.0),
                ("velocity", 30.0),
                ("volume", -8.0),
            ],
            adsr(1200.0, 2000.0, 85.0, 4000.0),
            filter(700.0, 20.0, 0.0),
        ),
        sound(
            tr("Strings: Ensemble"),
            &[
                ("lfo1_rate", 5.0),
                ("lfo1_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 1.0),
                ("mod1_amount", 1.0),
                ("mod2_source", 5.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 35.0),
                ("filter_type", 1.0),
                ("key_track", 30.0),
                ("osc1_voices", 7.0),
                ("detune", 22.0),
                ("spread", 90.0),
                ("drift", 25.0),
                ("velocity", 40.0),
                ("volume", -15.0),
            ],
            adsr(350.0, 800.0, 85.0, 1200.0),
            filter(2800.0, 5.0, 10.0),
        ),
        sound(
            tr("Brass: Analog"),
            &[
                ("lfo1_rate", 5.5),
                ("env2_attack", 1.0),
                ("env2_decay", 350.0),
                ("mod1_source", 5.0),
                ("mod1_target", 1.0),
                ("mod1_amount", 2.0),
                ("mod2_source", 3.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 20.0),
                ("key_track", 50.0),
                ("detune", 10.0),
                ("osc2_level", 70.0),
                ("osc2_fine", 7.0),
                ("drift", 20.0),
                ("volume", -13.0),
            ],
            adsr(60.0, 500.0, 70.0, 250.0),
            filter(900.0, 20.0, 70.0),
        ),
        sound(
            tr("Lead: Supersaw"),
            &[
                ("mod1_source", 5.0),
                ("mod1_target", 8.0),
                ("mod1_amount", 40.0),
                ("mod2_source", 7.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 40.0),
                ("filter_type", 1.0),
                ("key_track", 30.0),
                ("osc1_voices", 9.0),
                ("detune", 28.0),
                ("osc2_level", 40.0),
                ("osc2_octave", 1.0),
                ("osc2_voices", 5.0),
                ("osc2_detune", 18.0),
                ("spread", 85.0),
                ("volume", -14.0),
            ],
            adsr(5.0, 400.0, 80.0, 300.0),
            filter(6000.0, 10.0, 25.0),
        ),
        sound(
            tr("Lead: Mono glide"),
            &[
                ("lfo1_rate", 5.5),
                ("mod1_source", 5.0),
                ("mod1_target", 1.0),
                ("mod1_amount", 3.0),
                ("mod2_source", 4.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 20.0),
                ("filter_type", 2.0),
                ("drive", 20.0),
                ("key_track", 50.0),
                ("mode", 1.0),
                ("glide", 120.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 1.0),
                ("osc2_semi", 7.0),
                ("osc2_level", 50.0),
                ("sub_level", 30.0),
                ("volume", -5.0),
            ],
            adsr(5.0, 400.0, 80.0, 200.0),
            filter(3000.0, 30.0, 40.0),
        ),
        sound(
            tr("Lead: Chip"),
            &[
                ("wave", 1.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 1.0),
                ("osc2_level", 25.0),
                ("osc2_octave", 1.0),
                ("drift", 0.0),
                ("volume", -16.0),
            ],
            adsr(1.0, 200.0, 70.0, 80.0),
            filter(18000.0, 0.0, 0.0),
        ),
        sound(
            tr("Pluck: Bright"),
            &[
                ("mod1_source", 4.0),
                ("mod1_target", 8.0),
                ("mod1_amount", 20.0),
                ("key_track", 60.0),
                ("osc1_voices", 3.0),
                ("detune", 12.0),
                ("velocity", 80.0),
                ("volume", -11.0),
            ],
            adsr(1.0, 250.0, 0.0, 250.0),
            filter(600.0, 35.0, 75.0),
        ),
        sound(
            tr("Bell"),
            &[
                ("wave", 2.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 35.0),
                ("osc2_octave", 1.0),
                ("osc2_semi", 7.0),
                ("drift", 0.0),
                ("velocity", 80.0),
                ("volume", -14.0),
            ],
            adsr(1.0, 2500.0, 0.0, 2500.0),
            filter(18000.0, 0.0, 0.0),
        ),
        sound(
            tr("Pad: Evolving"),
            &[
                ("osc1_voices", 4.0),
                ("detune", 14.0),
                ("osc2_wave", 3.0),
                ("osc2_level", 70.0),
                ("osc2_octave", 1.0),
                ("osc2_voices", 2.0),
                ("filter_type", 1.0),
                ("spread", 90.0),
                ("drift", 25.0),
                ("lfo1_shape", 1.0),
                ("lfo1_rate", 0.08),
                ("lfo1_retrig", 0.0),
                ("lfo2_rate", 0.13),
                ("lfo2_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 8.0),
                ("mod1_amount", 30.0),
                ("mod2_source", 2.0),
                ("mod2_target", 5.0),
                ("mod2_amount", 60.0),
                ("mod3_source", 7.0),
                ("mod3_target", 9.0),
                ("mod3_amount", 50.0),
                ("velocity", 30.0),
                ("volume", -16.0),
            ],
            adsr(1500.0, 3000.0, 80.0, 4000.0),
            filter(900.0, 30.0, 10.0),
        ),
        sound(
            tr("Bass: Wobble"),
            &[
                ("osc1_voices", 2.0),
                ("detune", 12.0),
                ("osc2_wave", 1.0),
                ("osc2_level", 60.0),
                ("osc2_octave", -1.0),
                ("sub_level", 40.0),
                ("filter_type", 2.0),
                ("drive", 35.0),
                ("mode", 1.0),
                ("lfo1_sync", 1.0),
                ("lfo1_note", 5.0),
                ("lfo1_shape", 1.0),
                ("mod1_source", 1.0),
                ("mod1_target", 8.0),
                ("mod1_amount", 45.0),
                ("volume", -8.0),
            ],
            adsr(2.0, 300.0, 100.0, 120.0),
            filter(400.0, 55.0, 0.0),
        ),
        sound(
            tr("Bass: Sub"),
            &[
                ("wave", 2.0),
                ("osc1_voices", 1.0),
                ("sub_level", 60.0),
                ("mode", 1.0),
                ("drift", 5.0),
                ("velocity", 40.0),
                ("volume", -10.0),
            ],
            adsr(2.0, 300.0, 90.0, 120.0),
            filter(800.0, 0.0, 0.0),
        ),
        sound(
            tr("Bass: Reese"),
            &[
                ("detune", 25.0),
                ("osc2_level", 80.0),
                ("osc2_voices", 2.0),
                ("osc2_detune", 18.0),
                ("osc2_fine", -10.0),
                ("sub_level", 50.0),
                ("mode", 1.0),
                ("spread", 20.0),
                ("volume", -13.0),
            ],
            adsr(5.0, 300.0, 100.0, 150.0),
            filter(900.0, 15.0, 20.0),
        ),
        sound(
            tr("Bass: Acid"),
            &[
                ("mod1_source", 4.0),
                ("mod1_target", 8.0),
                ("mod1_amount", 25.0),
                ("filter_type", 2.0),
                ("drive", 40.0),
                ("osc1_voices", 1.0),
                ("mode", 2.0),
                ("glide", 90.0),
                ("drift", 0.0),
                ("volume", -5.0),
            ],
            adsr(1.0, 180.0, 20.0, 80.0),
            filter(300.0, 85.0, 80.0),
        ),
        sound(
            tr("Bass: Analog"),
            &[
                ("filter_type", 2.0),
                ("drive", 25.0),
                ("wave", 1.0),
                ("osc1_voices", 1.0),
                ("osc2_level", 60.0),
                ("osc2_octave", -1.0),
                ("mode", 1.0),
                ("drift", 15.0),
                ("volume", -7.0),
            ],
            adsr(2.0, 400.0, 50.0, 150.0),
            filter(700.0, 25.0, 55.0),
        ),
        sound(
            tr("FX: Noise sweep"),
            &[
                ("filter_type", 4.0),
                ("osc1_level", 0.0),
                ("noise", 100.0),
                ("spread", 100.0),
                ("volume", 2.0),
            ],
            adsr(1500.0, 2000.0, 40.0, 2500.0),
            filter(500.0, 70.0, 100.0),
        ),
    ]
}

/// Color of each modulation source, as on its pill, its ring and its routing.
fn source_color(source: usize) -> Color32 {
    match source {
        1 => Color32::from_rgb(70, 200, 190),
        2 => Color32::from_rgb(170, 120, 240),
        3 => Color32::from_rgb(240, 150, 70),
        4 => Color32::from_rgb(110, 200, 110),
        5 => Color32::from_rgb(90, 160, 250),
        6 => Color32::from_rgb(240, 110, 170),
        _ => Color32::from_rgb(235, 200, 80),
    }
}

/// The modulation target a knob stands for.
fn target_of(id: &str) -> Option<usize> {
    use audiostrada_dsp::synth::target;
    Some(match id {
        "osc1_fine" => target::OSC1_PITCH,
        "osc2_fine" => target::OSC2_PITCH,
        "osc1_level" => target::OSC1_LEVEL,
        "osc2_level" => target::OSC2_LEVEL,
        "sub_level" => target::SUB,
        "noise" => target::NOISE,
        "cutoff" => target::CUTOFF,
        "resonance" => target::RESONANCE,
        "drive" => target::DRIVE,
        "volume" => target::VOLUME,
        _ => return None,
    })
}

/// The routings in use: slot, source, target, amount (-1 to 1).
fn routes(params: &Params) -> Vec<(usize, usize, usize, f32)> {
    (0..audiostrada_dsp::synth::SLOTS)
        .filter_map(|k| {
            let source = params.get(&format!("mod{}_source", k + 1)).round() as usize;
            let target = params.get(&format!("mod{}_target", k + 1)).round() as usize;
            let amount = params.get(&format!("mod{}_amount", k + 1)) / 100.0;
            (source > 0 && target > 0).then_some((k, source, target, amount))
        })
        .collect()
}

/// A knob that takes modulation: dropped sources land on it, its routings show as colored arcs
/// around it.
fn mod_knob(ui: &mut Ui, params: &mut Params, state: &mut SynthState, id: &str, name: &str) {
    let accent = theme::current(ui).accent;
    let size = 38.0;
    let rect = ui
        .scope(|ui| wide_knob(ui, params, id, name, size, accent))
        .response
        .rect;
    let Some(target) = target_of(id) else {
        return;
    };
    state.targets.push((rect, target));
    // The knob sits under its name: the arcs go around it.
    let center = pos2(rect.center().x, rect.top() + 17.0 + size / 2.0);
    let value = params.param(id).map_or(0.5, |q| to_unit(q, params.get(id)));
    let sweep = 0.75 * std::f32::consts::TAU;
    let start = std::f32::consts::FRAC_PI_2 + (std::f32::consts::TAU - sweep) / 2.0;
    let mut radius = size / 2.0 + 4.0;
    for (_, source, t, amount) in routes(params) {
        if t != target || amount == 0.0 {
            continue;
        }
        let from = start + sweep * value;
        let to = start + sweep * (value + amount).clamp(0.0, 1.0);
        let steps = 24;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let a = from + (to - from) * i as f32 / steps as f32;
                center + radius * vec2(a.cos(), a.sin())
            })
            .collect();
        ui.painter()
            .add(Shape::line(points, Stroke::new(2.5, source_color(source))));
        radius += 3.5;
    }
    if state.dragging.is_some()
        && rect.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or_default())
    {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            6.0,
            Stroke::new(1.5, source_color(state.dragging.unwrap_or(0))),
            egui::StrokeKind::Outside,
        );
    }
}

/// A source's pill: dragged onto a knob, it routes the source there.
fn source_pill(ui: &mut Ui, state: &mut SynthState, source: usize) {
    let p = theme::current(ui);
    let name = tr(audiostrada_dsp::synth::SOURCES[source]);
    let color = source_color(source);
    let galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), FontId::proportional(11.0), p.text);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 24.0, 20.0), Sense::click_and_drag());
    ui.painter()
        .rect_filled(rect, 10.0, color.gamma_multiply(0.25));
    ui.painter().rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .circle_filled(pos2(rect.left() + 10.0, rect.center().y), 4.0, color);
    ui.painter().galley(
        pos2(rect.left() + 18.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    let response = response
        .on_hover_text(tr("Drag onto a knob to modulate it"))
        .on_hover_cursor(egui::CursorIcon::Grab);
    if response.drag_started() {
        state.dragging = Some(source);
    }
}

/// Where a dragged source is dropped: a new routing to the knob under the pointer.
fn drop_source(ui: &mut Ui, params: &mut Params, state: &mut SynthState) {
    let Some(source) = state.dragging else {
        return;
    };
    let pointer = ui.input(|i| i.pointer.hover_pos());
    if let Some(at) = pointer {
        // The pill follows the pointer.
        let color = source_color(source);
        let layer = egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("mod-drag"));
        let painter = ui.ctx().layer_painter(layer);
        painter.circle_filled(at, 7.0, color);
        painter.text(
            at + vec2(12.0, 0.0),
            Align2::LEFT_CENTER,
            tr(audiostrada_dsp::synth::SOURCES[source]),
            FontId::proportional(11.0),
            color,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if !ui.input(|i| i.pointer.any_released()) {
        return;
    }
    state.dragging = None;
    let Some(at) = pointer else {
        return;
    };
    let Some(&(_, target)) = state.targets.iter().find(|(r, _)| r.contains(at)) else {
        return;
    };
    let existing = routes(params);
    if existing
        .iter()
        .any(|&(_, s, t, _)| s == source && t == target)
    {
        return;
    }
    let free = (0..audiostrada_dsp::synth::SLOTS).find(|k| !existing.iter().any(|r| r.0 == *k));
    let Some(k) = free else {
        params
            .doc
            .notify(tr("Every modulation slot is in use: remove one first").into());
        return;
    };
    let changes = [
        (format!("mod{}_source", k + 1), source as f32),
        (format!("mod{}_target", k + 1), target as f32),
        (format!("mod{}_amount", k + 1), 50.0),
    ];
    let response = ui.response();
    for (id, v) in changes {
        params.set(&id, Some(v), &response);
    }
    params.doc.history_mut().end_gesture();
}

pub fn show(ui: &mut Ui, params: &mut Params, state: &mut SynthState) {
    let p = theme::current(ui);
    state.targets.clear();
    let height = 132.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (title, wave, ids, level) in [
            (
                "Oscillator 1",
                "wave",
                [
                    "osc1_octave",
                    "osc1_semi",
                    "osc1_fine",
                    "osc1_voices",
                    "detune",
                ],
                "osc1_level",
            ),
            (
                "Oscillator 2",
                "osc2_wave",
                [
                    "osc2_octave",
                    "osc2_semi",
                    "osc2_fine",
                    "osc2_voices",
                    "osc2_detune",
                ],
                "osc2_level",
            ),
        ] {
            section_frame(ui, tr(title), p.weak, height, |ui| {
                wave_picker(ui, params, wave);
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let names = [
                        tr("Octave"),
                        tr("Semi"),
                        tr("Fine"),
                        tr("Unison"),
                        tr("Detune"),
                    ];
                    for (id, name) in ids.iter().zip(names) {
                        mod_knob(ui, params, state, id, name);
                    }
                    mod_knob(ui, params, state, level, tr("Level"));
                });
            });
        }
        section_frame(ui, tr("Sub and noise"), p.weak, height, |ui| {
            ui.horizontal(|ui| segmented(ui, params, "sub_octave", &[]));
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                mod_knob(ui, params, state, "sub_level", tr("Sub"));
                mod_knob(ui, params, state, "noise", tr("Noise"));
            });
        });
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("Filter"), p.weak, height, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    choice_box(ui, params, "filter_type");
                    filter_curve(ui, params, vec2(150.0, 70.0));
                });
                ui.spacing_mut().item_spacing.x = 2.0;
                for (id, name) in [
                    ("cutoff", tr("Cutoff")),
                    ("resonance", tr("Resonance")),
                    ("env_amount", tr("Envelope")),
                    ("drive", tr("Drive")),
                    ("key_track", tr("Key track")),
                ] {
                    mod_knob(ui, params, state, id, name);
                }
            });
        });
        section_frame(ui, tr("Voice"), p.weak, height, |ui| {
            ui.horizontal(|ui| {
                segmented(
                    ui,
                    params,
                    "mode",
                    &[
                        "Chords: every note plays",
                        "One note at a time, each with its attack",
                        "One note at a time, a new note slides on without a new attack",
                    ],
                )
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                mod_knob(ui, params, state, "glide", tr("Glide"));
                mod_knob(ui, params, state, "spread", tr("Stereo"));
                mod_knob(ui, params, state, "drift", tr("Drift"));
            });
        });
        section_frame(ui, tr("Output"), p.weak, height, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                mod_knob(ui, params, state, "volume", tr("Volume"));
                mod_knob(ui, params, state, "velocity", tr("Velocity"));
                mod_knob(ui, params, state, "bend_range", tr("Bend"));
            });
            params.toggle(ui, "mpe", |ui, on| {
                let response = pill_switch(ui, *on, tr("MPE")).on_hover_text(tr(
                    "For MPE controllers (Roli Seaboard, LinnStrument, Osmose...), which send each \
                     note on its own MIDI channel: bend, pressure and slide then shape every note \
                     on its own. Leave it off for an ordinary keyboard.",
                ));
                if response.clicked() {
                    *on = !*on;
                }
                response
            });
        });
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("Envelope"), p.weak, 100.0, |ui| {
            ui.horizontal_top(|ui| {
                envelope(ui, params, 150.0);
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.x = 2.0;
                for (id, name) in [
                    ("attack", tr("Attack")),
                    ("decay", tr("Decay")),
                    ("sustain", tr("Sustain")),
                    ("release", tr("Release")),
                ] {
                    mod_knob(ui, params, state, id, name);
                }
            });
        });
        modulation_sources(ui, params, state);
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        macros_and_playing(ui, params, state);
        routings(ui, params);
    });
    ui.add_space(6.0);
    keyboard(ui, params, state);
    drop_source(ui, params, state);
}

/// The modulation sources, each with its few settings and its pill to drag.
fn modulation_sources(ui: &mut Ui, params: &mut Params, state: &mut SynthState) {
    let p = theme::current(ui);
    let accent = p.accent;
    let small = |ui: &mut Ui, params: &mut Params, id: &str, name: &str| {
        wide_knob(ui, params, id, name, 30.0, accent);
    };
    for l in 1..=2usize {
        section_frame(
            ui,
            tr(if l == 1 { "LFO 1" } else { "LFO 2" }),
            source_color(l),
            100.0,
            |ui| {
                ui.horizontal(|ui| {
                    source_pill(ui, state, l);
                    shape_picker(ui, params, &format!("lfo{l}_shape"));
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let synced = params.get(&format!("lfo{l}_sync")) >= 0.5;
                    if synced {
                        crate::effect_editors::choice_box(ui, params, &format!("lfo{l}_note"));
                    } else {
                        small(ui, params, &format!("lfo{l}_rate"), tr("Rate"));
                    }
                    ui.vertical(|ui| {
                        for (field, label, hint) in [
                            ("sync", "Sync", "A note value at the project's tempo"),
                            (
                                "retrig",
                                "Retrigger",
                                "Starts again at each note; off, every note follows the same wave",
                            ),
                        ] {
                            params.toggle(ui, &format!("lfo{l}_{field}"), |ui, on| {
                                let response =
                                    pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
                                if response.clicked() {
                                    *on = !*on;
                                }
                                response
                            });
                        }
                    });
                });
            },
        );
    }
    section_frame(ui, tr("Envelope 2"), source_color(3), 100.0, |ui| {
        source_pill(ui, state, 3);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (field, name) in [
                ("attack", tr("A")),
                ("decay", tr("D")),
                ("sustain", tr("S")),
                ("release", tr("R")),
            ] {
                small(ui, params, &format!("env2_{field}"), name);
            }
        });
    });
}

/// The four macros, knobs to turn or automate, and what is played: velocity, wheel, pressure.
fn macros_and_playing(ui: &mut Ui, params: &mut Params, state: &mut SynthState) {
    let p = theme::current(ui);
    let accent = p.accent;
    section_frame(ui, tr("Macros"), source_color(7), 96.0, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for m in 1..=4usize {
                ui.vertical(|ui| {
                    source_pill(ui, state, m + 6);
                    wide_knob(ui, params, &format!("macro{m}"), "", 30.0, accent);
                });
            }
        });
    });
    section_frame(ui, tr("Playing"), p.weak, 96.0, |ui| {
        for source in 4..=6 {
            source_pill(ui, state, source);
        }
    });
}

/// The routings in use, one line each: source, target, amount, removal.
fn routings(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    let list = routes(params);
    section_frame(ui, tr("Modulation"), p.weak, 0.0, |ui| {
        if list.is_empty() {
            ui.weak(tr(
                "Drag a colored source (LFO, envelope, macro, velocity...) onto a knob to \
                 modulate it",
            ));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 4.0);
            for (k, source, _, _) in list {
                let n = k + 1;
                egui::Frame::new()
                    .fill(source_color(source).gamma_multiply(0.12))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(6, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.painter().circle_filled(
                                ui.cursor().min + vec2(5.0, 10.0),
                                4.0,
                                source_color(source),
                            );
                            ui.add_space(12.0);
                            crate::effect_editors::choice_box(
                                ui,
                                params,
                                &format!("mod{n}_source"),
                            );
                            ui.label("→");
                            crate::effect_editors::choice_box(
                                ui,
                                params,
                                &format!("mod{n}_target"),
                            );
                            let id = format!("mod{n}_amount");
                            let mut amount = params.get(&id);
                            let response = ui.add(
                                egui::DragValue::new(&mut amount)
                                    .range(-100.0..=100.0)
                                    .suffix(" %")
                                    .speed(0.5),
                            );
                            params.set(&id, response.changed().then_some(amount), &response);
                            if ui.small_button("✕").on_hover_text(tr("Remove")).clicked() {
                                let response = ui.response();
                                params.set(&format!("mod{n}_source"), Some(0.0), &response);
                                params.set(&format!("mod{n}_amount"), Some(0.0), &response);
                                params.doc.history_mut().end_gesture();
                            }
                        });
                    });
            }
        });
    });
}

/// LFO shapes as small buttons drawing them.
fn shape_picker(ui: &mut Ui, params: &mut Params, id: &str) {
    use audiostrada_dsp::filter::{SHAPES, lfo_value};
    let p = theme::current(ui);
    let current = params.get(id).round() as usize;
    ui.spacing_mut().item_spacing.x = 2.0;
    for (i, name) in SHAPES.iter().enumerate() {
        let (rect, response) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::click());
        let on = i == current;
        ui.painter().rect_filled(
            rect,
            4.0,
            if on {
                p.accent.gamma_multiply(0.3)
            } else {
                p.surface
            },
        );
        let inner = rect.shrink2(vec2(4.0, 3.0));
        let held = [0.6, -0.3, 0.9, -0.8];
        let points: Vec<Pos2> = (0..=24)
            .map(|j| {
                let t = j as f32 / 24.0;
                let v = lfo_value(i, t, held[((t * 4.0) as usize).min(3)]);
                pos2(
                    inner.left() + t * inner.width(),
                    inner.center().y - v * inner.height() / 2.0,
                )
            })
            .collect();
        ui.painter().add(Shape::line(
            points,
            Stroke::new(1.0, if on { p.text } else { p.weak }),
        ));
        let response = response.on_hover_text(tr(name));
        let clicked = response.clicked();
        params.set(id, clicked.then_some(i as f32), &response);
        if clicked {
            params.doc.history_mut().end_gesture();
        }
    }
}

/// What the filter does to the spectrum, 20 Hz to 20 kHz, at its cutoff and resonance.
fn filter_curve(ui: &mut Ui, params: &mut Params, size: egui::Vec2) {
    use audiostrada_dsp::synth::{BP, HP, LADDER, LP12, LP24};
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, p.base);
    let kind = params.get("filter_type").round() as usize;
    let fc = params.get("cutoff");
    let r = params.get("resonance") / 95.0;
    let k = 2.0 * (1.0 - r * 0.95).max(0.05);
    // Analog prototypes of the filters, in the s plane, at w = f / fc.
    let gain = |f: f32| -> f32 {
        let w = f / fc;
        let svf = |num: f32| {
            let den = ((1.0 - w * w).powi(2) + (k * w).powi(2)).sqrt();
            num / den.max(1e-6)
        };
        match kind {
            LP12 => svf(1.0),
            LP24 => {
                let flat = 1.0 / ((1.0 - w * w).powi(2) + 2.0 * w * w).sqrt().max(1e-6);
                svf(1.0) * flat
            }
            LADDER => {
                // 1 / ((1 + s)^4 + 4.1 r), s = jw: the four poles and their feedback.
                let (re, im) = (1.0f32, w);
                let (r2, i2) = (re * re - im * im, 2.0 * re * im);
                let (r4, i4) = (r2 * r2 - i2 * i2, 2.0 * r2 * i2);
                (1.0 + 0.8 * 4.1 * r) / ((r4 + 4.1 * r).powi(2) + i4 * i4).sqrt().max(1e-6)
            }
            HP => svf(w * w),
            BP => svf(k * w),
            _ => svf((1.0 - w * w).abs()),
        }
    };
    let to_x = |f: f32| rect.left() + (f / 20.0).log10() / 3.0 * rect.width();
    let to_y = |db: f32| rect.center().y - 6.0 - (db / 24.0).clamp(-1.6, 1.0) * rect.height() * 0.4;
    let points: Vec<Pos2> = (0..=80)
        .map(|i| {
            let f = 20.0 * 1000f32.powf(i as f32 / 80.0);
            pos2(to_x(f), to_y(20.0 * gain(f).max(1e-6).log10()))
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.6, p.accent)));
    painter.circle_filled(
        pos2(to_x(fc), to_y(20.0 * gain(fc).max(1e-6).log10())),
        3.0,
        theme::SOLO,
    );
}

/// The four waves as buttons drawing their shape, the chosen one lit.
fn wave_picker(ui: &mut Ui, params: &mut Params, id: &str) {
    let p = theme::current(ui);
    let current = params.get(id).round() as usize;
    let hints = [
        "Saw: bright and buzzy, for brass, strings and leads",
        "Square: hollow, for clarinets and retro games",
        "Sine: pure, for sub basses and soft flutes",
        "Triangle: soft and round, for mellow keys",
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, hint) in hints.iter().enumerate() {
            let (rect, response) = ui.allocate_exact_size(vec2(34.0, 22.0), Sense::click());
            let on = i == current;
            let painter = ui.painter();
            painter.rect_filled(
                rect,
                5.0,
                if on {
                    p.accent.gamma_multiply(0.3)
                } else {
                    p.surface
                },
            );
            painter.rect_stroke(
                rect,
                5.0,
                Stroke::new(1.0, if on { p.accent } else { p.border }),
                egui::StrokeKind::Inside,
            );
            let inner = rect.shrink2(vec2(7.0, 5.0));
            let points: Vec<Pos2> = (0..=40)
                .map(|j| {
                    let t = j as f32 / 40.0;
                    let phase = (t * 2.0).fract();
                    let v = match i {
                        0 => 2.0 * phase - 1.0,
                        1 => {
                            if phase < 0.5 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                        2 => (phase * std::f32::consts::TAU).sin(),
                        _ => 1.0 - 4.0 * (phase - 0.5).abs(),
                    };
                    pos2(
                        inner.left() + t * inner.width(),
                        inner.center().y - v * inner.height() / 2.0,
                    )
                })
                .collect();
            painter.add(Shape::line(
                points,
                Stroke::new(1.3, if on { p.text } else { p.weak }),
            ));
            let response = response.on_hover_text(tr(hint));
            let clicked = response.clicked();
            params.set(id, clicked.then_some(i as f32), &response);
            if clicked {
                params.doc.history_mut().end_gesture();
            }
        }
    });
}

/// The envelope, its corners dragged to set attack, decay with sustain, and release.
fn envelope(ui: &mut Ui, params: &mut Params, width: f32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 84.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    let area = rect.shrink2(vec2(14.0, 10.0));
    // Attack, decay and release each get a quarter, sustain holds the third.
    let quarter = area.width() / 4.0;
    let unit =
        |params: &Params, id: &str| params.param(id).map_or(0.5, |q| to_unit(q, params.get(id)));
    let sustain = params.get("sustain") / 100.0;
    let y = |level: f32| area.bottom() - level * area.height();
    let attack = pos2(area.left() + quarter * unit(params, "attack"), y(1.0));
    let decay = pos2(attack.x + quarter * unit(params, "decay"), y(sustain));
    let hold = pos2(decay.x + quarter, y(sustain));
    let release = pos2(hold.x + quarter * unit(params, "release"), y(0.0));
    let points = vec![pos2(area.left(), y(0.0)), attack, decay, hold, release];
    let mut fill = points.clone();
    fill.push(pos2(area.left(), y(0.0)));
    painter.add(Shape::convex_polygon(
        fill,
        p.accent.gamma_multiply(0.12),
        Stroke::NONE,
    ));
    painter.add(Shape::line(points, Stroke::new(2.0, p.accent)));
    let handles = [
        ("attack", attack, area.left(), None),
        ("decay", decay, attack.x, Some("sustain")),
        ("release", release, hold.x, None),
    ];
    for (id, at, from, vertical) in handles {
        let response = ui
            .interact(
                Rect::from_center_size(at, vec2(16.0, 16.0)),
                ui.scope_id().with(("envelope", id)),
                Sense::drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Grab);
        let active = response.hovered() || response.dragged();
        painter.circle_filled(at, if active { 5.5 } else { 4.0 }, p.accent);
        let pointer = response
            .dragged()
            .then(|| response.interact_pointer_pos())
            .flatten();
        let changed = pointer.and_then(|pos| {
            let q = params.param(id)?;
            Some(from_unit(q, ((pos.x - from) / quarter).clamp(0.0, 1.0)))
        });
        params.change(id, changed, &response);
        if let Some(level) = vertical {
            let changed = pointer
                .map(|pos| ((area.bottom() - pos.y) / area.height() * 100.0).clamp(0.0, 100.0));
            params.change(level, changed, &response);
        }
    }
    for (point, letter) in [(attack, "A"), (decay, "D"), (hold, "S"), (release, "R")] {
        painter.text(
            pos2(point.x, area.bottom() + 1.0),
            Align2::CENTER_TOP,
            letter,
            FontId::proportional(9.0),
            p.weak,
        );
    }
    ui.add_space(4.0);
}

fn is_black(key: i32) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// Name of a C, as C3.
fn octave_name(key: i32) -> String {
    format!("C{}", key.div_euclid(12) - 1)
}

/// The track whose instrument the keyboard plays.
fn track_of(params: &Params) -> Option<audiostrada_core::TrackId> {
    match params.channel {
        Channel::Track(t) => Some(t),
        Channel::Master => None,
    }
}

fn keyboard(ui: &mut Ui, params: &mut Params, state: &mut SynthState) {
    let p = theme::current(ui);
    let ctx = ui.ctx().clone();
    let viewport = ctx.viewport_id();
    let Some(track) = track_of(params) else {
        return;
    };
    state.track = Some(track);
    let doc = &*params.doc;
    let play = |key: u8, velocity: Option<u8>| {
        let data = match velocity {
            Some(v) => [0x90, key, v.max(1)],
            None => [0x80, key, 0],
        };
        doc.player().play_key(track, data);
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(tr("KEYBOARD"))
                .size(9.5)
                .color(p.weak)
                .strong(),
        );
        if ui
            .small_button("◀")
            .on_hover_text(tr("Octave down (Z)"))
            .clicked()
        {
            state.low = (state.low - 12).max(0);
        }
        ui.label(
            RichText::new(format!(
                "{} – {}",
                octave_name(state.low),
                octave_name(state.low + 12 * OCTAVES)
            ))
            .monospace()
            .size(11.0),
        );
        if ui
            .small_button("▶")
            .on_hover_text(tr("Octave up (X)"))
            .clicked()
        {
            state.low = (state.low + 12).min(96);
        }
        ui.add_space(8.0);
        ui.toggle_value(&mut state.typing, tr("Computer keyboard"))
            .on_hover_text(tr(
                "Play with the letters: A S D F G H J K for the white keys, W E T Y U for the \
                 black ones, Z and X for the octave",
            ));
    });

    // Computer keys, taken from the window's shortcuts while the toggle is on.
    let focused = ctx.input(|i| i.focused);
    if state.typing && focused {
        ctx.data_mut(|d| d.insert_temp(claim_id(viewport), true));
    } else {
        ctx.data_mut(|d| d.remove::<bool>(claim_id(viewport)));
    }
    if !focused {
        for key in std::mem::take(&mut state.computer) {
            play(key, None);
        }
    }
    let typed: Vec<(egui::Key, bool)> = ctx
        .data_mut(|d| d.remove_temp(keys_id(viewport)))
        .unwrap_or_default();
    for (key, pressed) in typed {
        match computer_key(key) {
            Some(ComputerKey::Note(offset)) => {
                let Ok(note) = u8::try_from(state.low + offset) else {
                    continue;
                };
                if note > 127 {
                    continue;
                }
                if pressed && state.computer.insert(note) {
                    play(note, Some(100));
                } else if !pressed && state.computer.remove(&note) {
                    play(note, None);
                }
            }
            Some(ComputerKey::OctaveDown) if pressed => state.low = (state.low - 12).max(0),
            Some(ComputerKey::OctaveUp) if pressed => state.low = (state.low + 12).min(96),
            _ => {}
        }
    }

    // The keys, and the mouse.
    let whites: Vec<i32> = (state.low..=state.low + 12 * OCTAVES)
        .filter(|k| !is_black(*k))
        .collect();
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(420.0), KEYBOARD_HEIGHT),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    let white_width = rect.width() / whites.len() as f32;
    let black_size = vec2(white_width * 0.6, rect.height() * 0.6);
    let white_rect = |i: usize| {
        Rect::from_min_size(
            pos2(rect.left() + i as f32 * white_width, rect.top()),
            vec2(white_width, rect.height()),
        )
    };
    let black_rect = |i: usize| {
        Rect::from_min_size(
            pos2(
                rect.left() + (i + 1) as f32 * white_width - black_size.x / 2.0,
                rect.top(),
            ),
            black_size,
        )
    };
    let blacks: Vec<(i32, Rect)> = whites
        .iter()
        .enumerate()
        .filter(|(_, k)| is_black(**k + 1) && **k < state.low + 12 * OCTAVES)
        .map(|(i, k)| (k + 1, black_rect(i)))
        .collect();
    let under = |pos: Pos2| -> Option<(i32, f32)> {
        if let Some((key, r)) = blacks.iter().find(|(_, r)| r.contains(pos)) {
            return Some((*key, (pos.y - r.top()) / r.height()));
        }
        let i = ((pos.x - rect.left()) / white_width) as usize;
        whites
            .get(i)
            .map(|k| (*k, (pos.y - rect.top()) / rect.height()))
    };
    let pressed_key = response
        .is_pointer_button_down_on()
        .then(|| response.interact_pointer_pos())
        .flatten()
        .and_then(under);
    let wanted = pressed_key.and_then(|(k, _)| u8::try_from(k).ok());
    if wanted != state.mouse {
        if let Some(old) = state.mouse.take() {
            play(old, None);
        }
        if let (Some(key), Some((_, depth))) = (wanted, pressed_key) {
            // Lower on the key plays louder, as a finger pressing harder.
            let velocity = (30.0 + 97.0 * depth.clamp(0.0, 1.0)) as u8;
            play(key, Some(velocity));
            state.mouse = Some(key);
        }
    }
    let held = |key: i32| {
        u8::try_from(key).is_ok_and(|k| state.mouse == Some(k) || state.computer.contains(&k))
    };
    for (i, key) in whites.iter().enumerate() {
        let r = white_rect(i).shrink2(vec2(0.5, 0.0));
        let fill = if held(*key) {
            p.accent
        } else {
            Color32::from_gray(232)
        };
        painter.rect_filled(
            r,
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 3,
                se: 3,
            },
            fill,
        );
        if key.rem_euclid(12) == 0 {
            painter.text(
                pos2(r.center().x, r.bottom() - 4.0),
                Align2::CENTER_BOTTOM,
                octave_name(*key),
                FontId::proportional(9.0),
                Color32::from_gray(110),
            );
        }
    }
    for (key, r) in &blacks {
        let fill = if held(*key) {
            p.accent
        } else {
            Color32::from_gray(28)
        };
        painter.rect_filled(
            *r,
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 2,
                se: 2,
            },
            fill,
        );
    }
    if state.typing {
        // The letters on the keys they play.
        for (letter, offset) in [
            ("A", 0),
            ("W", 1),
            ("S", 2),
            ("E", 3),
            ("D", 4),
            ("F", 5),
            ("T", 6),
            ("G", 7),
            ("Y", 8),
            ("H", 9),
            ("U", 10),
            ("J", 11),
            ("K", 12),
        ] {
            let key = state.low + offset;
            let (r, color) = match blacks.iter().find(|(k, _)| *k == key) {
                Some((_, r)) => (*r, Color32::from_gray(200)),
                None => match whites.iter().position(|k| *k == key) {
                    Some(i) => (white_rect(i), Color32::from_gray(90)),
                    None => continue,
                },
            };
            painter.text(
                pos2(r.center().x, r.bottom() - 16.0),
                Align2::CENTER_BOTTOM,
                letter,
                FontId::monospace(10.0),
                color,
            );
        }
    }
    if state.mouse.is_some() || !state.computer.is_empty() {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_row_plays_an_octave_from_c() {
        let notes: Vec<_> = [egui::Key::A, egui::Key::W, egui::Key::J, egui::Key::K]
            .into_iter()
            .map(computer_key)
            .collect();
        assert_eq!(
            notes,
            [
                Some(ComputerKey::Note(0)),
                Some(ComputerKey::Note(1)),
                Some(ComputerKey::Note(11)),
                Some(ComputerKey::Note(12)),
            ]
        );
        assert_eq!(computer_key(egui::Key::Z), Some(ComputerKey::OctaveDown));
        assert_eq!(computer_key(egui::Key::X), Some(ComputerKey::OctaveUp));
        assert_eq!(computer_key(egui::Key::Space), None);
    }

    #[test]
    fn black_keys_and_octave_names() {
        let black: Vec<i32> = (60..72).filter(|k| is_black(*k)).collect();
        assert_eq!(black, [61, 63, 66, 68, 70]);
        assert_eq!(octave_name(48), "C3");
        assert_eq!(octave_name(60), "C4");
    }

    /// Loudness of each factory sound playing a chord, so that none jumps out of the list.
    #[test]
    fn factory_sounds_have_matching_levels() {
        use audiostrada_dsp::MidiEvent;
        let info = audiostrada_dsp::info("synth").unwrap();
        let mut levels = Vec::new();
        for preset in factory() {
            let mut synth = audiostrada_dsp::create("synth").unwrap();
            for (i, p) in info.params.iter().enumerate() {
                synth.set_param(i, preset.params.get(p.id).copied().unwrap_or(p.default));
            }
            synth.prepare(48_000, 512);
            // A held A minor chord around the middle of the keyboard, at a medium velocity.
            for key in [57, 60, 64] {
                synth.midi(&MidiEvent::note_on(0, 0, key, 100));
            }
            // The loudest 100 ms over two seconds: plucks are judged on their attack.
            let mut db = f64::MIN;
            let mut window = Vec::new();
            for _ in 0..188 {
                let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
                synth.process(&mut l, &mut r);
                window.extend(l.iter().chain(&r).map(|s| f64::from(s * s)));
                if window.len() >= 9600 {
                    let mean = window.iter().sum::<f64>() / window.len() as f64;
                    db = db.max(10.0 * mean.max(1e-12).log10());
                    window.clear();
                }
            }
            levels.push((preset.name.clone(), db));
        }
        for (name, db) in &levels {
            eprintln!("{name}: {db:.1} dB");
        }
        for (name, db) in &levels {
            assert!((-26.0..=-14.0).contains(db), "{name}: {db:.1} dB RMS");
        }
    }

    #[test]
    fn a_source_dropped_on_a_knob_routes_to_it() {
        use crate::document::{AppDirs, Document};
        use audiostrada_core::routing::{self, Channel};
        use eframe::egui::{Event, Modifiers, PointerButton, RawInput};
        let mut doc = Document::new(AppDirs::default());
        let mut insert = None;
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("A");
            insert = routing::add_insert(p, Channel::Track(t), "synth", Default::default());
        });
        let channel = Channel::Track(doc.project().tracks[0].id);
        let info = audiostrada_dsp::info("synth").unwrap();
        let ctx = egui::Context::default();
        let mut state = SynthState::default();
        let frame = |doc: &mut Document, state: &mut SynthState, events: Vec<Event>| {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 1100.0))),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                let values = info
                    .params
                    .iter()
                    .map(|p| {
                        doc.project().tracks[0].inserts[0]
                            .params
                            .get(p.id)
                            .copied()
                            .unwrap_or(p.default)
                    })
                    .collect();
                let mut params = Params {
                    values,
                    doc: &mut *doc,
                    channel,
                    insert: insert.unwrap(),
                    info,
                };
                show(ui, &mut params, state);
            })
            .drop_without_applying_deltas();
        };
        frame(&mut doc, &mut state, vec![]);
        let cutoff = state
            .targets
            .iter()
            .find(|(_, t)| *t == audiostrada_dsp::synth::target::CUTOFF)
            .map(|(r, _)| r.center())
            .unwrap();
        // LFO 1 picked up, carried over the cutoff knob and let go there.
        state.dragging = Some(1);
        let release = Event::PointerButton {
            pos: cutoff,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        };
        frame(
            &mut doc,
            &mut state,
            vec![Event::PointerMoved(cutoff), release],
        );
        let params = &doc.project().tracks[0].inserts[0].params;
        assert_eq!(params.get("mod1_source"), Some(&1.0));
        assert_eq!(
            params.get("mod1_target"),
            Some(&(audiostrada_dsp::synth::target::CUTOFF as f32))
        );
        assert_eq!(params.get("mod1_amount"), Some(&50.0));
        assert!(state.dragging.is_none());
    }
}
