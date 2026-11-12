//! Prepare the track: each track chosen is rendered alone and measured, its instrument
//! guessed, then the window proposes what Clean up and Bring out would set (see
//! `audiostrada_dsp::prepare`). While it is open the proposal is heard, against the track as
//! it was and at the same level; Apply writes it in one undo step.

use audiostrada_core::automation::AutomationLane;
use audiostrada_core::prepare::{self as write, Added, Preparation};
use audiostrada_core::{Insert, Prepared, TrackId};
use audiostrada_dsp::prepare::{self as rules, Analysis, Cut, Instrument, Step, Strength};
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};

use crate::document::{Document, Measuring};
use crate::effect_frame::{
    Face, Glyph, field_fill, glyph_button, line_edge, paint_bottom_band, paint_title,
    paint_top_band, radius, text_button,
};
use crate::i18n::tr;
use crate::theme::{self, Palette};
use crate::trf;

const WIDTH: f32 = 820.0;
/// Color of what the cleaning removes.
const CUT: Color32 = Color32::from_rgb(0xe3, 0xa2, 0x3b);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Clean,
    BringOut,
    Both,
}

impl Mode {
    fn cleans(self) -> bool {
        matches!(self, Mode::Clean | Mode::Both)
    }

    fn brings_out(self) -> bool {
        matches!(self, Mode::BringOut | Mode::Both)
    }

    fn title(self) -> &'static str {
        match self {
            Mode::Clean => tr("Clean up"),
            Mode::BringOut => tr("Bring out"),
            Mode::Both => tr("Clean up and bring out"),
        }
    }
}

fn request_id() -> egui::Id {
    egui::Id::unique("prepare-request")
}

/// Asks for the window on `tracks`.
pub fn request(ctx: &egui::Context, tracks: &[TrackId], mode: Mode) {
    if !tracks.is_empty() {
        ctx.data_mut(|d| d.insert_temp(request_id(), (tracks.to_vec(), mode)));
    }
}

/// Where the instrument came from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Guess {
    Name,
    Instrument,
    Sound,
    Chosen,
}

enum State {
    /// Waiting for its turn to be rendered.
    Queued,
    Measuring(Box<Measuring>),
    Analysing(std::thread::JoinHandle<Analysis>),
    Ready(Box<Ready>),
    Failed(String),
}

/// A step proposed: kept or not, and how much of it (0.5: as proposed).
struct Choice {
    step: Step,
    on: bool,
    amount: f32,
}

struct Ready {
    analysis: Analysis,
    instrument: Instrument,
    guess: Guess,
    steps: Vec<Choice>,
}

/// What the preview changes on a track, as it was when the window opened.
struct Saved {
    inserts: Vec<Insert>,
    automation: Vec<AutomationLane>,
    prepared: Option<Prepared>,
}

/// Effect settings, by parameter.
type Settings = Vec<(String, f32)>;

struct Row {
    track: TrackId,
    name: String,
    color: Color32,
    saved: Saved,
    state: State,
    /// Prepared with the others (several tracks: each can be left out).
    included: bool,
    /// The track holds the proposal, to be heard.
    previewed: bool,
    /// Gain (dB) keeping the loudness the track had, for the settings in `level_of`.
    level: f32,
    level_of: Settings,
    /// That gain being computed on its own thread, for these settings.
    leveling: Option<(Settings, std::thread::JoinHandle<f32>)>,
}

pub struct PrepareWindow {
    mode: Option<Mode>,
    rows: Vec<Row>,
    /// The track shown.
    shown: usize,
    strength: Strength,
    keep_level: bool,
    /// The preparation is heard (else the tracks as they were).
    after: bool,
    /// The proposal changed: the preview follows.
    dirty: bool,
}

impl Default for PrepareWindow {
    fn default() -> Self {
        Self {
            mode: None,
            rows: Vec::new(),
            shown: 0,
            strength: Strength::Normal,
            keep_level: true,
            after: true,
            dirty: false,
        }
    }
}

impl std::fmt::Debug for PrepareWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrepareWindow")
            .field("mode", &self.mode)
            .field("rows", &self.rows.len())
            .finish()
    }
}

/// The instrument of a track: as prepared before, else its name, its instrument, its sound.
fn guess(doc: &Document, track: TrackId, analysis: &Analysis) -> (Instrument, Guess) {
    let project = doc.project();
    let Some(t) = project.track(track) else {
        return (Instrument::Other, Guess::Sound);
    };
    if let Some(i) = t
        .prepared
        .as_ref()
        .and_then(|p| Instrument::from_id(&p.instrument))
    {
        return (i, Guess::Chosen);
    }
    if let Some(i) = Instrument::from_name(&t.name, analysis.pitch) {
        return (i, Guess::Name);
    }
    match t.inserts.first().map(|i| i.effect.as_str()) {
        Some("drums") => return (Instrument::Drums, Guess::Instrument),
        Some("synth") => return (Instrument::Synth, Guess::Instrument),
        _ => {}
    }
    (Instrument::from_sound(analysis), Guess::Sound)
}

fn propose(
    analysis: &Analysis,
    instrument: Instrument,
    mode: Mode,
    strength: Strength,
) -> Vec<Choice> {
    let mut steps = Vec::new();
    if mode.cleans() {
        steps.extend(rules::clean(analysis, instrument, strength));
    }
    if mode.brings_out() {
        steps.extend(rules::bring_out(analysis, instrument, strength));
    }
    steps
        .into_iter()
        .map(|step| Choice {
            on: step.default_on(),
            step,
            amount: 0.5,
        })
        .collect()
}

