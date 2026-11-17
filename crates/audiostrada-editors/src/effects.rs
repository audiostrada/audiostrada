//! The editors of the built-in effects drawn here rather than in a file of their own:
//! dynamics, levels, modulation, filter, tuner, Pitch Correct and the analyzer.

use std::collections::VecDeque;

use egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, pos2,
    vec2,
};

use crate::controls::*;
use crate::i18n::tr;
use crate::{theme, trf};

/// The memory of the built-in effects' editors between frames.
#[derive(Debug, Default)]
pub struct EffectsState {
    /// Gain reduction of the last frames, newest last.
    history: VecDeque<(f32, f32)>,
    /// Gain reduction, input and output of the last frames (dB), newest last.
    dynamics: VecDeque<[f32; 3]>,
    /// Position of an analog meter's needle, with its ballistics.
    needle: f32,
    /// The multiband compressor's chosen band and solo.
    pub mb: crate::mb_editor::MbState,
    /// The tape machine's needles and reels.
    pub tape: crate::tape_editor::TapeState,
    limiter: LimiterState,
    spectrum: SpectrumState,
    /// Center of the Pitch Correct view, in semitones (0 before any pitch).
    pitch_view: f32,
    pub verb: crate::verb_editor::VerbState,
    pub delay: crate::delay_editor::DelayState,
    pub drive: crate::drive_editor::DriveState,
    pub eq: crate::eq_editor::EqState,
    pub chain: crate::chain_editor::ChainState,
}

impl EffectsState {
    /// The window closed: what is heard only while it is open stops, a chain shows its knobs.
    pub fn close(&mut self) {
        self.limiter.comparing = false;
        self.chain.close();
    }
}

/// Draws the editor of a built-in effect; parameters without one get knobs.
pub fn show(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EffectsState) {
    match params.info().id {
        "mastering" | "vocals" => crate::chain_editor::show(ui, params, meters, &mut state.chain),
        "eq" => crate::eq_editor::show(ui, params, meters, &mut state.eq),
        "compressor" => compressor(ui, params, meters, state),
        "tuner" => tuner(ui, params, meters),
        "pitch_correct" => pitch_correct(ui, params, meters, state),
        "analyzer" => analyzer(ui, params, meters, &mut state.spectrum),
        "delay" => crate::delay_editor::show(ui, params, meters, &mut state.delay),
        "reverb" => crate::verb_editor::show(ui, params, meters, &mut state.verb),
        "maximizer" => maximizer(ui, params, meters, state),
        "tape" => crate::tape_editor::show(ui, params, meters, &mut state.tape),
        "mb_comp" => crate::mb_editor::show(ui, params, meters, &mut state.mb),
        "modulator" => crate::mod_editor::show(ui, params, meters),
        "pitch" => crate::pitch_editor::pitch(ui, params),
        "doubler" => crate::pitch_editor::doubler(ui, params),
        "flanger" | "phaser" | "chorus" => modulation(ui, params),
        "imager" => imager(ui, params, meters),
        "utility" => utility(ui, params, meters),
        "transient" => transient(ui, params, meters, state),
        "deesser" => deesser(ui, params, meters, state),
        "vintage_eq" => vintage_eq(ui, params),
        "filter" => filter(ui, params, meters),
        "drive" => crate::drive_editor::show(ui, params, meters, &mut state.drive),
        "gate" => gate(ui, params, meters, state),
        "limiter" => limiter(ui, params, meters, state),
        _ => knob_panel(ui, params, meters),
    }
}

/// Knobs for continuous parameters, switches above them, meters as bars.
fn knob_panel(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    let p = theme::current(ui);
    if params.info().params.iter().any(|q| q.toggle) {
        ui.horizontal_wrapped(|ui| toggles(ui, params));
        ui.add_space(6.0);
    }
    for (name, value) in params.info().meters.iter().zip(meters) {
        if name.is_empty() {
            continue;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(name)).size(10.5).color(p.weak));
            let (rect, _) = ui.allocate_exact_size(vec2(220.0, 8.0), Sense::hover());
            ui.painter().rect_filled(rect, 3.0, theme::screen(&p));
            let f = (-value / 24.0).clamp(0.0, 1.0);
            let bar = Rect::from_min_max(
                rect.min,
                pos2(rect.left() + rect.width() * f, rect.bottom()),
            );
            ui.painter().rect_filled(bar, 3.0, theme::RECORD);
            ui.label(
                RichText::new(format!("{value:.1} dB"))
                    .monospace()
                    .size(11.0),
            );
        });
    }
    let ids: Vec<&'static str> = params
        .info()
        .params
        .iter()
        .filter(|q| !q.toggle)
        .map(|q| q.id)
        .collect();
    for row in ids.chunks(6) {
        ui.horizontal(|ui| knobs(ui, params, row, 50.0, p.accent));
        ui.add_space(4.0);
    }
}

// ---------------------------------------------------------------- Tuner

fn tuner(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let p = theme::current(ui);
    let frequency = meters.first().copied().unwrap_or(0.0);
    let clarity = meters.get(1).copied().unwrap_or(0.0);
    let reference = params.get("reference");
    let strobe = params.get("view") >= 0.5;
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(360.0), 200.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    let heard = (clarity > 0.5 && frequency > 0.0)
        .then(|| audiostrada_dsp::tuner::note(frequency, reference));
    let color_of = |cents: f32| {
        if cents.abs() < 3.0 {
            theme::PLAY
        } else if cents.abs() < 15.0 {
            theme::SOLO
        } else {
            theme::RECORD
        }
    };
    let center = pos2(rect.center().x, rect.top() + 74.0);
    let width = rect.width() * 0.42;
    if strobe {
        // Bands that run right when sharp, left when flat, and stand still in tune; the
        // lower rows move faster, to see a few cents.
        let key = ui.scope_id().with(("strobe", params.salt()));
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let cents = heard.map_or(0.0, |(_, c)| c);
        let phase: f32 = ui.data_mut(|d| {
            let phase = d.get_temp_mut_or_default::<f32>(key);
            *phase = (*phase + cents * dt * 0.08).rem_euclid(1.0);
            *phase
        });
        let band = Rect::from_center_size(center, vec2(width * 2.0, 84.0));
        painter.rect_filled(band, 6.0, p.panel);
        let color = heard.map_or(p.weak.gamma_multiply(0.4), |(_, c)| color_of(c));
        for (row, speed) in [1.0f32, 2.0, 4.0].into_iter().enumerate() {
            let h = band.height() / 3.0;
            let top = band.top() + row as f32 * h;
            let period = 28.0 / speed.sqrt();
            let shift = (phase * speed).fract() * period;
            let mut x = band.left() - period + shift;
            while x < band.right() {
                let mark = Rect::from_min_max(
                    pos2(x.max(band.left()), top + 3.0),
                    pos2((x + period * 0.5).min(band.right()), top + h - 3.0),
                );
                if mark.width() > 0.0 {
                    painter.rect_filled(mark, 2.0, color);
                }
                x += period;
            }
        }
    } else {
        for cents in (-50..=50).step_by(10) {
            let x = center.x + cents as f32 / 50.0 * width;
            let tall = cents % 50 == 0 || cents == 0;
            painter.line_segment(
                [
                    pos2(x, center.y - if tall { 26.0 } else { 14.0 }),
                    pos2(x, center.y),
                ],
                Stroke::new(if cents == 0 { 2.0 } else { 1.0 }, p.weak),
            );
        }
        if let Some((_, cents)) = heard {
            let x = center.x + cents.clamp(-50.0, 50.0) / 50.0 * width;
            painter.line_segment(
                [pos2(x, center.y - 34.0), pos2(x, center.y + 4.0)],
                Stroke::new(3.0, color_of(cents)),
            );
        }
    }
    if let Some((number, cents)) = heard {
        let name = NAMES[number.rem_euclid(12) as usize];
        let octave = number.div_euclid(12) - 1;
        painter.text(
            pos2(center.x, rect.bottom() - 40.0),
            Align2::CENTER_CENTER,
            format!("{name}{octave}"),
            theme::strong(40.0),
            color_of(cents),
        );
        painter.text(
            pos2(rect.right() - 10.0, rect.bottom() - 8.0),
            Align2::RIGHT_BOTTOM,
            format!("{frequency:.1} Hz  {cents:+.0} cents"),
            FontId::monospace(12.0),
            p.weak,
        );
    } else {
        painter.text(
            pos2(center.x, rect.bottom() - 40.0),
            Align2::CENTER_CENTER,
            tr("Play a note"),
            theme::strong(18.0),
            p.weak,
        );
    }
    crate::controls::animate(ui.ctx());
    ui.add_space(8.0);
    let accent = p.accent;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("View"), p.weak, 90.0, |ui| {
            ui.horizontal(|ui| {
                segmented(
                    ui,
                    params,
                    "view",
                    &[
                        "A needle over the cents",
                        "Bands that run while out of tune and stand still in tune",
                    ],
                )
            });
        });
        section_frame(ui, tr("Tuning"), p.weak, 90.0, |ui| {
            wide_knob(ui, params, "reference", tr("Reference A4"), 40.0, accent);
        });
        section_frame(ui, tr("Output"), p.weak, 90.0, |ui| {
            params.toggle(ui, "mute", |ui, on| {
                let response = pill_switch(ui, *on, tr("Mute while tuning"));
                if response.clicked() {
                    *on = !*on;
                }
                response
            });
        });
    });
}

/// Pitch Correct: the sung pitch and the corrected one scrolling over the notes of the scale,
/// the note sung, the one aimed at and the move between them, then the settings.
fn pitch_correct(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut EffectsState,
) {
    use audiostrada_dsp::pitch_correct::{
        HARMONY, KEY_METER, MODE_HINTS, MODES, NOTE_NAMES, SCALE_NAMES, STAIRCASE, in_scale,
    };
    const SPAN: f32 = 14.0;
    let mode = params.get("mode").round().max(0.0) as usize;
    cards(ui, params, "mode", &MODES, &MODE_HINTS);
    ui.add_space(6.0);
    let notes = params.get("notes").round().max(0.0) as u32;
    let p = theme::current(ui);
    let meter = |i: usize| meters.get(i).copied().unwrap_or(0.0);
    let (input, output, target) = (meter(0), meter(1), meter(2));
    push_history(state, (input, output));
    if output > 0.0 {
        state.pitch_view = if state.pitch_view > 0.0 {
            state.pitch_view + 0.03 * (output - state.pitch_view)
        } else {
            output
        };
    }
    let view = if state.pitch_view > 0.0 {
        state.pitch_view
    } else {
        60.0
    };
    let key = params.get("key").round() as i32;
    let scale = params.get("scale").round().max(0.0) as usize;
    let (rect, painter) = display(ui, 230.0);
    let graph = Rect::from_min_max(
        rect.min + vec2(40.0, 8.0),
        pos2(rect.right() - 200.0, rect.bottom() - 8.0),
    );
    let low = view - SPAN / 2.0;
    let to_y = |n: f32| graph.bottom() - (n - low) / SPAN * graph.height();
    let row = graph.height() / SPAN;
    let name = |n: i32| {
        format!(
            "{}{}",
            NOTE_NAMES[n.rem_euclid(12) as usize],
            n.div_euclid(12) - 1
        )
    };
    for n in low.floor() as i32..=(low + SPAN).ceil() as i32 {
        let y = to_y(n as f32);
        let band = Rect::from_min_max(
            pos2(graph.left(), y - row / 2.0),
            pos2(graph.right(), y + row / 2.0),
        );
        let used = in_scale(n, key, scale, notes);
        if target > 0.0 && n == target as i32 {
            painter.rect_filled(band, 0.0, p.accent.gamma_multiply(0.18));
        } else if used && scale > 0 {
            painter.rect_filled(band, 0.0, p.panel.gamma_multiply(0.6));
        }
        painter.line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(1.0, p.weak.gamma_multiply(if used { 0.25 } else { 0.08 })),
        );
        if used {
            painter.text(
                pos2(rect.left() + 8.0, y),
                Align2::LEFT_CENTER,
                name(n),
                FontId::monospace(10.0),
                p.weak,
            );
        }
    }
    let step = graph.width() / HISTORY as f32;
    let x0 = graph.right() - state.history.len() as f32 * step;
    for (pick, color, width) in [(0, p.weak.gamma_multiply(0.7), 1.5), (1, p.accent, 2.5)] {
        let value = |v: (f32, f32)| if pick == 0 { v.0 } else { v.1 };
        let mut line = Vec::new();
        for (i, &v) in state.history.iter().enumerate() {
            if value(v) > 0.0 {
                line.push(pos2(x0 + i as f32 * step, to_y(value(v))));
            } else if !line.is_empty() {
                painter.add(Shape::line(
                    std::mem::take(&mut line),
                    Stroke::new(width, color),
                ));
            }
        }
        painter.add(Shape::line(line, Stroke::new(width, color)));
    }
    for (i, (text, color)) in [(tr("Input pitch"), p.weak), (tr("Output pitch"), p.accent)]
        .into_iter()
        .enumerate()
    {
        // Under the curves, which the pitch sung keeps in the middle.
        painter.text(
            graph.left_bottom() + vec2(8.0 + i as f32 * 96.0, -4.0),
            Align2::LEFT_BOTTOM,
            text,
            FontId::proportional(10.5),
            color,
        );
    }
    // At the right: the note sung, the one aimed at, and the move from one to the other.
    let side = Rect::from_min_max(
        pos2(graph.right() + 12.0, rect.top() + 8.0),
        rect.max - vec2(10.0, 8.0),
    );
    let middle = side.center().x;
    if target > 0.0 && input > 0.0 {
        let sung = input.round() as i32;
        painter.text(
            pos2(middle, side.top() + 10.0),
            Align2::CENTER_CENTER,
            tr("Sung"),
            FontId::proportional(10.5),
            p.weak,
        );
        painter.text(
            pos2(middle, side.top() + 30.0),
            Align2::CENTER_CENTER,
            format!("{}  {:+.0}", name(sung), (input - sung as f32) * 100.0),
            FontId::monospace(15.0),
            p.text,
        );
        painter.text(
            pos2(middle, side.top() + 62.0),
            Align2::CENTER_CENTER,
            tr("Target"),
            FontId::proportional(10.5),
            p.weak,
        );
        painter.text(
            pos2(middle, side.top() + 96.0),
            Align2::CENTER_CENTER,
            name(target as i32),
            theme::strong(40.0),
            p.accent,
        );
        // The move: a bar from the middle, a semitone at each end.
        let moved = (output - input) * 100.0;
        let bar = Rect::from_center_size(
            pos2(middle, side.bottom() - 44.0),
            vec2(side.width() - 8.0, 8.0),
        );
        painter.rect_filled(bar, 4.0, p.panel);
        let reach = (moved / 100.0).clamp(-1.0, 1.0) * bar.width() / 2.0;
        let fill = Rect::from_x_y_ranges(
            bar.center().x.min(bar.center().x + reach)..=bar.center().x.max(bar.center().x + reach),
            bar.y_range(),
        );
        painter.rect_filled(fill, 4.0, p.accent);
        painter.line_segment(
            [
                pos2(bar.center().x, bar.top() - 3.0),
                pos2(bar.center().x, bar.bottom() + 3.0),
            ],
            Stroke::new(1.0, p.weak),
        );
        painter.text(
            pos2(middle, side.bottom() - 22.0),
            Align2::CENTER_CENTER,
            trf!("Moved by {cents} cents", cents = format!("{moved:+.0}")),
            FontId::monospace(11.5),
            p.weak,
        );
    } else {
        painter.text(
            side.center(),
            Align2::CENTER_CENTER,
            tr("Sing or play a note"),
            theme::strong(14.0),
            p.weak,
        );
    }
    crate::controls::animate(ui.ctx());
    ui.add_space(8.0);
    // The key the engine heard in what was sung, to take as the scale.
    let meter = |i: usize| meters.get(i).copied().unwrap_or(-1.0);
    let heard = (meter(KEY_METER) >= 0.0 && meter(KEY_METER + 1) >= 1.0).then(|| {
        (
            meter(KEY_METER).round() as usize % 12,
            meter(KEY_METER + 1).round() as usize,
            meter(KEY_METER + 2),
        )
    });
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("Scale"), p.weak, 146.0, |ui| {
            ui.horizontal(|ui| {
                choice_box(ui, params, "key");
                choice_box(ui, params, "scale");
            });
            ui.add_space(4.0);
            scale_keyboard(ui, params, key, scale, notes);
            ui.add_space(4.0);
            ui.horizontal(|ui| match heard {
                Some((root, kind, sure)) => {
                    let label = trf!(
                        "Heard: {key} {scale}",
                        key = NOTE_NAMES[root],
                        scale = tr(SCALE_NAMES[kind.min(SCALE_NAMES.len() - 1)]).to_lowercase()
                    );
                    ui.label(RichText::new(label).size(11.5).color(p.text))
                        .on_hover_text(trf!(
                            "The key of what was sung so far, {percent} % sure",
                            percent = format!("{:.0}", sure * 100.0)
                        ));
                    let same = key == root as i32 && scale == kind;
                    let response = ui
                        .add_enabled(!same, egui::Button::new(tr("Use this key")).small())
                        .on_hover_text(tr("Correct to the key heard"));
                    if response.clicked() {
                        params.edit_many(
                            tr("Use this key"),
                            &[
                                ("key".to_owned(), root as f32),
                                ("scale".to_owned(), kind as f32),
                            ],
                        );
                    }
                }
                None => {
                    let text = tr("Heard: listening for the key...");
                    ui.label(RichText::new(text).size(11.5).color(p.weak));
                }
            });
        });
        // The staircase goes straight to the note: these settings are not its own.
        let stepped = mode == STAIRCASE;
        knob_section_items(
            ui,
            params,
            &[
                ("Correction", &["speed", "humanize", "tolerance"], None),
                ("Output", &["amount", "transpose"], None),
            ],
            &|id| !(stepped && matches!(id, "speed" | "humanize" | "tolerance" | "amount")),
        );
        section_frame(ui, tr("Harmony"), p.weak, 118.0, |ui| {
            ui.add_enabled_ui(mode == HARMONY, |ui| {
                ui.vertical(|ui| {
                    choice_box(ui, params, "harmony");
                    ui.add_space(2.0);
                    wide_knob(ui, params, "harmony_level", tr("Level"), 44.0, p.accent);
                });
            });
        });
    });
}

