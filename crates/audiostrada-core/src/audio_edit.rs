//! The edits of an audio clip made in the audio editor: a stack of retouches over its source,
//! each switched on or off and dosed at any time. The file is never changed: the clip plays a
//! media path standing for its source with the stack applied (see [`media_path`]).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// At or under this level a region is removed (silent).
pub const REMOVED_DB: f32 = -80.0;

/// A part of the source, in frames of the source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
        }
    }

    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// A region an edit acts on, with a level of its own when set apart from the others.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub start: u64,
    pub end: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db: Option<f32>,
    /// A pause shortened to a length of its own, frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<u64>,
}

impl Region {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
            db: None,
            keep: None,
        }
    }

    pub fn span(&self) -> Span {
        Span::new(self.start, self.end)
    }
}

/// A hit moved in time: the attack at `at` of the source plays at `to`. One left where it is
/// (`to` equal to `at`) holds the audio beyond it in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub at: u64,
    pub to: u64,
}

/// The level normalizing brings a part to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Level {
    /// Its highest peak, dBFS.
    Peak(f32),
    /// Its integrated loudness, LUFS.
    Loudness(f32),
}

/// What an edit does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    /// The background between the sounds, under `threshold_db`: lowered or brought out by
    /// `db` (removed at [`REMOVED_DB`]).
    Background {
        regions: Vec<Region>,
        db: f32,
        threshold_db: f32,
    },
    /// The phrases, each at a level of its own.
    Phrases { regions: Vec<Region> },
    /// The breaths of a voice, lowered by `db`.
    Breaths { regions: Vec<Region>, db: f32 },
    /// Parts chosen by hand, lowered or raised by `db`.
    Gain { regions: Vec<Region>, db: f32 },
    /// A part brought to a level.
    Normalize { span: Span, level: Level },
    /// Clicks and pops, drawn over.
    Clicks { regions: Vec<Region> },
    /// Clipped peaks, redrawn.
    Clipping { regions: Vec<Region> },
    /// Sibilants: their highs lowered by `db`.
    Sibilants { regions: Vec<Region>, db: f32 },
    /// Pauses longer than `keep` frames (or their own length) shortened to it, the audio after
    /// them moved up.
    Shorten { regions: Vec<Region>, keep: u64 },
    /// Hits moved in time, the audio between them stretched to follow.
    Moves { hits: Vec<Moved> },
}

impl Kind {
    /// The regions it acts on (a normalized part as one).
    pub fn regions(&self) -> Vec<Region> {
        match self {
            Kind::Background { regions, .. }
            | Kind::Phrases { regions }
            | Kind::Breaths { regions, .. }
            | Kind::Gain { regions, .. }
            | Kind::Clicks { regions }
            | Kind::Clipping { regions }
            | Kind::Sibilants { regions, .. }
            | Kind::Shorten { regions, .. } => regions.clone(),
            Kind::Normalize { span, .. } => vec![Region::new(span.start, span.end)],
            Kind::Moves { .. } => Vec::new(),
        }
    }

    /// Whether it changes nothing: no region left, no hit moved.
    pub fn is_empty(&self) -> bool {
        match self {
            Kind::Moves { hits } => hits.iter().all(|h| h.at == h.to),
            Kind::Phrases { regions } => regions.iter().all(|r| r.db.is_none()),
            other => other.regions().is_empty(),
        }
    }

    pub fn regions_mut(&mut self) -> Option<&mut Vec<Region>> {
        match self {
            Kind::Background { regions, .. }
            | Kind::Phrases { regions }
            | Kind::Breaths { regions, .. }
            | Kind::Gain { regions, .. }
            | Kind::Clicks { regions }
            | Kind::Clipping { regions }
            | Kind::Sibilants { regions, .. }
            | Kind::Shorten { regions, .. } => Some(regions),
            Kind::Normalize { .. } | Kind::Moves { .. } => None,
        }
    }

    /// Its dose in dB, for those that have one.
    pub fn db(&self) -> Option<f32> {
        match self {
            Kind::Background { db, .. }
            | Kind::Breaths { db, .. }
            | Kind::Gain { db, .. }
            | Kind::Sibilants { db, .. } => Some(*db),
            _ => None,
        }
    }

