//! Transport bar: playback controls, the position counter (its formats on a right click),
//! tempo and time signature. The edit mode and grid types live here, shown by the timeline.

use audiostrada_core::{Project, edit};
use audiostrada_engine::tempo::TempoMap;
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::actions::Action;
use crate::document::Document;
use crate::i18n::tr;
use crate::keymap::Keymap;
use crate::mixer::{end_gesture, format_time};
use crate::theme::{self, Icon};
use crate::trf;

/// How the counter tells a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeFormat {
    /// Bars, beats and ticks.
    #[default]
    Bars,
    /// Minutes, seconds and milliseconds.
    Time,
    /// Hours, minutes, seconds and frames at this many frames per second.
    Timecode(u32),
    Samples,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 6] = [
        TimeFormat::Bars,
        TimeFormat::Time,
        TimeFormat::Timecode(24),
        TimeFormat::Timecode(25),
        TimeFormat::Timecode(30),
        TimeFormat::Samples,
    ];

    pub fn name(self) -> String {
        match self {
            TimeFormat::Bars => tr("Bars and beats").to_owned(),
            TimeFormat::Time => tr("Minutes and seconds").to_owned(),
            TimeFormat::Timecode(fps) => trf!("Timecode, {fps} fps", fps = fps),
            TimeFormat::Samples => tr("Samples").to_owned(),
        }
    }

    pub fn format(self, project: &Project, position: u64) -> String {
        let rate = project.sample_rate.max(1);
        match self {
            TimeFormat::Bars => {
                let tempo = &project.tempo;
                let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(position, rate));
                format!("{:>3}.{}.{:03}", bbt.bar, bbt.beat, bbt.tick)
            }
            TimeFormat::Time => format_time(position, rate),
            TimeFormat::Timecode(fps) => {
                let frames = position * u64::from(fps) / u64::from(rate);
                let seconds = frames / u64::from(fps);
                format!(
                    "{:02}:{:02}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60,
                    frames % u64::from(fps)
                )
            }
            TimeFormat::Samples => position.to_string(),
        }
    }
}

/// The counter's large line and the small one under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counter {
    pub main: TimeFormat,
    pub second: Option<TimeFormat>,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            main: TimeFormat::Bars,
            second: Some(TimeFormat::Time),
        }
    }
}

/// Height of everything on the transport bar.
const HEIGHT: f32 = 40.0;

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
    keymap: &Keymap,
    mac: bool,
    recording: bool,
    counter: &mut Counter,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let looping = doc.project().looping().is_some();
    let tempo = doc.project().tempo.clone();
    let position = doc.player().position();
    let playing = doc.player().engine().playing();
    let hint = |action: Action| match keymap.label(action, mac) {
        Some(label) => format!("{} ({label})", action.label()),
        None => action.label().to_owned(),
    };
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.set_height(HEIGHT);
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
            .inner_margin(egui::Margin::symmetric(5, 6));
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
                        let low = doc.player().low_latency();
                        let bypassed = doc.player().low_latency_bypassed().len();
                        let low_hint = if low {
                            trf!(
                                "{action}: on, {count} effect(s) bypassed. Effects adding more \
                                 than 1 ms are bypassed, except on the tracks being played, so \
                                 that what you play is heard at once",
                                action = hint(Action::LowLatency),
                                count = bypassed
                            )
                        } else {
                            trf!(
                                "{action}: bypasses the effects adding more than 1 ms (except on \
                                 the tracks being played), so that what you play is heard at once",
                                action = hint(Action::LowLatency)
                            )
                        };
                        if theme::icon_button(ui, Icon::Latency, low, theme::MONITOR, &low_hint).clicked() {
                            actions.push(Action::LowLatency);
                        }
            });
        });
        ui.add_space(10.0);

        ui.add_space(4.0);
        counter_well(ui, doc, counter, position, playing, recording);
        ui.add_space(2.0);
        tempo_well(ui, doc, &tempo);
    });
    ui.add_space(5.0);
    actions
}