/// An octave of keys, lit for the notes the voice is corrected to: a click picks or drops a
/// note, which makes the scale a custom one.
fn scale_keyboard(ui: &mut Ui, params: &mut dyn EditorParams, key: i32, scale: usize, notes: u32) {
    use audiostrada_dsp::pitch_correct::{CUSTOM, in_scale};
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(196.0, 52.0), Sense::click());
    let painter = ui.painter_at(rect);
    let lit = |pc: i32| in_scale(pc, key, scale, notes);
    let white = [0, 2, 4, 5, 7, 9, 11];
    let w = rect.width() / 7.0;
    let mut keys: Vec<(i32, Rect)> = Vec::new();
    for (i, &pc) in white.iter().enumerate() {
        let r = Rect::from_min_size(
            rect.min + vec2(i as f32 * w, 0.0),
            vec2(w - 1.0, rect.height()),
        );
        painter.rect_filled(
            r,
            3.0,
            if lit(pc) {
                Color32::from_gray(225).lerp_to_gamma(p.accent, 0.55)
            } else {
                Color32::from_gray(205)
            },
        );
        keys.push((pc, r));
    }
    for (after, pc) in [(0, 1), (1, 3), (3, 6), (4, 8), (5, 10)] {
        let x = rect.left() + (after + 1) as f32 * w - w * 0.3;
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w * 0.6, rect.height() * 0.6));
        painter.rect_filled(
            r,
            2.0,
            if lit(pc) {
                Color32::from_gray(30).lerp_to_gamma(p.accent, 0.75)
            } else {
                Color32::from_gray(35)
            },
        );
        painter.rect_stroke(
            r,
            2.0,
            Stroke::new(1.0, Color32::from_gray(20)),
            egui::StrokeKind::Inside,
        );
        keys.push((pc, r));
    }
    // Black keys are on top: looked for first.
    let hit = response
        .interact_pointer_pos()
        .and_then(|at| keys.iter().rev().find(|(_, r)| r.contains(at)).map(|k| k.0));
    let response = response.on_hover_text(tr(
        "Click a note to add it to or take it from the scale: it becomes a custom one",
    ));
    let mask: u32 = (0..12).filter(|&pc| lit(pc)).fold(0, |m, pc| m | 1 << pc);
    let clicked = response.clicked().then_some(hit).flatten();
    params.set(
        "notes",
        clicked.map(|pc| (mask ^ 1 << pc) as f32),
        &response,
    );
    if clicked.is_some() {
        params.set("scale", Some(CUSTOM as f32), &response);
        params.end_gesture();
    }
}

// ---------------------------------------------------------------- Shared drawing

/// Remembers one frame of two meter values for the scrolling displays.
fn push_history(state: &mut EffectsState, values: (f32, f32)) {
    state.history.push_back(values);
    while state.history.len() > HISTORY {
        state.history.pop_front();
    }
}

/// A button for each on/off parameter of the effect.
fn toggles(ui: &mut Ui, params: &mut dyn EditorParams) {
    for q in params.info().params.iter().filter(|q| q.toggle) {
        params.toggle(ui, q.id, |ui, on| ui.toggle_value(on, tr(q.name)));
    }
}

// ---------------------------------------------------------------- Gate

fn gate(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EffectsState) {
    use audiostrada_dsp::gate::{DUCKER, EXPANDER, MODE_HINTS, MODES, expander_db};
    let mode = (params.get("mode").round().max(0.0) as usize).min(MODES.len() - 1);
    let gain = meters.first().copied().unwrap_or(0.0);
    let input = meters.get(1).copied().unwrap_or(-120.0);
    state.dynamics.push_back([gain, input, input + gain]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    cards(ui, params, "mode", &MODES, &MODE_HINTS);
    ui.add_space(8.0);
    let threshold = params.get("threshold");
    let close = threshold - params.get("hysteresis");
    let (range, ratio) = (params.get("range"), params.get("ratio"));
    let height = 230.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let width = (ui.available_width() - height - 60.0).max(260.0);
        dynamics_history(
            ui,
            params,
            state,
            vec2(width, height),
            (threshold, Some(close)),
            (gain, range.abs().max(6.0)),
        );
        // Output level for an input level, the gate open above the threshold.
        let out_of = |level: f32| match mode {
            DUCKER if level >= threshold => level + range,
            DUCKER => level,
            _ if level >= threshold => level,
            EXPANDER => level + expander_db(level, close, ratio, range),
            _ => level + range,
        };
        transfer_curve(ui, params, &out_of, threshold, input, height);
        gate_light(ui, gain, range, mode, height);
    });
    ui.add_space(8.0);
    knob_sections(
        ui,
        params,
        &[
            (
                "Detection",
                &["threshold", "hysteresis", "range", "ratio"],
                None,
            ),
            ("Time", &["attack", "hold", "release", "lookahead"], None),
            (
                "Sidechain",
                &["sc_hpf", "sc_lpf"],
                Some((
                    "listen",
                    "Listen",
                    "Hear what the detector hears: the key through its filters",
                )),
            ),
        ],
        &|id| id != "ratio" || mode == EXPANDER,
    );
}

/// Whether the gate lets the sound through, as a lamp, and its gain as a bar.
fn gate_light(ui: &mut Ui, gain: f32, range: f32, mode: usize, height: f32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(44.0, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let floor = range.min(-1.0);
    let depth = (gain / floor).clamp(0.0, 1.0);
    // A ducker is "open" while it lowers the sound: the lamp shows it working.
    let lit = if mode == audiostrada_dsp::gate::DUCKER {
        depth > 0.5
    } else {
        depth < 0.5
    };
    let lamp = pos2(rect.center().x, rect.top() + 12.0);
    let color = if lit {
        theme::PLAY
    } else {
        p.weak.gamma_multiply(0.4)
    };
    if lit {
        painter.circle_filled(lamp, 11.0, color.gamma_multiply(0.18));
    }
    painter.circle_filled(lamp, 6.5, color);
    let bar = Rect::from_min_max(
        pos2(rect.center().x - 6.0, rect.top() + 30.0),
        pos2(rect.center().x + 6.0, rect.bottom() - 16.0),
    );
    painter.rect_filled(bar, 4.0, theme::screen(&p));
    let open = 1.0 - depth;
    painter.rect_filled(
        Rect::from_min_max(
            pos2(bar.left(), bar.bottom() - open * bar.height()),
            bar.max,
        ),
        4.0,
        theme::PLAY.gamma_multiply(0.8),
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 2.0),
        Align2::CENTER_BOTTOM,
        format!("{:.0}", gain.round() + 0.0),
        FontId::monospace(10.0),
        p.weak,
    );
}

// ---------------------------------------------------------------- Vintage compressor

// ---------------------------------------------------------------- New effects

/// Knobs of one size for parameter ids, in the current layout.
fn knobs(ui: &mut Ui, params: &mut dyn EditorParams, ids: &[&str], size: f32, tint: Color32) {
    for id in ids {
        param_knob(ui, params, id, size, tint);
    }
}

/// A module's on/off switch: the power sign and its name, lit in its color while on.
pub fn module_switch(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    title: &str,
    tint: Color32,
) {
    let p = theme::current(ui);
    let on = params.get(id) >= 0.5;
    let galley =
        ui.painter()
            .layout_no_wrap(title.to_owned(), FontId::proportional(13.0), Color32::WHITE);
    let size = vec2(galley.size().x + 32.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = match (on, response.hovered()) {
        (true, _) => tint.gamma_multiply(0.3),
        (false, true) => p.hover,
        (false, false) => p.surface,
    };
    let ink = if on { p.text } else { p.weak };
    ui.painter().rect_filled(rect, 5.0, fill);
    theme::paint_power(
        ui.painter(),
        Rect::from_min_size(rect.min + vec2(3.0, 2.0), vec2(18.0, 18.0)),
        theme::power_ink(on, &p),
    );
    ui.painter().galley_with_override_text_color(
        pos2(rect.left() + 23.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    let response = response.on_hover_text(if on {
        tr("On: click to switch the module off")
    } else {
        tr("Off: click to switch the module on")
    });
    let changed = response.clicked().then_some(if on { 0.0 } else { 1.0 });
    params.set(id, changed, &response);
}

/// A dark display area with a subtle frame.
pub fn display(ui: &mut Ui, height: f32) -> (Rect, egui::Painter) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width().max(420.0), height),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, p.weak.gamma_multiply(0.15)),
        egui::StrokeKind::Inside,
    );
    (rect, painter)
}

/// The LFO of a modulation effect drawn over two cycles, with a dot running along it.
fn modulation(ui: &mut Ui, params: &mut dyn EditorParams) {
    let p = theme::current(ui);
    let id = params.info().id;
    let synced = params.get("sync") >= 0.5;
    let rate = if synced {
        let bpm = params.tempo();
        1.0 / audiostrada_dsp::delay::synced_seconds(params.get("note"), bpm).max(0.01)
    } else {
        params.get("rate")
    };
    let depth = params.get("depth") / 100.0;
    let stereo = params.get("stereo") / 360.0;
    let (rect, painter) = display(ui, 130.0);
    let inner = rect.shrink2(vec2(14.0, 18.0));
    let wave = |phase: f32| -> f32 {
        match id {
            "flanger" => {
                let t = phase.rem_euclid(1.0);
                if t < 0.5 { t * 2.0 } else { 2.0 - t * 2.0 }
            }
            "phaser" => 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos(),
            _ => 0.5 + 0.5 * (phase * std::f32::consts::TAU).sin(),
        }
    };
    let point = |phase: f32, x: f32| {
        pos2(
            x,
            inner.bottom() - (0.5 + (wave(phase) - 0.5) * depth) * inner.height(),
        )
    };
    for (offset, color) in [(0.0, p.accent), (stereo, theme::SOLO)] {
        let line: Vec<Pos2> = (0..=200)
            .map(|i| {
                let t = i as f32 / 200.0;
                point(t * 2.0 + offset, inner.left() + t * inner.width())
            })
            .collect();
        painter.add(Shape::line(line, Stroke::new(2.0, color)));
    }
    let time = ui.input(|i| i.time) as f32;
    let phase = (time * rate).fract();
    for (offset, color) in [(0.0, p.accent), (stereo, theme::SOLO)] {
        let x = inner.left() + phase * 0.5 * inner.width();
        painter.circle_filled(point(phase + offset, x), 5.0, color);
        let x2 = x + 0.5 * inner.width();
        painter.circle_filled(point(phase + offset, x2), 5.0, color.gamma_multiply(0.5));
    }
    painter.text(
        rect.right_top() + vec2(-10.0, 8.0),
        Align2::RIGHT_TOP,
        format!("{rate:.2} Hz"),
        theme::strong(14.0),
        p.accent,
    );
    painter.text(
        rect.left_top() + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        "L",
        FontId::proportional(10.0),
        p.accent,
    );
    painter.text(
        rect.left_top() + vec2(22.0, 8.0),
        Align2::LEFT_TOP,
        "R",
        FontId::proportional(10.0),
        theme::SOLO,
    );
    crate::controls::animate(ui.ctx());
    ui.add_space(8.0);
    let accent = p.accent;
    let knob = |ui: &mut Ui, params: &mut dyn EditorParams, id: &str| {
        let name = params.param(id).map_or("", |q| tr(q.name));
        wide_knob(ui, params, id, name, 44.0, accent);
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("LFO"), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                knob(ui, params, if synced { "note" } else { "rate" });
                for id in ["depth", "stereo"] {
                    knob(ui, params, id);
                }
            });
            ui.add_space(4.0);
            params.toggle(ui, "sync", |ui, on| {
                let response = pill_switch(ui, *on, tr("Sync"))
                    .on_hover_text(tr("A note value at the project's tempo"));
                if response.clicked() {
                    *on = !*on;
                }
                response
            });
        });
        match id {
            "flanger" => {
                section_frame(ui, tr("Comb"), p.weak, 118.0, |ui| {
                    let zero = params.get("through_zero") >= 0.5;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_enabled_ui(!zero, |ui| knob(ui, params, "delay"));
                        knob(ui, params, "feedback");
                    });
                    ui.add_space(4.0);
                    params.toggle(ui, "through_zero", |ui, on| {
                        let response = pill_switch(ui, *on, tr("Through zero")).on_hover_text(tr(
                            "As with two tape machines: the sweep passes the dry sound and \
                             cancels it for a moment. Delays the output by 5 ms.",
                        ));
                        if response.clicked() {
                            *on = !*on;
                        }
                        response
                    });
                });
                section_frame(ui, tr("Output"), p.weak, 118.0, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        knob(ui, params, "mix");
                        knob(ui, params, "output");
                    });
                });
            }
            "phaser" => {
                section_frame(ui, tr("Sweep"), p.weak, 118.0, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for id in ["low", "high", "feedback"] {
                            knob(ui, params, id);
                        }
                    });
                    ui.horizontal(|ui| segmented(ui, params, "stages", &[]));
                });
                section_frame(ui, tr("Output"), p.weak, 118.0, |ui| {
                    knob(ui, params, "mix");
                });
            }
            _ => {
                section_frame(ui, tr("Voices"), p.weak, 118.0, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for id in ["delay", "feedback", "tone"] {
                            knob(ui, params, id);
                        }
                    });
                    ui.horizontal(|ui| {
                        segmented(
                            ui,
                            params,
                            "voices",
                            &[
                                "One delay per side",
                                "Two delays per side, apart in the cycle",
                                "Three delays per side, as in the string ensembles",
                            ],
                        )
                    });
                });
                section_frame(ui, tr("Output"), p.weak, 118.0, |ui| {
                    knob(ui, params, "mix");
                });
            }
        }
    });
}

