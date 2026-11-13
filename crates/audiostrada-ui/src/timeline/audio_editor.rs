//! The audio editor: a clip in a large window, laid out as the other editors. It opens on the
//! waveform alone. The chips above it show what the analysis found, each one a layer to switch
//! on: the phrases and the background between them, breaths, sibilants, clicks, clipped
//! peaks, and the hits. Everything is retouched on the waveform: a level line to pull, an edge
//! to move, a pause to squeeze, a hit to slide onto the track drawn under the clip. Every
//! retouch stays in the clip's stack, switched, dosed or taken back at any time.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use audiostrada_core::SourceId;
use audiostrada_core::audio_edit::{
    AudioEdits, Entry, Kind, Level, Moved, REMOVED_DB, Region, Span, Timing, map_through,
    unmap_through,
};
use audiostrada_dsp::analysis::{self, Onsets};
use audiostrada_dsp::retouch::{self, Profile};

use super::*;
use crate::editor_frame::{self, Tool as FrameTool, ViewButton, Zoomed};
use crate::transport::EditSettings;

/// The tools of the audio editor, in the bar's order.
pub(super) const AUDIO_TOOLS: [FrameTool; 2] = [FrameTool::Select, FrameTool::Erase];

const RULER: f32 = 34.0;
/// Height of the row of chips above the waveform, and of the line of help under it.
const CHIPS: f32 = 36.0;
const STATUS: f32 = 24.0;
/// How near an edge or a line the pointer grabs it, in pixels.
const EDGE: f32 = 5.0;
const LINE: f32 = 6.0;
/// The level lines: how far a part is raised at the top, and lowered just above the bottom,
/// where it is removed.
const HIGHEST: f32 = 18.0;
const DEEPEST: f32 = -48.0;
/// Where a line rests while its part plays as recorded, from the top of the waveform.
const REST: f32 = 0.28;
/// A part lowered this far is removed.
const GONE: f32 = -60.0;
/// The level at the bottom of the waveform when the threshold is shown, dBFS.
const LOWEST: f32 = -96.0;
/// The shortest a pause is squeezed to, seconds.
const SHORTEST: f32 = 0.08;
/// How far a hit looks for another track's hit to snap onto, seconds.
const SNAP_REACH: f32 = 0.07;

/// Everything found in a source once: its profile, its clicks and clipped peaks, its attacks.
#[derive(Debug)]
pub(super) struct Found {
    profile: Profile,
    clicks: Vec<(usize, usize)>,
    clipping: Vec<(usize, usize)>,
    onsets: Onsets,
    /// The loudest sample: the waveform is drawn to fill its band.
    peak: f32,
}

/// Parts of a source, in frames.
type Ranges = Vec<retouch::Range>;

/// The analyses kept for the sources edited, by source.
pub(super) type FoundBySource = HashMap<SourceId, Arc<Found>>;

/// What the analysis looks for, as the chips above the waveform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Sort {
    Phrases,
    Background,
    Breaths,
    Sibilants,
    Clicks,
    Clipping,
    Hits,
}

impl Sort {
    const ALL: [Sort; 7] = [
        Sort::Phrases,
        Sort::Background,
        Sort::Breaths,
        Sort::Sibilants,
        Sort::Clicks,
        Sort::Clipping,
        Sort::Hits,
    ];

    fn name(self) -> &'static str {
        match self {
            Sort::Phrases => tr("Phrases"),
            Sort::Background => tr("Background"),
            Sort::Breaths => tr("Breaths"),
            Sort::Sibilants => tr("Sibilants"),
            Sort::Clicks => tr("Clicks"),
            Sort::Clipping => tr("Clipping"),
            Sort::Hits => tr("Hits"),
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Sort::Phrases => tr(
                "Each phrase with its level line: pull one up or down, or bring them all to \
                 the same level from the menu",
            ),
            Sort::Background => tr(
                "What lies between the sounds: noise, leaking headphones, another instrument, \
                 the room. Pull its line down to lower or remove it, up to bring it out; \
                 squeeze a pause by its handle",
            ),
            Sort::Breaths => tr("The breath before each phrase. Pull a line down to soften them"),
            Sort::Sibilants => tr("Each S too sharp. Pull a line down: only its highs soften"),
            Sort::Clicks => tr("Mouth or cable clicks. Click a mark to repair it"),
            Sort::Clipping => tr("Peaks cut flat by a level too high. Click a mark to redraw it"),
            Sort::Hits => tr(
                "Each note or hit by its attack. Slide one sideways to move it in time: it \
                 snaps to the hits of the track under the clip",
            ),
        }
    }

    fn color(self) -> Color32 {
        match self {
            Sort::Phrases => Color32::from_rgb(120, 205, 150),
            Sort::Background => Color32::from_rgb(150, 160, 180),
            Sort::Breaths => Color32::from_rgb(80, 190, 235),
            Sort::Sibilants => Color32::from_rgb(235, 200, 70),
            Sort::Clicks => Color32::from_rgb(240, 85, 85),
            Sort::Clipping => Color32::from_rgb(245, 150, 60),
            Sort::Hits => Color32::from_rgb(190, 150, 250),
        }
    }

    /// Whether it is shown as regions with a level line (else as marks, or hits).
    fn leveled(self) -> bool {
        matches!(
            self,
            Sort::Phrases | Sort::Background | Sort::Breaths | Sort::Sibilants
        )
    }

    /// Whether a line pulled takes the others of its sort along (Alt: that one alone).
    fn together(self) -> bool {
        matches!(self, Sort::Background | Sort::Breaths | Sort::Sibilants)
    }

    /// The highest its line goes, dB.
    fn ceiling(self) -> f32 {
        match self {
            Sort::Breaths | Sort::Sibilants => 0.0,
            _ => HIGHEST,
        }
    }

    /// Whether `kind` is its retouch in the stack.
    fn is(self, kind: &Kind) -> bool {
        matches!(
            (self, kind),
            (Sort::Phrases, Kind::Phrases { .. })
                | (Sort::Background, Kind::Background { .. })
                | (Sort::Breaths, Kind::Breaths { .. })
                | (Sort::Sibilants, Kind::Sibilants { .. })
                | (Sort::Clicks, Kind::Clicks { .. })
                | (Sort::Clipping, Kind::Clipping { .. })
                | (Sort::Hits, Kind::Moves { .. })
        )
    }

    fn kind(self, regions: Vec<Region>, db: f32, threshold_db: f32) -> Kind {
        match self {
            Sort::Phrases => Kind::Phrases { regions },
            Sort::Background => Kind::Background {
                regions,
                db,
                threshold_db,
            },
            Sort::Breaths => Kind::Breaths { regions, db },
            Sort::Sibilants => Kind::Sibilants { regions, db },
            Sort::Clicks => Kind::Clicks { regions },
            Sort::Clipping => Kind::Clipping { regions },
            Sort::Hits => Kind::Moves { hits: Vec::new() },
        }
    }
}

/// What owns a set of level lines.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    /// The regions of a sort, found or already retouched.
    Sort(Sort),
    /// A part retouched by hand: its retouch in the stack.
    Part(usize),
    /// The span selected, not retouched yet.
    Selection,
}

/// A set of regions shown with level lines.
struct Shown {
    owner: Owner,
    regions: Vec<Region>,
    /// The level they get, dB (each region may have its own).
    gain: f32,
    /// In the stack already and switched on (else only found).
    applied: bool,
    color: Color32,
}

impl Shown {
    /// Whether its lines move together unless Alt is held.
    fn together(&self) -> bool {
        match self.owner {
            Owner::Sort(sort) => sort.together(),
            Owner::Part(_) => self.regions.len() > 1,
            Owner::Selection => false,
        }
    }

    fn ceiling(&self) -> f32 {
        match self.owner {
            Owner::Sort(sort) => sort.ceiling(),
            _ => HIGHEST,
        }
    }
}

/// What lies under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Under {
    Line(usize, usize),
    Edge(usize, usize, bool),
    Pause(usize),
    /// A mark of clicks or clipped peaks, by its place in the marks drawn.
    Mark(usize),
    Threshold,
    /// A hit, by its place among those of the clip.
    Attack(usize),
    Region(usize, usize),
    Nothing,
}

/// A gesture under way.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    Select(u64),
    /// A level line: the set, the region grabbed, alone or with the others, the level now.
    Level {
        shown: usize,
        region: usize,
        alone: bool,
        gain: f32,
    },
    Edge {
        shown: usize,
        region: usize,
        start: bool,
        to: u64,
    },
    /// A pause squeezed to `keep` frames, grabbed at `from` (timeline sample).
    Pause {
        region: usize,
        alone: bool,
        keep: u64,
        from: u64,
    },
    Threshold(f32),
    /// A hit slid in time: found at `at`, playing at `to`; the pointer grabbed it `grab`
    /// samples after its attack. `snapped` once a hit of the other track or the grid holds it.
    Hit {
        at: u64,
        to: u64,
        grab: i64,
        snapped: bool,
    },
    Move {
        grab: u64,
        moved: i64,
    },
    Erase(u64),
}

/// What a right-click menu asks.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuAction {
    ToVoice,
    Normalize,
    Lower,
    Silence,
    Reset(usize, usize),
    Remove(usize, usize),
    Aside(usize, usize),
    HitBack(u64),
    HitSnap(u64),
}

/// What a chip's menu asks.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ChipAction {
    /// The level of every region of the sort, dB.
    Dose(f32),
    Repair,
    /// Every phrase to the same level.
    Even,
    /// Every pause to this length, seconds.
    Shorten(f32),
    PausesBack,
    /// Every hit onto the track under the clip, or onto the grid.
    SnapUnder,
    SnapGrid,
    HitsBack,
    Sensitivity(f32),
    Switch,
    TakeBack,
}

#[derive(Debug)]
pub(super) struct AudioEditor {
    clip: ClipId,
    zoom: f64,
    scroll: f64,
    tool: FrameTool,
    selection: Option<(u64, u64)>,
    drag: Option<Drag>,
    zoom_from: Option<f32>,
    fit_asked: bool,
    fitted: bool,
    /// The sorts shown on the waveform: none at first.
    shown: Vec<Sort>,
    /// The analysis of the clip's source, while it runs.
    pending: Option<(SourceId, Receiver<Found>)>,
    /// The threshold between sound and background, dBFS, once moved; the analysis proposes
    /// one.
    threshold: Option<f32>,
    /// How readily an attack counts as a hit, 0 to 1.
    sensitivity: f32,
    /// Regions found but set aside by hand.
    ignored: Vec<(Sort, Span)>,
    under: Option<TrackId>,
    /// What a right click was on, and where.
    menu: Option<(Under, u64)>,
    /// Pressed on the ruler: whether it was playing before.
    scrub: Option<bool>,
    /// What was found at a threshold, kept while it does not change.
    cache: Option<Parts>,
    /// The hits found at a sensitivity, in source frames.
    hits: Option<(f32, Arc<Vec<u64>>)>,
    /// The waveform taller or lower than its fit, by Alt and the wheel.
    wave: f32,
    /// The clip as last drawn: shown while a new retouch is being made ready.
    drawn: Option<audiostrada_core::Clip>,
    /// What the pointer is over, told at the right of the chips.
    hint: Option<&'static str>,
    /// What a menu asked, done once the frame knows the hits and the regions.
    asked: Option<(Sort, ChipAction)>,
    /// Ctrl+A pressed: the whole clip is selected.
    select_all: bool,
}

/// What the profile gives at a threshold.
#[derive(Clone, Debug, Default)]
struct Parts {
    threshold: f32,
    phrases: Ranges,
    background: Ranges,
    breaths: Ranges,
}

impl AudioEditor {
    fn new(clip: ClipId) -> Self {
        Self {
            clip,
            zoom: 400.0,
            scroll: 0.0,
            tool: FrameTool::Select,
            selection: None,
            drag: None,
            zoom_from: None,
            fit_asked: false,
            fitted: false,
            shown: Vec::new(),
            pending: None,
            threshold: None,
            sensitivity: 0.5,
            ignored: Vec::new(),
            under: None,
            menu: None,
            scrub: None,
            cache: None,
            hits: None,
            wave: 1.0,
            drawn: None,
            hint: None,
            asked: None,
            select_all: false,
        }
    }

    // TEMP-SHOT-BEGIN
    fn temp_setup(&mut self) {
        if let Ok(v) = std::env::var("AE_SHOW") {
            for name in v.split(',') {
                let sort = match name {
                    "phrases" => Sort::Phrases,
                    "background" => Sort::Background,
                    "breaths" => Sort::Breaths,
                    "sibilants" => Sort::Sibilants,
                    "clicks" => Sort::Clicks,
                    "clipping" => Sort::Clipping,
                    "hits" => Sort::Hits,
                    _ => continue,
                };
                self.show(sort);
            }
        }
    }
    // TEMP-SHOT-END

    fn shows(&self, sort: Sort) -> bool {
        self.shown.contains(&sort)
    }

    fn show(&mut self, sort: Sort) {
        if !self.shows(sort) {
            self.shown.push(sort);
        }
    }
}

/// Between the timeline and the frames of the clip's source before its edits.
struct Map {
    start: u64,
    offset: u64,
    end: u64,
    per: f64,
    timing: Timing,
}

impl Map {
    fn new(project: &Project, clip: &audiostrada_core::Clip) -> Self {
        Self {
            start: clip.start,
            offset: clip.offset,
            end: clip.end(),
            per: edit::played_per_source(project, clip),
            timing: clip
                .edits
                .as_ref()
                .map(|e| e.timing())
                .unwrap_or_default(),
        }
    }

    /// The timeline sample where a frame of the edited audio plays.
    fn played(&self, frame: u64) -> i64 {
        self.start as i64 + (frame as f64 * self.per) as i64 - self.offset as i64
    }

    /// The timeline sample where a source frame plays.
    fn sample(&self, frame: u64) -> i64 {
        self.played(self.timing.map(frame))
    }

    /// The source frame playing at a timeline sample.
    fn frame(&self, sample: u64) -> u64 {
        let played = (sample as i64 - self.start as i64 + self.offset as i64).max(0);
        self.timing.unmap((played as f64 / self.per) as u64)
    }

    /// The timeline sample of a place in the audio once its hits are moved, where a hit's
    /// `to` is counted.
    fn sample_moved(&self, place: u64) -> i64 {
        self.played(map_through(self.timing.cuts(), place))
    }

    /// The place, in the audio once its hits are moved, playing at a timeline sample.
    fn moved(&self, sample: i64) -> u64 {
        let played = (sample - self.start as i64 + self.offset as i64).max(0);
        unmap_through(self.timing.cuts(), (played as f64 / self.per) as u64)
    }

    /// The source frames the clip plays.
    fn window(&self) -> (u64, u64) {
        (self.frame(self.start), self.frame(self.end))
    }
}

/// What the gestures need of the frame being drawn.
struct Gesture<'a> {
    band: Rect,
    lane: Rect,
    scale: Scale,
    map: &'a Map,
    sets: &'a [Shown],
    /// Per set and region: its gain and the height of its line.
    lines: &'a [Vec<(f32, f32)>],
    guides: &'a [u64],
    marks: &'a [Mark],
    /// The hits of the clip where they play, and those found in its source.
    hits: &'a [Moved],
    found_hits: &'a [u64],
    parts: Option<&'a Parts>,
    threshold: f32,
    /// Frames of the source, and its rate.
    frames: u64,
    rate: u32,
    settings: &'a EditSettings,
}

/// A mark of clicks or clipped peaks on the waveform: one, or several close together.
#[derive(Clone, Copy, Debug)]
struct Mark {
    sort: Sort,
    /// The first one, among those found.
    first: usize,
    count: usize,
    left: f32,
    right: f32,
    /// All repaired.
    fixed: bool,
    /// Where they lie, source frames.
    start: u64,
    last: u64,
}

impl Mark {
    fn at(&self, band: Rect) -> Pos2 {
        pos2((self.left + self.right) / 2.0, band.top() + 9.0)
    }
}

/// The part of the band the waveform and its level lines take.
fn wave_of(band: Rect) -> Rect {
    band.shrink2(vec2(0.0, 16.0))
}

/// The height of a level line: at rest for 0 dB, up to raise, down to lower, at the bottom
/// once removed.
fn y_of_gain(band: Rect, gain: f32) -> f32 {
    let wave = wave_of(band);
    let t = if gain <= GONE {
        1.0
    } else if gain >= 0.0 {
        REST * (1.0 - (gain / HIGHEST).min(1.0))
    } else {
        REST + (1.0 - REST) * 0.94 * (gain / DEEPEST).min(1.0)
    };
    wave.top() + t * wave.height()
}

