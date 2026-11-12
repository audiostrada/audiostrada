//! The built-in synth's editor: ready-made sounds, the two oscillators with their sub and
//! noise, then the filter, the voice and the output, the envelope, and a keyboard played with
//! the mouse or the computer keyboard.

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
    /// Keys held on the computer keyboard, each with the note it started: let go, it ends
    /// that note even if the octave changed meanwhile.
    computer: Vec<(egui::Key, u8)>,
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
    /// The picture of the sound in the simple view: the one chosen by a click (else the
    /// family's), the one before that click, and whether it has the whole window.
    picture: Option<Picture>,
    picture_before: Option<Picture>,
    immersive: bool,
    /// The height of what lay under the top screen (the picture or the scope) at the last
    /// frame: in a window that does not follow its content, the screen takes the rest.
    below: f32,
    /// The ready-made sounds, to tell which one plays, and the one that does.
    sounds: Option<Vec<(u64, Sound)>>,
    sound: Option<Sound>,
    /// The landscape of the simple view: the wave's latest lines, newest first, when the
    /// last one was added, and the level they are drawn against.
    ridges: std::collections::VecDeque<Vec<f32>>,
    pushed: f64,
    scale: f32,
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
            computer: Vec::new(),
            typing: false,
            track: None,
            dragging: None,
            targets: Vec::new(),
            live: Live::default(),
            picture: None,
            picture_before: None,
            immersive: false,
            below: 0.0,
            sounds: None,
            sound: None,
            ridges: std::collections::VecDeque::new(),
            pushed: 0.0,
            scale: 0.02,
        }
    }
}

