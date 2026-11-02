//! Transport bar: playback controls, position, tempo, edit mode and grid.

use audiostrada_core::{Project, edit};
use audiostrada_engine::tempo::TempoMap;
use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::actions::Action;
use crate::document::Document;
use crate::i18n::tr;
use crate::keymap::Keymap;
use crate::mixer::{end_gesture, format_time};
use crate::theme::{self, Icon};
use crate::trf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EditMode {
    /// Free placement.
    #[default]
    Slip,
    /// Clips stay butted: removing or moving closes and opens gaps.
    Shuffle,
    /// Positions snap to the grid.
    Grid,
}

impl EditMode {
    pub const ALL: [EditMode; 3] = [EditMode::Slip, EditMode::Shuffle, EditMode::Grid];

    pub fn name(self) -> &'static str {
        match self {
            EditMode::Slip => "Slip",
            EditMode::Shuffle => "Shuffle",
            EditMode::Grid => tr("Grid"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Grid {
    Bar,
    Half,
    #[default]
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl Grid {
    pub const ALL: [Grid; 6] = [
        Grid::Bar,
        Grid::Half,
        Grid::Quarter,
        Grid::Eighth,
        Grid::Sixteenth,
        Grid::ThirtySecond,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Grid::Bar => tr("1 bar"),
            Grid::Half => "1/2",
            Grid::Quarter => "1/4",
            Grid::Eighth => "1/8",
            Grid::Sixteenth => "1/16",
            Grid::ThirtySecond => "1/32",
        }
    }

    /// Grid step in quarter-note beats; `None` for whole bars (signature dependent).
    pub fn beats(self) -> Option<f64> {
        match self {
            Grid::Bar => None,
            Grid::Half => Some(2.0),
            Grid::Quarter => Some(1.0),
            Grid::Eighth => Some(0.5),
            Grid::Sixteenth => Some(0.25),
            Grid::ThirtySecond => Some(0.125),
        }
    }

    pub fn finer(self) -> Self {
        let i = Self::ALL.iter().position(|g| *g == self).unwrap_or(0);
        Self::ALL[(i + 1).min(Self::ALL.len() - 1)]
    }

    pub fn coarser(self) -> Self {
        let i = Self::ALL.iter().position(|g| *g == self).unwrap_or(0);
        Self::ALL[i.saturating_sub(1)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EditSettings {
    pub mode: EditMode,
    pub grid: Grid,
    pub snap: bool,
}

impl EditSettings {
    /// Snaps when the grid mode or the snap toggle is active.
    pub fn snapping(&self) -> bool {
        self.mode == EditMode::Grid || self.snap
    }

    /// `position` on the nearest grid line when snapping, else unchanged.
    pub fn snap(&self, project: &Project, position: u64) -> u64 {
        if self.snapping() {
            edit::snap(
                position,
                self.grid.beats(),
                &project.tempo,
                project.sample_rate,
            )
        } else {
            position
        }
    }

    /// Length of one grid step from `position`, in samples.
    pub fn grid_step(&self, project: &Project, position: u64) -> u64 {
        step_length(project, position, self.grid.beats())
    }
}

/// Length in samples of `beats` quarter notes from `position`, or of a bar for `None`.
pub fn step_length(project: &Project, position: u64, beats: Option<f64>) -> u64 {
    let (tempo, rate) = (&project.tempo, project.sample_rate);
    let beat = tempo.sample_to_beat(position, rate);
    let step = beats.unwrap_or_else(|| tempo.signature_at_beat(beat).beats_per_bar());
    tempo
        .beat_to_sample(beat + step, rate)
        .saturating_sub(tempo.beat_to_sample(beat, rate))
}

/// Returns the actions triggered by the transport buttons this frame.
pub fn show(
    ui: &mut Ui,
    doc: &mut Document,
    edit: &mut EditSettings,
    keymap: &Keymap,
    mac: bool,
    recording: bool,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let rate = doc.project().sample_rate;
    let looping = doc.project().looping().is_some();
    let tempo = doc.project().tempo.clone();
    let mut bpm = tempo.bpm_at_beat(0.0);
    let signature = tempo.signature_at_beat(0.0);
    let position = doc.player().position();
    let playing = doc.player().engine().playing();
    let hint = |action: Action| match keymap.label(action, mac) {
        Some(label) => format!("{} ({label})", action.label()),
        None => action.label().to_owned(),
    };
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let icon = |ui: &mut Ui, icon: Icon, on: bool, tint: Color32, action: Action| {
            theme::icon_button(ui, icon, on, tint, &hint(action))
                .clicked()
                .then_some(action)
        };
        // The buttons sit in groups on a sunken strip, as the counter.
        let group = egui::Frame::new()
            .fill(p.base)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(7.0)
            .inner_margin(egui::Margin::same(3));
        group.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                        actions.extend(icon(ui, Icon::ToStart, false, p.accent, Action::GoToStart));
                        actions.extend(icon(
                            ui,
                            if playing { Icon::Pause } else { Icon::Play },
                            playing,
                            theme::PLAY,
                            Action::PlayPause,
                        ));
                        actions.extend(icon(ui, Icon::Stop, false, p.accent, Action::Stop));
                        actions.extend(icon(ui, Icon::Record, recording, theme::RECORD, Action::Record));
            });
        });
        ui.add_space(6.0);
        group.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                        actions.extend(icon(ui, Icon::Loop, looping, p.accent, Action::ToggleLoop));
                        let punch = doc.project().punch_range.is_some();
                        actions.extend(icon(ui, Icon::Punch, punch, theme::RECORD, Action::TogglePunch));
                        let click = doc.project().metronome.clone();
                        let response = theme::icon_button(
                            ui,
                            Icon::Metronome,
                            click.enabled,
                            p.accent,
                            &trf!(
                                "{action}. Right-click for the metronome settings",
                                action = hint(Action::ToggleMetronome)
                            ),
                        );
                        if response.clicked() {
                            actions.push(Action::ToggleMetronome);
                        }
                        if response.secondary_clicked() {
                            crate::metronome_window::open(ui.ctx());
                        }
                        // Reference track: lit while it is heard instead of the mix.
                        let listening = doc.player().reference_listening();
                        let text = RichText::new(tr("REF")).size(11.0).strong();
                        let button = if listening {
                            egui::Button::new(text.color(Color32::WHITE)).fill(p.accent.gamma_multiply(0.85))
                        } else {
                            egui::Button::new(text)
                        };
                        let reference_hint = if doc.project().reference.is_some() {
                            tr(
                                "Click to hear the reference track instead of the mix, at the same loudness; \
                                right-click for its settings",
                            )
                        } else {
                            tr("Choose a reference track to compare the mix with")
                        };
                        let response = ui
                            .add_sized([38.0, 26.0], button)
                            .on_hover_text(reference_hint);
                        if response.clicked() {
                            actions.push(Action::ListenToReference);
                        }
                        if response.secondary_clicked() {
                            crate::reference_window::open(ui.ctx());
                        }
            });
        });
        ui.add_space(10.0);

        // Counter: bars and beats large, clock time below.
        let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(position, rate));
        let (rect, _) = ui.allocate_exact_size(vec2(186.0, 40.0), Sense::hover());
        theme::well(ui.painter(), rect, dark);
        let color = if recording {
            theme::RECORD
        } else if playing {
            theme::PLAY
        } else {
            p.text
        };
        ui.painter().text(
            pos2(rect.left() + 10.0, rect.top() + 2.0),
            Align2::LEFT_TOP,
            format!("{:>3}.{}.{:03}", bbt.bar, bbt.beat, bbt.tick),
            FontId::monospace(20.0),
            color,
        );
        ui.painter().text(
            pos2(rect.right() - 8.0, rect.bottom() - 2.0),
            Align2::RIGHT_BOTTOM,
            format_time(position, rate),
            FontId::monospace(10.5),
            p.weak,
        );

        // Tempo and time signature.
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(p.base)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.horizontal(|ui| {
                        let response = ui
                            .add(
                                egui::DragValue::new(&mut bpm)
                                    .range(20.0..=400.0)
                                    .speed(0.5)
                                    .fixed_decimals(1),
                            )
                            .on_hover_text(tr("Tempo (drag or type)"));
                        let mut numerator = signature.numerator;
                        let mut denominator = signature.denominator;
                        let sig = ui
                            .add(egui::DragValue::new(&mut numerator).range(1..=32))
                            .on_hover_text(tr("Beats per bar"));
                        ui.label(RichText::new("/").color(p.weak));
                        let mut den_changed = false;
                        egui::ComboBox::from_id_salt("signature-denominator")
                            .width(36.0)
                            .selected_text(denominator.to_string())
                            .show_ui(ui, |ui| {
                                for d in [2, 4, 8, 16] {
                                    den_changed |= ui
                                        .selectable_value(&mut denominator, d, d.to_string())
                                        .changed();
                                }
                            });
                        if response.changed() || sig.changed() || den_changed {
                            doc.history_mut().edit_merged(tr("Tempo"), "tempo", |p| {
                                let tempo = TempoMap::constant(bpm, numerator, denominator);
                                audiostrada_core::edit::set_tempo(p, tempo);
                            });
                        }
                        end_gesture(doc.history_mut(), &response);
                        end_gesture(doc.history_mut(), &sig);
                    });
                    ui.label(
                        RichText::new(tr("BPM        SIGNATURE"))
                            .size(8.5)
                            .color(p.weak),
                    );
                });
            });
        ui.add_space(10.0);

        // Edit mode, grid and snap.
        segmented(ui, &mut edit.mode, &hint);
        egui::ComboBox::from_id_salt("grid")
            .width(64.0)
            .selected_text(edit.grid.name())
            .show_ui(ui, |ui| {
                for grid in Grid::ALL {
                    ui.selectable_value(&mut edit.grid, grid, grid.name());
                }
            })
            .response
            .on_hover_text(tr("Grid value"));
        if theme::icon_button(
            ui,
            Icon::Snap,
            edit.snap,
            p.accent,
            &hint(Action::ToggleSnap),
        )
        .clicked()
        {
            edit.snap = !edit.snap;
        }
    });
    ui.add_space(3.0);
    actions
}

