//! The built-in synth's editor: ready-made sounds, four plain sections (tone, brightness,
//! shape, playing) and a keyboard played with the mouse or the computer keyboard.

use std::collections::BTreeSet;

use audiostrada_core::routing::Channel;
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::effect_editors::{Params, from_unit, knob_row, mode_buttons, param_knob, to_unit};
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
}

impl Default for SynthState {
    fn default() -> Self {
        Self {
            low: 48,
            mouse: None,
            computer: BTreeSet::new(),
            typing: false,
            track: None,
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

/// Ready-made sounds, from the most useful.
pub(crate) fn factory() -> Vec<crate::presets::Preset> {
    use crate::presets::preset;
    // Waveforms: saw, square, sine, triangle.
    let sound = |name, wave: f32, adsr: [f32; 4], filter: [f32; 3], detune, velocity, volume| {
        let [attack, decay, sustain, release] = adsr;
        let [cutoff, resonance, env_amount] = filter;
        preset(
            name,
            &[
                ("wave", wave),
                ("attack", attack),
                ("decay", decay),
                ("sustain", sustain),
                ("release", release),
                ("cutoff", cutoff),
                ("resonance", resonance),
                ("env_amount", env_amount),
                ("detune", detune),
                ("velocity", velocity),
                ("volume", volume),
            ],
        )
    };
    vec![
        preset(tr("Init"), &[]),
        sound(
            tr("Soft keys"),
            3.0,
            [3.0, 900.0, 35.0, 500.0],
            [3500.0, 10.0, 35.0],
            4.0,
            80.0,
            -12.0,
        ),
        sound(
            tr("Warm pad"),
            0.0,
            [600.0, 1500.0, 80.0, 2500.0],
            [1200.0, 15.0, 20.0],
            18.0,
            30.0,
            -15.0,
        ),
        sound(
            tr("Strings"),
            0.0,
            [350.0, 800.0, 85.0, 1200.0],
            [2800.0, 5.0, 10.0],
            25.0,
            40.0,
            -14.0,
        ),
        sound(
            tr("Brass"),
            0.0,
            [60.0, 500.0, 70.0, 250.0],
            [900.0, 20.0, 70.0],
            12.0,
            70.0,
            -13.0,
        ),
        sound(
            tr("Pluck"),
            0.0,
            [1.0, 250.0, 0.0, 250.0],
            [600.0, 35.0, 75.0],
            6.0,
            80.0,
            -11.0,
        ),
        sound(
            tr("Bell"),
            2.0,
            [1.0, 2500.0, 0.0, 2500.0],
            [18000.0, 0.0, 0.0],
            0.0,
            80.0,
            -10.0,
        ),
        sound(
            tr("Square lead"),
            1.0,
            [5.0, 400.0, 75.0, 200.0],
            [4000.0, 25.0, 30.0],
            10.0,
            50.0,
            -14.0,
        ),
        sound(
            tr("Sub bass"),
            2.0,
            [2.0, 300.0, 90.0, 120.0],
            [800.0, 0.0, 0.0],
            0.0,
            40.0,
            -10.0,
        ),
        sound(
            tr("Acid bass"),
            0.0,
            [1.0, 180.0, 20.0, 80.0],
            [350.0, 80.0, 85.0],
            0.0,
            70.0,
            -12.0,
        ),
        sound(
            tr("Hollow pad"),
            1.0,
            [800.0, 2000.0, 70.0, 3000.0],
            [1500.0, 30.0, 15.0],
            20.0,
            20.0,
            -16.0,
        ),
    ]
}

pub fn show(ui: &mut Ui, params: &mut Params, state: &mut SynthState) {
    let p = theme::current(ui);
    let gap = ui.spacing().item_spacing.x;
    // Three panels a row, their margins (10 each side) included.
    let third = ((ui.available_width() - 2.0 * gap) / 3.0 - 20.0).max(200.0);
    ui.horizontal_top(|ui| {
        panel(ui, tr("Tone"), third, |ui| {
            mode_buttons(
                ui,
                params,
                "wave",
                &[
                    "Bright and buzzy: brass, strings, leads",
                    "Hollow: clarinet, retro games",
                    "Pure: sub bass, soft flutes",
                    "Soft and round: mellow keys",
                ],
            );
            ui.add_space(4.0);
            param_knob(ui, params, "detune", 40.0, p.accent);
        });
        panel(ui, tr("Brightness"), third, |ui| {
            knob_row(ui, params, &["cutoff", "resonance", "env_amount"], p.accent);
        });
        panel(ui, tr("Playing"), third, |ui| {
            knob_row(ui, params, &["volume", "velocity", "bend_range"], p.accent);
            params.toggle(ui, "mpe", |ui, on| {
                ui.checkbox(on, tr("MPE controller")).on_hover_text(tr(
                    "For MPE controllers (Roli Seaboard, LinnStrument, Osmose...), which send each \
                     note on its own MIDI channel: bend, pressure and slide then shape every note \
                     on its own. Leave it off for an ordinary keyboard.",
                ))
            });
        });
    });
    ui.add_space(6.0);
    let full = 3.0 * third + 2.0 * (gap + 20.0);
    panel(ui, tr("Shape"), full, |ui| {
        ui.horizontal_top(|ui| {
            envelope(ui, params, (full * 0.5).max(240.0));
            ui.add_space(8.0);
            knob_row(
                ui,
                params,
                &["attack", "decay", "sustain", "release"],
                p.accent,
            );
        });
    });
    ui.add_space(6.0);
    keyboard(ui, params, state);
}

/// A titled panel.
fn panel(ui: &mut Ui, title: &str, width: f32, add: impl FnOnce(&mut Ui)) {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(theme::mix(p.panel, p.base, 0.4))
        .corner_radius(8.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .size(9.5)
                        .color(p.weak)
                        .strong(),
                );
                ui.add_space(2.0);
                add(ui);
            });
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
}
