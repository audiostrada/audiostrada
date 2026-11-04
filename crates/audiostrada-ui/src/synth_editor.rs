//! The built-in synth's editor: ready-made sounds, the two oscillators with their sub and
//! noise, then the filter, the voice and the output, the envelope, and a keyboard played with
//! the mouse or the computer keyboard.

use std::collections::BTreeSet;

use audiostrada_core::routing::Channel;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::effect_editors::{
    Params, choice_box, from_unit, pill_switch, segmented, to_unit, wide_knob,
};
use crate::i18n::tr;
use crate::theme;
use crate::trf;
use audiostrada_dsp::wavetable::Table;

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
    /// What the newest note does, read from the meters.
    live: Live,
    /// The view the window was last sized for.
    sized: Option<bool>,
}

/// What the synth does now, from its meters.
#[derive(Clone, Debug, Default)]
struct Live {
    lfo_phase: [f32; 2],
    /// The newest note's envelope level and stage (1 attack to 4 release), while one plays.
    envelope: Option<(f32, usize)>,
    cutoff: Option<f32>,
    voices: usize,
    mods: Vec<f32>,
}

impl Live {
    fn read(meters: &[f32]) -> Self {
        use audiostrada_dsp::synth::meter;
        let at = |i: usize| meters.get(i).copied().unwrap_or(0.0);
        let voices = at(meter::VOICES).round().max(0.0) as usize;
        let playing = voices > 0;
        Self {
            lfo_phase: [at(meter::LFO_PHASE), at(meter::LFO_PHASE + 1)],
            envelope: playing.then(|| (at(meter::ENV), at(meter::STAGE).round() as usize)),
            cutoff: playing.then(|| at(meter::CUTOFF)),
            voices,
            mods: (meter::MODS..meter::COUNT).map(at).collect(),
        }
    }
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
            live: Live::default(),
            sized: None,
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
    let mut presets = vec![
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
            tr("Pad: Vowels"),
            &[
                ("wave", 4.0),
                ("osc1_table", 3.0),
                ("osc1_voices", 4.0),
                ("detune", 12.0),
                ("osc2_level", 0.0),
                ("spread", 80.0),
                ("drift", 15.0),
                ("lfo1_shape", 1.0),
                ("lfo1_rate", 0.12),
                ("lfo1_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 12.0),
                ("mod1_amount", 50.0),
                ("osc1_position", 50.0),
                ("volume", -11.0),
            ],
            adsr(800.0, 2000.0, 85.0, 2500.0),
            filter(6000.0, 10.0, 0.0),
        ),
        sound(
            tr("Bass: Fold"),
            &[
                ("wave", 4.0),
                ("osc1_table", 6.0),
                ("osc1_voices", 1.0),
                ("osc2_level", 0.0),
                ("sub_level", 50.0),
                ("mode", 1.0),
                ("env2_attack", 1.0),
                ("env2_decay", 350.0),
                ("env2_sustain", 10.0),
                ("mod1_source", 3.0),
                ("mod1_target", 12.0),
                ("mod1_amount", 80.0),
                ("velocity", 40.0),
                ("volume", -12.0),
            ],
            adsr(2.0, 300.0, 90.0, 120.0),
            filter(5000.0, 0.0, 0.0),
        ),
        sound(
            tr("Bell: Metal"),
            &[
                ("wave", 4.0),
                ("osc1_table", 4.0),
                ("osc1_voices", 1.0),
                ("osc1_position", 15.0),
                ("osc2_wave", 2.0),
                ("osc2_octave", 1.0),
                ("osc2_level", 30.0),
                ("mod1_source", 4.0),
                ("mod1_target", 12.0),
                ("mod1_amount", 40.0),
                ("spread", 60.0),
                ("velocity", 60.0),
                ("volume", -9.0),
            ],
            adsr(1.0, 1800.0, 0.0, 1800.0),
            filter(16000.0, 0.0, 0.0),
        ),
        sound(
            tr("Lead: Sync"),
            &[
                ("wave", 4.0),
                ("osc1_table", 5.0),
                ("osc1_voices", 2.0),
                ("detune", 8.0),
                ("osc2_level", 0.0),
                ("mode", 2.0),
                ("glide", 60.0),
                ("mod1_source", 5.0),
                ("mod1_target", 12.0),
                ("mod1_amount", 70.0),
                ("mod2_source", 7.0),
                ("mod2_target", 12.0),
                ("mod2_amount", 30.0),
                ("osc1_position", 20.0),
                ("volume", -6.0),
            ],
            adsr(5.0, 400.0, 85.0, 200.0),
            filter(7000.0, 15.0, 0.0),
        ),
        sound(
            tr("Keys: Organ"),
            &[
                ("wave", 4.0),
                ("osc1_table", 7.0),
                ("osc1_voices", 1.0),
                ("osc1_position", 60.0),
                ("osc2_level", 0.0),
                ("lfo1_rate", 6.0),
                ("lfo1_retrig", 0.0),
                ("mod1_source", 1.0),
                ("mod1_target", 11.0),
                ("mod1_amount", 8.0),
                ("drift", 5.0),
                ("volume", -11.0),
            ],
            adsr(3.0, 100.0, 100.0, 60.0),
            filter(12000.0, 0.0, 0.0),
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
        sound(
            tr("Keys: FM Electric"),
            &[
                ("wave", 2.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 0.0),
                ("fm", 15.0),
                ("env2_attack", 1.0),
                ("env2_decay", 700.0),
                ("env2_sustain", 0.0),
                ("mod1_source", 3.0),
                ("mod1_target", 14.0),
                ("mod1_amount", 45.0),
                ("mod2_source", 4.0),
                ("mod2_target", 14.0),
                ("mod2_amount", 25.0),
                ("drift", 5.0),
                ("velocity", 70.0),
                ("fx_chorus", 30.0),
                ("fx_reverb", 15.0),
                ("volume", -11.5),
            ],
            adsr(2.0, 1800.0, 25.0, 450.0),
            filter(16000.0, 0.0, 0.0),
        ),
        sound(
            tr("Lead: Sync sweep"),
            &[
                ("wave", 0.0),
                ("osc1_voices", 1.0),
                ("osc1_semi", 7.0),
                ("osc2_level", 50.0),
                ("osc2_voices", 1.0),
                ("sync", 1.0),
                ("mode", 2.0),
                ("glide", 50.0),
                ("env2_attack", 1.0),
                ("env2_decay", 450.0),
                ("env2_sustain", 20.0),
                ("mod1_source", 3.0),
                ("mod1_target", 2.0),
                ("mod1_amount", 70.0),
                ("mod2_source", 5.0),
                ("mod2_target", 2.0),
                ("mod2_amount", 40.0),
                ("fx_delay", 20.0),
                ("fx_reverb", 12.0),
                ("volume", -6.0),
            ],
            adsr(3.0, 400.0, 85.0, 200.0),
            filter(6000.0, 20.0, 10.0),
        ),
        sound(
            tr("Bell: Ring"),
            &[
                ("wave", 2.0),
                ("osc1_voices", 1.0),
                ("osc1_level", 30.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 0.0),
                ("osc2_octave", 1.0),
                ("osc2_semi", 3.0),
                ("osc2_fine", 12.0),
                ("ring", 80.0),
                ("velocity", 60.0),
                ("spread", 60.0),
                ("fx_reverb", 30.0),
                ("fx_reverb_decay", 4.0),
                ("volume", -8.0),
            ],
            adsr(1.0, 2200.0, 0.0, 2200.0),
            filter(14000.0, 0.0, 0.0),
        ),
        sound(
            tr("Bass: FM Growl"),
            &[
                ("wave", 0.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 0.0),
                ("osc2_octave", -1.0),
                ("fm", 20.0),
                ("sub_level", 35.0),
                ("mode", 1.0),
                ("lfo1_sync", 1.0),
                ("lfo1_note", 5.0),
                ("lfo1_shape", 1.0),
                ("mod1_source", 1.0),
                ("mod1_target", 14.0),
                ("mod1_amount", 40.0),
                ("mod2_source", 5.0),
                ("mod2_target", 8.0),
                ("mod2_amount", 30.0),
                ("drive", 30.0),
                ("volume", -6.0),
            ],
            adsr(2.0, 300.0, 90.0, 120.0),
            filter(1200.0, 25.0, 20.0),
        ),
        sound(
            tr("Pad: Space"),
            &[
                ("osc1_voices", 5.0),
                ("detune", 16.0),
                ("osc2_wave", 3.0),
                ("osc2_level", 50.0),
                ("osc2_octave", 1.0),
                ("filter_type", 1.0),
                ("spread", 100.0),
                ("drift", 20.0),
                ("fx_chorus", 40.0),
                ("fx_reverb", 45.0),
                ("fx_reverb_decay", 7.0),
                ("fx_delay", 15.0),
                ("fx_delay_note", 8.0),
                ("volume", -6.5),
            ],
            adsr(1800.0, 3000.0, 85.0, 4500.0),
            filter(2200.0, 15.0, 10.0),
        ),
        sound(
            tr("Arp: Bright pluck"),
            &[
                ("arp", 1.0),
                ("arp_octaves", 2.0),
                ("arp_gate", 50.0),
                ("osc1_voices", 3.0),
                ("detune", 10.0),
                ("osc2_wave", 1.0),
                ("osc2_level", 40.0),
                ("osc2_octave", 1.0),
                ("filter_type", 1.0),
                ("velocity", 40.0),
                ("fx_delay", 25.0),
                ("fx_reverb", 15.0),
                ("volume", -6.5),
            ],
            adsr(1.0, 250.0, 0.0, 150.0),
            filter(700.0, 30.0, 70.0),
        ),
        sound(
            tr("Arp: Up and down keys"),
            &[
                ("arp", 1.0),
                ("arp_mode", 2.0),
                ("arp_note", 5.0),
                ("arp_octaves", 2.0),
                ("arp_gate", 80.0),
                ("wave", 3.0),
                ("osc1_voices", 2.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 50.0),
                ("osc2_octave", 1.0),
                ("fx_chorus", 30.0),
                ("fx_reverb", 20.0),
                ("volume", -8.0),
            ],
            adsr(3.0, 600.0, 40.0, 300.0),
            filter(4000.0, 10.0, 20.0),
        ),
        sound(
            tr("Arp: Random bells"),
            &[
                ("arp", 1.0),
                ("arp_mode", 3.0),
                ("arp_note", 5.0),
                ("arp_octaves", 3.0),
                ("arp_gate", 40.0),
                ("arp_hold", 1.0),
                ("wave", 2.0),
                ("osc1_voices", 1.0),
                ("osc2_wave", 2.0),
                ("osc2_level", 0.0),
                ("osc2_octave", 2.0),
                ("fm", 25.0),
                ("env2_attack", 1.0),
                ("env2_decay", 400.0),
                ("env2_sustain", 0.0),
                ("mod1_source", 3.0),
                ("mod1_target", 14.0),
                ("mod1_amount", 40.0),
                ("spread", 70.0),
                ("fx_delay", 30.0),
                ("fx_delay_note", 6.0),
                ("fx_reverb", 30.0),
                ("fx_reverb_decay", 5.0),
                ("volume", -6.0),
            ],
            adsr(1.0, 900.0, 0.0, 900.0),
            filter(14000.0, 0.0, 0.0),
        ),
    ];
    // A touch of space on the sounds made for it.
    // A touch of space on the sounds made for it, and the level the dry part loses back.
    type Space<'a> = (&'a str, f32, &'a [(&'a str, f32)]);
    let space: [Space; 10] = [
        (
            "Keys: Soft",
            1.5,
            &[("fx_chorus", 25.0), ("fx_reverb", 15.0)],
        ),
        (
            "Keys: Electric",
            2.0,
            &[("fx_chorus", 35.0), ("fx_reverb", 12.0)],
        ),
        (
            "Pad: Warm",
            4.0,
            &[("fx_reverb", 25.0), ("fx_reverb_decay", 4.0)],
        ),
        (
            "Pad: Glass",
            2.0,
            &[("fx_reverb", 30.0), ("fx_reverb_decay", 5.0)],
        ),
        (
            "Pad: Airy",
            2.5,
            &[("fx_reverb", 35.0), ("fx_reverb_decay", 6.0)],
        ),
        (
            "Strings: Ensemble",
            4.0,
            &[("fx_chorus", 40.0), ("fx_reverb", 20.0)],
        ),
        (
            "Lead: Supersaw",
            4.0,
            &[("fx_delay", 15.0), ("fx_reverb", 15.0)],
        ),
        ("Lead: Mono glide", 2.0, &[("fx_delay", 20.0)]),
        (
            "Pluck: Bright",
            6.0,
            &[("fx_delay", 25.0), ("fx_reverb", 15.0)],
        ),
        (
            "Bell",
            2.5,
            &[("fx_reverb", 30.0), ("fx_reverb_decay", 4.0)],
        ),
    ];
    let volume = audiostrada_dsp::info("synth")
        .and_then(|i| i.params.iter().find(|p| p.id == "volume"))
        .map_or(-12.0, |p| p.default);
    for (name, louder, extra) in space {
        if let Some(sound) = presets.iter_mut().find(|p| p.name == tr(name)) {
            for &(id, v) in extra {
                sound.params.insert(id.to_owned(), v);
            }
            *sound.params.entry("volume".to_owned()).or_insert(volume) += louder;
        }
    }
    presets
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
        "osc1_position" => target::OSC1_POSITION,
        "fm" => target::FM,
        "osc2_position" => target::OSC2_POSITION,
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
    // Where the modulations take the knob now: a dot riding the outer arc.
    if let Some(&now) = state.live.mods.get(target)
        && now != 0.0
        && state.live.voices > 0
    {
        let a = start + sweep * (value + now).clamp(0.0, 1.0);
        let at = center + (radius - 3.5) * vec2(a.cos(), a.sin());
        ui.painter().circle_filled(at, 3.5, Color32::WHITE);
        ui.painter()
            .circle_stroke(at, 3.5, Stroke::new(1.0, Color32::from_black_alpha(120)));
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

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut SynthState) {
    state.targets.clear();
    let live = Live::read(meters);
    state.live = live.clone();
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::unique("synth-live-mods"), live.mods.clone()));
    let advanced = view_switch(ui, state);
    // Every row gets the same width, whatever a cell may have pushed out.
    let full = ui.available_width();
    hero(ui, meters, &live);
    ui.add_space(GAP);
    if advanced {
        advanced_face(ui, params, state, full, &live);
    } else {
        simple_face(ui, params, state, full, &live);
    }
    keyboard(ui, params, state);
    drop_source(ui, params, state);
}