/// Utility: the phase correlation and the left/right balance read live, then the level and
/// stereo controls, then the phase tools.
fn utility(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    let p = theme::current(ui);
    let correlation = meters.first().copied().unwrap_or(1.0);
    let lean = meters.get(1).copied().unwrap_or(0.0);
    let (rect, painter) = display(ui, 86.0);
    // Two scales: correlation from -1 to +1, balance from left to right (12 dB each way).
    let bar = |row: usize, at: f32, left: &str, right: &str, title: String, warn: bool| {
        let y = rect.top() + 30.0 + row as f32 * 38.0;
        let line = Rect::from_min_max(
            pos2(rect.left() + 60.0, y - 3.0),
            pos2(rect.right() - 60.0, y + 3.0),
        );
        painter.rect_filled(line, 3.0, p.panel);
        painter.line_segment(
            [
                pos2(line.center().x, line.top() - 4.0),
                pos2(line.center().x, line.bottom() + 4.0),
            ],
            Stroke::new(1.0, p.weak.gamma_multiply(0.5)),
        );
        let x = line.left() + at.clamp(0.0, 1.0) * line.width();
        painter.circle_filled(
            pos2(x, line.center().y),
            6.0,
            if warn { theme::RECORD } else { theme::PLAY },
        );
        let small = FontId::proportional(10.0);
        painter.text(
            pos2(line.left() - 10.0, y),
            Align2::RIGHT_CENTER,
            left,
            small.clone(),
            p.weak,
        );
        painter.text(
            pos2(line.right() + 10.0, y),
            Align2::LEFT_CENTER,
            right,
            small,
            p.weak,
        );
        painter.text(
            pos2(line.center().x, line.top() - 6.0),
            Align2::CENTER_BOTTOM,
            title,
            FontId::proportional(11.0),
            p.text,
        );
    };
    bar(
        0,
        (correlation + 1.0) / 2.0,
        "-1",
        "+1",
        format!("{} {correlation:+.2}", tr("Correlation")),
        correlation < 0.0,
    );
    let side = match lean {
        l if l > 0.5 => trf!("{db} dB to the right", db = format!("{l:.1}")),
        l if l < -0.5 => trf!("{db} dB to the left", db = format!("{:.1}", -l)),
        _ => tr("Centered").to_owned(),
    };
    bar(
        1,
        (lean / 12.0 + 1.0) / 2.0,
        tr("L"),
        tr("R"),
        format!("{} {side}", tr("Balance")),
        lean.abs() > 3.0,
    );
    ui.add_space(8.0);
    let accent = p.accent;
    let section = |ui: &mut Ui,
                   params: &mut dyn EditorParams,
                   title: &'static str,
                   knobs: &[&str],
                   pills: &[(&str, &'static str)]| {
        section_frame(ui, tr(title), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for &id in knobs {
                    let name = params.param(id).map_or("", |q| tr(q.name));
                    wide_knob(ui, params, id, name, 44.0, accent);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for &(id, label) in pills {
                    params.toggle(ui, id, |ui, on| {
                        let response = pill_switch(ui, *on, tr(label));
                        if response.clicked() {
                            *on = !*on;
                        }
                        response
                    });
                }
            });
        });
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section(
            ui,
            params,
            "Level",
            &["gain", "balance"],
            &[("dc", "DC filter")],
        );
        section(
            ui,
            params,
            "Stereo",
            &["width", "bass_mono"],
            &[("mono", "Mono"), ("swap", "Swap channels")],
        );
        section(
            ui,
            params,
            "Phase",
            &["rotation", "rotation_freq"],
            &[
                ("invert_left", "Invert left"),
                ("invert_right", "Invert right"),
            ],
        );
        section(ui, params, "Alignment", &["delay_left", "delay_right"], &[]);
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new(tr(
            "Delay a side to line up two microphones; turn the phase to match the lows of two \
             sources (DI and amp, two kick mics) where polarity alone is too much.",
        ))
        .size(10.5)
        .color(p.weak),
    );
}

fn imager(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    let p = theme::current(ui);
    let correlation = meters.first().copied().unwrap_or(1.0);
    let height = 220.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(vec2(height, height), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, theme::screen(&p));
        // Goniometer: mid up, side across, rotated 45 degrees as on the classic scopes.
        let size = rect.height() - 24.0;
        let scope = Rect::from_center_size(rect.center(), vec2(size, size));
        let c = scope.center();
        let grid = p.weak.gamma_multiply(0.2);
        painter.circle_stroke(c, size / 2.0, Stroke::new(1.0, grid));
        painter.line_segment(
            [pos2(c.x, scope.top()), pos2(c.x, scope.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.line_segment(
            [pos2(scope.left(), c.y), pos2(scope.right(), c.y)],
            Stroke::new(1.0, grid),
        );
        painter.text(
            scope.left_top(),
            Align2::LEFT_TOP,
            "L",
            FontId::proportional(10.0),
            p.weak,
        );
        painter.text(
            scope.right_top(),
            Align2::RIGHT_TOP,
            "R",
            FontId::proportional(10.0),
            p.weak,
        );
        let points: Vec<(f32, f32)> = meters
            .get(2..)
            .unwrap_or(&[])
            .chunks(2)
            .map(|v| (v[0], v.get(1).copied().unwrap_or(0.0)))
            .collect();
        let peak = points
            .iter()
            .fold(0.05f32, |m, (a, b)| m.max(a.abs()).max(b.abs()));
        let scale = size / 2.0 * 0.9 / peak;
        for (i, (m, s)) in points.iter().enumerate() {
            let alpha = (i as f32 / points.len().max(1) as f32).max(0.15);
            painter.circle_filled(
                pos2(c.x + s * scale, c.y - m * scale),
                2.0,
                p.accent.gamma_multiply(alpha),
            );
        }
        let width = ui.available_width().max(300.0);
        band_view(ui, params, vec2(width, height));
    });
    ui.add_space(6.0);
    correlation_bar(ui, correlation);
    crate::controls::animate(ui.ctx());
    ui.add_space(8.0);
    knob_sections(
        ui,
        params,
        &[
            ("Width", &["low_width", "mid_width", "high_width"], None),
            ("Bands", &["low_split", "high_split", "mono_bass"], None),
            ("Stereoize", &["stereoize", "stereoize_time"], None),
            ("Output", &["output"], None),
        ],
        &|_| true,
    );
}

/// The correlation of the sides, -1 (opposed) to +1 (mono), as a bar with its dot.
fn correlation_bar(ui: &mut Ui, correlation: f32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let bar = Rect::from_min_max(
        pos2(rect.left() + 130.0, rect.center().y - 4.0),
        pos2(rect.right() - 40.0, rect.center().y + 4.0),
    );
    painter.text(
        pos2(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        format!("{} {:+.2}", tr("Correlation"), correlation),
        FontId::proportional(11.5),
        p.text,
    );
    // Red where the sides cancel in mono, green where they hold.
    let mid = bar.center().x;
    painter.rect_filled(
        Rect::from_min_max(bar.min, pos2(mid, bar.bottom())),
        4.0,
        theme::RECORD.gamma_multiply(0.25),
    );
    painter.rect_filled(
        Rect::from_min_max(pos2(mid, bar.top()), bar.max),
        4.0,
        theme::PLAY.gamma_multiply(0.25),
    );
    for (text, x) in [("-1", bar.left()), ("0", mid), ("+1", bar.right())] {
        painter.text(
            pos2(x, bar.bottom() + 2.0),
            Align2::CENTER_TOP,
            text,
            FontId::proportional(8.5),
            p.weak,
        );
    }
    let x = bar.left() + (correlation.clamp(-1.0, 1.0) + 1.0) / 2.0 * bar.width();
    painter.circle_filled(
        pos2(x, bar.center().y),
        6.0,
        if correlation < 0.0 {
            theme::RECORD
        } else {
            theme::PLAY
        },
    );
}

/// The three bands over the spectrum, each as tall as it is wide: drag a band up or down for
/// its width, a split or the mono line sideways.
fn band_view(ui: &mut Ui, params: &mut dyn EditorParams, size: egui::Vec2) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    let inner = rect.shrink2(vec2(10.0, 22.0));
    let to_x = |f: f32| inner.left() + (f / 20.0).log10() / 3.0 * inner.width();
    let f_of = |x: f32| 20.0 * 1000f32.powf(((x - inner.left()) / inner.width()).clamp(0.0, 1.0));
    let grid = p.weak.gamma_multiply(0.14);
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        let x = to_x(f);
        painter.line_segment(
            [pos2(x, inner.top()), pos2(x, inner.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(x, rect.bottom() - 3.0),
            Align2::CENTER_BOTTOM,
            if f >= 1000.0 {
                format!("{}k", f / 1000.0)
            } else {
                format!("{f}")
            },
            FontId::proportional(9.0),
            p.weak,
        );
    }
    let (low, high, mono) = (
        params.get("low_split"),
        params.get("high_split"),
        params.get("mono_bass"),
    );
    let edges = [20.0, low, high, 20_000.0];
    let ids = ["low_width", "mid_width", "high_width"];
    let names = [tr("Low"), tr("Mid"), tr("High")];
    let mid_y = inner.center().y;
    painter.line_segment(
        [pos2(inner.left(), mid_y), pos2(inner.right(), mid_y)],
        Stroke::new(1.0, grid.gamma_multiply(1.5)),
    );
    for b in 0..3 {
        let width = params.get(ids[b]);
        let (x0, x1) = (to_x(edges[b]), to_x(edges[b + 1]));
        let half = width / 200.0 * inner.height() / 2.0;
        let body = Rect::from_min_max(pos2(x0 + 2.0, mid_y - half), pos2(x1 - 2.0, mid_y + half));
        painter.rect_filled(body, 4.0, p.accent.gamma_multiply(0.18 + width / 600.0));
        painter.rect_stroke(
            body,
            4.0,
            Stroke::new(1.0, p.accent.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            pos2((x0 + x1) / 2.0, rect.top() + 4.0),
            Align2::CENTER_TOP,
            format!("{} {width:.0} %", names[b]),
            FontId::proportional(10.5),
            p.text,
        );
    }
    // Mono below this line: the sides go away there.
    if mono > 21.0 {
        let x = to_x(mono);
        painter.rect_filled(
            Rect::from_min_max(pos2(inner.left(), inner.top()), pos2(x, inner.bottom())),
            0.0,
            p.weak.gamma_multiply(0.12),
        );
        painter.line_segment(
            [pos2(x, inner.top()), pos2(x, inner.bottom())],
            Stroke::new(1.5, theme::SOLO),
        );
        painter.text(
            pos2(x - 4.0, inner.bottom() - 2.0),
            Align2::RIGHT_BOTTOM,
            tr("Mono"),
            FontId::proportional(9.5),
            theme::SOLO,
        );
    }
    for split in [low, high] {
        let x = to_x(split);
        painter.line_segment(
            [pos2(x, inner.top()), pos2(x, inner.bottom())],
            Stroke::new(1.5, p.text.gamma_multiply(0.6)),
        );
        painter.circle_filled(pos2(x, mid_y), 4.0, p.text.gamma_multiply(0.8));
    }
    // What a drag moves, chosen where it starts.
    let key = ui.scope_id().with(("imager-drag", params.salt()));
    if response.drag_started()
        && let Some(at) = response.interact_pointer_pos()
    {
        let near = |f: f32| (to_x(f) - at.x).abs() < 6.0;
        let target = if near(low) {
            "low_split"
        } else if near(high) {
            "high_split"
        } else if mono > 21.0 && near(mono) {
            "mono_bass"
        } else {
            let f = f_of(at.x);
            if f < low {
                "low_width"
            } else if f < high {
                "mid_width"
            } else {
                "high_width"
            }
        };
        ui.data_mut(|d| d.insert_temp(key, target));
    }
    let target: Option<&'static str> = ui.data(|d| d.get_temp(key));
    let hover = response.hover_pos().map(|at| {
        let near = |f: f32| (to_x(f) - at.x).abs() < 6.0;
        near(low) || near(high) || (mono > 21.0 && near(mono))
    });
    if hover == Some(true) || target.is_some_and(|t| !t.ends_with("width")) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let response = response.on_hover_text(tr(
        "Drag a band up or down for its width, a split or the mono line sideways",
    ));
    if let Some(id) = target {
        let changed = response.dragged().then(|| {
            if id.ends_with("width") {
                params.get(id) - response.drag_delta().y / (inner.height() / 2.0) * 100.0
            } else {
                f_of(response.interact_pointer_pos().map_or(0.0, |q| q.x))
            }
        });
        params.set(id, changed, &response);
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<&'static str>(key));
        }
    }
}

/// A bipolar bar from the middle for a gain in dB (-12 to +12).
fn bipolar(painter: &egui::Painter, rect: Rect, db: f32, color: Color32, base: Color32) {
    painter.rect_filled(rect, 4.0, base);
    let mid = rect.center().x;
    let x = mid + (db / 12.0).clamp(-1.0, 1.0) * rect.width() / 2.0;
    painter.rect_filled(
        Rect::from_x_y_ranges(mid.min(x)..=mid.max(x), rect.y_range()),
        4.0,
        color,
    );
    painter.line_segment(
        [pos2(mid, rect.top() - 2.0), pos2(mid, rect.bottom() + 2.0)],
        Stroke::new(1.0, base.gamma_multiply(3.0)),
    );
}

fn transient(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EffectsState) {
    let p = theme::current(ui);
    let at = |i: usize, floor: f32| meters.get(i).copied().unwrap_or(floor);
    state
        .dynamics
        .push_back([at(2, 0.0), at(3, -120.0), at(4, -120.0)]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    let height = 220.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let width = (ui.available_width() - 190.0).max(300.0);
        transient_view(ui, state, vec2(width, height));
        // What the shaper does now, attack and sustain apart.
        let (rect, _) = ui.allocate_exact_size(vec2(182.0, height), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, theme::screen(&p));
        for (i, (name, db, color)) in [
            (tr("Attack"), at(0, 0.0), p.accent),
            (tr("Sustain"), at(1, 0.0), theme::SOLO),
        ]
        .into_iter()
        .enumerate()
        {
            let y = rect.top() + 46.0 + i as f32 * 74.0;
            painter.text(
                pos2(rect.left() + 12.0, y - 24.0),
                Align2::LEFT_TOP,
                name,
                FontId::proportional(11.0),
                p.weak,
            );
            painter.text(
                pos2(rect.right() - 12.0, y - 26.0),
                Align2::RIGHT_TOP,
                format!("{:+.1} dB", db + 0.0),
                theme::strong(14.0),
                color,
            );
            bipolar(
                &painter,
                Rect::from_min_size(pos2(rect.left() + 12.0, y), vec2(158.0, 12.0)),
                db,
                color,
                p.panel,
            );
        }
        painter.text(
            pos2(rect.center().x, rect.bottom() - 12.0),
            Align2::CENTER_BOTTOM,
            format!("{} {:.1} dBFS", tr("Out"), at(4, -120.0).max(-99.0)),
            FontId::monospace(10.5),
            p.weak,
        );
    });
    crate::controls::animate(ui.ctx());
    ui.add_space(8.0);
    knob_sections(
        ui,
        params,
        &[
            (
                "Shape",
                &["attack", "sustain", "speed", "sensitivity"],
                None,
            ),
            ("Detection", &["sc_hpf"], None),
            ("Output", &["mix", "output", "clip"], None),
        ],
        &|_| true,
    );
}

/// The sound over time, mirrored like a waveform: the input in grey, the output over it, and
/// the gain the shaper applies as a line around 0 dB.
fn transient_view(ui: &mut Ui, state: &EffectsState, size: egui::Vec2) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 10.0;
    // Levels from -48 dBFS (the middle) to 0 (the edges).
    let amp = |db: f32| ((db + 48.0) / 48.0).clamp(0.0, 1.0) * half;
    let step = rect.width() / HISTORY as f32;
    let x0 = rect.right() - state.dynamics.len() as f32 * step;
    let points = |k: usize| -> Vec<(f32, f32)> {
        state
            .dynamics
            .iter()
            .enumerate()
            .map(|(i, v)| (x0 + i as f32 * step, amp(v[k])))
            .collect()
    };
    painter.add(mirrored(&points(1), mid, p.weak.gamma_multiply(0.3)));
    painter.add(mirrored(&points(2), mid, p.accent.gamma_multiply(0.45)));
    // Gain: +12 dB at the top, -12 at the bottom.
    let gain_y = |db: f32| mid - (db / 12.0).clamp(-1.0, 1.0) * half;
    painter.line_segment(
        [pos2(rect.left(), mid), pos2(rect.right(), mid)],
        Stroke::new(1.0, p.weak.gamma_multiply(0.25)),
    );
    let gain: Vec<Pos2> = state
        .dynamics
        .iter()
        .enumerate()
        .map(|(i, v)| pos2(x0 + i as f32 * step, gain_y(v[0])))
        .collect();
    painter.add(Shape::line(gain, Stroke::new(1.8, theme::SOLO)));
    let legend = [
        (tr("Input"), p.weak),
        (tr("Output"), p.accent),
        (tr("Gain"), theme::SOLO),
    ];
    let mut x = rect.left() + 10.0;
    for (name, color) in legend {
        painter.rect_filled(
            Rect::from_min_size(pos2(x, rect.bottom() - 14.0), vec2(8.0, 3.0)),
            1.0,
            color,
        );
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(9.5), p.weak);
        let w = galley.size().x;
        painter.galley(pos2(x + 12.0, rect.bottom() - 19.0), galley, p.weak);
        x += w + 26.0;
    }
    for (db, label) in [(12.0, "+12"), (-12.0, "-12")] {
        painter.text(
            pos2(
                rect.right() - 6.0,
                gain_y(db) + if db > 0.0 { 2.0 } else { -2.0 },
            ),
            if db > 0.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::RIGHT_BOTTOM
            },
            label,
            FontId::proportional(8.5),
            theme::SOLO.gamma_multiply(0.7),
        );
    }
}

