//! The timeline's tool bar, from left to right: the editing tools, the edit mode and snap,
//! clip commands, markers and navigation, then the view at the right (waveforms, track
//! height, zoom). Each button tells what it does, its keys and its mouse gestures; groups that
//! do not fit go into a menu.

use eframe::egui::{self, Color32, Painter, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

use super::{MAX_TRACK_HEIGHT, MAX_ZOOM, MIN_TRACK_HEIGHT, MIN_ZOOM, TimelineView, WaveScale};
use crate::actions::Action;
use crate::i18n::tr;
use crate::theme;
use crate::transport::{EditMode, EditSettings, Grid};
use crate::trf;

pub(super) const HEIGHT: f32 = 32.0;
const BUTTON: egui::Vec2 = vec2(28.0, 24.0);

/// What the mouse does on the clips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    /// Moves, trims, fades by zone; the upper half of a clip draws a range.
    #[default]
    Smart,
    Range,
    Split,
    Erase,
    Mute,
}

impl Tool {
    pub const ALL: [Tool; 5] = [
        Tool::Smart,
        Tool::Range,
        Tool::Split,
        Tool::Erase,
        Tool::Mute,
    ];

    pub fn action(self) -> Action {
        match self {
            Tool::Smart => Action::ToolSmart,
            Tool::Range => Action::ToolRange,
            Tool::Split => Action::ToolSplit,
            Tool::Erase => Action::ToolErase,
            Tool::Mute => Action::ToolMute,
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Tool::Smart => Glyph::Smart,
            Tool::Range => Glyph::Range,
            Tool::Split => Glyph::Scissors,
            Tool::Erase => Glyph::Eraser,
            Tool::Mute => Glyph::MuteTool,
        }
    }

