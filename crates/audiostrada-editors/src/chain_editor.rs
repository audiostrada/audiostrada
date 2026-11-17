//! The chains' editor (Strada-Master, Strada-Vocals): the chain as a bar of stages to reorder
//! and open, a choice of style, the sound against a target, an assistant that listens to the
//! loudest passage and sets the knobs, and the big knobs, each opening the stage under it.

use std::collections::VecDeque;

use egui::{
    Align2, Color32, FontId, Mesh, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, pos2,
    vec2,
};

use audiostrada_dsp::EffectInfo;
use audiostrada_dsp::analyzer::{BANDS, band_frequency};
use audiostrada_dsp::chain::{self, master, vocals};
use audiostrada_dsp::imager::SCOPE_POINTS;

use crate::chain_presets;
use crate::controls::EditorParams;
use crate::effects::EffectsState;
use crate::i18n::tr;
use crate::verb_editor::chip;
use crate::{frame, theme, trf, widgets};

/// Seconds of music the assistant listens to, then lets the loudness settle before trimming it.
const LISTEN: f32 = 8.0;
const SETTLE: f32 = 6.0;
/// Strada-Master's and Strada-Vocals' own colors.
const GOLD: Color32 = Color32::from_rgb(240, 190, 80);
const ROSE: Color32 = Color32::from_rgb(242, 110, 156);
/// Heights that keep the window the same whatever is shown.
const BAR: f32 = 36.0;
const SOUND: f32 = 366.0;
const STRIP: f32 = 54.0;
const KNOBS: f32 = 222.0;
/// Seconds of loudness the history shows, and its points per second.
const HISTORY: f64 = 12.0;
const HISTORY_RATE: f64 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Master,
    Vocals,
}

impl Kind {
    fn of(info: &EffectInfo) -> Self {
        if info.id == "mastering" {
            Kind::Master
        } else {
            Kind::Vocals
        }
    }

    fn color(self) -> Color32 {
        match self {
            Kind::Master => GOLD,
            Kind::Vocals => ROSE,
        }
    }
}

/// What the editor remembers between frames.
#[derive(Debug, Default)]
pub struct ChainState {
    /// The stage whose full editor is open.
    detail: Option<&'static str>,
    stages: Vec<(&'static str, Box<EffectsState>)>,
    /// Smoothed input and output spectra (dB), and the output's held peaks.
    spectra: Vec<[f32; 3]>,
    /// Where the target sits against the output, smoothed (dB).
    level: f32,
    /// Output loudness and gain reduction, a point every so often, oldest first.
    history: VecDeque<(f32, f32)>,
    history_time: f64,
    pitch: VecDeque<f32>,
    /// The stage tab being dragged, and where it was taken.
    dragging: Option<usize>,
    comparing: bool,
    assistant: Assistant,
}

impl ChainState {
    fn stage_state(&mut self, stage: &'static str) -> &mut EffectsState {
        let at = match self.stages.iter().position(|(s, _)| *s == stage) {
            Some(at) => at,
            None => {
                self.stages.push((stage, Box::default()));
                self.stages.len() - 1
            }
        };
        &mut self.stages[at].1
    }

    /// The window closed: it opens again on the knobs, nothing compared nor listened to.
    pub fn close(&mut self) {
        self.detail = None;
        self.comparing = false;
        self.dragging = None;
        self.assistant.phase = Phase::Idle;
    }
}

#[derive(Debug, Default, PartialEq)]
enum Phase {
    #[default]
    Idle,
    Listening,
    Settling,
    Done,
}

/// The assistant: what it heard, and the settings before and after it.
#[derive(Debug, Default)]
struct Assistant {
    phase: Phase,
    last: f64,
    heard: f32,
    power: Vec<f64>,
    frames: u32,
    loudness: Vec<f32>,
    peak: f32,
    correlation: (f32, u32),
    notes: [f32; 12],
    voiced: f32,
    sibilance: Vec<f32>,
    before: Vec<(String, f32)>,
    after: Vec<(String, f32)>,
    amount: f32,
    found: String,
    /// Loudness trims made since the assistant or a new target started settling.
    rounds: u8,
    /// The output's power summed while settling, how many times, and the gain reduction.
    settle_power: f64,
    settle_count: u32,
    settle_reduction: f32,
    /// The Loudness knob and the loudness it gave at the last trim.
    trimmed: Option<(f32, f32)>,
}

/// A big knob: its parameter, its name, the stages under it and what it does.
struct Knob {
    id: &'static str,
    label: &'static str,
    stages: &'static [&'static str],
    hint: &'static str,
}

fn knobs(kind: Kind) -> Vec<Knob> {
    let k = |id, label, stages, hint| Knob {
        id,
        label,
        stages,
        hint,
    };
    match kind {
        Kind::Master => vec![
            k(
                "tone",
                tr("TONE"),
                &["eq"],
                tr("Darker or brighter, over the EQ's curve"),
            ),
            k(
                "glue",
                tr("GLUE"),
                &["mb_comp"],
                tr("Holds the mix together: lowers the multiband compressor's thresholds"),
            ),
            k(
                "color",
                tr("COLOR"),
                &["tape"],
                tr("Tape warmth and saturation"),
            ),
            k(
                "width",
                tr("WIDTH"),
                &["imager"],
                tr("Wider or narrower above the bass, which stays in the center"),
            ),
            k(
                "sub",
                tr("SUB"),
                &["eq", "imager"],
                tr(
                    "More or less of the lowest octave: a low shelf at 70 Hz, the last band of \
                    the EQ; Wide keeps it in the center",
                ),
            ),
            k(
                "loudness",
                tr("LOUDNESS"),
                &["maximizer", "limiter"],
                tr(
                    "Drives the maximizer toward the target; the limiter keeps the true peaks \
                    under the ceiling",
                ),
            ),
        ],
        Kind::Vocals => vec![
            k(
                "tune",
                tr("TUNE"),
                &["pitch_correct"],
                tr("Pitch correction: more is faster and firmer"),
            ),
            k(
                "tone",
                tr("TONE"),
                &["eq", "drive"],
                tr("Warmer or brighter, with air on top"),
            ),
            k(
                "comp",
                tr("COMP"),
                &["compressor"],
                tr("Evens the voice out: lowers the compressor's threshold"),
            ),
            k(
                "deess",
                tr("DE-ESS"),
                &["deesser"],
                tr("Softens the S and T sounds"),
            ),
            k(
                "double",
                tr("DOUBLE"),
                &["doubler"],
                tr("Doubles around the voice, for width"),
            ),
            k(
                "space",
                tr("SPACE"),
                &["delay", "reverb"],
                tr("Echo and reverb, ducked under the voice"),
            ),
        ],
    }
}

/// The parameters of one stage, under its prefix in the chain's, shown as they are played: with
/// the knobs applied, a change written back to the base they act around.
struct Stage<'a> {
    outer: &'a mut dyn EditorParams,
    info: &'static EffectInfo,
    /// The chain's own values, its knobs first.
    own: Vec<f32>,
}

impl<'a> Stage<'a> {
    fn new(outer: &'a mut dyn EditorParams, info: &'static EffectInfo) -> Self {
        let own = outer
            .info()
            .params
            .iter()
            .take_while(|p| !p.id.contains('.'))
            .map(|p| outer.get(p.id))
            .collect();
        Self { outer, info, own }
    }

    fn id(&self, id: &str) -> String {
        format!("{}.{id}", self.info.id)
    }

    fn range(&self, id: &str) -> (f32, f32) {
        self.info
            .params
            .iter()
            .find(|p| p.id == id)
            .map_or((f32::MIN, f32::MAX), |p| (p.min, p.max))
    }

    /// The base that plays `value`.
    fn base(&self, id: &str, value: f32) -> f32 {
        let (min, max) = self.range(id);
        chain::base_for(self.outer.info().id, &self.id(id), value, &self.own).clamp(min, max)
    }
}

