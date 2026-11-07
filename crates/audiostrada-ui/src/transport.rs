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
use crate::widgets::METER_FLOOR;
use audiostrada_io::StreamStatus;

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
/// Size of the large transport, relative to the normal one.
pub const LARGE: f32 = 1.4;

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

/// What positions pull edits while snapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapTargets {
    pub grid: bool,
    pub clips: bool,
    pub markers: bool,
    pub playhead: bool,
}

impl Default for SnapTargets {
    fn default() -> Self {
        Self {
            grid: true,
            clips: true,
            markers: true,
            playhead: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EditSettings {
    pub mode: EditMode,
    pub grid: Grid,
    pub snap: bool,
    #[serde(default)]
    pub targets: SnapTargets,
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
/// The audio stream as the transport shows it, and whether its settings were asked for.
pub struct Audio<'a> {
    pub status: Option<&'a StreamStatus>,
    pub open_settings: bool,
    pub recording_mode: bool,
}

pub fn show(
    ui: &mut Ui,
    doc: &mut Document,
    keymap: &Keymap,
    mac: bool,
    (recording, audio): (bool, &mut Audio<'_>),
    counter: &mut Counter,
    k: f32,
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
    ui.add_space(5.0 * k);
    ui.horizontal(|ui| {
        ui.set_height(HEIGHT * k);
        ui.spacing_mut().item_spacing.x = 4.0 * k;
        let icon = |ui: &mut Ui, icon: Icon, on: bool, tint: Color32, action: Action| {
            theme::icon_button(ui, icon, on, tint, &hint(action), k)
                .clicked()
                .then_some(action)
        };
        // The buttons sit in groups on a sunken strip, as the counter.
        let group = egui::Frame::new()
            .fill(p.base)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(7.0 * k)
            .inner_margin(egui::Margin::symmetric((5.0 * k) as i8, (6.0 * k) as i8));
        group.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0 * k;
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
                        // Panic: every instrument lets go of every note.
                        let panic = egui::Button::new(RichText::new(tr("Panic")).size(11.0 * k).strong());
                        if ui
                            .add_sized([52.0 * k, 26.0 * k], panic)
                            .on_hover_text(trf!(
                                "{action}: stops every note still sounding and silences the \
                                 effects' tails",
                                action = hint(Action::Panic)
                            ))
                            .clicked()
                        {
                            actions.push(Action::Panic);
                        }
            });
        });
        ui.add_space(6.0 * k);
        group.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0 * k;
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
                            k,
                        );
                        if response.clicked() {
                            actions.push(Action::ToggleMetronome);
                        }
                        if response.secondary_clicked() {
                            crate::metronome_window::open(ui.ctx());
                        }
                        // Reference track: lit while it is heard instead of the mix.
                        let listening = doc.player().reference_listening();
                        let text = RichText::new(tr("REF")).size(11.0 * k).strong();
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
                            .add_sized([38.0 * k, 26.0 * k], button)
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
                                 than 1 ms on the master and on the folders of the tracks being \
                                 played are bypassed, so that what you play is heard at once",
                                action = hint(Action::LowLatency),
                                count = bypassed
                            )
                        } else {
                            trf!(
                                "{action}: bypasses the effects adding more than 1 ms on the \
                                 master and on the folders of the tracks being played, so that \
                                 what you play is heard at once",
                                action = hint(Action::LowLatency)
                            )
                        };
                        if theme::icon_button(ui, Icon::Latency, low, theme::MONITOR, &low_hint, k).clicked() {
                            actions.push(Action::LowLatency);
                        }
            });
        });
        ui.add_space(14.0 * k);
        counter_well(ui, doc, counter, (position, playing, recording), k);
        ui.add_space(2.0 * k);
        let well = tempo_well(ui, doc, &tempo, k);
        // Its tap button is put inside it, which leaves the cursor behind.
        ui.add_space((well.right() + 10.0 * k - ui.cursor().left()).max(0.0));
        // With its name when there is room beside the meters, else as an icon, else in the
        // View menu only.
        let room = ui.available_width() - 410.0 * k;
        let toggle = if room > 170.0 {
            Some(theme::labeled_toggle(
                ui,
                Icon::Microphone,
                tr("Recording mode"),
                audio.recording_mode,
                theme::RECORD,
                &hint(Action::RecordingMode),
            ))
        } else if room > 36.0 * k {
            Some(theme::icon_button(
                ui,
                Icon::Microphone,
                audio.recording_mode,
                theme::RECORD,
                &hint(Action::RecordingMode),
                k,
            ))
        } else {
            None
        };
        if toggle.is_some_and(|t| t.clicked()) {
            actions.push(Action::RecordingMode);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(4.0 * k);
            master_well(ui, doc, k);
            ui.add_space(4.0 * k);
            if let Some(status) = audio.status {
                audio.open_settings |= load_well(ui, status, k);
            }
        });
    });
    ui.add_space(5.0 * k);
    actions
}