    fn gesture(self) -> &'static str {
        match self {
            Tool::Smart => tr(
                "Upper half of a clip: a range; lower half and name: move (Alt copies); \
                 edges: trim (Alt stretches); top corners: fades",
            ),
            Tool::Range => tr("Drag across clips and tracks to select a range"),
            Tool::Split => tr("Click a clip to split it there, Alt-click splits every track"),
            Tool::Erase => tr("Click a clip to delete it, drag over clips to delete them all"),
            Tool::Mute => tr("Click a clip to mute or unmute it, drag over clips to do it to all"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Command {
    Run(Action),
    /// Scrolls to the playhead.
    ShowPlayhead,
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Smart,
    Range,
    Scissors,
    Eraser,
    MuteTool,
    Split,
    Duplicate,
    Delete,
    Heal,
    Consolidate,
    StripSilence,
    Process,
    Marker,
    PreviousMarker,
    NextMarker,
    ToStart,
    ToEnd,
    Playhead,
    ZoomOut,
    ZoomIn,
    Fit,
    ZoomSelection,
    Shorter,
    Taller,
    Waveform,
}

const COMMANDS: &[(Command, Glyph)] = &[
    (Command::Run(Action::Split), Glyph::Split),
    (Command::Run(Action::Duplicate), Glyph::Duplicate),
    (Command::Run(Action::Delete), Glyph::Delete),
    (Command::Run(Action::Heal), Glyph::Heal),
    (Command::Run(Action::Consolidate), Glyph::Consolidate),
    (Command::Run(Action::StripSilence), Glyph::StripSilence),
    (Command::Run(Action::ProcessClips), Glyph::Process),
];

const NAVIGATION: &[(Command, Glyph)] = &[
    (Command::Run(Action::GoToStart), Glyph::ToStart),
    (Command::Run(Action::PreviousMarker), Glyph::PreviousMarker),
    (Command::Run(Action::AddMarker), Glyph::Marker),
    (Command::Run(Action::NextMarker), Glyph::NextMarker),
    (Command::Run(Action::GoToEnd), Glyph::ToEnd),
    (Command::ShowPlayhead, Glyph::Playhead),
];

/// What the mouse does for a command, if anything.
fn gesture(glyph: Glyph) -> Option<&'static str> {
    Some(match glyph {
        Glyph::ZoomIn | Glyph::ZoomOut => tr("Ctrl+wheel (or pinch), around the pointer"),
        Glyph::Shorter | Glyph::Taller => tr("Alt+wheel, around the pointer"),
        Glyph::Playhead => tr(
            "Shift+wheel or a sideways wheel moves in time, the wheel moves through the tracks, \
             and the bar at the bottom can be dragged",
        ),
        Glyph::Split => tr("Or the scissors tool, or Ctrl+Alt+click on a clip"),
        Glyph::Duplicate => tr("Alt-drag a clip to copy it"),
        _ => return None,
    })
}

fn hint(label: &str, action: Option<Action>, mouse: Option<&str>) -> String {
    let mut text = label.to_owned();
    let keys = action.map(crate::shortcut_labels).unwrap_or_default();
    if !keys.is_empty() {
        text.push('\n');
        text.push_str(&trf!("Keys: {keys}", keys = keys.join(", ")));
    }
    if let Some(mouse) = mouse {
        text.push('\n');
        text.push_str(&trf!("Mouse: {gesture}", gesture = mouse));
    }
    text
}

fn command_hint(command: Command, glyph: Glyph) -> String {
    match command {
        Command::Run(action) => hint(action.label(), Some(action), gesture(glyph)),
        Command::ShowPlayhead => hint(tr("Show the playhead"), None, gesture(glyph)),
    }
}

/// A square button with a glyph; `on` marks the chosen tool.
fn button(ui: &mut Ui, glyph: Glyph, on: bool, text: &str) -> egui::Response {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(BUTTON, Sense::click());
    let fill = if on {
        p.accent.gamma_multiply(0.85)
    } else if response.is_pointer_button_down_on() {
        p.pressed
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    let color = if on {
        Color32::WHITE
    } else if response.hovered() {
        p.text
    } else {
        p.text.gamma_multiply(0.78)
    };
    paint(ui.painter(), rect.shrink2(vec2(7.0, 5.0)), glyph, color);
    response.on_hover_text(text)
}

fn separator(ui: &mut Ui) {
    let p = theme::current(ui);
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, p.border.gamma_multiply(1.5));
    ui.add_space(4.0);
}

/// A thin slider over `0..=1`; gives the new value while it is dragged or clicked.
fn slider(ui: &mut Ui, value: f32, width: f32, text: &str) -> Option<f32> {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(width, BUTTON.y), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), vec2(width - 10.0, 4.0));
    ui.painter().rect_filled(track, 2.0, p.base);
    let x = track.left() + track.width() * value.clamp(0.0, 1.0);
    ui.painter().rect_filled(
        Rect::from_min_max(track.min, pos2(x, track.bottom())),
        2.0,
        p.accent.gamma_multiply(0.6),
    );
    let knob = if response.hovered() || response.dragged() {
        p.text
    } else {
        p.text.gamma_multiply(0.8)
    };
    ui.painter()
        .circle_filled(pos2(x, track.center().y), 5.5, knob);
    let changed = response
        .interact_pointer_pos()
        .filter(|_| response.dragged() || response.clicked())
        .map(|pos| ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0));
    response.on_hover_text(text);
    changed
}