/// The steps kept, each at its amount.
fn chosen(r: &Ready) -> Vec<Step> {
    r.steps
        .iter()
        .filter(|c| c.on)
        .map(|c| c.step.scaled(c.amount * 2.0))
        .collect()
}

/// What a track's ticked steps write; `gains` keep the level (the strip's, then each effect's).
fn preparation(r: &Ready, mode: Mode, rate: u32, after_first: bool, gains: &[f32]) -> Preparation {
    let picked = chosen(r);
    let ride = picked
        .iter()
        .find_map(|s| match s {
            Step::Ride(points) => Some(points),
            _ => None,
        })
        .map(|points| {
            std::iter::once((0, 0.0))
                .chain(
                    points
                        .iter()
                        .map(|&(s, db)| ((s * f64::from(rate)) as u64, db)),
                )
                .collect()
        })
        .unwrap_or_default();
    Preparation {
        instrument: r.instrument.id().to_owned(),
        clean: mode.cleans(),
        bring_out: mode.brings_out(),
        strip: rules::strip_settings(picked.iter()),
        strip_gain: gains.first().copied().unwrap_or(0.0),
        inserts: rules::inserts(picked.iter())
            .into_iter()
            .enumerate()
            .map(|(k, (effect, params))| Added {
                effect: effect.to_owned(),
                params,
                gain_db: gains.get(k + 1).copied().unwrap_or(0.0),
            })
            .collect(),
        ride,
        after_first,
    }
}

