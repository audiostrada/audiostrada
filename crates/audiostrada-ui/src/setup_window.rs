//! The setup assistant, at the first start and from the Help menu: the interface's language and
//! look, the shortcuts one is used to, the sound card, the MIDI inputs and the panels' layout.
//! Every step can be skipped; what was set before skipping stays.

use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::dialog;
use crate::host::AudioHost;
use crate::i18n::tr;
use crate::keymap::{Keymap, Preset};
use crate::layout::Start;
use crate::settings::{AudioSettingsPanel, SettingsChange};
use crate::theme;
use crate::ui_prefs::{ThemeChoice, UiPrefs, WindowState};

/// The panels' layout chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    One(Start),
    /// The classic layout, the mixer in its own window on another screen.
    TwoScreens,
}

/// What the application applies after a frame of the assistant.
#[derive(Debug, Default)]
pub struct SetupOutcome {
    pub ui_changed: bool,
    pub audio: SettingsChange,
    pub midi_changed: bool,
    /// On finishing: the layout, and for two screens the mixer's window.
    pub layout: Option<(Arrangement, Option<WindowState>)>,
}

const STEPS: usize = 4;

#[derive(Debug)]
pub struct SetupWindow {
    open: bool,
    step: usize,
    arrangement: Arrangement,
    /// The screens, read when the assistant opens, in window units.
    screens: Vec<Rect>,
    midi_ports: Option<Vec<String>>,
    check_updates: bool,
}

impl Default for SetupWindow {
    fn default() -> Self {
        Self {
            open: false,
            step: 0,
            arrangement: Arrangement::One(Start::Classic),
            screens: Vec::new(),
            midi_ports: None,
            check_updates: true,
        }
    }
}

impl SetupWindow {
    pub fn open(&mut self, ctx: &egui::Context) {
        *self = Self {
            open: true,
            screens: crate::screens::monitors(ctx),
            ..Self::default()
        };
        if self.screens.len() > 1 {
            self.arrangement = Arrangement::TwoScreens;
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        prefs: &mut UiPrefs,
        audio: &mut AudioSettingsPanel,
        host: &AudioHost,
    ) -> SetupOutcome {
        let mut outcome = SetupOutcome::default();
        if !self.open {
            return outcome;
        }
        let mut open = true;
        let mut skip = false;
        let mut go: Option<usize> = None;
        let mut finish = false;
        let detail = match self.step {
            0 => tr("Your language, your look and the shortcuts you know"),
            1 => tr("The sound card AudioStrada plays and records through"),
            2 => tr("The keyboards and controllers that play the instruments"),
            _ => tr("Where the panels go"),
        };
        dialog::Dialog::new("setup", tr("Welcome to AudioStrada"))
            .detail(detail)
            .width(680.0)
            .modal()
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    // The same height at every step: the buttons stay where they are.
                    ui.set_min_height(500.0);
                    steps_bar(ui, self.step);
                    ui.add_space(10.0);
                    match self.step {
                        0 => outcome.ui_changed |= welcome(ui, prefs),
                        1 => outcome.audio = audio.show(ui, host),
                        2 => {
                            let (toggled, refreshed) = crate::preferences::midi_inputs(
                                ui,
                                prefs,
                                &mut self.midi_ports,
                                None,
                            );
                            outcome.ui_changed |= toggled;
                            outcome.midi_changed |= toggled || refreshed;
                        }
                        _ => self.layout_step(ui),
                    }
                }
                dialog::Part::Actions => {
                    if dialog::secondary(ui, tr("Skip")).clicked() {
                        skip = true;
                    }
                    dialog::buttons(ui, |ui| {
                        let last = self.step + 1 == STEPS;
                        let next = if last { tr("Finish") } else { tr("Next") };
                        if dialog::primary(ui, next, true).clicked() {
                            if last {
                                finish = true;
                            } else {
                                go = Some(self.step + 1);
                            }
                        }
                        if self.step > 0 && dialog::secondary(ui, tr("Back")).clicked() {
                            go = Some(self.step - 1);
                        }
                    });
                }
            });
        if skip {
            open = false;
        }
        if let Some(step) = go {
            self.step = step;
        }
        if finish {
            if audiostrada_license::OFFICIAL {
                prefs.check_updates = Some(self.check_updates);
            }
            let window = (self.arrangement == Arrangement::TwoScreens)
                .then(|| mixer_window(&self.screens, ctx))
                .flatten();
            outcome.layout = Some((self.arrangement, window));
            outcome.ui_changed = true;
            open = false;
        }
        self.open = open;
        outcome
    }

    fn layout_step(&mut self, ui: &mut egui::Ui) {
        let two = self.screens.len() > 1;
        dialog::section(ui, tr("One screen"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                for start in Start::ALL {
                    let arrangement = Arrangement::One(start);
                    if layout_card(ui, arrangement, self.arrangement == arrangement, true).clicked()
                    {
                        self.arrangement = arrangement;
                    }
                }
            });
        });
        ui.add_space(8.0);
        dialog::section(ui, tr("Two screens"), |ui| {
            let chosen = self.arrangement == Arrangement::TwoScreens;
            let card = layout_card(ui, Arrangement::TwoScreens, chosen, two);
            if two && card.clicked() {
                self.arrangement = Arrangement::TwoScreens;
            }
            if !two {
                dialog::hint(
                    ui,
                    tr(
                        "Only one screen was found. Plug in another one, then open this assistant again from the Help menu.",
                    ),
                );
            }
        });
        if audiostrada_license::OFFICIAL {
            ui.add_space(8.0);
            ui.checkbox(
                &mut self.check_updates,
                tr("Check once a day whether a new version is out"),
            )
            .on_hover_text(tr(
                "Only a short version file is downloaded from the web site: nothing about you \
                 or your projects is sent.",
            ));
        }
        dialog::hint(
            ui,
            tr("Panels can be moved, shown and hidden at any time from the View menu."),
        );
    }
}

