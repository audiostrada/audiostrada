//! Express Mix: each track chosen is rendered alone and measured, its instrument
//! guessed, then the window proposes what Clean up and Bring out would set (see
//! `audiostrada_dsp::prepare`). While it is open the proposal is heard, against the track as
//! it was and at the same level; Apply writes it in one undo step.

use audiostrada_core::automation::AutomationLane;
use audiostrada_core::console::{Console, ConsoleModel};
use audiostrada_core::groups::{self, Link};
use audiostrada_core::prepare::{self as write, Added, Preparation};
use audiostrada_core::{Insert, Prepared, Project, TrackId, TrackSend};
use audiostrada_dsp::prepare::{
    self as rules, Analysis, Character, Cut, Flavor, Instrument, Step, Strength,
};
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};

use crate::document::{Document, Measuring};
use crate::effect_frame::{
    Face, Glyph, bar_fill, button, field_fill, glyph_button, line_edge, paint_bottom_band,
    paint_face, paint_glyph, paint_title, paint_top_band, radius, text_button,
};
use crate::i18n::tr;
use crate::settings::WARN;
use crate::theme::{self, Palette};
use crate::trf;

const WIDTH: f32 = 1280.0;
/// Width of the track list.
const LIST: f32 = 320.0;
/// Color of what the cleaning removes.
const CUT: Color32 = Color32::from_rgb(0xe3, 0xa2, 0x3b);
/// Color of the space.
const SPACE: Color32 = Color32::from_rgb(0x4f, 0xb3, 0xc8);
/// The mix's peak kept under this (dBFS), room for the master's processing.
const HEADROOM: f32 = -6.0;
/// Color of the spread.
const WIDE: Color32 = Color32::from_rgb(0x9b, 0x7f, 0xd9);

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
    /// Nothing told it: to be chosen.
    Unknown,
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
    /// Its own console color, else the one set for every track.
    console: Option<Console>,
}

/// What a bus holds.
struct Bus {
    /// Its audio tracks, at every depth.
    members: Vec<TrackId>,
    /// Of them, those prepared already or with it.
    prepared: usize,
}

impl Bus {
    /// The strength for the bus: lighter as more of its tracks are prepared.
    fn lighter(&self, s: Strength) -> Strength {
        let share = self.prepared as f32 / self.members.len().max(1) as f32;
        Strength(s.0 * (1.0 - 0.4 * share))
    }
}

/// The doses of the parts and the character asked, for every track.
type Doses = (Strength, Strength, Flavor);

/// What a track is given: its preparation and its send (dB) to the space bus.
type Planned = (TrackId, Preparation, Option<f32>);

/// The buses proposed: their group's key and their tracks.
type Groups = Vec<(&'static str, Vec<TrackId>)>;

/// What the preview changes on a track, as it was when the window opened.
struct Saved {
    inserts: Vec<Insert>,
    automation: Vec<AutomationLane>,
    prepared: Option<Prepared>,
    pan: f32,
    console: Option<Console>,
    sends: Vec<TrackSend>,
    parent: Option<TrackId>,
    master_send: bool,
    trim: f32,
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
    /// A bus (or a folder): prepared as a sum, not as an instrument.
    bus: Option<Bus>,
    /// A bus the window made to group tracks: its group's key.
    group: Option<&'static str>,
    /// Its proposal was written to the project, to be heard (and measured by its bus).
    heard: bool,
    /// Gain (dB) keeping the loudness the track had, for the settings in `level_of`.
    level: f32,
    level_of: Settings,
    /// That gain being computed on its own thread, for these settings.
    leveling: Option<(Settings, std::thread::JoinHandle<f32>)>,
}

/// What the right side shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum View {
    /// Several tracks: their instruments checked, and the grouping chosen, first.
    Check,
    Overview,
    Track(usize),
}