impl SynthState {
    /// Releases every held note (the window closes or loses the focus).
    pub fn release(&mut self, doc: &crate::document::Document) {
        let Some(track) = self.track else {
            return;
        };
        for key in self.mouse.take().into_iter().chain(
            std::mem::take(&mut self.computer)
                .into_iter()
                .map(|(_, k)| k),
        ) {
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
        "fx_chorus" => target::CHORUS,
        "fx_delay" => target::DELAY,
        "fx_reverb" => target::REVERB,
        "detune" | "osc2_detune" => target::DETUNE,
        "attack" => target::ATTACK,
        "decay" => target::DECAY,
        "release" => target::RELEASE,
        "env_amount" => target::ENV_AMOUNT,
        "lfo1_depth" => target::LFO_DEPTH,
        "lfo2_depth" => target::LFO_DEPTH + 1,
        "env2_depth" => target::ENV2_DEPTH,
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
    mod_knob_sized(ui, params, state, id, name, 38.0);
}

fn mod_knob_sized(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut SynthState,
    id: &str,
    name: &str,
    size: f32,
) {
    let accent = theme::current(ui).accent;
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
    source_pill_named(
        ui,
        state,
        source,
        tr(audiostrada_dsp::synth::SOURCES[source]),
    );
}

/// A source's pill under another name than the source's: a macro's.
fn source_pill_named(ui: &mut Ui, state: &mut SynthState, source: usize, name: &str) {
    let p = theme::current(ui);
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
    if !audiostrada_dsp::synth::reaches(source, target) {
        params.doc.notify(
            tr(
                "Only the macros and the mod wheel move this control: it is the whole \
                instrument's, not each note's",
            )
            .into(),
        );
        return;
    }
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
    let (view, expert) = view_switch(ui, params);
    // Every row gets the same width, whatever a cell may have pushed out.
    let full = ui.available_width();
    // The ready-made sound these settings are, if any.
    let print = fingerprint(|i, _| params.values.get(i).copied().unwrap_or(0.0));
    let known = state.sounds.get_or_insert_with(known_sounds);
    state.sound = known.iter().find(|s| s.0 == print).map(|s| s.1.clone());
    // A maximized window, or the picture given the whole window, keeps its height: the screen
    // at the top takes what the controls leave.
    let maximized = ui.ctx().input(|i| {
        let v = i.viewport();
        v.maximized == Some(true) || v.fullscreen == Some(true)
    });
    let immersive = state.immersive && view == View::Simple;
    let fill = maximized || immersive;
    if fill {
        crate::mixer::keep_height(ui.ctx());
    }
    let screen_height = |ui: &Ui, natural: f32, below: f32| {
        if fill {
            (ui.available_height() - below - 1.0).max(natural).floor()
        } else {
            natural
        }
    };
    // Where the screen starts and how high it is: what lies under it is measured from there.
    let top = ui.cursor().min.y;
    let screen;
    match view {
        View::Simple => {
            screen = screen_height(ui, PICTURE_HEIGHT, state.below);
            picture(ui, meters, state, screen);
            ui.add_space(GAP);
            if !immersive {
                simple_face(ui, params, state, full);
            }
        }
        _ => {
            screen = screen_height(ui, HERO_HEIGHT, state.below);
            hero(ui, meters, &live, screen);
            ui.add_space(GAP);
            match view {
                View::Oscillators => oscillators_face(ui, params, state, full, expert),
                View::Filter => filter_face(ui, params, state, full, &live, expert),
                View::Modulation => lfos_face(ui, params, state, full, expert),
                View::Effects => effects_face(ui, params, state, full),
                _ => macros_face(ui, params, full),
            }
            if view.controls() {
                sources_line(ui, params, state);
            }
        }
    }
    keyboard(ui, params, state);
    state.below = ui.min_rect().bottom() - top - screen;
    drop_source(ui, params, state);
    shot::tick(ui, params);
}

/// Development aid: with `AUDIOSTRADA_SCREENSHOT_SYNTH` set, opens Strada-Synth's window on the
/// sound named by `AUDIOSTRADA_SCREENSHOT_SOUND` (in the view of `AUDIOSTRADA_SCREENSHOT_VIEW`:
/// `oscillators`, `filter`, `modulation`, `effects` or `macros`, with the expert controls when
/// `AUDIOSTRADA_SCREENSHOT_EXPERT` is set) and holds a chord, for a capture of the window from outside.
pub(crate) mod shot {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static OPENED: Cell<u32> = const { Cell::new(0) };
        static FRAMES: Cell<u32> = const { Cell::new(0) };
    }
    const CHORD: [u8; 3] = [57, 60, 64];

    fn asked() -> Option<()> {
        std::env::var_os("AUDIOSTRADA_SCREENSHOT_SYNTH").map(|_| ())
    }

    /// The synth to open, once it is in the project: the first one, or one added for the shot.
    pub(crate) fn open(
        ctx: &egui::Context,
        doc: &mut crate::document::Document,
    ) -> Option<(Channel, audiostrada_core::InsertId)> {
        asked()?;
        ctx.request_repaint();
        let calls = OPENED.get();
        OPENED.set(calls.saturating_add(1));
        let found = doc.project().tracks.iter().find_map(|t| {
            let insert = t.inserts.iter().find(|i| i.effect == "synth")?;
            Some((Channel::Track(t.id), insert.id))
        });
        match (calls, found) {
            (0, None) => {
                doc.history_mut().edit("Synth", |p| {
                    let t = p.add_track("Synth");
                    audiostrada_core::routing::add_insert(
                        p,
                        Channel::Track(t),
                        "synth",
                        Default::default(),
                    );
                });
                None
            }
            (5, Some((channel, insert))) => {
                let wanted = std::env::var("AUDIOSTRADA_SCREENSHOT_SOUND").unwrap_or_default();
                let sound = crate::synth_presets::factory()
                    .into_iter()
                    .find(|s| s.name == wanted);
                if let Some(sound) = sound {
                    doc.history_mut().edit("Sound", |p| {
                        for (id, value) in &sound.params {
                            audiostrada_core::routing::set_insert_param(
                                p, channel, insert, id, *value,
                            );
                        }
                    });
                }
                Some((channel, insert))
            }
            _ => None,
        }
    }

    /// In the synth's window: the view asked for, the chord, then the picture.
    pub(crate) fn tick(ui: &mut Ui, params: &Params) {
        if asked().is_none() {
            return;
        }
        let frame = FRAMES.get() + 1;
        FRAMES.set(frame);
        let ctx = ui.ctx().clone();
        ctx.request_repaint();
        let play = |on: bool| {
            if let Some(track) = track_of(params) {
                for key in CHORD {
                    let data = if on { [0x90, key, 100] } else { [0x80, key, 0] };
                    params.doc.player().play_key(track, data);
                }
            }
        };
        if frame == 1 {
            let view = std::env::var("AUDIOSTRADA_SCREENSHOT_VIEW").unwrap_or_default();
            let at = [
                "",
                "oscillators",
                "filter",
                "modulation",
                "effects",
                "macros",
            ]
            .iter()
            .position(|v| *v == view)
            .unwrap_or(0);
            let expert = std::env::var_os("AUDIOSTRADA_SCREENSHOT_EXPERT").is_some();
            ctx.data_mut(|d| {
                d.insert_temp(View::id(), at);
                d.insert_temp(View::expert_id(), expert);
            });
        }
        if frame == 20 {
            play(true);
        }
    }
}

/// The routings of the LFOs, the second envelope and the player, one line each: source,
/// target, amount, removal.
fn routings(ui: &mut Ui, params: &mut Params) {
    let p = theme::current(ui);
    // The macros' own are with the macros.
    let list: Vec<_> = routes(params)
        .into_iter()
        .filter(|r| r.1 < audiostrada_dsp::synth::FIRST_MACRO)
        .collect();
    panel(ui, |ui| {
        title(ui, tr("LFOs, envelope and playing"), p.weak);
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

/// The views of the synth: the first to play it, the next four its controls by what they do,
/// the last its macros.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum View {
    Simple,
    Oscillators,
    Filter,
    Modulation,
    Effects,
    Macros,
}

impl View {
    const ALL: [View; 6] = [
        View::Simple,
        View::Oscillators,
        View::Filter,
        View::Modulation,
        View::Effects,
        View::Macros,
    ];

    /// Where the view chosen is kept, for every synth: its place in [`View::ALL`].
    pub(crate) fn id() -> egui::Id {
        egui::Id::unique("synth-view")
    }

    /// Where the expert controls' switch is kept.
    pub(crate) fn expert_id() -> egui::Id {
        egui::Id::unique("synth-expert")
    }

    fn name(self) -> (&'static str, &'static str) {
        match self {
            View::Simple => ("Simple", "The sound, its macros and its effects"),
            View::Oscillators => (
                "Oscillators",
                "The two oscillators, their wavetables, the sub and the noise",
            ),
            View::Filter => (
                "Filter and envelope",
                "The filter, the envelope, how the notes follow each other and the output",
            ),
            View::Modulation => (
                "Modulation",
                "The LFOs, the second envelope, and what they and the playing modulate",
            ),
            View::Effects => (
                "Arpeggiator and effects",
                "The arpeggiator, the chorus, the delay and the reverb",
            ),
            View::Macros => (
                "Macros",
                "The four macros of the simple view: their names and what each one moves",
            ),
        }
    }

    /// The views of the synth's controls, where sources are dragged onto knobs and the
    /// expert controls show.
    fn controls(self) -> bool {
        !matches!(self, View::Simple | View::Macros)
    }
}

/// The track's record arm, at the right of the views: armed, the track plays what the MIDI
/// keyboard sends and records it.
fn arm_switch(ui: &mut Ui, params: &mut Params) {
    let Some(track) = track_of(params) else {
        return;
    };
    let Some(armed) = params.doc.project().track(track).map(|t| t.armed) else {
        return;
    };
    let p = theme::current(ui);
    let red = theme::RECORD;
    let label = if armed {
        tr("Track armed")
    } else {
        tr("Arm the track")
    };
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, p.text);
    let size = vec2(galley.size().x + 40.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered();
    let fill = match (armed, hovered) {
        (true, _) => red.gamma_multiply(if hovered { 0.3 } else { 0.22 }),
        (false, true) => p.hover,
        (false, false) => p.surface,
    };
    painter.rect_filled(rect, 13.0, fill);
    painter.rect_stroke(
        rect,
        13.0,
        Stroke::new(1.0, if armed { red } else { p.border }),
        egui::StrokeKind::Inside,
    );
    let dot = pos2(rect.left() + 15.0, rect.center().y);
    if armed {
        crate::gear::glow(
            painter,
            dot,
            9.0,
            red.gamma_multiply(0.5),
            Color32::TRANSPARENT,
        );
        painter.circle_filled(dot, 5.0, red);
    } else {
        painter.circle_stroke(
            dot,
            4.5,
            Stroke::new(
                1.5,
                if hovered {
                    red
                } else {
                    red.gamma_multiply(0.7)
                },
            ),
        );
    }
    painter.galley(
        pos2(rect.left() + 27.0, rect.center().y - galley.size().y / 2.0),
        galley,
        if armed { p.text } else { p.weak },
    );
    let response = response
        .on_hover_text(tr(
            "Armed, the track plays what your MIDI keyboard sends, and records it",
        ))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        // Arming is not an edit of the song, as with the R buttons.
        params.doc.history_mut().modify_unrecorded(|p| {
            audiostrada_core::groups::set_switch(
                p,
                track,
                audiostrada_core::groups::Link::Arm,
                !armed,
            );
        });
    }
}

/// The switch between the views, and in those of the controls the switch that shows the
/// expert ones. Returns the view and whether the expert's controls show.
fn view_switch(ui: &mut Ui, params: &mut Params) -> (View, bool) {
    let at = ui
        .ctx()
        .data(|d| d.get_temp::<usize>(View::id()))
        .unwrap_or(0);
    let mut view = View::ALL[at.min(View::ALL.len() - 1)];
    let mut expert = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(View::expert_id()))
        .unwrap_or(false);
    ui.horizontal(|ui| {
        for tab in View::ALL {
            let (label, hint) = tab.name();
            let text = RichText::new(tr(label)).size(12.5);
            if ui
                .add(egui::Button::selectable(view == tab, text))
                .on_hover_text(tr(hint))
                .clicked()
            {
                view = tab;
            }
            // The simple view and the macros stand apart from the controls.
            if matches!(tab, View::Simple | View::Effects) {
                ui.add_space(10.0);
            }
        }
        if view.controls() {
            ui.add_space(8.0);
            let response = pill_switch(ui, expert, tr("Expert")).on_hover_text(tr(
                "Shows the controls few sounds need: fine tuning, FM, sync and ring, key \
                 tracking, drift, the depth of the LFOs, MPE",
            ));
            if response.clicked() {
                expert = !expert;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            arm_switch(ui, params);
        });
    });
    ui.ctx().data_mut(|d| {
        let at = View::ALL.iter().position(|v| *v == view).unwrap_or(0);
        d.insert_temp(View::id(), at);
        d.insert_temp(View::expert_id(), expert);
    });
    ui.add_space(4.0);
    (view, expert)
}

/// A switch of the synth, as a pill with its hint.
fn pill(ui: &mut Ui, params: &mut Params, id: &str, label: &'static str, hint: &'static str) {
    params.toggle(ui, id, |ui, on| {
        let response = pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
        if response.clicked() {
            *on = !*on;
        }
        response
    });
}

const ARP_HINT: &str = "Plays the held keys one after the other, in time";
const HOLD_HINT: &str = "Keeps playing once the keys are up, until the next chord";

/// The oscillators, the sub and the noise, then the wavetables; for the expert, fine tuning
/// and oscillator 2 shaping oscillator 1.
fn oscillators_face(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut SynthState,
    full: f32,
    expert: bool,
) {
    let p = theme::current(ui);
    row(ui, full, &[1, 1, 1], 112.0, |ui, c| match c {
        0 | 1 => oscillator(ui, params, state, c + 1, !expert),
        _ => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Sub and noise"), p.weak);
                ui.spacing_mut().combo_width = 96.0;
                choice_box(ui, params, "sub_octave");
            });
            ui.add_space(4.0);
            spread(ui, 2, |ui, k| {
                let (id, name) = [("sub_level", tr("Sub")), ("noise", tr("Noise"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    row(ui, full, &[1, 1, 1], 128.0, |ui, c| match c {
        0 | 1 => screen(ui, |ui| wavetable_half(ui, params, state, c + 1)),
        _ if expert => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Osc 2 → Osc 1"), p.weak);
                pill(
                    ui,
                    params,
                    "sync",
                    "Sync",
                    "Oscillator 1 starts its cycle again with oscillator 2: tune oscillator \
                     1 higher, or sweep its pitch, for the tearing sync sound",
                );
            });
            ui.add_space(6.0);
            spread(ui, 2, |ui, k| {
                let (id, name) = [("fm", tr("FM")), ("ring", tr("Ring"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
        _ => screen(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(tr(ABOUT_WAVETABLES)).weak().size(11.0)).wrap());
        }),
    });
}

/// The filter and the envelope, then how the notes follow each other and the output.
fn filter_face(
    ui: &mut Ui,
    params: &mut Params,
    state: &mut SynthState,
    full: f32,
    live: &Live,
    expert: bool,
) {
    let p = theme::current(ui);
    row(ui, full, &[1, 1, 1], 128.0, |ui, c| match c {
        0 => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Filter"), p.weak);
                choice_box(ui, params, "filter_type");
                let size = vec2(ui.available_width(), 40.0);
                filter_curve(ui, params, size, live.cutoff);
            });
            let shown = if expert { 5 } else { 4 };
            spread(ui, shown, |ui, k| {
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
            title(ui, tr("Output"), p.weak);
            ui.add_space(6.0);
            spread(ui, 2, |ui, k| {
                let (id, name) = [("volume", tr("Volume")), ("velocity", tr("Velocity"))][k];
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
    row(ui, full, &[2, 1], 100.0, |ui, c| match c {
        0 => panel(ui, |ui| {
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
            let shown = if expert { 3 } else { 1 };
            ui.horizontal(|ui| {
                for k in 0..shown {
                    let (id, name) = [
                        ("glide", tr("Glide")),
                        ("spread", tr("Stereo")),
                        ("drift", tr("Drift")),
                    ][k];
                    mod_knob(ui, params, state, id, name);
                    ui.add_space(18.0);
                }
            });
        }),
        _ if expert => panel(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, tr("Controller"), p.weak);
                pill(
                    ui,
                    params,
                    "mpe",
                    "MPE",
                    "For MPE controllers (Roli Seaboard, LinnStrument, Osmose...), which \
                     send each note on its own MIDI channel: bend, pressure and slide then \
                     shape every note on its own. Leave it off for an ordinary keyboard.",
                );
            });
            spread(ui, 1, |ui, _| {
                mod_knob(ui, params, state, "bend_range", tr("Bend"));
            });
        }),
        _ => {}
    });
}

/// What modulates: the two LFOs and the second envelope, then what they and the playing
/// move.
fn lfos_face(ui: &mut Ui, params: &mut Params, state: &mut SynthState, full: f32, expert: bool) {
    let p = theme::current(ui);
    row(ui, full, &[1, 1, 1], 100.0, |ui, c| match c {
        0 | 1 => lfo(ui, params, state, c + 1, expert),
        _ => panel(ui, |ui| {
            source_pill(ui, state, 3);
            let shown = if expert { 5 } else { 4 };
            spread(ui, shown, |ui, k| {
                let (field, name) = [
                    ("attack", tr("A")),
                    ("decay", tr("D")),
                    ("sustain", tr("S")),
                    ("release", tr("R")),
                    ("depth", tr("Depth")),
                ][k];
                let id = format!("env2_{field}");
                if k == 4 {
                    mod_knob_sized(ui, params, state, &id, name, 30.0);
                } else {
                    wide_knob(ui, params, &id, name, 30.0, p.accent);
                }
            });
        }),
    });
    row(ui, full, &[3], 64.0, |ui, _| routings(ui, params));
}

/// The arpeggiator and the effects after the voices.
fn effects_face(ui: &mut Ui, params: &mut Params, state: &mut SynthState, full: f32) {
    let p = theme::current(ui);
    row(ui, full, &[1, 2], 104.0, |ui, c| match c {
        0 => panel(ui, |ui| arpeggiator(ui, params, p.accent)),
        _ => panel(ui, |ui| {
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
                ui.spacing_mut().combo_width = 72.0;
                mod_knob(ui, params, state, id, name);
            });
        }),
    });
}

/// The arpeggiator: its switches, then the order, the rate, the range and the gate.
fn arpeggiator(ui: &mut Ui, params: &mut Params, color: Color32) {
    let p = theme::current(ui);
    ui.horizontal(|ui| {
        title(ui, tr("Arpeggiator"), p.weak);
        pill(ui, params, "arp", "On", ARP_HINT);
        pill(ui, params, "arp_hold", "Hold", HOLD_HINT);
    });
    spread(ui, 4, |ui, k| {
        let (id, name) = [
            ("arp_mode", tr("Order")),
            ("arp_note", tr("Rate")),
            ("arp_octaves", tr("Range")),
            ("arp_gate", tr("Gate")),
        ][k];
        ui.spacing_mut().combo_width = 80.0;
        wide_knob(ui, params, id, name, 38.0, color);
    });
}

/// Every source, to drag onto a knob of the view: what each one moves is told and set in the
/// Modulation and Macros views.
fn sources_line(ui: &mut Ui, params: &Params, state: &mut SynthState) {
    let p = theme::current(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(tr("Drag onto a knob:"))
                .size(11.0)
                .color(p.weak),
        );
        for source in 1..=3 {
            source_pill(ui, state, source);
        }
        for k in 0..4 {
            let name = macro_name(params, k);
            source_pill_named(ui, state, k + 7, &name);
        }
        for source in 4..=6 {
            source_pill(ui, state, source);
        }
    });
    ui.add_space(GAP);
}

/// The macros, each with its name and what it moves.
fn macros_face(ui: &mut Ui, params: &mut Params, full: f32) {
    use audiostrada_dsp::synth::{FIRST_MACRO, SLOTS, TARGETS, reaches};
    let color = source_color(FIRST_MACRO);
    let list = routes(params);
    let most = (0..4)
        .map(|k| list.iter().filter(|r| r.1 == FIRST_MACRO + k).count())
        .max()
        .unwrap_or(0);
    let height = MACRO_KNOB + 96.0 + 26.0 * (most + 1) as f32;
    let width = ((full - 3.0 * GAP) / 4.0).floor();
    let top = ui.cursor().min;
    let mut bottom = top.y + height;
    for k in 0..4 {
        let source = FIRST_MACRO + k;
        let rect = Rect::from_min_size(
            pos2(top.x + k as f32 * (width + GAP), top.y),
            vec2(width, height),
        );
        let builder = egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min));
        let used = ui
            .scope_builder(builder, |ui| {
                panel(ui, |ui| {
                    ui.vertical_centered(|ui| macro_knob(ui, params, k, color, MACRO_KNOB, true));
                    ui.add_space(6.0);
                    for &(slot, _, _, _) in list.iter().filter(|r| r.1 == source) {
                        let n = slot + 1;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.spacing_mut().combo_width = (ui.available_width() - 104.0).max(70.0);
                            choice_box(ui, params, &format!("mod{n}_target"));
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
                    }
                    // One more thing for the macro to move.
                    let mut picked = None;
                    ui.menu_button(RichText::new(tr("+ Add a target")).size(11.5), |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for (t, name) in TARGETS.iter().enumerate().skip(1) {
                                    let taken = list.iter().any(|r| r.1 == source && r.2 == t);
                                    if reaches(source, t) && !taken && ui.button(tr(name)).clicked()
                                    {
                                        picked = Some(t);
                                        ui.close();
                                    }
                                }
                            });
                    });
                    if let Some(target) = picked {
                        match (0..SLOTS).find(|k| !list.iter().any(|r| r.0 == *k)) {
                            Some(free) => {
                                let response = ui.response();
                                for (field, v) in [
                                    ("source", source as f32),
                                    ("target", target as f32),
                                    ("amount", 50.0),
                                ] {
                                    let id = format!("mod{}_{field}", free + 1);
                                    params.set(&id, Some(v), &response);
                                }
                                params.doc.history_mut().end_gesture();
                            }
                            None => params.doc.notify(
                                tr("Every modulation slot is in use: remove one first").into(),
                            ),
                        }
                    }
                });
            })
            .response
            .rect;
        bottom = bottom.max(used.bottom());
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + full, bottom)),
        Sense::hover(),
    );
    ui.add_space(GAP);
}