impl PrepareWindow {
    /// Every track measured.
    #[cfg(test)]
    pub fn ready(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|r| matches!(r.state, State::Ready(_)))
    }

    #[cfg(test)]
    pub fn apply_now(&mut self, doc: &mut Document) {
        if let Some(mode) = self.mode {
            self.apply(doc, mode);
        }
    }

    fn open(&mut self, doc: &mut Document, tracks: Vec<TrackId>, mode: Mode) {
        self.mode = Some(mode);
        self.shown = 0;
        self.after = true;
        self.rows = tracks
            .into_iter()
            .filter_map(|track| {
                let t = doc.project().track(track)?;
                let (name, color) = (t.name.clone(), theme::track_color(t.color));
                let saved = Saved {
                    inserts: t.inserts.clone(),
                    automation: t.automation.clone(),
                    prepared: t.prepared.clone(),
                };
                Some(Row {
                    track,
                    name,
                    color,
                    saved,
                    state: State::Queued,
                    included: true,
                    previewed: false,
                    level: 0.0,
                    level_of: Vec::new(),
                    leveling: None,
                })
            })
            .collect();
    }

    /// Renders the tracks a few at a time, measures each on its own thread, then proposes.
    fn poll(&mut self, doc: &mut Document, mode: Mode) {
        const AT_ONCE: usize = 3;
        let mut busy = 0;
        for row in &mut self.rows {
            let state = std::mem::replace(&mut row.state, State::Queued);
            row.state = match state {
                State::Measuring(mut m) => match m.poll() {
                    None => State::Measuring(m),
                    Some(Ok((left, right, rate))) => {
                        State::Analysing(std::thread::spawn(move || {
                            rules::analyse(&left, &right, rate)
                        }))
                    }
                    Some(Err(e)) => State::Failed(e),
                },
                State::Analysing(handle) if handle.is_finished() => match handle.join() {
                    Ok(analysis) if analysis.report.integrated.is_some() => {
                        let (instrument, guess) = guess(doc, row.track, &analysis);
                        let steps = propose(&analysis, instrument, mode, self.strength);
                        self.dirty = true;
                        State::Ready(Box::new(Ready {
                            analysis,
                            instrument,
                            guess,
                            steps,
                        }))
                    }
                    Ok(_) => State::Failed(tr("The track is silent").to_owned()),
                    Err(_) => State::Failed(tr("The analysis failed").to_owned()),
                },
                other => other,
            };
            if matches!(row.state, State::Failed(_)) {
                row.included = false;
            }
            busy += usize::from(matches!(
                row.state,
                State::Measuring(_) | State::Analysing(_)
            ));
        }
        for row in &mut self.rows {
            if busy >= AT_ONCE {
                break;
            }
            if matches!(row.state, State::Queued) {
                row.state = match doc.start_source_render(row.track) {
                    Ok(m) => State::Measuring(Box::new(m)),
                    Err(e) => {
                        row.included = false;
                        State::Failed(e)
                    }
                };
                busy += 1;
            }
        }
    }

    /// Computes, off the interface's thread, the gain keeping each track's loudness whenever
    /// its settings change; true while some are being computed.
    fn levels(&mut self) -> bool {
        let mut busy = false;
        for row in &mut self.rows {
            let State::Ready(r) = &row.state else {
                continue;
            };
            if !self.keep_level || !row.included {
                continue;
            }
            if row.leveling.as_ref().is_some_and(|l| l.1.is_finished())
                && let Some((settings, handle)) = row.leveling.take()
            {
                row.level = handle.join().unwrap_or(0.0);
                row.level_of = settings;
                self.dirty = true;
            }
            if row.leveling.is_none() {
                let picked = chosen(r);
                let strip = rules::strip_settings(picked.iter());
                let inserts = rules::inserts(picked.iter());
                let mut settings = strip.clone();
                for (effect, params) in &inserts {
                    settings.extend(params.iter().map(|(k, v)| (format!("{effect}/{k}"), *v)));
                }
                if settings != row.level_of {
                    let (excerpt, rate) = (r.analysis.excerpt.clone(), r.analysis.rate);
                    let handle = std::thread::spawn(move || {
                        rules::level_kept(&excerpt, rate, &strip, &inserts)
                    });
                    row.leveling = Some((settings, handle));
                }
            }
            busy |= row.leveling.is_some();
        }
        busy
    }

    /// Puts the tracks back as they were when the window opened, outside the undo history.
    fn restore(&self, doc: &mut Document) {
        doc.history_mut().modify_unrecorded(|p| {
            for row in &self.rows {
                if let Some(t) = p.track_mut(row.track) {
                    t.inserts = row.saved.inserts.clone();
                    t.automation = row.saved.automation.clone();
                    t.prepared = row.saved.prepared.clone();
                }
            }
        });
    }

    /// Lets the proposal be heard, once every track is measured: written outside the undo
    /// history over what is already heard (the effects stay in place, their settings change),
    /// or taken back out. False while tracks are still being measured.
    fn preview(&mut self, doc: &mut Document, mode: Mode) -> bool {
        if self
            .rows
            .iter()
            .any(|r| !matches!(r.state, State::Ready(_) | State::Failed(_)))
        {
            return false;
        }
        if !self.after {
            self.restore(doc);
            self.rows.iter_mut().for_each(|r| r.previewed = false);
            return true;
        }
        let rate = doc.project().sample_rate;
        let mut preparations: Vec<(TrackId, Preparation)> = Vec::new();
        let mut left_out: Vec<usize> = Vec::new();
        for (k, row) in self.rows.iter_mut().enumerate() {
            match &row.state {
                State::Ready(r) if row.included => {
                    let after_first = doc.is_instrument_track(row.track);
                    let gains: &[f32] = if self.keep_level { &[row.level] } else { &[] };
                    preparations.push((row.track, preparation(r, mode, rate, after_first, gains)));
                    row.previewed = true;
                }
                _ if row.previewed => {
                    left_out.push(k);
                    row.previewed = false;
                }
                _ => {}
            }
        }
        let rows = &self.rows;
        doc.history_mut().modify_unrecorded(|p| {
            for &k in &left_out {
                let row = &rows[k];
                if let Some(t) = p.track_mut(row.track) {
                    t.inserts = row.saved.inserts.clone();
                    t.automation = row.saved.automation.clone();
                    t.prepared = row.saved.prepared.clone();
                }
            }
            for (track, prep) in &preparations {
                write::prepare(p, *track, prep);
            }
        });
        true
    }

    /// Writes every track's ticked steps, in one undo step.
    fn apply(&mut self, doc: &mut Document, mode: Mode) {
        self.restore(doc);
        let rate = doc.project().sample_rate;
        let preparations: Vec<(TrackId, Preparation)> = self
            .rows
            .iter()
            .filter_map(|row| {
                let State::Ready(r) = &row.state else {
                    return None;
                };
                if !row.included {
                    return None;
                }
                let gains: &[f32] = if self.keep_level { &[row.level] } else { &[] };
                let after_first = doc.is_instrument_track(row.track);
                Some((row.track, preparation(r, mode, rate, after_first, gains)))
            })
            .collect();
        doc.history_mut().edit(tr("Prepare the track"), |p| {
            for (track, prep) in &preparations {
                write::prepare(p, *track, prep);
            }
        });
        self.mode = None;
        self.rows.clear();
    }

    /// Closes without keeping anything.
    fn cancel(&mut self, doc: &mut Document) {
        if self.mode.take().is_some() {
            self.restore(doc);
            self.rows.clear();
        }
    }

    /// Shows the window; true when it asks to play or pause.
    pub fn show(&mut self, ctx: &egui::Context, doc: &mut Document) -> bool {
        if let Some((tracks, mode)) =
            ctx.data_mut(|d| d.remove_temp::<(Vec<TrackId>, Mode)>(request_id()))
        {
            self.cancel(doc);
            self.open(doc, tracks, mode);
        }
        let Some(mode) = self.mode else {
            return false;
        };
        self.poll(doc, mode);
        let leveling = self.levels();
        if self.dirty && self.preview(doc, mode) {
            self.dirty = false;
        }
        let measuring = self
            .rows
            .iter()
            .any(|r| !matches!(r.state, State::Ready(_) | State::Failed(_)));
        if measuring || leveling {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.shown = self.shown.min(self.rows.len().saturating_sub(1));

        let p = theme::current_for(ctx);
        let mut asked = Asked::default();
        let playing = doc.player().engine().playing();
        egui::Window::new("prepare")
            .id(egui::Id::unique("prepare-window"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0, p.border))
                    .shadow(egui::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: Color32::from_black_alpha(110),
                    }),
            )
            .pivot(Align2::CENTER_TOP)
            .default_pos(ctx.content_rect().center_top() + vec2(0.0, 60.0))
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;
                self.header(ui, &p, mode, &mut asked);
                egui::Frame::new().inner_margin(16.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                    self.body(ui, &p, mode);
                });
                self.footer(ui, &p, playing, &mut asked);
            });
        if asked.apply {
            self.apply(doc, mode);
        } else if asked.close {
            self.cancel(doc);
        }
        asked.play
    }

    /// The top bar: the logo, what is being done and to which tracks, the close button.
    fn header(&mut self, ui: &mut Ui, p: &Palette, mode: Mode, asked: &mut Asked) {
        let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 58.0), Sense::hover());
        let color = self.rows.get(self.shown).map_or(p.accent, |r| r.color);
        let painter = ui.painter().clone();
        let bar = paint_top_band(&painter, p, rect, color);
        let names = match self.rows.as_slice() {
            [one] => one.name.clone(),
            rows => trf!("{count} tracks", count = rows.len()),
        };
        let detail = format!("{} · {names}", mode.title());
        let weak = p.weak;
        paint_title(
            &painter,
            p,
            bar,
            WIDTH - 120.0,
            tr("Prepare the track"),
            &|painter, at| {
                painter.text(
                    at,
                    Align2::LEFT_CENTER,
                    &detail,
                    FontId::proportional(11.5),
                    weak,
                );
            },
            false,
        );
        let close =
            Rect::from_center_size(pos2(bar.right() - 26.0, bar.center().y), vec2(28.0, 28.0));
        if glyph_button(
            ui,
            close,
            ui.scope_id().with("close"),
            Glyph::Close,
            None,
            true,
        )
        .on_hover_text(tr("Close without keeping anything"))
        .clicked()
        {
            asked.close = true;
        }
    }

    fn body(&mut self, ui: &mut Ui, p: &Palette, mode: Mode) {
        if self.rows.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for (k, row) in self.rows.iter().enumerate() {
                    let face = if k == self.shown {
                        Face::Lit(theme::mix(p.surface, row.color, 0.55))
                    } else {
                        Face::Plain
                    };
                    let mark = match row.state {
                        State::Failed(_) => "! ",
                        State::Ready(_) if row.included => "",
                        State::Ready(_) => "– ",
                        _ => "… ",
                    };
                    if chip(ui, ("track", k), &format!("{mark}{}", row.name), face).clicked() {
                        self.shown = k;
                    }
                }
            });
        }
        let strength = self.strength;
        let mut pick = None;
        let several = self.rows.len() > 1;
        let Some(row) = self.rows.get_mut(self.shown) else {
            return;
        };
        if several && matches!(row.state, State::Ready(_)) {
            ui.horizontal(|ui| {
                if toggle(ui, p, &mut row.included).changed() {
                    self.dirty = true;
                }
                ui.label(trf!("Prepare “{name}”", name = row.name.as_str()))
                    .on_hover_text(tr("Off, this track is left as it is"));
            });
        }
        match &mut row.state {
            State::Queued => waiting(ui, p, 0.0),
            State::Analysing(_) => waiting(ui, p, 1.0),
            State::Measuring(m) => {
                waiting(ui, p, m.progress());
            }
            State::Failed(e) => {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.colored_label(theme::MUTE, e.as_str()));
                ui.add_space(30.0);
            }
            State::Ready(r) => {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("The instrument is")).color(p.weak));
                    egui::ComboBox::from_id_salt("instrument")
                        .width(210.0)
                        .selected_text(RichText::new(tr(r.instrument.name())).strong())
                        .show_ui(ui, |ui| {
                            for i in Instrument::ALL {
                                if ui
                                    .selectable_label(r.instrument == i, tr(i.name()))
                                    .clicked()
                                {
                                    r.instrument = i;
                                    r.guess = Guess::Chosen;
                                    changed = true;
                                }
                            }
                        });
                    ui.label(
                        RichText::new(match r.guess {
                            Guess::Name => tr("from the track's name"),
                            Guess::Instrument => tr("from its instrument"),
                            Guess::Sound => tr("from its sound: check it"),
                            Guess::Chosen => "",
                        })
                        .size(11.0)
                        .color(p.weak),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (k, s) in Strength::ALL.into_iter().enumerate().rev() {
                            let face = if s == strength {
                                Face::Lit(theme::mix(p.surface, p.accent, 0.6))
                            } else {
                                Face::Plain
                            };
                            let corners = segment_corners(p, k, Strength::ALL.len());
                            if chip_with(ui, ("strength", k), tr(s.name()), face, corners)
                                .on_hover_text(tr(
                                    "How far the preparation goes, for every track here",
                                ))
                                .clicked()
                            {
                                pick = Some(s);
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new(tr("Strength")).color(p.weak));
                    });
                });
                if changed {
                    r.steps = propose(&r.analysis, r.instrument, mode, strength);
                    self.dirty = true;
                }
                spectrum(ui, p, r);
                let both = mode == Mode::Both;
                let mut toggled = false;
                ui.columns(if both { 2 } else { 1 }, |columns| {
                    let mut column = 0;
                    for (brings_out, title, tint) in [
                        (false, tr("Clean up"), CUT),
                        (true, tr("Bring out"), p.accent),
                    ] {
                        if brings_out && !mode.brings_out() || !brings_out && !mode.cleans() {
                            continue;
                        }
                        toggled |= card(&mut columns[column], p, title, tint, r, brings_out);
                        column += 1;
                    }
                });
                if toggled {
                    self.dirty = true;
                }
            }
        }
        if let Some(s) = pick {
            self.strength = s;
            for row in &mut self.rows {
                if let State::Ready(r) = &mut row.state {
                    r.steps = propose(&r.analysis, r.instrument, mode, s);
                }
            }
            self.dirty = true;
        }
    }

    /// The bottom bar: listening (play, before and after, the level kept), then the outcome.
    fn footer(&mut self, ui: &mut Ui, p: &Palette, playing: bool, asked: &mut Asked) {
        let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 54.0), Sense::hover());
        paint_bottom_band(ui.painter(), p, rect);
        let ready = self.rows.iter().any(|r| matches!(r.state, State::Ready(_)));
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(16.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut bar;
        ui.spacing_mut().item_spacing.x = 0.0;
        let play = if playing { tr("Pause") } else { tr("Listen") };
        if chip(ui, "play", play, Face::Plain)
            .on_hover_text(tr("Plays the session: solo the track to hear it alone"))
            .clicked()
        {
            asked.play = true;
        }
        ui.add_space(12.0);
        for (k, (after, text)) in [(false, tr("Before")), (true, tr("After"))]
            .into_iter()
            .enumerate()
        {
            let face = if self.after == after {
                Face::Lit(theme::mix(p.surface, p.accent, 0.6))
            } else {
                Face::Plain
            };
            if chip_with(ui, ("ab", k), text, face, segment_corners(p, k, 2))
                .on_hover_text(tr(
                    "What is heard: the track as it was, or prepared as proposed here",
                ))
                .clicked()
                && self.after != after
            {
                self.after = after;
                self.dirty = true;
            }
        }
        ui.add_space(14.0);
        if toggle(ui, p, &mut self.keep_level).changed() {
            self.dirty = true;
        }
        ui.add_space(6.0);
        let total: f32 = self.rows.get(self.shown).map_or(0.0, |r| r.level);
        ui.label(tr("Same level")).on_hover_text(tr(
            "Brings the prepared track to the loudness it had, measured on its loudest 20 \
             seconds: before and after compare at equal level, and Apply keeps that gain on \
             the channel strip's output",
        ));
        if self.keep_level {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{total:+.1} dB"))
                    .font(FontId::monospace(11.0))
                    .color(p.accent),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let apply = chip(
                ui,
                "apply",
                tr("Apply"),
                if ready {
                    Face::Lit(p.accent)
                } else {
                    Face::Plain
                },
            );
            if apply.clicked() && ready {
                asked.apply = true;
            }
            if chip(ui, "cancel", tr("Cancel"), Face::Plain).clicked() {
                asked.close = true;
            }
        });
    }
}