pub struct PrepareWindow {
    mode: Option<Mode>,
    rows: Vec<Row>,
    view: View,
    /// How far each part goes, for every track.
    clean: Strength,
    bring: Strength,
    /// The shared reverb, 0 (off) to 1.
    space: f32,
    /// The spread of pairs, 0 (off) to 1.
    wide: f32,
    /// The character asked of the mix.
    flavor: Flavor,
    /// The project's console color, set by the window (the tracks with their own keep theirs).
    console: Console,
    /// The project's console color and switch when the window opened.
    project_console: (Console, bool),
    /// Tracks grouped into buses by family, in this order of the groups.
    grouping: bool,
    group_order: Vec<&'static str>,
    /// The buses made for the preview, and for which groups.
    made: Vec<(&'static str, TrackId)>,
    made_for: Groups,
    /// The tracks' order when the window opened.
    structure: Vec<TrackId>,
    /// Room kept before the master's processing when the mix peaks too high, given back after.
    headroom: bool,
    /// The master's effects and room when the window opened.
    master_saved: (Vec<Insert>, Option<audiostrada_core::model::Headroom>),
    /// The master follows the mix measured or the switch.
    master_dirty: bool,
    /// The mix being rendered to measure its peak, and that peak (dBFS) once known.
    mix: Option<Box<Measuring>>,
    mix_peak: Option<f32>,
    /// The proposal heard changed since the mix was measured.
    mix_stale: bool,
    /// The project had a space bus when the window opened.
    space_existed: bool,
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
            view: View::Overview,
            clean: Strength::NORMAL,
            bring: Strength::NORMAL,
            space: 0.0,
            wide: 0.0,
            flavor: Flavor {
                character: Character::Natural,
                amount: 0.5,
            },
            console: Console::default(),
            project_console: (Console::default(), false),
            grouping: false,
            group_order: rules::GROUPS.iter().map(|g| g.0).collect(),
            made: Vec::new(),
            made_for: Vec::new(),
            structure: Vec::new(),
            headroom: true,
            master_saved: (Vec::new(), None),
            master_dirty: false,
            mix: None,
            mix_peak: None,
            mix_stale: true,
            space_existed: false,
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

/// The family a bus holds: as prepared before, else what most of its tracks were prepared as,
/// else its name, else what most of its tracks' names say.
fn guess_bus(doc: &Document, track: TrackId, members: &[TrackId]) -> (Instrument, Guess) {
    let project = doc.project();
    let Some(t) = project.track(track) else {
        return (Instrument::Other, Guess::Unknown);
    };
    if let Some(i) = t
        .prepared
        .as_ref()
        .and_then(|p| Instrument::from_id(&p.instrument))
    {
        return (rules::bus_family(i), Guess::Chosen);
    }
    let most = |found: Vec<Instrument>| {
        rules::BUSES
            .into_iter()
            .map(|b| {
                (
                    b,
                    found.iter().filter(|i| rules::bus_family(**i) == b).count(),
                )
            })
            .filter(|(_, n)| *n > 0)
            .max_by_key(|(_, n)| *n)
            .map(|(b, _)| b)
    };
    let tracks = || members.iter().filter_map(|m| project.track(*m));
    let prepared = tracks()
        .filter_map(|m| {
            m.prepared
                .as_ref()
                .and_then(|p| Instrument::from_id(&p.instrument))
        })
        .collect();
    if let Some(b) = most(prepared) {
        return (b, Guess::Instrument);
    }
    if let Some(i) = Instrument::from_name(&t.name, None) {
        return (rules::bus_family(i), Guess::Name);
    }
    let named = tracks()
        .filter_map(|m| Instrument::from_name(&m.name, None))
        .collect();
    match most(named) {
        Some(b) => (b, Guess::Instrument),
        None => (Instrument::Other, Guess::Unknown),
    }
}

/// The instrument of a track: as prepared before, else its name, its instrument, its sound.
fn guess(doc: &Document, track: TrackId, analysis: &Analysis) -> (Instrument, Guess) {
    let project = doc.project();
    let Some(t) = project.track(track) else {
        return (Instrument::Other, Guess::Unknown);
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
    (Instrument::Other, Guess::Unknown)
}

fn propose(
    analysis: &Analysis,
    instrument: Instrument,
    mode: Mode,
    (clean, bring, flavor): Doses,
    bus: Option<&Bus>,
) -> Vec<Choice> {
    let mut steps = Vec::new();
    match bus {
        Some(bus) => {
            if mode.cleans() {
                steps.extend(rules::clean_bus(analysis, instrument));
            }
            if mode.brings_out() {
                steps.extend(rules::bring_out_bus(
                    analysis,
                    instrument,
                    bus.lighter(bring),
                    flavor,
                ));
            }
        }
        None => {
            if mode.cleans() {
                steps.extend(rules::clean(analysis, instrument, clean));
            }
            if mode.brings_out() {
                steps.extend(rules::bring_out(analysis, instrument, bring, flavor));
            }
        }
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

/// Keeps in `new` what was ticked and dosed by hand in `old`, step by step of the same kind.
fn carry(old: &[Choice], new: &mut [Choice]) {
    let kind = |c: &Choice| std::mem::discriminant(&c.step);
    let rank = |list: &[Choice], k: usize| {
        list[..k]
            .iter()
            .filter(|c| kind(c) == kind(&list[k]))
            .count()
    };
    for k in 0..new.len() {
        let n = rank(new, k);
        if let Some(o) =
            (0..old.len()).find(|&j| kind(&old[j]) == kind(&new[k]) && rank(old, j) == n)
        {
            new[k].on = old[o].on;
            new[k].amount = old[o].amount;
        }
    }
}

/// The steps kept, each at its amount.
fn chosen(r: &Ready) -> Vec<Step> {
    r.steps
        .iter()
        .filter(|c| c.on)
        .map(|c| c.step.scaled(c.amount * 2.0))
        .collect()
}

/// What a track's ticked steps write, for a track played `shift` dB louder (its input gain
/// raised); `gain` keeps the level on the strip's output.
fn preparation(
    r: &Ready,
    mode: Mode,
    rate: u32,
    (after_first, shift): (bool, f32),
    gain: f32,
) -> Preparation {
    let picked: Vec<Step> = chosen(r).iter().map(|s| s.shifted(shift)).collect();
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
    let mut inserts = Vec::new();
    for out in [false, true] {
        let part = picked.iter().filter(|s| s.brings_out() == out);
        inserts.extend(
            rules::inserts(part)
                .into_iter()
                .map(|(effect, params)| Added {
                    effect: effect.to_owned(),
                    params,
                    gain_db: 0.0,
                    out,
                }),
        );
    }
    Preparation {
        instrument: r.instrument.id().to_owned(),
        clean: mode.cleans(),
        bring_out: mode.brings_out(),
        strip: rules::strip_settings(picked.iter()),
        strip_gain: gain,
        inserts,
        ride,
        after_first,
        ..Default::default()
    }
}

/// What the preview may change on `t`, as it is.
fn saved(t: &audiostrada_core::Track) -> Saved {
    Saved {
        inserts: t.inserts.clone(),
        automation: t.automation.clone(),
        prepared: t.prepared.clone(),
        pan: t.pan,
        console: t.console,
        sends: t.sends.clone(),
        parent: t.parent,
        master_send: t.master_send,
        trim: t.trim_db,
    }
}

/// Puts a track back as it was when the window opened.
fn put_back(p: &mut Project, row: &Row) {
    if let Some(t) = p.track_mut(row.track) {
        t.inserts = row.saved.inserts.clone();
        t.automation = row.saved.automation.clone();
        t.prepared = row.saved.prepared.clone();
        t.pan = row.saved.pan;
        t.console = row.saved.console;
        t.sends = row.saved.sends.clone();
        t.trim_db = row.saved.trim;
    }
}

/// Writes the preparations, each with its send (dB) to the space bus, made if needed, and the
/// project's console color.
fn write_all(p: &mut Project, plan: &[Planned], bus_name: &str, console: Option<Console>) {
    if let Some(console) = console {
        p.console = console;
        if console.colors() {
            p.console_off = false;
        }
    }
    let bus = plan
        .iter()
        .any(|x| x.2.is_some())
        .then(|| write::ensure_space_bus(p, bus_name, &rules::SPACE_REVERB));
    for (track, prep, send) in plan {
        let mut prep = prep.clone();
        prep.space = bus.zip(*send);
        write::prepare(p, *track, &prep);
    }
    write::tidy_space(p);
}

/// Makes the buses of `groups`; their keys and ids.
fn make(
    p: &mut Project,
    groups: &Groups,
    instruments: &[(TrackId, Instrument)],
) -> Vec<(&'static str, TrackId)> {
    let list: Vec<write::Group> = groups
        .iter()
        .map(|(key, tracks)| {
            let family = tracks
                .first()
                .and_then(|t| instruments.iter().find(|i| i.0 == *t))
                .map_or(Instrument::Other, |i| rules::bus_family(i.1));
            let name = rules::GROUPS
                .iter()
                .find(|g| g.0 == *key)
                .map_or("", |g| tr(g.1));
            write::Group {
                family: family.id().to_owned(),
                name: name.to_owned(),
                tracks: tracks.clone(),
            }
        })
        .collect();
    let buses = write::make_groups(p, &list);
    groups.iter().map(|g| g.0).zip(buses).collect()
}

/// Takes the buses made back out: the tracks back where they were.
fn unmake(p: &mut Project, made: &[(&'static str, TrackId)], rows: &[Row], structure: &[TrackId]) {
    if made.is_empty() {
        return;
    }
    for (_, bus) in made {
        p.remove_track(*bus);
    }
    for row in rows.iter().filter(|r| r.group.is_none()) {
        if let Some(t) = p.track_mut(row.track) {
            t.parent = row.saved.parent;
            t.master_send = row.saved.master_send;
        }
    }
    p.tracks.sort_by_key(|t| {
        structure
            .iter()
            .position(|id| *id == t.id)
            .unwrap_or(usize::MAX)
    });
    audiostrada_core::routing::normalize_order(p);
}

/// The key of [`Kept`] in `Project::extensions`.
const SETTINGS: &str = "express_mix";

/// The window's settings last applied, saved with the project to open with them again.
#[derive(serde::Serialize, serde::Deserialize)]
struct Kept {
    clean: f32,
    bring: f32,
    space: f32,
    wide: f32,
    style: usize,
    intensity: f32,
}

impl PrepareWindow {
    fn kept(&self) -> Kept {
        Kept {
            clean: self.clean.0,
            bring: self.bring.0,
            space: self.space,
            wide: self.wide,
            style: Character::STYLES
                .iter()
                .position(|c| *c == self.flavor.character)
                .unwrap_or(0),
            intensity: self.flavor.amount,
        }
    }

    fn take(&mut self, kept: &Kept) {
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        self.clean = Strength(unit(kept.clean));
        self.bring = Strength(unit(kept.bring));
        self.space = unit(kept.space);
        self.wide = unit(kept.wide);
        self.flavor = Flavor {
            character: Character::STYLES
                .get(kept.style)
                .copied()
                .unwrap_or_default(),
            amount: unit(kept.intensity),
        };
    }

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
        self.space_existed = write::space_bus(doc.project()).is_some();
        self.structure = doc.project().tracks.iter().map(|t| t.id).collect();
        self.made.clear();
        self.made_for.clear();
        self.mix = None;
        self.mix_peak = None;
        self.mix_stale = true;
        let project = doc.project();
        self.master_saved = (
            project.master.inserts.clone(),
            project.master.headroom.clone(),
        );
        self.project_console = (project.console, project.console_off);
        self.console = project.console;
        if let Some(kept) = project
            .extensions
            .get(SETTINGS)
            .and_then(|v| serde_json::from_value::<Kept>(v.clone()).ok())
        {
            self.take(&kept);
        }
        // Prepared before, their instruments are known: no need to check them again.
        let prepared_before = tracks.iter().all(|t| {
            doc.project()
                .track(*t)
                .and_then(|t| t.prepared.as_ref())
                .is_some_and(|p| p.cleaned || p.brought_out)
        });
        self.view = match (tracks.len(), prepared_before) {
            (1, _) => View::Track(0),
            (_, true) => View::Overview,
            _ => View::Check,
        };
        self.after = true;
        self.rows = tracks
            .iter()
            .copied()
            .filter_map(|track| {
                let t = doc.project().track(track)?;
                if t.kind == audiostrada_core::TrackKind::Vca
                    || t.prepared
                        .as_ref()
                        .is_some_and(|p| p.instrument == write::SPACE)
                {
                    return None;
                }
                let (name, color) = (t.name.clone(), theme::track_color(t.color));
                let project = doc.project();
                let bus = (t.kind == audiostrada_core::TrackKind::Bus
                    || audiostrada_core::routing::has_children(project, track))
                .then(|| {
                    let members: Vec<TrackId> = audiostrada_core::routing::subtree(project, track)
                        .into_iter()
                        .filter(|m| {
                            *m != track
                                && project
                                    .track(*m)
                                    .is_some_and(|m| m.kind == audiostrada_core::TrackKind::Audio)
                        })
                        .collect();
                    let prepared = members
                        .iter()
                        .filter(|m| {
                            tracks.contains(m)
                                || project
                                    .track(**m)
                                    .and_then(|m| m.prepared.as_ref())
                                    .is_some_and(|p| p.cleaned || p.brought_out)
                        })
                        .count();
                    Bus { members, prepared }
                });
                let saved = saved(t);
                Some(Row {
                    track,
                    name,
                    color,
                    saved,
                    state: State::Queued,
                    included: true,
                    bus,
                    group: None,
                    heard: false,
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
                        let (instrument, guess) = match &row.bus {
                            Some(bus) => guess_bus(doc, row.track, &bus.members),
                            None => guess(doc, row.track, &analysis),
                        };
                        let steps = propose(
                            &analysis,
                            instrument,
                            mode,
                            (self.clean, self.bring, self.flavor),
                            row.bus.as_ref(),
                        );
                        row.heard = false;
                        self.dirty = true;
                        State::Ready(Box::new(Ready {
                            analysis,
                            instrument,
                            guess,
                            steps,
                            console: None,
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
        // A bus is measured once the tracks it holds here are heard as proposed.
        let waiting: Vec<bool> = self
            .rows
            .iter()
            .map(|row| {
                row.bus.as_ref().is_some_and(|bus| {
                    self.rows.iter().any(|m| {
                        bus.members.contains(&m.track)
                            && m.included
                            && !m.heard
                            && !matches!(m.state, State::Failed(_))
                    })
                }) && self.after
            })
            .collect();
        for (row, waiting) in self.rows.iter_mut().zip(waiting) {
            if busy >= AT_ONCE {
                break;
            }
            if matches!(row.state, State::Queued) && !waiting {
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
        let shifts: Vec<f32> = self.rows.iter().map(|row| self.staged(row).1).collect();
        for (row, shift) in self.rows.iter_mut().zip(shifts) {
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
                let picked: Vec<Step> = chosen(r).iter().map(|s| s.shifted(shift)).collect();
                let strip = rules::strip_settings(picked.iter());
                let inserts = rules::inserts(picked.iter());
                let mut settings = strip.clone();
                settings.push(("trim".to_owned(), shift));
                for (effect, params) in &inserts {
                    settings.extend(params.iter().map(|(k, v)| (format!("{effect}/{k}"), *v)));
                }
                if settings != row.level_of {
                    let (excerpt, rate) = (r.analysis.excerpt.clone(), r.analysis.rate);
                    let handle = std::thread::spawn(move || {
                        rules::level_kept(&excerpt, rate, shift, &strip, &inserts)
                    });
                    row.leveling = Some((settings, handle));
                }
            }
            busy |= row.leveling.is_some();
        }
        busy
    }

    /// The input gain a track gets, and how much louder than when measured it then plays.
    /// Buses keep theirs.
    fn staged(&self, row: &Row) -> (Option<f32>, f32) {
        let State::Ready(r) = &row.state else {
            return (None, 0.0);
        };
        if row.bus.is_some() {
            return (None, 0.0);
        }
        // Measured at the input gain it had before a preparation (see `start_source_render`).
        let now = row.saved.trim;
        let measured = row
            .saved
            .prepared
            .as_ref()
            .and_then(|p| p.trim)
            .unwrap_or(now);
        let trim = rules::staging(&r.analysis).map_or(now, |d| (measured + d).clamp(-24.0, 24.0));
        (Some(trim), trim - measured)
    }

    /// How much room is kept on the master, once the mix is measured.
    fn lowered(&self) -> Option<f32> {
        let peak = self.mix_peak?;
        (self.headroom && peak > HEADROOM).then(|| ((peak - HEADROOM) * 10.0).ceil() / 10.0)
    }

    /// Measures the mix heard, in the background, whenever the proposal changed and every
    /// track is ready; true while it is measured.
    fn measure_mix(&mut self, doc: &mut Document) -> bool {
        if let Some(m) = &mut self.mix {
            match m.poll() {
                None => return true,
                Some(Ok((left, right, _))) => {
                    let peak = left
                        .iter()
                        .chain(&right)
                        .fold(0.0f32, |m, x| m.max(x.abs()));
                    self.mix_peak = Some(20.0 * peak.max(1e-6).log10());
                    self.master_dirty = true;
                }
                Some(Err(_)) => self.mix_peak = None,
            }
            self.mix = None;
        }
        let ready = self
            .rows
            .iter()
            .all(|r| matches!(r.state, State::Ready(_) | State::Failed(_)));
        if self.headroom && self.mix_stale && ready && self.after && !self.dirty {
            self.mix_stale = false;
            self.mix = doc.start_mix_render().ok().map(Box::new);
        }
        self.mix.is_some()
    }

    /// Puts the tracks back as they were when the window opened, outside the undo history.
    fn restore(&mut self, doc: &mut Document) {
        let space_existed = self.space_existed;
        let (console, off) = self.project_console;
        let (made, rows, structure) = (&self.made, &self.rows, &self.structure);
        let master = self.master_saved.clone();
        doc.history_mut().modify_unrecorded(|p| {
            p.console = console;
            p.console_off = off;
            (p.master.inserts, p.master.headroom) = master;
            unmake(p, made, rows, structure);
            for row in rows.iter().filter(|r| r.group.is_none()) {
                put_back(p, row);
            }
            if !space_existed {
                write::tidy_space(p);
            }
        });
        self.made.clear();
        self.made_for.clear();
    }

    /// The console color a track gets of its own, if any.
    fn console_of(&self, r: &Ready) -> Option<Console> {
        r.console
    }

    /// The instruments of the tracks measured.
    fn instruments(&self) -> Vec<(TrackId, Instrument)> {
        self.rows
            .iter()
            .filter_map(|row| match &row.state {
                State::Ready(r) => Some((row.track, r.instrument)),
                _ => None,
            })
            .collect()
    }

    /// The buses proposed: the groups of at least two tracks kept that are in no bus yet, in
    /// the order set, each with its tracks in the usual order of their instruments.
    fn groups(&self) -> Groups {
        if !self.grouping {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &key in &self.group_order {
            let mut tracks: Vec<(usize, TrackId)> = self
                .rows
                .iter()
                .filter_map(|row| {
                    let State::Ready(r) = &row.state else {
                        return None;
                    };
                    let place = Instrument::ALL.iter().position(|i| *i == r.instrument)?;
                    (row.included
                        && row.bus.is_none()
                        && row.saved.parent.is_none()
                        && rules::group(r.instrument) == Some(key))
                    .then_some((place, row.track))
                })
                .collect();
            if tracks.len() >= 2 {
                tracks.sort_by_key(|t| t.0);
                out.push((key, tracks.into_iter().map(|t| t.1).collect()));
            }
        }
        out
    }

    /// Keeps a row for each bus made, measured once its tracks are heard: a new row, or the
    /// same one with the bus's new id (measured again if it holds other tracks).
    fn sync_groups(&mut self, doc: &Document) {
        let made = self.made.clone();
        let shown = match self.view {
            View::Track(k) => self.rows.get(k).map(|r| r.track),
            View::Overview | View::Check => None,
        };
        self.rows
            .retain(|r| r.group.is_none_or(|g| made.iter().any(|m| m.0 == g)));
        for (key, bus) in made {
            let members = self
                .made_for
                .iter()
                .find(|g| g.0 == key)
                .map(|g| g.1.clone())
                .unwrap_or_default();
            if let Some(row) = self.rows.iter_mut().find(|r| r.group == Some(key)) {
                row.track = bus;
                if row.bus.as_ref().is_none_or(|b| b.members != members) {
                    row.state = State::Queued;
                    row.heard = false;
                    row.level_of.clear();
                }
                row.bus = Some(Bus {
                    prepared: members.len(),
                    members,
                });
                continue;
            }
            let Some(t) = doc.project().track(bus) else {
                continue;
            };
            self.rows.push(Row {
                track: bus,
                name: t.name.clone(),
                color: theme::track_color(t.color),
                saved: saved(t),
                state: State::Queued,
                included: true,
                bus: Some(Bus {
                    prepared: members.len(),
                    members,
                }),
                group: Some(key),
                heard: false,
                level: 0.0,
                level_of: Vec::new(),
                leveling: None,
            });
        }
        self.view = match shown.and_then(|t| self.rows.iter().position(|r| r.track == t)) {
            Some(k) => View::Track(k),
            None if shown.is_some() => View::Overview,
            None => self.view,
        };
    }

    /// What each track kept is given: its preparation and its send to the space bus. Bringing
    /// out also sets its console color, its send, and spreads the pairs left in the center.
    fn plan(&self, doc: &Document, mode: Mode) -> Vec<Planned> {
        let rate = doc.project().sample_rate;
        let kept: Vec<(&Row, &Ready)> = self
            .rows
            .iter()
            .filter_map(|row| match &row.state {
                State::Ready(r) if row.included => Some((row, &**r)),
                _ => None,
            })
            .collect();
        let pans = if mode.brings_out() {
            let tracks: Vec<(Instrument, &str)> = kept
                .iter()
                .map(|(row, r)| {
                    let centered = row
                        .saved
                        .prepared
                        .as_ref()
                        .and_then(|p| p.pan)
                        .unwrap_or(row.saved.pan)
                        .abs()
                        < 0.005;
                    // Panned by hand, or a bus: left alone.
                    let instrument = if centered && row.bus.is_none() {
                        r.instrument
                    } else {
                        Instrument::Other
                    };
                    (instrument, row.name.as_str())
                })
                .collect();
            rules::pans(&tracks, self.wide)
        } else {
            vec![None; kept.len()]
        };
        kept.iter()
            .zip(pans)
            .map(|((row, r), pan)| {
                let after_first = doc.is_instrument_track(row.track);
                let (trim, shift) = self.staged(row);
                // Until the level is measured again for a new input gain, that gain is taken
                // back at once: no jump in level meanwhile.
                let measured = row
                    .level_of
                    .iter()
                    .find(|(k, _)| k == "trim")
                    .map_or(0.0, |x| x.1);
                let gain = if self.keep_level {
                    row.level - (shift - measured)
                } else {
                    0.0
                };
                let mut prep = preparation(r, mode, rate, (after_first, shift), gain);
                prep.trim = trim;
                let mut send = None;
                if mode.brings_out() {
                    prep.pan = pan;
                    prep.console = self.console_of(r);
                    if row.bus.is_none() {
                        send = rules::space_send_db(r.instrument, self.space);
                    }
                }
                (row.track, prep, send)
            })
            .collect()
    }

    /// Lets the proposal be heard: written outside the undo history over what is already heard
    /// (the effects stay in place, their settings change), the buses made when the grouping
    /// changes, or taken back out. False while tracks are still being measured.
    fn preview(&mut self, doc: &mut Document, mode: Mode) -> bool {
        // Buses are measured after their tracks: those are heard first.
        if self
            .rows
            .iter()
            .any(|r| r.bus.is_none() && !matches!(r.state, State::Ready(_) | State::Failed(_)))
        {
            return false;
        }
        if !self.after {
            self.restore(doc);
            return true;
        }
        let groups = self.groups();
        let rebuild = groups != self.made_for;
        let instruments = self.instruments();
        let plan = self.plan(doc, mode);
        let console = mode.brings_out().then_some(self.console);
        let (rows, structure) = (&self.rows, &self.structure);
        let mut made = self.made.clone();
        doc.history_mut().modify_unrecorded(|p| {
            if rebuild {
                unmake(p, &made, rows, structure);
                made = make(p, &groups, &instruments);
            }
            for row in rows.iter().filter(|r| r.group.is_none()) {
                if !plan.iter().any(|x| x.0 == row.track) {
                    put_back(p, row);
                }
            }
            let plan: Vec<_> = plan
                .iter()
                .filter(|x| !rebuild || !rows.iter().any(|r| r.track == x.0 && r.group.is_some()))
                .cloned()
                .collect();
            write_all(p, &plan, tr("Space"), console);
        });
        for row in &mut self.rows {
            row.heard |= plan.iter().any(|x| x.0 == row.track);
        }
        if rebuild {
            self.made = made;
            self.made_for = groups;
            self.sync_groups(doc);
            self.dirty = true;
        }
        true
    }

    /// Writes every track's ticked steps, and the buses grouping them, in one undo step.
    fn apply(&mut self, doc: &mut Document, mode: Mode) {
        let groups = self.groups();
        let instruments = self.instruments();
        let plan = self.plan(doc, mode);
        let console = mode.brings_out().then_some(self.console);
        let lowered = self.lowered();
        let keys: Vec<(TrackId, &'static str)> = self
            .rows
            .iter()
            .filter_map(|r| r.group.map(|g| (r.track, g)))
            .collect();
        let kept = self.kept();
        self.restore(doc);
        doc.history_mut().edit(tr("Express Mix"), |p| {
            let made = make(p, &groups, &instruments);
            // The buses made again have new ids.
            let plan: Vec<_> = plan
                .into_iter()
                .filter_map(
                    |(track, prep, send)| match keys.iter().find(|k| k.0 == track) {
                        Some((_, key)) => {
                            made.iter().find(|m| m.0 == *key).map(|m| (m.1, prep, send))
                        }
                        None => Some((track, prep, send)),
                    },
                )
                .collect();
            write_all(p, &plan, tr("Space"), console);
            write::master_headroom(p, lowered);
            if let Ok(value) = serde_json::to_value(&kept) {
                p.extensions.insert(SETTINGS.to_owned(), value);
            }
        });
        self.mode = None;
        self.mix = None;
        self.rows.clear();
    }

    /// Closes without keeping anything.
    fn cancel(&mut self, doc: &mut Document) {
        self.mix = None;
        if self.mode.take().is_some() {
            self.restore(doc);
            self.rows.clear();
        }
    }

    /// Proposes again at the strengths set, keeping what was ticked and dosed by hand.
    fn repropose(&mut self, mode: Mode) {
        for row in &mut self.rows {
            if let State::Ready(r) = &mut row.state {
                let mut steps = propose(
                    &r.analysis,
                    r.instrument,
                    mode,
                    (self.clean, self.bring, self.flavor),
                    row.bus.as_ref(),
                );
                carry(&r.steps, &mut steps);
                r.steps = steps;
            }
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
            self.mix_stale = true;
            self.master_dirty = true;
        }
        let mixing = self.measure_mix(doc);
        if self.master_dirty && self.after {
            self.master_dirty = false;
            let db = self.lowered();
            doc.history_mut()
                .modify_unrecorded(|p| write::master_headroom(p, db));
        }
        let measuring = self
            .rows
            .iter()
            .any(|r| !matches!(r.state, State::Ready(_) | State::Failed(_)));
        if measuring || leveling || mixing || self.mix_stale {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if matches!(self.view, View::Track(k) if k >= self.rows.len()) {
            self.view = View::Overview;
        }

        let screen = ctx.content_rect();
        let width = WIDTH.min(screen.width() - 40.0).max(720.0);
        let height = (screen.height() - 200.0).clamp(560.0, 760.0);
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
            .default_pos(screen.center_top() + vec2(0.0, 50.0))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                self.header(ui, &p, mode, width, &mut asked);
                self.body(ui, &p, doc, mode, vec2(width, height));
                self.footer(ui, &p, playing, width, &mut asked);
            });
        if asked.apply {
            self.apply(doc, mode);
        } else if asked.close {
            self.cancel(doc);
        }
        asked.play
    }

    /// The top bar: the logo, what is being done and to which tracks, the close button.
    fn header(&mut self, ui: &mut Ui, p: &Palette, mode: Mode, width: f32, asked: &mut Asked) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 58.0), Sense::hover());
        let color = match self.view {
            View::Track(k) => self.rows.get(k).map_or(p.accent, |r| r.color),
            View::Overview | View::Check => p.accent,
        };
        let painter = ui.painter().clone();
        let bar = paint_top_band(&painter, p, rect, color);
        let (title, detail) = match self.rows.as_slice() {
            [one] => (
                tr("Express Mix").to_owned(),
                format!("{} · {}", mode.title(), one.name),
            ),
            rows => (
                trf!("Express Mix: {count} tracks", count = rows.len()),
                mode.title().to_owned(),
            ),
        };
        let weak = p.weak;
        paint_title(
            &painter,
            p,
            bar,
            width - 120.0,
            &title,
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

    /// The track list on the left, the overview or the track chosen on the right.
    fn body(&mut self, ui: &mut Ui, p: &Palette, doc: &mut Document, mode: Mode, size: egui::Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        if self.view == View::Check {
            let mut all = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(24.0, 16.0)))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            all.set_clip_rect(rect);
            self.check(&mut all, p, doc, mode);
            return;
        }
        let list = rect.with_max_x(rect.left() + LIST);
        let side = rect.with_min_x(list.right() + 1.0);
        ui.painter().rect_filled(list, 0.0, bar_fill(p));
        ui.painter().line_segment(
            [list.right_top(), list.right_bottom()],
            Stroke::new(1.0, line_edge(p)),
        );
        let column = |area: Rect| {
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::top_down(egui::Align::Min))
        };
        let mut left = ui.new_child(column(list.shrink(8.0)));
        left.set_clip_rect(list);
        self.list(&mut left, p, doc, mode);
        let mut right = ui.new_child(column(side.shrink2(vec2(22.0, 16.0))));
        right.set_clip_rect(side);
        match self.view {
            View::Overview | View::Check => {
                egui::ScrollArea::vertical()
                    .id_salt("overview")
                    .auto_shrink([false, false])
                    .show(&mut right, |ui| self.overview(ui, p, doc, mode));
            }
            View::Track(k) => self.track(&mut right, p, doc, mode, k),
        }
    }

    /// The first screen for several tracks, while they are listened to: each one's instrument
    /// as found, to correct, whether it is included, its mute and solo; then the grouping into
    /// buses. Continue leads to the overview, where all of it can still be changed.
    fn check(&mut self, ui: &mut Ui, p: &Palette, doc: &mut Document, mode: Mode) {
        ui.spacing_mut().item_spacing.y = 6.0;
        heading(
            ui,
            p,
            tr("Check the tracks"),
            tr(
                "Each instrument is found from the track's name: correct those that are wrong, \
                leave out those to keep as they are.",
            ),
            |ui| ui.label(""),
        );
        let strengths = (self.clean, self.bring, self.flavor);
        let footer = 132.0;
        let mut changed = false;
        egui::ScrollArea::vertical()
            .id_salt("check")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - footer).max(120.0))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (k, row) in self.rows.iter_mut().enumerate() {
                    if row.group.is_none() {
                        changed |= check_row(ui, p, doc, row, k, mode, strengths);
                    }
                }
            });
        ui.add_space(8.0);
        let mut grouping = self.grouping;
        heading(
            ui,
            p,
            tr("Group into buses"),
            tr(
                "Each family of two tracks or more in a bus of its own: drums, guitars, backing \
                vocals...",
            ),
            |ui| switch_labeled(ui, p, &mut grouping),
        );
        if grouping != self.grouping {
            self.grouping = grouping;
            changed = true;
        }
        let groups = self.groups();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            if !self.grouping {
                ui.label(RichText::new(tr("The tracks stay where they are.")).color(p.weak));
            } else if groups.is_empty() {
                ui.label(
                    RichText::new(tr("No family has two tracks or more outside a bus yet."))
                        .color(p.weak),
                );
            }
            for (key, tracks) in &groups {
                let name = rules::GROUPS
                    .iter()
                    .find(|g| g.0 == *key)
                    .map_or("", |g| tr(g.1));
                let names: Vec<&str> = tracks
                    .iter()
                    .filter_map(|t| self.rows.iter().find(|r| r.track == *t))
                    .map(|r| r.name.as_str())
                    .collect();
                pill(ui, p, &format!("{name}: {}", names.join(", ")), p.accent);
            }
        });
        let listening = self
            .rows
            .iter()
            .any(|r| r.bus.is_none() && !matches!(r.state, State::Ready(_) | State::Failed(_)));
        let unknown = self
            .rows
            .iter()
            .filter(|r| {
                r.included && matches!(&r.state, State::Ready(x) if x.guess == Guess::Unknown)
            })
            .count();
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let face = if listening {
                        Face::Plain
                    } else {
                        Face::Lit(p.accent)
                    };
                    let next = chip(ui, "continue", tr("Continue"), face);
                    if next.clicked() && !listening {
                        self.view = View::Overview;
                    }
                    ui.add_space(10.0);
                    let (text, ink) = if listening {
                        (tr("Listening to the tracks…").to_owned(), p.weak)
                    } else if unknown > 0 {
                        (
                            trf!(
                                "{count} tracks without an instrument will be prepared as \
                                 something else",
                                count = unknown
                            ),
                            WARN,
                        )
                    } else {
                        (tr("Every instrument is known.").to_owned(), p.weak)
                    };
                    ui.label(RichText::new(text).size(11.5).color(ink));
                });
            });
        });
        if changed {
            self.dirty = true;
        }
    }

    /// The overview's entry, then every track: whether it is prepared, its mute and solo, its
    /// instrument; a click shows what is proposed for it. Grouped, the buses proposed head
    /// their tracks, and are dragged to change their order.
    fn list(&mut self, ui: &mut Ui, p: &Palette, doc: &mut Document, mode: Mode) {
        ui.spacing_mut().item_spacing.y = 0.0;
        if overview_entry(ui, p, self.view == View::Overview).clicked() {
            self.view = View::Overview;
        }
        ui.add_space(14.0);
        let total = self.rows.iter().filter(|r| r.group.is_none()).count();
        let kept = self
            .rows
            .iter()
            .filter(|r| r.group.is_none() && r.included)
            .count();
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            line.left_center() + vec2(8.0, 0.0),
            Align2::LEFT_CENTER,
            tr("Tracks"),
            theme::strong(11.0),
            p.weak,
        );
        painter.text(
            line.right_center() - vec2(8.0, 0.0),
            Align2::RIGHT_CENTER,
            trf!("{count} of {total} included", count = kept, total = total),
            FontId::proportional(10.5),
            p.weak,
        );
        ui.add_space(4.0);
        let strengths = (self.clean, self.bring, self.flavor);
        let groups = self.groups();
        let grouped: Vec<TrackId> = groups.iter().flat_map(|g| g.1.clone()).collect();
        let mut opened = None;
        let mut changed = false;
        let mut moved = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (key, tracks) in &groups {
                    let row = self.rows.iter().position(|r| r.group == Some(*key));
                    let from = self.group_order.iter().position(|k| k == key).unwrap_or(0);
                    let busy = row.is_some_and(|k| {
                        !matches!(self.rows[k].state, State::Ready(_) | State::Failed(_))
                    });
                    let shown = row.is_some_and(|k| self.view == View::Track(k));
                    let header = group_header(ui, p, key, from, shown, busy);
                    if header.clicked
                        && let Some(k) = row
                    {
                        opened = Some(k);
                    }
                    if let Some(dragged) = header.dropped {
                        moved = Some((dragged, from));
                    }
                    for t in tracks {
                        let Some(k) = self.rows.iter().position(|r| r.track == *t) else {
                            continue;
                        };
                        let shown = self.view == View::Track(k);
                        let row = &mut self.rows[k];
                        ui.horizontal(|ui| {
                            ui.add_space(16.0);
                            let (open, edit) =
                                track_row(ui, p, doc, row, k, shown, mode, strengths);
                            if open {
                                opened = Some(k);
                            }
                            changed |= edit;
                        });
                    }
                    ui.add_space(4.0);
                }
                for (k, row) in self.rows.iter_mut().enumerate() {
                    if row.group.is_some() || grouped.contains(&row.track) {
                        continue;
                    }
                    let shown = self.view == View::Track(k);
                    let (open, edit) = track_row(ui, p, doc, row, k, shown, mode, strengths);
                    if open {
                        opened = Some(k);
                    }
                    changed |= edit;
                }
            });
        if let Some((from, to)) = moved
            && from != to
            && from < self.group_order.len()
        {
            let key = self.group_order.remove(from);
            self.group_order.insert(to.min(self.group_order.len()), key);
            changed = true;
        }
        if let Some(k) = opened {
            self.view = View::Track(k);
        }
        if changed {
            self.dirty = true;
        }
    }

    /// The console color, the style of the mix, the doses and the grouping into buses, for
    /// every track, and what they propose in all; all of it without scrolling.
    fn overview(&mut self, ui: &mut Ui, p: &Palette, doc: &Document, mode: Mode) {
        ui.spacing_mut().item_spacing.y = 6.0;
        let (mut changed, mut settled) = (false, false);
        if mode.brings_out() {
            heading(
                ui,
                p,
                tr("Console color"),
                tr("The project's, heard on every track unless a track has its own"),
                |ui| ui.label(""),
            );
            let (c, s) = console_picker(ui, p, "every", &mut self.console, false);
            if s || (c && !ui.input(|i| i.pointer.any_down())) {
                self.dirty = true;
            }
            ui.add_space(10.0);
            heading(
                ui,
                p,
                tr("Style"),
                tr("The style of the song: it doses each move and the balance between instruments"),
                |ui| ui.label(""),
            );
            if let Some(c) = style_tiles(ui, p, self.flavor.character) {
                self.flavor.character = c;
                changed = true;
                settled = true;
            }
            let mut intensity = self.flavor.amount * 100.0;
            let natural = self.flavor.character == Character::Natural;
            let slider = fader(
                ui,
                p,
                ("style-amount", tr("Intensity")),
                &mut intensity,
                style_color(self.flavor.character),
                !natural,
            );
            if slider.changed() {
                self.flavor.amount = intensity / 100.0;
                changed = true;
                settled |= !slider.dragged();
            }
            settled |= slider.drag_stopped();
            ui.add_space(10.0);
        }
        heading(
            ui,
            p,
            tr("Processing"),
            tr("For every track at once: each one can then be adjusted on its own."),
            |ui| {
                let mut headroom = self.headroom;
                let r = switch_labeled_as(ui, p, tr("Headroom for the master"), &mut headroom)
                    .on_hover_text(tr(
                        "When the mix peaks above -6 dBFS, it is lowered before the master's \
                         effects and brought back after by a Strada-Max: the same loudness, \
                         room for the master's processing, no clipping",
                    ));
                if headroom != self.headroom {
                    self.headroom = headroom;
                    self.master_dirty = true;
                }
                r
            },
        );
        let parts = usize::from(mode.cleans()) + 3 * usize::from(mode.brings_out());
        let gap = 10.0;
        let width = (ui.available_width() - gap * (parts as f32 - 1.0)) / parts as f32;
        let mut spaced = false;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if mode.cleans() {
                let (c, s) = dose(
                    ui,
                    p,
                    ("clean", tr("Clean up"), CUT),
                    tr("Useless lows, resonances, plosives and sibilance, bleed"),
                    &mut self.clean.0,
                    width,
                    &|v| tr(Strength(v).name()),
                );
                changed |= c;
                settled |= s;
            }
            if mode.brings_out() {
                let (c, s) = dose(
                    ui,
                    p,
                    ("bring", tr("Bring out"), p.accent),
                    tr("The instrument's colour, an even level"),
                    &mut self.bring.0,
                    width,
                    &|v| tr(Strength(v).name()),
                );
                changed |= c;
                settled |= s;
                let (_, s) = dose(
                    ui,
                    p,
                    ("space", tr("Space"), SPACE),
                    tr("A light reverb shared by the tracks that call for it"),
                    &mut self.space,
                    width,
                    &space_word,
                );
                spaced = s;
                let (_, s) = dose(
                    ui,
                    p,
                    ("wide", tr("Wide"), WIDE),
                    tr("Pairs of an instrument spread a little around the center"),
                    &mut self.wide,
                    width,
                    &wide_word,
                );
                spaced |= s;
            }
        });
        if changed {
            self.repropose(mode);
        }
        if settled || spaced {
            self.dirty = true;
        }
        ui.add_space(10.0);
        let mut grouping = self.grouping;
        heading(
            ui,
            p,
            tr("Buses"),
            tr(
                "Each family of two tracks or more in a bus of its own, shown in the list: drag the \
                buses there to change their order",
            ),
            |ui| switch_labeled(ui, p, &mut grouping),
        );
        if grouping != self.grouping {
            self.grouping = grouping;
            self.dirty = true;
        }
        ui.add_space(10.0);
        let total = self.rows.len();
        let done = self
            .rows
            .iter()
            .filter(|r| matches!(r.state, State::Ready(_) | State::Failed(_)))
            .count();
        if done < total {
            let progress = self
                .rows
                .iter()
                .map(|r| match &r.state {
                    State::Queued => 0.0,
                    State::Measuring(m) => m.progress() * 0.9,
                    State::Analysing(_) => 0.95,
                    _ => 1.0,
                })
                .sum::<f32>()
                / total.max(1) as f32;
            ui.label(
                RichText::new(trf!(
                    "Listening to the tracks: {done} of {total}",
                    done = done,
                    total = total
                ))
                .color(p.weak),
            );
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
            ui.painter().rect_filled(bar, 2.0, field_fill(p));
            ui.painter().rect_filled(
                bar.with_max_x(bar.left() + bar.width() * progress),
                2.0,
                p.accent,
            );
        } else {
            let plan = self.plan(doc, mode);
            let headroom = self
                .headroom
                .then(|| match (self.mix_peak, self.lowered()) {
                    (Some(peak), Some(db)) if !self.mix_stale && self.mix.is_none() => trf!(
                        "Mix peak {peak} dBFS: {db} dB of room kept for the master",
                        peak = format!("{peak:.1}"),
                        db = format!("{db:.1}")
                    ),
                    (Some(peak), None) if !self.mix_stale && self.mix.is_none() => trf!(
                        "Mix peak {peak} dBFS: room kept",
                        peak = format!("{peak:.1}")
                    ),
                    _ => tr("Measuring the mix…").to_owned(),
                });
            if let Some(k) = summary(ui, p, &self.rows, &plan, headroom) {
                self.view = View::Track(k);
            }
        }
    }

    /// What is proposed for one track: who it is, its color and its place in the mix, then its
    /// spectrum and the steps of each part.
    fn track(&mut self, ui: &mut Ui, p: &Palette, doc: &Document, mode: Mode, k: usize) {
        let several = self.rows.len() > 1;
        let strengths = (self.clean, self.bring, self.flavor);
        let every = Some(self.console);
        let planned = self
            .rows
            .get(k)
            .and_then(|row| self.plan(doc, mode).into_iter().find(|x| x.0 == row.track));
        let shift = self.rows.get(k).map_or(0.0, |row| self.staged(row).1);
        let group = self.rows.get(k).and_then(|row| {
            self.made_for
                .iter()
                .find(|g| g.1.contains(&row.track))
                .and_then(|g| rules::GROUPS.iter().find(|x| x.0 == g.0))
                .map(|g| tr(g.1))
        });
        let mut back = false;
        let mut changed = false;
        let Some(row) = self.rows.get_mut(k) else {
            return;
        };
        ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
        ui.horizontal(|ui| {
            if several && back_button(ui, p).clicked() {
                back = true;
            }
            ui.add_space(4.0);
            let (bar, _) = ui.allocate_exact_size(vec2(4.0, 26.0), Sense::hover());
            ui.painter().rect_filled(bar, 2.0, row.color);
            ui.label(RichText::new(&row.name).font(theme::strong(18.0)));
            if row.bus.is_some() {
                ui.label(RichText::new(tr("Bus")).size(10.5).strong().color(p.weak));
            }
            if several && !matches!(row.state, State::Failed(_)) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if toggle(ui, p, &mut row.included)
                        .on_hover_text(tr("Off, this track is left as it is"))
                        .changed()
                    {
                        changed = true;
                    }
                    ui.label(RichText::new(tr("Include this track")).color(p.weak));
                });
            }
        });
        match &mut row.state {
            State::Queued => waiting(ui, p, 0.0),
            State::Analysing(_) => waiting(ui, p, 1.0),
            State::Measuring(m) => waiting(ui, p, m.progress()),
            State::Failed(e) => {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.colored_label(theme::MUTE, e.as_str()));
            }
            State::Ready(r) => {
                let bus = row.bus.as_ref();
                egui::Frame::new()
                    .fill(field_fill(p))
                    .stroke(Stroke::new(1.0, line_edge(p)))
                    .corner_radius(radius(p))
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.columns(3, |c| {
                            changed |= identity(&mut c[0], p, k, r, bus, mode, strengths);
                            if mode.brings_out() {
                                changed |= own_console(&mut c[1], p, k, r, every);
                            }
                            placement(
                                &mut c[2],
                                p,
                                (planned.as_ref(), shift),
                                group,
                                bus.is_some(),
                            );
                        });
                    });
                egui::ScrollArea::vertical()
                    .id_salt(("steps", k))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
                        spectrum(ui, p, r);
                        ui.columns(if mode == Mode::Both { 2 } else { 1 }, |columns| {
                            let mut column = 0;
                            for (brings_out, title, tint) in [
                                (false, tr("Clean up"), CUT),
                                (true, tr("Bring out"), p.accent),
                            ] {
                                if brings_out && !mode.brings_out() || !brings_out && !mode.cleans()
                                {
                                    continue;
                                }
                                changed |=
                                    card(&mut columns[column], p, title, tint, r, brings_out);
                                column += 1;
                            }
                        });
                    });
            }
        }
        if back {
            self.view = View::Overview;
        }
        if changed {
            self.dirty = true;
        }
    }

    /// The bottom bar: listening (play, before and after, the level kept), then the outcome.
    fn footer(&mut self, ui: &mut Ui, p: &Palette, playing: bool, width: f32, asked: &mut Asked) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 54.0), Sense::hover());
        paint_bottom_band(ui.painter(), p, rect);
        let ready = self
            .rows
            .iter()
            .filter(|r| r.included && matches!(r.state, State::Ready(_)))
            .count();
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
        ui.label(tr("Same level")).on_hover_text(tr(
            "Brings the prepared track to the loudness it had, measured on its loudest 20 \
             seconds: before and after compare at equal level, and Apply keeps that gain on \
             the channel strip's output",
        ));
        if let (true, View::Track(k)) = (self.keep_level, self.view)
            && let Some(row) = self.rows.get(k)
            && row.included
            && matches!(row.state, State::Ready(_))
        {
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{:+.1} dB", row.level))
                    .font(FontId::monospace(11.0))
                    .color(p.accent),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let text = if self.rows.len() > 1 {
                trf!("Apply to {count} tracks", count = ready)
            } else {
                tr("Apply").to_owned()
            };
            let face = if ready > 0 {
                Face::Lit(p.accent)
            } else {
                Face::Plain
            };
            if chip(ui, "apply", &text, face).clicked() && ready > 0 {
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
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 16.0), Sense::click());
    switch(ui, p, rect, response, on)
}