/// The level a line gives at a height; back to 0 dB within half a dB of it.
fn gain_at(band: Rect, y: f32) -> f32 {
    let wave = wave_of(band);
    let t = ((y - wave.top()) / wave.height().max(1.0)).clamp(0.0, 1.0);
    let gain = if t <= REST {
        HIGHEST * (1.0 - t / REST)
    } else {
        let down = (t - REST) / ((1.0 - REST) * 0.94);
        if down > 1.0 {
            return GONE - 0.1;
        }
        DEEPEST * down
    };
    if gain.abs() < 0.5 { 0.0 } else { gain }
}

/// The height of a level in dBFS, when the threshold is shown against the clip's level.
fn y_of_level(band: Rect, db: f32) -> f32 {
    let wave = wave_of(band);
    let t = (db.clamp(LOWEST, 0.0) / LOWEST).clamp(0.0, 1.0);
    wave.top() + t * wave.height()
}

/// A gain as written: removed at the bottom of its range.
fn stored(gain: f32) -> f32 {
    if gain <= GONE { REMOVED_DB } else { gain }
}

fn gain_text(gain: f32) -> String {
    if gain <= GONE {
        tr("Removed").to_owned()
    } else if gain.abs() < 0.05 {
        "0 dB".to_owned()
    } else {
        format!("{gain:+.1} dB")
    }
}

fn ms_text(frames: i64, rate: u32) -> String {
    format!("{:+.0} ms", frames as f64 * 1000.0 / f64::from(rate.max(1)))
}

/// The regions of a sort found by the analysis inside `window`, without those set aside.
fn found_regions(
    found: &Found,
    sort: Sort,
    parts: &Parts,
    window: (u64, u64),
    ignored: &[(Sort, Span)],
) -> Vec<Region> {
    let list: Vec<(usize, usize)> = match sort {
        Sort::Phrases => parts.phrases.clone(),
        Sort::Background => parts.background.clone(),
        Sort::Breaths => parts.breaths.clone(),
        Sort::Sibilants => found.profile.sibilants(),
        Sort::Clicks => found.clicks.clone(),
        Sort::Clipping => found.clipping.clone(),
        Sort::Hits => Vec::new(),
    };
    list.into_iter()
        .filter_map(|(a, b)| {
            let (start, end) = ((a as u64).max(window.0), (b as u64).min(window.1));
            (end > start).then(|| Region::new(start, end))
        })
        .filter(|r| {
            !ignored
                .iter()
                .any(|(s, span)| *s == sort && span.start < r.end && r.start < span.end)
        })
        .collect()
}

/// `new` regions keeping what `old` ones set apart where they overlap.
fn carry_regions(old: &[Region], mut new: Vec<Region>) -> Vec<Region> {
    for r in &mut new {
        if let Some(o) = old.iter().find(|o| o.start < r.end && r.start < o.end) {
            r.db = o.db;
            r.keep = o.keep;
        }
    }
    new
}

/// The level that brings each phrase to the loudness they share: the middle one, no phrase
/// moved by more than 12 dB.
fn evened(profile: &Profile, mut regions: Vec<Region>) -> Vec<Region> {
    let levels: Vec<f32> = regions
        .iter()
        .map(|r| profile.loudness((r.start as usize, r.end as usize)))
        .collect();
    let mut sorted = levels.clone();
    sorted.sort_by(f32::total_cmp);
    let Some(&middle) = sorted.get(sorted.len() / 2) else {
        return regions;
    };
    for (r, level) in regions.iter_mut().zip(levels) {
        let gain = (middle - level).clamp(-12.0, 12.0);
        r.db = (gain.abs() >= 0.5).then_some((gain * 10.0).round() / 10.0);
    }
    regions
}

/// The hits of the clip as they stand, in order: those found, and those of the stack where
/// it moved them.
fn placed_hits(found: &[u64], edits: &AudioEdits, window: (u64, u64), near: u64) -> Vec<Moved> {
    // What only holds the audio in place (its end, a hit no longer found) is not shown.
    let mut out: Vec<Moved> = edits
        .anchors()
        .into_iter()
        .map(|(at, to)| Moved { at, to })
        .filter(|m| m.at != m.to || found.iter().any(|f| f.abs_diff(m.at) <= near))
        .collect();
    let stack = out.clone();
    out.extend(
        found
            .iter()
            .filter(|f| !stack.iter().any(|m| m.at.abs_diff(**f) <= near))
            .map(|&f| Moved { at: f, to: f }),
    );
    out.sort_by_key(|m| m.at);
    // A hit found where one of the stack was moved past it gives way.
    let mut kept: Vec<Moved> = Vec::new();
    for m in out {
        if stack.iter().any(|s| s.at == m.at) {
            while kept.last().is_some_and(|l| l.to >= m.to) {
                kept.pop();
            }
            kept.push(m);
        } else if kept.last().is_none_or(|l| m.to > l.to) {
            kept.push(m);
        }
    }
    kept.retain(|m| m.at >= window.0 && m.at < window.1);
    kept
}

/// How far the hit found at `at` may go between its neighbours: the audio on each side
/// stretched to no more than twice its length, squeezed to no less than half.
fn room(before: Moved, at: u64, after: Moved) -> Option<(u64, u64)> {
    let (left, right) = (at.saturating_sub(before.at), after.at.saturating_sub(at));
    let low = (before.to + left / 2 + 1).max(after.to.saturating_sub(2 * right));
    let high = (before.to + 2 * left).min(after.to.saturating_sub(right / 2 + 1));
    (low <= high).then_some((low, high))
}

/// The neighbours of the hit `k` of `hits`: the start of the audio and its end when it has
/// none.
fn beside(hits: &[Moved], k: usize, frames: u64) -> (Moved, Moved) {
    let end = Moved {
        at: frames,
        to: frames,
    };
    (
        k.checked_sub(1)
            .and_then(|i| hits.get(i))
            .copied()
            .unwrap_or(Moved { at: 0, to: 0 }),
        hits.get(k + 1).copied().unwrap_or(end),
    )
}

/// Each hit brought onto the target `target_of` gives for where it plays, within `reach`
/// frames and the room its neighbours leave.
fn snapped_hits(
    hits: &[Moved],
    target_of: impl Fn(u64) -> Option<u64>,
    reach: u64,
    frames: u64,
) -> Vec<Moved> {
    let mut out = hits.to_vec();
    for k in 0..out.len() {
        let hit = out[k];
        let Some(target) = target_of(hit.to).filter(|t| t.abs_diff(hit.to) <= reach) else {
            continue;
        };
        let (before, after) = beside(&out, k, frames);
        // Not past the middle of the way to a neighbour: that target is the neighbour's.
        let near = (hit.to - before.to).min(after.to.saturating_sub(hit.to)) * 45 / 100;
        let fits = room(before, hit.at, after).is_some_and(|(a, b)| (a..=b).contains(&target));
        if target.abs_diff(hit.to) <= near && fits {
            out[k].to = target;
        }
    }
    out
}

/// The hits to write: those moved, and beside each the one (or the end of the audio) that
/// holds what lies beyond it in place.
fn with_holds(hits: &[Moved], frames: u64) -> Vec<Moved> {
    let mut out: Vec<Moved> = Vec::new();
    for (k, hit) in hits.iter().enumerate() {
        let moved = |i: usize| hits.get(i).is_some_and(|h| h.at != h.to);
        let holds = (k > 0 && moved(k - 1)) || moved(k + 1);
        if hit.at != hit.to || holds {
            out.push(*hit);
        }
    }
    if hits.last().is_some_and(|h| h.at != h.to) && frames > 1 {
        out.push(Moved {
            at: frames - 1,
            to: frames - 1,
        });
    }
    out
}

/// The level of `clips` from `from` (timeline samples), `len` blocks of `step` samples each
/// averaged, mixed to one channel.
fn track_audio(
    doc: &Document,
    project: &Project,
    clips: &[&audiostrada_core::Clip],
    (from, len, step): (u64, usize, u64),
) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    for clip in clips {
        let Some(data) = audiostrada_engine::project::clip_media(project, doc.paths(), clip)
            .and_then(|p| doc.media().original(&p).cloned())
        else {
            continue;
        };
        let ratio = f64::from(data.sample_rate()) / f64::from(project.sample_rate.max(1));
        let gain = 10f32.powf(clip.gain_db / 20.0) / data.channels().max(1) as f32;
        for (k, v) in out.iter_mut().enumerate() {
            let t = from + k as u64 * step;
            if t < clip.start || t + step > clip.end() {
                continue;
            }
            let first = ((t - clip.start + clip.offset) as f64 * ratio) as usize;
            let count = ((step as f64 * ratio) as usize).max(1);
            let mut sum = 0.0;
            for c in 0..data.channels() {
                let ch = data.channel(c);
                sum += ch[first.min(ch.len())..(first + count).min(ch.len())]
                    .iter()
                    .sum::<f32>();
            }
            *v += sum / count as f32 * gain;
        }
    }
    out
}

/// How `a` and `b` line up best within `lag` blocks: their correlation (-1 to 1) and by how
/// many blocks `b` comes after `a`.
fn line_up(a: &[f32], b: &[f32], lag: i64) -> Option<(f32, i64)> {
    let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
    if energy(a) < 1e-9 || energy(b) < 1e-9 {
        return None;
    }
    let mut best: Option<(f32, i64)> = None;
    for l in -lag..=lag {
        let (mut num, mut ea, mut eb) = (0.0f32, 0.0f32, 0.0f32);
        for (k, x) in a.iter().enumerate() {
            let j = k as i64 + l;
            if j < 0 || j as usize >= b.len() {
                continue;
            }
            let y = b[j as usize];
            num += x * y;
            ea += x * x;
            eb += y * y;
        }
        if ea > 0.0 && eb > 0.0 {
            let c = num / (ea * eb).sqrt();
            if best.is_none_or(|(b, _)| c.abs() > b.abs()) {
                best = Some((c, l));
            }
        }
    }
    best
}

/// The analysis of a source, on another thread.
fn analyse(data: Arc<audiostrada_media::AudioData>) -> Receiver<Found> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let rate = data.sample_rate();
        let channels: Vec<&[f32]> = (0..data.channels()).map(|c| data.channel(c)).collect();
        let found = Found {
            profile: retouch::profile(&channels, rate),
            clicks: retouch::find_clicks(&channels, rate, 0.5),
            clipping: retouch::find_clipping(&channels),
            onsets: analysis::detect_onsets(&channels, rate),
            peak: channels
                .iter()
                .flat_map(|c| c.iter())
                .fold(0.0f32, |m, v| m.max(v.abs())),
        };
        let _ = tx.send(found);
    });
    rx
}

/// A text beside the pointer, on a dark pill.
fn pointer_tag(painter: &egui::Painter, pos: Pos2, text: String) {
    let galley = painter.layout_no_wrap(text, crate::theme::strong(11.5), Color32::WHITE);
    let at = pos + vec2(14.0, -26.0);
    let rect = Rect::from_min_size(at, galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(rect, 5.0, Color32::from_black_alpha(210));
    painter.galley(at + vec2(6.0, 3.0), galley, Color32::WHITE);
}

/// A small text on a pill of `color`, centered at `at`.
fn tag(painter: &egui::Painter, at: Pos2, text: &str, color: Color32, ink: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(10.5), ink);
    let rect = Rect::from_center_size(at, galley.size() + vec2(10.0, 4.0));
    painter.rect_filled(rect, 7.0, color);
    painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
}

impl TimelineView {
    /// Opens the audio editor on `clip`, selecting it; the other editors close.
    pub(super) fn open_audio_editor(&mut self, clip: ClipId) {
        self.selection = vec![clip];
        if self.audio.as_ref().is_none_or(|a| a.clip != clip) {
            self.audio = Some(AudioEditor::new(clip));
            // TEMP-SHOT-BEGIN
            if let Some(a) = &mut self.audio {
                a.temp_setup();
            }
            // TEMP-SHOT-END
        }
        self.auto_editor = None;
        self.notes.open = false;
    }

    /// Opens the audio editor on the selected audio clip (else the one under the playhead on
    /// the track chosen, else its first, else the project's first), or closes it.
    pub(super) fn toggle_audio_editor(&mut self, doc: &Document) {
        if self.audio.take().is_some() {
            return;
        }
        let project = doc.project();
        let audio = |c: &&audiostrada_core::Clip| !c.is_midi();
        let position = doc.player().position();
        let clip = self
            .selection
            .iter()
            .filter_map(|c| edit::clip(project, *c))
            .find(audio)
            .or_else(|| {
                let track = doc.focused_track().and_then(|t| project.track(t))?;
                let clips: Vec<_> = track.clips.iter().filter(audio).collect();
                clips
                    .iter()
                    .find(|c| c.start <= position && position < c.end())
                    .or(clips.first())
                    .copied()
            })
            .or_else(|| project.tracks.iter().flat_map(|t| &t.clips).find(audio))
            .map(|c| c.id);
        if let Some(clip) = clip {
            self.open_audio_editor(clip);
        }
    }

    /// Whether the audio editor is open.
    #[cfg(test)]
    pub fn audio_editor_open(&self) -> bool {
        self.audio.is_some()
    }