/// What the window's buttons asked for.
#[derive(Default)]
struct Asked {
    apply: bool,
    close: bool,
    play: bool,
}

fn segment_corners(p: &Palette, k: usize, count: usize) -> egui::CornerRadius {
    let r = radius(p) as u8;
    egui::CornerRadius {
        nw: if k == 0 { r } else { 0 },
        sw: if k == 0 { r } else { 0 },
        ne: if k + 1 == count { r } else { 0 },
        se: if k + 1 == count { r } else { 0 },
    }
}

/// A button of the window's bars, as wide as its text.
fn chip(ui: &mut Ui, id: impl egui::AsIdSalt, text: &str, face: Face) -> Response {
    let corners = egui::CornerRadius::same(radius(&theme::current(ui)) as u8);
    chip_with(ui, id, text, face, corners)
}

fn chip_with(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    text: &str,
    face: Face,
    corners: egui::CornerRadius,
) -> Response {
    let p = theme::current(ui);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
        .size()
        .x
        + 26.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    text_button(ui, rect, ui.scope_id().with(id), text, face, corners)
}

/// A small switch.
fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(28.0, 16.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let fill = if *on {
        theme::mix(p.surface, p.accent, 0.8)
    } else {
        field_fill(p)
    };
    ui.painter().rect(
        rect,
        8.0,
        fill,
        Stroke::new(1.0, line_edge(p)),
        StrokeKind::Inside,
    );
    let x = if *on {
        rect.right() - 8.0
    } else {
        rect.left() + 8.0
    };
    ui.painter().circle_filled(
        pos2(x, rect.center().y),
        5.0,
        if *on { Color32::WHITE } else { p.weak },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The track being rendered and measured.
fn waiting(ui: &mut Ui, p: &Palette, progress: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 220.0), Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        radius(p),
        field_fill(p),
        Stroke::new(1.0, line_edge(p)),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center() - vec2(0.0, 14.0),
        Align2::CENTER_CENTER,
        tr("Listening to the track…"),
        theme::strong(13.0),
        p.text,
    );
    let bar = Rect::from_center_size(rect.center() + vec2(0.0, 12.0), vec2(260.0, 4.0));
    painter.rect_filled(bar, 2.0, p.surface);
    painter.rect_filled(
        bar.with_max_x(bar.left() + bar.width() * progress.clamp(0.0, 1.0)),
        2.0,
        p.accent,
    );
}