/// The steps, the current one lit.
fn steps_bar(ui: &mut egui::Ui, current: usize) {
    let p = theme::current(ui);
    let names = [tr("Welcome"), tr("Audio"), tr("MIDI"), tr("Layout")];
    ui.horizontal(|ui| {
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("›").color(p.weak));
            }
            let color = if i == current { p.accent } else { p.weak };
            let text = RichText::new(format!("{}  {name}", i + 1)).color(color);
            ui.label(if i == current { text.strong() } else { text });
        }
    });
}

/// Language, theme and shortcuts; true when one changed.
fn welcome(ui: &mut egui::Ui, prefs: &mut UiPrefs) -> bool {
    let mut changed = false;
    // The language is the whole process's: tests running side by side keep English.
    if !cfg!(test) {
        dialog::section(ui, tr("Language"), |ui| {
            ui.horizontal(|ui| {
                for language in crate::i18n::Language::ALL {
                    if ui
                        .selectable_label(prefs.language == language, language.name())
                        .clicked()
                        && prefs.language != language
                    {
                        prefs.language = language;
                        changed = true;
                    }
                }
            });
        });
    }
    ui.add_space(6.0);
    dialog::section(ui, tr("Theme"), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for choice in [ThemeChoice::Dark, ThemeChoice::Studio, ThemeChoice::Light] {
                let palette = choice.palette(&prefs.custom);
                if crate::preferences::theme_card(ui, choice.name(), palette, prefs.theme == choice)
                    .clicked()
                    && prefs.theme != choice
                {
                    prefs.theme = choice;
                    changed = true;
                }
            }
        });
    });
    ui.add_space(6.0);
    dialog::section(ui, tr("Keyboard shortcuts"), |ui| {
        ui.label(
            RichText::new(tr(
                "Coming from another program? Use the shortcuts you know.",
            ))
            .color(theme::current(ui).weak),
        );
        ui.horizontal(|ui| {
            for preset in Preset::ALL {
                if ui
                    .selectable_label(prefs.keymap.preset == preset, preset.name())
                    .clicked()
                    && prefs.keymap.preset != preset
                {
                    prefs.keymap = Keymap::preset(preset);
                    changed = true;
                }
            }
        });
    });
    changed
}

