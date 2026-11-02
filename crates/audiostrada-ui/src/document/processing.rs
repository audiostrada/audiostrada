//! Clip processing: Strip Silence, Heal and Beat Detective (consolidating and offline effects
//! render through the export jobs).

use audiostrada_core::edit::{self, StripMode};
use audiostrada_core::{ClipId, TrackId};
use std::sync::Arc;

use audiostrada_dsp::analysis::{Onsets, SilenceSettings, detect_onsets, non_silent};
use audiostrada_media::AudioData;

use super::Document;
use crate::i18n::tr;

/// Strip Silence settings, in dB and milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripSettings {
    pub threshold_db: f32,
    pub min_silence_ms: f32,
    pub pad_before_ms: f32,
    pub pad_after_ms: f32,
}

impl Default for StripSettings {
    fn default() -> Self {
        Self {
            threshold_db: -48.0,
            min_silence_ms: 100.0,
            pad_before_ms: 5.0,
            pad_after_ms: 30.0,
        }
    }
}

/// Timeline ranges holding sound, per clip.
pub type Found = Vec<(ClipId, Vec<(u64, u64)>)>;

/// The possible hits of the clips Beat Detective looks at, ready to be picked at any
/// sensitivity.
#[derive(Debug, Default)]
pub struct HitScan {
    /// Per clip: where its analysed audio starts on the timeline, the source to timeline rate
    /// ratio and its attacks.
    parts: Vec<(u64, f64, Onsets)>,
    near: u64,
}

impl HitScan {
    /// Hits at `sensitivity`, in timeline samples, sorted.
    pub fn hits(&self, sensitivity: f32) -> Vec<u64> {
        let mut hits: Vec<u64> = self
            .parts
            .iter()
            .flat_map(|(from, ratio, onsets)| {
                onsets
                    .pick(sensitivity)
                    .into_iter()
                    .map(move |f| from + (f as f64 / ratio) as u64)
            })
            .collect();
        hits.sort_unstable();
        let mut merged: Vec<u64> = Vec::new();
        for h in hits {
            if merged.last().is_none_or(|&l| h - l > self.near) {
                merged.push(h);
            }
        }
        merged
    }
}

/// One clip's audio to analyse for hits.
struct HitJob {
    data: Arc<AudioData>,
    frames: std::ops::Range<usize>,
    rate: u32,
    from: u64,
    ratio: f64,
}

impl HitJob {
    fn run(&self) -> (u64, f64, Onsets) {
        let channels: Vec<&[f32]> = (0..self.data.channels())
            .map(|ch| &self.data.channel(ch)[self.frames.clone()])
            .collect();
        (self.from, self.ratio, detect_onsets(&channels, self.rate))
    }
}

impl Document {
    /// Where each audio clip holds sound, from its decoded source (clips whose audio is not
    /// loaded yet are left out).
    pub fn find_sound(&self, clips: &[ClipId], settings: &StripSettings) -> Found {
        self.scan_sound(clips, settings)()
    }