/// What a macro is called: the name chosen for it, else what it moves first.
fn macro_name(params: &Params, k: usize) -> String {
    use audiostrada_dsp::synth::{FIRST_MACRO, MACRO_NAMES, SOURCES, TARGETS};
    let chosen = params.get(&format!("macro{}_name", k + 1)).round() as usize;
    if (1..MACRO_NAMES.len()).contains(&chosen) {
        return tr(MACRO_NAMES[chosen]).to_owned();
    }
    let source = FIRST_MACRO + k;
    routes(params)
        .iter()
        .find(|r| r.1 == source && r.3 != 0.0 && r.2 < TARGETS.len())
        .map_or_else(
            || tr(SOURCES[source]).to_owned(),
            |r| tr(TARGETS[r.2]).to_owned(),
        )
}

/// A macro's name over its knob in the macros' view: clicked, the names to choose from.
fn macro_name_menu(ui: &mut Ui, params: &mut Params, k: usize) {
    use audiostrada_dsp::synth::MACRO_NAMES;
    let p = theme::current(ui);
    let id = format!("macro{}_name", k + 1);
    let chosen = params.get(&id).round() as usize;
    let text = RichText::new(macro_name(params, k))
        .size(14.0)
        .strong()
        .color(p.text);
    let mut picked = None;
    let response = ui
        .menu_button(text, |ui| {
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, name) in MACRO_NAMES.iter().enumerate() {
                        if ui.selectable_label(i == chosen, tr(name)).clicked() {
                            picked = Some(i as f32);
                            ui.close();
                        }
                    }
                });
        })
        .response
        .on_hover_text(tr("The macro's name; Auto names it after what it moves"));
    params.change(&id, picked, &response);
    if picked.is_some() {
        params.doc.history_mut().end_gesture();
    }
}