/// The routings in use, one line each: source, target, amount, removal.
fn routings(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    let list = routes(params);
    panel(ui, |ui| {
        title(ui, tr("Modulation"), p.weak);
        if list.is_empty() {
            ui.weak(tr(
                "Drag a colored source (LFO, envelope, macro, velocity...) onto a knob to \
                 modulate it",
            ));
            return;
        }
        // Two per line, each filling its half.
        let pairs: Vec<_> = list.chunks(2).map(<[_]>::to_vec).collect();
        for pair in pairs {
            ui.columns(2, |columns| {
                for ((k, source, _, _), ui) in pair.into_iter().zip(columns.iter_mut()) {
                    let n = k + 1;
                    let width = ui.available_width();
                    ui.spacing_mut().combo_width = ((width - 220.0) / 2.0).max(60.0);
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
        }
    });
}

const ABOUT_WAVETABLES: &str = "A wavetable is a row of waves: the position glides from one \
     to the next, so the sound changes as it plays. Move it with an LFO or an envelope.";

/// The screen behind the scope and the wavetables.
const SCREEN: Color32 = Color32::from_rgb(12, 16, 22);

/// Oscillator `n`'s wavetable: the table, what it does, its frames, position and file.
fn wavetable_half(ui: &mut Ui, params: &mut Params, state: &mut SynthState, n: usize) {
    use audiostrada_dsp::wavetable::{LOADED, TABLE_HINTS};
    let p = theme::current(ui);
    let wave = if n == 1 { "wave" } else { "osc2_wave" };
    let used = params.get(wave).round() as u32 == audiostrada_dsp::synth::WAVETABLE;
    let table = format!("osc{n}_table");
    ui.horizontal(|ui| {
        let title = [tr("Wavetable 1"), tr("Wavetable 2")][n - 1];
        ui.label(RichText::new(title).strong().size(12.5).color(p.accent))
            .on_hover_text(tr(ABOUT_WAVETABLES));
        if used {
            choice_box(ui, params, &table);
            wavetable_file(ui, params, n);
        }
    });
    if !used {
        ui.add(egui::Label::new(RichText::new(tr(ABOUT_WAVETABLES)).weak().size(11.0)).wrap());
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.weak(trf!("Oscillator {n} plays another wave.", n = n));
            let response = ui.button(trf!("Play it on oscillator {n}", n = n));
            if response.clicked() {
                let v = audiostrada_dsp::synth::WAVETABLE as f32;
                params.set(wave, Some(v), &response);
            }
        });
        return;
    }
    let kind = (params.get(&table).round().max(0.0) as usize).min(LOADED);
    ui.add(egui::Label::new(RichText::new(tr(TABLE_HINTS[kind])).weak().size(11.0)).wrap());
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() - 90.0).max(120.0);
        wavetable_view(ui, params, n, vec2(width, 54.0));
        mod_knob(
            ui,
            params,
            state,
            &format!("osc{n}_position"),
            tr("Position"),
        );
    });
}