fn hz(f: f32) -> String {
    if f >= 1000.0 {
        let k = f / 1000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{k:.0} kHz")
        } else {
            format!("{k:.1} kHz")
        }
    } else {
        format!("{f:.0} Hz")
    }
}

/// What a step does, and a word on why or how.
fn describe(step: &Step) -> (String, String) {
    let down = |cut: &Cut| format!("{:.1}", cut.depth);
    match step {
        Step::Filters { hpf, lpf, steep } => {
            let what = match (hpf, lpf) {
                (Some(h), Some(l)) => trf!(
                    "High pass {low}, low pass {high}",
                    low = hz(*h),
                    high = hz(*l)
                ),
                (Some(h), None) => trf!("High pass {low}", low = hz(*h)),
                (None, Some(l)) => trf!("Low pass {high}", high = hz(*l)),
                (None, None) => String::new(),
            };
            let slope = if *steep { "24 dB/oct" } else { "12 dB/oct" };
            (what, trf!("Nothing useful beyond, {slope}", slope = slope))
        }
        Step::Resonance(cut) => (
            trf!(
                "Resonance at {freq}: {gain} dB",
                freq = hz(cut.freq),
                gain = down(cut)
            ),
            if cut.threshold.is_some() {
                tr("Strada-EQ, dynamic: only while it stands out").to_owned()
            } else {
                tr("Strada-EQ, fixed: it is always there").to_owned()
            },
        ),
        Step::Plosives(cut) => (
            trf!(
                "Plosives: {gain} dB under {freq}",
                gain = down(cut),
                freq = hz(cut.freq)
            ),
            tr("Strada-EQ, dynamic: only on the bursts of P and B").to_owned(),
        ),
        Step::DeEss {
            freq, range, sure, ..
        } => (
            trf!(
                "De-esser from {freq}: up to -{range} dB",
                freq = hz(*freq),
                range = format!("{range:.0}")
            ),
            if *sure {
                tr("Strada-DeEss, on the S and T sounds heard").to_owned()
            } else {
                tr("Strada-DeEss: to try on harsh cymbals").to_owned()
            },
        ),
        Step::Gate { threshold, .. } => (
            trf!(
                "Gate at {threshold} dB",
                threshold = format!("{threshold:.0}")
            ),
            tr("Bleed between the hits").to_owned(),
        ),
        Step::Eq {
            band,
            freq,
            gain,
            bell,
            purpose,
            ..
        } => {
            let name = [tr("Low"), tr("Low mid"), tr("High mid"), tr("High")][(*band).min(3)];
            let shape = if *bell { tr("bell") } else { tr("shelf") };
            (
                trf!(
                    "{purpose}: {gain} dB at {freq}",
                    purpose = tr(purpose),
                    gain = format!("{gain:+.1}"),
                    freq = hz(*freq)
                ),
                trf!(
                    "Channel strip EQ, {band}, {shape}",
                    band = name,
                    shape = shape
                ),
            )
        }
        Step::Compressor {
            ratio,
            attack,
            release,
            threshold,
            reduction,
        } => (
            trf!(
                "Compressor {ratio}:1, about -{reduction} dB",
                ratio = format!("{ratio:.1}"),
                reduction = format!("{reduction:.1}")
            ),
            trf!(
                "Threshold {threshold} dB, attack {attack} ms, release {release} ms",
                threshold = format!("{threshold:.0}"),
                attack = format!("{attack:.0}"),
                release = format!("{release:.0}")
            ),
        ),
        Step::Ride(_) => (
            tr("Level riding, within 6 dB").to_owned(),
            tr("Written as automation of the strip's output, editable").to_owned(),
        ),
        Step::StripSilence(share) => (
            tr("Strip silence (Edit menu)").to_owned(),
            trf!(
                "{percent} % of silence: does better than a gate",
                percent = format!("{:.0}", share * 100.0)
            ),
        ),
    }
}