/// The position, large in the main format and small in the second; a right click chooses
/// them.
fn counter_well(
    ui: &mut Ui,
    doc: &Document,
    counter: &mut Counter,
    (position, playing, recording): (u64, bool, bool),
    k: f32,
) {
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    let (rect, response) = ui.allocate_exact_size(vec2(196.0, HEIGHT) * k, Sense::click());
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
        rect.top() + 15.0 * k
    } else {
        rect.center().y
    };
    ui.painter().text(
        pos2(rect.right() - 10.0 * k, main_y),
        Align2::RIGHT_CENTER,
        counter.main.format(project, position),
        FontId::monospace(19.0 * k),
        color,
    );
    if let Some(second) = counter.second {
        ui.painter().text(
            pos2(rect.right() - 10.0 * k, rect.bottom() - 9.0 * k),
            Align2::RIGHT_CENTER,
            second.format(project, position),
            FontId::monospace(10.5 * k),
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

/// The master's level: both channels' peaks with their holds, the clip light (a click clears
/// it) and the short-term loudness.
fn master_well(ui: &mut Ui, doc: &mut Document, k: f32) {
    doc.update_levels();
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    let mut level = doc.master_level();
    let (rect, response) = ui.allocate_exact_size(vec2(264.0, HEIGHT) * k, Sense::click());
    theme::well(ui.painter(), rect, dark);
    let painter = ui.painter_at(rect);
    let small = FontId::proportional(8.5 * k);
    let bars = Rect::from_min_max(
        pos2(rect.left() + 20.0 * k, rect.top() + 7.0 * k),
        pos2(rect.right() - 92.0 * k, rect.top() + 23.0 * k),
    );
    let x_of = |db: f32| bars.left() + bars.width() * fraction(db);
    for (ch, name) in ["L", "R"].into_iter().enumerate() {
        let bar = Rect::from_min_size(
            pos2(bars.left(), bars.top() + ch as f32 * 9.0 * k),
            vec2(bars.width(), 7.0 * k),
        );
        painter.rect_filled(bar, 1.0, Color32::from_black_alpha(90));
        painter.text(
            pos2(rect.left() + 10.0 * k, bar.center().y),
            Align2::CENTER_CENTER,
            name,
            small.clone(),
            p.weak,
        );
        let lit = Rect::from_min_max(bar.min, pos2(x_of(level.level[ch]), bar.bottom()));
        if lit.width() > 0.0 {
            gradient(&painter.with_clip_rect(lit), bar);
        }
        if level.hold[ch] > METER_FLOOR {
            let x = x_of(level.hold[ch]);
            painter.line_segment(
                [pos2(x, bar.top()), pos2(x, bar.bottom())],
                egui::Stroke::new(1.5 * k, Color32::from_gray(230)),
            );
        }
    }
    for db in [-48.0f32, -24.0, -12.0, -6.0, -3.0, 0.0] {
        painter.text(
            pos2(x_of(db), bars.bottom() + 8.0 * k),
            Align2::CENTER_CENTER,
            format!("{}", db.abs()),
            small.clone(),
            p.weak,
        );
    }
    // The clip light, lit until clicked.
    let clip = Rect::from_center_size(
        pos2(bars.right() + 10.0 * k, bars.center().y),
        vec2(8.0, 13.0) * k,
    );
    painter.rect_filled(
        clip,
        2.0,
        if level.clipped {
            theme::RECORD
        } else {
            Color32::from_black_alpha(90)
        },
    );
    let loudness = doc.player().loudness().short_term();
    let shown = match loudness {
        Some(v) if v.is_finite() && v > -70.0 => format!("{v:.1}"),
        _ => "--".to_owned(),
    };
    let loud = loudness.is_some_and(|v| v > -9.0);
    painter.text(
        pos2(rect.right() - 10.0 * k, rect.top() + 15.0 * k),
        Align2::RIGHT_CENTER,
        shown,
        FontId::monospace(16.0 * k),
        if loud { theme::RECORD } else { p.text },
    );
    painter.text(
        pos2(rect.right() - 10.0 * k, rect.bottom() - 9.0 * k),
        Align2::RIGHT_CENTER,
        tr("LUFS short-term"),
        small,
        p.weak,
    );
    if response.clicked() {
        level.clipped = false;
        doc.set_master_level(level);
    }
    response.on_hover_text(trf!(
        "Master: peak {left} / {right} dBFS, short-term loudness (3 s) in LUFS. Click to clear \
         the clip light",
        left = format!("{:.1}", level.hold[0]),
        right = format!("{:.1}", level.hold[1])
    ));
}

/// Where a level sits on the master's bar: -60 to 0 dB, the top decibels spread wider.
fn fraction(db: f32) -> f32 {
    (1.0 - (-db).clamp(0.0, -METER_FLOOR) / -METER_FLOOR).powi(2)
}

/// Green to yellow to red along a horizontal bar, as the vertical meters.
fn gradient(painter: &egui::Painter, bar: Rect) {
    let x_of = |db: f32| bar.left() + bar.width() * fraction(db);
    let stops = [
        (METER_FLOOR, Color32::from_rgb(40, 150, 90)),
        (-18.0, Color32::from_rgb(80, 200, 120)),
        (-9.0, Color32::from_rgb(200, 210, 80)),
        (-3.0, Color32::from_rgb(240, 170, 60)),
        (0.0, Color32::from_rgb(240, 80, 70)),
    ];
    let mut mesh = egui::Mesh::default();
    for w in stops.windows(2) {
        let (x0, x1) = (x_of(w[0].0), x_of(w[1].0));
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x0, bar.top()), w[0].1);
        mesh.colored_vertex(pos2(x1, bar.top()), w[1].1);
        mesh.colored_vertex(pos2(x1, bar.bottom()), w[1].1);
        mesh.colored_vertex(pos2(x0, bar.bottom()), w[0].1);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The audio processing load, its recent peak and the dropouts; a click opens the audio
/// settings.
fn load_well(ui: &mut Ui, status: &StreamStatus, k: f32) -> bool {
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    let (rect, response) = ui.allocate_exact_size(vec2(118.0, HEIGHT) * k, Sense::click());
    // The peak holds for two seconds, then follows.
    let id = ui.scope_id().with("dsp-peak");
    let now = ui.input(|i| i.time);
    let fresh = status.take_load_peak();
    let (mut peak, mut since) = ui
        .data(|d| d.get_temp::<(f32, f64)>(id))
        .unwrap_or((0.0, now));
    if fresh >= peak || now - since > 2.0 {
        peak = fresh;
        since = now;
    }
    ui.data_mut(|d| d.insert_temp(id, (peak, since)));
    let load = status.load();
    let xruns = status.xruns();
    theme::well(ui.painter(), rect, dark);
    let painter = ui.painter_at(rect);
    let small = FontId::proportional(8.5 * k);
    let color = |v: f32| {
        if v > 0.9 {
            theme::RECORD
        } else if v > 0.7 {
            Color32::from_rgb(240, 170, 60)
        } else {
            Color32::from_rgb(80, 200, 120)
        }
    };
    painter.text(
        pos2(rect.left() + 10.0 * k, rect.top() + 12.0 * k),
        Align2::LEFT_CENTER,
        tr("DSP"),
        small.clone(),
        p.weak,
    );
    painter.text(
        pos2(rect.right() - 10.0 * k, rect.top() + 12.0 * k),
        Align2::RIGHT_CENTER,
        format!("{:.0} %", load * 100.0),
        FontId::monospace(13.0 * k),
        color(load),
    );
    let bar = Rect::from_min_max(
        pos2(rect.left() + 10.0 * k, rect.top() + 21.0 * k),
        pos2(rect.right() - 10.0 * k, rect.top() + 26.0 * k),
    );
    painter.rect_filled(bar, 1.0, Color32::from_black_alpha(90));
    let x_of = |v: f32| bar.left() + bar.width() * v.clamp(0.0, 1.0);
    painter.rect_filled(
        Rect::from_min_max(bar.min, pos2(x_of(load), bar.bottom())),
        1.0,
        color(load),
    );
    let x = x_of(peak);
    painter.line_segment(
        [pos2(x, bar.top()), pos2(x, bar.bottom())],
        egui::Stroke::new(1.5 * k, color(peak)),
    );
    let (text, tint) = if xruns == 0 {
        (tr("No dropout").to_owned(), p.weak)
    } else {
        (
            trf!("{count} dropouts", count = xruns),
            Color32::from_rgb(240, 170, 60),
        )
    };
    painter.text(
        pos2(rect.left() + 10.0 * k, rect.bottom() - 7.0 * k),
        Align2::LEFT_CENTER,
        text,
        small,
        tint,
    );
    response
        .on_hover_text(trf!(
            "Audio processing: {load} % of each buffer's time on average, {peak} % at most \
             lately. Above 100 %, the sound drops out: raise the buffer size or bypass effects. \
             Click for the audio settings",
            load = format!("{:.0}", load * 100.0),
            peak = format!("{:.0}", peak * 100.0)
        ))
        .clicked()
}

/// Tempo and time signature, each over its caption.
/// Returns where it was drawn.
fn tempo_well(ui: &mut Ui, doc: &mut Document, tempo: &TempoMap, k: f32) -> Rect {
    let dark = ui.visuals().dark_mode;
    let p = theme::palette(dark);
    let mut bpm = tempo.bpm_at_beat(0.0);
    let signature = tempo.signature_at_beat(0.0);
    let (mut numerator, mut denominator) = (signature.numerator, signature.denominator);
    let (whole, _) = ui.allocate_exact_size(vec2(226.0, HEIGHT) * k, Sense::hover());
    theme::well(ui.painter(), whole, dark);
    let rect = Rect::from_min_max(whole.min, pos2(whole.right() - 48.0 * k, whole.bottom()));
    let divider = rect.left() + 74.0 * k;
    for x in [divider, rect.right()] {
        ui.painter().vline(
            x,
            rect.top() + 7.0 * k..=rect.bottom() - 7.0 * k,
            egui::Stroke::new(1.0, p.border),
        );
    }
    // Tap tempo: the beat of the last taps, started over after a pause.
    let tap_cell = Rect::from_center_size(
        pos2((rect.right() + whole.right()) / 2.0, whole.center().y),
        vec2(38.0, HEIGHT - 16.0) * k,
    );
    let tap = ui
        .put(
            tap_cell,
            egui::Button::new(RichText::new(tr("TAP")).size(10.0 * k))
                .wrap_mode(egui::TextWrapMode::Extend),
        )
        .on_hover_text(tr("Tap tempo: click on the beat, at least twice"));
    let id = egui::Id::unique("tap-tempo");
    let mut tapped = None;
    // On the press, not the release: the tap is the beat.
    if tap.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
        let now = ui.input(|i| i.time);
        let mut taps: Vec<f64> = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        if taps.last().is_some_and(|&last| now - last > 2.0) {
            taps.clear();
        }
        taps.push(now);
        if taps.len() > 8 {
            taps.remove(0);
        }
        tapped = tap_bpm(&taps);
        ui.data_mut(|d| d.insert_temp(id, taps));
    }
    let caption = |x: f32, text: &str| {
        ui.painter().text(
            pos2(x, rect.bottom() - 7.0 * k),
            Align2::CENTER_CENTER,
            text.to_uppercase(),
            FontId::proportional(8.5 * k),
            p.weak,
        );
    };
    caption((rect.left() + divider) / 2.0, tr("BPM"));
    caption((divider + rect.right()) / 2.0, tr("Signature"));
    let top = rect.top() + 4.0 * k;
    let row = 20.0 * k;
    let tempo_cell = Rect::from_min_size(pos2(rect.left() + 8.0 * k, top), vec2(58.0 * k, row));
    if k != 1.0 {
        scale_widgets(ui, k);
    }
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
        pos2(divider + 10.0 * k, top),
        vec2(rect.right() - divider - 18.0 * k, row),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(cell), |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0 * k;
            sig = Some(
                ui.add(egui::DragValue::new(&mut numerator).range(1..=32))
                    .on_hover_text(tr("Beats per bar")),
            );
            ui.label(RichText::new("/").color(p.weak));
            egui::ComboBox::from_id_salt("signature-denominator")
                .width(34.0 * k)
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
    if let Some(t) = tapped {
        bpm = t;
    }
    if response.changed() || sig_changed || den_changed || tapped.is_some() {
        doc.history_mut().edit_merged(tr("Tempo"), "tempo", |p| {
            let tempo = TempoMap::constant(bpm, numerator, denominator);
            audiostrada_core::edit::set_tempo(p, tempo);
        });
    }
    end_gesture(doc.history_mut(), &response);
    if let Some(sig) = &sig {
        end_gesture(doc.history_mut(), sig);
    }
    whole
}

/// Text and controls `k` times larger in `ui` (the large transport's fields).
fn scale_widgets(ui: &mut Ui, k: f32) {
    let style = ui.style_mut();
    for font in style.text_styles.values_mut() {
        font.size *= k;
    }
    style.spacing.interact_size *= k;
    style.spacing.button_padding *= k;
}

/// The tempo of taps at `times` (seconds): the mean interval, to a tenth of a BPM.
fn tap_bpm(times: &[f64]) -> Option<f64> {
    let (first, last) = (times.first()?, times.last()?);
    if times.len() < 2 || last <= first {
        return None;
    }
    let interval = (last - first) / (times.len() - 1) as f64;
    Some((60.0 / interval * 10.0).round().clamp(200.0, 4000.0) / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_give_the_tempo() {
        assert_eq!(tap_bpm(&[1.0]), None);
        assert_eq!(tap_bpm(&[0.0, 0.5, 1.0, 1.5]), Some(120.0));
        assert_eq!(tap_bpm(&[0.0, 0.62, 1.2, 1.8]), Some(100.0));
    }

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