    pub fn set_db(&mut self, value: f32) {
        if let Kind::Background { db, .. }
        | Kind::Breaths { db, .. }
        | Kind::Gain { db, .. }
        | Kind::Sibilants { db, .. } = self
        {
            *db = value;
        }
    }

    /// Whether it is of the same sort as `other` (one edit of each sort found by analysis).
    pub fn same_sort(&self, other: &Kind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// One edit of the stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default = "yes", skip_serializing_if = "is_on")]
    pub on: bool,
    #[serde(flatten)]
    pub kind: Kind,
}

fn yes() -> bool {
    true
}

fn is_on(on: &bool) -> bool {
    *on
}

impl Entry {
    pub fn new(kind: Kind) -> Self {
        Self { on: true, kind }
    }
}

/// The edits of a clip, applied in order; what moves the audio comes last: the hits moved,
/// then the pauses shortened.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioEdits {
    pub entries: Vec<Entry>,
}

impl AudioEdits {
    /// Whether anything is switched on.
    pub fn active(&self) -> bool {
        self.entries.iter().any(|e| e.on)
    }

    /// The hits moved and those holding the audio in place, as (source frame, where it
    /// plays), in order and never crossing.
    pub fn anchors(&self) -> Vec<(u64, u64)> {
        let mut all: Vec<(u64, u64)> = self
            .entries
            .iter()
            .filter(|e| e.on)
            .flat_map(|e| match &e.kind {
                Kind::Moves { hits } => hits.iter().map(|h| (h.at, h.to)).collect(),
                _ => Vec::new(),
            })
            .collect();
        all.sort_unstable();
        let mut out: Vec<(u64, u64)> = Vec::new();
        for (at, to) in all {
            if at > 0 && to > 0 && out.last().is_none_or(|l| at > l.0 && to > l.1) {
                out.push((at, to));
            }
        }
        out
    }