fn switch(ui: &Ui, p: &Palette, rect: Rect, mut response: Response, on: &mut bool) -> Response {
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

/// The first entry of the list, leading back to the doses for every track.
fn overview_entry(ui: &mut Ui, p: &Palette, shown: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    let painter = ui.painter();
    let fill = if shown {
        theme::mix(p.surface, p.accent, 0.25)
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if shown {
        Stroke::new(1.0, theme::mix(p.surface, p.accent, 0.6))
    } else {
        Stroke::NONE
    };
    painter.rect(rect, radius(p), fill, stroke, StrokeKind::Inside);
    // Two small knobs, the doses.
    for (k, tint) in [CUT, p.accent].into_iter().enumerate() {
        let c = pos2(rect.left() + 16.0 + k as f32 * 13.0, rect.center().y);
        painter.circle_stroke(c, 5.5, Stroke::new(2.0, tint));
        painter.line_segment([c, c + vec2(2.5, -3.5)], Stroke::new(1.5, p.text));
    }
    let left = rect.left() + 46.0;
    painter.text(
        pos2(left, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        tr("Overview"),
        theme::strong(12.5),
        p.text,
    );
    painter.text(
        pos2(left, rect.center().y + 9.0),
        Align2::LEFT_CENTER,
        tr("Settings for every track, what is proposed"),
        FontId::proportional(10.5),
        p.weak,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A track of the list; returns whether it was clicked and whether its proposal changed.
#[allow(clippy::too_many_arguments)]
fn track_row(
    ui: &mut Ui,
    p: &Palette,
    doc: &mut Document,
    row: &mut Row,
    k: usize,
    shown: bool,
    mode: Mode,
    strengths: Doses,
) -> (bool, bool) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let painter = ui.painter().clone();
    let fill = if shown {
        theme::mix(p.surface, row.color, 0.22)
    } else if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    let unknown =
        row.included && matches!(&row.state, State::Ready(r) if r.guess == Guess::Unknown);
    let stroke = if unknown {
        Stroke::new(1.0, WARN.gamma_multiply(0.8))
    } else if shown {
        Stroke::new(1.0, theme::mix(p.surface, row.color, 0.6))
    } else {
        Stroke::NONE
    };
    painter.rect(rect, radius(p), fill, stroke, StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_min_size(
            rect.left_top() + vec2(4.0, 9.0),
            vec2(3.0, rect.height() - 18.0),
        ),
        1.5,
        row.color,
    );
    let mut changed = false;
    let failed = matches!(row.state, State::Failed(_));
    let id = ui.scope_id().with(("row", k));
    let on = Rect::from_min_size(
        pos2(rect.left() + 14.0, rect.top() + 10.0),
        vec2(28.0, 16.0),
    );
    if !failed
        && switch(
            ui,
            p,
            on,
            ui.interact(on, id.with("on"), Sense::click()),
            &mut row.included,
        )
        .on_hover_text(tr("Off, this track is left as it is"))
        .changed()
    {
        changed = true;
    }
    // Mute and solo, to hear the proposal in the mix or alone.
    let (mute, solo) = doc
        .project()
        .track(row.track)
        .map_or((false, false), |t| (t.mute, t.solo));
    let solo_rect = Rect::from_min_size(
        pos2(rect.right() - 28.0, rect.top() + 9.0),
        vec2(20.0, 18.0),
    );
    let mute_rect = solo_rect.translate(vec2(-24.0, 0.0));
    for (rect, link, letter, on, color, hint) in [
        (mute_rect, Link::Mute, "M", mute, theme::MUTE, tr("Mute")),
        (solo_rect, Link::Solo, "S", solo, theme::SOLO, tr("Solo")),
    ] {
        let response = ui.interact(rect, id.with(letter), Sense::click());
        theme::flat_toggle(ui, rect, &response, letter, on, color);
        if response.on_hover_text(hint).clicked() {
            doc.history_mut()
                .modify_unrecorded(|project| groups::set_switch(project, row.track, link, !on));
        }
    }
    let left = on.right() + 10.0;
    let ink = if row.included && !failed {
        p.text
    } else {
        p.weak
    };
    painter
        .with_clip_rect(Rect::from_min_max(
            pos2(left, rect.top()),
            pos2(mute_rect.left() - 6.0, rect.bottom()),
        ))
        .text(
            pos2(left, on.center().y),
            Align2::LEFT_CENTER,
            &row.name,
            theme::strong(12.5),
            ink,
        );
    let line = Rect::from_min_max(
        pos2(left, rect.top() + 29.0),
        pos2(rect.right() - 8.0, rect.bottom() - 4.0),
    );
    let small = FontId::proportional(10.5);
    match &mut row.state {
        State::Failed(e) => {
            painter.text(
                line.left_center(),
                Align2::LEFT_CENTER,
                e.as_str(),
                small,
                theme::MUTE,
            );
        }
        State::Ready(r) => {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(line)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().interact_size.y = 18.0;
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let bus = row.bus.as_ref();
                    if instrument_combo(ui, ("instrument", k), r, bus.is_some(), 160.0) {
                        r.steps = propose(&r.analysis, r.instrument, mode, strengths, bus);
                        changed = true;
                    }
                },
            );
        }
        state => {
            let progress = match state {
                State::Measuring(m) => m.progress(),
                State::Analysing(_) => 1.0,
                _ => 0.0,
            };
            let text = if matches!(state, State::Queued) {
                tr("Waiting")
            } else {
                tr("Listening…")
            };
            painter.text(
                line.left_center() - vec2(0.0, 3.0),
                Align2::LEFT_CENTER,
                text,
                small,
                p.weak,
            );
            let bar = Rect::from_min_size(
                pos2(line.left(), line.center().y + 6.0),
                vec2(line.width().min(150.0), 3.0),
            );
            painter.rect_filled(bar, 1.5, field_fill(p));
            painter.rect_filled(
                bar.with_max_x(bar.left() + bar.width() * progress.clamp(0.0, 1.0)),
                1.5,
                p.accent,
            );
        }
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    (response.clicked(), changed)
}

/// A track on the check screen, on one line: its color, whether it is included, its name,
/// its instrument as found (or the listening going on), its mute and solo. True when its
/// proposal changed.
fn check_row(
    ui: &mut Ui,
    p: &Palette,
    doc: &mut Document,
    row: &mut Row,
    k: usize,
    mode: Mode,
    strengths: Doses,
) -> bool {
    let mut changed = false;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
    let painter = ui.painter().clone();
    let unknown =
        row.included && matches!(&row.state, State::Ready(r) if r.guess == Guess::Unknown);
    let stroke = if unknown {
        Stroke::new(1.0, WARN.gamma_multiply(0.8))
    } else {
        Stroke::new(1.0, line_edge(p))
    };
    painter.rect(rect, radius(p), field_fill(p), stroke, StrokeKind::Inside);
    painter.rect_filled(
        Rect::from_min_size(
            rect.left_top() + vec2(5.0, 8.0),
            vec2(3.0, rect.height() - 16.0),
        ),
        1.5,
        row.color,
    );
    let id = ui.scope_id().with(("check", k));
    let failed = matches!(row.state, State::Failed(_));
    let on = Rect::from_center_size(pos2(rect.left() + 34.0, rect.center().y), vec2(28.0, 16.0));
    if !failed
        && switch(
            ui,
            p,
            on,
            ui.interact(on, id.with("on"), Sense::click()),
            &mut row.included,
        )
        .on_hover_text(tr("Off, this track is left as it is"))
        .changed()
    {
        changed = true;
    }
    let ink = if row.included && !failed {
        p.text
    } else {
        p.weak
    };
    let name_left = on.right() + 14.0;
    let combo_left = name_left + 230.0;
    painter
        .with_clip_rect(Rect::from_min_max(
            pos2(name_left, rect.top()),
            pos2(combo_left - 10.0, rect.bottom()),
        ))
        .text(
            pos2(name_left, rect.center().y),
            Align2::LEFT_CENTER,
            &row.name,
            theme::strong(13.0),
            ink,
        );
    let (mute, solo) = doc
        .project()
        .track(row.track)
        .map_or((false, false), |t| (t.mute, t.solo));
    let solo_rect =
        Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(20.0, 18.0));
    let mute_rect = solo_rect.translate(vec2(-24.0, 0.0));
    for (r, link, letter, on, color, hint) in [
        (mute_rect, Link::Mute, "M", mute, theme::MUTE, tr("Mute")),
        (solo_rect, Link::Solo, "S", solo, theme::SOLO, tr("Solo")),
    ] {
        let response = ui.interact(r, id.with(letter), Sense::click());
        theme::flat_toggle(ui, r, &response, letter, on, color);
        if response.on_hover_text(hint).clicked() {
            doc.history_mut()
                .modify_unrecorded(|project| groups::set_switch(project, row.track, link, !on));
        }
    }
    let line = Rect::from_min_max(
        pos2(combo_left, rect.top() + 6.0),
        pos2(mute_rect.left() - 12.0, rect.bottom() - 6.0),
    );
    let small = FontId::proportional(11.0);
    match &mut row.state {
        State::Failed(e) => {
            painter.text(
                line.left_center(),
                Align2::LEFT_CENTER,
                e.as_str(),
                small,
                theme::MUTE,
            );
        }
        State::Ready(r) => {
            let bus = row.bus.as_ref();
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(line)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if instrument_combo(ui, ("check-instrument", k), r, bus.is_some(), 240.0) {
                        r.steps = propose(&r.analysis, r.instrument, mode, strengths, bus);
                        changed = true;
                    }
                    let (found, color) = match r.guess {
                        Guess::Name => (tr("from the track's name"), p.weak),
                        Guess::Instrument if bus.is_some() => (tr("from its tracks"), p.weak),
                        Guess::Instrument => (tr("from its instrument"), p.weak),
                        Guess::Unknown => (tr("not found from its name: choose it"), WARN),
                        Guess::Chosen => (tr("chosen"), p.weak),
                    };
                    ui.label(RichText::new(found).size(11.0).color(color));
                },
            );
        }
        state => {
            let progress = match state {
                State::Measuring(m) => m.progress(),
                State::Analysing(_) => 1.0,
                _ => 0.0,
            };
            let text = if matches!(state, State::Queued) {
                tr("Waiting")
            } else {
                tr("Listening…")
            };
            painter.text(line.left_center(), Align2::LEFT_CENTER, text, small, p.weak);
            let bar = Rect::from_min_size(
                pos2(line.left() + 90.0, line.center().y - 1.5),
                vec2((line.width() - 90.0).min(240.0), 3.0),
            );
            painter.rect_filled(bar, 1.5, bar_fill(p));
            painter.rect_filled(
                bar.with_max_x(bar.left() + bar.width() * progress.clamp(0.0, 1.0)),
                1.5,
                p.accent,
            );
        }
    }
    changed
}