/// A card of the steps of one part; true when one was ticked or unticked.
fn card(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    tint: Color32,
    r: &mut Ready,
    brings_out: bool,
) -> bool {
    let mut toggled = false;
    egui::Frame::new()
        .fill(field_fill(p))
        .stroke(Stroke::new(1.0, line_edge(p)))
        .corner_radius(radius(p))
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(150.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, tint);
                ui.label(RichText::new(title).font(theme::strong(13.0)));
            });
            let mut any = false;
            for (k, c) in r.steps.iter_mut().enumerate() {
                if c.step.brings_out() != brings_out {
                    continue;
                }
                any = true;
                let (what, why) = describe(&c.step.scaled(c.amount * 2.0));
                ui.push_id(k, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if matches!(c.step, Step::StripSilence(_)) {
                            ui.add_space(28.0);
                        } else if toggle(ui, p, &mut c.on).changed() {
                            toggled = true;
                        }
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let ink = if c.on { p.text } else { p.weak };
                            ui.label(RichText::new(what).color(ink));
                            ui.label(RichText::new(why).size(10.5).color(p.weak));
                        });
                        if c.on && c.step.adjustable() {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().slider_width = 64.0;
                                    let hint = trf!(
                                        "How much of it: {percent} % (50 % is what is proposed)",
                                        percent = format!("{:.0}", c.amount * 100.0)
                                    );
                                    let slider = egui::Slider::new(&mut c.amount, 0.0..=1.0)
                                        .show_value(false);
                                    let response = ui.add(slider).on_hover_text(hint);
                                    if response.changed() {
                                        toggled = true;
                                    }
                                    if response.double_clicked() {
                                        c.amount = 0.5;
                                        toggled = true;
                                    }
                                },
                            );
                        }
                    });
                });
            }
            if !any {
                ui.label(
                    RichText::new(tr("Nothing to do here: it is fine as it is.")).color(p.weak),
                );
            }
        });
    toggled
}

/// The response (dB) at `freq` of what the ticked steps set: the strip's filters and EQ, and
/// the cuts of the Strada-EQ at their deepest.
struct Curve {
    strip: audiostrada_dsp::channel::Channel,
    eq: Vec<f32>,
}