/// The file oscillator `n`'s wavetable may play, right-aligned: its name, Load and Remove.
/// Loading one plays it at once.
fn wavetable_file(ui: &mut Ui, params: &mut Params, n: usize) {
    use audiostrada_dsp::wavetable::LOADED;
    let table = format!("osc{n}_table");
    let slot = &params.info.files[n - 1];
    let loaded = {
        let project = params.doc.project();
        audiostrada_core::routing::find_insert(project, params.channel, params.insert)
            .and_then(|i| i.files.get(slot.id).cloned())
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if loaded.is_some() && ui.button(tr("Remove")).clicked() {
            let _ = params
                .doc
                .set_effect_file(params.channel, params.insert, slot.id, None);
        }
        let (doc, channel, insert) = (&mut *params.doc, params.channel, params.insert);
        if crate::mixer::load_button(ui, doc, channel, insert, slot) {
            let response = ui.response();
            params.set(&table, Some(LOADED as f32), &response);
        }
        match &loaded {
            Some(path) => {
                if let Some(Err(e)) = params.doc.player().effect_file(params.insert, slot.id) {
                    ui.colored_label(crate::settings::ERROR, e);
                }
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                ui.add(egui::Label::new(RichText::new(name).size(11.0)).truncate())
                    .on_hover_text(path.display().to_string());
            }
            None => {
                ui.weak(tr("No file loaded"));
            }
        }
    });
}