/// The instrument's choice, by family; true when another was chosen. Not found from the
/// track's name, it asks to be chosen.
fn instrument_combo(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    r: &mut Ready,
    bus: bool,
    width: f32,
) -> bool {
    let p = theme::current(ui);
    let mut changed = false;
    // A bus by its family, an instrument by its name.
    let name = |i: Instrument| {
        if !bus {
            tr(i.name())
        } else if i == Instrument::Other {
            tr("A mix of everything")
        } else {
            tr(i.category())
        }
    };
    let shown = if r.guess == Guess::Unknown {
        let text = if bus {
            tr("Choose what it holds")
        } else {
            tr("Choose the instrument")
        };
        RichText::new(text).strong().color(WARN)
    } else {
        RichText::new(name(r.instrument)).strong()
    };
    let response = egui::ComboBox::from_id_salt(id)
        .width(width)
        .height(640.0)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if bus {
                for i in rules::BUSES {
                    if ui.selectable_label(r.instrument == i, name(i)).clicked()
                        && (r.instrument != i || r.guess == Guess::Unknown)
                    {
                        r.instrument = i;
                        r.guess = Guess::Chosen;
                        changed = true;
                    }
                }
                return;
            }
            let mut family = "";
            for i in Instrument::ALL {
                if i.category() != family {
                    family = i.category();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(tr(family).to_uppercase())
                            .size(9.5)
                            .strong()
                            .color(p.weak),
                    );
                }
                if ui
                    .selectable_label(r.instrument == i, tr(i.name()))
                    .clicked()
                    && (r.instrument != i || r.guess == Guess::Unknown)
                {
                    r.instrument = i;
                    r.guess = Guess::Chosen;
                    changed = true;
                }
            }
        })
        .response;
    if r.guess == Guess::Unknown {
        response.on_hover_text(tr(
            "Its name does not tell the instrument: choose it for a proposal that suits it",
        ));
    }
    changed
}