/// What a macro moves, by name.
fn macro_targets(params: &Params, k: usize) -> Vec<&'static str> {
    use audiostrada_dsp::synth::{FIRST_MACRO, TARGETS};
    routes(params)
        .iter()
        .filter(|r| r.1 == FIRST_MACRO + k && r.3 != 0.0 && r.2 < TARGETS.len())
        .map(|r| tr(TARGETS[r.2]))
        .collect()
}

/// A macro: its name (a menu to choose it where `rename`), a large knob in `color` with its
/// ring, its value.
fn macro_knob(ui: &mut Ui, params: &mut Params, k: usize, color: Color32, size: f32, rename: bool) {
    let p = theme::current(ui);
    let id = format!("macro{}", k + 1);
    let Some(info) = params.param(&id) else {
        return;
    };
    let name = macro_name(params, k);
    let moves = macro_targets(params, k);
    let value = params.get(&id);
    if rename {
        macro_name_menu(ui, params, k);
    } else {
        ui.label(RichText::new(&name).size(14.0).strong().color(p.text));
    }
    ui.add_space(10.0);
    let (changed, response) = crate::widgets::knob(
        ui,
        to_unit(info, value),
        (0.0, 1.0),
        0.0,
        size,
        false,
        if moves.is_empty() { p.weak } else { color },
    );
    let changed = changed.map(|t| from_unit(info, t));
    let shown = changed.unwrap_or(value);
    if !moves.is_empty() {
        ring(
            ui,
            response.rect.center(),
            size,
            to_unit(info, shown),
            color,
        );
    }
    let hint = if moves.is_empty() {
        tr("This macro moves nothing yet: give it a target in the Macros view").to_owned()
    } else {
        trf!(
            "{name} moves: {targets}. Right-click to assign a MIDI controller",
            name = name,
            targets = moves.join(", ")
        )
    };
    let response = response.on_hover_text(hint);
    params.set(&id, changed, &response);
    ui.add_space(8.0);
    ui.label(
        RichText::new(format!("{shown:.0} %"))
            .monospace()
            .size(11.0)
            .color(p.weak),
    );
}