impl EditorParams for Stage<'_> {
    fn info(&self) -> &'static EffectInfo {
        self.info
    }

    fn get(&self, id: &str) -> f32 {
        let (min, max) = self.range(id);
        let full = self.id(id);
        chain::played(
            self.outer.info().id,
            &full,
            self.outer.get(&full),
            &self.own,
        )
        .clamp(min, max)
    }

    fn change(&mut self, id: &str, changed: Option<f32>, response: &Response) {
        let changed = changed.map(|v| self.base(id, v));
        let id = self.id(id);
        self.outer.change(&id, changed, response);
    }

    fn set_with(
        &mut self,
        id: &str,
        changed: Option<f32>,
        response: &Response,
        more: &mut dyn FnMut(&mut Ui),
    ) {
        let changed = changed.map(|v| self.base(id, v));
        let id = self.id(id);
        self.outer.set_with(&id, changed, response, more);
    }

    fn end_gesture(&mut self) {
        self.outer.end_gesture();
    }

    fn salt(&self) -> u64 {
        self.info
            .id
            .bytes()
            .fold(self.outer.salt() ^ 0xcbf2_9ce4_8422_2325, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
            })
    }

    fn shown(&self, id: &str) {
        self.outer.shown(&self.id(id));
    }

    fn edit_many(&mut self, label: &str, changes: &[(String, f32)]) {
        let changes: Vec<(String, f32)> = changes
            .iter()
            .map(|(k, v)| (self.id(k), self.base(k, *v)))
            .collect();
        self.outer.edit_many(label, &changes);
    }

    fn hear(&mut self, id: &str, value: Option<f32>) {
        let value = value.map(|v| self.base(id, v));
        let id = self.id(id);
        self.outer.hear(&id, value);
    }

    fn sample_rate(&self) -> u32 {
        self.outer.sample_rate()
    }

    fn tempo(&self) -> f64 {
        self.outer.tempo()
    }

    fn notify(&mut self, message: String) {
        self.outer.notify(message);
    }
}

fn meter(meters: &[f32], i: usize) -> f32 {
    meters.get(i).copied().unwrap_or(-120.0)
}

/// The first meter of `stage` in the chain's meters.
fn stage_meter(info: &EffectInfo, stage: &str) -> usize {
    chain::stage_range(info, stage).map_or(usize::MAX, |(_, m)| m)
}

fn target_lufs(params: &dyn EditorParams) -> f32 {
    master::TARGET_LUFS[(params.get("target").round().max(0.0) as usize).min(6)]
}

pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut ChainState) {
    let kind = Kind::of(params.info());
    // Development aid: `AUDIOSTRADA_SCREENSHOT_DETAIL=<stage>` opens that stage's editor once.
    static ASKED: std::sync::Once = std::sync::Once::new();
    ASKED.call_once(|| {
        let asked = std::env::var("AUDIOSTRADA_SCREENSHOT_DETAIL").unwrap_or_default();
        state.detail = chain::stages_of(params.info().id)
            .and_then(|(stages, _)| stages.iter().copied().find(|s| *s == asked));
    });
    record(ui, meters, state, params.info());
    let height = BAR + 8.0 + 36.0 + SOUND + STRIP + KNOBS + 30.0;
    ui.set_min_height(height);
    chain_bar(ui, params, state, kind);
    ui.add_space(8.0);
    if let Some(stage) = state.detail {
        detail(ui, params, meters, state, stage, height - BAR - 8.0);
        return;
    }
    header(ui, params, state, kind);
    ui.add_space(6.0);
    sound(ui, params, meters, state, kind);
    ui.add_space(8.0);
    assistant(ui, params, meters, state, kind);
    ui.add_space(8.0);
    knob_row(ui, params, kind, state);
}

/// Keeps the loudness history and the sung pitch, whatever view is open.
fn record(ui: &Ui, meters: &[f32], state: &mut ChainState, info: &EffectInfo) {
    let now = ui.input(|i| i.time);
    if now - state.history_time >= 1.0 / HISTORY_RATE {
        state.history_time = now;
        let reduction = meter(meters, stage_meter(info, "maximizer"));
        state
            .history
            .push_back((meter(meters, chain::OUT_LOUDNESS), reduction.min(0.0)));
        while state.history.len() > (HISTORY * HISTORY_RATE) as usize {
            state.history.pop_front();
        }
        if info.id == "vocals" {
            let pitch = meter(meters, stage_meter(info, "pitch_correct"));
            state.pitch.push_back(if pitch > 0.0 { pitch } else { 0.0 });
            while state.pitch.len() > (HISTORY * HISTORY_RATE) as usize {
                state.pitch.pop_front();
            }
        }
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

// ---------------------------------------------------------------- The chain's bar

fn family(stage: &str) -> Color32 {
    frame::family_color(audiostrada_dsp::info(stage).map_or("", |i| i.category))
}

fn full_name(stage: &str) -> &'static str {
    audiostrada_dsp::info(stage).map_or("", |i| i.name)
}

fn short_name(stage: &str) -> &'static str {
    full_name(stage).trim_start_matches("Strada-")
}

/// The chain as tabs: its name (the knobs), then its stages in the order the sound goes
/// through them, dragged to reorder, clicked to open.
fn chain_bar(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut ChainState, kind: Kind) {
    let p = theme::current(ui);
    let Some((stages, movable)) = chain::stages_of(params.info().id) else {
        return;
    };
    params.shown("order");
    let order = chain::order(params.get("order"), movable);
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR), Sense::hover());
    let painter = ui.painter_at(bar);
    painter.rect_filled(bar, 10.0, theme::screen(&p));
    let font = FontId::proportional(12.5);
    // The chain's tab: lit on the knobs, a clear way back from a stage.
    let home = format!(
        "{}  {}",
        if state.detail.is_some() { "←" } else { "◉" },
        params.info().name
    );
    let home_width = painter
        .layout_no_wrap(home.clone(), FontId::proportional(13.0), p.text)
        .size()
        .x
        + 28.0;
    let home_rect = Rect::from_min_size(bar.min + vec2(4.0, 4.0), vec2(home_width, BAR - 8.0));
    let home_response = ui
        .interact(
            home_rect,
            ui.scope_id().with(("chain-home", params.salt())),
            Sense::click(),
        )
        .on_hover_text(tr("Back to the knobs"));
    // Always lit: on a stage, it is the way back.
    let fill = if home_response.hovered() {
        kind.color().gamma_multiply(1.15)
    } else {
        kind.color()
    };
    painter.rect_filled(home_rect, 8.0, fill);
    painter.text(
        home_rect.center(),
        Align2::CENTER_CENTER,
        home,
        FontId::proportional(13.0),
        Color32::from_rgb(24, 20, 14),
    );
    if home_response.clicked() {
        state.detail = None;
    }
    // The stages after it, equally wide.
    let start = home_rect.right() + 10.0;
    let width = ((bar.right() - 4.0 - start) / stages.len() as f32).max(40.0);
    let tab = |k: usize| {
        Rect::from_min_size(
            pos2(start + k as f32 * width, bar.top() + 4.0),
            vec2(width - 6.0, BAR - 8.0),
        )
    };
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let mut drop_at = None;
    if let (Some(from), Some(at)) = (state.dragging, pointer) {
        let slot = (((at.x - start) / width).floor().max(0.0) as usize).min(movable - 1);
        drop_at = Some(slot);
        let x = start + slot as f32 * width - if slot > from { -width + 3.0 } else { 3.0 };
        painter.line_segment(
            [pos2(x, bar.top() + 6.0), pos2(x, bar.bottom() - 6.0)],
            Stroke::new(2.5, kind.color()),
        );
    }
    for (k, &at) in order.iter().enumerate().take(stages.len()) {
        let s = if k < movable { at } else { k };
        let stage = stages[s];
        let rect = tab(k);
        let locked = k >= movable;
        let sense = if locked {
            Sense::click()
        } else {
            Sense::click_and_drag()
        };
        let response = ui
            .interact(
                rect,
                ui.scope_id().with(("chain-tab", k, params.salt())),
                sense,
            )
            .on_hover_text(if locked {
                trf!(
                    "{name}, always last: it keeps the true peaks under the ceiling",
                    name = full_name(stage)
                )
            } else {
                trf!(
                    "{name}: click to open, drag to move it in the chain",
                    name = full_name(stage)
                )
            });
        let open = state.detail == Some(stage);
        let tint = family(stage);
        let fill = if open {
            tint.gamma_multiply(0.35)
        } else if response.hovered() {
            p.hover
        } else {
            p.surface
        };
        let lifted = state.dragging == Some(k);
        painter.rect_filled(
            rect,
            7.0,
            if lifted {
                fill.gamma_multiply(0.5)
            } else {
                fill
            },
        );
        if open {
            painter.rect_stroke(rect, 7.0, Stroke::new(1.5, tint), egui::StrokeKind::Inside);
        }
        painter.circle_filled(pos2(rect.left() + 12.0, rect.center().y), 4.0, tint);
        let text = painter.text(
            pos2(rect.left() + 22.0, rect.center().y),
            Align2::LEFT_CENTER,
            short_name(stage),
            font.clone(),
            if open {
                p.text
            } else {
                p.text.gamma_multiply(0.85)
            },
        );
        if locked {
            padlock(
                &painter,
                pos2(text.right() + 10.0, rect.center().y),
                11.0,
                p.weak,
            );
        }
        if k + 1 < stages.len() {
            painter.text(
                pos2(rect.right() + 3.0, rect.center().y),
                Align2::CENTER_CENTER,
                "›",
                FontId::proportional(13.0),
                p.weak,
            );
        }
        if response.drag_started() && !locked {
            state.dragging = Some(k);
        }
        if response.clicked() {
            state.detail = Some(stage);
        }
    }
    let released = ui.input(|i| i.pointer.any_released());
    if released && let (Some(from), Some(to)) = (state.dragging.take(), drop_at) {
        if from != to {
            let mut moved: Vec<usize> = order[..movable].to_vec();
            let stage = moved.remove(from);
            moved.insert(to, stage);
            params.edit_many(
                tr("Order of the stages"),
                &[("order".into(), chain::order_code(&moved))],
            );
        }
    } else if released {
        state.dragging = None;
    }
}