/// The position, large in the main format and small in the second; a right click chooses
/// them.
fn counter_well(
    ui: &mut Ui,
    doc: &Document,
    counter: &mut Counter,
    position: u64,
    playing: bool,
    recording: bool,
) {
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    let (rect, response) = ui.allocate_exact_size(vec2(196.0, HEIGHT), Sense::click());
    theme::well(ui.painter(), rect, dark);
    let color = if recording {
        theme::RECORD
    } else if playing {
        theme::PLAY
    } else {
        p.text
    };
    let project = doc.project();
    let main_y = if counter.second.is_some() {
        rect.top() + 15.0
    } else {
        rect.center().y
    };
    ui.painter().text(
        pos2(rect.right() - 10.0, main_y),
        Align2::RIGHT_CENTER,
        counter.main.format(project, position),
        FontId::monospace(19.0),
        color,
    );
    if let Some(second) = counter.second {
        ui.painter().text(
            pos2(rect.right() - 10.0, rect.bottom() - 9.0),
            Align2::RIGHT_CENTER,
            second.format(project, position),
            FontId::monospace(10.5),
            p.weak,
        );
    }
    let response = response.on_hover_text(tr("Right-click to choose how the position is shown"));
    response.context_menu(|ui| {
        ui.label(RichText::new(tr("Main display")).small().weak());
        for format in TimeFormat::ALL {
            if ui
                .selectable_label(counter.main == format, format.name())
                .clicked()
            {
                counter.main = format;
                if counter.second == Some(format) {
                    counter.second = None;
                }
                ui.close();
            }
        }
        ui.separator();
        ui.label(RichText::new(tr("Second line")).small().weak());
        if ui
            .selectable_label(counter.second.is_none(), tr("None"))
            .clicked()
        {
            counter.second = None;
            ui.close();
        }
        for format in TimeFormat::ALL.into_iter().filter(|f| *f != counter.main) {
            if ui
                .selectable_label(counter.second == Some(format), format.name())
                .clicked()
            {
                counter.second = Some(format);
                ui.close();
            }
        }
    });
}

/// Tempo and time signature, each over its caption.
fn tempo_well(ui: &mut Ui, doc: &mut Document, tempo: &TempoMap) {
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    let mut bpm = tempo.bpm_at_beat(0.0);
    let signature = tempo.signature_at_beat(0.0);
    let (mut numerator, mut denominator) = (signature.numerator, signature.denominator);
    let (rect, _) = ui.allocate_exact_size(vec2(178.0, HEIGHT), Sense::hover());
    theme::well(ui.painter(), rect, dark);
    let divider = rect.left() + 74.0;
    ui.painter().vline(
        divider,
        rect.top() + 7.0..=rect.bottom() - 7.0,
        egui::Stroke::new(1.0, p.border),
    );
    let caption = |x: f32, text: &str| {
        ui.painter().text(
            pos2(x, rect.bottom() - 7.0),
            Align2::CENTER_CENTER,
            text.to_uppercase(),
            FontId::proportional(8.5),
            p.weak,
        );
    };
    caption((rect.left() + divider) / 2.0, tr("BPM"));
    caption((divider + rect.right()) / 2.0, tr("Signature"));
    let top = rect.top() + 4.0;
    let row = 20.0;
    let tempo_cell = Rect::from_min_size(pos2(rect.left() + 8.0, top), vec2(58.0, row));
    let response = ui
        .put(
            tempo_cell,
            egui::DragValue::new(&mut bpm)
                .range(20.0..=400.0)
                .speed(0.5)
                .fixed_decimals(1),
        )
        .on_hover_text(tr("Tempo (drag or type)"));
    let mut sig = None;
    let mut den_changed = false;
    let cell = Rect::from_min_size(
        pos2(divider + 10.0, top),
        vec2(rect.right() - divider - 18.0, row),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(cell), |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            sig = Some(
                ui.add(egui::DragValue::new(&mut numerator).range(1..=32))
                    .on_hover_text(tr("Beats per bar")),
            );
            ui.label(RichText::new("/").color(p.weak));
            egui::ComboBox::from_id_salt("signature-denominator")
                .width(34.0)
                .selected_text(denominator.to_string())
                .show_ui(ui, |ui| {
                    for d in [2, 4, 8, 16] {
                        den_changed |= ui
                            .selectable_value(&mut denominator, d, d.to_string())
                            .changed();
                    }
                });
        });
    });
    let sig_changed = sig.as_ref().is_some_and(|r| r.changed());
    if response.changed() || sig_changed || den_changed {
        doc.history_mut().edit_merged(tr("Tempo"), "tempo", |p| {
            let tempo = TempoMap::constant(bpm, numerator, denominator);
            audiostrada_core::edit::set_tempo(p, tempo);
        });
    }
    end_gesture(doc.history_mut(), &response);
    if let Some(sig) = &sig {
        end_gesture(doc.history_mut(), sig);
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

    #[test]
    fn the_counter_formats() {
        let project = Project::new("t", 48_000);
        // 61.5 s at 120 BPM in 4/4: 123 beats, bar 31, beat 4.
        let at = 48_000 * 61 + 24_000;
        assert_eq!(TimeFormat::Bars.format(&project, at), " 31.4.000");
        assert_eq!(TimeFormat::Time.format(&project, at), "01:01.500");
        assert_eq!(TimeFormat::Timecode(25).format(&project, at), "00:01:01:12");
        assert_eq!(TimeFormat::Samples.format(&project, at), "2952000");
    }
}