impl TimelineView {
    /// The tool bar above the ruler.
    pub(super) fn toolbar(&mut self, ui: &mut Ui, settings: &mut EditSettings, playhead: u64) {
        let p = theme::current(ui);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
        ui.painter().rect_filled(bar, 0.0, p.panel);
        ui.painter().line_segment(
            [bar.left_bottom(), bar.right_bottom()],
            Stroke::new(1.0, p.border),
        );
        // Commands and navigation give way to a menu when the bar is narrow.
        let step = BUTTON.x + 2.0;
        let fixed = 5.0 * step + 330.0 + 420.0;
        let commands = COMMANDS.len() as f32 * step + 9.0;
        let navigation = NAVIGATION.len() as f32 * step + 9.0;
        let (show_commands, show_navigation) = (
            bar.width() >= fixed + commands + 40.0,
            bar.width() >= fixed + commands + navigation + 40.0,
        );
        let mut requests = Vec::new();
        let mut show_playhead = false;
        let mut run = |command: Command, requests: &mut Vec<Action>| match command {
            Command::Run(action) => requests.push(action),
            Command::ShowPlayhead => show_playhead = true,
        };
        let inner = bar.shrink2(vec2(6.0, 0.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal_centered(|ui| {
                // The tools.
                for tool in Tool::ALL {
                    let action = tool.action();
                    let text = hint(action.label(), Some(action), Some(tool.gesture()));
                    if button(ui, tool.glyph(), self.tool == tool, &text).clicked() {
                        self.tool = tool;
                    }
                }
                separator(ui);
                // The edit mode, the grid and snap.
                edit_mode(ui, &mut settings.mode);
                ui.add_space(4.0);
                egui::ComboBox::from_id_salt("timeline-grid")
                    .width(58.0)
                    .selected_text(settings.grid.name())
                    .show_ui(ui, |ui| {
                        for grid in Grid::ALL {
                            ui.selectable_value(&mut settings.grid, grid, grid.name());
                        }
                    })
                    .response
                    .on_hover_text(hint(tr("Grid value"), Some(Action::GridFiner), None));
                let snap = hint(Action::ToggleSnap.label(), Some(Action::ToggleSnap), None);
                if theme::icon_button(ui, theme::Icon::Snap, settings.snap, p.accent, &snap)
                    .clicked()
                {
                    settings.snap = !settings.snap;
                }
                snap_targets(ui, settings);
                if show_commands {
                    separator(ui);
                    for &(command, glyph) in COMMANDS {
                        if button(ui, glyph, false, &command_hint(command, glyph)).clicked() {
                            run(command, &mut requests);
                        }
                    }
                }
                if show_navigation {
                    separator(ui);
                    for &(command, glyph) in NAVIGATION {
                        if button(ui, glyph, false, &command_hint(command, glyph)).clicked() {
                            run(command, &mut requests);
                        }
                    }
                }
                if !show_navigation {
                    let hidden: Vec<(Command, Glyph)> = if show_commands {
                        NAVIGATION.to_vec()
                    } else {
                        COMMANDS.iter().chain(NAVIGATION).copied().collect()
                    };
                    ui.menu_button("»", |ui| {
                        for (command, _) in hidden {
                            let label = match command {
                                Command::Run(action) => action.label(),
                                Command::ShowPlayhead => tr("Show the playhead"),
                            };
                            if ui.button(label).clicked() {
                                run(command, &mut requests);
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(tr("More commands"));
                }

                // The view, from the right.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let selection = Action::ZoomToSelection;
                    let text = hint(selection.label(), Some(selection), None);
                    if button(ui, Glyph::ZoomSelection, false, &text).clicked() {
                        requests.push(selection);
                    }
                    let fit = Command::Run(Action::ZoomToFit);
                    if button(ui, Glyph::Fit, false, &command_hint(fit, Glyph::Fit)).clicked() {
                        requests.push(Action::ZoomToFit);
                    }
                    let zoom_in = Command::Run(Action::ZoomIn);
                    if button(
                        ui,
                        Glyph::ZoomIn,
                        false,
                        &command_hint(zoom_in, Glyph::ZoomIn),
                    )
                    .clicked()
                    {
                        requests.push(Action::ZoomIn);
                    }
                    // Zoomed in to the right; the span is logarithmic.
                    let span = (MAX_ZOOM / MIN_ZOOM).ln();
                    let value = 1.0 - ((self.zoom / MIN_ZOOM).ln() / span) as f32;
                    let text = hint(tr("Horizontal zoom"), None, gesture(Glyph::ZoomIn));
                    if let Some(v) = slider(ui, value, 110.0, &text) {
                        let zoom = MIN_ZOOM * (span * f64::from(1.0 - v)).exp();
                        let center = f64::from(self.lanes_width / 2.0);
                        let anchor = self.scroll + center * self.zoom;
                        self.zoom_around(self.zoom / zoom, anchor, self.lanes_width / 2.0);
                    }
                    let zoom_out = Command::Run(Action::ZoomOut);
                    if button(
                        ui,
                        Glyph::ZoomOut,
                        false,
                        &command_hint(zoom_out, Glyph::ZoomOut),
                    )
                    .clicked()
                    {
                        requests.push(Action::ZoomOut);
                    }
                    separator(ui);
                    let taller = Command::Run(Action::TallerTracks);
                    if button(
                        ui,
                        Glyph::Taller,
                        false,
                        &command_hint(taller, Glyph::Taller),
                    )
                    .clicked()
                    {
                        requests.push(Action::TallerTracks);
                    }
                    let span = (MAX_TRACK_HEIGHT / MIN_TRACK_HEIGHT).ln();
                    let value = (self.track_height / MIN_TRACK_HEIGHT).ln() / span;
                    let text = hint(tr("Track height"), None, gesture(Glyph::Taller));
                    if let Some(v) = slider(ui, value, 70.0, &text) {
                        self.set_track_height(MIN_TRACK_HEIGHT * (span * v).exp(), None);
                    }
                    let shorter = Command::Run(Action::ShorterTracks);
                    if button(
                        ui,
                        Glyph::Shorter,
                        false,
                        &command_hint(shorter, Glyph::Shorter),
                    )
                    .clicked()
                    {
                        requests.push(Action::ShorterTracks);
                    }
                    separator(ui);
                    self.waveform_menu(ui, &mut requests);
                });
            });
        });
        self.requests.extend(requests);
        if show_playhead {
            self.show_playhead(playhead);
        }
    }

    /// How waveforms are drawn.
    fn waveform_menu(&mut self, ui: &mut Ui, requests: &mut Vec<Action>) {
        let text = hint(
            tr("Waveforms"),
            None,
            Some(tr("Alt+Shift+wheel: waveform zoom")),
        );
        let response = button(ui, Glyph::Waveform, false, &text);
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let view = &mut self.wave_view;
                ui.label(egui::RichText::new(tr("Scale")).small().weak());
                for (scale, name) in [
                    (WaveScale::Linear, tr("Linear")),
                    (WaveScale::Log, tr("Logarithmic (quiet details show)")),
                    (
                        WaveScale::Rectified,
                        tr("Rectified (level only, twice as tall)"),
                    ),
                ] {
                    ui.radio_value(&mut view.scale, scale, name);
                }
                ui.separator();
                ui.checkbox(
                    &mut view.spectral,
                    tr("Spectral colors (lows red, mids green, highs blue)"),
                );
                ui.checkbox(&mut view.merged, tr("Channels merged"));
                ui.checkbox(
                    &mut view.fit,
                    tr("Each clip as tall as it can be (display only)"),
                );
                ui.separator();
                ui.label(tr("Waveform zoom"));
                ui.add(
                    egui::Slider::new(&mut self.wave_zoom, 1.0..=super::MAX_WAVE_ZOOM)
                        .logarithmic(true)
                        .suffix(" ×"),
                );
                if ui.button(Action::WaveformZoomReset.label()).clicked() {
                    requests.push(Action::WaveformZoomReset);
                }
            });
    }