/// A section's title and what it does, a control at its right.
fn heading(
    ui: &mut Ui,
    p: &Palette,
    title: &str,
    what: &str,
    right: impl FnOnce(&mut Ui) -> Response,
) -> Response {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).font(theme::strong(15.0)));
            if !what.is_empty() {
                ui.label(RichText::new(what).size(11.0).color(p.weak));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            out = Some(right(ui));
        });
    });
    ui.add_space(4.0);
    out.unwrap_or_else(|| ui.label(""))
}

/// A switch named beside it, in a layout from the right.
fn switch_labeled_as(ui: &mut Ui, p: &Palette, name: &str, on: &mut bool) -> Response {
    let response = toggle(ui, p, on);
    ui.label(RichText::new(name).color(if *on { p.text } else { p.weak }));
    response
}

/// A switch with its state written beside it.
fn switch_labeled(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let response = toggle(ui, p, on);
    ui.label(RichText::new(if *on { tr("On") } else { tr("Off") }).color(p.weak));
    response
}

/// A column's small title.
fn caption(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(9.5)
            .strong()
            .color(p.weak),
    );
}

/// The color of a style, to find it among the others.
fn style_color(c: Character) -> Color32 {
    match c {
        Character::Natural => Color32::from_rgb(0x9a, 0x9c, 0xa3),
        Character::Pop => Color32::from_rgb(0xe8, 0x6f, 0xb0),
        Character::Rock => Color32::from_rgb(0xe0, 0x4f, 0x4f),
        Character::Acoustic => Color32::from_rgb(0xc9, 0x9a, 0x5b),
        Character::Electronic => Color32::from_rgb(0x6f, 0x8c, 0xf2),
    }
}