    /// The keys of the audio editor, while it is open: its tools, the view's, and Delete
    /// removing what is selected or setting aside what the pointer is over.
    pub(super) fn take_audio_keys(&mut self, ctx: &egui::Context) {
        let Some(editor) = &mut self.audio else {
            return;
        };
        editor_frame::keep_escape(ctx, editor_frame::AUDIO);
        if let Some(tool) = editor_frame::take_tool_key(ctx, &AUDIO_TOOLS) {
            editor.tool = tool;
        }
        if editor_frame::take_view_keys(ctx, editor_frame::AUDIO) {
            editor.fit_asked = true;
        }
        if !ctx.text_edit_focused()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A))
        {
            editor.select_all = true;
        }
        if !ctx.text_edit_focused()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            })
        {
            self.audio_delete = true;
        }
    }

    /// The editor's window, while it is open; puts the clip back as it plays when it closes
    /// while heard as it was.
    pub(super) fn audio_window(
        &mut self,
        ctx: &egui::Context,
        doc: &mut Document,
        settings: &EditSettings,
    ) {
        let open_clip = self.audio.as_ref().map(|a| a.clip);
        if let Some((clip, edits)) = self.audio_before.take() {
            if open_clip == Some(clip) {
                self.audio_before = Some((clip, edits));
            } else {
                doc.history_mut()
                    .modify_unrecorded(|p| edit::set_clip_edits(p, clip, Some(edits)));
            }
        }
        let Some(mut editor) = self.audio.take() else {
            doc.keep_media(Vec::new());
            return;
        };
        let project = doc.project().clone();
        let Some(clip) = edit::clip(&project, editor.clip).cloned() else {
            doc.keep_media(Vec::new());
            return;
        };
        let Some(track) = edit::track_of_clip(&project, clip.id).and_then(|t| project.track(t))
        else {
            return;
        };
        self.audio_analysis(doc, &project, &clip, &mut editor);
        let title = format!("{} · {}: {}", tr("Audio"), track.name, clip.name);
        // Escape first lets go of a gesture, then of the selection.
        let mut escape = editor_frame::take_escape(ctx, editor_frame::AUDIO);
        if escape && (editor.drag.take().is_some() | editor.selection.take().is_some()) {
            escape = false;
        }
        let open = editor_frame::window(ctx, editor_frame::AUDIO, title, escape, |ui| {
            self.audio_editor(ui, doc, &project, &clip, &mut editor, settings);
        });
        // The clip as it plays, as it was and as last drawn stay loaded: switching is instant
        // and the waveform never blanks while a retouch is made ready.
        let now = edit::clip(doc.project(), clip.id).cloned();
        let edits = self
            .audio_before
            .as_ref()
            .map(|(_, e)| Arc::new(e.clone()))
            .or_else(|| now.as_ref().and_then(|c| c.edits.clone()));
        let kept: Vec<_> = now
            .into_iter()
            .flat_map(|c| {
                [
                    audiostrada_core::Clip {
                        edits: edits.clone(),
                        ..c.clone()
                    },
                    audiostrada_core::Clip { edits: None, ..c },
                ]
            })
            .chain(editor.drawn.clone())
            .filter_map(|c| audiostrada_engine::project::clip_media(doc.project(), doc.paths(), &c))
            .collect();
        doc.keep_media(kept);
        if open {
            self.audio = Some(editor);
        }
    }

    /// Starts the analysis of the clip's source if it is not known, takes it in when done.
    fn audio_analysis(
        &mut self,
        doc: &Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
    ) {
        if let Some((source, rx)) = &editor.pending
            && let Ok(found) = rx.try_recv()
        {
            self.audio_found.insert(*source, Arc::new(found));
            editor.pending = None;
            editor.cache = None;
            editor.hits = None;
        }
        if self.audio_found.contains_key(&clip.source)
            || editor.pending.as_ref().is_some_and(|p| p.0 == clip.source)
        {
            return;
        }
        if let Some(data) = project
            .sources
            .get(&clip.source)
            .and_then(|s| doc.media().original(&doc.resolve(&s.path)).cloned())
        {
            editor.pending = Some((clip.source, analyse(data)));
        }
    }

    /// The edits of the clip as they stand (those put aside while it is heard as it was).
    fn current_edits(&self, clip: &audiostrada_core::Clip) -> AudioEdits {
        match &self.audio_before {
            Some((id, edits)) if *id == clip.id => edits.clone(),
            _ => clip.edits.as_deref().cloned().unwrap_or_default(),
        }
    }

    /// Writes the clip's edits in one undo step, heard as they are from then on.
    fn commit_edits(
        &mut self,
        doc: &mut Document,
        clip: ClipId,
        mut edits: AudioEdits,
        label: &str,
    ) {
        self.audio_before = None;
        edits.entries.retain(|e| !e.kind.is_empty());
        doc.history_mut()
            .edit(label, |p| edit::set_clip_edits(p, clip, Some(edits)));
    }

    /// What the profile gives at the threshold in use.
    fn found_at(editor: &mut AudioEditor, found: &Found) -> Parts {
        let threshold = match editor.drag {
            Some(Drag::Threshold(t)) => t,
            _ => editor
                .threshold
                .unwrap_or_else(|| found.profile.auto_threshold()),
        };
        if editor.cache.as_ref().is_none_or(|c| c.threshold != threshold) {
            editor.cache = Some(Parts {
                threshold,
                phrases: found.profile.phrases(threshold),
                background: found.profile.silences(threshold),
                breaths: found.profile.breaths(threshold),
            });
        }
        editor.cache.clone().unwrap_or_default()
    }

    /// The hits found in the source at the sensitivity in use, in its frames.
    fn found_hits(
        &self,
        doc: &Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
    ) -> Arc<Vec<u64>> {
        let sensitivity = editor.sensitivity;
        if let Some((s, hits)) = &editor.hits
            && *s == sensitivity
        {
            return Arc::clone(hits);
        }
        let found = self.audio_found.get(&clip.source);
        let data = project
            .sources
            .get(&clip.source)
            .and_then(|s| doc.media().original(&doc.resolve(&s.path)));
        let (Some(found), Some(data)) = (found, data) else {
            return Arc::default();
        };
        let channels: Vec<&[f32]> = (0..data.channels()).map(|c| data.channel(c)).collect();
        // More sensitive: softer attacks count too.
        let least = 0.3 * 0.07f32.powf(sensitivity);
        let picked = found.onsets.pick(sensitivity);
        let hits: Vec<u64> = retouch::hits(&channels, data.sample_rate(), &picked, least)
            .into_iter()
            .map(|h| h as u64)
            .collect();
        let hits = Arc::new(hits);
        editor.hits = Some((sensitivity, Arc::clone(&hits)));
        hits
    }

    /// Every set of regions with level lines: the sorts shown, the parts retouched by hand,
    /// the selection.
    fn shown_sets(
        &self,
        editor: &mut AudioEditor,
        clip: &audiostrada_core::Clip,
        map: &Map,
        edits: &AudioEdits,
    ) -> Vec<Shown> {
        let mut out = Vec::new();
        if let Some(found) = self.audio_found.get(&clip.source).cloned() {
            let parts = Self::found_at(editor, &found);
            for sort in Sort::ALL.into_iter().filter(|s| s.leveled()) {
                if !editor.shows(sort) {
                    continue;
                }
                let (regions, gain, applied) = match edits.entries.iter().find(|e| sort.is(&e.kind))
                {
                    Some(e) => (e.kind.regions(), e.kind.db().unwrap_or(0.0), e.on),
                    None => (
                        found_regions(&found, sort, &parts, map.window(), &editor.ignored),
                        0.0,
                        false,
                    ),
                };
                out.push(Shown {
                    owner: Owner::Sort(sort),
                    regions,
                    gain,
                    applied,
                    color: sort.color(),
                });
            }
        }
        for (k, e) in edits.entries.iter().enumerate() {
            if let Kind::Gain { regions, db } = &e.kind {
                out.push(Shown {
                    owner: Owner::Part(k),
                    regions: regions.clone(),
                    gain: *db,
                    applied: e.on,
                    color: Color32::from_rgb(230, 130, 190),
                });
            }
        }
        if let Some((a, b)) = editor.selection {
            out.push(Shown {
                owner: Owner::Selection,
                regions: vec![Region::new(map.frame(a), map.frame(b))],
                gain: 0.0,
                applied: false,
                color: Color32::from_rgb(120, 170, 255),
            });
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn audio_editor(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        settings: &EditSettings,
    ) {
        let visuals = ui.visuals().clone();
        self.audio_tools(ui, doc, project, clip, editor, settings);
        let all = ui.available_rect_before_wrap();
        ui.allocate_rect(all, Sense::hover());
        let chips = Rect::from_min_max(all.min, pos2(all.right(), all.top() + CHIPS));
        let status = Rect::from_min_max(pos2(all.left(), all.bottom() - STATUS), all.max);
        let canvas = Rect::from_min_max(chips.left_bottom(), status.right_top());
        self.audio_chips(ui, doc, project, clip, editor, chips);
        self.audio_canvas(ui, doc, project, clip, editor, canvas, &visuals, settings);
        self.audio_status(ui, clip, editor, status);
    }

    /// The line under the waveform: what the pointer can do where it is, and what the stack
    /// holds.
    fn audio_status(
        &self,
        ui: &mut Ui,
        clip: &audiostrada_core::Clip,
        editor: &AudioEditor,
        rect: Rect,
    ) {
        let p = crate::theme::current(ui);
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        let retouches = self
            .current_edits(clip)
            .entries
            .iter()
            .filter(|e| e.on)
            .count();
        let summary = if !self.audio_found.contains_key(&clip.source) {
            tr("Listening to the clip…").to_owned()
        } else if retouches == 0 {
            tr("No retouch: the clip plays as recorded").to_owned()
        } else if retouches == 1 {
            tr("1 retouch · the file is never changed").to_owned()
        } else {
            trf!(
                "{count} retouches · the file is never changed",
                count = retouches
            )
        };
        let summary = ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            summary,
            FontId::proportional(11.5),
            p.weak,
        );
        let hint = editor.hint.unwrap_or_else(|| {
            tr("Switch on a chip to see what was found, or drag across the waveform to select a part")
        });
        let mut job = egui::text::LayoutJob::simple_singleline(
            hint.to_owned(),
            FontId::proportional(11.5),
            if editor.hint.is_some() { p.text } else { p.weak },
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(
            (summary.left() - rect.left() - 36.0).max(0.0),
        );
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        ui.painter().galley(
            pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
            galley,
            p.text,
        );
    }

    /// The bar above: tools, view, original or retouched, the track under the clip.
    fn audio_tools(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        settings: &EditSettings,
    ) {
        let tools = [
            (
                FrameTool::Select,
                tr(
                    "Hand: pull a level line, move an edge, squeeze a pause, slide a hit; drag \
                    across the waveform to select a part, Alt+drag to move the clip (S)",
                ),
            ),
            (
                FrameTool::Erase,
                tr(
                    "Eraser: drag across a noise to remove it, click a region to set it aside \
                    (E)",
                ),
            ),
        ];
        let mut asked = None;
        let mut retouched = self.audio_before.as_ref().is_none_or(|b| b.0 != clip.id);
        let was = retouched;
        let mut under = editor.under;
        let mut line_up_asked = false;
        let mut snap_asked = false;
        let mut flip = false;
        let own_track = edit::track_of_clip(project, clip.id);
        let inverted = own_track
            .and_then(|t| project.track(t))
            .is_some_and(|t| t.invert);
        let picked = editor_frame::tool_bar(ui, editor_frame::AUDIO, &tools, editor.tool, |ui| {
            asked = editor_frame::view_buttons(ui, editor.tool, tr("Fit the clip"));
            editor_frame::separator(ui);
            crate::dialog::segmented(
                ui,
                &mut retouched,
                &[(false, tr("Original"), true), (true, tr("Retouched"), true)],
            );
            ui.add_space(4.0);
            editor_frame::separator(ui);
            let p = crate::theme::current(ui);
            ui.label(egui::RichText::new(tr("Under")).size(12.0).color(p.weak));
            let name = |t: Option<TrackId>| {
                t.and_then(|t| project.track(t))
                    .map_or_else(|| tr("No track").to_owned(), |t| t.name.clone())
            };
            egui::ComboBox::from_id_salt("audio-editor-under")
                .selected_text(name(under))
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut under, None, tr("No track"));
                    for t in project
                        .tracks
                        .iter()
                        .filter(|t| t.kind == TrackKind::Audio && Some(t.id) != own_track)
                    {
                        ui.selectable_value(&mut under, Some(t.id), t.name.clone());
                    }
                })
                .response
                .on_hover_text(tr(
                    "Another track drawn under the clip, its hits marked across: a bass on the \
                     kick, backing vocals on the lead. The clip's hits then slide onto them",
                ));
            if under.is_some() {
                let button = ui.button(format!("{} ▾", tr("Line up"))).on_hover_text(tr(
                    "Brings the clip onto the track under it: each hit onto the nearest one, or \
                     the clip as a whole",
                ));
                egui::Popup::menu(&button).show(|ui| {
                    if ui
                        .button(trf!("Every hit onto {track}", track = name(under)))
                        .clicked()
                    {
                        snap_asked = true;
                        ui.close();
                    }
                    if ui
                        .button(tr("The whole clip"))
                        .on_hover_text(tr(
                            "Moves the clip to where it lines up best with the track under it, \
                             measured over the selection or the clip's first seconds",
                        ))
                        .clicked()
                    {
                        line_up_asked = true;
                        ui.close();
                    }
                });
                if ui
                    .selectable_label(inverted, "Ø")
                    .on_hover_text(tr(
                        "The clip's track with its polarity inverted: when the two cancel each \
                         other out, this brings the low end back",
                    ))
                    .clicked()
                {
                    flip = true;
                }
            }
        });
        // TEMP-SHOT-BEGIN
        if std::env::var("AE_UNDER").is_ok() && under.is_none() {
            under = project
                .tracks
                .iter()
                .find(|t| t.kind == TrackKind::Audio && Some(t.id) != own_track)
                .map(|t| t.id);
        }
        // TEMP-SHOT-END
        // A track put under the clip is there to line up with: the hits show.
        if under != editor.under && under.is_some() {
            editor.show(Sort::Hits);
        }
        editor.under = under;
        if snap_asked {
            editor.show(Sort::Hits);
            editor.asked = Some((Sort::Hits, ChipAction::SnapUnder));
        }
        let _ = settings;
        if let Some(tool) = picked {
            editor.tool = tool;
        }
        match asked {
            Some(ViewButton::ZoomTool) => editor.tool = FrameTool::Zoom,
            Some(ViewButton::ZoomOut) => editor.zoom = (editor.zoom * 2.0).min(MAX_ZOOM),
            Some(ViewButton::Fit) => editor.fit_asked = true,
            None => {}
        }
        if retouched != was {
            if retouched {
                if let Some((id, edits)) = self.audio_before.take() {
                    doc.history_mut()
                        .modify_unrecorded(|p| edit::set_clip_edits(p, id, Some(edits)));
                }
            } else {
                let edits = clip.edits.as_deref().cloned().unwrap_or_default();
                doc.history_mut()
                    .modify_unrecorded(|p| edit::set_clip_edits(p, clip.id, None));
                self.audio_before = Some((clip.id, edits));
            }
        }
        if flip && let Some(track) = own_track {
            doc.history_mut().edit(tr("Invert the polarity"), |p| {
                if let Some(t) = p.track_mut(track) {
                    t.invert = !t.invert;
                }
            });
        }
        if line_up_asked && let Some(under) = editor.under.and_then(|t| project.track(t)) {
            let (from, to) = editor.selection.unwrap_or((
                clip.start,
                clip.end()
                    .min(clip.start + u64::from(project.sample_rate) * 4),
            ));
            let step = u64::from((project.sample_rate / 8000).max(1));
            let len = ((to - from) / step) as usize;
            let mine = track_audio(doc, project, &[clip], (from, len, step));
            let theirs: Vec<&audiostrada_core::Clip> =
                under.clips.iter().filter(|c| !c.is_midi()).collect();
            let other = track_audio(doc, project, &theirs, (from, len, step));
            // Up to 30 ms either way.
            let lag = (u64::from(project.sample_rate) * 3 / 100 / step) as i64;
            match line_up(&mine, &other, lag) {
                Some((corr, l)) => {
                    let moved = l * step as i64;
                    if moved != 0 {
                        doc.history_mut().edit(tr("Line up the clip"), |p| {
                            edit::move_clips(p, &[clip.id], moved, 0);
                        });
                    }
                    let ms = moved as f64 * 1000.0 / f64::from(project.sample_rate);
                    let mut text = trf!(
                        "Lined up: moved {ms} ms, correlation {corr}",
                        ms = format!("{ms:+.1}"),
                        corr = format!("{corr:+.2}"),
                    );
                    if corr < -0.3 {
                        text.push_str(" · ");
                        text.push_str(tr(
                            "opposite polarity: switch on Ø to bring the low end back",
                        ));
                    }
                    doc.notify(text);
                }
                None => doc.notify(tr("Nothing to compare there").into()),
            }
        }
    }

    /// The row of chips: each shows or hides its sort on the waveform, with how many were
    /// found; its arrow (or a right click) opens what applies to all of them.
    fn audio_chips(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        rect: Rect,
    ) {
        let p = crate::theme::current(ui);
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        let found = self.audio_found.get(&clip.source).cloned();
        let map = Map::new(project, clip);
        let edits = self.current_edits(clip);
        let parts = found.as_ref().map(|f| Self::found_at(editor, f));
        let hits = if found.is_some() {
            let all = self.found_hits(doc, project, clip, editor);
            let near = u64::from(project.sample_rate / 100);
            placed_hits(&all, &edits, map.window(), near)
        } else {
            Vec::new()
        };
        let under = editor
            .under
            .and_then(|t| project.track(t))
            .map(|t| t.name.clone());
        let mut x = rect.left() + 10.0;
        for (k, sort) in Sort::ALL.into_iter().enumerate() {
            let entry = edits.entries.iter().find(|e| sort.is(&e.kind));
            let on = editor.shows(sort);
            let applied = entry.is_some_and(|e| e.on);
            let count = match (&found, &parts) {
                (Some(f), Some(parts)) => Some(match (sort, entry) {
                    (Sort::Hits, _) => hits.len(),
                    (_, Some(e)) if sort.leveled() => e.kind.regions().len(),
                    _ => found_regions(f, sort, parts, map.window(), &editor.ignored).len(),
                }),
                _ => None,
            };
            // The hits stand apart: they are about time, the others about sound.
            if sort == Sort::Hits {
                x += 6.0;
                ui.painter().vline(
                    x,
                    rect.center().y - 9.0..=rect.center().y + 9.0,
                    Stroke::new(1.0, p.border),
                );
                x += 12.0;
            }
            let dim = !on && count == Some(0);
            let ink = if on {
                p.text
            } else if dim {
                p.weak.gamma_multiply(0.6)
            } else {
                p.weak
            };
            let name =
                ui.painter()
                    .layout_no_wrap(sort.name().to_owned(), crate::theme::strong(12.0), ink);
            let number = ui.painter().layout_no_wrap(
                count.map_or_else(|| "…".to_owned(), |c| c.to_string()),
                FontId::proportional(11.5),
                p.weak,
            );
            let width = 26.0 + name.size().x + 7.0 + number.size().x + 22.0;
            let chip = Rect::from_min_size(pos2(x, rect.center().y - 12.0), vec2(width, 24.0));
            x += width + 6.0;
            let arrow = Rect::from_min_max(pos2(chip.right() - 20.0, chip.top()), chip.max);
            let body = Rect::from_min_max(chip.min, pos2(arrow.left(), chip.bottom()));
            let id = ui.scope_id().with(("audio-chip", k));
            let response = ui.interact(body, id, Sense::click());
            let more = ui.interact(arrow, id.with("more"), Sense::click());
            let color = sort.color();
            let hovered = response.hovered() || more.hovered();
            let fill = if on {
                color.gamma_multiply(if applied { 0.3 } else { 0.14 })
            } else if hovered {
                p.hover
            } else {
                p.surface
            };
            ui.painter().rect_filled(chip, 12.0, fill);
            ui.painter().rect_stroke(
                chip,
                12.0,
                Stroke::new(1.0, if on { color } else { p.border }),
                StrokeKind::Inside,
            );
            let dot = pos2(chip.left() + 13.0, chip.center().y);
            let dot_color = if dim { color.gamma_multiply(0.4) } else { color };
            if applied {
                ui.painter().circle_filled(dot, 4.5, dot_color);
            } else {
                ui.painter()
                    .circle_stroke(dot, 4.0, Stroke::new(1.5, dot_color));
            }
            let text_y = chip.center().y - name.size().y / 2.0;
            let name_width = name.size().x;
            ui.painter()
                .galley(pos2(chip.left() + 26.0, text_y), name, ink);
            ui.painter().galley(
                pos2(chip.left() + 26.0 + name_width + 7.0, text_y),
                number,
                p.weak,
            );
            ui.painter().text(
                arrow.center() - vec2(2.0, 0.0),
                Align2::CENTER_CENTER,
                "▾",
                FontId::proportional(11.0),
                if more.hovered() { p.text } else { p.weak },
            );
            let response = response
                .on_hover_text(sort.hint())
                .on_hover_cursor(CursorIcon::PointingHand);
            let more = more
                .on_hover_text(tr("What applies to all of them"))
                .on_hover_cursor(CursorIcon::PointingHand);
            if response.clicked() {
                if on {
                    editor.shown.retain(|s| *s != sort);
                } else {
                    editor.shown.push(sort);
                }
            }
            let state = ChipState {
                entry: entry.map(|e| e.on),
                shortened: edits
                    .entries
                    .iter()
                    .any(|e| matches!(e.kind, Kind::Shorten { .. })),
                under: under.clone(),
                sensitivity: editor.sensitivity,
            };
            let mut asked = None;
            egui::Popup::menu(&more)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| asked = chip_menu(ui, sort, &state));
            response.context_menu(|ui| {
                if let Some(a) = chip_menu(ui, sort, &state) {
                    asked = Some(a);
                }
            });
            match asked {
                Some(ChipAction::Sensitivity(s)) => editor.sensitivity = s,
                Some(action) => {
                    editor.show(sort);
                    editor.asked = Some((sort, action));
                }
                None => {}
            }
        }
    }
}