    /// Scrolls so that the playhead is a third into the lanes.
    fn show_playhead(&mut self, playhead: u64) {
        let visible = f64::from(self.lanes_width) * self.zoom;
        self.scroll = (playhead as f64 - visible / 3.0).max(0.0);
    }
}

/// What pulls while snapping, in a menu beside the snap button.
fn snap_targets(ui: &mut Ui, settings: &mut EditSettings) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(14.0, BUTTON.y), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 3.0, p.hover);
    }
    let c = rect.center();
    ui.painter().add(Shape::convex_polygon(
        vec![
            c + vec2(-3.5, -1.5),
            c + vec2(3.5, -1.5),
            c + vec2(0.0, 2.5),
        ],
        p.text.gamma_multiply(0.8),
        Stroke::NONE,
    ));
    let response = response.on_hover_text(tr("What the edits snap to"));
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let t = &mut settings.targets;
            ui.checkbox(&mut t.grid, tr("Grid"));
            ui.checkbox(&mut t.clips, tr("Clip edges"));
            ui.checkbox(&mut t.markers, tr("Markers"));
            ui.checkbox(&mut t.playhead, tr("Playhead"));
            ui.separator();
            ui.label(
                egui::RichText::new(tr(
                    "Edges, markers and the playhead pull from a few pixels away, before the grid",
                ))
                .small()
                .weak(),
            );
        });
}

/// Slip / Shuffle / Grid as one segmented control.
fn edit_mode(ui: &mut Ui, mode: &mut EditMode) {
    let p = theme::current(ui);
    let item = vec2(54.0, BUTTON.y);
    let (rect, _) = ui.allocate_exact_size(vec2(item.x * 3.0, item.y), Sense::hover());
    ui.painter().rect_filled(rect, 5.0, p.base);
    for (i, m) in EditMode::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + item.x * i as f32, rect.top()), item)
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
            egui::Align2::CENTER_CENTER,
            m.name(),
            theme::strong(11.5),
            if on { Color32::WHITE } else { p.weak },
        );
        let text = hint(action.label(), Some(action), None);
        if response.on_hover_text(text).clicked() {
            *mode = m;
        }
    }
}