/// The styles, as tiles, none first; the one clicked.
fn style_tiles(ui: &mut Ui, p: &Palette, current: Character) -> Option<Character> {
    let gap = 8.0;
    let count = Character::STYLES.len() as f32;
    let width = (ui.available_width() - gap * (count - 1.0)) / count;
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for c in Character::STYLES {
            let (rect, response) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
            let on = c == current;
            let tint = style_color(c);
            let fill = if on {
                theme::mix(p.surface, tint, 0.22)
            } else if response.hovered() {
                p.hover
            } else {
                field_fill(p)
            };
            let stroke = if on {
                Stroke::new(1.5, tint)
            } else {
                Stroke::new(1.0, line_edge(p))
            };
            let painter = ui.painter_at(rect);
            painter.rect(rect, radius(p), fill, stroke, StrokeKind::Inside);
            painter.circle_filled(pos2(rect.left() + 15.0, rect.top() + 16.0), 5.0, tint);
            painter.text(
                pos2(rect.left() + 27.0, rect.top() + 16.0),
                Align2::LEFT_CENTER,
                tr(c.name()),
                theme::strong(13.0),
                p.text,
            );
            let galley = painter.layout(
                tr(c.description()).to_owned(),
                FontId::proportional(10.5),
                p.weak,
                width - 24.0,
            );
            painter.galley(pos2(rect.left() + 12.0, rect.top() + 27.0), galley, p.weak);
            if response
                .on_hover_text(tr(c.description()))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !on
            {
                picked = Some(c);
            }
        }
    });
    picked
}

/// A bus proposed, heading its tracks in the list: what was clicked, and the bus dropped on it.
struct Header {
    clicked: bool,
    dropped: Option<usize>,
}

/// A bus proposed in the list: its grip, its name, its place `index` in the order (carried
/// when it is dragged onto another).
fn group_header(
    ui: &mut Ui,
    p: &Palette,
    key: &str,
    index: usize,
    shown: bool,
    busy: bool,
) -> Header {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click_and_drag());
    response.dnd_set_drag_payload(index);
    let painter = ui.painter_at(rect);
    let fill = if response.dragged() {
        theme::mix(p.surface, p.accent, 0.25)
    } else if shown {
        theme::mix(p.surface, p.accent, 0.18)
    } else if response.hovered() {
        p.hover
    } else {
        theme::mix(bar_fill(p), p.surface, 0.5)
    };
    painter.rect(
        rect,
        radius(p),
        fill,
        Stroke::new(1.0, line_edge(p)),
        StrokeKind::Inside,
    );
    for (dx, dy) in [
        (0.0, -4.0),
        (0.0, 0.0),
        (0.0, 4.0),
        (4.0, -4.0),
        (4.0, 0.0),
        (4.0, 4.0),
    ] {
        painter.circle_filled(
            pos2(rect.left() + 10.0 + dx, rect.center().y + dy),
            1.2,
            p.weak,
        );
    }
    let name = rules::GROUPS
        .iter()
        .find(|g| g.0 == key)
        .map_or("", |g| tr(g.1));
    let title = painter.text(
        pos2(rect.left() + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::strong(12.0),
        p.text,
    );
    painter.text(
        pos2(title.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        tr("BUS"),
        theme::strong(9.0),
        p.weak,
    );
    if busy {
        painter.text(
            rect.right_center() - vec2(10.0, 0.0),
            Align2::RIGHT_CENTER,
            tr("Listening…"),
            FontId::proportional(10.0),
            p.weak,
        );
    }
    if response.dnd_hover_payload::<usize>().is_some() {
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(2.0, p.accent),
        );
    }
    let dropped = response.dnd_release_payload::<usize>().map(|d| *d);
    let response = response
        .on_hover_text(tr(
            "Drag to change the order of the buses; click to see its proposal",
        ))
        .on_hover_cursor(egui::CursorIcon::Grab);
    Header {
        clicked: response.clicked(),
        dropped,
    }
}

/// The five console colors to choose from, then how much of it; whether it changed, and
/// whether that settled (not in the middle of a drag). `compact`: colored dots, the name
/// of the one chosen below.
fn console_picker(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    console: &mut Console,
    compact: bool,
) -> (bool, bool) {
    if compact {
        return ui
            .horizontal(|ui| picker_body(ui, p, id, console, true))
            .inner;
    }
    picker_body(ui, p, id, console, false)
}

fn picker_body(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    console: &mut Console,
    compact: bool,
) -> (bool, bool) {
    let (mut changed, mut settled) = (false, false);
    let gap = if compact { 6.0 } else { 8.0 };
    let count = ConsoleModel::ALL.len() as f32;
    let width = if compact {
        30.0
    } else {
        (ui.available_width() - gap * (count - 1.0)) / count
    };
    let height = if compact { 30.0 } else { 54.0 };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for m in ConsoleModel::ALL {
            let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
            let on = console.model == m;
            let tint = crate::console_ui::color(m);
            let painter = ui.painter_at(rect);
            if compact {
                let c = rect.center();
                if on {
                    painter.circle_stroke(c, 13.0, Stroke::new(2.0, tint));
                } else if response.hovered() {
                    painter.circle_filled(c, 13.0, p.hover);
                }
                painter.circle_filled(c, 9.0, tint);
            } else {
                let fill = if on {
                    theme::mix(p.surface, tint, 0.25)
                } else if response.hovered() {
                    p.hover
                } else {
                    field_fill(p)
                };
                let stroke = if on {
                    Stroke::new(1.5, tint)
                } else {
                    Stroke::new(1.0, line_edge(p))
                };
                painter.rect(rect, radius(p), fill, stroke, StrokeKind::Inside);
                painter.rect_filled(
                    Rect::from_min_size(
                        rect.left_top() + vec2(10.0, 10.0),
                        vec2(rect.width() - 20.0, 6.0),
                    ),
                    3.0,
                    tint,
                );
                painter.text(
                    pos2(rect.center().x, rect.top() + 34.0),
                    Align2::CENTER_CENTER,
                    crate::console_ui::name(m),
                    theme::strong(12.5),
                    p.text,
                );
            }
            let hint = format!(
                "{}: {}",
                crate::console_ui::name(m),
                crate::console_ui::hint(m)
            );
            if response
                .on_hover_text(hint)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !on
            {
                console.model = m;
                changed = true;
                settled = true;
            }
        }
    });
    let mut amount = console.amount;
    let colors = console.model != ConsoleModel::Clean;
    let response = if compact {
        ui.push_id(id, |ui| {
            ui.spacing_mut().slider_width = 70.0;
            let slider = egui::Slider::new(&mut amount, 0.0..=100.0)
                .suffix(" %")
                .fixed_decimals(0);
            ui.add_enabled(colors, slider)
        })
        .inner
    } else {
        let tint = crate::console_ui::color(console.model);
        fader(ui, p, (id, tr("Amount")), &mut amount, tint, colors)
    };
    let response = response.on_hover_text(tr("How much of the console's color"));
    if response.changed() {
        console.amount = amount;
        changed = true;
        settled |= !response.dragged();
    }
    settled |= response.drag_stopped();
    (changed, settled)
}