impl Curve {
    fn new(r: &Ready) -> Self {
        use audiostrada_dsp::Effect;
        let mut strip = audiostrada_dsp::channel::Channel::default();
        strip.prepare(48_000, 64);
        let info = &audiostrada_dsp::channel::INFO;
        for (id, value) in rules::strip_settings(chosen(r).iter()) {
            if let Some(i) = info.params.iter().position(|p| p.id == id) {
                strip.set_param(i, value);
            }
        }
        // Dynamic cuts drawn as deep as they go.
        let deepest: Vec<Step> = chosen(r)
            .iter()
            .filter_map(|s| match s {
                Step::Resonance(c) => Some(Step::Resonance(Cut {
                    threshold: None,
                    ..*c
                })),
                Step::Plosives(c) => Some(Step::Plosives(Cut {
                    threshold: None,
                    ..*c
                })),
                _ => None,
            })
            .collect();
        let info = &audiostrada_dsp::eq::INFO;
        let mut eq: Vec<f32> = info.params.iter().map(|p| p.default).collect();
        for (effect, params) in rules::inserts(deepest.iter()) {
            if effect != "eq" {
                continue;
            }
            for (id, value) in params {
                if let Some(i) = info.params.iter().position(|p| p.id == id) {
                    eq[i] = value;
                }
            }
        }
        Self { strip, eq }
    }

    fn at(&self, freq: f32) -> f32 {
        (self.strip.eq_response_db(f64::from(freq))
            + audiostrada_dsp::eq::response_db(&self.eq, &[], f64::from(freq), 48_000.0))
            as f32
    }
}

/// The measured spectrum and what the proposal makes of it, with its moves marked.
fn spectrum(ui: &mut Ui, p: &Palette, r: &Ready) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 220.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        radius(p),
        field_fill(p),
        Stroke::new(1.0, line_edge(p)),
        StrokeKind::Inside,
    );
    let plot = Rect::from_min_max(rect.min + vec2(1.0, 22.0), rect.max - vec2(1.0, 18.0));
    let x = |f: f32| plot.left() + plot.width() * ((f / 20.0).log10() / 3.0).clamp(0.0, 1.0);
    let curve = &r.analysis.report.curve;
    let picked = chosen(r);
    let response = Curve::new(r);
    let after: Vec<(f32, f32)> = curve
        .iter()
        .map(|&(f, db)| (f, db + response.at(f)))
        .collect();
    let top = curve
        .iter()
        .chain(&after)
        .map(|c| c.1)
        .fold(f32::MIN, f32::max)
        + 4.0;
    const SPAN: f32 = 60.0;
    let y = |db: f32| plot.top() + (top - db).clamp(0.0, SPAN) / SPAN * plot.height();
    let grid = if p.dark {
        Color32::from_white_alpha(14)
    } else {
        Color32::from_black_alpha(22)
    };
    for f in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
        painter.line_segment(
            [pos2(x(f), plot.top()), pos2(x(f), plot.bottom())],
            Stroke::new(1.0, grid),
        );
        painter.text(
            pos2(x(f), rect.bottom() - 9.0),
            Align2::CENTER_CENTER,
            hz(f).replace(" Hz", "").replace(" kHz", "k"),
            FontId::proportional(9.5),
            p.weak,
        );
    }
    // What the filters take away.
    for step in &picked {
        if let Step::Filters { hpf, lpf, .. } = step {
            let shade = CUT.gamma_multiply(0.08);
            if let Some(h) = hpf {
                painter.rect_filled(plot.with_max_x(x(*h)), 0.0, shade);
            }
            if let Some(l) = lpf {
                painter.rect_filled(plot.with_min_x(x(*l)), 0.0, shade);
            }
        }
    }
    // The measured spectrum, filled; then the proposal over it.
    let measured: Vec<egui::Pos2> = curve.iter().map(|&(f, db)| pos2(x(f), y(db))).collect();
    for pair in measured.windows(2) {
        painter.add(egui::Shape::convex_polygon(
            vec![
                pair[0],
                pair[1],
                pos2(pair[1].x, plot.bottom()),
                pos2(pair[0].x, plot.bottom()),
            ],
            p.weak.gamma_multiply(0.16),
            Stroke::NONE,
        ));
    }
    painter.add(egui::Shape::line(
        measured,
        Stroke::new(1.2, p.weak.gamma_multiply(0.9)),
    ));
    let line: Vec<egui::Pos2> = after.iter().map(|&(f, db)| pos2(x(f), y(db))).collect();
    painter.add(egui::Shape::line(
        line.clone(),
        Stroke::new(5.0, p.accent.gamma_multiply(0.18)),
    ));
    painter.add(egui::Shape::line(line, Stroke::new(1.8, p.accent)));
    let level = |f: f32| {
        after
            .iter()
            .min_by(|a, b| (a.0 / f).log2().abs().total_cmp(&(b.0 / f).log2().abs()))
            .map_or(plot.center().y, |c| y(c.1))
    };
    for step in &picked {
        match step {
            Step::Resonance(cut) | Step::Plosives(cut) => {
                let c = pos2(x(cut.freq), plot.top() + 5.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        c + vec2(-4.5, -4.0),
                        c + vec2(4.5, -4.0),
                        c + vec2(0.0, 4.0),
                    ],
                    CUT,
                    Stroke::NONE,
                ));
                painter.line_segment(
                    [c + vec2(0.0, 4.0), pos2(c.x, level(cut.freq))],
                    Stroke::new(1.0, CUT.gamma_multiply(0.45)),
                );
            }
            Step::Eq {
                freq,
                gain,
                purpose,
                ..
            } => {
                let c = pos2(x(*freq), level(*freq));
                painter.circle(c, 4.5, p.accent, Stroke::new(1.5, field_fill(p)));
                let above = *gain >= 0.0;
                painter.text(
                    c + vec2(0.0, if above { -11.0 } else { 12.0 }),
                    Align2::CENTER_CENTER,
                    tr(purpose),
                    FontId::proportional(10.0),
                    p.text,
                );
            }
            _ => {}
        }
    }
    // The legend.
    let mut at = pos2(rect.left() + 12.0, rect.top() + 12.0);
    for (text, color) in [(tr("Measured"), p.weak), (tr("Prepared"), p.accent)] {
        painter.line_segment([at, at + vec2(14.0, 0.0)], Stroke::new(2.0, color));
        let r = painter.text(
            at + vec2(19.0, 0.0),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(10.5),
            p.weak,
        );
        at.x = r.right() + 16.0;
    }
}