// ---------------------------------------------------------------- Header

fn header(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut ChainState, kind: Kind) {
    ui.allocate_ui(vec2(ui.available_width(), 30.0), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            match kind {
                Kind::Master => {
                    label(ui, tr("Style"));
                    let hints = [
                        tr("Transparent: the mix as it is, only finished"),
                        tr("Rounder lows, softer top, tape at 15 ips"),
                        tr("Brighter, wider top, details brought out"),
                        tr("Tight low end, clear low mids, louder"),
                    ];
                    choices(ui, params, "style", &master::STYLES, &hints, chain_presets::master_style);
                    ui.add_space(14.0);
                    label(ui, tr("Target"));
                    target_menu(ui, params, state);
                    ui.add_space(4.0);
                    switch_chip(
                        ui,
                        params,
                        "follow",
                        tr("Follow"),
                        tr("Reaches the target, then keeps it whenever a setting changes: each \
                            change is measured over a few seconds and made up for. The music \
                            getting louder or softer never moves it"),
                    );
                }
                Kind::Vocals => {
                    label(ui, tr("Voice"));
                    let hints = [
                        tr("A sung lead or backing vocal"),
                        tr("Rap: upfront, firm, close"),
                        tr("Speech: podcast, voice-over, audiobook"),
                    ];
                    choices(ui, params, "voice", &vocals::VOICES, &hints, chain_presets::vocal_voice);
                    ui.add_space(14.0);
                    label(ui, tr("Space"));
                    let hints = [
                        tr("A small room, close and natural"),
                        tr("A plate: smooth and bright, the classic vocal reverb"),
                        tr("A hall, ducked under the voice"),
                        tr("A slapback echo, for rock and rap"),
                        tr("A quarter-note echo thrown after the phrases"),
                    ];
                    choices(ui, params, "room", &vocals::ROOMS, &hints, chain_presets::vocal_room);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match kind {
                Kind::Master => {
                    params.shown("compare");
                    let response = chip(ui, &format!("⇄ {}", tr("Before, same loudness")), state.comparing)
                        .on_hover_text(tr(
                            "Hear the mix without Strada-Master, raised to its loudness: what the \
                             processing does, without the loudness speaking for it",
                        ));
                    if response.clicked() {
                        state.comparing = !state.comparing;
                        params.hear("compare", state.comparing.then_some(1.0));
                    }
                }
                Kind::Vocals => switch_chip(
                    ui,
                    params,
                    "take",
                    tr("Take"),
                    tr("While recording: no pitch correction nor space, hence no added latency"),
                ),
            });
        });
    });
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(11.0)
            .color(theme::current(ui).weak),
    );
}

fn switch_chip(ui: &mut Ui, params: &mut dyn EditorParams, id: &str, text: &str, hint: &str) {
    params.shown(id);
    let on = params.get(id) >= 0.5;
    let response = chip(ui, text, on).on_hover_text(hint);
    let changed = response.clicked().then_some(if on { 0.0 } else { 1.0 });
    params.set_with(id, changed, &response, &mut |_| {});
    if changed.is_some() {
        params.end_gesture();
    }
}