/// What a chip's menu needs to know.
struct ChipState {
    /// Its retouch in the stack: switched on or off.
    entry: Option<bool>,
    shortened: bool,
    /// The track under the clip.
    under: Option<String>,
    sensitivity: f32,
}

/// The menu of a chip: what applies to every region of its sort.
fn chip_menu(ui: &mut Ui, sort: Sort, state: &ChipState) -> Option<ChipAction> {
    fn item(ui: &mut Ui, asked: &mut Option<ChipAction>, text: &str, action: ChipAction) {
        if ui.button(text).clicked() {
            *asked = Some(action);
            ui.close();
        }
    }
    let mut asked = None;
    let a = &mut asked;
    match sort {
        Sort::Phrases => item(
            ui,
            a,
            tr("Bring them all to the same level"),
            ChipAction::Even,
        ),
        Sort::Background => {
            item(ui, a, tr("Lower by 12 dB"), ChipAction::Dose(-12.0));
            item(ui, a, tr("Remove"), ChipAction::Dose(REMOVED_DB));
            item(ui, a, tr("Bring out by 6 dB"), ChipAction::Dose(6.0));
            ui.separator();
            for s in [0.3f32, 0.5, 0.8] {
                item(
                    ui,
                    a,
                    &trf!("Shorten every pause to {s} s", s = format!("{s:.1}")),
                    ChipAction::Shorten(s),
                );
            }
            if state.shortened {
                item(
                    ui,
                    a,
                    tr("Pauses back to their length"),
                    ChipAction::PausesBack,
                );
            }
        }
        Sort::Breaths => {
            item(ui, a, tr("Lower by 9 dB"), ChipAction::Dose(-9.0));
            item(ui, a, tr("Remove"), ChipAction::Dose(REMOVED_DB));
        }
        Sort::Sibilants => item(ui, a, tr("Soften by 6 dB"), ChipAction::Dose(-6.0)),
        Sort::Clicks => item(ui, a, tr("Repair them all"), ChipAction::Repair),
        Sort::Clipping => item(ui, a, tr("Redraw them all"), ChipAction::Repair),
        Sort::Hits => {
            let p = crate::theme::current(ui);
            ui.label(
                egui::RichText::new(tr("Sensitivity: fewer or more hits"))
                    .size(11.5)
                    .color(p.weak),
            );
            let mut value = state.sensitivity;
            if ui
                .add(egui::Slider::new(&mut value, 0.0..=1.0).show_value(false))
                .changed()
            {
                *a = Some(ChipAction::Sensitivity(value));
            }
            ui.separator();
            if let Some(track) = &state.under {
                item(
                    ui,
                    a,
                    &trf!("Every hit onto {track}", track = track),
                    ChipAction::SnapUnder,
                );
            }
            item(ui, a, tr("Every hit onto the grid"), ChipAction::SnapGrid);
            if state.entry.is_some() {
                item(ui, a, tr("Hits back in place"), ChipAction::HitsBack);
            }
        }
    }
    if let Some(on) = state.entry {
        ui.separator();
        let text = if on {
            tr("Switch off")
        } else {
            tr("Switch on")
        };
        item(ui, a, text, ChipAction::Switch);
        if sort != Sort::Hits {
            item(ui, a, tr("Take back"), ChipAction::TakeBack);
        }
    }
    asked
}

impl TimelineView {
    /// A stretch of the clip's audio as drawn, `from..to` (timeline samples), laid across
    /// `x0..x1`: what a gesture is about to make of it.
    #[allow(clippy::too_many_arguments)]
    fn draw_piece(
        &mut self,
        painter: &egui::Painter,
        doc: &Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        (from, to): (i64, i64),
        (x0, x1): (f32, f32),
        wave: Rect,
        color: Color32,
    ) {
        let from = from.max(clip.start as i64);
        let to = to.min(clip.end() as i64);
        if to <= from || x1 - x0 < 0.5 {
            return;
        }
        let mut piece = clip.clone();
        piece.offset = clip.offset + (from as u64 - clip.start);
        piece.start = from as u64;
        piece.length = (to - from) as u64;
        piece.fade_in = 0;
        piece.fade_out = 0;
        let rect = Rect::from_min_max(pos2(x0, wave.top()), pos2(x1, wave.bottom()));
        let zoom = std::mem::replace(&mut self.zoom, (to - from) as f64 / f64::from(x1 - x0));
        let within = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        self.draw_waveform(&within, rect, doc, project, &piece, color);
        self.zoom = zoom;
    }