/// Diameter of the macros of the simple view.
const MACRO_KNOB: f32 = 68.0;

/// A setting of the sound itself among the macros of the simple view: a smaller knob, with
/// the same ring.
fn plain_knob(ui: &mut Ui, params: &mut Params, id: &str, label: &str, color: Color32) {
    let p = theme::current(ui);
    let Some(info) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    ui.add_space(MACRO_KNOB - SIDE_KNOB);
    ui.label(RichText::new(label).size(12.5).strong().color(p.text));
    ui.add_space(8.0);
    let (changed, response) = crate::widgets::knob(
        ui,
        to_unit(info, value),
        (0.0, 1.0),
        to_unit(info, info.default),
        SIDE_KNOB,
        false,
        color,
    );
    let changed = changed.map(|t| from_unit(info, t));
    let shown = changed.unwrap_or(value);
    ring(
        ui,
        response.rect.center(),
        SIDE_KNOB,
        to_unit(info, shown),
        color,
    );
    let response = response.on_hover_text(trf!(
        "{name}: {value}. Right-click to assign a MIDI controller",
        name = tr(info.name),
        value = format!("{shown:.0} %")
    ));
    params.set(id, changed, &response);
    ui.add_space(8.0);
    ui.label(
        RichText::new(format!("{shown:.0} %"))
            .monospace()
            .size(11.0)
            .color(p.weak),
    );
}

/// Diameter of the sub's and the noise's knobs beside the macros.
const SIDE_KNOB: f32 = 50.0;

/// Around a large knob of `diameter`, a ring in `color` filled up to `turned` (0 to 1).
fn ring(ui: &Ui, center: Pos2, diameter: f32, turned: f32, color: Color32) {
    let p = theme::current(ui);
    let sweep = 0.75 * std::f32::consts::TAU;
    let start = std::f32::consts::FRAC_PI_2 + (std::f32::consts::TAU - sweep) / 2.0;
    let radius = diameter / 2.0 + 7.0;
    let arc = |to: f32| -> Vec<Pos2> {
        let steps = 48;
        (0..=steps)
            .map(|i| {
                let a = start + sweep * to * i as f32 / steps as f32;
                center + radius * vec2(a.cos(), a.sin())
            })
            .collect()
    };
    let painter = ui.painter();
    painter.add(Shape::line(
        arc(1.0),
        Stroke::new(3.0, theme::mix(p.base, color, 0.22)),
    ));
    if turned > 0.004 {
        painter.add(Shape::line(
            arc(turned),
            Stroke::new(7.0, color.gamma_multiply(0.18)),
        ));
        painter.add(Shape::line(arc(turned), Stroke::new(3.0, color)));
    }
}

/// The sound and what plays it best: its four macros, large, with the sub and the noise
/// beside them, then the effects, the arpeggiator, and the glide of a sound that plays one
/// note at a time or the release of one that plays chords.
fn simple_face(ui: &mut Ui, params: &mut Params, state: &mut SynthState, full: f32) {
    let p = theme::current(ui);
    let color = state.sound.as_ref().map_or(p.accent, |s| s.color);
    let top = ui.cursor().min;
    let height = MACRO_KNOB + 104.0;
    let rect = Rect::from_min_size(top, vec2(full, height));
    ui.painter().rect_filled(rect, 10.0, p.base);
    let inner = rect.shrink2(vec2(16.0, 12.0));
    // The macros over two thirds, the sub and the noise over the last one, a line between.
    let split = inner.left() + inner.width() * 0.7;
    let cell = |ui: &mut Ui, area: Rect, add: &mut dyn FnMut(&mut Ui)| {
        let builder = egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::top_down(egui::Align::Min));
        ui.scope_builder(builder, |ui| add(ui));
    };
    let macros = Rect::from_min_max(inner.min, pos2(split - 12.0, inner.bottom() - 18.0));
    cell(ui, macros, &mut |ui| {
        spread(ui, 4, |ui, k| {
            macro_knob(ui, params, k, color, MACRO_KNOB, false)
        });
    });
    ui.painter().line_segment(
        [
            pos2(split, inner.top() + 8.0),
            pos2(split, inner.bottom() - 26.0),
        ],
        Stroke::new(1.0, theme::mix(p.base, p.weak, 0.25)),
    );
    let sides = Rect::from_min_max(
        pos2(split + 12.0, inner.top()),
        pos2(inner.right(), inner.bottom() - 18.0),
    );
    cell(ui, sides, &mut |ui| {
        spread(ui, 2, |ui, k| {
            let (id, name) = [("sub_level", tr("Sub")), ("noise", tr("Noise"))][k];
            plain_knob(ui, params, id, name, color);
        });
    });
    // How to put the knobs under the fingers, said once and softly.
    ui.painter().text(
        pos2(inner.center().x, inner.bottom() - 4.0),
        Align2::CENTER_BOTTOM,
        tr("Right-click a knob to assign it to a MIDI controller, or to automate it"),
        FontId::proportional(10.5),
        theme::mix(p.weak, p.base, 0.35),
    );
    ui.allocate_rect(rect, Sense::hover());
    ui.add_space(GAP);
    let mono = params.get("mode") >= 0.5;
    row(ui, full, &[1, 1, 1], 100.0, |ui, c| match c {
        0 => panel(ui, |ui| arpeggiator(ui, params, color)),
        1 => panel(ui, |ui| {
            title(ui, tr("Effects"), p.weak);
            ui.add_space(4.0);
            spread(ui, 3, |ui, k| {
                let (id, name) = [
                    ("fx_chorus", tr("Chorus")),
                    ("fx_delay", tr("Delay")),
                    ("fx_reverb", tr("Reverb")),
                ][k];
                wide_knob(ui, params, id, name, 38.0, color);
            });
        }),
        _ => panel(ui, |ui| {
            title(ui, tr("Playing"), p.weak);
            ui.add_space(4.0);
            spread(ui, 2, |ui, k| {
                // One note at a time slides to the next; chords ring on.
                let (id, name) = match (k, mono) {
                    (0, true) => ("glide", tr("Glide")),
                    (0, false) => ("release", tr("Release")),
                    _ => ("volume", tr("Volume")),
                };
                wide_knob(ui, params, id, name, 38.0, color);
            });
        }),
    });
}