/// One chip per named value of `id`; picking one applies its settings.
fn choices(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    names: &[&'static str],
    hints: &[&'static str],
    settings: fn(usize) -> Vec<(String, f32)>,
) {
    params.shown(id);
    let current = params.get(id).round().max(0.0) as usize;
    for (i, name) in names.iter().enumerate() {
        let response = chip(ui, tr(name), i == current).on_hover_text(hints[i]);
        if response.clicked() && i != current {
            params.edit_many(tr(name), &settings(i));
        }
    }
}

fn target_menu(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut ChainState) {
    params.shown("target");
    let current = (params.get("target").round().max(0.0) as usize).min(master::TARGETS.len() - 1);
    let hints = [
        tr("Spotify, YouTube, Deezer, Tidal"),
        tr("Apple Music, podcasts"),
        tr("A loud CD: peaks up to -0.3 dB"),
        tr("For DJs and clubs: as loud as it gets"),
        tr("A vinyl cut: lows in mono, room under the peaks"),
        tr("Television and radio (EBU R128)"),
        tr("For a mastering engineer: nothing maximized, peaks held at -6 dBFS"),
    ];
    let name = |i: usize| {
        if i == 6 {
            tr(master::TARGETS[i]).to_owned()
        } else {
            format!(
                "{} · {:.0} LUFS",
                tr(master::TARGETS[i]),
                master::TARGET_LUFS[i]
            )
        }
    };
    egui::ComboBox::from_id_salt(("chain-target", params.salt()))
        .selected_text(name(current))
        .width(210.0)
        .show_ui(ui, |ui| {
            for (i, hint) in hints.iter().enumerate() {
                let response = ui
                    .selectable_label(i == current, name(i))
                    .on_hover_text(*hint);
                if response.clicked() && i != current {
                    let mut changes = chain_presets::master_target(i);
                    if i != 6 && params.get("follow") < 0.5 {
                        let gap = master::TARGET_LUFS[i] - master::TARGET_LUFS[current];
                        let loudness = (params.get("loudness") + gap).clamp(-12.0, 18.0);
                        changes.push(("loudness".into(), loudness));
                        state.assistant = Assistant {
                            phase: Phase::Settling,
                            last: ui.input(|i| i.time),
                            ..Assistant::default()
                        };
                    }
                    params.edit_many(tr("Target"), &changes);
                }
            }
        });
}

// ---------------------------------------------------------------- The sound against a target

/// The spectrum a style aims at, relative (dB) at `hz`.
fn target(kind: Kind, style: usize, hz: f32) -> f32 {
    let oct = (hz / 1000.0).log2();
    let bump =
        |center: f32, width: f32, gain: f32| gain * (-((hz / center).log2() / width).powi(2)).exp();
    match kind {
        Kind::Master => {
            let slope = [-4.5, -5.0, -4.0, -4.5][style.min(3)];
            slope * oct
                + match style {
                    1 => bump(200.0, 1.0, 1.5) - bump(9000.0, 1.0, 1.5),
                    2 => bump(10_000.0, 1.0, 1.5) + bump(3000.0, 1.0, 0.8),
                    3 => bump(70.0, 0.8, 2.0) - bump(350.0, 0.8, 1.2),
                    _ => 0.0,
                }
        }
        Kind::Vocals => {
            let cut = if hz < 100.0 {
                -12.0 * (100.0 / hz).log2()
            } else {
                0.0
            };
            -3.0 * oct + bump(3000.0, 1.0, 2.0) + bump(11_000.0, 0.8, 1.5) - bump(300.0, 0.8, 1.5)
                + cut
        }
    }
}

const TOP_DB: f32 = -6.0;
const FLOOR_DB: f32 = -96.0;

/// An area under `points` down to `bottom`, fading from `color` at the top to nothing.
fn glow_area(painter: &egui::Painter, points: &[Pos2], bottom: f32, color: Color32) {
    let mut mesh = Mesh::default();
    let top = points.iter().map(|p| p.y).fold(bottom, f32::min);
    let shade = |y: f32| {
        let t = ((bottom - y) / (bottom - top).max(1.0)).clamp(0.0, 1.0);
        color.gamma_multiply(0.05 + 0.4 * t)
    };
    for p in points {
        mesh.colored_vertex(*p, shade(p.y));
        mesh.colored_vertex(pos2(p.x, bottom), color.gamma_multiply(0.0));
    }
    for i in 0..points.len().saturating_sub(1) {
        let a = (2 * i) as u32;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// A line with a soft halo around it.
fn glow_line(painter: &egui::Painter, points: Vec<Pos2>, color: Color32, width: f32) {
    painter.add(Shape::line(
        points.clone(),
        Stroke::new(width * 5.0, color.gamma_multiply(0.08)),
    ));
    painter.add(Shape::line(
        points.clone(),
        Stroke::new(width * 2.4, color.gamma_multiply(0.2)),
    ));
    painter.add(Shape::line(points, Stroke::new(width, color)));
}

fn sound(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut ChainState,
    kind: Kind,
) {
    let p = theme::current(ui);
    if state.spectra.len() != BANDS {
        state.spectra = vec![[FLOOR_DB; 3]; BANDS];
    }
    for (b, s) in state.spectra.iter_mut().enumerate() {
        for (k, start) in [chain::IN_SPECTRUM, chain::OUT_SPECTRUM]
            .into_iter()
            .enumerate()
        {
            let v = meter(meters, start + b).max(FLOOR_DB);
            s[k] += 0.35 * (v - s[k]);
        }
        s[2] = (s[2] - 0.25).max(s[1]);
    }
    let (rect, painter) = crate::effects::display(ui, SOUND);
    // A soft light from below, in the chain's color.
    let mut light = Mesh::default();
    for (pt, a) in [
        (rect.left_bottom(), 0.14),
        (rect.right_bottom(), 0.14),
        (rect.right_top(), 0.0),
        (rect.left_top(), 0.0),
    ] {
        light.colored_vertex(pt, kind.color().gamma_multiply(a));
    }
    light.add_triangle(0, 1, 2);
    light.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(light));
    let side = 250.0;
    let left = Rect::from_min_max(
        rect.min + vec2(12.0, 12.0),
        pos2(rect.right() - side - 12.0, rect.bottom() - 10.0),
    );
    let split = left.top() + left.height() * 0.66;
    let graph = Rect::from_min_max(left.min, pos2(left.right(), split - 18.0));
    let lower = Rect::from_min_max(pos2(left.left(), split + 4.0), left.right_bottom());
    spectrum(&painter, params, state, kind, graph, &p);
    let column = Rect::from_min_max(
        pos2(rect.right() - side, rect.top() + 12.0),
        rect.right_bottom() - vec2(12.0, 10.0),
    );
    match kind {
        Kind::Master => {
            loudness_history(&painter, params, state, lower, &p);
            loudness_column(&painter, params, meters, column, &p);
        }
        Kind::Vocals => {
            pitch_roll(&painter, state, lower, &p);
            voice_column(&painter, params, meters, column, &p);
        }
    }
}

fn spectrum(
    painter: &egui::Painter,
    params: &mut dyn EditorParams,
    state: &mut ChainState,
    kind: Kind,
    graph: Rect,
    p: &theme::Palette,
) {
    let color = kind.color();
    let x = |hz: f32| graph.left() + graph.width() * ((hz / 20.0).log10() / 3.0).clamp(0.0, 1.0);
    let y = |db: f32| {
        graph.top() + graph.height() * ((TOP_DB - db) / (TOP_DB - FLOOR_DB)).clamp(0.0, 1.0)
    };
    for db in [-24.0, -48.0, -72.0] {
        painter.line_segment(
            [pos2(graph.left(), y(db)), pos2(graph.right(), y(db))],
            Stroke::new(1.0, p.weak.gamma_multiply(0.06)),
        );
    }
    for hz in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        painter.line_segment(
            [pos2(x(hz), graph.top()), pos2(x(hz), graph.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.08)),
        );
        let text = if hz >= 1000.0 {
            format!("{}k", hz / 1000.0)
        } else {
            format!("{hz}")
        };
        painter.text(
            pos2(x(hz), graph.bottom() + 3.0),
            Align2::CENTER_TOP,
            text,
            FontId::proportional(9.5),
            p.weak,
        );
    }
    let style = match kind {
        Kind::Master => params.get("style").round().max(0.0) as usize,
        Kind::Vocals => 0,
    };
    // The target sits where the output's mids are, as the eye compares shapes, not levels.
    let in_mids = |hz: f32| (300.0..3000.0).contains(&hz);
    let bands: Vec<usize> = (0..BANDS).filter(|&b| in_mids(band_frequency(b))).collect();
    let n = bands.len().max(1) as f32;
    let out_mids = bands.iter().map(|&b| state.spectra[b][1]).sum::<f32>() / n;
    let target_mids = bands
        .iter()
        .map(|&b| target(kind, style, band_frequency(b)))
        .sum::<f32>()
        / n;
    let level = if out_mids > FLOOR_DB + 6.0 {
        out_mids - target_mids
    } else {
        -40.0
    };
    state.level += 0.08 * (level - state.level);
    let tunnel = |side: f32| -> Vec<Pos2> {
        (0..BANDS)
            .map(|b| {
                let hz = band_frequency(b);
                pos2(x(hz), y(target(kind, style, hz) + state.level + side))
            })
            .collect()
    };
    let (upper, lower) = (tunnel(3.0), tunnel(-3.0));
    let mut mesh = Mesh::default();
    for (u, l) in upper.iter().zip(&lower) {
        mesh.colored_vertex(*u, color.gamma_multiply(0.05));
        mesh.colored_vertex(*l, color.gamma_multiply(0.16));
    }
    for i in 0..upper.len().saturating_sub(1) {
        let a = (2 * i) as u32;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    painter.add(Shape::mesh(mesh));
    for edge in [upper, lower] {
        painter.add(Shape::dashed_line(
            &edge,
            Stroke::new(1.0, color.gamma_multiply(0.45)),
            4.0,
            3.0,
        ));
    }
    let curve = |k: usize| -> Vec<Pos2> {
        (0..BANDS)
            .map(|b| pos2(x(band_frequency(b)), y(state.spectra[b][k])))
            .collect()
    };
    painter.add(Shape::line(
        curve(0),
        Stroke::new(1.0, p.weak.gamma_multiply(0.55)),
    ));
    let out = curve(1);
    glow_area(painter, &out, graph.bottom(), color);
    glow_line(painter, out, color, 1.6);
    for (b, s) in state.spectra.iter().enumerate().step_by(2) {
        if s[2] > FLOOR_DB + 3.0 {
            painter.circle_filled(
                pos2(x(band_frequency(b)), y(s[2])),
                1.3,
                color.gamma_multiply(0.7),
            );
        }
    }
    let legend = [
        (tr("Input"), p.weak),
        (tr("Output"), color),
        (tr("Target"), color.gamma_multiply(0.5)),
    ];
    let mut at = graph.left_top() + vec2(4.0, 0.0);
    for (name, c) in legend {
        painter.circle_filled(at + vec2(4.0, 7.0), 3.5, c);
        let r = painter.text(
            at + vec2(12.0, 0.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(10.5),
            p.weak,
        );
        at.x = r.right() + 12.0;
    }
}

/// The output loudness of the last seconds against the target, gain reduction from the top.
fn loudness_history(
    painter: &egui::Painter,
    params: &mut dyn EditorParams,
    state: &ChainState,
    rect: Rect,
    p: &theme::Palette,
) {
    let (low, high) = (-36.0f32, 0.0f32);
    let y = |db: f32| rect.bottom() - rect.height() * ((db - low) / (high - low)).clamp(0.0, 1.0);
    painter.rect_filled(rect, 6.0, p.panel.gamma_multiply(0.5));
    let count = (HISTORY * HISTORY_RATE) as usize;
    let x = |i: usize, len: usize| {
        rect.right() - (len - 1 - i) as f32 * rect.width() / (count - 1) as f32
    };
    let len = state.history.len();
    if len > 1 {
        let points: Vec<Pos2> = state
            .history
            .iter()
            .enumerate()
            .map(|(i, (lufs, _))| pos2(x(i, len), y(lufs.max(low))))
            .collect();
        glow_area(painter, &points, rect.bottom(), GOLD);
        glow_line(painter, points, GOLD, 1.4);
        let reduction: Vec<Pos2> = state
            .history
            .iter()
            .enumerate()
            .map(|(i, (_, gr))| {
                pos2(
                    x(i, len),
                    rect.top() + (-gr / 12.0).clamp(0.0, 1.0) * rect.height() * 0.5,
                )
            })
            .collect();
        painter.add(Shape::line(
            reduction,
            Stroke::new(1.2, theme::RECORD.gamma_multiply(0.8)),
        ));
    }
    let goal = target_lufs(params);
    let ty = y(goal);
    painter.add(Shape::dashed_line(
        &[pos2(rect.left(), ty), pos2(rect.right(), ty)],
        Stroke::new(1.2, Color32::WHITE.gamma_multiply(0.7)),
        6.0,
        4.0,
    ));
    painter.text(
        pos2(rect.left() + 6.0, ty - 2.0),
        Align2::LEFT_BOTTOM,
        trf!("Target {lufs} LUFS", lufs = format!("{goal:.0}")),
        FontId::proportional(10.0),
        p.text.gamma_multiply(0.8),
    );
    painter.text(
        rect.left_top() + vec2(6.0, 4.0),
        Align2::LEFT_TOP,
        tr("Loudness, last seconds"),
        FontId::proportional(10.0),
        p.weak,
    );
    painter.text(
        rect.right_top() + vec2(-6.0, 4.0),
        Align2::RIGHT_TOP,
        tr("Reduction"),
        FontId::proportional(10.0),
        theme::RECORD.gamma_multiply(0.8),
    );
}

fn loudness_column(
    painter: &egui::Painter,
    params: &mut dyn EditorParams,
    meters: &[f32],
    rect: Rect,
    p: &theme::Palette,
) {
    let goal = target_lufs(params);
    let short = meter(meters, chain::OUT_LOUDNESS);
    let heard = short > -70.0;
    let off = short - goal;
    let tint = if !heard {
        p.weak
    } else if off.abs() <= 1.0 {
        theme::PLAY
    } else if off > 1.0 {
        theme::RECORD
    } else {
        theme::SOLO
    };
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        tr("LOUDNESS"),
        FontId::proportional(10.5),
        p.weak,
    );
    let value = if heard {
        format!("{short:.1}")
    } else {
        "–".into()
    };
    let r = painter.text(
        rect.left_top() + vec2(0.0, 14.0),
        Align2::LEFT_TOP,
        value,
        FontId::monospace(38.0),
        tint,
    );
    painter.text(
        pos2(r.right() + 6.0, r.bottom() - 7.0),
        Align2::LEFT_BOTTOM,
        "LUFS",
        FontId::proportional(11.0),
        p.weak,
    );
    let followed = meter(meters, chain::FOLLOWED);
    let line = if params.get("follow") >= 0.5 {
        trf!(
            "Following {lufs} LUFS · {db} dB",
            lufs = format!("{goal:.0}"),
            db = format!("{followed:+.1}")
        )
    } else {
        trf!("Target {lufs} LUFS", lufs = format!("{goal:.0}"))
    };
    painter.text(
        rect.left_top() + vec2(0.0, 62.0),
        Align2::LEFT_TOP,
        line,
        FontId::proportional(10.5),
        p.weak,
    );
    // The stereo image: each dot a moment, mid up, sides across.
    let scope = Rect::from_center_size(
        pos2(rect.left() + 62.0, rect.top() + 160.0),
        vec2(112.0, 112.0),
    );
    painter.circle_stroke(
        scope.center(),
        56.0,
        Stroke::new(1.0, p.weak.gamma_multiply(0.2)),
    );
    painter.line_segment(
        [scope.center_top(), scope.center_bottom()],
        Stroke::new(1.0, p.weak.gamma_multiply(0.12)),
    );
    painter.line_segment(
        [scope.left_center(), scope.right_center()],
        Stroke::new(1.0, p.weak.gamma_multiply(0.12)),
    );
    let imager = stage_meter(params.info(), "imager");
    let scale = (0..SCOPE_POINTS)
        .map(|i| {
            meter(meters, imager + 2 + 2 * i)
                .abs()
                .max(meter(meters, imager + 3 + 2 * i).abs())
        })
        .fold(0.05f32, f32::max);
    for i in 0..SCOPE_POINTS {
        let (m, s) = (
            meter(meters, imager + 2 + 2 * i),
            meter(meters, imager + 3 + 2 * i),
        );
        if m.abs() + s.abs() < 1e-5 || m < -100.0 {
            continue;
        }
        let at = scope.center() + vec2(s / scale * 52.0, -m / scale * 52.0);
        painter.circle_filled(
            at,
            1.6,
            GOLD.gamma_multiply(0.3 + 0.7 * i as f32 / SCOPE_POINTS as f32),
        );
    }
    let correlation = meter(meters, imager);
    painter.text(
        pos2(scope.center().x, scope.bottom() + 6.0),
        Align2::CENTER_TOP,
        trf!("Correlation {c}", c = format!("{correlation:+.2}")),
        FontId::proportional(9.5),
        p.weak,
    );
    let lines = [
        (tr("Integrated"), meter(meters, chain::INTEGRATED), "LUFS"),
        (tr("True peak"), meter(meters, chain::TRUE_PEAK), "dBTP"),
        (
            tr("Reduction"),
            meter(meters, stage_meter(params.info(), "maximizer")),
            "dB",
        ),
    ];
    for (k, (name, v, unit)) in lines.into_iter().enumerate() {
        let at = pos2(rect.left() + 136.0, rect.top() + 100.0 + k as f32 * 40.0);
        painter.text(
            at,
            Align2::LEFT_TOP,
            name,
            FontId::proportional(10.0),
            p.weak,
        );
        let text = if v > -100.0 {
            format!("{v:.1} {unit}")
        } else {
            format!("– {unit}")
        };
        painter.text(
            at + vec2(0.0, 13.0),
            Align2::LEFT_TOP,
            text,
            FontId::monospace(13.0),
            p.text,
        );
    }
    // The short-term loudness as a bar, the target marked across it.
    let bar = Rect::from_min_max(
        pos2(rect.right() - 14.0, rect.top() + 2.0),
        pos2(rect.right() - 2.0, rect.bottom() - 2.0),
    );
    let (low, high) = (-36.0f32, 0.0f32);
    let yof = |db: f32| bar.bottom() - bar.height() * ((db - low) / (high - low)).clamp(0.0, 1.0);
    painter.rect_filled(bar, 4.0, p.panel);
    if heard {
        painter.rect_filled(
            Rect::from_min_max(pos2(bar.left(), yof(short)), bar.right_bottom()),
            4.0,
            tint.gamma_multiply(0.85),
        );
    }
    let ty = yof(goal);
    painter.line_segment(
        [pos2(bar.left() - 6.0, ty), pos2(bar.right() + 2.0, ty)],
        Stroke::new(2.0, GOLD),
    );
}

/// The sung pitch of the last seconds, a row per semitone around the last note.
fn pitch_roll(painter: &egui::Painter, state: &ChainState, rect: Rect, p: &theme::Palette) {
    painter.rect_filled(rect, 6.0, p.panel.gamma_multiply(0.5));
    let center = state
        .pitch
        .iter()
        .rev()
        .find(|v| **v > 0.0)
        .copied()
        .unwrap_or(60.0)
        .round();
    let span = 12.0;
    let y = |n: f32| rect.center().y - (n - center) / span * rect.height();
    for k in -6..=6 {
        let n = center + k as f32;
        let yy = y(n);
        let black = matches!((n as i32).rem_euclid(12), 1 | 3 | 6 | 8 | 10);
        painter.line_segment(
            [pos2(rect.left() + 34.0, yy), pos2(rect.right(), yy)],
            Stroke::new(1.0, p.weak.gamma_multiply(if black { 0.05 } else { 0.1 })),
        );
        if k % 2 == 0 {
            painter.text(
                pos2(rect.left() + 6.0, yy),
                Align2::LEFT_CENTER,
                note(n as i32),
                FontId::proportional(9.0),
                p.weak,
            );
        }
    }
    let count = (HISTORY * HISTORY_RATE) as usize;
    let len = state.pitch.len();
    let x =
        |i: usize| rect.right() - (len - 1 - i) as f32 * (rect.width() - 36.0) / (count - 1) as f32;
    let mut run: Vec<Pos2> = Vec::new();
    for (i, v) in state.pitch.iter().enumerate() {
        if *v <= 0.0 || (y(*v) - rect.center().y).abs() > rect.height() / 2.0 {
            if run.len() > 1 {
                glow_line(painter, std::mem::take(&mut run), ROSE, 1.8);
            }
            run.clear();
            continue;
        }
        run.push(pos2(x(i), y(*v)));
    }
    if run.len() > 1 {
        glow_line(painter, run, ROSE, 1.8);
    }
    painter.text(
        rect.right_top() + vec2(-6.0, 4.0),
        Align2::RIGHT_TOP,
        tr("Pitch, last seconds"),
        FontId::proportional(10.0),
        p.weak,
    );
}

fn voice_column(
    painter: &egui::Painter,
    params: &mut dyn EditorParams,
    meters: &[f32],
    rect: Rect,
    p: &theme::Palette,
) {
    let info = params.info();
    let pitch = meter(meters, stage_meter(info, "pitch_correct"));
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        tr("Pitch"),
        FontId::proportional(10.5),
        p.weak,
    );
    let (name, cents) = if pitch > 0.0 {
        let n = pitch.round();
        (note(n as i32), format!("{:+.0} ct", (pitch - n) * 100.0))
    } else {
        ("–".to_owned(), String::new())
    };
    let r = painter.text(
        rect.left_top() + vec2(0.0, 14.0),
        Align2::LEFT_TOP,
        name,
        FontId::monospace(38.0),
        ROSE,
    );
    painter.text(
        pos2(r.right() + 8.0, r.bottom() - 6.0),
        Align2::LEFT_BOTTOM,
        cents,
        FontId::monospace(12.0),
        p.weak,
    );
    let bars = [
        (
            tr("Compression"),
            meter(meters, stage_meter(info, "compressor")),
            theme::MUTE,
        ),
        (
            tr("De-essing"),
            meter(meters, stage_meter(info, "deesser")),
            theme::SOLO,
        ),
    ];
    for (k, (name, db, color)) in bars.into_iter().enumerate() {
        let top = rect.top() + 84.0 + k as f32 * 46.0;
        painter.text(
            pos2(rect.left(), top),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(10.0),
            p.weak,
        );
        let bar = Rect::from_min_size(
            pos2(rect.left(), top + 16.0),
            vec2(rect.width() - 70.0, 9.0),
        );
        painter.rect_filled(bar, 4.0, p.panel);
        let depth = (-db.min(0.0) / 12.0).clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width() * depth, bar.height())),
            4.0,
            color,
        );
        painter.text(
            pos2(rect.right(), top + 20.0),
            Align2::RIGHT_CENTER,
            format!("{:.1} dB", db.min(0.0)),
            FontId::monospace(11.0),
            p.text,
        );
    }
    let short = meter(meters, chain::OUT_LOUDNESS);
    painter.text(
        pos2(rect.left(), rect.top() + 184.0),
        Align2::LEFT_TOP,
        tr("Output"),
        FontId::proportional(10.0),
        p.weak,
    );
    let text = if short > -70.0 {
        format!("{short:.1} LUFS")
    } else {
        "– LUFS".into()
    };
    painter.text(
        pos2(rect.left(), rect.top() + 198.0),
        Align2::LEFT_TOP,
        text,
        FontId::monospace(15.0),
        p.text,
    );
    let peak = meter(meters, chain::OUT_PEAK);
    let text = if peak > -100.0 {
        format!("{peak:.1} dBFS")
    } else {
        "– dBFS".into()
    };
    painter.text(
        pos2(rect.left() + 120.0, rect.top() + 184.0),
        Align2::LEFT_TOP,
        tr("Peak"),
        FontId::proportional(10.0),
        p.weak,
    );
    painter.text(
        pos2(rect.left() + 120.0, rect.top() + 198.0),
        Align2::LEFT_TOP,
        text,
        FontId::monospace(15.0),
        p.text,
    );
}