fn deesser(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EffectsState) {
    use audiostrada_dsp::deesser::{MODE_HINTS, MODES};
    let reduction = meters.first().copied().unwrap_or(0.0);
    let level = meters.get(1).copied().unwrap_or(-120.0);
    state
        .dynamics
        .push_back([reduction, level, level + reduction]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    cards(ui, params, "mode", &MODES, &MODE_HINTS);
    ui.add_space(8.0);
    let threshold = params.get("threshold");
    let height = 210.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let side = 300.0;
        let width = (ui.available_width() - side - 52.0).max(260.0);
        dynamics_history(
            ui,
            params,
            state,
            vec2(width, height),
            (threshold, None),
            (reduction, 24.0),
        );
        detection_curve(ui, params, vec2(side, height));
        reduction_meter(ui, reduction, height);
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let weak = theme::current(ui).weak;
        ui.label(RichText::new(tr("Detector")).size(11.0).color(weak));
        segmented(
            ui,
            params,
            "detector",
            &[
                "Everything above the frequency: catches every S",
                "Only around the frequency: leaves the air above alone",
            ],
        );
        ui.add_space(16.0);
        ui.label(RichText::new(tr("Listen")).size(11.0).color(weak));
        segmented(
            ui,
            params,
            "listen",
            &[
                "The de-essed sound",
                "What the detector hears",
                "Only what is removed",
            ],
        );
    });
    ui.add_space(6.0);
    knob_sections(
        ui,
        params,
        &[
            (
                "Detection",
                &["frequency", "threshold", "range"],
                Some((
                    "relative",
                    "Relative",
                    "Compares the sibilance with the whole voice, so the threshold holds \
                     whether the singer is loud or soft",
                )),
            ),
            ("Time", &["attack", "release", "lookahead"], None),
        ],
        &|_| true,
    );
}

/// What the detector hears, from 1 to 20 kHz, with the frequency to drag sideways; in split
/// mode, the highs that are turned down are shaded.
fn detection_curve(ui: &mut Ui, params: &mut dyn EditorParams, size: egui::Vec2) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    let inner = rect.shrink2(vec2(10.0, 14.0));
    let (low, high) = (1000.0f32, 20_000.0f32);
    let to_x = |f: f32| inner.left() + (f / low).ln() / (high / low).ln() * inner.width();
    let of_x =
        |x: f32| low * (high / low).powf(((x - inner.left()) / inner.width()).clamp(0.0, 1.0));
    let to_y = |db: f32| inner.top() + ((6.0 - db) / 36.0).clamp(0.0, 1.0) * inner.height();
    let grid = p.weak.gamma_multiply(0.14);
    for f in [2000.0, 5000.0, 10_000.0] {
        painter.line_segment(
            [pos2(to_x(f), inner.top()), pos2(to_x(f), inner.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(to_x(f) + 3.0, rect.bottom() - 3.0),
            Align2::LEFT_BOTTOM,
            format!("{}k", f / 1000.0),
            FontId::proportional(9.0),
            p.weak,
        );
    }
    let rate = f64::from(params.sample_rate().max(1));
    let frequency = params.get("frequency");
    if params.get("mode") < 0.5 {
        let x = to_x(frequency * audiostrada_dsp::deesser::SPLIT as f32);
        painter.rect_filled(
            Rect::from_min_max(pos2(x, rect.top()), rect.max),
            0.0,
            theme::RECORD.gamma_multiply(0.08),
        );
    }
    let filter = audiostrada_dsp::deesser::detector(&params.values(), rate);
    let points: Vec<Pos2> = (0..=160)
        .map(|i| {
            let f = low * (high / low).powf(i as f32 / 160.0);
            pos2(
                to_x(f),
                to_y(filter.magnitude_db(f64::from(f), rate) as f32),
            )
        })
        .collect();
    painter.add(fill_under(
        &points,
        inner.bottom(),
        p.accent.gamma_multiply(0.16),
    ));
    painter.add(Shape::line(points, Stroke::new(2.0, p.accent)));
    let hot = response.hovered() || response.dragged();
    let x = to_x(frequency);
    painter.line_segment(
        [pos2(x, rect.top() + 4.0), pos2(x, rect.bottom() - 4.0)],
        Stroke::new(if hot { 2.0 } else { 1.2 }, theme::SOLO),
    );
    painter.text(
        pos2(x + 5.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
        format_hz(frequency),
        theme::strong(11.0),
        theme::SOLO,
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let response = response.on_hover_text(tr("Drag sideways to move the frequency"));
    let changed = response
        .dragged()
        .then(|| of_x(x + response.drag_delta().x));
    params.set("frequency", changed, &response);
}

/// A frequency response from 20 Hz to 20 kHz on a dB range, with labeled grid lines.
fn response_plot(
    ui: &mut Ui,
    height: f32,
    range: f32,
    curve: impl Fn(f32) -> f32,
    color: Color32,
    marker: Option<f32>,
) {
    let p = theme::current(ui);
    let (rect, painter) = display(ui, height);
    let inner = rect.shrink2(vec2(10.0, 12.0));
    let to_x = |f: f32| inner.left() + (f / 20.0).log10() / 3.0 * inner.width();
    let to_y = |db: f32| inner.center().y - (db / range).clamp(-1.0, 1.0) * inner.height() / 2.0;
    let grid = p.weak.gamma_multiply(0.15);
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        painter.line_segment(
            [pos2(to_x(f), inner.top()), pos2(to_x(f), inner.bottom())],
            Stroke::new(1.0, grid),
        );
        let label = if f >= 1000.0 {
            format!("{}k", f / 1000.0)
        } else {
            format!("{f}")
        };
        painter.text(
            pos2(to_x(f), inner.bottom()),
            Align2::CENTER_BOTTOM,
            label,
            FontId::proportional(9.0),
            p.weak,
        );
    }
    for db in [-range / 2.0, 0.0, range / 2.0] {
        painter.line_segment(
            [pos2(inner.left(), to_y(db)), pos2(inner.right(), to_y(db))],
            Stroke::new(1.0, grid),
        );
    }
    let points: Vec<Pos2> = (0..=240)
        .map(|i| {
            let f = 20.0 * 1000f32.powf(i as f32 / 240.0);
            pos2(to_x(f), to_y(curve(f)))
        })
        .collect();
    painter.add(fill_under(&points, to_y(0.0), color.gamma_multiply(0.18)));
    painter.add(Shape::line(points, Stroke::new(2.4, color)));
    if let Some(f) = marker {
        painter.circle_filled(pos2(to_x(f), to_y(curve(f))), 5.0, theme::SOLO);
    }
}

fn vintage_eq(ui: &mut Ui, params: &mut dyn EditorParams) {
    use audiostrada_dsp::vintage_eq::{CUT_FREQS, HIGH_FREQS, LOW_FREQS};
    let values = params.values();
    let rate = params.sample_rate().max(1) as f32;
    let amber = Color32::from_rgb(230, 170, 90);
    response_plot(
        ui,
        150.0,
        18.0,
        |f| audiostrada_dsp::vintage_eq::response_db(&values, rate, f),
        amber,
        None,
    );
    ui.add_space(8.0);
    // The faceplate: knobs above, frequency selectors below, by section.
    let plate = Plate::TEAL;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 268.0), Sense::hover());
    paint_faceplate(&ui.painter_at(rect), rect, &plate);
    let inner = rect.shrink2(vec2(30.0, 10.0));
    let unit = inner.width() / 6.0;
    let strip = |from: f32, cells: f32| {
        Rect::from_min_size(
            pos2(inner.left() + from * unit, inner.top()),
            vec2(cells * unit, inner.height()),
        )
    };
    let short = |names: &[&'static str]| -> Vec<String> {
        names
            .iter()
            .map(|n| {
                n.trim_end_matches(" Hz")
                    .trim_end_matches(" kHz")
                    .to_owned()
            })
            .collect()
    };
    let sections = [
        (strip(0.0, 2.0), tr("Low frequency"), "CPS"),
        (strip(2.0, 2.0), tr("High frequency"), "KCS"),
        (strip(4.0, 1.0), tr("High atten"), "KCS"),
        (strip(5.0, 1.0), tr("Tube"), ""),
    ];
    for (i, (column, title, _)) in sections.iter().enumerate() {
        ui.painter().text(
            pos2(column.center().x, column.top() + 8.0),
            Align2::CENTER_CENTER,
            title.to_uppercase(),
            theme::strong(10.5),
            plate.ink,
        );
        if i > 0 {
            // Engraved: a dark line and its light edge.
            let x = column.left();
            let y = column.top() + 2.0..=column.bottom() - 2.0;
            ui.painter().vline(
                x,
                y.clone(),
                Stroke::new(1.0, Color32::from_black_alpha(70)),
            );
            ui.painter()
                .vline(x + 1.0, y, Stroke::new(1.0, Color32::from_white_alpha(28)));
        }
    }
    let row = |column: Rect, cell: usize, cells: usize, second: bool| {
        let w = column.width() / cells as f32;
        let top = column.top() + 20.0 + if second { 116.0 } else { 0.0 };
        Rect::from_min_size(pos2(column.left() + w * cell as f32, top), vec2(w, 112.0))
    };
    let (low, high, cut, tube) = (sections[0].0, sections[1].0, sections[2].0, sections[3].0);
    plate_knob(
        ui,
        params,
        "low_boost",
        tr("Boost"),
        row(low, 0, 2, false),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "low_cut",
        tr("Atten"),
        row(low, 1, 2, false),
        &plate,
    );
    plate_switch(
        ui,
        params,
        "low_freq",
        "CPS",
        &short(&LOW_FREQS),
        row(low, 0, 1, true),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "high_boost",
        tr("Boost"),
        row(high, 0, 2, false),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "bandwidth",
        tr("Bandwidth"),
        row(high, 1, 2, false),
        &plate,
    );
    plate_switch(
        ui,
        params,
        "high_freq",
        "KCS",
        &short(&HIGH_FREQS),
        row(high, 0, 1, true),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "high_cut",
        tr("Atten"),
        row(cut, 0, 1, false),
        &plate,
    );
    plate_switch(
        ui,
        params,
        "cut_freq",
        "KCS",
        &short(&CUT_FREQS),
        row(cut, 0, 1, true),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "warmth",
        tr("Warmth"),
        row(tube, 0, 1, false),
        &plate,
    );
    plate_knob(
        ui,
        params,
        "output",
        tr("Output"),
        row(tube, 0, 1, true),
        &plate,
    );
    // The pilot lamp, a red jewel: lit while powered, brighter with the warmth.
    let glow = 0.45 + 0.55 * params.get("warmth") / 100.0;
    let lamp = pos2(tube.right() - 24.0, tube.top() + 10.0);
    let lit = Color32::from_rgb(255, 64, 52);
    let painter = ui.painter();
    for (radius, alpha) in [(18.0, 0.07), (13.0, 0.14), (9.5, 0.22)] {
        painter.circle_filled(lamp, radius, lit.gamma_multiply(alpha * glow));
    }
    painter.circle_filled(lamp, 8.0, Color32::from_rgb(150, 150, 156));
    painter.circle_stroke(lamp, 8.0, Stroke::new(1.0, Color32::from_black_alpha(140)));
    painter.circle_filled(
        lamp,
        6.0,
        Color32::from_rgb(80, 8, 8).lerp_to_gamma(lit, glow),
    );
    // Facets of the jewel, and the light caught on it.
    for k in 0..6 {
        let a = k as f32 * std::f32::consts::PI / 3.0;
        painter.line_segment(
            [lamp, plate_point(lamp, 6.0, a)],
            Stroke::new(0.8, Color32::from_black_alpha(50)),
        );
    }
    painter.circle_filled(lamp + vec2(-2.0, -2.2), 1.8, Color32::from_white_alpha(170));
}

