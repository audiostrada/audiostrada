//! Snapping in the timeline: clip edges, markers and the playhead pull a position when it
//! comes within a few pixels, else the grid takes it. The last pull is kept to draw its guide.

use std::cell::Cell;

use audiostrada_core::{ClipId, Project};

use crate::i18n::tr;
use crate::transport::{EditMode, EditSettings};

/// How near, in pixels, an edge, a marker or the playhead pulls.
const REACH: f64 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pull {
    Grid,
    Clip,
    Marker,
    Playhead,
}

impl Pull {
    pub(super) fn name(self) -> &'static str {
        match self {
            Pull::Grid => tr("Grid"),
            Pull::Clip => tr("Clip edge"),
            Pull::Marker => tr("Marker"),
            Pull::Playhead => tr("Playhead"),
        }
    }
}

pub(super) struct Magnet<'a> {
    settings: &'a EditSettings,
    project: &'a Project,
    points: Vec<(u64, Pull)>,
    reach: u64,
    /// Where the last snap went and what pulled it.
    pub(super) hit: Cell<Option<(u64, Pull)>>,
}

impl<'a> Magnet<'a> {
    /// `zoom` in samples per pixel; the clips in `moving` do not pull.
    pub(super) fn new(
        settings: &'a EditSettings,
        project: &'a Project,
        zoom: f64,
        playhead: u64,
        moving: &[ClipId],
    ) -> Self {
        let targets = settings.targets;
        let mut points = Vec::new();
        if settings.snapping() {
            if targets.clips {
                for clip in project
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .filter(|c| !moving.contains(&c.id))
                {
                    points.push((clip.start, Pull::Clip));
                    points.push((clip.end(), Pull::Clip));
                }
            }
            if targets.markers {
                points.extend(project.markers.iter().map(|m| (m.position, Pull::Marker)));
            }
            if targets.playhead {
                points.push((playhead, Pull::Playhead));
            }
        }
        Self {
            settings,
            project,
            points,
            reach: (REACH * zoom).max(1.0) as u64,
            hit: Cell::new(None),
        }
    }

    fn grid(&self) -> bool {
        self.settings.targets.grid || self.settings.mode == EditMode::Grid
    }

    /// The nearest point within reach, else the grid line, else `at` itself.
    fn pull(&self, at: u64) -> Option<(u64, Pull)> {
        if !self.settings.snapping() {
            return None;
        }
        self.points
            .iter()
            .filter(|(p, _)| p.abs_diff(at) <= self.reach)
            .min_by_key(|(p, _)| p.abs_diff(at))
            .copied()
            .or_else(|| {
                self.grid()
                    .then(|| (self.settings.snap(self.project, at), Pull::Grid))
            })
    }

    pub(super) fn snap(&self, at: u64) -> u64 {
        let pulled = self.pull(at);
        self.hit.set(pulled);
        pulled.map_or(at, |p| p.0)
    }

    /// The new start of a span of `length` from `start`: whichever edge is pulled nearer.
    pub(super) fn snap_span(&self, start: u64, length: u64) -> u64 {
        let head = self.pull(start);
        let tail = self.pull(start + length);
        let gap = |p: Option<(u64, Pull)>, x: u64| p.map_or(u64::MAX, |p| p.0.abs_diff(x));
        // A real edge beats the grid at the other end.
        let tail_wins = match (head, tail) {
            (Some(h), Some(t)) if h.1 == Pull::Grid && t.1 != Pull::Grid => true,
            (Some(h), Some(t)) if h.1 != Pull::Grid && t.1 == Pull::Grid => false,
            _ => gap(tail, start + length) < gap(head, start),
        };
        if tail_wins && let Some((p, pull)) = tail {
            self.hit.set(Some((p, pull)));
            return p.saturating_sub(length);
        }
        self.hit.set(head);
        head.map_or(start, |h| h.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_pull_before_the_grid() {
        let mut project = Project::new("t", 48_000);
        let t = project.add_track("a");
        let s = project.add_source(audiostrada_core::Source {
            path: "a.wav".into(),
            channels: 1,
            sample_rate: 48_000,
            frames: 10_000,
        });
        let clip = project.add_clip(t, s, 30_000).unwrap();
        let settings = EditSettings {
            snap: true,
            ..Default::default()
        };
        // 100 samples a pixel: the clip's start pulls from 900 samples away.
        let magnet = Magnet::new(&settings, &project, 100.0, 0, &[]);
        assert_eq!(magnet.snap(30_500), 30_000);
        assert_eq!(magnet.hit.get().unwrap().1, Pull::Clip);
        // Far from any edge, the grid (a beat is 24 000 samples at 120 BPM).
        assert_eq!(magnet.snap(50_000), 48_000);
        assert_eq!(magnet.hit.get().unwrap().1, Pull::Grid);
        // A span whose end comes near the clip's start lands against it.
        assert_eq!(magnet.snap_span(25_600, 5_000), 25_000);
        // The moving clip does not pull itself; snap off, nothing moves.
        let alone = Magnet::new(&settings, &project, 100.0, 0, &[clip]);
        assert_eq!(alone.snap(30_500), 24_000);
        let free = EditSettings::default();
        assert_eq!(
            Magnet::new(&free, &project, 100.0, 0, &[]).snap(30_500),
            30_500
        );
    }
}