    /// The parts cut out by shortening, sorted and apart, in source frames.
    pub fn cuts(&self) -> Vec<Span> {
        let mut cuts: Vec<Span> = self
            .entries
            .iter()
            .filter(|e| e.on)
            .flat_map(|e| match &e.kind {
                Kind::Shorten { regions, keep } => regions
                    .iter()
                    .filter_map(|r| {
                        let keep = r.keep.unwrap_or(*keep);
                        (r.end - r.start > keep).then(|| {
                            let half = keep / 2;
                            Span::new(r.start + half, r.end - (keep - half))
                        })
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .filter(|s| !s.is_empty())
            .collect();
        cuts.sort_by_key(|s| s.start);
        let mut merged: Vec<Span> = Vec::new();
        for c in cuts {
            match merged.last_mut() {
                Some(last) if c.start <= last.end => last.end = last.end.max(c.end),
                _ => merged.push(c),
            }
        }
        merged
    }

    /// How the edits move the audio in time.
    pub fn timing(&self) -> Timing {
        let anchors = self.anchors();
        let cuts = self
            .cuts()
            .into_iter()
            .map(|c| {
                Span::new(
                    warp_through(&anchors, c.start),
                    warp_through(&anchors, c.end),
                )
            })
            .filter(|s| !s.is_empty())
            .collect();
        Timing { anchors, cuts }
    }

    /// Where `frame` of the source lands in the edited audio (a frame inside a cut lands
    /// where the cut was).
    pub fn map(&self, frame: u64) -> u64 {
        self.timing().map(frame)
    }

    /// The source frame playing at `frame` of the edited audio.
    pub fn unmap(&self, frame: u64) -> u64 {
        self.timing().unmap(frame)
    }

    /// A key of what it does: equal stacks share their audio.
    pub fn key(&self) -> u64 {
        let text = serde_json::to_string(self).unwrap_or_default();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

/// How a stack moves the audio in time: its hits moved first, then its pauses cut out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timing {
    anchors: Vec<(u64, u64)>,
    cuts: Vec<Span>,
}

impl Timing {
    /// The hits moved, as (source frame, where it plays).
    pub fn anchors(&self) -> &[(u64, u64)] {
        &self.anchors
    }

    /// The parts cut out, in frames of the audio once its hits are moved.
    pub fn cuts(&self) -> &[Span] {
        &self.cuts
    }

    /// Where a source frame plays once the hits are moved, before the cuts.
    pub fn warp(&self, frame: u64) -> u64 {
        warp_through(&self.anchors, frame)
    }

    /// The source frame playing at `frame` of the audio with its hits moved.
    pub fn unwarp(&self, frame: u64) -> u64 {
        let back: Vec<(u64, u64)> = self.anchors.iter().map(|&(at, to)| (to, at)).collect();
        warp_through(&back, frame)
    }

    /// Where a source frame lands in the edited audio.
    pub fn map(&self, frame: u64) -> u64 {
        map_through(&self.cuts, self.warp(frame))
    }

    /// The source frame playing at `frame` of the edited audio.
    pub fn unmap(&self, frame: u64) -> u64 {
        self.unwarp(unmap_through(&self.cuts, frame))
    }
}

/// `frame` once the audio between `anchors` (in order, never crossing) is stretched to bring
/// each to its place; what lies past the last one follows it.
pub fn warp_through(anchors: &[(u64, u64)], frame: u64) -> u64 {
    let after = anchors.partition_point(|a| a.0 <= frame);
    let (s0, d0) = if after == 0 {
        (0, 0)
    } else {
        anchors[after - 1]
    };
    match anchors.get(after) {
        Some(&(s1, d1)) if s1 > s0 => {
            let t = (frame - s0) as f64 / (s1 - s0) as f64;
            d0 + ((d1 - d0) as f64 * t).round() as u64
        }
        _ => d0 + (frame - s0),
    }
}

/// `frame` once `cuts` (sorted, apart) are taken out.
pub fn map_through(cuts: &[Span], frame: u64) -> u64 {
    let mut removed = 0;
    for c in cuts {
        if frame >= c.end {
            removed += c.len();
        } else if frame > c.start {
            removed += frame - c.start;
        } else {
            break;
        }
    }
    frame - removed
}

/// The source frame playing at `frame` once `cuts` (sorted, apart) are taken out.
pub fn unmap_through(cuts: &[Span], frame: u64) -> u64 {
    let mut out = frame;
    for c in cuts {
        if c.start <= out {
            out += c.len();
        } else {
            break;
        }
    }
    out
}

const MARK: &str = "#audiostrada-edits=";

/// The stacks the media paths stand for, by key: what decodes a path finds its edits here.
fn registry() -> &'static Mutex<HashMap<u64, Arc<AudioEdits>>> {
    static REGISTRY: std::sync::OnceLock<Mutex<HashMap<u64, Arc<AudioEdits>>>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// Media path standing for `path` with `edits` applied (decoded by the media pool).
pub fn media_path(path: &Path, edits: &Arc<AudioEdits>) -> PathBuf {
    let key = edits.key();
    if let Ok(mut known) = registry().lock() {
        known.entry(key).or_insert_with(|| Arc::clone(edits));
    }
    format!("{}{MARK}{key:016x}", path.display()).into()
}

/// The file a media path stands for, without its edits.
pub fn strip_edits(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.rsplit_once(MARK) {
        Some((file, _)) => file.into(),
        None => path.to_path_buf(),
    }
}

/// The edits a media path stands for: none for a plain file, an error when they are unknown.
pub fn edits_of(path: &Path) -> Result<Option<Arc<AudioEdits>>, String> {
    let text = path.to_string_lossy();
    let Some((_, key)) = text.rsplit_once(MARK) else {
        return Ok(None);
    };
    let key = key.split('#').next().unwrap_or_default();
    let key = u64::from_str_radix(key, 16).map_err(|e| e.to_string())?;
    registry()
        .lock()
        .ok()
        .and_then(|known| known.get(&key).cloned())
        .map(Some)
        .ok_or_else(|| "unknown edits".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shorten(regions: &[(u64, u64)], keep: u64) -> AudioEdits {
        AudioEdits {
            entries: vec![Entry::new(Kind::Shorten {
                regions: regions.iter().map(|&(a, b)| Region::new(a, b)).collect(),
                keep,
            })],
        }
    }

    #[test]
    fn long_pauses_keep_their_length_at_each_end() {
        let edits = shorten(&[(100, 200), (300, 320)], 40);
        assert_eq!(
            edits.cuts(),
            vec![Span::new(120, 180)],
            "the short one stays"
        );
        assert_eq!(edits.map(50), 50);
        assert_eq!(edits.map(150), 120, "inside the cut, where it was");
        assert_eq!(edits.map(250), 190);
        assert_eq!(edits.unmap(190), 250);
        assert_eq!(edits.unmap(50), 50);
    }

    #[test]
    fn switched_off_shortening_moves_nothing() {
        let mut edits = shorten(&[(100, 200)], 40);
        edits.entries[0].on = false;
        assert!(edits.cuts().is_empty());
        assert_eq!(edits.map(500), 500);
    }

    fn moves(hits: &[(u64, u64)]) -> Entry {
        Entry::new(Kind::Moves {
            hits: hits.iter().map(|&(at, to)| Moved { at, to }).collect(),
        })
    }

    #[test]
    fn a_hit_moved_takes_the_audio_around_it_and_no_more() {
        let edits = AudioEdits {
            entries: vec![moves(&[(100, 100), (200, 220), (300, 300)])],
        };
        let timing = edits.timing();
        assert_eq!(timing.warp(50), 50, "before the hit that holds");
        assert_eq!(timing.warp(200), 220, "the hit itself");
        assert_eq!(timing.warp(150), 160, "the note before it, stretched");
        assert_eq!(timing.warp(250), 260, "its own, squeezed");
        assert_eq!(timing.warp(400), 400, "past the next hit, in place");
        for frame in [0, 50, 100, 200, 300, 400] {
            assert_eq!(timing.unwarp(timing.warp(frame)), frame);
        }
    }

    #[test]
    fn hits_that_would_cross_are_left_out() {
        let edits = AudioEdits {
            entries: vec![moves(&[(100, 100), (200, 90), (300, 300)])],
        };
        assert_eq!(edits.anchors(), vec![(100, 100), (300, 300)]);
        let mut off = AudioEdits {
            entries: vec![moves(&[(200, 220)])],
        };
        off.entries[0].on = false;
        assert!(off.anchors().is_empty());
    }

    #[test]
    fn pauses_are_cut_where_the_moved_hits_left_them() {
        let mut edits = shorten(&[(100, 200)], 40);
        edits.entries.push(moves(&[(50, 60), (400, 400)]));
        let timing = edits.timing();
        // The pause follows the audio stretched around it.
        let (a, b) = (timing.warp(120), timing.warp(180));
        assert_eq!(timing.cuts(), &[Span::new(a, b)]);
        assert_eq!(timing.map(400), 400 - (b - a));
        assert_eq!(timing.unmap(timing.map(300)), 300);
    }

    #[test]
    fn a_media_path_finds_its_edits_and_its_file() {
        let edits = Arc::new(shorten(&[(0, 10)], 2));
        let path = media_path(Path::new("/a/voice.wav"), &edits);
        assert_eq!(strip_edits(&path), PathBuf::from("/a/voice.wav"));
        assert_eq!(edits_of(&path).unwrap().as_deref(), Some(&*edits));
        assert_eq!(edits_of(Path::new("/a/voice.wav")).unwrap(), None);
    }

    #[test]
    fn edits_read_back_as_written() {
        let edits = AudioEdits {
            entries: vec![
                Entry::new(Kind::Background {
                    regions: vec![Region {
                        db: Some(-6.0),
                        ..Region::new(5, 9)
                    }],
                    db: REMOVED_DB,
                    threshold_db: -50.0,
                }),
                Entry::new(Kind::Moves {
                    hits: vec![Moved { at: 40, to: 46 }],
                }),
                Entry {
                    on: false,
                    kind: Kind::Normalize {
                        span: Span::new(0, 100),
                        level: Level::Loudness(-16.0),
                    },
                },
            ],
        };
        let text = serde_json::to_string(&edits).unwrap();
        assert_eq!(serde_json::from_str::<AudioEdits>(&text).unwrap(), edits);
    }
}
