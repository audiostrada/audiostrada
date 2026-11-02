//! The reference track: a finished song to compare the mix with, at the same loudness.

use std::path::{Path, PathBuf};

use audiostrada_core::ReferenceTrack;
use audiostrada_engine::loudness::LoudnessAnalyzer;

use super::Document;
use crate::i18n::tr;

impl Document {
    /// Sets the song to compare with (`None` removes it).
    pub fn set_reference(&mut self, file: Option<&Path>) {
        let path = file.map(|f| {
            self.paths
                .as_ref()
                .map_or_else(|| f.to_path_buf(), |p| p.relativize(f))
        });
        let label = if path.is_some() {
            tr("Reference track")
        } else {
            tr("Remove reference track")
        };
        self.history.edit(label, |p| {
            p.reference = path.map(|path| ReferenceTrack {
                path,
                matched: p.reference.as_ref().is_none_or(|r| r.matched),
            });
        });
        if file.is_none() {
            self.player.set_reference_listening(false);
        }
    }

    pub fn set_reference_matched(&mut self, matched: bool) {
        self.history.edit(tr("Reference loudness"), |p| {
            if let Some(r) = &mut p.reference {
                r.matched = matched;
            }
        });
    }

    /// Integrated loudness of the reference (LUFS), once its audio is loaded.
    pub fn reference_loudness(&self) -> Option<f64> {
        self.reference_lufs.as_ref().and_then(|(_, lufs)| *lufs)
    }

    /// Loudness of the mix to match: integrated since the meters' reset, else short-term.
    pub fn mix_loudness(&self) -> Option<f64> {
        let readout = self.player.loudness();
        readout
            .integrated()
            .or_else(|| readout.short_term())
            .filter(|v| v.is_finite())
    }

    /// Gain the reference is heard with, in dB.
    pub fn reference_gain(&self) -> f32 {
        let matched = self.project().reference.as_ref().is_some_and(|r| r.matched);
        match (matched, self.mix_loudness(), self.reference_loudness()) {
            (true, Some(mix), Some(reference)) => ((mix - reference) as f32).clamp(-30.0, 30.0),
            _ => 0.0,
        }
    }

    /// Measures the reference once loaded and keeps its gain on the mix's loudness.
    pub(super) fn update_reference(&mut self) {
        let Some(reference) = self.project().reference.clone() else {
            self.reference_lufs = None;
            return;
        };
        let path: PathBuf = self.resolve(&reference.path);
        if self.reference_lufs.as_ref().is_none_or(|(p, _)| *p != path)
            && let Some(audio) = self.media.original(&path)
        {
            let mut analyzer = LoudnessAnalyzer::new(audio.sample_rate());
            let right = audio.channel(audio.channels().min(2) - 1);
            analyzer.push(audio.channel(0), right);
            self.reference_lufs = Some((path, analyzer.integrated()));
        }
        let gain = self.reference_gain();
        self.player.set_reference_gain(gain);
    }
}