fn paint(painter: &Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) * 0.5;
    let stroke = Stroke::new(1.4, color);
    let clip = |a: f32, b: f32| {
        Rect::from_min_max(
            pos2(c.x + a * s, c.y - 0.45 * s),
            pos2(c.x + b * s, c.y + 0.45 * s),
        )
    };
    let triangle = |dir: f32| {
        // A triangle pointing left (dir 1) or right (dir -1), on the left or right half.
        vec![
            pos2(c.x - 0.1 * s * dir, c.y - 0.55 * s),
            pos2(c.x - 0.75 * s * dir, c.y),
            pos2(c.x - 0.1 * s * dir, c.y + 0.55 * s),
        ]
    };
    let flag = |x: f32| {
        painter.line_segment([pos2(x, c.y - 0.85 * s), pos2(x, c.y + 0.85 * s)], stroke);
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x, c.y - 0.85 * s),
                pos2(x + 0.7 * s, c.y - 0.55 * s),
                pos2(x, c.y - 0.25 * s),
            ],
            color,
            Stroke::NONE,
        ));
    };
    let magnifier = |plus: bool| {
        let center = pos2(c.x - 0.15 * s, c.y - 0.15 * s);
        let r = 0.6 * s;
        painter.circle_stroke(center, r, stroke);
        painter.line_segment(
            [
                center + vec2(r, r) * 0.7,
                pos2(c.x + 0.9 * s, c.y + 0.9 * s),
            ],
            Stroke::new(2.0, color),
        );
        let d = 0.32 * s;
        painter.line_segment([center - vec2(d, 0.0), center + vec2(d, 0.0)], stroke);
        if plus {
            painter.line_segment([center - vec2(0.0, d), center + vec2(0.0, d)], stroke);
        }
    };
    let arrow = |from: egui::Pos2, to: egui::Pos2| {
        painter.line_segment([from, to], stroke);
        let dir = (to - from).normalized();
        let side = vec2(-dir.y, dir.x);
        let head = 0.3 * s;
        painter.add(Shape::convex_polygon(
            vec![
                to + dir * 0.1,
                to - dir * head + side * head * 0.8,
                to - dir * head - side * head * 0.8,
            ],
            color,
            Stroke::NONE,
        ));
    };
    let wave = |amplitude: f32| {
        let points = (0..=24)
            .map(|i| {
                let t = i as f32 / 24.0;
                let y = (t * std::f32::consts::TAU * 2.0).sin() * amplitude * s;
                pos2(c.x - s + 2.0 * s * t, c.y + y)
            })
            .collect();
        painter.add(Shape::line(points, stroke));
    };
    match glyph {
        Glyph::Split => {
            painter.rect_filled(clip(-1.0, -0.2), 2.0, color);
            painter.rect_filled(clip(0.2, 1.0), 2.0, color);
            painter.line_segment([pos2(c.x, c.y - s), pos2(c.x, c.y + s)], stroke);
        }
        Glyph::Duplicate => {
            let back = Rect::from_min_max(
                pos2(c.x - s, c.y - 0.8 * s),
                pos2(c.x + 0.4 * s, c.y + 0.3 * s),
            );
            painter.rect_stroke(back, 2.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(back.translate(vec2(0.6 * s, 0.5 * s)), 2.0, color);
        }
        Glyph::Delete => {
            painter.line_segment(
                [
                    pos2(c.x - 0.8 * s, c.y - 0.6 * s),
                    pos2(c.x + 0.8 * s, c.y - 0.6 * s),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(c.x - 0.25 * s, c.y - 0.85 * s),
                    pos2(c.x + 0.25 * s, c.y - 0.85 * s),
                ],
                stroke,
            );
            painter.add(Shape::closed_line(
                vec![
                    pos2(c.x - 0.6 * s, c.y - 0.45 * s),
                    pos2(c.x + 0.6 * s, c.y - 0.45 * s),
                    pos2(c.x + 0.45 * s, c.y + 0.9 * s),
                    pos2(c.x - 0.45 * s, c.y + 0.9 * s),
                ],
                stroke,
            ));
        }
        Glyph::Heal => {
            painter.rect_filled(clip(-1.0, 1.0), 2.0, color.gamma_multiply(0.35));
            painter.rect_stroke(clip(-1.0, 1.0), 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([pos2(c.x - 0.3 * s, c.y), pos2(c.x + 0.3 * s, c.y)], stroke);
            painter.line_segment([pos2(c.x, c.y - 0.3 * s), pos2(c.x, c.y + 0.3 * s)], stroke);
        }
        Glyph::Consolidate => {
            for a in [-1.0, -0.3, 0.4] {
                let r = Rect::from_min_max(
                    pos2(c.x + a * s, c.y - 0.85 * s),
                    pos2(c.x + (a + 0.55) * s, c.y - 0.25 * s),
                );
                painter.rect_filled(r, 1.0, color);
            }
            painter.rect_filled(
                Rect::from_min_max(pos2(c.x - s, c.y + 0.2 * s), pos2(c.x + s, c.y + 0.85 * s)),
                1.0,
                color,
            );
        }
        Glyph::StripSilence => {
            for (a, b) in [(-1.0, -0.35), (0.35, 1.0)] {
                let points = (0..=8)
                    .map(|i| {
                        let t = i as f32 / 8.0;
                        let y =
                            if i % 2 == 0 { -0.7 } else { 0.7 } * s * (1.0 - (2.0 * t - 1.0).abs());
                        pos2(c.x + (a + (b - a) * t) * s, c.y + y)
                    })
                    .collect();
                painter.add(Shape::line(points, stroke));
            }
            painter.line_segment(
                [pos2(c.x - 0.35 * s, c.y), pos2(c.x + 0.35 * s, c.y)],
                Stroke::new(1.0, color.gamma_multiply(0.5)),
            );
        }
        Glyph::Marker => flag(c.x - 0.3 * s),
        Glyph::PreviousMarker => {
            painter.add(Shape::convex_polygon(triangle(1.0), color, Stroke::NONE));
            flag(c.x + 0.2 * s);
        }
        Glyph::NextMarker => {
            painter.add(Shape::convex_polygon(triangle(-1.0), color, Stroke::NONE));
            flag(c.x - 0.75 * s);
        }
        Glyph::ToStart => theme::paint_icon(painter, rect, theme::Icon::ToStart, color),
        Glyph::ToEnd => {
            painter.rect_filled(
                Rect::from_center_size(pos2(c.x + s * 0.5, c.y), vec2(s * 0.2, s * 1.1)),
                1.0,
                color,
            );
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - s * 0.55, c.y - s * 0.55),
                    pos2(c.x + s * 0.3, c.y),
                    pos2(c.x - s * 0.55, c.y + s * 0.55),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Playhead => {
            painter.line_segment([pos2(c.x, c.y - 0.4 * s), pos2(c.x, c.y + s)], stroke);
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - 0.5 * s, c.y - s),
                    pos2(c.x + 0.5 * s, c.y - s),
                    pos2(c.x, c.y - 0.35 * s),
                ],
                color,
                Stroke::NONE,
            ));
            arrow(
                pos2(c.x - 0.35 * s, c.y + 0.35 * s),
                pos2(c.x - s, c.y + 0.35 * s),
            );
            arrow(
                pos2(c.x + 0.35 * s, c.y + 0.35 * s),
                pos2(c.x + s, c.y + 0.35 * s),
            );
        }
        Glyph::ZoomOut => magnifier(false),
        Glyph::ZoomIn => magnifier(true),
        Glyph::Fit => {
            for x in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        pos2(c.x + x * s, c.y - 0.8 * s),
                        pos2(c.x + x * s, c.y + 0.8 * s),
                    ],
                    stroke,
                );
            }
            arrow(c, pos2(c.x - 0.75 * s, c.y));
            arrow(c, pos2(c.x + 0.75 * s, c.y));
        }
        Glyph::Shorter | Glyph::Taller => {
            // Rows of tracks: three low ones, or two tall ones.
            let rows = if matches!(glyph, Glyph::Shorter) {
                3
            } else {
                2
            };
            let h = 2.0 * s / rows as f32;
            for r in 0..rows {
                let top = c.y - s + r as f32 * h;
                painter.rect_filled(
                    Rect::from_min_max(pos2(c.x - s, top + 1.0), pos2(c.x + s, top + h - 1.0)),
                    1.5,
                    color,
                );
            }
        }
        Glyph::Waveform => wave(0.75),
        Glyph::Smart => {
            // An arrow pointer.
            let tip = pos2(c.x - 0.55 * s, c.y - s);
            painter.add(Shape::convex_polygon(
                vec![
                    tip,
                    pos2(c.x + 0.6 * s, c.y + 0.15 * s),
                    pos2(c.x, c.y + 0.2 * s),
                    pos2(c.x - 0.55 * s, c.y + 0.75 * s),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    pos2(c.x - 0.05 * s, c.y + 0.1 * s),
                    pos2(c.x + 0.4 * s, c.y + s),
                ],
                Stroke::new(2.0, color),
            );
        }
        Glyph::Range => {
            // An I-beam over a selected span.
            painter.rect_filled(
                Rect::from_min_max(pos2(c.x - s, c.y - 0.45 * s), pos2(c.x + s, c.y + 0.45 * s)),
                1.0,
                color.gamma_multiply(0.35),
            );
            painter.line_segment([pos2(c.x, c.y - s), pos2(c.x, c.y + s)], stroke);
            for y in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        pos2(c.x - 0.3 * s, c.y + y * s),
                        pos2(c.x + 0.3 * s, c.y + y * s),
                    ],
                    stroke,
                );
            }
        }
        Glyph::Scissors => {
            for x in [-1.0, 1.0] {
                painter.circle_stroke(pos2(c.x + x * 0.5 * s, c.y + 0.6 * s), 0.32 * s, stroke);
                painter.line_segment(
                    [
                        pos2(c.x + x * 0.35 * s, c.y + 0.35 * s),
                        pos2(c.x - x * 0.45 * s, c.y - s),
                    ],
                    stroke,
                );
            }
        }
        Glyph::Eraser => {
            let body = vec![
                pos2(c.x - 0.2 * s, c.y - 0.9 * s),
                pos2(c.x + s, c.y + 0.1 * s),
                pos2(c.x + 0.2 * s, c.y + 0.9 * s),
                pos2(c.x - s, c.y - 0.1 * s),
            ];
            painter.add(Shape::closed_line(body, stroke));
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - 0.4 * s, c.y + 0.5 * s),
                    pos2(c.x + 0.2 * s, c.y + 0.9 * s),
                    pos2(c.x - s, c.y - 0.1 * s),
                    pos2(c.x - 0.6 * s, c.y - 0.45 * s),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::MuteTool => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - s, c.y - 0.3 * s),
                    pos2(c.x - 0.55 * s, c.y - 0.3 * s),
                    pos2(c.x, c.y - 0.85 * s),
                    pos2(c.x, c.y + 0.85 * s),
                    pos2(c.x - 0.55 * s, c.y + 0.3 * s),
                    pos2(c.x - s, c.y + 0.3 * s),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    pos2(c.x + 0.3 * s, c.y - 0.35 * s),
                    pos2(c.x + s, c.y + 0.35 * s),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(c.x + 0.3 * s, c.y + 0.35 * s),
                    pos2(c.x + s, c.y - 0.35 * s),
                ],
                stroke,
            );
        }
        Glyph::Process => {
            // A waveform through a box.
            painter.rect_stroke(clip(-0.45, 0.45), 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([pos2(c.x - s, c.y), pos2(c.x - 0.45 * s, c.y)], stroke);
            painter.line_segment([pos2(c.x + 0.45 * s, c.y), pos2(c.x + s, c.y)], stroke);
            let points = (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    let y = (t * std::f32::consts::TAU).sin() * 0.3 * s;
                    pos2(c.x - 0.35 * s + 0.7 * s * t, c.y + y)
                })
                .collect();
            painter.add(Shape::line(points, stroke));
        }
        Glyph::ZoomSelection => {
            let r = Rect::from_center_size(c, vec2(1.4 * s, 1.1 * s));
            painter.rect_filled(r, 1.0, color.gamma_multiply(0.3));
            for (corner, dx, dy) in [
                (pos2(c.x - s, c.y - s), 1.0, 1.0),
                (pos2(c.x + s, c.y - s), -1.0, 1.0),
                (pos2(c.x - s, c.y + s), 1.0, -1.0),
                (pos2(c.x + s, c.y + s), -1.0, -1.0),
            ] {
                painter.line_segment([corner, corner + vec2(0.5 * s * dx, 0.0)], stroke);
                painter.line_segment([corner, corner + vec2(0.0, 0.5 * s * dy)], stroke);
            }
        }
    }
}