/// A horizontal fader the whole width, from 0 to 100 %: its name, a groove lit in a gradient
/// of `tint` up to the value, a cap; a drag or a click sets it, a double-click brings it back
/// to 50 %.
fn fader(
    ui: &mut Ui,
    p: &Palette,
    (id, label): (&str, &str),
    value: &mut f32,
    tint: Color32,
    enabled: bool,
) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let label_width = 96.0;
    let groove = Rect::from_min_max(
        pos2(rect.left() + label_width, rect.center().y - 4.0),
        pos2(rect.right() - 56.0, rect.center().y + 4.0),
    );
    let mut response = ui.interact(
        groove.expand2(vec2(8.0, 9.0)),
        ui.scope_id().with(id),
        sense,
    );
    if let Some(pos) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked())
    {
        let v = ((pos.x - groove.left()) / groove.width() * 100.0).clamp(0.0, 100.0);
        if (v - *value).abs() > f32::EPSILON {
            *value = v;
            response.mark_changed();
        }
    }
    if response.double_clicked() {
        *value = 50.0;
        response.mark_changed();
    }
    let mut painter = ui.painter_at(rect);
    if !enabled {
        painter.multiply_opacity(0.4);
    }
    painter.text(
        pos2(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::strong(12.0),
        p.text,
    );
    painter.text(
        pos2(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{:.0} %", *value),
        FontId::monospace(12.0),
        p.text,
    );
    // The groove, then its gradient up to the value.
    let gradient = |painter: &egui::Painter, area: Rect, from: Color32, to: Color32| {
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(area.left_top(), from);
        mesh.colored_vertex(area.right_top(), to);
        mesh.colored_vertex(area.right_bottom(), to);
        mesh.colored_vertex(area.left_bottom(), from);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(mesh);
        painter.circle_filled(area.left_center(), area.height() / 2.0, from);
        painter.circle_filled(area.right_center(), area.height() / 2.0, to);
    };
    gradient(
        &painter,
        groove,
        field_fill(p),
        theme::mix(field_fill(p), tint, 0.35),
    );
    let x = groove.left() + groove.width() * (*value / 100.0);
    if *value > 0.5 {
        let lit = groove.with_max_x(x);
        let at_value = theme::mix(tint.gamma_multiply(0.35), tint, *value / 100.0);
        gradient(&painter, lit, tint.gamma_multiply(0.25), at_value);
    }
    for k in 0..=4 {
        let tx = groove.left() + groove.width() * k as f32 / 4.0;
        painter.line_segment(
            [
                pos2(tx, groove.bottom() + 5.0),
                pos2(tx, groove.bottom() + 8.0),
            ],
            Stroke::new(1.0, p.weak.gamma_multiply(0.6)),
        );
    }
    // The cap, as on a fader.
    let cap = Rect::from_center_size(pos2(x, groove.center().y), vec2(14.0, 22.0));
    let active = response.hovered() || response.dragged();
    painter.rect(
        cap.translate(vec2(0.0, 1.5)),
        3.0,
        Color32::from_black_alpha(80),
        Stroke::NONE,
        StrokeKind::Inside,
    );
    painter.rect(
        cap,
        3.0,
        if active { p.pressed } else { p.hover },
        Stroke::new(1.0, line_edge(p)),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            cap.center_top() + vec2(0.0, 5.0),
            cap.center_bottom() - vec2(0.0, 5.0),
        ],
        Stroke::new(2.0, tint),
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
    } else {
        response
    }
}

/// The track's view, first column: its instrument (or what a bus holds), and how it was found.
fn identity(
    ui: &mut Ui,
    p: &Palette,
    k: usize,
    r: &mut Ready,
    bus: Option<&Bus>,
    mode: Mode,
    strengths: Doses,
) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 6.0;
    caption(
        ui,
        p,
        if bus.is_some() {
            tr("What the bus holds")
        } else {
            tr("Instrument")
        },
    );
    let width = ui.available_width() - 12.0;
    if instrument_combo(ui, ("instrument", k), r, bus.is_some(), width) {
        r.steps = propose(&r.analysis, r.instrument, mode, strengths, bus);
        changed = true;
    }
    let found = match r.guess {
        Guess::Name => tr("from the track's name"),
        Guess::Instrument if bus.is_some() => tr("from its tracks"),
        Guess::Instrument => tr("from its instrument"),
        Guess::Unknown => tr("not found from its name: choose it"),
        Guess::Chosen => "",
    };
    let ink = if r.guess == Guess::Unknown {
        WARN
    } else {
        p.weak
    };
    ui.label(RichText::new(found).size(11.0).color(ink));
    if let Some(bus) = bus {
        let text = if bus.prepared > 0 {
            trf!(
                "{count} of its {total} tracks have their Express Mix: the bus gets a lighter \
                 touch and some glue",
                count = bus.prepared,
                total = bus.members.len()
            )
        } else {
            tr("Some glue and a light touch, nothing that belongs to its tracks").to_owned()
        };
        ui.label(RichText::new(text).size(11.0).color(p.weak));
    }
    changed
}

/// The track's view, second column: the console color set for every track, or its own.
fn own_console(ui: &mut Ui, p: &Palette, k: usize, r: &mut Ready, every: Option<Console>) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 6.0;
    caption(ui, p, tr("Console color"));
    let mut own = r.console.is_some();
    ui.horizontal(|ui| {
        if toggle(ui, p, &mut own).changed() {
            r.console = own.then(|| every.unwrap_or_default());
            changed = true;
        }
        ui.label(tr("Its own color"));
    });
    match &mut r.console {
        Some(console) => {
            let (_, settled) = console_picker(ui, p, &format!("own-{k}"), console, true);
            changed |= settled;
        }
        None => {
            let text = match every {
                Some(c) => trf!(
                    "As every track: {name}, {amount} %",
                    name = crate::console_ui::name(c.model),
                    amount = format!("{:.0}", c.amount)
                ),
                None => tr("Left as it is").to_owned(),
            };
            ui.label(RichText::new(text).size(11.0).color(p.weak));
        }
    }
    changed
}

/// The track's view, third column: where it goes in the mix.
fn placement(
    ui: &mut Ui,
    p: &Palette,
    (planned, shift): (Option<&Planned>, f32),
    group: Option<&str>,
    bus: bool,
) {
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    caption(ui, p, tr("In the mix"));
    let mut any = false;
    ui.horizontal_wrapped(|ui| {
        if shift.abs() >= 0.1 {
            let text = trf!("Input gain: {gain} dB", gain = format!("{shift:+.1}"));
            pill(ui, p, &text, CUT).on_hover_text(tr(
                "Brought to -18 dBFS first, the level its console and its compressors expect",
            ));
            any = true;
        }
        if let Some(name) = group {
            pill(ui, p, &trf!("In the bus {name}", name = name), p.accent);
            any = true;
        }
        if let Some((_, prep, send)) = planned {
            if let Some(db) = send {
                let text = trf!("Space: send at {db} dB", db = format!("{db:.0}"));
                pill(ui, p, &text, SPACE);
                any = true;
            }
            if let Some(pan) = prep.pan.filter(|p| p.abs() > 0.005) {
                pill(ui, p, &trf!("Pan: {pan}", pan = pan_text(pan)), WIDE);
                any = true;
            }
        }
    });
    if !any {
        let text = if bus {
            tr("A bus: no space, no spread")
        } else {
            tr("Stays where it is")
        };
        ui.label(RichText::new(text).size(11.0).color(p.weak));
    }
}

/// A pan as the mixer reads it: L 30, R 30.
fn pan_text(pan: f32) -> String {
    let side = if pan < 0.0 { "L" } else { "R" };
    format!("{side} {:.0}", pan.abs() * 100.0)
}

/// The word for the spread's dose.
fn wide_word(v: f32) -> &'static str {
    match v {
        x if x < 0.01 => tr("Off"),
        x if x < 0.35 => tr("Subtle"),
        x if x < 0.7 => tr("Open"),
        _ => tr("Wide open"),
    }
}

/// The word for the space's dose.
fn space_word(v: f32) -> &'static str {
    match v {
        x if x < 0.01 => tr("Off"),
        x if x < 0.35 => tr("A touch"),
        x if x < 0.7 => tr("Some room"),
        _ => tr("Generous"),
    }
}

/// The back button of a track's view.
fn back_button(ui: &mut Ui, p: &Palette) -> Response {
    let text = tr("Overview");
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
        .size()
        .x
        + 40.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let response = button(ui, rect, ui.scope_id().with("back"), true);
    paint_face(ui, rect, radius(p), &response, Face::Plain);
    paint_glyph(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 15.0, rect.center().y), vec2(13.0, 13.0)),
        Glyph::Left,
        p.text,
    );
    ui.painter().text(
        pos2(rect.left() + 27.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::strong(12.0),
        p.text,
    );
    response
        .on_hover_text(tr("Back to the doses for every track"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A part's dose, with its big knob; returns whether it moved, and whether it settled (the
/// preview follows then, not at every step of a drag).
fn dose(
    ui: &mut Ui,
    p: &Palette,
    (id, title, tint): (&str, &str, Color32),
    what: &str,
    value: &mut f32,
    width: f32,
    word: &dyn Fn(f32) -> &'static str,
) -> (bool, bool) {
    let mut out = (false, false);
    egui::Frame::new()
        .fill(field_fill(p))
        .stroke(Stroke::new(1.0, line_edge(p)))
        .corner_radius(radius(p))
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(RichText::new(title).font(theme::strong(14.0)));
                ui.add_space(6.0);
                let diameter = (width - 70.0).clamp(72.0, 96.0);
                let (v, response) = big_knob(ui, p, id, *value, tint, diameter);
                let moved = v != *value;
                *value = v;
                out = (
                    moved,
                    (moved && !response.dragged()) || response.drag_stopped(),
                );
                ui.add_space(6.0);
                let ink = if *value < 0.01 { p.weak } else { p.text };
                ui.label(
                    RichText::new(word(*value))
                        .font(theme::strong(16.0))
                        .color(ink),
                );
                ui.label(
                    RichText::new(format!("{:.0} %", *value * 100.0))
                        .font(FontId::monospace(11.0))
                        .color(p.weak),
                );
                ui.add_space(6.0);
                ui.label(RichText::new(what).size(11.0).color(p.weak));
            });
        });
    out
}

/// A large knob from 0 to 1, its usual dose (0.5) marked; drag, wheel, double-click to reset.
fn big_knob(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    value: f32,
    tint: Color32,
    diameter: f32,
) -> (f32, Response) {
    use std::f32::consts::PI;
    let (rect, _) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::hover());
    let response = ui.interact(
        rect,
        ui.scope_id().with(("knob", id)),
        Sense::click_and_drag(),
    );
    let fine = ui.input(|i| i.modifiers.shift);
    let mut v = value;
    if response.dragged() {
        let d = response.drag_delta();
        v += (d.x - d.y) / 240.0 * if fine { 0.2 } else { 1.0 };
    }
    if response.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            v += wheel.signum() * if fine { 0.005 } else { 0.02 };
        }
    }
    if response.double_clicked() {
        v = 0.5;
    }
    let v = v.clamp(0.0, 1.0);
    let painter = ui.painter();
    let c = rect.center();
    let r = diameter * 0.5;
    let angle = |f: f32| PI * 0.75 + PI * 1.5 * f.clamp(0.0, 1.0);
    let at = |a: f32, radius: f32| pos2(c.x + radius * a.cos(), c.y + radius * a.sin());
    let arc = |from: f32, to: f32, radius: f32| -> Vec<egui::Pos2> {
        let n = ((to - from).abs() / 0.05).ceil().max(1.0) as usize;
        (0..=n)
            .map(|i| at(from + (to - from) * i as f32 / n as f32, radius))
            .collect()
    };
    // The scale, the usual dose a longer mark.
    for k in 0..=10 {
        let a = angle(k as f32 / 10.0);
        let long = k == 5;
        painter.line_segment(
            [at(a, r - 1.0), at(a, r - if long { 9.0 } else { 5.0 })],
            Stroke::new(
                if long { 2.0 } else { 1.0 },
                p.weak.gamma_multiply(if long { 0.9 } else { 0.5 }),
            ),
        );
    }
    let ring = r - 15.0;
    painter.add(egui::Shape::line(
        arc(angle(0.0), angle(1.0), ring),
        Stroke::new(6.0, p.base),
    ));
    if v > 0.005 {
        let lit = arc(angle(0.0), angle(v), ring);
        painter.add(egui::Shape::line(
            lit.clone(),
            Stroke::new(14.0, tint.gamma_multiply(0.16)),
        ));
        painter.add(egui::Shape::line(lit, Stroke::new(6.0, tint)));
    }
    let active = response.hovered() || response.dragged();
    let body = r - 26.0;
    painter.circle_filled(
        c + vec2(0.0, 2.5),
        body + 1.0,
        Color32::from_black_alpha(90),
    );
    painter.circle_filled(c, body, if active { p.pressed } else { p.hover });
    crate::gear::glow(
        painter,
        c - vec2(0.0, body * 0.35),
        body * 0.9,
        Color32::from_white_alpha(if p.dark { 26 } else { 60 }),
        Color32::TRANSPARENT,
    );
    painter.circle_stroke(c, body, Stroke::new(1.0, Color32::from_black_alpha(110)));
    let a = angle(v);
    painter.line_segment(
        [at(a, body * 0.3), at(a, body * 0.85)],
        Stroke::new(3.0, p.text),
    );
    let response = response
        .on_hover_text(tr(
            "Drag up or down (Shift: finely), double-click for the usual dose",
        ))
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    (v, response)
}