/// The sound a synth plays, when it is one of the ready-made ones.
#[derive(Clone, Debug)]
struct Sound {
    category: String,
    name: String,
    /// Its family's place in the list, and its color.
    family: usize,
    color: Color32,
}

/// What the simple view lets the player change: a sound stays itself whatever they are.
const PLAYED: [&str; 19] = [
    "macro1",
    "macro2",
    "macro3",
    "macro4",
    "sub_level",
    "noise",
    "volume",
    "attack",
    "release",
    "glide",
    "fx_chorus",
    "fx_delay",
    "fx_reverb",
    "arp",
    "arp_hold",
    "arp_mode",
    "arp_note",
    "arp_octaves",
    "arp_gate",
];

/// A color for each family of sounds, in the order the families come in the list.
const FAMILY_COLORS: [Color32; 13] = [
    Color32::from_rgb(255, 122, 69),
    Color32::from_rgb(255, 205, 64),
    Color32::from_rgb(110, 190, 255),
    Color32::from_rgb(150, 130, 255),
    Color32::from_rgb(255, 140, 165),
    Color32::from_rgb(255, 172, 60),
    Color32::from_rgb(105, 230, 160),
    Color32::from_rgb(95, 225, 235),
    Color32::from_rgb(205, 140, 255),
    Color32::from_rgb(255, 110, 200),
    Color32::from_rgb(85, 205, 190),
    Color32::from_rgb(245, 95, 115),
    Color32::from_rgb(190, 222, 90),
];