/// The switch between the simple view and the advanced one, shared by every synth; the
/// window takes the size of the view chosen.
fn view_switch(ui: &mut Ui, state: &mut SynthState) -> bool {
    let id = egui::Id::unique("synth-advanced");
    let mut advanced = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ui.horizontal(|ui| {
        for (on, label, hint) in [
            (false, "Simple", "The controls that shape the sound most"),
            (
                true,
                "Advanced",
                "Every control: wavetables, LFOs, routings, FM...",
            ),
        ] {
            let text = RichText::new(tr(label)).size(12.5);
            if ui
                .add(egui::Button::selectable(advanced == on, text))
                .on_hover_text(tr(hint))
                .clicked()
            {
                advanced = on;
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, advanced));
    if state.sized != Some(advanced) {
        state.sized = Some(advanced);
        let height = if advanced {
            ADVANCED_HEIGHT
        } else {
            SIMPLE_HEIGHT
        };
        let width = ui
            .ctx()
            .input(|i| i.viewport().inner_rect.map_or(1180.0, |r| r.width()));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(width, height)));
    }
    ui.add_space(4.0);
    advanced
}

/// Window heights that show each view whole, keyboard included.
const SIMPLE_HEIGHT: f32 = 910.0;
const ADVANCED_HEIGHT: f32 = 1180.0;