/// The faceplate of the tube EQ, as the old hardware in its teal paint: lit from the top left,
/// darker toward the bottom, its edges bevelled, rack slots at its ends and chrome screws.
fn paint_faceplate(painter: &egui::Painter, rect: Rect, plate: &Plate) {
    theme::gradient_rect(painter, rect, plate.bottom, plate.top);
    // A soft light from the top left, fading out across the plate.
    let light = pos2(
        rect.left() + rect.width() * 0.3,
        rect.top() + rect.height() * 0.15,
    );
    let reach = rect.width() * 0.75;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(light, Color32::from_white_alpha(32));
    let n = 48;
    for k in 0..=n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            light + vec2(a.cos() * reach, a.sin() * reach * 0.55),
            Color32::TRANSPARENT,
        );
        if k > 0 {
            mesh.add_triangle(0, k, k + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
    // The bottom in shade.
    let shade = Rect::from_min_max(
        pos2(rect.left(), rect.bottom() - rect.height() * 0.35),
        rect.max,
    );
    theme::gradient_rect(
        painter,
        shade,
        Color32::from_black_alpha(70),
        Color32::TRANSPARENT,
    );
    // Bevelled edges: light above and at the left, dark below and at the right.
    let edge = |a: Pos2, b: Pos2, c: Color32| painter.line_segment([a, b], Stroke::new(1.5, c));
    edge(
        rect.left_top(),
        rect.right_top(),
        Color32::from_white_alpha(70),
    );
    edge(
        rect.left_top(),
        rect.left_bottom(),
        Color32::from_white_alpha(35),
    );
    edge(
        rect.left_bottom(),
        rect.right_bottom(),
        Color32::from_black_alpha(120),
    );
    edge(
        rect.right_top(),
        rect.right_bottom(),
        Color32::from_black_alpha(80),
    );
    // Rack slots at both ends, and a chrome screw in each corner.
    for x in [rect.left() + 8.0, rect.right() - 8.0] {
        for y in [
            rect.top() + rect.height() * 0.22,
            rect.bottom() - rect.height() * 0.22,
        ] {
            let slot = Rect::from_center_size(pos2(x, y), vec2(9.0, 22.0));
            painter.rect_filled(slot, 4.5, Color32::from_rgb(12, 20, 24));
            painter.rect_stroke(
                slot,
                4.5,
                Stroke::new(1.0, Color32::from_white_alpha(30)),
                egui::StrokeKind::Outside,
            );
        }
    }
    for (x, y) in [
        (rect.left() + 22.0, rect.top() + 16.0),
        (rect.right() - 22.0, rect.top() + 16.0),
        (rect.left() + 22.0, rect.bottom() - 16.0),
        (rect.right() - 22.0, rect.bottom() - 16.0),
    ] {
        let c = pos2(x, y);
        painter.circle_filled(c + vec2(0.0, 1.0), 5.5, Color32::from_black_alpha(90));
        painter.circle_filled(c, 5.0, Color32::from_rgb(168, 172, 176));
        painter.circle_filled(c + vec2(-1.2, -1.2), 3.2, Color32::from_rgb(214, 218, 222));
        painter.circle_stroke(c, 5.0, Stroke::new(0.8, Color32::from_rgb(90, 94, 98)));
        let slot = vec2(3.6, -1.6);
        painter.line_segment(
            [c - slot, c + slot],
            Stroke::new(1.3, Color32::from_rgb(70, 74, 78)),
        );
    }
}

/// Colors of the tube EQ's faceplate: the teal paint of the old hardware with white lettering,
/// whatever the theme, as the tape machines keep their look.
struct Plate {
    top: Color32,
    bottom: Color32,
    ink: Color32,
    knob: Color32,
    head: Color32,
    edge: Color32,
}

impl Plate {
    const TEAL: Plate = Plate {
        top: Color32::from_rgb(36, 112, 130),
        bottom: Color32::from_rgb(23, 80, 96),
        ink: Color32::from_rgb(244, 246, 242),
        knob: Color32::from_rgb(16, 16, 18),
        head: Color32::from_rgb(28, 28, 32),
        edge: Color32::from_rgb(96, 100, 108),
    };
}

/// Where a hardware control points: 0 straight up, clockwise, in radians.
fn plate_point(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    center + radius * vec2(angle.sin(), -angle.cos())
}

/// A pointer knob as on the old hardware: a round skirt, a pointed head with its line.
fn paint_pointer_knob(painter: &egui::Painter, center: Pos2, r: f32, angle: f32, plate: &Plate) {
    painter.circle_filled(
        center + vec2(0.0, 2.5),
        r * 1.04,
        Color32::from_black_alpha(80),
    );
    painter.circle_filled(center, r, plate.knob);
    painter.circle_stroke(center, r, Stroke::new(1.0, plate.edge));
    let dir = vec2(angle.sin(), -angle.cos());
    let side = vec2(-dir.y, dir.x);
    let head = vec![
        center + dir * r * 1.3,
        center + side * r * 0.5,
        center - dir * r * 0.75,
        center - side * r * 0.5,
    ];
    painter.add(Shape::convex_polygon(
        head,
        plate.head,
        Stroke::new(1.0, plate.edge),
    ));
    // Light from the top left: a sheen on the skirt and a rim catching it.
    painter.circle_filled(
        center - vec2(r * 0.3, r * 0.35),
        r * 0.45,
        Color32::from_white_alpha(22),
    );
    let rim: Vec<Pos2> = (0..=16)
        .map(|k| plate_point(center, r - 1.0, -2.4 + k as f32 * 0.15))
        .collect();
    painter.add(Shape::line(
        rim,
        Stroke::new(1.4, Color32::from_white_alpha(80)),
    ));
    painter.line_segment(
        [center + dir * r * 0.2, center + dir * r * 1.15],
        Stroke::new(2.0, Color32::from_rgb(240, 234, 220)),
    );
}

/// A knob of the faceplate in `cell`: its name, a scale from 0 to 10 (or -12 to +12), the
/// value under it.
fn plate_knob(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    label: &str,
    cell: Rect,
    plate: &Plate,
) {
    let Some(p) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let r = 21.0;
    let center = pos2(cell.center().x, cell.top() + 58.0);
    let response = ui
        .interact(
            Rect::from_center_size(center, vec2(2.0 * r + 30.0, 2.0 * r + 30.0)),
            ui.scope_id().with(("plate", params.salt(), id)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let mut t = to_unit(p, value);
    let fine = ui.input(|i| i.modifiers.shift);
    if response.dragged() {
        t -= response.drag_delta().y / 150.0 * if fine { 0.15 } else { 1.0 };
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            t += wheel.signum() * if fine { 0.002 } else { 0.02 };
        }
    }
    if response.double_clicked() {
        t = to_unit(p, p.default);
    }
    let t = t.clamp(0.0, 1.0);
    let changed = ((t - to_unit(p, value)).abs() > 1e-6).then(|| from_unit(p, t));
    let shown = changed.unwrap_or(value);
    let painter = ui.painter();
    painter.text(
        pos2(cell.center().x, cell.top() + 12.0),
        Align2::CENTER_CENTER,
        label.to_uppercase(),
        FontId::proportional(10.0),
        plate.ink.gamma_multiply(0.85),
    );
    let sweep = 0.75 * std::f32::consts::PI;
    let bipolar = p.min < 0.0;
    let marks: Vec<(f32, String)> = if bipolar {
        [-12.0f32, -6.0, 0.0, 6.0, 12.0]
            .iter()
            .map(|v| (to_unit(p, *v), format!("{v:+}").replace("+0", "0")))
            .collect()
    } else {
        (0..=10)
            .map(|k| {
                (
                    k as f32 / 10.0,
                    if k % 2 == 0 {
                        (k as f32 * p.max / 10.0).to_string()
                    } else {
                        String::new()
                    },
                )
            })
            .collect()
    };
    for (u, text) in &marks {
        let a = -sweep + 2.0 * sweep * u;
        painter.line_segment(
            [
                plate_point(center, r + 4.0, a),
                plate_point(center, r + 8.0, a),
            ],
            Stroke::new(1.2, plate.ink.gamma_multiply(0.8)),
        );
        if !text.is_empty() {
            painter.text(
                plate_point(center, r + 16.0, a),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(9.0),
                plate.ink.gamma_multiply(0.75),
            );
        }
    }
    paint_pointer_knob(painter, center, r, -sweep + 2.0 * sweep * t, plate);
    painter.text(
        pos2(cell.center().x, cell.bottom() - 8.0),
        Align2::CENTER_CENTER,
        format_value(p, shown),
        FontId::monospace(10.0),
        plate.ink.gamma_multiply(0.7),
    );
    let response = response.on_hover_text(crate::trf!(
        "{name}: {value}. Drag, Shift for fine, double-click to reset",
        name = tr(p.name),
        value = format_value(p, shown)
    ));
    params.set(id, changed, &response);
}

/// A rotary selector of the faceplate in `cell`, its positions written around it: click a
/// position, or the knob for the next one; drag or wheel to step.
fn plate_switch(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    unit: &str,
    labels: &[String],
    cell: Rect,
    plate: &Plate,
) {
    let Some(p) = params.param(id) else {
        return;
    };
    let n = labels.len().max(1);
    let current = ((params.get(id) - p.min).round().max(0.0) as usize).min(n - 1);
    let r = 16.0;
    let center = pos2(cell.center().x, cell.top() + 62.0);
    let step = (std::f32::consts::PI * 1.5 / (n - 1).max(1) as f32).min(0.8);
    let angle = |i: usize| (i as f32 - (n - 1) as f32 / 2.0) * step;
    let key = ui.scope_id().with(("plate", params.salt(), id));
    let response = ui.interact(
        Rect::from_center_size(center, vec2(112.0, 96.0)),
        key,
        Sense::click_and_drag(),
    );
    let mut chosen = None;
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        let near = (0..n)
            .map(|i| (i, plate_point(center, r + 22.0, angle(i)).distance(at)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, d)| *d < 13.0);
        chosen = Some(near.map_or((current + 1) % n, |(i, _)| i));
    }
    let wheel = if response.hovered() {
        ui.input(|i| i.smooth_scroll_delta.y)
    } else {
        0.0
    };
    let travel: f32 = ui.data(|d| d.get_temp(key).unwrap_or(0.0));
    let travel = if response.dragged() {
        travel - response.drag_delta().y
    } else {
        0.0
    } + wheel;
    if travel.abs() >= 24.0 {
        let next = current as i32 + travel.signum() as i32;
        chosen = Some(next.clamp(0, n as i32 - 1) as usize);
        ui.data_mut(|d| d.insert_temp(key, 0.0f32));
    } else {
        ui.data_mut(|d| d.insert_temp(key, travel));
    }
    let shown = chosen.unwrap_or(current);
    let painter = ui.painter();
    painter.text(
        pos2(cell.center().x, cell.top() + 12.0),
        Align2::CENTER_CENTER,
        unit,
        FontId::proportional(10.0),
        plate.ink.gamma_multiply(0.85),
    );
    for (i, label) in labels.iter().enumerate() {
        let a = angle(i);
        let on = i == shown;
        painter.line_segment(
            [
                plate_point(center, r + 4.0, a),
                plate_point(center, r + 9.0, a),
            ],
            Stroke::new(1.2, plate.ink.gamma_multiply(0.8)),
        );
        painter.text(
            plate_point(center, r + 22.0, a),
            Align2::CENTER_CENTER,
            label,
            if on {
                theme::strong(11.0)
            } else {
                FontId::proportional(10.0)
            },
            if on {
                plate.ink
            } else {
                plate.ink.gamma_multiply(0.6)
            },
        );
    }
    paint_pointer_knob(painter, center, r, angle(shown), plate);
    let response = response.on_hover_text(format!(
        "{}: {}",
        tr(p.name),
        p.choices.get(shown).map_or("", |c| tr(c))
    ));
    let changed = chosen.filter(|&c| c != current).map(|c| p.min + c as f32);
    params.set(id, changed, &response);
    if changed.is_some() {
        params.end_gesture();
    }
}

fn filter(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32]) {
    use audiostrada_dsp::filter::{SHAPES, TYPE_HINTS, TYPES, lfo_value};
    let p = theme::current(ui);
    cards(ui, params, "type", &TYPES, &TYPE_HINTS);
    ui.add_space(8.0);
    let kind = params.get("type").round() as usize;
    let k = 2.0 - 1.95 * params.get("resonance") / 100.0;
    let steep = params.get("slope") >= 0.5;
    let cutoff = meters
        .first()
        .copied()
        .filter(|c| *c > 0.0)
        .unwrap_or(params.get("cutoff"));
    // One state-variable stage; the steep slope adds a flat one before the resonant one.
    let stage = |w: f32, k: f32| -> f32 {
        let den = ((1.0 - w * w).powi(2) + (k * w).powi(2)).sqrt();
        let num = match kind {
            0 => 1.0,
            1 => w * w,
            2 => k * w,
            _ => (1.0 - w * w).abs(),
        };
        (num / den.max(1e-6)).max(1e-6)
    };
    let curve = |f: f32| -> f32 {
        let w = f / cutoff;
        let gain = stage(w, k)
            * if steep {
                stage(w, std::f32::consts::SQRT_2)
            } else {
                1.0
            };
        20.0 * gain.log10()
    };
    response_plot(ui, 170.0, 36.0, curve, p.accent, Some(cutoff));
    crate::controls::animate(ui.ctx());
    ui.add_space(8.0);
    let accent = p.accent;
    // Short names inside their section.
    let knob = |ui: &mut Ui, params: &mut dyn EditorParams, id: &str, tint: Color32| {
        let name = match id {
            "lfo_rate" => tr("Rate"),
            "note" => tr("Note"),
            "lfo_depth" => tr("Depth"),
            "lfo_phase" => tr("Stereo phase"),
            "envelope" => tr("Amount"),
            "env_attack" => tr("Attack"),
            "env_release" => tr("Release"),
            _ => params.param(id).map_or("", |q| tr(q.name)),
        };
        wide_knob(ui, params, id, name, 44.0, tint);
    };
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        section_frame(ui, tr("Filter"), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for id in ["cutoff", "resonance", "drive"] {
                    knob(ui, params, id, accent);
                }
            });
            ui.horizontal(|ui| segmented(ui, params, "slope", &[]));
        });
        section_frame(ui, tr("LFO"), p.weak, 118.0, |ui| {
            let shape = (params.get("lfo_shape").round().max(0.0) as usize).min(SHAPES.len() - 1);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                // Each shape drawn on its button.
                for (i, name) in SHAPES.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 20.0), Sense::click());
                    let on = i == shape;
                    let painter = ui.painter();
                    painter.rect_filled(
                        rect,
                        5.0,
                        if on {
                            accent.gamma_multiply(0.3)
                        } else {
                            p.surface
                        },
                    );
                    painter.rect_stroke(
                        rect,
                        5.0,
                        Stroke::new(1.0, if on { accent } else { p.border }),
                        egui::StrokeKind::Inside,
                    );
                    let inner = rect.shrink2(vec2(6.0, 5.0));
                    let held = [0.6, -0.3, 0.9, -0.8];
                    let points: Vec<Pos2> = (0..=32)
                        .map(|j| {
                            let t = j as f32 / 32.0;
                            let random = held[((t * 4.0) as usize).min(3)];
                            let v = lfo_value(i, (t * 2.0).fract(), random);
                            pos2(
                                inner.left() + t * inner.width(),
                                inner.center().y - v * inner.height() / 2.0,
                            )
                        })
                        .collect();
                    painter.add(Shape::line(
                        points,
                        Stroke::new(1.2, if on { p.text } else { p.weak }),
                    ));
                    let response = response.on_hover_text(tr(name));
                    let clicked = response.clicked();
                    params.set("lfo_shape", clicked.then_some(i as f32), &response);
                    if clicked {
                        params.end_gesture();
                    }
                }
                ui.add_space(6.0);
                params.toggle(ui, "sync", |ui, on| {
                    let response = pill_switch(ui, *on, tr("Sync"))
                        .on_hover_text(tr("A note value at the project's tempo"));
                    if response.clicked() {
                        *on = !*on;
                    }
                    response
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let rate = if params.get("sync") >= 0.5 {
                    "note"
                } else {
                    "lfo_rate"
                };
                for id in [rate, "lfo_depth", "lfo_phase"] {
                    knob(ui, params, id, accent);
                }
            });
        });
        section_frame(ui, tr("Envelope"), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for id in ["envelope", "env_attack", "env_release"] {
                    knob(ui, params, id, theme::SOLO);
                }
            });
        });
        section_frame(ui, tr("Output"), p.weak, 118.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for id in ["mix", "output"] {
                    knob(ui, params, id, accent);
                }
            });
        });
    });
}

