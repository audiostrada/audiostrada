//! Clip processing: Strip Silence and Heal (consolidating and offline effects render through
//! the export jobs).

use audiostrada_core::edit::{self, StripMode};
use audiostrada_core::{ClipId, TrackId};

use audiostrada_dsp::analysis::{SilenceSettings, non_silent};

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
}