fn note(n: i32) -> String {
    use audiostrada_dsp::pitch_correct::NOTE_NAMES;
    format!(
        "{}{}",
        NOTE_NAMES[n.rem_euclid(12) as usize],
        n.div_euclid(12) - 1
    )
}

// ---------------------------------------------------------------- Knobs

fn knob_row(ui: &mut Ui, params: &mut dyn EditorParams, kind: Kind, state: &mut ChainState) {
    let p = theme::current(ui);
    let list = knobs(kind);
    let gap = 10.0;
    let width = (ui.available_width() - gap * (list.len() - 1) as f32) / list.len() as f32;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for knob in &list {
            let tint = family(knob.stages[0]);
            let (fill, edge) = theme::card(&p);
            egui::Frame::new()
                .fill(fill)
                .stroke(edge)
                .corner_radius(12.0)
                .inner_margin(egui::Margin::symmetric(8, 10))
                .show(ui, |ui| {
                    ui.set_width(width - 16.0);
                    ui.set_height(KNOBS - 22.0);
                    ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new(knob.label).size(13.5).strong().color(p.text));
                        let size = if knob.id == "loudness" { 90.0 } else { 80.0 };
                        big_knob(ui, params, knob, tint, size);
                        if kind == Kind::Vocals && knob.id == "tone" {
                            saturation(ui, params, state);
                        } else {
                            stage_links(ui, knob.stages, state, tint);
                        }
                    });
                });
        }
    });
}