/// Every control.
fn advanced_face(ui: &mut Ui, params: &mut Params, state: &mut SynthState, full: f32, live: &Live) {
    let p = theme::current(ui);
    // Three columns kept from row to row: the two oscillators' width, then a narrow one.
    row(ui, full, &[1, 1, 1], 112.0, |ui, c| match c {
        0 | 1 => oscillator(ui, params, state, c + 1, false),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Sub and noise"), p.weak);
                segmented(ui, params, "sub_octave", &[]);
            });
            spread(ui, 2, |ui, k| {
                let (id, name) = [("sub_level", tr("Sub")), ("noise", tr("Noise"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    row(ui, full, &[1, 1, 1], 128.0, |ui, c| match c {
        0 | 1 => screen(ui, |ui| wavetable_half(ui, params, state, c + 1)),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Voice"), p.weak);
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
            ui.add_space(6.0);
            spread(ui, 3, |ui, k| {
                let (id, name) = [
                    ("glide", tr("Glide")),
                    ("spread", tr("Stereo")),
                    ("drift", tr("Drift")),
                ][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    row(ui, full, &[1, 1, 1], 128.0, |ui, c| match c {
        0 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Filter"), p.weak);
                choice_box(ui, params, "filter_type");
                let size = vec2(ui.available_width(), 40.0);
                filter_curve(ui, params, size, live.cutoff);
            });
            spread(ui, 5, |ui, k| {
                let (id, name) = [
                    ("cutoff", tr("Cutoff")),
                    ("resonance", tr("Resonance")),
                    ("env_amount", tr("Envelope")),
                    ("drive", tr("Drive")),
                    ("key_track", tr("Key track")),
                ][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
        1 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Envelope"), p.weak);
                let width = ui.available_width();
                envelope(ui, params, vec2(width, 40.0), live.envelope);
            });
            spread(ui, 4, |ui, k| {
                let (id, name) = [
                    ("attack", tr("Attack")),
                    ("decay", tr("Decay")),
                    ("sustain", tr("Sustain")),
                    ("release", tr("Release")),
                ][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Output"), p.weak);
                params.toggle(ui, "mpe", |ui, on| {
                    let response = pill_switch(ui, *on, tr("MPE")).on_hover_text(tr(
                        "For MPE controllers (Roli Seaboard, LinnStrument, Osmose...), which \
                         send each note on its own MIDI channel: bend, pressure and slide then \
                         shape every note on its own. Leave it off for an ordinary keyboard.",
                    ));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            });
            spread(ui, 3, |ui, k| {
                let (id, name) = [
                    ("volume", tr("Volume")),
                    ("velocity", tr("Velocity")),
                    ("bend_range", tr("Bend")),
                ][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    // The arpeggiator, the effects, and oscillator 2 shaping oscillator 1.
    row(ui, full, &[1, 1, 1], 100.0, |ui, c| match c {
        0 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Arpeggiator"), p.weak);
                for (id, label, hint) in [
                    (
                        "arp",
                        "On",
                        "Plays the held keys one after the other, in time",
                    ),
                    (
                        "arp_hold",
                        "Hold",
                        "Keeps playing once the keys are up, until the next chord",
                    ),
                ] {
                    params.toggle(ui, id, |ui, on| {
                        let response = pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
                        if response.clicked() {
                            *on = !*on;
                        }
                        response
                    });
                }
            });
            spread(ui, 4, |ui, k| {
                let (id, name) = [
                    ("arp_mode", tr("Order")),
                    ("arp_note", tr("Rate")),
                    ("arp_octaves", tr("Range")),
                    ("arp_gate", tr("Gate")),
                ][k];
                ui.spacing_mut().combo_width = 80.0;
                wide_knob(ui, params, id, name, 38.0, p.accent);
            });
        }),
        1 => panel(ui, |ui| {
            title(ui, tr("Effects"), p.weak);
            spread(ui, 6, |ui, k| {
                let (id, name) = [
                    ("fx_chorus", tr("Chorus")),
                    ("fx_delay", tr("Delay")),
                    ("fx_delay_note", tr("Time")),
                    ("fx_delay_feedback", tr("Feedback")),
                    ("fx_reverb", tr("Reverb")),
                    ("fx_reverb_decay", tr("Decay")),
                ][k];
                ui.spacing_mut().combo_width = 56.0;
                wide_knob(ui, params, id, name, 34.0, p.accent);
            });
        }),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Osc 2 → Osc 1"), p.weak);
                params.toggle(ui, "sync", |ui, on| {
                    let response = pill_switch(ui, *on, tr("Sync")).on_hover_text(tr(
                        "Oscillator 1 starts its cycle again with oscillator 2: tune oscillator \
                         1 higher, or sweep its pitch, for the tearing sync sound",
                    ));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            });
            spread(ui, 2, |ui, k| {
                let (id, name) = [("fm", tr("FM")), ("ring", tr("Ring"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    row(ui, full, &[1, 1, 1], 100.0, |ui, c| match c {
        0 | 1 => lfo(ui, params, state, c + 1),
        _ => panel(ui, |ui| {
            source_pill(ui, state, 3);
            spread(ui, 4, |ui, k| {
                let (field, name) = [
                    ("attack", tr("A")),
                    ("decay", tr("D")),
                    ("sustain", tr("S")),
                    ("release", tr("R")),
                ][k];
                wide_knob(ui, params, &format!("env2_{field}"), name, 30.0, p.accent);
            });
        }),
    });
    // Macros and what the player does, then the routings over the two other columns.
    row(ui, full, &[1, 2], 92.0, |ui, c| match c {
        0 => panel(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 110.0).max(200.0));
                    spread(ui, 4, |ui, k| {
                        source_pill(ui, state, k + 7);
                        wide_knob(ui, params, &format!("macro{}", k + 1), "", 30.0, p.accent);
                    });
                });
                ui.vertical(|ui| {
                    for source in 4..=6 {
                        source_pill(ui, state, source);
                    }
                });
            });
        }),
        _ => routings(ui, params),
    });
}

/// The controls that shape the sound most, on the same grid.
fn simple_face(ui: &mut Ui, params: &mut Params, state: &mut SynthState, full: f32, live: &Live) {
    let p = theme::current(ui);
    row(ui, full, &[1, 1, 1], 112.0, |ui, c| match c {
        0 | 1 => oscillator(ui, params, state, c + 1, true),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Sub and noise"), p.weak);
                segmented(ui, params, "sub_octave", &[]);
            });
            spread(ui, 2, |ui, k| {
                let (id, name) = [("sub_level", tr("Sub")), ("noise", tr("Noise"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    // A wavetable chosen on an oscillator brings its panel.
    let tables = ["wave", "osc2_wave"]
        .map(|w| params.get(w).round() as u32 == audiostrada_dsp::synth::WAVETABLE);
    if tables.contains(&true) {
        row(ui, full, &[1, 1, 1], 128.0, |ui, c| match c {
            0 | 1 => screen(ui, |ui| wavetable_half(ui, params, state, c + 1)),
            _ => screen(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(tr(ABOUT_WAVETABLES)).weak().size(11.0)).wrap(),
                );
            }),
        });
    }
    row(ui, full, &[1, 1, 1], 128.0, |ui, c| match c {
        0 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Filter"), p.weak);
                choice_box(ui, params, "filter_type");
                let size = vec2(ui.available_width(), 40.0);
                filter_curve(ui, params, size, live.cutoff);
            });
            spread(ui, 3, |ui, k| {
                let (id, name) = [
                    ("cutoff", tr("Cutoff")),
                    ("resonance", tr("Resonance")),
                    ("env_amount", tr("Envelope")),
                ][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
        1 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Envelope"), p.weak);
                let width = ui.available_width();
                envelope(ui, params, vec2(width, 40.0), live.envelope);
            });
            spread(ui, 4, |ui, k| {
                let (id, name) = [
                    ("attack", tr("Attack")),
                    ("decay", tr("Decay")),
                    ("sustain", tr("Sustain")),
                    ("release", tr("Release")),
                ][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
        _ => panel(ui, |ui| {
            title(ui, tr("Output"), p.weak);
            spread(ui, 2, |ui, k| {
                let (id, name) = [("volume", tr("Volume")), ("velocity", tr("Velocity"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    row(ui, full, &[1, 1, 1], 100.0, |ui, c| match c {
        0 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Arpeggiator"), p.weak);
                params.toggle(ui, "arp", |ui, on| {
                    let response = pill_switch(ui, *on, tr("On"))
                        .on_hover_text(tr("Plays the held keys one after the other, in time"));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            });
            spread(ui, 2, |ui, k| {
                let (id, name) = [("arp_mode", tr("Order")), ("arp_note", tr("Rate"))][k];
                ui.spacing_mut().combo_width = 90.0;
                wide_knob(ui, params, id, name, 38.0, p.accent);
            });
        }),
        1 => panel(ui, |ui| {
            title(ui, tr("Effects"), p.weak);
            spread(ui, 3, |ui, k| {
                let (id, name) = [
                    ("fx_chorus", tr("Chorus")),
                    ("fx_delay", tr("Delay")),
                    ("fx_reverb", tr("Reverb")),
                ][k];
                wide_knob(ui, params, id, name, 38.0, p.accent);
            });
        }),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Voice"), p.weak);
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
            spread(ui, 1, |ui, _| {
                mod_knob(ui, params, state, "glide", tr("Glide"))
            });
        }),
    });
    // The macros, to drop on a knob, and what the player does.
    row(ui, full, &[3], 92.0, |ui, _| {
        panel(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 130.0).max(200.0));
                    spread(ui, 4, |ui, k| {
                        source_pill(ui, state, k + 7);
                        wide_knob(ui, params, &format!("macro{}", k + 1), "", 30.0, p.accent);
                    });
                });
                ui.vertical(|ui| {
                    for source in 4..=6 {
                        source_pill(ui, state, source);
                    }
                });
            });
        });
    });
}

/// Space between the panels.
const GAP: f32 = 8.0;
/// Width shares of the three columns.
const GRID: [f32; 3] = [3.0, 3.0, 1.8];

/// One row of the grid, filling the width: cells spanning `spans` columns, of `height`.
fn row(ui: &mut Ui, full: f32, spans: &[usize], height: f32, mut add: impl FnMut(&mut Ui, usize)) {
    let total: f32 = GRID.iter().sum();
    let columns: Vec<f32> = GRID
        .iter()
        .map(|share| ((full - GAP * (GRID.len() - 1) as f32) * share / total).floor())
        .collect();
    // Each cell at its own place: one that needs more room never shifts the next.
    let top = ui.cursor().min;
    let (mut x, mut bottom, mut first) = (top.x, top.y + height, 0);
    for (c, &span) in spans.iter().enumerate() {
        let cells = &columns[first..(first + span).min(columns.len())];
        first += span;
        let w = cells.iter().sum::<f32>() + GAP * (cells.len().max(1) - 1) as f32;
        let rect = Rect::from_min_size(pos2(x, top.y), vec2(w, height));
        let builder = egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min));
        let used = ui.scope_builder(builder, |ui| add(ui, c)).response.rect;
        bottom = bottom.max(used.bottom());
        x += w + GAP;
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + full, bottom)),
        Sense::hover(),
    );
    ui.add_space(GAP);
}

/// A panel's name, on the line of its first controls.
fn title(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).strong().size(11.5).color(color));
}

/// A panel filling its cell.
fn panel(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let p = theme::current(ui);
    let size = ui.available_size();
    egui::Frame::new()
        .fill(p.base)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_size((size - vec2(20.0, 16.0)).max(egui::Vec2::ZERO));
            ui.set_max_width(size.x - 20.0);
            add(ui);
        });
}

/// A panel on the dark screen, with light controls whatever the theme.
fn screen(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let size = ui.available_size();
    egui::Frame::new()
        .fill(SCREEN)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            let selection = ui.visuals().selection;
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().selection = selection;
            ui.set_min_size((size - vec2(24.0, 16.0)).max(egui::Vec2::ZERO));
            ui.set_max_width(size.x - 24.0);
            add(ui);
        });
}

/// `n` controls spread evenly over the width left, each centered in its share.
fn spread(ui: &mut Ui, n: usize, mut add: impl FnMut(&mut Ui, usize)) {
    ui.spacing_mut().item_spacing.x = 2.0;
    ui.columns(n, |columns| {
        for (k, column) in columns.iter_mut().enumerate() {
            column.vertical_centered(|ui| add(ui, k));
        }
    });
}

/// An oscillator: its wave and how it sounds, then its knobs.
fn oscillator(ui: &mut Ui, params: &mut Params, state: &mut SynthState, n: usize, simple: bool) {
    let p = theme::current(ui);
    let (name, wave, ids, level) = if n == 1 {
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
        )
    } else {
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
        )
    };
    panel(ui, |ui| {
        ui.horizontal(|ui| {
            title(ui, tr(name), p.weak);
            wave_picker(ui, params, wave);
            oscillator_view(ui, params, wave, ids[3], level);
        });
        ui.add_space(2.0);
        // The simple view keeps octave, unison and level.
        let shown: &[usize] = if simple {
            &[0, 3, 5]
        } else {
            &[0, 1, 2, 3, 4, 5]
        };
        spread(ui, shown.len(), |ui, k| {
            let names = [
                tr("Octave"),
                tr("Semi"),
                tr("Fine"),
                tr("Unison"),
                tr("Detune"),
                tr("Level"),
            ];
            let k = shown[k];
            let id = if k < 5 { ids[k] } else { level };
            mod_knob(ui, params, state, id, names[k]);
        });
    });
}

/// An LFO: its shape and where it is, its rate or note, sync and retrigger.
fn lfo(ui: &mut Ui, params: &mut Params, state: &mut SynthState, l: usize) {
    let accent = theme::current(ui).accent;
    panel(ui, |ui| {
        ui.horizontal(|ui| {
            source_pill(ui, state, l);
            shape_picker(ui, params, &format!("lfo{l}_shape"));
            lfo_view(ui, params, l, state.live.lfo_phase[l - 1]);
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let synced = params.get(&format!("lfo{l}_sync")) >= 0.5;
            if synced {
                choice_box(ui, params, &format!("lfo{l}_note"));
            } else {
                wide_knob(
                    ui,
                    params,
                    &format!("lfo{l}_rate"),
                    tr("Rate"),
                    30.0,
                    accent,
                );
            }
            ui.add_space(8.0);
            for (field, label, hint) in [
                ("sync", "Sync", "A note value at the project's tempo"),
                (
                    "retrig",
                    "Retrigger",
                    "Starts again at each note; off, every note follows the same wave",
                ),
            ] {
                params.toggle(ui, &format!("lfo{l}_{field}"), |ui, on| {
                    let response = pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            }
        });
    });
}

/// The frames of an oscillator's wavetable in depth, the one at the position lit, as the
/// position (with its modulation) moves through them.
fn wavetable_view(ui: &mut Ui, params: &mut Params, n: usize, size: egui::Vec2) {
    use audiostrada_dsp::synth::target;
    use audiostrada_dsp::wavetable::{LOADED, factory};
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::from_rgb(20, 26, 34));
    let kind = params.get(&format!("osc{n}_table")).round() as usize;
    let loaded = (kind >= LOADED).then(|| loaded_table(params, n)).flatten();
    let Some(table) = loaded.or_else(|| factory().get(kind).cloned()) else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            tr("Loaded file"),
            FontId::proportional(10.0),
            p.weak,
        );
        return;
    };
    let mods = ui
        .ctx()
        .data(|d| d.get_temp::<Vec<f32>>(egui::Id::unique("synth-live-mods")))
        .unwrap_or_default();
    let index = if n == 1 {
        target::OSC1_POSITION
    } else {
        target::OSC2_POSITION
    };
    let position = (params.get(&format!("osc{n}_position")) / 100.0
        + mods.get(index).copied().unwrap_or(0.0))
    .clamp(0.0, 1.0);
    let depth = 10;
    let inner = rect.shrink2(vec2(6.0, 6.0));
    let (w, h) = (inner.width() * 0.7, inner.height() * 0.55);
    for k in (0..depth).rev() {
        let t = k as f32 / (depth - 1) as f32;
        // Back frames up and to the right, as a perspective.
        let origin = pos2(
            inner.left() + t * (inner.width() - w),
            inner.bottom() - h / 2.0 - t * (inner.height() - h),
        );
        let frame = table.frame(t);
        let points: Vec<Pos2> = (0..=48)
            .map(|j| {
                let s = frame[j * (frame.len() - 1) / 48];
                pos2(origin.x + j as f32 / 48.0 * w, origin.y - s * h / 2.0)
            })
            .collect();
        let near = (t - position).abs() < 0.5 / (depth - 1) as f32;
        let color = if near {
            p.accent
        } else {
            p.accent.gamma_multiply(0.18 + 0.2 * (1.0 - t))
        };
        painter.add(Shape::line(
            points,
            Stroke::new(if near { 1.8 } else { 1.0 }, color),
        ));
    }
}

/// The table read from oscillator `n`'s file, for drawing; read once per file.
fn loaded_table(params: &mut Params, n: usize) -> Option<std::sync::Arc<Table>> {
    let slot = params.info.files.get(n - 1)?;
    let path = audiostrada_core::routing::find_insert(
        params.doc.project(),
        params.channel,
        params.insert,
    )?
    .files
    .get(slot.id)?
    .clone();
    type Cache = std::collections::HashMap<std::path::PathBuf, Option<std::sync::Arc<Table>>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let mut cache = CACHE.lock().ok()?;
    cache
        .get_or_insert_with(Default::default)
        .entry(path.clone())
        .or_insert_with(|| {
            let audio = audiostrada_media::load(&path).ok()?;
            let samples = audio.channel(0).to_vec();
            Table::from_samples(&samples).ok().map(std::sync::Arc::new)
        })
        .clone()
}

/// An LFO's wave over one cycle, with a dot where it is now.
fn lfo_view(ui: &mut Ui, params: &mut Params, l: usize, phase: f32) {
    use audiostrada_dsp::filter::lfo_value;
    let p = theme::current(ui);
    let color = source_color(l);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(60.0), 24.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, p.base);
    let shape = params.get(&format!("lfo{l}_shape")).round() as usize;
    let inner = rect.shrink2(vec2(6.0, 5.0));
    let held = [0.6, -0.3, 0.9, -0.8];
    let point = |t: f32| {
        let v = lfo_value(shape, t, held[((t * 4.0) as usize).min(3)]);
        pos2(
            inner.left() + t * inner.width(),
            inner.center().y - v * inner.height() / 2.0,
        )
    };
    let points: Vec<Pos2> = (0..=80).map(|j| point(j as f32 / 80.0)).collect();
    painter.add(Shape::line(
        points,
        Stroke::new(1.5, color.gamma_multiply(0.8)),
    ));
    let at = point(phase.fract());
    painter.line_segment(
        [pos2(at.x, rect.top()), pos2(at.x, rect.bottom())],
        Stroke::new(1.0, color.gamma_multiply(0.35)),
    );
    painter.circle_filled(at, 7.0, color.gamma_multiply(0.25));
    painter.circle_filled(at, 3.5, color);
}

/// The top of the synth: the sound as an oscilloscope, glowing, and the voices playing.
fn hero(ui: &mut Ui, meters: &[f32], live: &Live) {
    use audiostrada_dsp::synth::meter;
    let p = theme::current(ui);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 70.0), Sense::hover());
    let painter = ui.painter_at(rect);
    // A deep screen whatever the theme, as on hardware.
    let screen = SCREEN;
    painter.rect_filled(rect, 10.0, screen);
    let scope = Rect::from_min_max(
        rect.min + vec2(14.0, 10.0),
        pos2(rect.right() - 200.0, rect.bottom() - 10.0),
    );
    let grid = Color32::from_rgb(40, 52, 66);
    for k in 1..8 {
        let x = scope.left() + scope.width() * k as f32 / 8.0;
        painter.line_segment(
            [pos2(x, scope.top()), pos2(x, scope.bottom())],
            Stroke::new(1.0, grid.gamma_multiply(0.5)),
        );
    }
    painter.line_segment(
        [
            pos2(scope.left(), scope.center().y),
            pos2(scope.right(), scope.center().y),
        ],
        Stroke::new(1.0, grid),
    );
    let samples = meters.get(..meter::SCOPE).unwrap_or(&[]);
    // Scaled to the loudest sample, so that soft and loud sounds both read.
    let peak = samples.iter().fold(0.05f32, |m, s| m.max(s.abs()));
    let points: Vec<Pos2> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            pos2(
                scope.left() + i as f32 / (meter::SCOPE - 1) as f32 * scope.width(),
                scope.center().y - s / peak * scope.height() * 0.45,
            )
        })
        .collect();
    let glow = p.accent;
    if live.voices > 0 {
        for (width, alpha) in [(7.0, 0.08), (3.5, 0.2), (1.6, 1.0)] {
            painter.add(Shape::line(
                points.clone(),
                Stroke::new(width, glow.gamma_multiply(alpha)),
            ));
        }
    } else {
        painter.line_segment(
            [
                pos2(scope.left(), scope.center().y),
                pos2(scope.right(), scope.center().y),
            ],
            Stroke::new(1.5, glow.gamma_multiply(0.5)),
        );
    }
    // The sixteen voices as lights.
    let panel = Rect::from_min_max(
        pos2(scope.right() + 16.0, rect.top() + 10.0),
        rect.max - vec2(14.0, 10.0),
    );
    painter.text(
        panel.left_top(),
        Align2::LEFT_TOP,
        crate::trf!("{count} voices", count = live.voices),
        FontId::monospace(11.0),
        Color32::from_rgb(170, 190, 210),
    );
    for v in 0..16 {
        let (row, col) = (v / 8, v % 8);
        let at = pos2(
            panel.left() + 6.0 + col as f32 * 20.0,
            panel.top() + 24.0 + row as f32 * 16.0,
        );
        let lit = v < live.voices;
        if lit {
            painter.circle_filled(at, 7.0, glow.gamma_multiply(0.18));
        }
        painter.circle_filled(at, 4.0, if lit { glow } else { grid });
    }
}