    #[allow(clippy::too_many_arguments)]
    fn audio_canvas(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        canvas: Rect,
        visuals: &egui::Visuals,
        settings: &EditSettings,
    ) {
        let p = crate::theme::current(ui);
        let ruler = Rect::from_min_max(canvas.min, pos2(canvas.right(), canvas.top() + RULER));
        let lane = Rect::from_min_max(pos2(canvas.left(), ruler.bottom()), canvas.max);
        // The clip on top, the track under it in a band of its own below.
        let split = if editor.under.is_some() {
            lane.top() + lane.height() * 0.64
        } else {
            lane.bottom()
        };
        let band = Rect::from_min_max(lane.min, pos2(lane.right(), split));
        let under_band = Rect::from_min_max(pos2(lane.left(), split), lane.max);
        if (!editor.fitted || editor.fit_asked) && lane.width() > 10.0 {
            let length = clip.length.max(1) as f64;
            editor.zoom = (length * 1.04 / f64::from(lane.width())).clamp(MIN_ZOOM, MAX_ZOOM);
            editor.scroll = (clip.start as f64 - length * 0.02).max(0.0);
            editor.fitted = true;
            editor.fit_asked = false;
        }
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| canvas.contains(*p));
        if let Some(pointer) = pointer {
            let (zoom, wheel, alt) =
                ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.alt));
            if (zoom - 1.0).abs() > 1e-3 {
                let at = editor.scroll + f64::from(pointer.x - lane.left()) * editor.zoom;
                editor.zoom = (editor.zoom / f64::from(zoom)).clamp(MIN_ZOOM, MAX_ZOOM);
                editor.scroll = at - f64::from(pointer.x - lane.left()) * editor.zoom;
            }
            if alt {
                // Alt and the wheel: the waveform taller or lower, as in the timeline.
                editor.wave = (editor.wave * ((wheel.x + wheel.y) * 0.01).exp()).clamp(0.25, 16.0);
            } else {
                editor.scroll -= f64::from(wheel.x + wheel.y) * editor.zoom;
            }
        }
        // TEMP-SHOT-BEGIN
        if let Ok(v) = std::env::var("AE_VIEW") {
            let v: Vec<f64> = v.split(',').filter_map(|t| t.parse().ok()).collect();
            let rate = f64::from(project.sample_rate);
            editor.scroll = v[0] * rate;
            editor.zoom = v[1] * rate / f64::from(lane.width());
        }
        // TEMP-SHOT-END
        editor.scroll = editor.scroll.max(0.0);
        let scale = Scale {
            left: lane.left(),
            scroll: editor.scroll,
            zoom: editor.zoom,
        };
        let painter = ui.painter().with_clip_rect(canvas);
        self.draw_ruler(
            &painter.with_clip_rect(ruler),
            ruler,
            project,
            scale,
            visuals,
        );
        let lane_painter = painter.with_clip_rect(lane);
        lane_painter.rect_filled(lane, 0.0, visuals.extreme_bg_color);
        let timeline_view = (self.zoom, self.wave_zoom, self.wave_view);
        self.zoom = scale.zoom;
        self.draw_grid(&lane_painter, lane, project, scale, visuals);
        let map = Map::new(project, clip);
        let edits = self.current_edits(clip);
        let found = self.audio_found.get(&clip.source).cloned();
        let profile = found.as_ref().map(|f| &f.profile);
        let rate = profile.map_or(project.sample_rate, |p| p.rate).max(1);
        let frames = profile.map_or(0, |p| p.frames as u64);
        let sets = self.shown_sets(editor, clip, &map, &edits);
        let parts = found.as_ref().map(|f| Self::found_at(editor, f));
        let threshold = parts.as_ref().map_or(-50.0, |p| p.threshold);
        let x_of = |frame: u64| scale.x(map.sample(frame).max(0) as u64);
        let x_moved = |place: u64| scale.x(map.sample_moved(place).max(0) as u64);
        let track_color = edit::track_of_clip(project, clip.id)
            .and_then(|t| project.track(t))
            .map_or(visuals.selection.bg_fill, |t| {
                crate::theme::track_color(t.color)
            });
        let wave = wave_of(band);

        // The track under the clip, in its band, its hits marked across both.
        let mut guides = Vec::new();
        let mut under_color = p.weak;
        if let Some(under) = editor.under.and_then(|t| project.track(t)) {
            under_color = crate::theme::track_color(under.color);
            lane_painter.rect_filled(under_band, 0.0, under_color.gamma_multiply(0.07));
            lane_painter.hline(
                lane.x_range(),
                under_band.top(),
                Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            );
            self.wave_zoom = 1.0;
            self.wave_view.fit = true;
            let body = under_band.shrink2(vec2(0.0, 8.0));
            for c in under.clips.iter().filter(|c| !c.is_midi()) {
                let body = Rect::from_min_max(
                    pos2(scale.x(c.start), body.top()),
                    pos2(scale.x(c.end()), body.bottom()),
                );
                if body.right() < lane.left() || body.left() > lane.right() {
                    continue;
                }
                self.draw_waveform(&lane_painter, body, doc, project, c, under_color);
            }
            lane_painter.text(
                pos2(lane.left() + 10.0, under_band.top() + 12.0),
                Align2::LEFT_CENTER,
                &under.name,
                crate::theme::strong(11.5),
                under_color,
            );
            guides = self.attacks(doc, under.id, true);
        }

        // The hits of the clip, where they play.
        let found_hits = if found.is_some() && editor.shows(Sort::Hits) {
            self.found_hits(doc, project, clip, editor)
        } else {
            Arc::default()
        };
        let near = u64::from(rate / 100);
        let playing = clip.edits.as_deref().cloned().unwrap_or_default();
        let hits = placed_hits(&found_hits, &playing, map.window(), near);

        // The level lines: per set and region, its gain and the line's height.
        let lines: Vec<Vec<(f32, f32)>> = sets
            .iter()
            .enumerate()
            .map(|(s, set)| {
                set.regions
                    .iter()
                    .enumerate()
                    .map(|(j, r)| {
                        let mut gain = if set.applied {
                            r.db.unwrap_or(set.gain)
                        } else {
                            0.0
                        };
                        if let Some(Drag::Level {
                            shown,
                            region,
                            alone,
                            gain: g,
                        }) = editor.drag
                            && shown == s
                            && (!alone || region == j)
                        {
                            gain = g;
                        }
                        (gain, y_of_gain(band, gain))
                    })
                    .collect()
            })
            .collect();

        // The marks of clicks and clipped peaks; those too close together make one bubble
        // with their count.
        let mut marks: Vec<Mark> = Vec::new();
        if let (Some(f), Some(parts)) = (&found, &parts) {
            for sort in [Sort::Clicks, Sort::Clipping] {
                if !editor.shows(sort) {
                    continue;
                }
                let repaired: Vec<Region> = edits
                    .entries
                    .iter()
                    .find(|e| sort.is(&e.kind) && e.on)
                    .map(|e| e.kind.regions())
                    .unwrap_or_default();
                for (j, r) in found_regions(f, sort, parts, map.window(), &editor.ignored)
                    .iter()
                    .enumerate()
                {
                    let x = (x_of(r.start) + x_of(r.end)) / 2.0;
                    if x < lane.left() || x > lane.right() {
                        continue;
                    }
                    let fixed = repaired.iter().any(|o| o.start < r.end && r.start < o.end);
                    match marks.last_mut() {
                        Some(m) if m.sort == sort && x - m.right < 14.0 => {
                            m.right = x;
                            m.count += 1;
                            m.fixed &= fixed;
                            m.last = r.end;
                        }
                        _ => marks.push(Mark {
                            sort,
                            first: j,
                            count: 1,
                            left: x,
                            right: x,
                            fixed,
                            start: r.start,
                            last: r.end,
                        }),
                    }
                }
            }
        }

        // The threshold between sound and background: a tab at the left, its line and the
        // clip's level against it while it is pointed at or dragged.
        let threshold_shown = found.is_some()
            && [Sort::Phrases, Sort::Background, Sort::Breaths]
                .iter()
                .any(|s| editor.shows(*s));
        let tab = Rect::from_min_size(
            pos2(lane.left() + 8.0, band.bottom() - 26.0),
            vec2(112.0, 18.0),
        );
        let hits_shown = editor.shows(Sort::Hits);
        let under_of = |pos: Pos2| -> Under {
            if !band.contains(pos) {
                return Under::Nothing;
            }
            if let Some(k) = marks
                .iter()
                .position(|m| (pos - m.at(band)).length() < 9.0 + (m.right - m.left) / 2.0)
            {
                return Under::Mark(k);
            }
            if threshold_shown && tab.expand(2.0).contains(pos) {
                return Under::Threshold;
            }
            // The selection and the parts first: they lie over what was found.
            let mut inside = Under::Nothing;
            for s in (0..sets.len()).rev() {
                for (j, r) in sets[s].regions.iter().enumerate() {
                    let (x0, x1) = (x_of(r.start), x_of(r.end));
                    if pos.x < x0 - EDGE || pos.x > x1 + EDGE {
                        continue;
                    }
                    if (pos.y - lines[s][j].1).abs() < LINE && pos.x > x0 && pos.x < x1 {
                        return Under::Line(s, j);
                    }
                    let middle = (x0.max(lane.left()) + x1.min(lane.right())) / 2.0;
                    if sets[s].owner == Owner::Sort(Sort::Background)
                        && pos.y > band.bottom() - 20.0
                        && (pos.x - middle).abs() < 15.0
                    {
                        return Under::Pause(j);
                    }
                    if (pos.x - x0).abs() < EDGE {
                        return Under::Edge(s, j, true);
                    }
                    if (pos.x - x1).abs() < EDGE {
                        return Under::Edge(s, j, false);
                    }
                    if pos.x > x0 && pos.x < x1 && inside == Under::Nothing {
                        inside = Under::Region(s, j);
                    }
                }
            }
            // A hit is grabbed anywhere on its note, a little before its attack too.
            if hits_shown
                && let Some(k) = hits.iter().rposition(|h| x_moved(h.to) <= pos.x + 4.0)
                && pos.x < scale.x(clip.end())
            {
                return Under::Attack(k);
            }
            inside
        };
        let hover = match editor.drag {
            Some(Drag::Level { shown, region, .. }) => Under::Line(shown, region),
            Some(Drag::Edge {
                shown,
                region,
                start,
                ..
            }) => Under::Edge(shown, region, start),
            Some(Drag::Pause { region, .. }) => Under::Pause(region),
            Some(Drag::Threshold(_)) => Under::Threshold,
            Some(Drag::Hit { at, .. }) => hits
                .iter()
                .position(|h| h.at == at)
                .map_or(Under::Nothing, Under::Attack),
            Some(_) => Under::Nothing,
            None if editor.tool == FrameTool::Zoom => Under::Nothing,
            None => pointer
                .filter(|_| !egui::Popup::is_any_open(ui.ctx()))
                .map_or(Under::Nothing, &under_of),
        };

        // TEMP-SHOT-BEGIN
        let hover = match std::env::var("AE_HOVER").ok().as_deref().map(|v| {
            v.split(':').map(str::to_owned).collect::<Vec<_>>()
        }) {
            Some(v) if v[0] == "attack" => Under::Attack(
                hits.iter()
                    .position(|h| x_moved(h.to) >= lane.left())
                    .unwrap_or(0)
                    + v[1].parse::<usize>().unwrap(),
            ),
            Some(v) if v[0] == "line" => Under::Line(v[1].parse().unwrap(), v[2].parse().unwrap()),
            Some(v) if v[0] == "region" => {
                Under::Region(v[1].parse().unwrap(), v[2].parse().unwrap())
            }
            Some(v) if v[0] == "threshold" => Under::Threshold,
            _ => hover,
        };
        {
            use std::sync::atomic::{AtomicU32, Ordering};
            static FRAME: AtomicU32 = AtomicU32::new(0);
            let n = if found.is_some() {
                FRAME.fetch_add(1, Ordering::Relaxed)
            } else {
                0
            };
            if let Ok(v) = std::env::var("AE_DO") {
                for (k, what) in v.split(',').enumerate() {
                    if n != 40 + 25 * k as u32 || found.is_none() {
                        continue;
                    }
                    let (name, value) = what.split_once('=').unwrap_or((what, "0"));
                    let value: f32 = value.parse().unwrap_or(0.0);
                    editor.asked = Some(match name {
                        "even" => (Sort::Phrases, ChipAction::Even),
                        "background" => (Sort::Background, ChipAction::Dose(value)),
                        "breaths" => (Sort::Breaths, ChipAction::Dose(value)),
                        "sibilants" => (Sort::Sibilants, ChipAction::Dose(value)),
                        "clicks" => (Sort::Clicks, ChipAction::Repair),
                        "shorten" => (Sort::Background, ChipAction::Shorten(value)),
                        "snap" => (Sort::Hits, ChipAction::SnapUnder),
                        "grid" => (Sort::Hits, ChipAction::SnapGrid),
                        _ => continue,
                    });
                }
            }
            if let Ok(v) = std::env::var("AE_DRAG")
                && n > 40
                && found.is_some()
            {
                let v: Vec<f32> = v.split(':').skip(1).filter_map(|t| t.parse().ok()).collect();
                if std::env::var("AE_DRAG").unwrap().starts_with("hit") {
                    let first = hits
                        .iter()
                        .position(|h| x_moved(h.to) >= lane.left())
                        .unwrap_or(0);
                    if let Some(h) = hits.get(first + v[0] as usize) {
                        let d = (v[1] / 1000.0 * rate as f32) as i64;
                        editor.drag = Some(Drag::Hit {
                            at: h.at,
                            to: (h.to as i64 + d) as u64,
                            grab: 0,
                            snapped: false,
                        });
                    }
                } else if sets.len() > v[0] as usize {
                    editor.drag = Some(Drag::Level {
                        shown: v[0] as usize,
                        region: v[1] as usize,
                        alone: false,
                        gain: v[2],
                    });
                }
            }
        }
        // TEMP-SHOT-END
        // The clip, drawn to fill its band; while a retouch is made ready, as it last was.
        let ready = |c: &audiostrada_core::Clip| {
            audiostrada_engine::project::clip_media(project, doc.paths(), c)
                .is_some_and(|p| doc.media().peaks(&p).is_some() || doc.media().original(&p).is_some())
        };
        let mut shown_clip = if ready(clip) {
            editor.drawn = Some(clip.clone());
            clip.clone()
        } else {
            editor
                .drawn
                .clone()
                .filter(|c| c.id == clip.id)
                .unwrap_or_else(|| clip.clone())
        };
        let moved = match editor.drag {
            Some(Drag::Move { moved, .. }) => moved,
            _ => 0,
        };
        shown_clip.start = (shown_clip.start as i64 + moved).max(0) as u64;
        let loudest = found.as_ref().map_or(1.0, |f| f.peak) * 10f32.powf(clip.gain_db / 20.0);
        self.wave_zoom = (0.82 / loudest.max(1e-4) * editor.wave).clamp(1.0, MAX_WAVE_ZOOM);
        self.wave_view.fit = false;
        let body = Rect::from_min_max(
            pos2(scale.x(shown_clip.start), wave.top()),
            pos2(scale.x(shown_clip.end()), wave.bottom()),
        );
        lane_painter.rect_filled(
            Rect::from_min_max(pos2(body.left(), band.top()), pos2(body.right(), band.bottom())),
            0.0,
            track_color.gamma_multiply(0.05),
        );
        self.draw_waveform(&lane_painter, body, doc, project, &shown_clip, track_color);
        // The parts whose level is being pulled, redrawn at that level.
        if let Some(Drag::Level { shown, .. }) = editor.drag
            && let Some(set) = sets.get(shown)
        {
            for (j, r) in set.regions.iter().enumerate() {
                let now = if set.applied {
                    stored(r.db.unwrap_or(set.gain))
                } else {
                    0.0
                };
                let delta = stored(lines[shown][j].0).max(-90.0) - now.max(-90.0);
                let rect = Rect::from_min_max(
                    pos2(x_of(r.start), wave.top()),
                    pos2(x_of(r.end), wave.bottom()),
                )
                .intersect(body);
                if rect.width() <= 0.0 || delta.abs() < 0.01 {
                    continue;
                }
                lane_painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
                lane_painter.rect_filled(rect, 0.0, track_color.gamma_multiply(0.05));
                let mut piece = shown_clip.clone();
                piece.gain_db += delta;
                piece.fade_in = 0;
                piece.fade_out = 0;
                self.draw_waveform(
                    &lane_painter.with_clip_rect(rect),
                    body,
                    doc,
                    project,
                    &piece,
                    track_color,
                );
            }
        }
        // The hit being slid: its note and the one before, laid out where they will play.
        if let Some(Drag::Hit { at, to, .. }) = editor.drag
            && let Some(k) = hits.iter().position(|h| h.at == at)
            && hits[k].to != to
        {
            let (before, after) = beside(&hits, k, frames);
            let (start, was, now, end) = (
                map.sample_moved(before.to),
                map.sample_moved(hits[k].to),
                map.sample_moved(to),
                map.sample_moved(after.to),
            );
            let x = |sample: i64| scale.x(sample.max(0) as u64);
            let all = Rect::from_min_max(pos2(x(start), wave.top()), pos2(x(end), wave.bottom()))
                .intersect(body);
            lane_painter.rect_filled(all, 0.0, visuals.extreme_bg_color);
            lane_painter.rect_filled(all, 0.0, track_color.gamma_multiply(0.05));
            let within = lane_painter.with_clip_rect(all);
            for (from, until, lands, ends) in [(start, was, start, now), (was, end, now, end)] {
                let (len, played) = ((until - from).max(0), (ends - lands).max(0));
                let kept = retouch::kept_of(len as usize, played as usize, project.sample_rate) as i64;
                self.draw_piece(
                    &within,
                    doc,
                    project,
                    clip,
                    (from, from + kept),
                    (x(lands), x(lands + kept)),
                    wave,
                    track_color,
                );
                self.draw_piece(
                    &within,
                    doc,
                    project,
                    clip,
                    (from + kept, until),
                    (x(lands + kept), x(ends)),
                    wave,
                    track_color,
                );
            }
        }
        (self.zoom, self.wave_zoom, self.wave_view) = timeline_view;

        // The clip's level against the threshold, while the threshold is in hand.
        if threshold_shown
            && hover == Under::Threshold
            && let Some(profile) = profile
        {
            lane_painter.rect_filled(band, 0.0, Color32::from_black_alpha(110));
            let hop = profile.hop.max(1);
            let mut x = body.left().max(lane.left());
            while x < body.right().min(lane.right()) {
                // The loudest step under these pixels: the picture does not flicker.
                let a = map.frame(scale.sample(x)) as usize / hop;
                let b = (map.frame(scale.sample(x + 2.0)) as usize / hop).max(a + 1);
                let level = profile.level[a.min(profile.level.len())..b.min(profile.level.len())]
                    .iter()
                    .fold(-120.0f32, |m, v| m.max(*v));
                let color = if level > threshold {
                    track_color.gamma_multiply(0.7)
                } else {
                    Sort::Background.color().gamma_multiply(0.45)
                };
                lane_painter.rect_filled(
                    Rect::from_min_max(
                        pos2(x, y_of_level(band, level)),
                        pos2(x + 2.0, wave.bottom()),
                    ),
                    0.0,
                    color,
                );
                x += 2.0;
            }
            let y = y_of_level(band, threshold);
            lane_painter.hline(lane.x_range(), y, Stroke::new(1.5, Color32::WHITE));
        }

        // The regions and their level lines.
        let shortened: Vec<Region> = edits
            .entries
            .iter()
            .filter(|e| e.on)
            .filter_map(|e| match &e.kind {
                Kind::Shorten { regions, keep } => Some(
                    regions
                        .iter()
                        .filter(|r| r.end - r.start > r.keep.unwrap_or(*keep))
                        .copied()
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect();
        for (s, set) in sets.iter().enumerate() {
            let phrases = set.owner == Owner::Sort(Sort::Phrases);
            for (j, r) in set.regions.iter().enumerate() {
                let (mut a, mut b) = (r.start, r.end);
                if let Some(Drag::Edge {
                    shown,
                    region,
                    start,
                    to,
                }) = editor.drag
                    && (shown, region) == (s, j)
                {
                    if start {
                        a = to.min(b.saturating_sub(1));
                    } else {
                        b = to.max(a + 1);
                    }
                }
                let (x0, x1) = (x_of(a), x_of(b).max(x_of(a) + 2.0));
                if x1 < lane.left() || x0 > lane.right() {
                    continue;
                }
                let (gain, y) = lines[s][j];
                let pointed = matches!(
                    hover,
                    Under::Line(hs, hj) | Under::Edge(hs, hj, _) | Under::Region(hs, hj)
                        if (hs, hj) == (s, j)
                ) || (hover == Under::Pause(j) && set.owner == Owner::Sort(Sort::Background));
                let on_line = hover == Under::Line(s, j);
                // Labels sit in the middle of what is in view.
                let middle = (x0.max(lane.left()) + x1.min(lane.right())) / 2.0;
                let visible = x1.min(lane.right()) - x0.max(lane.left());
                let dragged = matches!(editor.drag, Some(Drag::Level { shown, .. }) if shown == s);
                let touched = (set.applied || dragged) && gain.abs() > 0.05;
                let rect = Rect::from_min_max(pos2(x0, band.top()), pos2(x1, band.bottom()));
                let tint = match (phrases, pointed) {
                    (true, false) => 0.0,
                    (true, true) => 0.06,
                    (false, false) => 0.12,
                    (false, true) => 0.2,
                };
                if tint > 0.0 {
                    lane_painter.rect_filled(rect, 0.0, set.color.gamma_multiply(tint));
                }
                if pointed || set.owner == Owner::Selection {
                    let edge = Stroke::new(1.0, set.color.gamma_multiply(0.6));
                    lane_painter.vline(x0, rect.y_range(), edge);
                    lane_painter.vline(x1, rect.y_range(), edge);
                }
                // The line stops short of the edges: each region reads as a bar of its own.
                let inset = ((x1 - x0) * 0.15).min(3.0);
                let line = [pos2(x0 + inset, y), pos2(x1 - inset, y)];
                let ink = if touched || pointed {
                    set.color
                } else {
                    set.color.gamma_multiply(0.75)
                };
                // What is found between the sounds shows as a tint; its line comes with the
                // pointer, or once it is dosed.
                let lined = touched
                    || pointed
                    || phrases
                    || dragged
                    || matches!(set.owner, Owner::Selection | Owner::Part(_));
                if touched || on_line {
                    lane_painter
                        .line_segment(line, Stroke::new(if on_line { 2.5 } else { 2.0 }, ink));
                } else if lined {
                    lane_painter.extend(egui::Shape::dashed_line(
                        &line,
                        Stroke::new(1.2, ink),
                        5.0,
                        3.0,
                    ));
                }
                if lined {
                    for x in [line[0].x, line[1].x] {
                        lane_painter.vline(x, y - 3.0..=y + 3.0, Stroke::new(1.2, ink));
                    }
                }
                if pointed && visible > 16.0 {
                    lane_painter.circle_filled(pos2(middle, y), 4.0, set.color);
                    lane_painter.circle_stroke(
                        pos2(middle, y),
                        4.0,
                        Stroke::new(1.0, visuals.extreme_bg_color),
                    );
                }
                if (touched || r.db.is_some() && set.applied) && visible > 54.0 {
                    let above = if y - wave.top() < 22.0 { 12.0 } else { -11.0 };
                    lane_painter.text(
                        pos2(middle, y + above),
                        Align2::CENTER_CENTER,
                        gain_text(gain),
                        FontId::proportional(10.5),
                        set.color,
                    );
                }
                // A pause's handle, to squeeze it; its length once squeezed.
                if set.owner == Owner::Sort(Sort::Background) && visible > 28.0 {
                    let squeezed = shortened
                        .iter()
                        .any(|o| o.start < r.end && r.start < o.end);
                    if pointed || squeezed {
                        let at = pos2(middle, band.bottom() - 11.0);
                        let color = set.color.gamma_multiply(if pointed { 0.7 } else { 0.35 });
                        tag(&lane_painter, at, "⟷", color, visuals.strong_text_color());
                    }
                }
            }
        }
        // A pause being squeezed: what will go, hatched.
        if let Some(Drag::Pause {
            region,
            alone,
            keep,
            ..
        }) = editor.drag
            && let Some(set) = sets
                .iter()
                .find(|s| s.owner == Owner::Sort(Sort::Background))
        {
            for (j, r) in set.regions.iter().enumerate() {
                if (alone && j != region) || r.end - r.start <= keep {
                    continue;
                }
                let half = keep / 2;
                let cut = Rect::from_min_max(
                    pos2(x_of(r.start + half), band.top()),
                    pos2(x_of(r.end - (keep - half)), band.bottom()),
                );
                lane_painter.rect_filled(cut, 0.0, Color32::from_black_alpha(120));
                let mut x = cut.left();
                while x < cut.right() {
                    lane_painter.line_segment(
                        [
                            pos2(x, cut.bottom()),
                            pos2((x + 12.0).min(cut.right()), cut.top()),
                        ],
                        Stroke::new(1.0, Sort::Background.color().gamma_multiply(0.4)),
                    );
                    x += 8.0;
                }
            }
        }

        // The hits of the track under the clip, across both bands.
        for &t in &guides {
            let x = scale.x(t);
            if x < lane.left() || x > lane.right() {
                continue;
            }
            lane_painter.extend(egui::Shape::dashed_line(
                &[pos2(x, lane.top()), pos2(x, lane.bottom())],
                Stroke::new(1.0, under_color.gamma_multiply(0.55)),
                4.0,
                4.0,
            ));
        }
        // The hits of the clip: a flag at the foot of each, its note lit under the pointer;
        // late or early against the track under it, the gap shows.
        if hits_shown {
            let color = Sort::Hits.color();
            let reach = (SNAP_REACH * project.sample_rate as f32) as i64;
            let tight = i64::from(project.sample_rate / 500).max((1.5 * scale.zoom) as i64);
            let in_view = hits
                .iter()
                .filter(|h| (lane.left()..=lane.right()).contains(&x_moved(h.to)))
                .count();
            let crowded = in_view as f32 * 7.0 > lane.width();
            let dragged = match editor.drag {
                Some(Drag::Hit { at, to, snapped, .. }) => Some((at, to, snapped)),
                _ => None,
            };
            for (k, hit) in hits.iter().enumerate() {
                let place = dragged
                    .filter(|d| d.0 == hit.at)
                    .map_or(hit.to, |d| d.1);
                let sample = map.sample_moved(place);
                let x = scale.x(sample.max(0) as u64);
                if x < lane.left() - 40.0 || x > lane.right() + 40.0 {
                    continue;
                }
                let pointed = hover == Under::Attack(k);
                let moved = place != hit.at;
                if crowded && !moved && !pointed {
                    continue;
                }
                // The nearest hit of the track under it, and how far.
                let gap = guides
                    .iter()
                    .map(|&g| g as i64 - sample)
                    .filter(|d| d.abs() <= reach)
                    .min_by_key(|d| d.abs());
                let ink = match gap {
                    Some(d) if d.abs() <= tight => Color32::from_rgb(110, 210, 140),
                    Some(_) => Color32::from_rgb(245, 170, 60),
                    None if moved => color,
                    None => color.gamma_multiply(0.6),
                };
                if pointed {
                    let next = hits
                        .get(k + 1)
                        .map_or(scale.x(clip.end()), |n| x_moved(n.to));
                    lane_painter.rect_filled(
                        Rect::from_min_max(pos2(x, band.top()), pos2(next, band.bottom())),
                        0.0,
                        Color32::from_white_alpha(20),
                    );
                }
                let strength = if pointed {
                    0.95
                } else if moved {
                    0.55
                } else {
                    0.22
                };
                lane_painter.vline(
                    x,
                    band.top()..=band.bottom(),
                    Stroke::new(1.0, ink.gamma_multiply(strength)),
                );
                let foot = band.bottom();
                lane_painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(x, foot - 9.0),
                        pos2(x + 5.0, foot),
                        pos2(x - 5.0, foot),
                    ],
                    if moved || pointed { ink } else { ink.gamma_multiply(0.75) },
                    Stroke::NONE,
                ));
                if let Some(d) = gap
                    && d.abs() > tight
                {
                    let there = scale.x((sample + d).max(0) as u64);
                    lane_painter.hline(
                        x.min(there)..=x.max(there),
                        foot - 3.0,
                        Stroke::new(3.0, ink.gamma_multiply(0.8)),
                    );
                    if pointed && dragged.is_none() {
                        let ms = format!("{:.0}", d.abs() as f64 * 1000.0 / f64::from(project.sample_rate));
                        let text = if d > 0 {
                            trf!("{ms} ms early", ms = ms)
                        } else {
                            trf!("{ms} ms late", ms = ms)
                        };
                        tag(
                            &lane_painter,
                            pos2(x, band.top() + 12.0),
                            &text,
                            Color32::from_black_alpha(200),
                            ink,
                        );
                    }
                }
                if moved {
                    // Where it was found.
                    let was = x_moved(hit.at);
                    lane_painter.extend(egui::Shape::dashed_line(
                        &[pos2(was, wave.bottom()), pos2(was, band.bottom())],
                        Stroke::new(1.0, color.gamma_multiply(0.6)),
                        2.0,
                        2.0,
                    ));
                    if pointed && dragged.is_none() {
                        let d = (hit.to as i64 - hit.at as i64) * i64::from(project.sample_rate)
                            / i64::from(rate);
                        tag(
                            &lane_painter,
                            pos2(x, band.top() + 30.0),
                            &ms_text(d, project.sample_rate),
                            Color32::from_black_alpha(200),
                            Color32::WHITE,
                        );
                    }
                }
                if dragged.is_some_and(|d| d.0 == hit.at && d.2) {
                    lane_painter.vline(x, lane.y_range(), Stroke::new(1.5, Color32::WHITE));
                }
            }
        }
        for m in &marks {
            let color = m.sort.color();
            let at = m.at(band);
            lane_painter.vline(
                at.x,
                band.top() + 16.0..=band.bottom(),
                Stroke::new(1.0, color.gamma_multiply(0.3)),
            );
            if m.count > 1 {
                let text = m.count.to_string();
                let galley = lane_painter.layout_no_wrap(
                    text,
                    crate::theme::strong(10.5),
                    visuals.strong_text_color(),
                );
                let pill = Rect::from_center_size(
                    at,
                    vec2(galley.size().x + 12.0, 16.0).max(vec2(18.0, 16.0)),
                );
                lane_painter.rect_filled(
                    pill,
                    8.0,
                    if m.fixed {
                        color
                    } else {
                        color.gamma_multiply(0.35)
                    },
                );
                lane_painter.rect_stroke(pill, 8.0, Stroke::new(1.0, color), StrokeKind::Inside);
                lane_painter.galley(
                    pill.center() - galley.size() / 2.0,
                    galley,
                    visuals.strong_text_color(),
                );
            } else if m.fixed {
                lane_painter.circle_filled(at, 5.0, color);
            } else {
                lane_painter.circle_filled(at, 5.0, visuals.extreme_bg_color);
                lane_painter.circle_stroke(at, 4.5, Stroke::new(1.5, color));
            }
        }
        if threshold_shown {
            let color = Sort::Background.color();
            let lit = hover == Under::Threshold;
            lane_painter.rect_filled(tab, 9.0, color.gamma_multiply(if lit { 0.75 } else { 0.4 }));
            lane_painter.text(
                tab.center(),
                Align2::CENTER_CENTER,
                trf!("Threshold {db}", db = format!("{threshold:.0} dB")),
                FontId::proportional(10.5),
                visuals.strong_text_color(),
            );
        }
        if let Some((a, b)) = editor.selection {
            let span = Rect::from_min_max(
                pos2(scale.x(a), band.top()),
                pos2(scale.x(b), band.bottom()),
            );
            lane_painter.rect_stroke(
                span,
                0.0,
                Stroke::new(1.0, visuals.selection.bg_fill),
                StrokeKind::Inside,
            );
        }
        let position = doc.player().position();
        editor_frame::playhead(&painter, scale.x(position), ruler.top(), lane.bottom());
        self.audio_ruler(ui, doc, project, editor, ruler, scale, position);

        let gesture = Gesture {
            band,
            lane,
            scale,
            map: &map,
            sets: &sets,
            lines: &lines,
            guides: &guides,
            marks: &marks,
            hits: &hits,
            found_hits: &found_hits,
            parts: parts.as_ref(),
            threshold,
            frames,
            rate,
            settings,
        };
        if std::mem::take(&mut editor.select_all) {
            editor.selection = Some((clip.start, clip.end()));
        }
        // What a menu asked, now that the regions and the hits are known.
        if let Some((sort, action)) = editor.asked.take() {
            self.run_chip(doc, clip, editor, &gesture, sort, action);
            return;
        }
        if editor.drag.is_none() {
            editor.hint = None;
        }
        if editor.tool == FrameTool::Zoom {
            if let Some(z) = editor_frame::zoom_tool(ui, lane, &painter, &mut editor.zoom_from) {
                match z {
                    Zoomed::Span(a, b) => {
                        editor.scroll = scale.sample(a) as f64;
                        editor.zoom = (f64::from(b - a) * editor.zoom / f64::from(lane.width()))
                            .clamp(MIN_ZOOM, MAX_ZOOM);
                    }
                    Zoomed::In(x) | Zoomed::Out(x) => {
                        let at = editor.scroll + f64::from(x - lane.left()) * editor.zoom;
                        let factor = if matches!(z, Zoomed::In(_)) { 0.5 } else { 2.0 };
                        editor.zoom = (editor.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
                        editor.scroll = at - f64::from(x - lane.left()) * editor.zoom;
                    }
                }
            }
            editor.hint = Some(tr(
                "Drag across a span to fill the view with it, click to zoom in, Alt+click to \
                 zoom out",
            ));
        } else {
            self.audio_gestures(ui, doc, clip, editor, &gesture, &under_of, &painter);
        }
    }

    /// The ruler: a press plays from there while held, a drag follows.
    #[allow(clippy::too_many_arguments)]
    fn audio_ruler(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        editor: &mut AudioEditor,
        ruler: Rect,
        scale: Scale,
        position: u64,
    ) {
        let response = ui.interact(
            ruler,
            ui.scope_id().with("audio-ruler"),
            Sense::click_and_drag(),
        );
        if let Some(pos) = response.interact_pointer_pos()
            && response.is_pointer_button_down_on()
        {
            let at = scale.sample(pos.x);
            if editor.scrub.is_none() {
                editor.scrub = Some(doc.player().engine().playing());
                doc.player_mut().locate(at);
                if !doc.player().engine().playing() {
                    doc.player_mut().engine_mut().play();
                }
            } else if response.dragged()
                && (at as i64 - position as i64).unsigned_abs()
                    > u64::from(project.sample_rate / 25)
            {
                doc.player_mut().locate(at);
            }
        }
        if !response.is_pointer_button_down_on()
            && let Some(was) = editor.scrub.take()
            && !was
        {
            doc.player_mut().engine_mut().stop();
        }
        response.on_hover_text(tr(
            "Press to hear from there while held, drag to follow the pointer",
        ));
    }

    /// The hits of the clip as the stack places them, whatever is heard now.
    fn stack_hits(&self, clip: &audiostrada_core::Clip, g: &Gesture) -> Vec<Moved> {
        let near = u64::from(g.rate / 100);
        placed_hits(g.found_hits, &self.current_edits(clip), g.map.window(), near)
    }

    /// Writes the hits of the clip, `hits` where each one plays: those moved, and what holds
    /// the audio around them. The hits of the stack outside the clip stay.
    fn write_hits(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        g: &Gesture,
        hits: &[Moved],
        label: &str,
    ) {
        let mut edits = self.current_edits(clip);
        let window = g.map.window();
        let mut all: Vec<Moved> = edits
            .entries
            .iter()
            .filter_map(|e| match &e.kind {
                Kind::Moves { hits } => Some(hits.clone()),
                _ => None,
            })
            .flatten()
            .filter(|h| h.at < window.0 || h.at >= window.1)
            .collect();
        all.extend(with_holds(hits, g.frames));
        all.sort_by_key(|h| h.at);
        all.dedup_by_key(|h| h.at);
        edits
            .entries
            .retain(|e| !matches!(e.kind, Kind::Moves { .. }));
        edits.entries.push(Entry::new(Kind::Moves { hits: all }));
        self.commit_edits(doc, clip.id, edits, label);
    }

    /// Does what a chip's menu asked, for every region or hit of its sort.
    fn run_chip(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        g: &Gesture,
        sort: Sort,
        action: ChipAction,
    ) {
        let mut edits = self.current_edits(clip);
        let found = self.audio_found.get(&clip.source).cloned();
        let entry = edits.entries.iter().position(|e| sort.is(&e.kind));
        let regions_of = |sort: Sort| -> Vec<Region> {
            if let Some(set) = g.sets.iter().find(|s| s.owner == Owner::Sort(sort)) {
                return set.regions.clone();
            }
            match (&found, g.parts) {
                (Some(f), Some(parts)) => {
                    found_regions(f, sort, parts, g.map.window(), &editor.ignored)
                }
                _ => Vec::new(),
            }
        };
        let label = match action {
            ChipAction::Dose(db) => {
                let k = entry.unwrap_or_else(|| {
                    let kind = sort.kind(regions_of(sort), 0.0, g.threshold);
                    edits.entries.insert(0, Entry::new(kind));
                    0
                });
                edits.entries[k].on = true;
                edits.entries[k].kind.set_db(db);
                // Each one follows: none keeps a level of its own.
                if let Some(regions) = edits.entries[k].kind.regions_mut() {
                    regions.iter_mut().for_each(|r| r.db = None);
                }
                tr("Set a level")
            }
            ChipAction::Repair => {
                edits.entries.retain(|e| !sort.is(&e.kind));
                edits
                    .entries
                    .insert(0, Entry::new(sort.kind(regions_of(sort), 0.0, 0.0)));
                tr("Repair the audio")
            }
            ChipAction::Even => {
                let Some(found) = &found else {
                    return;
                };
                let regions = evened(&found.profile, regions_of(Sort::Phrases));
                edits.entries.retain(|e| !sort.is(&e.kind));
                edits
                    .entries
                    .insert(0, Entry::new(Kind::Phrases { regions }));
                tr("Even out the phrases")
            }
            ChipAction::Shorten(seconds) => {
                let keep = (seconds * g.rate as f32) as u64;
                edits
                    .entries
                    .retain(|e| !matches!(e.kind, Kind::Shorten { .. }));
                edits.entries.push(Entry::new(Kind::Shorten {
                    regions: regions_of(Sort::Background),
                    keep,
                }));
                tr("Shorten the pauses")
            }
            ChipAction::PausesBack => {
                edits
                    .entries
                    .retain(|e| !matches!(e.kind, Kind::Shorten { .. }));
                tr("Shorten the pauses")
            }
            ChipAction::SnapUnder | ChipAction::SnapGrid => {
                let hits = self.stack_hits(clip, g);
                let project = doc.project();
                let reach = (SNAP_REACH * g.rate as f32) as u64;
                let grid = g.settings.grid.beats();
                let snapped = snapped_hits(
                    &hits,
                    |place| {
                        let sample = g.map.sample_moved(place);
                        let target = if action == ChipAction::SnapGrid {
                            edit::snap(
                                sample.max(0) as u64,
                                grid,
                                &project.tempo,
                                project.sample_rate,
                            ) as i64
                        } else {
                            g.guides
                                .iter()
                                .map(|&u| u as i64)
                                .min_by_key(|u| (u - sample).abs())?
                        };
                        Some(g.map.moved(target))
                    },
                    reach,
                    g.frames,
                );
                let count = snapped
                    .iter()
                    .zip(&hits)
                    .filter(|(a, b)| a.to != b.to)
                    .count();
                doc.notify(if count == 0 {
                    tr("Every hit is in place already").to_owned()
                } else {
                    trf!("{count} hits lined up", count = count)
                });
                if count > 0 {
                    self.write_hits(doc, clip, g, &snapped, tr("Line up the hits"));
                }
                return;
            }
            ChipAction::HitsBack => {
                edits
                    .entries
                    .retain(|e| !matches!(e.kind, Kind::Moves { .. }));
                tr("Hits back in place")
            }
            ChipAction::Switch => {
                let Some(k) = entry else {
                    return;
                };
                edits.entries[k].on = !edits.entries[k].on;
                tr("Switch a retouch")
            }
            ChipAction::TakeBack => {
                let Some(k) = entry else {
                    return;
                };
                edits.entries.remove(k);
                tr("Take a retouch out")
            }
            ChipAction::Sensitivity(_) => return,
        };
        self.commit_edits(doc, clip.id, edits, label);
    }

    /// Writes a level pulled onto a set of regions: its sort's retouch, a part, or the
    /// selection made a part. `reset` gives a region back the level of the others.
    #[allow(clippy::too_many_arguments)]
    fn commit_level(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        set: &Shown,
        (region, alone, gain): (usize, bool, f32),
        reset: bool,
        threshold: f32,
    ) {
        let mut edits = self.current_edits(clip);
        let gain = stored(gain);
        let own = |gain: f32| (!reset).then_some(gain);
        match set.owner {
            Owner::Sort(sort) => {
                let k = match edits.entries.iter().position(|e| sort.is(&e.kind)) {
                    Some(k) => k,
                    None => {
                        edits.entries.insert(
                            0,
                            Entry::new(sort.kind(set.regions.clone(), 0.0, threshold)),
                        );
                        0
                    }
                };
                let entry = &mut edits.entries[k];
                entry.on = true;
                if alone || !sort.together() {
                    if let Some(r) = entry.kind.regions_mut().and_then(|r| r.get_mut(region)) {
                        // A phrase at 0 dB plays as recorded: nothing to keep.
                        r.db = own(gain).filter(|g| sort.together() || g.abs() > 0.05);
                    }
                } else {
                    entry.kind.set_db(gain);
                }
            }
            Owner::Part(k) => {
                if let Some(entry) = edits.entries.get_mut(k) {
                    if alone {
                        if let Some(r) = entry.kind.regions_mut().and_then(|r| r.get_mut(region)) {
                            r.db = own(gain);
                        }
                    } else {
                        entry.kind.set_db(gain);
                    }
                }
            }
            Owner::Selection => {
                edits.entries.push(Entry::new(Kind::Gain {
                    regions: set.regions.clone(),
                    db: gain,
                }));
                editor.selection = None;
            }
        }
        self.commit_edits(doc, clip.id, edits, tr("Set a level"));
    }

    /// Sets a region aside: out of its retouch, and no longer proposed.
    fn set_aside(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        set: &Shown,
        region: usize,
    ) {
        let Some(r) = set.regions.get(region).copied() else {
            return;
        };
        let mut edits = self.current_edits(clip);
        let k = match set.owner {
            Owner::Sort(sort) => {
                editor.ignored.push((sort, r.span()));
                edits.entries.iter().position(|e| sort.is(&e.kind))
            }
            Owner::Part(k) => Some(k),
            Owner::Selection => {
                editor.selection = None;
                None
            }
        };
        if let Some(regions) = k
            .and_then(|k| edits.entries.get_mut(k))
            .and_then(|e| e.kind.regions_mut())
        {
            regions.retain(|o| o.start != r.start || o.end != r.end);
            // A retouch left with nothing to act on goes with its last region.
            edits.entries.retain(|e| {
                matches!(e.kind, Kind::Moves { .. } | Kind::Normalize { .. })
                    || !e.kind.regions().is_empty()
            });
            self.commit_edits(doc, clip.id, edits, tr("Set a region aside"));
        }
    }

    /// What the pointer can do where it is, for the right of the chips.
    fn hint_of(editor: &AudioEditor, g: &Gesture, under: Under) -> &'static str {
        if editor.tool == FrameTool::Erase {
            return tr("Drag across a noise to remove it; click a region to set it aside");
        }
        match under {
            Under::Line(s, _) if g.sets[s].together() => tr(
                "Pull: the level of them all · Alt: this one alone · double-click: back to 0 dB",
            ),
            Under::Line(..) => tr("Pull: its level · double-click: back to 0 dB"),
            Under::Edge(..) => tr("Pull the edge to where the region should start or end"),
            Under::Pause(_) => tr("Pull: every pause squeezed · Alt: this one alone"),
            Under::Mark(k) if g.marks[k].count > 1 => tr("Click: a closer look at these"),
            Under::Mark(k) if g.marks[k].fixed => tr("Click: as it was recorded again"),
            Under::Mark(_) => tr("Click: repair it"),
            Under::Threshold => tr("Pull: what stays under the line is background"),
            Under::Attack(_) => tr(
                "Slide: this hit moves in time · Ctrl: free of the magnet · double-click: back \
                 in place · Shift+drag: select",
            ),
            Under::Region(s, _) if g.sets[s].owner == Owner::Selection => tr(
                "Pull its line: its level · Delete: remove it · right-click: more",
            ),
            _ => tr(
                "Drag: select a part · double-click: the whole phrase · Alt+drag: move the clip",
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn audio_gestures(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        g: &Gesture,
        under_of: &dyn Fn(Pos2) -> Under,
        painter: &egui::Painter,
    ) {
        let response = ui.interact(
            g.lane,
            ui.scope_id().with("audio-lane"),
            Sense::click_and_drag(),
        );
        let (alt, shift, free) =
            ui.input(|i| (i.modifiers.alt, i.modifiers.shift, i.modifiers.command));
        // The pointer's look tells what a drag would do.
        if let Some(pos) = response.hover_pos()
            && editor.drag.is_none()
        {
            let under = under_of(pos);
            let icon = match (editor.tool, under) {
                (FrameTool::Erase, Under::Region(..) | Under::Line(..)) => CursorIcon::PointingHand,
                (FrameTool::Erase, _) => CursorIcon::Crosshair,
                (_, Under::Line(..) | Under::Threshold) => CursorIcon::ResizeVertical,
                (_, Under::Edge(..) | Under::Pause(..)) => CursorIcon::ResizeHorizontal,
                (_, Under::Mark(..)) => CursorIcon::PointingHand,
                _ if alt => CursorIcon::Grab,
                (_, Under::Attack(_)) if !shift => CursorIcon::Grab,
                _ => CursorIcon::Text,
            };
            ui.ctx().set_cursor_icon(icon);
            if g.band.contains(pos) {
                editor.hint = Some(Self::hint_of(editor, g, under));
            }
        }
        // Delete removes the part selected, else sets aside what the pointer is over.
        if std::mem::take(&mut self.audio_delete)
            && let Some(pos) = response.hover_pos()
        {
            match under_of(pos) {
                _ if editor.selection.is_some() => {
                    self.run_menu(doc, clip, editor, g, MenuAction::Silence);
                }
                Under::Line(s, j) | Under::Region(s, j) | Under::Edge(s, j, _) => {
                    self.set_aside(doc, clip, editor, &g.sets[s], j);
                }
                Under::Attack(k) => {
                    self.run_menu(doc, clip, editor, g, MenuAction::HitBack(g.hits[k].at));
                }
                _ => {}
            }
            return;
        }
        let pos = response.interact_pointer_pos().or(response.hover_pos());
        if response.secondary_clicked()
            && let Some(pos) = pos
        {
            editor.menu = Some((under_of(pos), g.scale.sample(pos.x)));
        }
        let mut menu_action = None;
        let menu = editor.menu;
        let selection = editor.selection;
        let under_name = editor
            .under
            .and_then(|t| doc.project().track(t))
            .map(|t| t.name.clone());
        response.context_menu(|ui| {
            if let Some((under, at)) = menu {
                Self::audio_menu(ui, selection, g, under, at, under_name.as_deref(), &mut menu_action);
            }
        });
        if let Some(action) = menu_action {
            self.run_menu(doc, clip, editor, g, action);
            return;
        }
        let Some(pos) = pos else {
            return;
        };
        let at = g.scale.sample(pos.x);
        if response.drag_started() {
            let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let start = g.scale.sample(from.x);
            let under = under_of(from);
            editor.drag = Some(match (editor.tool, under) {
                (FrameTool::Erase, _) => Drag::Erase(start),
                (_, Under::Line(s, j)) => Drag::Level {
                    shown: s,
                    region: j,
                    alone: alt || !g.sets[s].together(),
                    gain: g.lines[s][j].0,
                },
                (_, Under::Pause(j)) => {
                    let len = g
                        .sets
                        .iter()
                        .find(|s| s.owner == Owner::Sort(Sort::Background))
                        .and_then(|s| s.regions.get(j))
                        .map_or(0, |r| r.end - r.start);
                    Drag::Pause {
                        region: j,
                        alone: alt,
                        keep: len,
                        from: start,
                    }
                }
                (_, Under::Threshold) => Drag::Threshold(g.threshold),
                _ if alt => Drag::Move {
                    grab: start,
                    moved: 0,
                },
                (_, Under::Edge(s, j, start_edge)) => Drag::Edge {
                    shown: s,
                    region: j,
                    start: start_edge,
                    to: g.map.frame(start),
                },
                (_, Under::Attack(k)) if !shift => Drag::Hit {
                    at: g.hits[k].at,
                    to: g.hits[k].to,
                    grab: start as i64 - g.map.sample_moved(g.hits[k].to),
                    snapped: false,
                },
                _ => Drag::Select(start),
            });
        }
        if response.dragged() {
            match &mut editor.drag {
                Some(Drag::Select(anchor)) => {
                    let (a, b) = ((*anchor).min(at), (*anchor).max(at));
                    editor.selection = (b > a).then_some((a, b));
                }
                Some(Drag::Level { shown, gain, .. }) => {
                    *gain = gain_at(g.band, pos.y).min(g.sets[*shown].ceiling());
                }
                Some(Drag::Edge { to, .. }) => *to = g.map.frame(at),
                Some(Drag::Pause {
                    keep, region, from, ..
                }) => {
                    let len = g
                        .sets
                        .iter()
                        .find(|s| s.owner == Owner::Sort(Sort::Background))
                        .and_then(|s| s.regions.get(*region))
                        .map_or(0, |r| r.end - r.start);
                    // Each side of the pause comes in by the way the pointer went.
                    let moved = (at as i64 - *from as i64).unsigned_abs();
                    let frames = (moved as f64 * 2.0 / g.map.per) as u64;
                    let shortest = (SHORTEST * g.rate as f32) as u64;
                    *keep = len
                        .saturating_sub(frames)
                        .max(shortest)
                        .min(len.max(shortest));
                }
                Some(Drag::Threshold(t)) => {
                    // By the way the pointer goes: its line follows at the same pace.
                    let step = LOWEST / wave_of(g.band).height().max(1.0);
                    let moved = ui.input(|i| i.pointer.delta().y);
                    *t = (*t + moved * step).clamp(-90.0, -6.0);
                }
                Some(Drag::Hit {
                    at: found,
                    to,
                    grab,
                    snapped,
                }) => {
                    let wanted = at as i64 - *grab;
                    let mut place = wanted;
                    *snapped = false;
                    if !free {
                        // Onto a hit of the track under it, the grid, or where it was found.
                        let project = doc.project();
                        let reach = (8.0 * g.scale.zoom) as i64;
                        let mut targets: Vec<(i64, bool)> =
                            g.guides.iter().map(|&u| (u as i64, true)).collect();
                        targets.push((g.map.sample_moved(*found), false));
                        if g.settings.snapping() {
                            let line = g.settings.snap(project, wanted.max(0) as u64);
                            targets.push((line as i64, true));
                        }
                        if let Some((t, shown)) = targets
                            .into_iter()
                            .filter(|(t, _)| (t - wanted).abs() <= reach)
                            .min_by_key(|(t, _)| (t - wanted).abs())
                        {
                            place = t;
                            *snapped = shown;
                        }
                    }
                    if let Some(k) = g.hits.iter().position(|h| h.at == *found) {
                        let (before, after) = beside(g.hits, k, g.frames);
                        if let Some((low, high)) = room(before, *found, after) {
                            let wanted = g.map.moved(place);
                            *to = wanted.clamp(low, high);
                            *snapped &= *to == wanted;
                        }
                    }
                }
                Some(Drag::Move { grab, moved }) => {
                    let mut m = at as i64 - *grab as i64;
                    // Snapped to the hits of the track under it.
                    let near = (8.0 * g.scale.zoom) as i64;
                    let own: Vec<u64> = edit::track_of_clip(doc.project(), clip.id)
                        .map(|t| self.attacks(doc, t, true))
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|t| (clip.start..clip.end()).contains(t))
                        .collect();
                    let mut best: Option<i64> = None;
                    for &o in &own {
                        for &u in g.guides {
                            let d = u as i64 - (o as i64 + m);
                            if d.abs() < near && best.is_none_or(|b| d.abs() < b.abs()) {
                                best = Some(d);
                            }
                        }
                    }
                    if let Some(d) = best {
                        m += d;
                    }
                    *moved = m;
                }
                Some(Drag::Erase(_)) | None => {}
            }
        }
        // What the drag does, beside the pointer.
        let project_rate = doc.project().sample_rate;
        let text = match editor.drag {
            Some(Drag::Level {
                shown, gain, alone, ..
            }) => {
                let what = match g.sets.get(shown).map(|s| s.owner) {
                    Some(Owner::Sort(sort)) if !alone => trf!("{sort}: all", sort = sort.name()),
                    Some(Owner::Sort(sort)) if sort.together() => {
                        trf!("{sort}: this one", sort = sort.name())
                    }
                    Some(Owner::Sort(Sort::Phrases)) => tr("This phrase").to_owned(),
                    _ => tr("This part").to_owned(),
                };
                Some(format!("{what} · {}", gain_text(gain)))
            }
            Some(Drag::Pause { keep, alone, .. }) => Some(trf!(
                "{what}: {s} s",
                what = if alone {
                    tr("This pause")
                } else {
                    tr("Every pause")
                },
                s = format!("{:.2}", keep as f32 / g.rate as f32)
            )),
            Some(Drag::Threshold(t)) => Some(trf!("Threshold {db}", db = format!("{t:.0} dB"))),
            Some(Drag::Hit { at, to, .. }) => {
                Some(ms_text(to as i64 - at as i64, g.rate))
            }
            Some(Drag::Move { moved, .. }) => Some(ms_text(moved, project_rate)),
            _ => None,
        };
        if let Some(text) = text {
            pointer_tag(painter, pos, text);
        }
        if let Some(Drag::Erase(from)) = editor.drag {
            let span = Rect::from_min_max(
                pos2(g.scale.x(from.min(at)), g.band.top()),
                pos2(g.scale.x(from.max(at)), g.band.bottom()),
            );
            painter.rect_filled(span, 0.0, Color32::from_rgba_unmultiplied(240, 80, 80, 40));
        }
        if matches!(editor.drag, Some(Drag::Hit { .. } | Drag::Move { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        if response.drag_stopped() {
            if let Some(drag) = editor.drag.take() {
                self.end_drag(doc, clip, editor, g, drag, at);
            }
            return;
        }
        if response.double_clicked() {
            match under_of(pos) {
                Under::Line(s, j) if g.sets[s].applied => {
                    let set = &g.sets[s];
                    let alone = alt || !set.together();
                    self.commit_level(doc, clip, editor, set, (j, alone, 0.0), true, g.threshold);
                }
                Under::Attack(k) if g.hits[k].at != g.hits[k].to => {
                    self.run_menu(doc, clip, editor, g, MenuAction::HitBack(g.hits[k].at));
                }
                Under::Line(..) | Under::Mark(_) | Under::Threshold => {}
                _ => {
                    // The phrase, the breath or the pause there, as a whole.
                    let frame = g.map.frame(at) as usize;
                    editor.selection = g.parts.and_then(|parts| {
                        [&parts.breaths, &parts.phrases, &parts.background]
                            .into_iter()
                            .flatten()
                            .find(|r| r.0 <= frame && frame < r.1)
                            .map(|r| {
                                (
                                    g.map.sample(r.0 as u64).max(clip.start as i64) as u64,
                                    (g.map.sample(r.1 as u64).max(0) as u64).min(clip.end()),
                                )
                            })
                            .filter(|(a, b)| b > a)
                    });
                    if let Some(span) = editor.selection {
                        doc.history_mut()
                            .modify_unrecorded(|p| p.loop_range = Some(span));
                    }
                }
            }
            return;
        }
        if response.clicked() {
            match (editor.tool, under_of(pos)) {
                (_, Under::Mark(k)) => {
                    let m = g.marks[k];
                    if m.count == 1 {
                        self.toggle_mark(doc, clip, editor, g, m.sort, m.first);
                    } else {
                        // Several together: closer, to tell them apart.
                        let (a, b) = (g.map.sample(m.start), g.map.sample(m.last));
                        let width = f64::from(g.lane.width());
                        editor.zoom =
                            ((b - a).max(1) as f64 * 3.0 / width).clamp(MIN_ZOOM, MAX_ZOOM);
                        editor.scroll = ((a + b) as f64 / 2.0
                            - f64::from(g.lane.width()) / 2.0 * editor.zoom)
                            .max(0.0);
                    }
                }
                (FrameTool::Erase, Under::Region(s, j) | Under::Line(s, j)) => {
                    self.set_aside(doc, clip, editor, &g.sets[s], j);
                }
                (_, Under::Nothing | Under::Attack(_) | Under::Region(..)) => {
                    editor.selection = None;
                    doc.player_mut().locate(at);
                }
                _ => {}
            }
        }
    }

    /// Writes what a drag did once it is let go.
    fn end_drag(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        g: &Gesture,
        drag: Drag,
        at: u64,
    ) {
        match drag {
            Drag::Select(_) => {
                if let Some(span) = editor.selection {
                    doc.history_mut()
                        .modify_unrecorded(|p| p.loop_range = Some(span));
                }
            }
            Drag::Level {
                shown,
                region,
                alone,
                gain,
            } => {
                if let Some(set) = g.sets.get(shown) {
                    let level = (region, alone, gain);
                    self.commit_level(doc, clip, editor, set, level, false, g.threshold);
                }
            }
            Drag::Edge {
                shown,
                region,
                start,
                to,
            } => self.commit_edge(doc, clip, editor, g, (shown, region, start, to)),
            Drag::Pause {
                region,
                alone,
                keep,
                ..
            } => {
                let pauses = g
                    .sets
                    .iter()
                    .find(|s| s.owner == Owner::Sort(Sort::Background))
                    .map(|s| s.regions.clone())
                    .unwrap_or_default();
                let mut edits = self.current_edits(clip);
                let k = match edits
                    .entries
                    .iter()
                    .position(|e| matches!(e.kind, Kind::Shorten { .. }))
                {
                    Some(k) => k,
                    None => {
                        let longest = pauses.iter().map(|r| r.end - r.start).max();
                        edits.entries.push(Entry::new(Kind::Shorten {
                            regions: pauses.iter().map(|r| Region::new(r.start, r.end)).collect(),
                            keep: longest.unwrap_or(0),
                        }));
                        edits.entries.len() - 1
                    }
                };
                if let Kind::Shorten { regions, keep: all } = &mut edits.entries[k].kind {
                    if alone {
                        if let Some(r) = pauses.get(region)
                            && let Some(o) = regions
                                .iter_mut()
                                .find(|o| o.start < r.end && r.start < o.end)
                        {
                            o.keep = Some(keep);
                        }
                    } else {
                        *all = keep;
                    }
                }
                edits.entries[k].on = true;
                self.commit_edits(doc, clip.id, edits, tr("Shorten the pauses"));
            }
            Drag::Threshold(t) => {
                let t = t.round();
                editor.threshold = Some(t);
                editor.cache = None;
                // What was retouched already follows the new threshold.
                let Some(found) = self.audio_found.get(&clip.source).cloned() else {
                    return;
                };
                let parts = Self::found_at(editor, &found);
                let mut edits = self.current_edits(clip);
                let mut changed = false;
                for sort in [Sort::Phrases, Sort::Background, Sort::Breaths] {
                    if let Some(e) = edits.entries.iter_mut().find(|e| sort.is(&e.kind)) {
                        let new =
                            found_regions(&found, sort, &parts, g.map.window(), &editor.ignored);
                        if let Some(regions) = e.kind.regions_mut() {
                            *regions = carry_regions(regions, new);
                        }
                        if let Kind::Background { threshold_db, .. } = &mut e.kind {
                            *threshold_db = t;
                        }
                        changed = true;
                    }
                }
                if changed {
                    self.commit_edits(doc, clip.id, edits, tr("Set the threshold"));
                }
            }
            Drag::Hit { at: found, to, .. } => {
                let mut hits = self.stack_hits(clip, g);
                match hits.iter_mut().find(|h| h.at == found) {
                    Some(h) if h.to != to => h.to = to,
                    _ => return,
                }
                self.write_hits(doc, clip, g, &hits, tr("Move a hit"));
            }
            Drag::Move { moved, .. } if moved != 0 => {
                doc.history_mut().edit(tr("Move clips"), |p| {
                    edit::move_clips(p, &[clip.id], moved, 0);
                });
            }
            Drag::Erase(from) => {
                let (a, b) = (g.map.frame(from.min(at)), g.map.frame(from.max(at)));
                if b > a {
                    let mut edits = self.current_edits(clip);
                    edits.entries.push(Entry::new(Kind::Gain {
                        regions: vec![Region::new(a, b)],
                        db: REMOVED_DB,
                    }));
                    self.commit_edits(doc, clip.id, edits, tr("Erase a noise"));
                }
            }
            Drag::Move { .. } => {}
        }
    }

    /// Repairs a click or a clipped peak, or takes the repair back.
    fn toggle_mark(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &AudioEditor,
        g: &Gesture,
        sort: Sort,
        mark: usize,
    ) {
        let (Some(found), Some(parts)) = (self.audio_found.get(&clip.source).cloned(), g.parts)
        else {
            return;
        };
        let marks = found_regions(&found, sort, parts, g.map.window(), &editor.ignored);
        let Some(r) = marks.get(mark).copied() else {
            return;
        };
        let mut edits = self.current_edits(clip);
        let k = match edits.entries.iter().position(|e| sort.is(&e.kind)) {
            Some(k) => k,
            None => {
                edits
                    .entries
                    .insert(0, Entry::new(sort.kind(Vec::new(), 0.0, 0.0)));
                0
            }
        };
        if let Some(regions) = edits.entries[k].kind.regions_mut() {
            match regions
                .iter()
                .position(|o| o.start < r.end && r.start < o.end)
            {
                Some(i) => {
                    regions.remove(i);
                }
                None => regions.push(r),
            }
        }
        edits.entries[k].on = true;
        self.commit_edits(doc, clip.id, edits, tr("Repair the audio"));
    }

    /// Writes a region's edge moved: in its retouch, or in a new one made of what was found.
    fn commit_edge(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        g: &Gesture,
        (shown, region, start, to): (usize, usize, bool, u64),
    ) {
        let Some(set) = g.sets.get(shown) else {
            return;
        };
        let mut regions = set.regions.clone();
        if let Some(r) = regions.get_mut(region) {
            if start {
                r.start = to.min(r.end.saturating_sub(1));
            } else {
                r.end = to.max(r.start + 1);
            }
        }
        let mut edits = self.current_edits(clip);
        match set.owner {
            Owner::Selection => {
                if let Some(r) = regions.first() {
                    editor.selection = Some((
                        g.map.sample(r.start).max(0) as u64,
                        g.map.sample(r.end).max(0) as u64,
                    ));
                }
                return;
            }
            Owner::Part(k) => {
                if let Some(rs) = edits.entries.get_mut(k).and_then(|e| e.kind.regions_mut()) {
                    *rs = regions;
                }
            }
            Owner::Sort(sort) => match edits.entries.iter_mut().find(|e| sort.is(&e.kind)) {
                Some(e) => {
                    if let Some(rs) = e.kind.regions_mut() {
                        *rs = regions;
                    }
                }
                None => {
                    // Found only so far: its regions with this edge moved (a phrase held at
                    // 0 dB, or nothing of it would be kept).
                    let mut entry = Entry::new(sort.kind(regions, 0.0, g.threshold));
                    if sort == Sort::Phrases
                        && let Some(r) = entry.kind.regions_mut().and_then(|r| r.get_mut(region))
                    {
                        r.db = Some(0.0);
                    }
                    edits.entries.insert(0, entry);
                }
            },
        }
        self.commit_edits(doc, clip.id, edits, tr("Retouch a region"));
    }

    /// The menu of what was right-clicked.
    fn audio_menu(
        ui: &mut Ui,
        selection: Option<(u64, u64)>,
        g: &Gesture,
        under: Under,
        at: u64,
        track: Option<&str>,
        action: &mut Option<MenuAction>,
    ) {
        let mut item = |ui: &mut Ui, text: &str, a: MenuAction| {
            if ui.button(text).clicked() {
                *action = Some(a);
                ui.close();
            }
        };
        let in_selection = selection.is_some_and(|(a, b)| a <= at && at < b);
        match under {
            _ if in_selection => {
                item(ui, tr("Bring up to the voice's level"), MenuAction::ToVoice);
                item(ui, tr("Normalize to -1 dB peak"), MenuAction::Normalize);
                item(ui, tr("Lower by 6 dB"), MenuAction::Lower);
                item(ui, tr("Remove"), MenuAction::Silence);
            }
            Under::Line(s, j) | Under::Region(s, j) | Under::Edge(s, j, _) => {
                let set = &g.sets[s];
                if set.applied {
                    item(ui, tr("Back to its own level"), MenuAction::Reset(s, j));
                }
                item(ui, tr("Remove this one"), MenuAction::Remove(s, j));
                item(ui, tr("Set it aside"), MenuAction::Aside(s, j));
            }
            Under::Attack(k) => {
                let hit = g.hits[k];
                if let Some(track) = track {
                    item(
                        ui,
                        &trf!("Onto the nearest hit of {track}", track = track),
                        MenuAction::HitSnap(hit.at),
                    );
                }
                if hit.at != hit.to {
                    item(ui, tr("Back in place"), MenuAction::HitBack(hit.at));
                }
                if track.is_none() && hit.at == hit.to {
                    ui.label(
                        egui::RichText::new(tr("Slide this hit sideways to move it in time"))
                            .weak(),
                    );
                }
            }
            _ => {
                ui.label(
                    egui::RichText::new(tr("Drag across the waveform to select a part first"))
                        .weak(),
                );
            }
        }
    }

    fn run_menu(
        &mut self,
        doc: &mut Document,
        clip: &audiostrada_core::Clip,
        editor: &mut AudioEditor,
        g: &Gesture,
        action: MenuAction,
    ) {
        let selection = editor
            .selection
            .map(|(a, b)| Region::new(g.map.frame(a), g.map.frame(b)));
        let profile = self
            .audio_found
            .get(&clip.source)
            .map(|f| f.profile.clone());
        let part = |db: f32| {
            selection.map(|r| Kind::Gain {
                regions: vec![r],
                db,
            })
        };
        let (kind, label) = match action {
            MenuAction::ToVoice => {
                let gain = selection.and_then(|r| {
                    let p = profile.as_ref()?;
                    let level = p.loudness((r.start as usize, r.end as usize));
                    Some((p.voice - 6.0 - level).clamp(-24.0, 24.0))
                });
                (gain.and_then(part), tr("Set a level"))
            }
            MenuAction::Normalize => (
                selection.map(|r| Kind::Normalize {
                    span: r.span(),
                    level: Level::Peak(-1.0),
                }),
                tr("Normalize"),
            ),
            MenuAction::Lower => (part(-6.0), tr("Set a level")),
            MenuAction::Silence => (part(REMOVED_DB), tr("Erase a noise")),
            MenuAction::Reset(s, j) => {
                if let Some(set) = g.sets.get(s) {
                    self.commit_level(doc, clip, editor, set, (j, true, 0.0), true, g.threshold);
                }
                return;
            }
            MenuAction::Remove(s, j) => {
                if let Some(set) = g.sets.get(s) {
                    let level = (j, true, GONE - 1.0);
                    self.commit_level(doc, clip, editor, set, level, false, g.threshold);
                }
                return;
            }
            MenuAction::Aside(s, j) => {
                if let Some(set) = g.sets.get(s) {
                    self.set_aside(doc, clip, editor, set, j);
                }
                return;
            }
            MenuAction::HitBack(found) | MenuAction::HitSnap(found) => {
                let mut hits = self.stack_hits(clip, g);
                let Some(k) = hits.iter().position(|h| h.at == found) else {
                    return;
                };
                let to = if matches!(action, MenuAction::HitBack(_)) {
                    Some(found)
                } else {
                    let sample = g.map.sample_moved(hits[k].to);
                    let (before, after) = beside(&hits, k, g.frames);
                    g.guides
                        .iter()
                        .map(|&u| u as i64)
                        .min_by_key(|u| (u - sample).abs())
                        .map(|u| g.map.moved(u))
                        .zip(room(before, found, after))
                        .map(|(place, (low, high))| place.clamp(low, high))
                };
                if let Some(to) = to.filter(|to| *to != hits[k].to) {
                    hits[k].to = to;
                    self.write_hits(doc, clip, g, &hits, tr("Move a hit"));
                }
                return;
            }
        };
        if let Some(kind) = kind {
            let mut edits = self.current_edits(clip);
            edits.entries.push(Entry::new(kind));
            editor.selection = None;
            self.commit_edits(doc, clip.id, edits, label);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_copies_line_up_where_one_was_moved() {
        let a: Vec<f32> = (0..400)
            .map(|i| ((i * 7919) % 97) as f32 / 97.0 - 0.5)
            .collect();
        let mut b = vec![0.0; 5];
        b.extend_from_slice(&a[..395]);
        let (corr, lag) = line_up(&a, &b, 20).unwrap();
        assert_eq!(lag, 5);
        assert!(corr > 0.95);
        let inverted: Vec<f32> = b.iter().map(|v| -v).collect();
        let (corr, _) = line_up(&a, &inverted, 20).unwrap();
        assert!(corr < -0.95, "opposite polarity");
    }

    #[test]
    fn a_level_line_and_its_level_go_both_ways() {
        let band = Rect::from_min_size(pos2(0.0, 100.0), vec2(500.0, 300.0));
        for gain in [12.0, 6.0, -6.0, -30.0] {
            assert!((gain_at(band, y_of_gain(band, gain)) - gain).abs() < 0.01);
        }
        assert_eq!(gain_at(band, y_of_gain(band, 0.0)), 0.0);
        assert_eq!(gain_at(band, y_of_gain(band, 0.3)), 0.0, "0 dB holds the line");
        assert!(y_of_gain(band, 6.0) < y_of_gain(band, 0.0), "louder is higher");
        assert!(y_of_gain(band, -20.0) > y_of_gain(band, 0.0));
        assert!(
            gain_at(band, wave_of(band).bottom()) <= GONE,
            "removed at the bottom"
        );
        assert!(y_of_level(band, -10.0) < y_of_level(band, -40.0));
    }

    fn parts() -> Parts {
        Parts::default()
    }

    fn found(clicks: Vec<(usize, usize)>) -> Found {
        Found {
            profile: Profile::default(),
            clicks,
            clipping: Vec::new(),
            onsets: Onsets::default(),
            peak: 1.0,
        }
    }

    #[test]
    fn regions_found_stay_inside_the_clip_and_those_set_aside_go() {
        let found = found(vec![(10, 20), (50, 60), (90, 120)]);
        let aside = [(Sort::Clicks, Span::new(55, 58))];
        let regions = found_regions(&found, Sort::Clicks, &parts(), (15, 100), &aside);
        let spans: Vec<(u64, u64)> = regions.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(spans, vec![(15, 20), (90, 100)]);
    }

    #[test]
    fn regions_found_again_keep_what_was_set_apart() {
        let old = vec![Region {
            db: Some(-9.0),
            ..Region::new(100, 200)
        }];
        let new = carry_regions(&old, vec![Region::new(120, 210), Region::new(400, 500)]);
        assert_eq!(new[0].db, Some(-9.0));
        assert_eq!(new[1].db, None);
    }

    fn hit(at: u64, to: u64) -> Moved {
        Moved { at, to }
    }

    fn moves(hits: &[Moved]) -> AudioEdits {
        AudioEdits {
            entries: vec![Entry::new(Kind::Moves {
                hits: hits.to_vec(),
            })],
        }
    }

    #[test]
    fn the_hits_of_a_clip_are_those_found_where_the_stack_moved_them() {
        let stack = moves(&[hit(1_000, 1_000), hit(2_000, 2_300), hit(3_000, 3_000)]);
        let placed = placed_hits(&[1_005, 2_010, 3_000, 4_000, 9_000], &stack, (500, 5_000), 20);
        assert_eq!(
            placed,
            vec![
                hit(1_000, 1_000),
                hit(2_000, 2_300),
                hit(3_000, 3_000),
                hit(4_000, 4_000)
            ],
            "those of the stack once, the others where they were found, inside the clip"
        );
        // A hit found where another was moved past it gives way.
        let stack = moves(&[hit(2_000, 2_600), hit(3_000, 3_000)]);
        let placed = placed_hits(&[2_000, 2_500, 3_000], &stack, (0, 5_000), 20);
        assert_eq!(placed, vec![hit(2_000, 2_600), hit(3_000, 3_000)]);
    }

    #[test]
    fn a_hit_goes_no_further_than_its_neighbours_allow() {
        let (before, after) = (hit(1_000, 1_000), hit(3_000, 3_000));
        let (low, high) = room(before, 2_000, after).unwrap();
        assert!(low > 1_000 && high < 3_000);
        assert!(low <= 1_600 && high >= 2_400, "{low}..{high}");
        // Squeezed to half on one side at most.
        assert!(low >= 1_500 && high <= 2_500, "{low}..{high}");
        let (first, last) = beside(&[hit(2_000, 2_000)], 0, 10_000);
        assert_eq!((first, last), (hit(0, 0), hit(10_000, 10_000)));
    }

    #[test]
    fn hits_snap_to_the_nearest_target_within_reach_only() {
        let hits = [hit(1_000, 1_000), hit(2_000, 2_000), hit(3_000, 3_000)];
        let targets = [1_040u64, 2_400, 2_990];
        let nearest = |place: u64| targets.iter().copied().min_by_key(|t| t.abs_diff(place));
        let snapped = snapped_hits(&hits, nearest, 100, 10_000);
        assert_eq!(
            snapped,
            vec![hit(1_000, 1_040), hit(2_000, 2_000), hit(3_000, 2_990)],
            "the middle one is too far from any"
        );
    }

    #[test]
    fn a_moved_hit_is_written_with_what_holds_the_audio_around_it() {
        let hits = [
            hit(1_000, 1_000),
            hit(2_000, 2_000),
            hit(3_000, 3_050),
            hit(4_000, 4_000),
            hit(5_000, 5_000),
        ];
        assert_eq!(
            with_holds(&hits, 9_000),
            vec![hit(2_000, 2_000), hit(3_000, 3_050), hit(4_000, 4_000)]
        );
        let last = [hit(1_000, 1_000), hit(2_000, 1_950)];
        assert_eq!(
            with_holds(&last, 9_000),
            vec![hit(1_000, 1_000), hit(2_000, 1_950), hit(8_999, 8_999)],
            "the end of the audio holds after the last hit"
        );
        assert!(with_holds(&hits[..2], 9_000).is_empty(), "nothing moved");
    }

    #[test]
    fn phrases_come_to_the_level_of_the_middle_one() {
        let profile = Profile {
            rate: 100,
            hop: 1,
            frames: 300,
            level: [vec![-30.0; 100], vec![-20.0; 100], vec![-14.0; 100]].concat(),
            ..Profile::default()
        };
        let regions = vec![
            Region::new(0, 100),
            Region::new(100, 200),
            Region::new(200, 300),
        ];
        let evened = evened(&profile, regions);
        let gains: Vec<Option<f32>> = evened.iter().map(|r| r.db).collect();
        assert_eq!(gains, vec![Some(10.0), None, Some(-6.0)]);
    }
}