// ---------------------------------------------------------------- Compressor

const HISTORY: usize = 240;

/// The compressor: its character, the clean curve or the vintage VU meter, and its knobs.
fn compressor(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut EffectsState,
) {
    use audiostrada_dsp::compressor::{TYPE_HINTS, TYPES, uses};
    let kind = (params.get("type").round().max(0.0) as usize).min(TYPES.len() - 1);
    let reduction = meters.first().copied().unwrap_or(0.0);
    let input = meters.get(1).copied().unwrap_or(-120.0);
    let output = meters.get(2).copied().unwrap_or(-120.0);
    state.dynamics.push_back([reduction, input, output]);
    while state.dynamics.len() > HISTORY {
        state.dynamics.pop_front();
    }
    // The characters, as cards: their name and what they do.
    cards(ui, params, "type", &TYPES, &TYPE_HINTS);
    ui.add_space(8.0);
    // The display: levels and reduction over time, the curve, the reduction meter.
    let threshold = params.get("threshold");
    let curve = (threshold, params.get("ratio"), params.get("knee"));
    let range = params.get("range");
    let makeup = params.get("makeup")
        + if params.get("auto_gain") >= 0.5 {
            audiostrada_dsp::compressor::auto_makeup(curve.0, curve.1)
        } else {
            0.0
        };
    let height = 230.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let side = if kind == audiostrada_dsp::compressor::CLEAN {
            height
        } else {
            height * 1.45
        };
        let width = (ui.available_width() - side - 52.0).max(260.0);
        dynamics_history(
            ui,
            params,
            state,
            vec2(width, height),
            (threshold, None),
            (reduction, 24.0),
        );
        // Modern: the curve; vintage: a needle, as on the hardware.
        if kind == audiostrada_dsp::compressor::CLEAN {
            let out_of = |level: f32| {
                level
                    + audiostrada_dsp::compressor::reduction_at(kind, level, curve, range)
                    + makeup
            };
            transfer_curve(ui, params, &out_of, threshold, input, height);
        } else {
            let amber = kind == 3;
            vu_meter(
                ui,
                reduction,
                &mut state.needle,
                vec2(height * 1.45, height),
                (
                    if amber { VuFace::Amber } else { VuFace::Theme },
                    tr("GAIN REDUCTION"),
                ),
            );
        }
        reduction_meter(ui, reduction, height);
    });
    ui.add_space(8.0);
    // The controls, by what they act on, in sections of one height.
    knob_sections(
        ui,
        params,
        &[
            ("Dynamics", &["threshold", "ratio", "knee", "range"], None),
            (
                "Time",
                &["attack", "release", "hold", "lookahead"],
                Some((
                    "auto",
                    "Auto release",
                    "Fast recovery after peaks, slow after sustained compression",
                )),
            ),
            (
                "Sidechain",
                &["sc_hpf", "link"],
                Some((
                    "listen",
                    "Listen",
                    "Hear what the detector hears: the key through its high-pass",
                )),
            ),
            (
                "Output",
                &["makeup", "mix", "color"],
                Some((
                    "auto_gain",
                    "Auto gain",
                    "Makeup from the threshold and ratio, so the level stays about the same",
                )),
            ),
        ],
        &|id| uses(kind, id),
    );
}

/// The values of a choice as cards side by side: their name and two lines of what they do.
fn cards(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    id: &str,
    names: &[&'static str],
    hints: &[&'static str],
) {
    params.shown(id);
    let p = theme::current(ui);
    let kind = (params.get(id).round().max(0.0) as usize).min(names.len() - 1);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let n = names.len() as f32;
        let width = ((ui.available_width() - 6.0 * (n - 1.0)) / n).max(120.0);
        for (k, name) in names.iter().enumerate() {
            let on = k == kind;
            let (rect, response) = ui.allocate_exact_size(vec2(width, 58.0), Sense::click());
            let fill = if on {
                p.accent.gamma_multiply(0.22)
            } else if response.hovered() {
                p.hover
            } else {
                p.base
            };
            let painter = ui.painter();
            painter.rect_filled(rect, 8.0, fill);
            painter.rect_stroke(
                rect,
                8.0,
                Stroke::new(
                    if on { 1.5 } else { 1.0 },
                    if on { p.accent } else { p.border },
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.left_top() + vec2(12.0, 7.0),
                Align2::LEFT_TOP,
                tr(name),
                theme::strong(13.5),
                if on {
                    p.text
                } else {
                    p.text.gamma_multiply(0.8)
                },
            );
            // Two lines at most of what it does.
            let mut job = egui::text::LayoutJob::simple(
                tr(hints[k]).to_owned(),
                FontId::proportional(9.5),
                p.weak,
                rect.width() - 24.0,
            );
            job.wrap.max_rows = 2;
            job.wrap.overflow_character = Some('…');
            let hint = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter()
                .galley(rect.left_top() + vec2(12.0, 26.0), hint, p.weak);
            let response = response.on_hover_text(tr(hints[k]));
            if response.clicked() && !on {
                params.set(id, Some(k as f32), &response);
            }
        }
    });
}

/// Switch under a section's knobs: its parameter, name and what it does.
type SectionSwitch<'a> = Option<(&'a str, &'static str, &'static str)>;

/// Titled panels of knobs side by side, all of one height, each with its switch under the
/// knobs; the controls `enabled` refuses are greyed.
fn knob_sections(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    sections: &[(&'static str, &[&str], SectionSwitch)],
    enabled: &dyn Fn(&str) -> bool,
) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        knob_section_items(ui, params, sections, enabled);
    });
}

/// The sections of `knob_sections`, in the row `ui` already is.
fn knob_section_items(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    sections: &[(&'static str, &[&str], SectionSwitch)],
    enabled: &dyn Fn(&str) -> bool,
) {
    let p = theme::current(ui);
    let accent = p.accent;
    {
        for &(title, ids, switch) in sections {
            section_frame(ui, tr(title), p.weak, 118.0, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for &id in ids {
                        let tint = if id == "color" { theme::SOLO } else { accent };
                        let name = match id {
                            "sc_hpf" => tr("High pass"),
                            "sc_lpf" => tr("Low pass"),
                            _ => params.param(id).map_or("", |q| tr(q.name)),
                        };
                        ui.add_enabled_ui(enabled(id), |ui| {
                            wide_knob(ui, params, id, name, 44.0, tint)
                        });
                    }
                });
                if let Some((id, label, hint)) = switch {
                    ui.add_space(4.0);
                    ui.add_enabled_ui(enabled(id), |ui| {
                        params.toggle(ui, id, |ui, on| {
                            let response = pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
                            if response.clicked() {
                                *on = !*on;
                            }
                            response
                        });
                    });
                }
            });
        }
    }
}