fn stage_links(ui: &mut Ui, stages: &[&'static str], state: &mut ChainState, tint: Color32) {
    ui.horizontal(|ui| {
        let total: f32 = stages
            .iter()
            .map(|s| short_name(s).len() as f32 * 6.5 + 26.0)
            .sum();
        ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
        for stage in stages {
            let response = ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("{} ↗", short_name(stage)))
                            .size(10.5)
                            .color(tint),
                    )
                    .frame(false),
                )
                .on_hover_text(trf!("Full settings of {name}", name = full_name(stage)));
            if response.clicked() {
                state.detail = Some(stage);
            }
        }
    });
}

/// The voice's saturation under its tone: a small knob, and Strada-Drive behind it.
fn saturation(ui: &mut Ui, params: &mut dyn EditorParams, state: &mut ChainState) {
    let p = theme::current(ui);
    let tint = family("drive");
    stage_links(ui, &["eq"], state, family("eq"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(((ui.available_width() - 176.0) / 2.0).max(0.0));
        params.shown("drive");
        let value = params.get("drive");
        let (changed, response) = widgets::knob(ui, value, (0.0, 100.0), 0.0, 40.0, false, tint);
        let response = response.on_hover_text(tr(
            "Saturation: from a touch of warmth to grit, through Strada-Drive",
        ));
        params.set_with("drive", changed, &response, &mut |_| {});
        ui.label(
            RichText::new(format!(
                "{} {:.1}",
                tr("Saturation"),
                changed.unwrap_or(value) / 10.0
            ))
            .size(10.5)
            .color(p.weak),
        );
        let drive = ui
            .add(egui::Button::new(RichText::new("Drive ↗").size(10.5).color(tint)).frame(false))
            .on_hover_text(trf!("Full settings of {name}", name = full_name("drive")));
        if drive.clicked() {
            state.detail = Some("drive");
        }
    });
}

/// A small padlock.
fn padlock(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let body = Rect::from_center_size(
        center + vec2(0.0, size * 0.18),
        vec2(size * 0.8, size * 0.56),
    );
    painter.rect_filled(body, size * 0.1, color);
    let r = size * 0.24;
    let top = body.top();
    let arc: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            pos2(center.x + r * a.cos(), top + r * a.sin())
        })
        .collect();
    painter.add(Shape::line(arc, Stroke::new(size * 0.12, color)));
}

fn big_knob(ui: &mut Ui, params: &mut dyn EditorParams, knob: &Knob, tint: Color32, size: f32) {
    let Some(info) = params.param(knob.id) else {
        return;
    };
    params.shown(knob.id);
    let value = params.get(knob.id);
    let (changed, response) = widgets::knob(
        ui,
        value,
        (info.min, info.max),
        info.default,
        size,
        info.min < 0.0,
        tint,
    );
    let response = response.on_hover_text(knob.hint);
    params.set_with(knob.id, changed, &response, &mut |_| {});
    let shown = changed.unwrap_or(value);
    let text = match knob.id {
        "loudness" if params.get("follow") >= 0.5 => tr("Following the target").to_owned(),
        "loudness" => format!("{shown:+.1} dB"),
        "width" => format!("{shown:.0} %"),
        "tone" | "sub" => {
            if shown.abs() < 0.5 {
                tr("Neutral").to_owned()
            } else if knob.id == "sub" {
                format!("{:+.1}", shown / 10.0)
            } else if shown < 0.0 {
                trf!("Warmer {n}", n = format!("{:.0}", -shown))
            } else {
                trf!("Brighter {n}", n = format!("{shown:.0}"))
            }
        }
        _ => format!("{:.1}", shown / 10.0),
    };
    ui.label(RichText::new(text).monospace().size(12.0));
}

// ---------------------------------------------------------------- A stage's full editor

fn detail(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut ChainState,
    stage: &'static str,
    height: f32,
) {
    let p = theme::current(ui);
    let Some(info) = audiostrada_dsp::info(stage) else {
        state.detail = None;
        return;
    };
    let access = params.stage_access(stage);
    ui.horizontal(|ui| {
        let acting: Vec<&str> = knobs(Kind::of(params.info()))
            .into_iter()
            .filter(|k| k.stages.contains(&stage))
            .map(|k| {
                if stage == "drive" {
                    tr("Saturation")
                } else {
                    k.label
                }
            })
            .collect();
        if let Some(name) = acting.first() {
            ui.label(
                RichText::new(trf!(
                    "The {knob} knob acts around these settings",
                    knob = name
                ))
                .size(10.5)
                .color(p.weak),
            );
        }
        if access.is_ok() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                stage_presets(ui, params, stage);
            });
        }
    });
    ui.add_space(4.0);
    if let Err(why) = access {
        locked(ui, params, stage, &why);
        return;
    }
    let start = stage_meter(params.info(), stage);
    let sub_meters = meters.get(start..start + info.meters.len()).unwrap_or(&[]);
    let sub_state = state.stage_state(stage);
    egui::ScrollArea::vertical()
        .id_salt(("chain-detail", stage))
        .max_height((height - 30.0).max(200.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut adapter = Stage::new(params, info);
            crate::effects::show(ui, &mut adapter, sub_meters, sub_state);
        });
}

fn stage_presets(ui: &mut Ui, params: &mut dyn EditorParams, stage: &'static str) {
    let list = crate::presets::factory(stage);
    if list.is_empty() {
        return;
    }
    egui::ComboBox::from_id_salt(("chain-stage-preset", stage, params.salt()))
        .selected_text(tr("Stage presets"))
        .width(220.0)
        .show_ui(ui, |ui| {
            for preset in &list {
                if ui.selectable_label(false, &preset.name).clicked() {
                    params.edit_many(&preset.name, &chain_presets::stage_from(stage, preset));
                }
            }
        });
}