    /// The analysis behind [`Self::find_sound`], as a job for another thread.
    pub fn scan_sound(
        &self,
        clips: &[ClipId],
        settings: &StripSettings,
    ) -> impl FnOnce() -> Found + Send + 'static {
        let project = self.project();
        let mut jobs = Vec::new();
        for &id in clips {
            let Some(clip) = edit::clip(project, id).filter(|c| c.notes.is_none()) else {
                continue;
            };
            let Some(source) = project.sources.get(&clip.source) else {
                continue;
            };
            let Some(data) = audiostrada_engine::project::clip_media(project, self.paths(), clip)
                .and_then(|path| self.media().original(&path).cloned())
            else {
                continue;
            };
            let ratio = f64::from(source.sample_rate) / f64::from(project.sample_rate.max(1));
            let from = ((clip.offset as f64 * ratio) as usize).min(data.frames());
            let to = (((clip.offset + clip.length) as f64 * ratio) as usize).min(data.frames());
            let frames = |ms: f32| (ms.max(0.0) / 1000.0 * source.sample_rate as f32) as usize;
            let silence = SilenceSettings {
                // A quieter clip plays its audio quieter.
                threshold_db: settings.threshold_db - clip.gain_db,
                min_silence: frames(settings.min_silence_ms),
                pad_before: frames(settings.pad_before_ms),
                pad_after: frames(settings.pad_after_ms),
            };
            jobs.push((id, data, from..to, ratio, silence, clip.start, clip.end()));
        }
        move || {
            jobs.into_iter()
                .map(|(id, data, frames, ratio, silence, start, end)| {
                    let channels: Vec<&[f32]> = (0..data.channels())
                        .map(|ch| &data.channel(ch)[frames.clone()])
                        .collect();
                    let to_timeline = |f: usize| start + (f as f64 / ratio) as u64;
                    let ranges = non_silent(&channels, &silence)
                        .into_iter()
                        .map(|(a, b)| (to_timeline(a), to_timeline(b).min(end)))
                        .collect();
                    (id, ranges)
                })
                .collect()
        }
    }

    /// Strip Silence with what [`Self::find_sound`] found; returns the clips left.
    pub fn strip_silence(&mut self, found: &Found, mode: StripMode) -> Vec<ClipId> {
        let label = match mode {
            StripMode::Strip => tr("Strip silence"),
            StripMode::Extract => tr("Extract silence"),
            StripMode::Separate => tr("Separate at silences"),
        };
        let mut left = Vec::new();
        self.history.edit(label, |p| {
            for (clip, ranges) in found {
                left.extend(edit::strip_clip(p, *clip, ranges, mode));
            }
        });
        left
    }

    /// Heal on `tracks` within `start..end`; returns how many joins were made.
    pub fn heal(&mut self, tracks: &[TrackId], start: u64, end: u64) -> usize {
        let mut healed = 0;
        self.history
            .edit(tr("Heal"), |p| healed = edit::heal(p, tracks, start, end));
        healed
    }

    /// Hits of `tracks` within `start..end` (timeline samples), gathered across the tracks so
    /// every track is cut at the same places (microphones of one kit stay in phase).
    pub fn find_hits(
        &self,
        tracks: &[TrackId],
        start: u64,
        end: u64,
        sensitivity: f32,
    ) -> Vec<u64> {
        self.scan_hits(tracks, start, end)().hits(sensitivity)
    }

    /// The analysis behind [`Self::find_hits`], as a job for another thread: the audio of
    /// long takes takes a while to analyse.
    pub fn scan_hits(
        &self,
        tracks: &[TrackId],
        start: u64,
        end: u64,
    ) -> impl FnOnce() -> HitScan + Send + 'static {
        let project = self.project();
        let mut jobs: Vec<HitJob> = Vec::new();
        for track in tracks.iter().filter_map(|t| project.track(*t)) {
            for clip in track.clips.iter().filter(|c| c.notes.is_none()) {
                let (from, to) = (clip.start.max(start), clip.end().min(end));
                if from >= to {
                    continue;
                }
                let Some(source) = project.sources.get(&clip.source) else {
                    continue;
                };
                let Some(data) =
                    audiostrada_engine::project::clip_media(project, self.paths(), clip)
                        .and_then(|path| self.media().original(&path).cloned())
                else {
                    continue;
                };
                let ratio = f64::from(source.sample_rate) / f64::from(project.sample_rate.max(1));
                let first = (((clip.offset + from - clip.start) as f64) * ratio) as usize;
                let last = ((((clip.offset + to - clip.start) as f64) * ratio) as usize)
                    .min(data.frames());
                if first < last {
                    jobs.push(HitJob {
                        data,
                        frames: first..last,
                        rate: source.sample_rate,
                        from,
                        ratio,
                    });
                }
            }
        }
        // Hits of different microphones a few milliseconds apart are the same hit.
        let near = u64::from(project.sample_rate) / 50;
        move || {
            let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
            let chunk = jobs.len().div_ceil(threads).max(1);
            let parts = std::thread::scope(|scope| {
                let workers: Vec<_> = jobs
                    .chunks(chunk)
                    .map(|jobs| scope.spawn(move || jobs.iter().map(HitJob::run).collect()))
                    .collect();
                workers
                    .into_iter()
                    .flat_map(|w| w.join().unwrap_or_else(|_| Vec::new()))
                    .collect()
            });
            HitScan { parts, near }
        }
    }

    /// Beat Detective: cuts `tracks` at `hits` and, with a grid (`beats` per step, `None` for
    /// bars), moves each slice toward its nearest line by `strength`, joining the slices with
    /// short fades. One undo step.
    pub fn beat_detective(
        &mut self,
        tracks: &[TrackId],
        start: u64,
        end: u64,
        hits: &[u64],
        conform: Option<(Option<f64>, f64)>,
    ) {
        let label = if conform.is_some() {
            tr("Conform to the grid")
        } else {
            tr("Separate at hits")
        };
        let rate = self.project().sample_rate;
        let tempo = self.project().tempo.clone();
        let fade = u64::from(rate) / 500;
        self.history.edit(label, |p| {
            edit::split_at(p, tracks, hits);
            if let Some((beats, strength)) = conform {
                let target = |at: u64| edit::snap(at, beats, &tempo, rate);
                edit::conform(p, tracks, start, end, target, strength, fade);
            }
        });
    }
}