/// A layout's card: a sketch of its panels, its name and what it is for.
fn layout_card(
    ui: &mut egui::Ui,
    arrangement: Arrangement,
    chosen: bool,
    enabled: bool,
) -> egui::Response {
    let p = theme::current(ui);
    let (name, about) = match arrangement {
        Arrangement::One(Start::Classic) => (tr("Classic"), tr("The mixer under the timeline")),
        Arrangement::One(Start::Strip) => (
            tr("Strip at the left"),
            tr("The selected track's strip beside the timeline, as in Logic Pro"),
        ),
        Arrangement::One(Start::Focus) => (
            tr("Uncluttered"),
            tr("The timeline alone, the other panels a key away"),
        ),
        Arrangement::TwoScreens => (
            tr("Mixer on the second screen"),
            tr("The timeline here, the mixer full screen on the other one"),
        ),
    };
    let (rect, response) = ui.allocate_exact_size(vec2(196.0, 158.0), Sense::click());
    let painter = ui.painter_at(rect.expand(2.0));
    let border = if chosen { p.accent } else { p.border };
    painter.rect_filled(rect, 8.0, p.panel);
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(if chosen { 2.0 } else { 1.0 }, border),
        StrokeKind::Inside,
    );
    let sketch = Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(rect.width() - 24.0, 76.0));
    draw_sketch(&painter, sketch, arrangement, &p);
    let text = if enabled { p.text } else { p.weak };
    painter.text(
        pos2(rect.left() + 12.0, sketch.bottom() + 10.0),
        egui::Align2::LEFT_TOP,
        name,
        theme::strong(12.5),
        text,
    );
    let galley = painter.layout(
        about.to_owned(),
        egui::FontId::proportional(10.5),
        p.weak,
        rect.width() - 24.0,
    );
    painter.galley(
        pos2(rect.left() + 12.0, sketch.bottom() + 28.0),
        galley,
        p.weak,
    );
    if !enabled {
        painter.rect_filled(rect, 8.0, p.base.gamma_multiply(0.45));
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// The layout drawn small: the timeline's lanes, the mixer's strips, the strip column.
fn draw_sketch(painter: &egui::Painter, r: Rect, arrangement: Arrangement, p: &theme::Palette) {
    let lanes = |area: Rect| {
        painter.rect_filled(area, 3.0, p.base);
        let rows = 4;
        let h = (area.height() - 6.0) / rows as f32;
        for i in 0..rows {
            let y = area.top() + 3.0 + i as f32 * h;
            let clip = Rect::from_min_size(
                pos2(area.left() + 6.0 + (i as f32 * 9.0) % 24.0, y + 2.0),
                vec2(area.width() * 0.45, h - 4.0),
            );
            painter.rect_filled(clip, 2.0, p.accent.gamma_multiply(0.55));
        }
    };
    let strips = |area: Rect, count: usize| {
        painter.rect_filled(area, 3.0, p.base);
        let w = (area.width() - 4.0) / count as f32;
        for i in 0..count {
            let x = area.left() + 2.0 + i as f32 * w;
            let strip = Rect::from_min_size(
                pos2(x + 1.0, area.top() + 3.0),
                vec2(w - 2.0, area.height() - 6.0),
            );
            painter.rect_filled(strip, 2.0, p.surface);
            let fader = Rect::from_center_size(
                pos2(strip.center().x, strip.bottom() - strip.height() * 0.35),
                vec2(3.0, strip.height() * 0.4),
            );
            painter.rect_filled(fader, 1.0, p.weak);
        }
    };
    let screen = |area: Rect| {
        painter.rect_stroke(area, 4.0, Stroke::new(1.0, p.border), StrokeKind::Outside);
    };
    match arrangement {
        Arrangement::One(Start::Classic) => {
            let split = r.top() + r.height() * 0.55;
            lanes(Rect::from_min_max(r.min, pos2(r.right(), split - 2.0)));
            strips(Rect::from_min_max(pos2(r.left(), split), r.max), 9);
            screen(r);
        }
        Arrangement::One(Start::Strip) => {
            let split = r.left() + r.width() * 0.24;
            strips(Rect::from_min_max(r.min, pos2(split - 2.0, r.bottom())), 2);
            lanes(Rect::from_min_max(pos2(split, r.top()), r.max));
            screen(r);
        }
        Arrangement::One(Start::Focus) => {
            lanes(r);
            screen(r);
        }
        Arrangement::TwoScreens => {
            let half = (r.width() - 8.0) / 2.0;
            let left = Rect::from_min_size(r.min, vec2(half, r.height()));
            let right =
                Rect::from_min_size(pos2(r.left() + half + 8.0, r.top()), vec2(half, r.height()));
            lanes(left);
            strips(right, 6);
            screen(left);
            screen(right);
        }
    }
}

/// The mixer's window on a screen other than the main window's, maximized there.
pub(crate) fn mixer_window(screens: &[Rect], ctx: &egui::Context) -> Option<WindowState> {
    // Screens come in window units; positions are kept in the interface's points.
    let zoom = ctx.zoom_factor().max(0.25);
    let points: Vec<Rect> = screens
        .iter()
        .map(|s| {
            Rect::from_min_max(
                (s.min.to_vec2() / zoom).to_pos2(),
                (s.max.to_vec2() / zoom).to_pos2(),
            )
        })
        .collect();
    let main = ctx.input(|i| i.viewport().outer_rect).map(|r| r.center());
    other_screen(&points, main).map(|s| WindowState {
        size: [1100.0, 560.0],
        position: Some([s.left() + 40.0, s.top() + 40.0]),
        maximized: true,
    })
}

/// A screen that does not hold `main`, else the second one listed.
fn other_screen(screens: &[Rect], main: Option<Pos2>) -> Option<Rect> {
    if screens.len() < 2 {
        return None;
    }
    main.and_then(|m| screens.iter().find(|s| !s.contains(m)).copied())
        .or(screens.get(1).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mixer_goes_to_the_screen_the_window_is_not_on() {
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0));
        let right = Rect::from_min_size(pos2(1920.0, 0.0), vec2(2560.0, 1440.0));
        assert_eq!(
            other_screen(&[left, right], Some(pos2(900.0, 500.0))),
            Some(right)
        );
        assert_eq!(
            other_screen(&[left, right], Some(pos2(2500.0, 500.0))),
            Some(left)
        );
        assert_eq!(other_screen(&[left, right], None), Some(right));
        assert_eq!(other_screen(&[left], Some(pos2(900.0, 500.0))), None);
        assert_eq!(other_screen(&[], None), None);
    }
}