/// What tells a sound from the others: its settings but those the player turns.
fn fingerprint(value_of: impl Fn(usize, &audiostrada_dsp::ParamInfo) -> f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let Some(info) = audiostrada_dsp::info("synth") else {
        return 0;
    };
    for (i, param) in info.params.iter().enumerate() {
        if !PLAYED.contains(&param.id) {
            ((value_of(i, param) * 100.0).round() as i64).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// The ready-made sounds by fingerprint, each with its family's color.
fn known_sounds() -> Vec<(u64, Sound)> {
    let mut families: Vec<String> = Vec::new();
    crate::synth_presets::factory()
        .into_iter()
        .skip(1)
        .filter_map(|preset| {
            let (category, name) = crate::effect_editors::preset_category(&preset.name)?;
            let family = families
                .iter()
                .position(|f| *f == category)
                .unwrap_or_else(|| {
                    families.push(category.clone());
                    families.len() - 1
                });
            let print = fingerprint(|_, p| preset.params.get(p.id).copied().unwrap_or(p.default));
            let color = FAMILY_COLORS[family % FAMILY_COLORS.len()];
            Some((
                print,
                Sound {
                    category,
                    name,
                    family,
                    color,
                },
            ))
        })
        .collect()
}

/// Height of the scope over the controls' views.
const HERO_HEIGHT: f32 = 70.0;

/// Height of the sound's picture in the simple view.
const PICTURE_HEIGHT: f32 = 210.0;

/// Lines of the wave kept, how many points each, and how often one is added.
const RIDGES: usize = 30;
const RIDGE_POINTS: usize = 128;
const RIDGE_EVERY: f64 = 0.035;

/// How the sound is drawn in the simple view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Picture {
    /// A landscape, each line a moment of the wave, receding: pads, strings, atmospheres.
    Landscape,
    /// Rings spreading from the middle, each one a moment of the wave: bells, plucks, keys.
    Rings,
    /// The wave as a ribbon and its echoes: basses, leads, brass.
    Ribbon,
}

impl Picture {
    const ALL: [Picture; 3] = [Picture::Landscape, Picture::Rings, Picture::Ribbon];

    /// The picture that suits a family of sounds, by its place in the list.
    fn of_family(family: usize) -> Self {
        match family {
            0 | 1 | 5 | 11 => Picture::Ribbon,
            2 | 6 | 7 | 9 => Picture::Rings,
            _ => Picture::Landscape,
        }
    }
}

/// The color of a line of the picture `depth` back (0 the newest, 1 the oldest): the sound's
/// color fading into the distance, or in full window every color in turn.
fn picture_color(color: Color32, depth: f32, time: f64, vivid: bool) -> Color32 {
    if vivid {
        let hue = (depth * 0.7 + time as f32 * 0.06).fract();
        egui::ecolor::Hsva::new(hue, 0.8, 1.0, 1.0).into()
    } else {
        theme::mix(color, Color32::from_rgb(70, 60, 150), depth.sqrt())
    }
}

/// The top of the simple view: the sound drawn by its wave in the sound's color, with its
/// family and name and the voices playing. A click draws it another way, a double click
/// gives it the whole window.
fn picture(ui: &mut Ui, meters: &[f32], state: &mut SynthState, height: f32) {
    use audiostrada_dsp::synth::meter;
    let p = theme::current(ui);
    let color = state.sound.as_ref().map_or(p.accent, |s| s.color);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let response = response.on_hover_text(tr(
        "Click for another picture of the sound, double-click to give it the whole window",
    ));
    if response.double_clicked() {
        state.immersive = !state.immersive;
        // The first click of the two changed the picture: back to it.
        state.picture = state.picture_before;
    } else if response.clicked() {
        let family = state.sound.as_ref().map_or(usize::MAX, |s| s.family);
        let now = state.picture.unwrap_or_else(|| Picture::of_family(family));
        let at = Picture::ALL.iter().position(|k| *k == now).unwrap_or(0);
        state.picture_before = state.picture;
        state.picture = Some(Picture::ALL[(at + 1) % Picture::ALL.len()]);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 12.0, SCREEN);
    // A new line of the wave every few hundredths of a second.
    let now = ui.input(|i| i.time);
    if now - state.pushed >= RIDGE_EVERY {
        state.pushed = now;
        let samples = meters.get(..meter::SCOPE).unwrap_or(&[]);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        // The loudest of late sets the scale: a note that fades is seen fading.
        state.scale = peak.max(state.scale * 0.985).max(0.02);
        let step = (samples.len() / RIDGE_POINTS).max(1);
        let line: Vec<f32> = if state.live.voices > 0 || peak > 1e-4 {
            let raw: Vec<f32> = samples
                .chunks(step)
                .take(RIDGE_POINTS)
                .map(|c| c.iter().sum::<f32>() / c.len() as f32 / state.scale)
                .collect();
            // Rounded a little: a landscape, not a saw blade.
            (0..raw.len())
                .map(|k| {
                    let at = |j: usize| raw[j.min(raw.len() - 1)];
                    0.25 * at(k.saturating_sub(1)) + 0.5 * at(k) + 0.25 * at(k + 1)
                })
                .collect()
        } else {
            Vec::new()
        };
        state.ridges.push_front(line);
        state.ridges.truncate(RIDGES);
    }
    if state.ridges.iter().any(|r| !r.is_empty()) || state.immersive {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(RIDGE_EVERY));
    }
    let family = state.sound.as_ref().map_or(usize::MAX, |s| s.family);
    let kind = state.picture.unwrap_or_else(|| Picture::of_family(family));
    let vivid = state.immersive;
    match kind {
        Picture::Landscape => landscape(&painter, rect, &state.ridges, color, now, vivid),
        Picture::Rings => rings(&painter, rect, &state.ridges, color, now, vivid),
        Picture::Ribbon => ribbon(&painter, rect, &state.ridges, color, now, vivid),
    }
    // The sound's family and name.
    let at = rect.min + vec2(18.0, 14.0);
    match &state.sound {
        Some(sound) => {
            painter.text(
                at,
                Align2::LEFT_TOP,
                sound.category.to_uppercase(),
                FontId::proportional(10.5),
                sound.color,
            );
            painter.text(
                at + vec2(0.0, 15.0),
                Align2::LEFT_TOP,
                &sound.name,
                FontId::proportional(22.0),
                Color32::from_rgb(236, 240, 246),
            );
        }
        None => {
            painter.text(
                at,
                Align2::LEFT_TOP,
                tr("YOUR SOUND"),
                FontId::proportional(10.5),
                Color32::from_rgb(150, 165, 185),
            );
        }
    }
    // The sixteen voices as lights.
    for v in 0..16 {
        let at = pos2(
            rect.right() - 22.0 - (15 - v) as f32 * 11.0,
            rect.top() + 22.0,
        );
        let lit = v < state.live.voices;
        if lit {
            painter.circle_filled(at, 5.5, color.gamma_multiply(0.2));
        }
        painter.circle_filled(
            at,
            2.5,
            if lit {
                color
            } else {
                Color32::from_rgb(40, 52, 66)
            },
        );
    }
}

type Ridges = std::collections::VecDeque<Vec<f32>>;

/// The wave's lines as a landscape: the newest in front, the older ones receding behind, each
/// hiding what lies behind it.
fn landscape(
    painter: &egui::Painter,
    rect: Rect,
    ridges: &Ridges,
    color: Color32,
    time: f64,
    vivid: bool,
) {
    let field = rect.shrink2(vec2(24.0, 0.0));
    for (i, line) in ridges.iter().enumerate().rev() {
        let depth = i as f32 / (RIDGES - 1) as f32;
        let base = rect.bottom() - 30.0 - depth.powf(0.85) * rect.height() * 0.5;
        let inset = depth * field.width() * 0.14;
        let (left, right) = (field.left() + inset, field.right() - inset);
        let reach = rect.height() * 0.3 * (1.0 - 0.5 * depth);
        let near = 1.0 - depth;
        let tint = picture_color(color, depth, time, vivid);
        let stroke = theme::mix(SCREEN, tint, 0.25 + 0.75 * near * near);
        // The ends rest on the ground: the wave rises in the middle of the field.
        let window = |k: usize| {
            let t = k as f32 / (RIDGE_POINTS - 1) as f32;
            (std::f32::consts::PI * t).sin().powf(0.7)
        };
        let points: Vec<Pos2> = (0..RIDGE_POINTS)
            .map(|k| {
                let t = k as f32 / (RIDGE_POINTS - 1) as f32;
                let s = line.get(k).copied().unwrap_or(0.0).clamp(-1.2, 1.2);
                pos2(left + t * (right - left), base - s * window(k) * reach)
            })
            .collect();
        // The ground under the line, lit near the line where the wave stands.
        let mut mesh = egui::Mesh::default();
        for (k, point) in points.iter().enumerate() {
            let lit = if line.is_empty() {
                0.0
            } else {
                0.2 * near * window(k)
            };
            mesh.colored_vertex(*point, theme::mix(SCREEN, tint, lit));
            mesh.colored_vertex(pos2(point.x, base + reach), SCREEN);
        }
        for k in 0..points.len() as u32 - 1 {
            mesh.add_triangle(2 * k, 2 * k + 1, 2 * k + 2);
            mesh.add_triangle(2 * k + 1, 2 * k + 3, 2 * k + 2);
        }
        painter.add(Shape::mesh(mesh));
        if i == 0 && !line.is_empty() {
            for (width, alpha) in [(8.0, 0.07), (4.0, 0.18)] {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(width, tint.gamma_multiply(alpha)),
                ));
            }
        }
        let width = if i == 0 { 1.8 } else { 1.0 };
        painter.add(Shape::line(points, Stroke::new(width, stroke)));
    }
}

/// The wave's lines as rings spreading from the middle, the newest the smallest and the
/// brightest: what a struck sound does to still water.
fn rings(
    painter: &egui::Painter,
    rect: Rect,
    ridges: &Ridges,
    color: Color32,
    time: f64,
    vivid: bool,
) {
    let center = rect.center() + vec2(0.0, 8.0);
    // Wider than high, as the screen is.
    let (across, up) = (rect.width() * 0.46, rect.height() * 0.44);
    for (i, line) in ridges.iter().enumerate().rev() {
        let depth = i as f32 / (RIDGES - 1) as f32;
        let near = 1.0 - depth;
        let size = 0.08 + 0.92 * depth.powf(0.8);
        let tint = picture_color(color, depth, time, vivid);
        let stroke = theme::mix(SCREEN, tint, 0.12 + 0.88 * near * near);
        let swell = 0.22 * (0.35 + 0.65 * near);
        let points: Vec<Pos2> = (0..=RIDGE_POINTS)
            .map(|k| {
                let k = k % RIDGE_POINTS;
                let t = k as f32 / RIDGE_POINTS as f32;
                // The wave's two ends meet: each fades into the other around the ring.
                let s = line.get(k).copied().unwrap_or(0.0);
                let other = line.get(RIDGE_POINTS - 1 - k).copied().unwrap_or(0.0);
                let blend = (0.5 - (t - 0.5).abs()) * 2.0;
                let s = (s * blend.min(1.0) + other * (1.0 - blend.min(1.0))).clamp(-1.2, 1.2);
                let a = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                let r = size + s * swell;
                center + vec2(a.cos() * r * across, a.sin() * r * up)
            })
            .collect();
        if i == 0 && !line.is_empty() {
            painter.add(Shape::line(
                points.clone(),
                Stroke::new(6.0, tint.gamma_multiply(0.15)),
            ));
        }
        let width = if i == 0 { 1.8 } else { 1.0 };
        painter.add(Shape::line(points, Stroke::new(width, stroke)));
    }
}