fn locked(ui: &mut Ui, params: &mut dyn EditorParams, stage: &'static str, why: &str) {
    let p = theme::current(ui);
    let name = full_name(stage);
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        let (lock, _) = ui.allocate_exact_size(vec2(40.0, 44.0), Sense::hover());
        padlock(ui.painter(), lock.center(), 36.0, p.weak);
        ui.label(
            RichText::new(trf!("Fine settings with {name}", name = name))
                .size(18.0)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(RichText::new(why).size(12.0).color(p.weak));
        ui.label(
            RichText::new(tr(
                "The stage keeps playing as the knobs and presets set it.",
            ))
            .size(12.0)
            .color(p.weak),
        );
        ui.add_space(10.0);
        if ui
            .button(RichText::new(trf!("Get {name}", name = name)).size(13.0))
            .clicked()
        {
            params.get_stage(stage);
        }
    });
}

// ---------------------------------------------------------------- The assistant

fn start_listening(ui: &Ui, params: &mut dyn EditorParams, state: &mut ChainState) {
    state.assistant = Assistant {
        phase: Phase::Listening,
        last: ui.input(|i| i.time),
        power: vec![0.0; BANDS],
        amount: 100.0,
        ..Assistant::default()
    };
    offered(params);
}

/// The assistant was offered: it is not offered again when the window opens.
fn offered(params: &mut dyn EditorParams) {
    if params.get("assisted") < 0.5 {
        params.edit_many(tr("Assistant"), &[("assisted".into(), 1.0)]);
    }
}

/// The assistant's strip, always there: an offer, its listening, or what it found.
fn assistant(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut ChainState,
    kind: Kind,
) {
    let p = theme::current(ui);
    let now = ui.input(|i| i.time);
    let info = params.info();
    params.shown("assisted");
    let first = params.get("assisted") < 0.5;
    {
        let a = &mut state.assistant;
        let dt = ((now - a.last) as f32).clamp(0.0, 0.1);
        a.last = now;
        let input = meter(meters, chain::IN_LOUDNESS);
        let playing = input > -50.0;
        match a.phase {
            Phase::Listening => {
                if playing {
                    a.heard += dt;
                    for (b, power) in a.power.iter_mut().enumerate() {
                        *power +=
                            10f64.powf(f64::from(meter(meters, chain::IN_SPECTRUM + b)) / 10.0);
                    }
                    a.frames += 1;
                    a.loudness.push(input);
                    a.peak = a.peak.max(meter(meters, chain::IN_PEAK));
                    if kind == Kind::Master {
                        a.correlation.0 += meter(meters, stage_meter(info, "imager"));
                        a.correlation.1 += 1;
                    } else {
                        let pitch = meter(meters, stage_meter(info, "pitch_correct"));
                        if pitch > 0.0 {
                            a.notes[(pitch.round() as i32).rem_euclid(12) as usize] += dt;
                            a.voiced += dt;
                        }
                        a.sibilance
                            .push(meter(meters, stage_meter(info, "deesser") + 1));
                    }
                }
                if a.heard >= LISTEN {
                    let (found, sentence) = suggest(params, &state.assistant, kind);
                    let a = &mut state.assistant;
                    a.before = found
                        .iter()
                        .map(|(k, _)| (k.clone(), params.get(k)))
                        .collect();
                    params.edit_many(tr("Assistant"), &found);
                    a.after = found;
                    a.found = sentence;
                    a.heard = 0.0;
                    a.phase = if kind == Kind::Master && params.get("follow") < 0.5 {
                        Phase::Settling
                    } else {
                        Phase::Done
                    };
                }
            }
            Phase::Settling => {
                let out = meter(meters, chain::OUT_LOUDNESS);
                if playing && out > -70.0 {
                    a.heard += dt;
                    a.settle_power += 10f64.powf(f64::from(out) / 10.0);
                    a.settle_count += 1;
                    a.settle_reduction += meter(meters, stage_meter(info, "maximizer"));
                }
                if a.heard >= SETTLE && a.settle_count > 0 {
                    // The loudness over the last seconds, not at one moment: a target is an
                    // average, a quiet verse must not pass for the song.
                    let heard =
                        (10.0 * (a.settle_power / f64::from(a.settle_count)).log10()) as f32;
                    let reduction = a.settle_reduction / a.settle_count as f32;
                    let off = target_lufs(params) - heard;
                    let was = params.get("loudness");
                    // How much louder a dB of the knob makes it: less while the maximizer works.
                    let slope = match a.trimmed {
                        Some((knob, lufs)) if (was - knob).abs() > 0.3 => {
                            ((heard - lufs) / (was - knob)).clamp(0.25, 1.0)
                        }
                        _ if reduction < -3.0 => 0.6,
                        _ => 1.0,
                    };
                    let loudness = (was + off / slope).clamp(-12.0, 18.0);
                    let trim = off.abs() > 0.5;
                    if trim {
                        params.edit_many(tr("Assistant"), &[("loudness".into(), loudness)]);
                    }
                    a.trimmed = Some((was, heard));
                    a.rounds += 1;
                    (a.heard, a.settle_power, a.settle_count, a.settle_reduction) =
                        (0.0, 0.0, 0, 0.0);
                    let settled = off.abs() <= 0.7 || a.rounds >= 3 || !trim;
                    if !a.after.is_empty() && trim {
                        match a.after.iter_mut().find(|(k, _)| k == "loudness") {
                            Some(entry) => entry.1 = loudness,
                            None => {
                                a.before.push(("loudness".into(), was));
                                a.after.push(("loudness".into(), loudness));
                            }
                        }
                    }
                    if settled {
                        // A new target has nothing to blend: the strip goes back to its offer.
                        a.phase = if a.after.is_empty() {
                            Phase::Idle
                        } else {
                            Phase::Done
                        };
                    }
                }
            }
            Phase::Idle | Phase::Done => {}
        }
    }
    let color = kind.color();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP), Sense::hover());
    let lit = first || state.assistant.phase != Phase::Idle;
    ui.painter().rect_filled(
        rect,
        12.0,
        if lit {
            color.gamma_multiply(0.16)
        } else {
            theme::card(&p).0
        },
    );
    ui.painter().rect_stroke(
        rect,
        12.0,
        Stroke::new(
            1.0,
            if lit {
                color.gamma_multiply(0.7)
            } else {
                theme::card(&p).1.color
            },
        ),
        egui::StrokeKind::Inside,
    );
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(12.0, 8.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut inner;
    let button = |ui: &mut Ui, text: &str| {
        ui.add(
            egui::Button::new(
                RichText::new(format!("✦  {text}"))
                    .size(13.5)
                    .strong()
                    .color(Color32::from_rgb(24, 20, 14)),
            )
            .fill(color)
            .corner_radius(9.0)
            .min_size(vec2(170.0, 34.0)),
        )
    };
    let playing = meter(meters, chain::IN_LOUDNESS) > -50.0;
    match state.assistant.phase {
        Phase::Idle => {
            let text = if first {
                tr(
                    "New here? Play the loudest passage and let the assistant set the knobs for this sound.",
                )
            } else {
                tr("The assistant listens to the loudest passage and sets the knobs for it.")
            };
            if button(ui, tr("Listen and set")).clicked() {
                start_listening(ui, params, state);
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(text)
                    .size(12.5)
                    .color(if first { p.text } else { p.weak }),
            );
            if first {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("Later")).clicked() {
                        offered(params);
                    }
                });
            }
        }
        Phase::Listening | Phase::Settling => {
            ui.ctx().request_repaint();
            let a = &mut state.assistant;
            if button(ui, tr("Stop")).clicked() {
                a.phase = Phase::Idle;
                return;
            }
            ui.add_space(10.0);
            let (text, progress) = if a.phase == Phase::Settling {
                (
                    tr("Bringing the loudness to the target…").to_owned(),
                    a.heard / SETTLE,
                )
            } else if playing {
                (
                    trf!(
                        "Listening… {s} / {total} s",
                        s = format!("{:.0}", a.heard),
                        total = format!("{LISTEN:.0}")
                    ),
                    a.heard / LISTEN,
                )
            } else {
                (
                    tr("Play the loudest passage of the song: the assistant listens to it")
                        .to_owned(),
                    a.heard / LISTEN,
                )
            };
            ui.add(
                egui::ProgressBar::new(progress)
                    .desired_width(180.0)
                    .fill(color),
            );
            ui.add_space(8.0);
            ui.label(RichText::new(text).size(12.5));
        }
        Phase::Done => {
            let a = &mut state.assistant;
            ui.label(RichText::new("✦").size(16.0).color(color));
            ui.label(RichText::new(&a.found).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Keep")).clicked() {
                    a.phase = Phase::Idle;
                }
                if ui.button(tr("Undo")).clicked() {
                    params.edit_many(tr("Assistant"), &a.before);
                    a.phase = Phase::Idle;
                }
                ui.add_space(8.0);
                ui.label(RichText::new(tr("Assistant")).size(10.5).color(p.weak));
                let response = ui.add(
                    egui::Slider::new(&mut a.amount, 0.0..=100.0)
                        .show_value(false)
                        .trailing_fill(true),
                );
                ui.label(RichText::new(tr("Your settings")).size(10.5).color(p.weak));
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    let blend = blend(params, &a.before, &a.after, a.amount / 100.0);
                    params.edit_many(tr("Assistant"), &blend);
                }
            });
        }
    }
}