/// The tracks a menu of `track` acts on: the selection when it holds it, else that track.
fn targets(doc: &Document, track: TrackId) -> Vec<TrackId> {
    if doc.selected_tracks().contains(&track) {
        doc.selected_tracks().to_vec()
    } else {
        vec![track]
    }
}

fn menu_items(ui: &mut Ui, doc: &mut Document, tracks: &[TrackId]) {
    for (mode, label, hint) in [
        (
            Mode::Clean,
            tr("Clean up…"),
            tr("Useless lows, resonances, plosives and sibilance, bleed"),
        ),
        (
            Mode::BringOut,
            tr("Bring out…"),
            tr("The instrument's colour, an even level"),
        ),
        (Mode::Both, tr("Both…"), tr("Clean up, then bring out")),
    ] {
        if ui.button(label).on_hover_text(hint).clicked() {
            request(ui.ctx(), tracks, mode);
            ui.close();
        }
    }
    let project = doc.project();
    let prepared = tracks
        .iter()
        .any(|t| project.track(*t).is_some_and(|t| t.prepared.is_some()));
    ui.separator();
    if ui
        .add_enabled(prepared, egui::Button::new(tr("Remove the preparation")))
        .clicked()
    {
        let tracks = tracks.to_vec();
        doc.history_mut().edit(tr("Remove the preparation"), |p| {
            for t in tracks {
                write::unprepare(p, t);
            }
        });
        ui.close();
    }
}

/// The preparation's submenu, in a track's menu.
pub fn menu(ui: &mut Ui, doc: &mut Document, tracks: &[TrackId]) {
    ui.menu_button(tr("Prepare the track"), |ui| menu_items(ui, doc, tracks));
}

/// The wand of a mixer strip: lit once the track is prepared; a click opens the menu.
pub fn wand(ui: &mut Ui, doc: &mut Document, track: TrackId) {
    let p = theme::current(ui);
    let prepared = doc.project().track(track).and_then(|t| t.prepared.clone());
    let (rect, response) = ui.allocate_exact_size(vec2(20.0, 13.0), Sense::click());
    let lit = prepared.is_some();
    // Always tinted, to be found; full once the track is prepared.
    let fill = p.accent.gamma_multiply(match (lit, response.hovered()) {
        (true, _) => 0.9,
        (false, true) => 0.55,
        (false, false) => 0.3,
    });
    ui.painter().rect_filled(rect, 3.0, fill);
    let ink = if lit || response.hovered() {
        Color32::WHITE
    } else {
        p.text
    };
    let rect = Rect::from_center_size(rect.center(), vec2(13.0, 13.0));
    let painter = ui.painter();
    let stroke = Stroke::new(1.2, ink);
    let (a, b) = (
        rect.left_bottom() + vec2(2.0, -2.0),
        rect.center() + vec2(1.5, -1.5),
    );
    painter.line_segment([a, b], stroke);
    for (c, r) in [
        (vec2(4.0, -4.0), 2.0),
        (vec2(-1.0, -4.5), 1.2),
        (vec2(4.5, 1.0), 1.2),
    ] {
        let c = rect.center() + c;
        painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
    }
    let hint = match &prepared {
        Some(done) => {
            let instrument = Instrument::from_id(&done.instrument).unwrap_or(Instrument::Other);
            let what = match (done.cleaned, done.brought_out) {
                (true, true) => tr("cleaned up and brought out"),
                (true, false) => tr("cleaned up"),
                _ => tr("brought out"),
            };
            trf!(
                "Prepared as: {instrument}, {what}. Click to redo it, right-click to remove it",
                instrument = tr(instrument.name()),
                what = what
            )
        }
        None => {
            tr("Prepare the track: clean it up, bring it out (right-click for more)").to_owned()
        }
    };
    let response = response
        .on_hover_text(hint)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let tracks = targets(doc, track);
    if response.clicked() {
        request(ui.ctx(), &tracks, Mode::Both);
    }
    egui::Popup::context_menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let name = doc
                .project()
                .track(track)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            let title = if tracks.len() > 1 {
                trf!("Prepare {count} tracks", count = tracks.len())
            } else {
                trf!("Prepare “{name}”", name = name)
            };
            ui.label(RichText::new(title).small().weak());
            menu_items(ui, doc, &tracks);
        });
}

/// The mixer's button preparing every track at once (not the buses nor the VCAs).
pub fn all_button(ui: &mut Ui, doc: &Document) {
    let p = theme::current(ui);
    let button = egui::Button::new(RichText::new(tr("Prepare all")).strong())
        .fill(theme::mix(p.surface, p.accent, 0.5));
    if ui
        .add(button)
        .on_hover_text(tr(
            "Cleans up and brings out every track (not the buses): each one is listened to, \
             then you hear the proposal and leave out the tracks you want",
        ))
        .clicked()
    {
        let tracks: Vec<TrackId> = doc
            .project()
            .tracks
            .iter()
            .filter(|t| t.kind == audiostrada_core::TrackKind::Audio)
            .map(|t| t.id)
            .collect();
        request(ui.ctx(), &tracks, Mode::Both);
    }
}