/// Input (filled), output (line) and gain reduction (from the top) over time, with the
/// threshold as a line to drag.
fn dynamics_history(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    state: &EffectsState,
    size: egui::Vec2,
    (threshold, close): (f32, Option<f32>),
    (reduction, floor): (f32, f32),
) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    // Levels from -60 to 0 dBFS; the reduction from 0 down to -24 dB.
    let level_y = |db: f32| rect.bottom() - ((db + 60.0) / 60.0).clamp(0.0, 1.0) * rect.height();
    let gr_y = |db: f32| rect.top() + (-db / floor).clamp(0.0, 1.0) * rect.height();
    let grid = p.weak.gamma_multiply(0.14);
    for db in [-6.0f32, -12.0, -18.0, -24.0, -36.0, -48.0] {
        let y = level_y(db);
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(rect.right() - 6.0, y - 1.0),
            Align2::RIGHT_BOTTOM,
            format!("{db:.0}"),
            FontId::proportional(8.5),
            p.weak.gamma_multiply(0.7),
        );
    }
    let step = rect.width() / HISTORY as f32;
    let x0 = rect.right() - state.dynamics.len() as f32 * step;
    let line = |pick: &dyn Fn(&[f32; 3]) -> f32| -> Vec<Pos2> {
        state
            .dynamics
            .iter()
            .enumerate()
            .map(|(i, v)| pos2(x0 + i as f32 * step, pick(v)))
            .collect()
    };
    let input = line(&|v| level_y(v[1]));
    painter.add(fill_under(
        &input,
        rect.bottom(),
        p.weak.gamma_multiply(0.22),
    ));
    painter.add(Shape::line(
        line(&|v| level_y(v[2])),
        Stroke::new(1.4, p.text.gamma_multiply(0.75)),
    ));
    let gr = line(&|v| gr_y(v[0]));
    // The reduction hangs from the top, filled.
    if gr.len() > 1 {
        let mut mesh = egui::Mesh::default();
        let color = theme::RECORD.gamma_multiply(0.22);
        for (k, point) in gr.iter().enumerate() {
            mesh.colored_vertex(pos2(point.x, rect.top()), color);
            mesh.colored_vertex(*point, color);
            if k > 0 {
                let i = (k * 2) as u32;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i, i + 1);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    painter.add(Shape::line(gr, Stroke::new(2.0, theme::RECORD)));
    // The threshold, to drag up and down; darker on a light theme, to read.
    let ty = level_y(threshold);
    let hot = response.hovered() || response.dragged();
    let solo = if p.dark {
        theme::SOLO
    } else {
        Color32::from_rgb(170, 112, 0)
    };
    painter.add(Shape::dashed_line(
        &[pos2(rect.left(), ty), pos2(rect.right(), ty)],
        Stroke::new(if hot { 2.0 } else { 1.2 }, solo),
        6.0,
        4.0,
    ));
    painter.text(
        pos2(rect.left() + 10.0, ty + 3.0),
        Align2::LEFT_TOP,
        format!("{} {threshold:.1} dB", tr("Threshold")),
        FontId::proportional(10.0),
        solo,
    );
    // Where it closes again, below by the hysteresis.
    if let Some(close) = close.filter(|c| *c < threshold - 0.05) {
        let cy = level_y(close);
        painter.add(Shape::dashed_line(
            &[pos2(rect.left(), cy), pos2(rect.right(), cy)],
            Stroke::new(1.0, solo.gamma_multiply(0.5)),
            2.0,
            4.0,
        ));
        painter.text(
            pos2(rect.right() - 40.0, cy + 3.0),
            Align2::RIGHT_TOP,
            format!("{} {close:.1} dB", tr("Closes at")),
            FontId::proportional(9.5),
            solo.gamma_multiply(0.75),
        );
    }
    painter.text(
        rect.left_top() + vec2(10.0, 8.0),
        Align2::LEFT_TOP,
        format!(
            "{} {:.1} dB",
            tr("GR"),
            if reduction > -0.05 { 0.0 } else { reduction }
        ),
        theme::strong(15.0),
        theme::RECORD,
    );
    // Legend.
    let legend = [
        (tr("Input"), p.weak),
        (tr("Output"), p.text),
        (tr("Reduction"), theme::RECORD),
    ];
    let mut x = rect.left() + 10.0;
    for (name, color) in legend {
        painter.rect_filled(
            Rect::from_min_size(pos2(x, rect.bottom() - 14.0), vec2(8.0, 3.0)),
            1.0,
            color,
        );
        let galley = painter.layout_no_wrap(name.to_owned(), FontId::proportional(9.5), p.weak);
        let w = galley.size().x;
        painter.galley(pos2(x + 12.0, rect.bottom() - 19.0), galley, p.weak);
        x += w + 26.0;
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let response = response.on_hover_text(tr("Drag up and down to move the threshold"));
    let changed = response
        .dragged()
        .then(|| threshold - response.drag_delta().y / rect.height() * 60.0);
    params.set("threshold", changed, &response);
}

/// The static curve of the character, with where the input is now.
fn transfer_curve(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    out_of: &dyn Fn(f32) -> f32,
    threshold: f32,
    input: f32,
    size: f32,
) {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    let to_x = |db: f32| rect.left() + (db + 60.0) / 60.0 * rect.width();
    let to_y = |db: f32| rect.bottom() - (db + 60.0) / 60.0 * rect.height();
    let grid = p.weak.gamma_multiply(0.14);
    for db in [-48.0, -36.0, -24.0, -12.0] {
        painter.line_segment(
            [pos2(to_x(db), rect.top()), pos2(to_x(db), rect.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.line_segment(
            [pos2(rect.left(), to_y(db)), pos2(rect.right(), to_y(db))],
            Stroke::new(1.0, grid),
        );
    }
    painter.line_segment(
        [pos2(to_x(-60.0), to_y(-60.0)), pos2(to_x(0.0), to_y(0.0))],
        Stroke::new(1.0, grid.gamma_multiply(2.0)),
    );
    let at = |level: f32| pos2(to_x(level), to_y(out_of(level).clamp(-60.0, 0.0)));
    let points: Vec<Pos2> = (0..=120).map(|i| at(-60.0 + i as f32 * 0.5)).collect();
    painter.add(Shape::line(points, Stroke::new(2.2, p.accent)));
    let tx = to_x(threshold);
    painter.line_segment(
        [pos2(tx, rect.top()), pos2(tx, rect.bottom())],
        Stroke::new(1.0, theme::SOLO.gamma_multiply(0.6)),
    );
    if input > -60.0 {
        let dot = at(input.min(0.0));
        painter.circle_filled(dot, 7.0, theme::SOLO.gamma_multiply(0.25));
        painter.circle_filled(dot, 4.5, theme::SOLO);
    }
    for (text, anchor, align) in [
        (
            tr("OUT"),
            rect.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
        ),
        (
            tr("IN"),
            rect.right_bottom() - vec2(8.0, 6.0),
            Align2::RIGHT_BOTTOM,
        ),
    ] {
        painter.text(anchor, align, text, FontId::proportional(9.0), p.weak);
    }
    let response = response.on_hover_text(tr("Drag sideways to move the threshold"));
    let changed = response
        .dragged()
        .then(|| threshold + response.drag_delta().x / rect.width() * 60.0);
    params.set("threshold", changed, &response);
}

/// The gain reduction as a bar from the top, with its scale.
fn reduction_meter(ui: &mut Ui, reduction: f32, height: f32) {
    let p = theme::current(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let bar = Rect::from_min_max(
        rect.left_top() + vec2(4.0, 0.0),
        pos2(rect.left() + 14.0, rect.bottom()),
    );
    painter.rect_filled(bar, 4.0, theme::screen(&p));
    let depth = (-reduction / 24.0).clamp(0.0, 1.0);
    painter.rect_filled(
        Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + depth * bar.height())),
        4.0,
        theme::RECORD,
    );
    for db in [0.0f32, -3.0, -6.0, -12.0, -18.0, -24.0] {
        let y = bar.top() + (-db / 24.0) * bar.height();
        painter.text(
            pos2(rect.right(), y.clamp(rect.top() + 5.0, rect.bottom() - 5.0)),
            Align2::RIGHT_CENTER,
            format!("{:.0}", db.abs()),
            FontId::proportional(8.5),
            p.weak,
        );
    }
}

// ---------------------------------------------------------------- Limiter and Maximizer

/// Bottom of the limiter meters and display, and top of the display, in dBFS.
const METER_FLOOR: f32 = -36.0;
const DISPLAY_TOP: f32 = 6.0;
/// Points per second of the scrolling display, and how many it shows.
const TRACE_RATE: f64 = 30.0;
const TRACE_POINTS: usize = 180;
/// Loudness targets for Learn, in LUFS.
const TARGETS: [(f32, &str); 5] = [
    (-9.0, "-9 LUFS (loud)"),
    (-11.0, "-11 LUFS"),
    (-14.0, "-14 LUFS (streaming)"),
    (-16.0, "-16 LUFS (Apple Music)"),
    (-23.0, "-23 LUFS (broadcast)"),
];
const MAXIMIZER_MODES: [&str; 3] = [
    "Cleanest: no clipping, slower release",
    "Shaves the tallest peaks first, faster release",
    "Loudest: more clipping, fastest release",
];

/// What the limiter display remembers between frames.
#[derive(Debug, Default)]
pub struct LimiterState {
    /// Driven input and output peaks (dBFS), oldest first.
    trace: VecDeque<[f32; 2]>,
    trace_time: f64,
    target: Option<f32>,
    /// When Learn started and when it last moved the threshold.
    learn: Option<(f64, f64)>,
    comparing: bool,
}

fn limiter(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EffectsState) {
    limiter_view(ui, params, meters, &mut state.limiter, "input");
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        knob_section_items(
            ui,
            params,
            &[
                (
                    "Time",
                    &["release", "lookahead"],
                    Some((
                        "auto",
                        "Auto release",
                        "Fast recovery after short peaks, slow on dense passages",
                    )),
                ),
                (
                    "Stereo",
                    &["link"],
                    Some((
                        "true_peak",
                        "True peak",
                        "Also catches the peaks between the samples, which converters and \
                         encoders bring back",
                    )),
                ),
            ],
            &|_| true,
        );
        monitor_switches(ui, params, &mut state.limiter.comparing);
    });
}

fn maximizer(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut EffectsState) {
    let styles = params.param("style").map_or(&[][..], |p| p.choices);
    cards(ui, params, "style", styles, &MAXIMIZER_MODES);
    ui.add_space(8.0);
    limiter_view(ui, params, meters, &mut state.limiter, "gain");
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        knob_section_items(
            ui,
            params,
            &[("Release", &["release", "link"], None)],
            &|_| true,
        );
        monitor_switches(ui, params, &mut state.limiter.comparing);
    });
}

/// How to listen to a limiter: at the level it came in, or only what it takes away.
fn monitor_switches(ui: &mut Ui, params: &mut dyn EditorParams, comparing: &mut bool) {
    let p = theme::current(ui);
    section_frame(ui, tr("Listen"), p.weak, 146.0, |ui| {
        ui.add_space(4.0);
        for (id, label, hint) in [
            (
                "unity",
                "Unity gain",
                "Takes the gain back off the output, to judge the limiting at the \
                         level it came in rather than louder",
            ),
            (
                "audition",
                "Audition",
                "Plays only what the limiter takes away",
            ),
        ] {
            params.toggle(ui, id, |ui, on| {
                let response = pill_switch(ui, *on, tr(label)).on_hover_text(tr(hint));
                if response.clicked() {
                    *on = !*on;
                }
                response
            });
            ui.add_space(4.0);
        }
        // Heard while the window is open, never saved.
        params.shown("compare");
        let response = pill_switch(ui, *comparing, tr("Before, same loudness")).on_hover_text(tr(
            "Hear the input without the limiter, raised to its output's loudness: what the \
             limiting does, without the loudness speaking for it",
        ));
        if response.clicked() {
            *comparing = !*comparing;
            params.hear("compare", comparing.then_some(1.0));
        }
    });
}

/// A mastering limiter's face: the input meter with the threshold, what the limiter takes
/// away scrolling in the middle, the output meter with the ceiling, then the readouts.
fn limiter_view(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    meters: &[f32],
    state: &mut LimiterState,
    gain_id: &str,
) {
    let p = theme::current(ui);
    let meter = |i: usize, default: f32| meters.get(i).copied().unwrap_or(default);
    let reduction = meter(0, 0.0);
    let inputs = [meter(2, -120.0), meter(3, -120.0)];
    let outputs = [meter(4, -120.0), meter(5, -120.0)];
    let (in_lufs, out_lufs) = (meter(6, -120.0), meter(7, -120.0));
    let gain = params.get(gain_id);
    let ceiling = params.get("ceiling");
    record_trace(
        ui,
        state,
        inputs[0].max(inputs[1]) + gain,
        outputs[0].max(outputs[1]),
    );

    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(560.0), 230.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, p.panel);
    let side = 78.0;
    let left = Rect::from_min_max(rect.min, pos2(rect.left() + side, rect.bottom()));
    let right = Rect::from_min_max(pos2(rect.right() - side, rect.top()), rect.max);
    let display = Rect::from_min_max(
        pos2(left.right(), rect.top() + 8.0),
        pos2(right.left(), rect.bottom() - 8.0),
    );
    draw_trace(&painter, display, &state.trace, ceiling, &p);

    // Input peaks above the threshold are pushed into the ceiling.
    let threshold = ceiling - gain;
    let (response, moved) = level_column(
        ui,
        &painter,
        left,
        (
            tr("THRESHOLD"),
            tr("Drag down to push the input harder into the ceiling"),
        ),
        (inputs, threshold),
        p.accent,
        true,
    );
    let default = params.param(gain_id).map_or(0.0, |p| p.default);
    let moved = if response.double_clicked() {
        Some(default)
    } else {
        moved.map(|db| ceiling - db)
    };
    params.set(gain_id, moved, &response);
    let (response, moved) = level_column(
        ui,
        &painter,
        right,
        (
            tr("CEILING"),
            tr("Drag to set the highest level the output reaches"),
        ),
        (outputs, ceiling),
        theme::SOLO,
        false,
    );
    let default = params.param("ceiling").map_or(0.0, |p| p.default);
    let moved = if response.double_clicked() {
        Some(default)
    } else {
        moved
    };
    params.set("ceiling", moved, &response);

    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        let text = |v: f32, unit: &str| {
            if v > -100.0 {
                format!("{v:.1} {unit}")
            } else {
                "-".into()
            }
        };
        let reduced = if reduction < -0.1 {
            theme::RECORD
        } else {
            p.text
        };
        let shown = if reduction > -0.05 { 0.0 } else { reduction };
        readout(ui, tr("GAIN REDUCTION"), format!("{shown:.1} dB"), reduced);
        let peak = outputs[0].max(outputs[1]);
        readout(ui, tr("OUTPUT PEAK"), text(peak, "dB"), p.text);
        readout(ui, tr("SHORT-TERM"), text(out_lufs, "LUFS"), p.text);
        readout(ui, tr("INTEGRATED"), text(meter(8, -120.0), "LUFS"), p.text);
        let true_peak = meter(9, -120.0);
        let over = if true_peak > ceiling + 0.05 {
            theme::RECORD
        } else {
            p.text
        };
        readout(ui, tr("TRUE PEAK"), text(true_peak, "dBTP"), over);
        let louder = (in_lufs > -70.0 && out_lufs > -70.0).then(|| {
            let louder = out_lufs - in_lufs;
            format!("{:+.1} dB", if louder.abs() < 0.05 { 0.0 } else { louder })
        });
        readout(
            ui,
            tr("LOUDER BY"),
            louder.unwrap_or_else(|| "-".into()),
            p.accent,
        );
        ui.add_space(10.0);
        learn(ui, params, state, gain_id, out_lufs);
    });
    ui.add_space(6.0);
}

/// Adds the levels of the frames since the last call to the scrolling display.
fn record_trace(ui: &Ui, state: &mut LimiterState, input: f32, output: f32) {
    let now = ui.input(|i| i.time);
    let step = 1.0 / TRACE_RATE;
    if now - state.trace_time > 1.0 {
        state.trace_time = now - step;
    }
    while now - state.trace_time >= step {
        state.trace_time += step;
        state.trace.push_back([input, output]);
    }
    while state.trace.len() > TRACE_POINTS {
        state.trace.pop_front();
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs_f64(step));
}

/// The input pushed into the limiter (red where it is taken away) and the output over it,
/// mirrored around the middle like a waveform, with the ceiling.
fn draw_trace(
    painter: &egui::Painter,
    rect: Rect,
    trace: &VecDeque<[f32; 2]>,
    ceiling: f32,
    p: &theme::Palette,
) {
    painter.rect_filled(rect, 6.0, theme::screen(p));
    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 6.0;
    let reach = |db: f32| ((db - METER_FLOOR) / (DISPLAY_TOP - METER_FLOOR)).clamp(0.0, 1.0) * half;
    let grid = p.weak.gamma_multiply(0.15);
    for db in [-24.0, -12.0, -6.0, 0.0] {
        for y in [mid - reach(db), mid + reach(db)] {
            painter.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(1.0, grid),
            );
        }
    }
    let step = rect.width() / TRACE_POINTS as f32;
    let x0 = rect.right() - trace.len() as f32 * step;
    let points = |k: usize| -> Vec<(f32, f32)> {
        trace
            .iter()
            .enumerate()
            .map(|(i, v)| (x0 + i as f32 * step, reach(v[k])))
            .collect()
    };
    painter.add(mirrored(
        &points(0),
        mid,
        theme::RECORD.gamma_multiply(0.75),
    ));
    let output = points(1);
    painter.add(mirrored(&output, mid, p.base.lerp_to_gamma(p.accent, 0.5)));
    for sign in [-1.0, 1.0] {
        let edge = output
            .iter()
            .map(|&(x, a)| pos2(x, mid - sign * a))
            .collect();
        painter.add(Shape::line(edge, Stroke::new(1.2, p.accent)));
    }
    for y in [mid - reach(ceiling), mid + reach(ceiling)] {
        painter.extend(Shape::dashed_line(
            &[pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.2, theme::SOLO),
            6.0,
            4.0,
        ));
    }
}