/// Between the settings before the assistant (0) and its own (1); named values switch half-way.
fn blend(
    params: &mut dyn EditorParams,
    before: &[(String, f32)],
    after: &[(String, f32)],
    t: f32,
) -> Vec<(String, f32)> {
    before
        .iter()
        .zip(after)
        .map(|((k, a), (_, b))| {
            let stepped = params
                .param(k)
                .is_some_and(|p| p.toggle || !p.choices.is_empty());
            let v = if stepped {
                if t >= 0.5 { *b } else { *a }
            } else {
                a + (b - a) * t
            };
            (k.clone(), v)
        })
        .collect()
}

fn percentile(values: &[f32], q: f32) -> f32 {
    let mut v: Vec<f32> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return -120.0;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q).round() as usize]
}

/// Slope of the average spectrum between `low` and `high` Hz, in dB per octave.
fn slope(a: &Assistant, low: f32, high: f32) -> f32 {
    let points: Vec<(f32, f32)> = (0..BANDS)
        .map(|b| (band_frequency(b), a.power[b] / f64::from(a.frames.max(1))))
        .filter(|(hz, power)| (low..high).contains(hz) && *power > 0.0)
        .map(|(hz, power)| (hz.log2(), (10.0 * power.log10()) as f32))
        .collect();
    let n = points.len() as f32;
    if n < 4.0 {
        return 0.0;
    }
    let (mx, my) = (
        points.iter().map(|p| p.0).sum::<f32>() / n,
        points.iter().map(|p| p.1).sum::<f32>() / n,
    );
    let (num, den) = points.iter().fold((0.0, 0.0), |(num, den), (x, y)| {
        (num + (x - mx) * (y - my), den + (x - mx).powi(2))
    });
    if den > 0.0 { num / den } else { 0.0 }
}

/// Energy of the average spectrum between `low` and `high` Hz against the whole (dB).
fn share(a: &Assistant, low: f32, high: f32) -> f32 {
    let (mut part, mut all) = (0.0f64, 0.0f64);
    for (b, &power) in a.power.iter().enumerate() {
        all += power;
        if (low..high).contains(&band_frequency(b)) {
            part += power;
        }
    }
    if all > 0.0 && part > 0.0 {
        (10.0 * (part / all).log10()) as f32
    } else {
        -60.0
    }
}

fn round5(v: f32) -> f32 {
    (v / 5.0).round() * 5.0
}

/// What the assistant sets after listening, and a sentence saying what it found.
fn suggest(
    params: &mut dyn EditorParams,
    a: &Assistant,
    kind: Kind,
) -> (Vec<(String, f32)>, String) {
    let loud = percentile(&a.loudness, 0.9);
    match kind {
        Kind::Master => {
            let style = (params.get("style").round().max(0.0) as usize).min(3);
            let wanted = [-4.5, -5.0, -4.0, -4.5][style];
            let measured = slope(a, 100.0, 10_000.0);
            let tone = round5(((wanted - measured) / 1.4 * 70.0).clamp(-60.0, 60.0));
            let crest = a.peak - loud;
            let glue = if crest > 14.0 {
                65.0
            } else if crest < 9.0 {
                35.0
            } else {
                50.0
            };
            let correlation = if a.correlation.1 > 0 {
                a.correlation.0 / a.correlation.1 as f32
            } else {
                0.7
            };
            let width = if correlation > 0.85 {
                120.0
            } else if correlation < 0.4 {
                85.0
            } else {
                100.0
            };
            // The lowest octave against the whole: thin mixes get some, boomy ones lose some.
            let sub_share = share(a, 30.0, 70.0);
            let sub = round5(((-14.0 - sub_share) * 8.0).clamp(-40.0, 40.0));
            let goal = target_lufs(params);
            let loudness = (goal - loud).clamp(-12.0, 18.0).round();
            let found = trf!(
                "Heard {lufs} LUFS, peaks {crest} dB above: tone, glue, width, sub and loudness set for this mix.",
                lufs = format!("{loud:.1}"),
                crest = format!("{crest:.0}")
            );
            let mut out = vec![
                ("tone".into(), tone),
                ("glue".into(), glue),
                ("width".into(), width),
                ("sub".into(), sub),
            ];
            if params.get("follow") < 0.5 {
                out.push(("loudness".into(), loudness));
            }
            (out, found)
        }
        Kind::Vocals => {
            let quiet = percentile(&a.loudness, 0.1);
            let middle = percentile(&a.loudness, 0.5);
            let spread = loud - quiet;
            let comp = if spread > 14.0 {
                65.0
            } else if spread < 7.0 {
                40.0
            } else {
                50.0
            };
            let measured = slope(a, 200.0, 8000.0);
            let tone = round5(((-3.0 - measured) / 1.6 * 60.0).clamp(-60.0, 60.0));
            let mut out: Vec<(String, f32)> = vec![
                ("tone".into(), tone),
                ("comp".into(), comp),
                (
                    "compressor.threshold".into(),
                    (middle + 2.0).clamp(-50.0, -6.0),
                ),
            ];
            let sibilance = percentile(&a.sibilance, 0.9);
            if sibilance > -100.0 {
                out.push((
                    "deesser.threshold".into(),
                    (sibilance - 4.0).clamp(-60.0, 0.0),
                ));
                let harsh = sibilance - loud > -6.0;
                out.push(("deess".into(), if harsh { 65.0 } else { 50.0 }));
            }
            let mut found = trf!(
                "Heard the voice at {lufs} LUFS: tone, compression and de-essing set for it.",
                lufs = format!("{middle:.1}")
            );
            if a.voiced > 3.0 {
                let (key, minor) = key_of(&a.notes);
                out.push(("pitch_correct.key".into(), key as f32));
                out.push(("pitch_correct.scale".into(), if minor { 2.0 } else { 1.0 }));
                let scale = if minor { tr("minor") } else { tr("major") };
                found = format!(
                    "{found} {}",
                    trf!(
                        "Key: {key} {scale}.",
                        key = audiostrada_dsp::pitch_correct::NOTE_NAMES[key],
                        scale = scale
                    )
                );
            }
            (out, found)
        }
    }
}

/// The key of the notes sung (seconds per pitch class), after Krumhansl's profiles.
fn key_of(notes: &[f32; 12]) -> (usize, bool) {
    const MAJOR: [f32; 12] = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const MINOR: [f32; 12] = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let fit = |profile: &[f32; 12], key: usize| -> f32 {
        let mean_n = notes.iter().sum::<f32>() / 12.0;
        let mean_p = profile.iter().sum::<f32>() / 12.0;
        let (mut num, mut a, mut b) = (0.0, 0.0, 0.0);
        for i in 0..12 {
            let x = notes[(i + key) % 12] - mean_n;
            let y = profile[i] - mean_p;
            num += x * y;
            a += x * x;
            b += y * y;
        }
        num / (a * b).sqrt().max(1e-9)
    };
    let mut best = (0, false, f32::MIN);
    for key in 0..12 {
        for (minor, profile) in [(false, &MAJOR), (true, &MINOR)] {
            let r = fit(profile, key);
            if r > best.2 {
                best = (key, minor, r);
            }
        }
    }
    (best.0, best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_found_from_the_notes_sung() {
        let mut notes = [0.0; 12];
        for n in [0, 2, 4, 5, 7, 9, 11] {
            notes[n] = 1.0;
        }
        notes[0] = 3.0;
        notes[7] = 2.0;
        assert_eq!(key_of(&notes), (0, false));
        let mut notes = [0.0; 12];
        for n in [9, 11, 0, 2, 4, 5, 7] {
            notes[n] = 1.0;
        }
        notes[9] = 3.0;
        notes[4] = 2.0;
        assert_eq!(key_of(&notes), (9, true));
    }

    #[test]
    fn both_chains_have_a_knob_or_a_tab_for_each_of_their_stages() {
        for (kind, stages) in [
            (Kind::Master, &master::STAGES[..]),
            (Kind::Vocals, &vocals::STAGES[..]),
        ] {
            for stage in stages {
                let reached = knobs(kind).iter().any(|k| k.stages.contains(stage));
                assert!(reached || *stage == "limiter", "no knob opens {stage}");
            }
        }
    }
}