/// An oscillator's wave as it sounds: its unison voices stacked, drifting apart.
fn oscillator_view(ui: &mut Ui, params: &mut Params, wave: &str, voices: &str, level: &str) {
    let p = theme::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(60.0), 22.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5.0, p.base);
    let kind = params.get(wave).round() as usize;
    let n = params.get(voices).round().clamp(1.0, 16.0) as usize;
    let gain = (params.get(level) / 100.0).clamp(0.0, 1.0);
    let inner = rect.shrink2(vec2(6.0, 4.0));
    let time = ui.input(|i| i.time) as f32;
    // A wavetable draws the frame at its position.
    let frame: Option<Vec<f32>> = (kind == audiostrada_dsp::synth::WAVETABLE as usize)
        .then(|| {
            use audiostrada_dsp::wavetable::{LOADED, factory};
            let n = if wave == "wave" { 1 } else { 2 };
            let choice = params.get(&format!("osc{n}_table")).round() as usize;
            let position = params.get(&format!("osc{n}_position")) / 100.0;
            let table = if choice >= LOADED {
                loaded_table(params, n)
            } else {
                factory().get(choice).cloned()
            }?;
            Some(table.frame(position).to_vec())
        })
        .flatten();
    for k in 0..n.min(7) {
        let offset = if n > 1 {
            k as f32 / n as f32 * 0.35 + time * 0.05 * k as f32
        } else {
            0.0
        };
        let alpha = if k == 0 { 1.0 } else { 0.35 };
        let points: Vec<Pos2> = (0..=60)
            .map(|j| {
                let t = j as f32 / 60.0;
                let phase = (t * 2.0 + offset).fract();
                let v = match kind {
                    0 => 2.0 * phase - 1.0,
                    1 => {
                        if phase < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    2 => (phase * std::f32::consts::TAU).sin(),
                    3 => 1.0 - 4.0 * (phase - 0.5).abs(),
                    _ => frame.as_ref().map_or(0.0, |f| {
                        f[((phase * f.len() as f32) as usize).min(f.len() - 1)]
                    }),
                };
                pos2(
                    inner.left() + t * inner.width(),
                    inner.center().y - v * gain * inner.height() / 2.0,
                )
            })
            .collect();
        painter.add(Shape::line(
            points,
            Stroke::new(1.2, p.accent.gamma_multiply(alpha * (0.3 + 0.7 * gain))),
        ));
    }
    if n > 1 {
        ui.ctx().request_repaint();
    }
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
fn filter_curve(ui: &mut Ui, params: &mut Params, size: egui::Vec2, live: Option<f32>) {
    use audiostrada_dsp::synth::{BP, HP, LADDER, LP12, LP24};
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, p.base);
    let kind = params.get("filter_type").round() as usize;
    // The cutoff the newest note has now, with its envelope and modulations.
    let fc = live.filter(|f| *f > 0.0).unwrap_or(params.get("cutoff"));
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
        "Wavetable: a row of waves to glide through with the position",
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
                        3 => 1.0 - 4.0 * (phase - 0.5).abs(),
                        // Wavetable: smaller waves stacked in depth.
                        _ => (phase * std::f32::consts::TAU).sin() * 0.6,
                    };
                    pos2(
                        inner.left() + t * inner.width(),
                        inner.center().y - v * inner.height() / 2.0,
                    )
                })
                .collect();
            let ink = if on { p.text } else { p.weak };
            if i == 4 {
                for k in 1..3 {
                    let shift = vec2(k as f32 * 2.5, -(k as f32) * 2.0);
                    let back: Vec<Pos2> = points.iter().map(|q| *q + shift).collect();
                    painter.add(Shape::line(
                        back,
                        Stroke::new(1.0, ink.gamma_multiply(0.45)),
                    ));
                }
            }
            painter.add(Shape::line(points, Stroke::new(1.3, ink)));
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
fn envelope(ui: &mut Ui, params: &mut Params, size: egui::Vec2, live: Option<(f32, usize)>) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.base);
    let area = rect.shrink2(vec2(10.0, 6.0));
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
    // Where the newest note is on its envelope, glowing.
    if let Some((level, stage)) = live {
        let at = match stage {
            1 => pos2(area.left() + (attack.x - area.left()) * level, y(level)),
            2 => attack.lerp(
                decay,
                ((1.0 - level) / (1.0 - sustain).max(1e-3)).clamp(0.0, 1.0),
            ),
            3 => pos2((decay.x + hold.x) / 2.0, y(sustain)),
            _ => hold.lerp(release, (1.0 - level / sustain.max(1e-3)).clamp(0.0, 1.0)),
        };
        painter.circle_filled(at, 9.0, Color32::WHITE.gamma_multiply(0.12));
        painter.circle_filled(at, 4.5, Color32::WHITE);
    }
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
    // The knobs below name the stages; a taller curve names them itself.
    let letters = if rect.height() >= 60.0 {
        &[(attack, "A"), (decay, "D"), (hold, "S"), (release, "R")][..]
    } else {
        &[]
    };
    for &(point, letter) in letters {
        painter.text(
            pos2(point.x, area.bottom() + 1.0),
            Align2::CENTER_TOP,
            letter,
            FontId::proportional(9.0),
            p.weak,
        );
    }
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
                show(ui, &mut params, &[], state);
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