/// What the tracks prepared receive in all, by kind of step; the tracks whose instrument is
/// to choose first. Returns the track to show when that warning is clicked.
fn summary(
    ui: &mut Ui,
    p: &Palette,
    rows: &[Row],
    plan: &[Planned],
    headroom: Option<String>,
) -> Option<usize> {
    let mut counts = [0usize; 12];
    let mut left_out = 0;
    let mut unknown = Vec::new();
    for (k, row) in rows.iter().enumerate() {
        let State::Ready(r) = &row.state else {
            left_out += 1;
            continue;
        };
        if !row.included {
            left_out += 1;
            continue;
        }
        if r.guess == Guess::Unknown {
            unknown.push(k);
        }
        for step in chosen(r) {
            let k = match step {
                Step::Filters { .. } => 0,
                Step::Resonance(_) => 1,
                Step::Plosives(_) => 2,
                Step::DeEss { .. } => 3,
                Step::Gate { .. } => 4,
                Step::Eq { .. } => 5,
                Step::Compressor { .. } => 6,
                Step::Transient { .. } => 7,
                Step::Ride(_) => 8,
                Step::StripSilence(_) => continue,
            };
            counts[k] += 1;
        }
    }
    for (_, prep, send) in plan {
        counts[9] += usize::from(prep.console.is_some());
        counts[10] += usize::from(send.is_some());
        counts[11] += usize::from(prep.pan.is_some_and(|p| p.abs() > 0.005));
    }
    let names = [
        tr("Filters: {count}"),
        tr("Resonances: {count}"),
        tr("Plosives: {count}"),
        tr("De-essers: {count}"),
        tr("Gates: {count}"),
        tr("EQ moves: {count}"),
        tr("Compressors: {count}"),
        tr("Transient shapers: {count}"),
        tr("Level riding: {count}"),
        tr("Console colors: {count}"),
        tr("Sent to the space: {count}"),
        tr("Spread in pairs: {count}"),
    ];
    let mut open = None;
    let mut pills: Vec<(String, Color32)> = Vec::new();
    for (k, (count, name)) in counts.iter().zip(names).enumerate() {
        if *count > 0 {
            let tint = match k {
                0..5 => CUT,
                9 => crate::console_ui::color(ConsoleModel::Iron),
                10 => SPACE,
                11 => WIDE,
                _ => p.accent,
            };
            pills.push((
                crate::i18n::fill(name, &[("count", count.to_string())]),
                tint,
            ));
        }
    }
    let staged = plan.iter().filter(|x| x.1.trim.is_some()).count();
    if staged > 0 {
        pills.insert(0, (trf!("Input gains set: {count}", count = staged), CUT));
    }
    if let Some(text) = headroom {
        pills.insert(0, (text, p.accent));
    }
    if left_out > 0 {
        pills.push((trf!("Left as they are: {count}", count = left_out), p.weak));
    }
    // One line: what does not fit is listed under "+N".
    let font = FontId::proportional(11.5);
    let painter = ui.painter().clone();
    let width_of = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), p.text)
            .size()
            .x
            + 36.0
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut room = ui.available_width() - 70.0;
        if let Some(&first) = unknown.first() {
            let text = trf!("Instrument to choose: {count}", count = unknown.len());
            room -= width_of(&text);
            if pill(ui, p, &text, WARN)
                .on_hover_text(tr(
                    "Their names do not tell the instrument: click to choose it",
                ))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some(first);
            }
        }
        let mut rest = Vec::new();
        for (text, tint) in &pills {
            let w = width_of(text);
            if rest.is_empty() && w <= room {
                room -= w;
                pill(ui, p, text, *tint);
            } else {
                rest.push(text.as_str());
            }
        }
        if !rest.is_empty() {
            pill(ui, p, &format!("+{}", rest.len()), p.weak).on_hover_text(rest.join("\n"));
        }
        if pills.is_empty() && unknown.is_empty() {
            ui.label(
                RichText::new(tr("Nothing to do: the tracks are fine as they are.")).color(p.weak),
            );
        }
    });
    open
}

/// A count of the summary, its dot telling the part.
fn pill(ui: &mut Ui, p: &Palette, text: &str, tint: Color32) -> Response {
    let font = FontId::proportional(11.5);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), p.text)
        .size()
        .x
        + 30.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    let painter = ui.painter();
    painter.rect(
        rect,
        12.0,
        field_fill(p),
        Stroke::new(1.0, line_edge(p)),
        StrokeKind::Inside,
    );
    painter.circle_filled(pos2(rect.left() + 12.0, rect.center().y), 3.5, tint);
    painter.text(
        pos2(rect.left() + 21.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        p.text,
    );
    response
}

/// The track being rendered and measured.
fn waiting(ui: &mut Ui, p: &Palette, progress: f32) {
    let height = ui.available_height().clamp(160.0, 300.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
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
        Step::Transient { attack, sustain } => (
            trf!(
                "Transients: attack {attack} %, sustain {sustain} %",
                attack = format!("{attack:+.0}"),
                sustain = format!("{sustain:+.0}")
            ),
            if *sustain < 0.0 {
                tr("Strada-Transient: more punch, a shorter ring").to_owned()
            } else {
                tr("Strada-Transient: more punch on soft hits").to_owned()
            },
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
            ui.spacing_mut().item_spacing.y = 6.0;
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
                        let ink = if c.on { p.text } else { p.weak };
                        ui.add(egui::Label::new(RichText::new(what).color(ink)).truncate())
                            .on_hover_text(why);
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
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
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
    for (text, color) in [(tr("Measured"), p.weak), (tr("After"), p.accent)] {
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
        .add_enabled(prepared, egui::Button::new(tr("Remove the Express Mix")))
        .clicked()
    {
        let tracks = tracks.to_vec();
        doc.history_mut().edit(tr("Remove the Express Mix"), |p| {
            for t in tracks {
                write::unprepare(p, t);
            }
            write::tidy_space(p);
        });
        ui.close();
    }
    let bypassed = is_bypassed(doc.project(), tracks);
    let label = if bypassed {
        tr("Turn the Express Mix back on")
    } else {
        tr("Bypass the Express Mix")
    };
    if ui
        .add_enabled(prepared, egui::Button::new(label))
        .on_hover_text(tr(
            "Compares with the track as it was, without losing anything",
        ))
        .clicked()
    {
        set_bypassed(doc, tracks, !bypassed);
        ui.close();
    }
}

/// Some of `tracks` have their preparation bypassed.
fn is_bypassed(project: &Project, tracks: &[TrackId]) -> bool {
    tracks.iter().any(|t| {
        project
            .track(*t)
            .and_then(|t| t.prepared.as_ref())
            .is_some_and(|p| p.bypassed.is_some())
    })
}

/// Bypasses the preparation of `tracks`, or puts it back, in one undo step.
fn set_bypassed(doc: &mut Document, tracks: &[TrackId], on: bool) {
    let label = if on {
        tr("Bypass the Express Mix")
    } else {
        tr("Turn the Express Mix back on")
    };
    doc.history_mut().edit(label, |p| {
        for &t in tracks {
            write::set_bypassed(p, t, on);
        }
    });
}

/// The preparation's submenu, in a track's menu.
pub fn menu(ui: &mut Ui, doc: &mut Document, tracks: &[TrackId]) {
    ui.menu_button(tr("Express Mix"), |ui| menu_items(ui, doc, tracks));
}

/// A mixer strip's line of the preparation: the button preparing the track (lit once it is
/// prepared; right-click for the menu) and the bypass of what it did, beside it.
pub fn strip_line(ui: &mut Ui, doc: &mut Document, track: TrackId, width: f32) {
    let p = theme::current(ui);
    let prepared = doc.project().track(track).and_then(|t| t.prepared.clone());
    if prepared
        .as_ref()
        .is_some_and(|d| d.instrument == write::SPACE)
    {
        return;
    }
    let done = prepared.as_ref().filter(|d| d.cleaned || d.brought_out);
    let bypassed = done.is_some_and(|d| d.bypassed.is_some());
    let tracks = targets(doc, track);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let (rect, response) = ui.allocate_exact_size(vec2(width - 20.0, 17.0), Sense::click());
        let hovered = response.hovered();
        let fill = match (done.is_some(), bypassed) {
            (true, false) => theme::mix(p.surface, p.accent, if hovered { 0.75 } else { 0.6 }),
            (true, true) => theme::mix(p.surface, p.accent, 0.2),
            (false, _) if hovered => p.hover,
            (false, _) => p.surface,
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            3.0,
            fill,
            Stroke::new(1.0, theme::mix(p.surface, p.accent, 0.5)),
            StrokeKind::Inside,
        );
        let ink = match (done.is_some(), bypassed) {
            (true, false) => theme::text_on(fill),
            (true, true) => p.weak,
            (false, _) => p.text,
        };
        paint_wand(
            painter,
            Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), vec2(11.0, 11.0)),
            ink,
        );
        let text = match (done.is_some(), bypassed) {
            (true, false) => tr("Express Mix"),
            (true, true) => tr("Bypassed"),
            (false, _) => tr("Express Mix"),
        };
        painter.with_clip_rect(rect.shrink(1.0)).text(
            pos2(rect.left() + 19.0, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            theme::strong(10.0),
            ink,
        );
        let hint = match done {
            Some(done) => {
                let instrument = Instrument::from_id(&done.instrument).unwrap_or(Instrument::Other);
                let what = match (done.cleaned, done.brought_out) {
                    (true, true) => tr("cleaned up and brought out"),
                    (true, false) => tr("cleaned up"),
                    _ => tr("brought out"),
                };
                trf!(
                    "Express Mix as: {instrument}, {what}. Click to redo it, right-click to remove it",
                    instrument = tr(instrument.name()),
                    what = what
                )
            }
            None => {
                tr("Express Mix: clean the track up, bring it out (right-click for more)").to_owned()
            }
        };
        let response = response
            .on_hover_text(hint)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
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
                    trf!("Express Mix: {count} tracks", count = tracks.len())
                } else {
                    trf!("Express Mix: “{name}”", name = name)
                };
                ui.label(RichText::new(title).small().weak());
                menu_items(ui, doc, &tracks);
            });
        // The bypass: lit while what the preparation did is switched off.
        let (rect, response) = ui.allocate_exact_size(vec2(18.0, 17.0), Sense::click());
        let enabled = done.is_some();
        let fill = if bypassed {
            theme::MUTE
        } else if enabled && response.hovered() {
            p.hover
        } else {
            p.surface
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            3.0,
            fill,
            Stroke::new(1.0, line_edge(&p)),
            StrokeKind::Inside,
        );
        let ink = if bypassed {
            theme::text_on(theme::MUTE)
        } else if enabled {
            p.text
        } else {
            p.weak.gamma_multiply(0.5)
        };
        paint_power(painter, rect.center(), 4.5, ink);
        if enabled {
            let hint = if bypassed {
                tr("The Express Mix is bypassed: click to hear it again")
            } else {
                tr("Bypass the Express Mix, to compare with the track as it was")
            };
            if response
                .on_hover_text(hint)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                let prepared: Vec<TrackId> = tracks
                    .iter()
                    .copied()
                    .filter(|t| {
                        doc.project()
                            .track(*t)
                            .is_some_and(|t| t.prepared.is_some())
                    })
                    .collect();
                set_bypassed(doc, &prepared, !bypassed);
            }
        }
    });
}

/// A wand with its sparkles, in `rect`.
fn paint_wand(painter: &egui::Painter, rect: Rect, ink: Color32) {
    let stroke = Stroke::new(1.2, ink);
    let (a, b) = (
        rect.left_bottom() + vec2(1.0, -1.0),
        rect.center() + vec2(1.0, -1.0),
    );
    painter.line_segment([a, b], stroke);
    for (c, r) in [
        (vec2(3.5, -3.5), 1.8),
        (vec2(-1.0, -4.0), 1.0),
        (vec2(4.0, 1.0), 1.0),
    ] {
        let c = rect.center() + c;
        painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
    }
}

/// A power symbol: an open ring and its bar.
fn paint_power(painter: &egui::Painter, c: egui::Pos2, r: f32, ink: Color32) {
    use std::f32::consts::PI;
    let stroke = Stroke::new(1.3, ink);
    let points: Vec<egui::Pos2> = (0..=20)
        .map(|i| {
            let a = -PI / 2.0 + 0.6 + (2.0 * PI - 1.2) * i as f32 / 20.0;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
    painter.line_segment([c - vec2(0.0, r + 1.0), c - vec2(0.0, 1.0)], stroke);
}

/// The mixer's button preparing every track at once (not the buses nor the VCAs).
pub fn all_button(ui: &mut Ui, doc: &Document) {
    let p = theme::current(ui);
    let button = egui::Button::new(RichText::new(tr("Express Mix: all tracks")).strong())
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