/// The wave as a ribbon across the screen, mirrored about its middle, its last moments
/// spreading behind it as echoes.
fn ribbon(
    painter: &egui::Painter,
    rect: Rect,
    ridges: &Ridges,
    color: Color32,
    time: f64,
    vivid: bool,
) {
    const ECHOES: usize = 10;
    let field = rect.shrink2(vec2(24.0, 0.0));
    let middle = rect.center().y + 10.0;
    for (i, line) in ridges.iter().enumerate().take(ECHOES).rev() {
        if line.is_empty() {
            continue;
        }
        let depth = i as f32 / (ECHOES - 1) as f32;
        let near = 1.0 - depth;
        let tint = picture_color(color, depth, time, vivid);
        let reach = rect.height() * 0.36 * (1.0 + 0.35 * depth);
        let window = |k: usize| {
            let t = k as f32 / (RIDGE_POINTS - 1) as f32;
            (std::f32::consts::PI * t).sin().powf(0.5)
        };
        let at = |k: usize, side: f32| {
            let t = k as f32 / (RIDGE_POINTS - 1) as f32;
            let s = line.get(k).copied().unwrap_or(0.0).clamp(-1.2, 1.2);
            pos2(
                field.left() + t * field.width(),
                middle - side * s * window(k) * reach,
            )
        };
        // Between the wave and its mirror, light that thins out with the echoes.
        let mut mesh = egui::Mesh::default();
        let fill = tint.gamma_multiply(0.05 + 0.2 * near * near);
        for k in 0..RIDGE_POINTS {
            mesh.colored_vertex(at(k, 1.0), fill);
            mesh.colored_vertex(at(k, -1.0), fill);
        }
        for k in 0..RIDGE_POINTS as u32 - 1 {
            mesh.add_triangle(2 * k, 2 * k + 1, 2 * k + 2);
            mesh.add_triangle(2 * k + 1, 2 * k + 3, 2 * k + 2);
        }
        painter.add(Shape::mesh(mesh));
        let stroke = theme::mix(SCREEN, tint, 0.15 + 0.85 * near * near);
        for side in [1.0, -1.0] {
            let points: Vec<Pos2> = (0..RIDGE_POINTS).map(|k| at(k, side)).collect();
            if i == 0 {
                painter.add(Shape::line(
                    points.clone(),
                    Stroke::new(6.0, tint.gamma_multiply(0.14)),
                ));
            }
            let width = if i == 0 { 1.8 } else { 1.0 };
            painter.add(Shape::line(points, Stroke::new(width, stroke)));
        }
    }
    if ridges.front().is_none_or(Vec::is_empty) {
        painter.line_segment(
            [pos2(field.left(), middle), pos2(field.right(), middle)],
            Stroke::new(1.5, color.gamma_multiply(0.5)),
        );
    }
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
        // Without the expert controls: octave, unison, detune and level.
        let shown: &[usize] = if simple {
            &[0, 3, 4, 5]
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
fn lfo(ui: &mut Ui, params: &mut Params, state: &mut SynthState, l: usize, expert: bool) {
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
            if expert {
                mod_knob_sized(
                    ui,
                    params,
                    state,
                    &format!("lfo{l}_depth"),
                    tr("Depth"),
                    30.0,
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
    painter.rect_filled(rect, 5.0, theme::screen(&p));
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
fn hero(ui: &mut Ui, meters: &[f32], live: &Live, height: f32) {
    use audiostrada_dsp::synth::meter;
    let p = theme::current(ui);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
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
    painter.rect_filled(rect, 5.0, theme::screen(&p));
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
    painter.rect_filled(rect, 6.0, theme::screen(&p));
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
    painter.rect_filled(rect, 8.0, theme::screen(&p));
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
        Channel::Master | Channel::Clip(_) => None,
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
        for (_, note) in std::mem::take(&mut state.computer) {
            play(note, None);
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
                let held = state.computer.iter().position(|(k, _)| *k == key);
                if pressed && held.is_none() {
                    state.computer.push((key, note));
                    play(note, Some(100));
                } else if !pressed && let Some(i) = held {
                    let (_, started) = state.computer.remove(i);
                    play(started, None);
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
        u8::try_from(key)
            .is_ok_and(|k| state.mouse == Some(k) || state.computer.iter().any(|(_, n)| *n == k))
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

    /// The picture given the whole window fills its height, whatever the window's: the window
    /// is asked to keep its height rather than to follow the content, and the arm switch
    /// arms the track.
    #[test]
    fn the_picture_fills_the_window_it_is_given() {
        use crate::document::{AppDirs, Document};
        use audiostrada_core::routing::{self, Channel};
        use eframe::egui::RawInput;
        let mut doc = Document::new(AppDirs::default());
        let mut insert = None;
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("A");
            insert = routing::add_insert(p, Channel::Track(t), "synth", Default::default());
        });
        let channel = Channel::Track(doc.project().tracks[0].id);
        let info = audiostrada_dsp::info("synth").unwrap();
        let ctx = egui::Context::default();
        let mut state = SynthState {
            immersive: true,
            ..Default::default()
        };
        for height in [700.0, 1000.0, 1000.0, 1000.0, 820.0, 820.0, 820.0] {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, height))),
                ..Default::default()
            };
            let mut bottom = 0.0;
            ctx.run_ui(input, |ui| {
                let mut params = Params {
                    values: info.params.iter().map(|p| p.default).collect(),
                    doc: &mut doc,
                    channel,
                    insert: insert.unwrap(),
                    info,
                };
                show(ui, &mut params, &[], &mut state);
                bottom = ui.min_rect().bottom();
            })
            .drop_without_applying_deltas();
            let kept = ctx
                .data_mut(|d| {
                    d.remove_temp::<bool>(egui::Id::unique((
                        "fit-keep-height",
                        egui::ViewportId::ROOT,
                    )))
                })
                .unwrap_or(false);
            assert!(kept, "the window keeps its height");
            if height > 700.0 && state.below > 0.0 {
                assert!(
                    (height - 24.0..=height).contains(&bottom),
                    "{bottom} in a window of {height}"
                );
            }
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
        // The filter's view, where the cutoff is.
        ctx.data_mut(|d| d.insert_temp(View::id(), 2usize));
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