/// A band between `mid - a` and `mid + a` along the points `(x, a)`.
fn mirrored(points: &[(f32, f32)], mid: f32, color: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    for pair in points.windows(2) {
        let i = mesh.vertices.len() as u32;
        let [(x0, a0), (x1, a1)] = [pair[0], pair[1]];
        for q in [
            pos2(x0, mid - a0),
            pos2(x1, mid - a1),
            pos2(x1, mid + a1),
            pos2(x0, mid + a0),
        ] {
            mesh.colored_vertex(q, color);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    Shape::mesh(mesh)
}

/// A stereo peak meter with a level marker (threshold or ceiling) dragged up and down;
/// returns the column's response and where the marker was dragged to.
fn level_column(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    (label, hint): (&str, &str),
    (levels, marker): ([f32; 2], f32),
    color: Color32,
    scale_left: bool,
) -> (Response, Option<f32>) {
    let p = theme::current(ui);
    let (top, bottom) = (rect.top() + 24.0, rect.bottom() - 26.0);
    let height = bottom - top;
    let y = |db: f32| bottom - ((db - METER_FLOOR) / -METER_FLOOR).clamp(0.0, 1.0) * height;
    let cx = rect.center().x + if scale_left { 8.0 } else { -8.0 };
    for (i, level) in levels.into_iter().enumerate() {
        let x = cx - 10.5 + i as f32 * 12.0;
        let bar = Rect::from_min_max(pos2(x, top), pos2(x + 9.0, bottom));
        painter.rect_filled(bar, 2.0, theme::screen(&p));
        for (low, high, zone) in [
            (METER_FLOOR, -12.0, theme::PLAY),
            (-12.0, -3.0, theme::SOLO),
            (-3.0, 0.0, theme::RECORD),
        ] {
            if level > low {
                painter.rect_filled(
                    Rect::from_min_max(pos2(x, y(level.min(high))), pos2(x + 9.0, y(low))),
                    0.0,
                    zone,
                );
            }
        }
    }
    let (inner, outer) = if scale_left {
        (cx + 16.0, cx - 16.0)
    } else {
        (cx - 16.0, cx + 16.0)
    };
    for db in [0.0, -6.0, -12.0, -24.0, -36.0] {
        painter.text(
            pos2(outer, y(db)),
            if scale_left {
                Align2::RIGHT_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            format!("{db}"),
            FontId::proportional(8.5),
            p.weak,
        );
    }
    let my = y(marker);
    painter.line_segment(
        [pos2(cx - 15.0, my), pos2(cx + 15.0, my)],
        Stroke::new(2.0, color),
    );
    let tip = if scale_left { -1.0 } else { 1.0 };
    painter.add(Shape::convex_polygon(
        vec![
            pos2(inner, my),
            pos2(inner - tip * 9.0, my - 6.0),
            pos2(inner - tip * 9.0, my + 6.0),
        ],
        color,
        Stroke::NONE,
    ));
    painter.text(
        pos2(rect.center().x, rect.top() + 7.0),
        Align2::CENTER_TOP,
        label,
        FontId::proportional(9.0),
        p.weak,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 7.0),
        Align2::CENTER_BOTTOM,
        format!("{marker:.1} dB"),
        FontId::monospace(11.0),
        color,
    );
    let response = ui
        .interact(
            rect,
            ui.scope_id().with(("limiter", label)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
        .on_hover_text(format!(
            "{hint}. {}",
            tr("Shift for fine, double-click to reset")
        ));
    let fine = if ui.input(|i| i.modifiers.shift) {
        0.2
    } else {
        1.0
    };
    let moved = response
        .dragged()
        .then(|| marker + response.drag_delta().y / height * METER_FLOOR * fine);
    (response, moved)
}

fn readout(ui: &mut Ui, label: &str, value: String, color: Color32) {
    let p = theme::current(ui);
    let (fill, edge) = theme::card(&p);
    egui::Frame::new()
        .fill(fill)
        .stroke(edge)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(84.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new(label).size(9.0).color(p.weak));
                ui.label(RichText::new(value).monospace().size(14.0).color(color));
            });
        });
}

/// Learn: while the music plays, moves the threshold until the output reaches the target
/// loudness, waiting for the short-term loudness (3 s) to follow each move.
fn learn(
    ui: &mut Ui,
    params: &mut dyn EditorParams,
    state: &mut LimiterState,
    gain_id: &str,
    out_lufs: f32,
) {
    let p = theme::current(ui);
    let target = state.target.unwrap_or(-14.0);
    ui.vertical(|ui| {
        ui.label(RichText::new(tr("TARGET")).size(9.0).color(p.weak));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("limiter-target", params.salt()))
                .selected_text(format!("{target:.0} LUFS"))
                .show_ui(ui, |ui| {
                    for (value, name) in TARGETS {
                        if ui.selectable_label(target == value, tr(name)).clicked() {
                            state.target = Some(value);
                        }
                    }
                });
            let now = ui.input(|i| i.time);
            let learning = state.learn.is_some();
            let response = ui
                .add(
                    egui::Button::new(if learning {
                        tr("Learning")
                    } else {
                        tr("Learn")
                    })
                    .selected(learning),
                )
                .on_hover_text(tr(
                    "Play the loudest part: the threshold moves until the output reaches \
                     the target loudness",
                ));
            if response.clicked() {
                state.learn = (!learning).then_some((now, now - 3.0));
            }
            let Some((started, last)) = state.learn else {
                return;
            };
            ui.spinner();
            if now - started > 30.0 {
                state.learn = None;
                return;
            }
            if out_lufs < -70.0 || now - last < 3.0 {
                return;
            }
            let difference = target - out_lufs;
            let Some(range) = params.param(gain_id).map(|p| p.min..=p.max) else {
                return;
            };
            let gain = params.get(gain_id);
            let next = (gain + difference).clamp(*range.start(), *range.end());
            if difference.abs() < 0.3 || (next - gain).abs() < 0.05 {
                state.learn = None;
                return;
            }
            params.set(gain_id, Some(next), &response);
            params.end_gesture();
            state.learn = Some((started, now));
        });
    });
}

// ---------------------------------------------------------------- Spectrum analyzer

/// Rows of the spectrogram, one per frame drawn.
const SPECTROGRAM_ROWS: usize = 300;

/// What the analyzer display remembers between frames.
#[derive(Default)]
pub struct SpectrumState {
    /// Highest level of each band since the hold was reset.
    peaks: Vec<f32>,
    /// Spectrogram rows, newest last.
    rows: VecDeque<Vec<f32>>,
    texture: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for SpectrumState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpectrumState")
            .field("rows", &self.rows.len())
            .finish()
    }
}

/// Position of a frequency across the display, 0 at 20 Hz and 1 at 20 kHz.
fn frequency_fraction(hz: f32) -> f32 {
    (hz / 20.0).log10() / 3.0
}

/// Name of the note nearest a frequency, as A4 or C#3.
fn note_name(hz: f32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let number = (69.0 + 12.0 * (hz / 440.0).log2()).round() as i32;
    format!(
        "{}{}",
        NAMES[number.rem_euclid(12) as usize],
        number.div_euclid(12) - 1
    )
}

fn format_hz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// Color of a level in the spectrogram, from the floor (dark) to 0 dB (white hot).
fn heat(t: f32, accent: Color32, base: Color32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        base.lerp_to_gamma(accent, t * 2.0)
    } else if t < 0.8 {
        accent.lerp_to_gamma(theme::SOLO, (t - 0.5) / 0.3)
    } else {
        theme::SOLO.lerp_to_gamma(Color32::WHITE, (t - 0.8) / 0.2)
    }
}

fn analyzer(ui: &mut Ui, params: &mut dyn EditorParams, meters: &[f32], state: &mut SpectrumState) {
    let p = theme::current(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.horizontal(|ui| segmented(ui, params, "view", &[]));
        ui.separator();
        ui.horizontal(|ui| {
            segmented(
                ui,
                params,
                "source",
                &[
                    "Both channels together",
                    "Left channel",
                    "Right channel",
                    "What differs between the channels",
                ],
            )
        });
        ui.separator();
        ui.horizontal(|ui| segmented(ui, params, "range", &[]));
        ui.separator();
        params.toggle(ui, "hold", |ui, on| {
            ui.toggle_value(on, tr("Peak hold")).on_hover_text(tr(
                "Keeps the highest level of each band; click the display to clear it",
            ))
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        param_knob(ui, params, "decay", 34.0, p.accent);
        ui.weak(tr("How fast the display falls back after a peak"))
            .on_hover_text(tr("Slow speeds show the average tone, fast ones every hit"));
        ui.add_space(12.0);
        param_knob(ui, params, "tilt", 34.0, p.accent);
        ui.weak(tr("Raises the highs, so that a balanced mix looks flat"));
    });
    ui.add_space(6.0);

    let floor = audiostrada_dsp::analyzer::range_floor(params.get("range"));
    let tilt = params.get("tilt");
    let view = params.get("view").round() as usize;
    let hold = params.get("hold") > 0.5;
    let bands = audiostrada_dsp::analyzer::BANDS;
    let levels: Vec<f32> = (0..bands)
        .map(|b| {
            let db = meters.get(b).copied().unwrap_or(-120.0);
            let hz = audiostrada_dsp::analyzer::band_frequency(b);
            db + tilt * (hz / 1000.0).log2()
        })
        .collect();
    if state.peaks.len() != bands || !hold {
        state.peaks = levels.clone();
    } else {
        for (peak, &db) in state.peaks.iter_mut().zip(&levels) {
            *peak = peak.max(db);
        }
    }
    state.rows.push_back(levels.clone());
    while state.rows.len() > SPECTROGRAM_ROWS {
        state.rows.pop_front();
    }

    let size = vec2(
        ui.available_width().max(420.0),
        (ui.available_height() - 8.0).max(220.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        state.peaks = levels.clone();
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::screen(&p));
    // Room for the scales.
    let plot = Rect::from_min_max(rect.min + vec2(36.0, 8.0), rect.max - vec2(8.0, 20.0));
    let x_of = |hz: f32| plot.left() + frequency_fraction(hz) * plot.width();
    let y_of = |db: f32| plot.top() + (db / floor).clamp(0.0, 1.0) * plot.height();
    let grid = p.weak.gamma_multiply(0.18);
    let label = |pos: Pos2, align: Align2, text: String| {
        painter.text(pos, align, text, FontId::proportional(10.0), p.weak);
    };
    for hz in [
        30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0,
        800.0, 900.0, 1000.0, 2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 7000.0, 8000.0, 9000.0,
        10000.0, 20000.0,
    ] {
        let x = x_of(hz);
        let major = [100.0, 1000.0, 10000.0].contains(&hz);
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            Stroke::new(
                1.0,
                if major {
                    grid.gamma_multiply(2.0)
                } else {
                    grid
                },
            ),
        );
        if [
            50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
        ]
        .contains(&hz)
        {
            let text = if hz >= 1000.0 {
                format!("{}k", hz / 1000.0)
            } else {
                format!("{hz}")
            };
            label(pos2(x, plot.bottom() + 4.0), Align2::CENTER_TOP, text);
        }
    }
    if view == 2 {
        draw_spectrogram(ui, &painter, plot, state, floor, &p);
    } else {
        let step = match floor as i32 {
            -60 => 10,
            -90 => 15,
            _ => 20,
        };
        for db in (floor as i32..=0).step_by(step) {
            let y = y_of(db as f32);
            painter.line_segment(
                [pos2(plot.left(), y), pos2(plot.right(), y)],
                Stroke::new(1.0, grid),
            );
            label(
                pos2(plot.left() - 4.0, y),
                Align2::RIGHT_CENTER,
                format!("{db}"),
            );
        }
        let points: Vec<Pos2> = levels
            .iter()
            .enumerate()
            .map(|(b, &db)| pos2(x_of(audiostrada_dsp::analyzer::band_frequency(b)), y_of(db)))
            .collect();
        if view == 1 {
            let width = plot.width() / bands as f32;
            for point in &points {
                let bar = Rect::from_min_max(
                    pos2(point.x - width / 2.0 + 0.5, point.y),
                    pos2(point.x + width / 2.0 - 0.5, plot.bottom()),
                );
                painter.rect_filled(bar, 0.0, p.accent.gamma_multiply(0.8));
            }
        } else {
            let mut mesh = egui::Mesh::default();
            let fill = p.accent.gamma_multiply(0.25);
            for point in &points {
                mesh.colored_vertex(*point, fill);
                mesh.colored_vertex(pos2(point.x, plot.bottom()), fill.gamma_multiply(0.2));
            }
            for i in 0..points.len().saturating_sub(1) as u32 {
                mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
                mesh.add_triangle(2 * i + 1, 2 * i + 2, 2 * i + 3);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::line(points, Stroke::new(1.5, p.accent)));
        }
        if hold {
            let peaks = state
                .peaks
                .iter()
                .enumerate()
                .map(|(b, &db)| pos2(x_of(audiostrada_dsp::analyzer::band_frequency(b)), y_of(db)))
                .collect();
            painter.add(Shape::line(
                peaks,
                Stroke::new(1.0, theme::SOLO.gamma_multiply(0.8)),
            ));
        }
    }
    if let Some(pointer) = response.hover_pos().filter(|pos| plot.contains(*pos)) {
        let hz = 20.0 * 1000f32.powf((pointer.x - plot.left()) / plot.width());
        let band = ((frequency_fraction(hz) * bands as f32) as usize).min(bands - 1);
        painter.line_segment(
            [pos2(pointer.x, plot.top()), pos2(pointer.x, plot.bottom())],
            Stroke::new(1.0, p.weak.gamma_multiply(0.5)),
        );
        let text = format!(
            "{}  {}  {:.1} dB",
            format_hz(hz),
            note_name(hz),
            levels[band]
        );
        let align = if pointer.x > plot.center().x {
            Align2::RIGHT_TOP
        } else {
            Align2::LEFT_TOP
        };
        let offset = if pointer.x > plot.center().x {
            -8.0
        } else {
            8.0
        };
        painter.text(
            pos2(pointer.x + offset, plot.top() + 4.0),
            align,
            text,
            FontId::monospace(11.0),
            p.text,
        );
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

/// Frequencies across, time downwards (newest at the top), level as heat.
fn draw_spectrogram(
    ui: &Ui,
    painter: &egui::Painter,
    plot: Rect,
    state: &mut SpectrumState,
    floor: f32,
    p: &theme::Palette,
) {
    let bands = audiostrada_dsp::analyzer::BANDS;
    let mut image = egui::ColorImage::filled([bands, SPECTROGRAM_ROWS], p.base);
    for (row, levels) in state.rows.iter().rev().enumerate() {
        for (band, &db) in levels.iter().enumerate() {
            image.pixels[row * bands + band] = heat(1.0 - db / floor, p.accent, p.base);
        }
    }
    let texture = match &mut state.texture {
        Some(texture) => {
            texture.set(image, egui::TextureOptions::LINEAR);
            texture
        }
        None => state.texture.insert(ui.ctx().load_texture(
            "spectrogram",
            image,
            egui::TextureOptions::LINEAR,
        )),
    };
    // Bands are log-spaced like the frequency scale, so the image maps straight onto it.
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(texture.id(), plot, uv, Color32::WHITE);
}