/// Slip / Shuffle / Grid as one segmented control.
fn segmented(ui: &mut Ui, mode: &mut EditMode, hint: &dyn Fn(Action) -> String) {
    let p = theme::current(ui);
    let item = vec2(58.0, 24.0);
    let (rect, _) = ui.allocate_exact_size(vec2(item.x * 3.0, item.y), Sense::hover());
    ui.painter().rect_filled(rect, 5.0, p.base);
    for (i, m) in EditMode::ALL.into_iter().enumerate() {
        let r = egui::Rect::from_min_size(pos2(rect.left() + item.x * i as f32, rect.top()), item)
            .shrink(2.0);
        let response = ui.interact(r, ui.scope_id().with(("edit-mode", i)), Sense::click());
        let action = match m {
            EditMode::Slip => Action::ModeSlip,
            EditMode::Shuffle => Action::ModeShuffle,
            EditMode::Grid => Action::ModeGrid,
        };
        let on = *mode == m;
        if on {
            ui.painter()
                .rect_filled(r, 4.0, p.accent.gamma_multiply(0.85));
        } else if response.hovered() {
            ui.painter().rect_filled(r, 4.0, p.hover);
        }
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            m.name(),
            theme::strong(12.0),
            if on { Color32::WHITE } else { p.weak },
        );
        if response.on_hover_text(hint(action)).clicked() {
            *mode = m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_steps() {
        assert_eq!(Grid::Quarter.finer(), Grid::Eighth);
        assert_eq!(Grid::ThirtySecond.finer(), Grid::ThirtySecond);
        assert_eq!(Grid::Bar.coarser(), Grid::Bar);
        assert_eq!(Grid::Sixteenth.beats(), Some(0.25));
        let settings = EditSettings::default();
        assert!(!settings.snapping());
        assert!(
            EditSettings {
                mode: EditMode::Grid,
                ..settings
            }
            .snapping()
        );
    }

    #[test]
    fn snaps_and_steps_follow_the_grid_and_the_signature() {
        let mut project = Project::new("t", 48_000);
        let free = EditSettings::default();
        assert_eq!(free.snap(&project, 30_000), 30_000, "no snapping");
        let snapping = EditSettings { snap: true, ..free };
        assert_eq!(snapping.snap(&project, 30_000), 24_000);
        assert_eq!(snapping.snap(&project, 37_000), 48_000);
        assert_eq!(snapping.grid_step(&project, 1_000), 24_000);
        let bars = EditSettings {
            grid: Grid::Bar,
            ..snapping
        };
        assert_eq!(bars.grid_step(&project, 0), 96_000);
        assert_eq!(bars.snap(&project, 60_000), 96_000);
        project.tempo = TempoMap::constant(120.0, 6, 8);
        assert_eq!(bars.grid_step(&project, 0), 72_000);
        assert_eq!(step_length(&project, 0, Some(0.5)), 12_000);
    }
}
