//! Groove: the hits of some tracks (a bass, guitars, keys...) moved onto those of a reference
//! (the drums), by stretching the audio between them. The tracks grooved together are cut at
//! the same places so their phase is kept; each gets the result as a new playlist.

use std::path::PathBuf;
use std::sync::Arc;

use audiostrada_core::edit;
use audiostrada_core::{Project, Source, TrackId, TrackKind};
use audiostrada_dsp::analysis::{Onsets, detect_onsets};
use audiostrada_dsp::biquad::{Biquad, Coefficients, FilterKind};
use audiostrada_media::AudioData;

use super::Document;
use crate::i18n::tr;
use crate::trf;

/// Which hits of the reference count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Band {
    #[default]
    All,
    /// The kick and other low hits.
    Low,
    /// The snare, hi-hats and cymbals.
    High,
}

impl Band {
    pub const ALL: [Band; 3] = [Band::All, Band::Low, Band::High];

    pub fn name(self) -> &'static str {
        match self {
            Band::All => "All hits",
            Band::Low => "Kick (lows)",
            Band::High => "Snare and hi-hats (highs)",
        }
    }
}

/// How the hits are matched and moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrooveSettings {
    pub band: Band,
    /// How readily hits are found, 0 to 1 (both in the reference and the tracks).
    pub sensitivity: f32,
    /// How far a reference hit may be to pull a hit of the tracks.
    pub tolerance_ms: f32,
    /// How much of the way each hit moves, 0 to 1.
    pub strength: f64,
}

impl Default for GrooveSettings {
    fn default() -> Self {
        Self {
            band: Band::All,
            sensitivity: 0.5,
            tolerance_ms: 35.0,
            strength: 1.0,
        }
    }
}

/// A clip's audio placed on the timeline.
struct Layer {
    data: Arc<AudioData>,
    /// Source frames per timeline sample.
    ratio: f64,
    start: u64,
    offset: u64,
    length: u64,
    gain: f32,
    fade_in: (u64, audiostrada_core::FadeShape),
    fade_out: (u64, audiostrada_core::FadeShape),
}

/// What tracks play from `start` (clip gains and fades, no effects), mixed in `channels`.
fn mix(layers: &[Layer], start: u64, length: usize, channels: usize) -> Vec<Vec<f32>> {
    let mut out = vec![vec![0.0f32; length]; channels];
    for layer in layers {
        let from = layer.start.max(start);
        let to = (layer.start + layer.length).min(start + length as u64);
        for t in from..to {
            let into = t - layer.start;
            let mut gain = layer.gain;
            if into < layer.fade_in.0 {
                gain *= layer.fade_in.1.gain(into as f32 / layer.fade_in.0 as f32);
            }
            let left = layer.length - into;
            if left < layer.fade_out.0 {
                gain *= layer.fade_out.1.gain(left as f32 / layer.fade_out.0 as f32);
            }
            let at = (layer.offset + into) as f64 * layer.ratio;
            let (i, frac) = (at as usize, (at.fract()) as f32);
            if i >= layer.data.frames() {
                break;
            }
            let src = layer.data.channels();
            for (ch, o) in out.iter_mut().enumerate() {
                let c = layer.data.channel(ch.min(src - 1));
                let next = c.get(i + 1).copied().unwrap_or(0.0);
                o[(t - start) as usize] += gain * (c[i] + frac * (next - c[i]));
            }
        }
    }
    out
}

/// The tracks to groove and the reference, ready to be matched at any setting.
#[derive(Debug)]
pub struct GrooveScan {
    pub start: u64,
    rate: u32,
    /// Per track grooved, its audio over the span.
    pub tracks: Vec<(TrackId, Vec<Vec<f32>>)>,
    target: Onsets,
    /// The reference hits, per [`Band`].
    reference: [Onsets; 3],
}

/// Where the hits of the tracks go, in timeline samples.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrooveMoves {
    /// `(from, to)` per hit moved, relative to the span start, with the span ends first and
    /// last.
    pub anchors: Vec<(usize, usize)>,
    /// The notes of the tracks (the first hit of each), whose attacks the stretch keeps whole.
    pub notes: Vec<usize>,
    /// Hits found on the tracks, and in the reference.
    pub hits: usize,
    pub reference_hits: usize,
}

impl GrooveMoves {
    /// Hits that move.
    pub fn moved(&self) -> usize {
        self.anchors.iter().filter(|(a, b)| a != b).count()
    }

    /// Largest move, in samples.
    pub fn largest(&self) -> usize {
        self.anchors
            .iter()
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0)
    }
}

impl GrooveScan {
    pub fn length(&self) -> usize {
        self.tracks
            .first()
            .and_then(|t| t.1.first())
            .map_or(0, Vec::len)
    }

    pub fn moves(&self, settings: &GrooveSettings) -> GrooveMoves {
        let band = Band::ALL
            .iter()
            .position(|b| *b == settings.band)
            .unwrap_or(0);
        let reference = self.reference[band].pick(settings.sensitivity);
        let hits = self.target.pick(settings.sensitivity);
        let ms = |ms: f32| (ms.max(0.0) / 1000.0 * self.rate as f32) as usize;
        GrooveMoves {
            notes: audiostrada_dsp::groove::notes(&hits, ms(30.0)),
            anchors: audiostrada_dsp::groove::anchors(
                &reference,
                &hits,
                ms(settings.tolerance_ms),
                settings.strength,
                ms(30.0),
                self.length(),
            ),
            hits: hits.len(),
            reference_hits: reference.len(),
        }
    }
}

/// A grooved track's new audio file.
#[derive(Debug)]
pub struct Grooved {
    track: TrackId,
    path: PathBuf,
    channels: u16,
    frames: u64,
}

impl Grooved {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Document {
    /// Tracks a reference choice stands for: an audio track, or the audio tracks in a folder.
    pub fn groove_sources(&self, track: TrackId) -> Vec<TrackId> {
        let project = self.project();
        let mut found = Vec::new();
        let mut look = vec![track];
        while let Some(id) = look.pop() {
            if let Some(t) = project.track(id)
                && t.kind == TrackKind::Audio
                && !found.contains(&id)
            {
                found.push(id);
            }
            look.extend(
                project
                    .tracks
                    .iter()
                    .filter(|t| t.parent == Some(id))
                    .map(|t| t.id),
            );
        }
        found
    }

    fn layers(&self, tracks: &[TrackId]) -> Result<Vec<Layer>, String> {
        let project = self.project();
        let mut layers = Vec::new();
        for track in tracks.iter().filter_map(|t| project.track(*t)) {
            for clip in track.clips.iter().filter(|c| c.notes.is_none() && !c.muted) {
                let Some(source) = project.sources.get(&clip.source) else {
                    continue;
                };
                let data = audiostrada_engine::project::clip_media(project, self.paths(), clip)
                    .and_then(|path| self.media().original(&path).cloned())
                    .ok_or_else(|| trf!("{clip} is still loading, try again", clip = clip.name))?;
                if data.channels() == 0 {
                    continue;
                }
                layers.push(Layer {
                    data,
                    ratio: f64::from(source.sample_rate) / f64::from(project.sample_rate.max(1)),
                    start: clip.start,
                    offset: clip.offset,
                    length: clip.length,
                    gain: audiostrada_dsp::db_to_gain(clip.gain_db),
                    fade_in: (clip.fade_in, clip.fade_shape),
                    fade_out: (
                        clip.fade_out,
                        clip.fade_out_shape.unwrap_or(clip.fade_shape),
                    ),
                });
            }
        }
        Ok(layers)
    }

    /// Prepares the analysis of `tracks` against `reference` over `start..end`, to run on
    /// another thread.
    pub fn scan_groove(
        &self,
        tracks: &[TrackId],
        reference: &[TrackId],
        start: u64,
        end: u64,
    ) -> Result<impl FnOnce() -> GrooveScan + Send + 'static, String> {
        if end <= start {
            return Err(tr("nothing to groove").into());
        }
        let rate = self.project().sample_rate;
        let mut targets = Vec::new();
        for &track in tracks {
            let layers = self.layers(&[track])?;
            let channels = layers.iter().map(|l| l.data.channels()).max().unwrap_or(1);
            targets.push((track, layers, channels.min(2)));
        }
        let reference = self.layers(reference)?;
        if reference.is_empty() {
            return Err(tr("the reference holds no audio").into());
        }
        let length = (end - start) as usize;
        Ok(move || {
            let tracks: Vec<(TrackId, Vec<Vec<f32>>)> = targets
                .iter()
                .map(|(id, layers, channels)| (*id, mix(layers, start, length, *channels)))
                .collect();
            let all: Vec<&[f32]> = tracks
                .iter()
                .flat_map(|(_, c)| c.iter().map(Vec::as_slice))
                .collect();
            let target = detect_onsets(&all, rate);
            let drums = mix(&reference, start, length, 1).remove(0);
            let filtered = |kind: FilterKind, hz: f64| {
                let q = [0.541, 1.307];
                let mut stages = q.map(|q| {
                    (
                        Coefficients::design(kind, hz, q, 0.0, f64::from(rate)),
                        Biquad::default(),
                    )
                });
                drums
                    .iter()
                    .map(|&x| {
                        let mut y = f64::from(x);
                        for (c, f) in &mut stages {
                            y = f.process(c, y);
                        }
                        y as f32
                    })
                    .collect::<Vec<f32>>()
            };
            let low = filtered(FilterKind::LowPass, 150.0);
            let high = filtered(FilterKind::HighPass, 2500.0);
            let reference = [
                detect_onsets(&[&drums], rate),
                detect_onsets(&[&low], rate),
                detect_onsets(&[&high], rate),
            ];
            GrooveScan {
                start,
                rate,
                tracks,
                target,
                reference,
            }
        })
    }

    /// Prepares the stretching of the scanned tracks by `anchors` and the writing of their new
    /// files, to run on another thread.
    /// A preview writes its files in a temporary folder.
    pub fn groove_job(
        &self,
        scan: Arc<GrooveScan>,
        moves: &GrooveMoves,
        preview: bool,
    ) -> Result<impl FnOnce() -> Result<Vec<Grooved>, String> + Send + 'static, String> {
        let (anchors, notes) = (moves.anchors.clone(), moves.notes.clone());
        let mut files = Vec::new();
        for (track, _) in &scan.tracks {
            let name = self
                .project()
                .track(*track)
                .map_or_else(|| "groove".to_owned(), |t| format!("{} (groove)", t.name));
            files.push(if preview {
                preview_file(&name)?
            } else {
                self.render_file(&name)?
            });
        }
        Ok(move || {
            let all: Vec<&[f32]> = scan
                .tracks
                .iter()
                .flat_map(|(_, c)| c.iter().map(Vec::as_slice))
                .collect();
            let mut warped =
                audiostrada_dsp::groove::warp(&all, &anchors, &notes, scan.rate).into_iter();
            let mut done = Vec::new();
            for ((track, channels), path) in scan.tracks.iter().zip(files) {
                let audio: Vec<Vec<f32>> = warped.by_ref().take(channels.len()).collect();
                let frames = audio.first().map_or(0, Vec::len) as u64;
                let data = AudioData::new(audio, scan.rate);
                audiostrada_media::write_wav(&path, &data).map_err(|e| e.to_string())?;
                done.push(Grooved {
                    track: *track,
                    path,
                    channels: channels.len() as u16,
                    frames,
                });
            }
            Ok(done)
        })
    }

    /// Gives each grooved track its new audio as a new playlist, the previous one kept as an
    /// alternate.
    /// Returns the revision of the project with the result, and the new files.
    pub fn apply_groove(
        &mut self,
        grooved: Vec<Grooved>,
        start: u64,
        preview: bool,
    ) -> (u64, Vec<PathBuf>) {
        let files: Vec<PathBuf> = grooved.iter().map(|g| g.path.clone()).collect();
        let rate = self.project().sample_rate;
        let sources: Vec<(TrackId, Source, u64)> = grooved
            .into_iter()
            .map(|g| {
                let path = self
                    .paths()
                    .map_or_else(|| g.path.clone(), |p| p.relativize(&g.path));
                let source = Source {
                    path,
                    channels: g.channels,
                    sample_rate: rate,
                    frames: g.frames,
                };
                (g.track, source, g.frames)
            })
            .collect();
        let count = sources.len();
        let label = if preview {
            tr("Groove preview")
        } else {
            tr("Groove")
        };
        self.history_mut().edit(label, |p| {
            for (track, source, frames) in sources {
                groove_playlist(p, track, source, start, frames);
            }
        });
        if !preview {
            self.notify(trf!(
                "{count} track(s) grooved, as a new playlist",
                count = count
            ));
        }
        (self.history().revision(), files)
    }

    /// Takes a preview back, unless the project was edited since, and deletes its files.
    pub fn withdraw_groove(&mut self, revision: u64, files: &[PathBuf]) {
        if self.history().revision() == revision {
            self.history_mut().withdraw();
        } else {
            self.notify(
                tr(
                    "The Groove preview stays in the project: it was edited meanwhile (undo it if \
                 needed)",
                )
                .to_owned(),
            );
            return;
        }
        for file in files {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// A new file for a preview, in the temporary folder.
fn preview_file(name: &str) -> Result<PathBuf, String> {
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("audiostrada-groove-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(dir.join(format!("{safe} {n}.wav")))
}

/// Starts a playlist on `track` holding its current clips, with `source` in place of what
/// they play from `start` for `frames`.
fn groove_playlist(p: &mut Project, track: TrackId, source: Source, start: u64, frames: u64) {
    let Some(name) = p.track(track).map(|t| t.name.clone()) else {
        return;
    };
    let clips = p.track(track).map(|t| t.clips.clone()).unwrap_or_default();
    edit::new_playlist(p, track);
    let ids: Vec<_> = clips.iter().map(|_| p.new_clip_id()).collect();
    if let Some(t) = p.track_mut(track) {
        t.playlist = trf!("{name} groove", name = name);
        t.clips = clips
            .into_iter()
            .zip(ids)
            .map(|(mut c, id)| {
                c.id = id;
                c
            })
            .collect();
    }
    edit::delete_range(p, &[track], start, start + frames, false);
    let source = p.add_source(source);
    if let Some(id) = p.add_clip(track, source, start)
        && let Some(c) = edit::clip_mut(p, id)
    {
        c.length = frames;
        c.name = trf!("{name} groove", name = name);
    }
    if let Some(t) = p.track_mut(track) {
        t.clips.sort_by_key(|c| (c.start, c.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;
    use audiostrada_core::Clip;

    #[test]
    fn a_late_and_early_bass_lands_on_the_kick() {
        let dir = std::env::temp_dir().join(format!("audiostrada-groove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rate = 48_000usize;
        let beats: Vec<usize> = (1..8).map(|b| b * rate / 2).collect();
        let length = 4 * rate + rate / 4;
        let mut noise = 1u32;
        let mut drums = vec![0.0f32; length];
        for &b in &beats {
            for i in 0..2400 {
                noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let white = (noise >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                drums[b + i] += white * (-(i as f32) / 300.0).exp();
            }
        }
        // The bassist plays up to 30 ms around the beat.
        let offsets = [25i64, -20, 30, -15, 20, -25, 10];
        let mut bass = vec![0.0f32; length];
        for (&b, &o) in beats.iter().zip(&offsets) {
            let at = (b as i64 + o * 48) as usize;
            for i in 0..rate / 3 {
                let t = i as f32 / rate as f32;
                let phase = t * 55.0 * std::f32::consts::TAU;
                bass[at + i] += 0.5 * (-t * 6.0).exp() * (phase.sin() + 0.5 * (2.0 * phase).sin());
            }
        }
        let files = [dir.join("drums.wav"), dir.join("bass.wav")];
        for (file, audio) in files.iter().zip([drums, bass]) {
            audiostrada_media::write_wav(file, &AudioData::new(vec![audio], rate as u32)).unwrap();
        }
        let mut doc = Document::new(AppDirs {
            recordings: Some(dir.join("recordings")),
            ..AppDirs::default()
        });
        doc.import(&files);
        assert!(doc.wait_loaded(std::time::Duration::from_secs(10)));
        let (drums, bass) = (doc.project().tracks[0].id, doc.project().tracks[1].id);
        assert_eq!(doc.groove_sources(drums), [drums]);

        let scan = doc
            .scan_groove(&[bass], &[drums], 0, length as u64)
            .unwrap()();
        let moves = scan.moves(&GrooveSettings::default());
        assert_eq!(moves.moved(), beats.len(), "{moves:?}");
        let job = doc.groove_job(Arc::new(scan), &moves, false).unwrap();
        doc.apply_groove(job().unwrap(), 0, false);
        assert!(doc.wait_loaded(std::time::Duration::from_secs(10)));
        let project = doc.project();
        let track = project.track(bass).unwrap();
        assert_eq!(track.clips.len(), 1);
        let path = doc.resolve(&project.sources[&track.clips[0].source].path);
        let grooved = audiostrada_media::load(&path).unwrap();
        assert_eq!(grooved.frames(), length);
        let found = audiostrada_dsp::analysis::onsets(&[grooved.channel(0)], rate as u32, 0.5);
        for b in &beats {
            let nearest = found.iter().map(|f| f.abs_diff(*b)).min().unwrap();
            assert!(nearest < 96, "a note on the beat at {b}: {found:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_preview_comes_and_goes_without_a_trace() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut().edit("Setup", |p| {
            track = Some(p.add_track("Bass"));
        });
        let before = doc.project().clone();
        let file = preview_file("Bass (groove)").unwrap();
        std::fs::write(&file, b"").unwrap();
        let grooved = |path: &std::path::Path| Grooved {
            track: track.unwrap(),
            path: path.to_path_buf(),
            channels: 1,
            frames: 4800,
        };
        let (revision, files) = doc.apply_groove(vec![grooved(&file)], 0, true);
        assert_ne!(doc.project(), &before);
        assert_eq!(doc.history().undo_label(), Some("Groove preview"));
        doc.withdraw_groove(revision, &files);
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history().redo_label(), None);
        assert!(!file.exists(), "its file goes too");
        // Edited since: the preview stays, the user decides.
        let (revision, files) = doc.apply_groove(vec![grooved(&file)], 0, true);
        doc.history_mut().edit("Rename", |p| {
            if let Some(t) = p.track_mut(track.unwrap()) {
                t.name = "Bass 2".into();
            }
        });
        doc.withdraw_groove(revision, &files);
        assert_eq!(doc.project().tracks[0].name, "Bass 2");
        assert_eq!(doc.project().tracks[0].alternates.len(), 1);
    }

    #[test]
    fn clips_are_mixed_where_they_sit_with_their_gain() {
        let data = Arc::new(AudioData::new(vec![vec![1.0; 100]], 48_000));
        let layer = Layer {
            data,
            ratio: 1.0,
            start: 10,
            offset: 20,
            length: 50,
            gain: 0.5,
            fade_in: (10, audiostrada_core::FadeShape::Linear),
            fade_out: (0, audiostrada_core::FadeShape::Linear),
        };
        let out = mix(&[layer], 0, 80, 2);
        assert_eq!(out.len(), 2, "a mono clip on both sides");
        assert_eq!(out[0][9], 0.0);
        assert!((out[0][15] - 0.25).abs() < 1e-6, "half way in its fade-in");
        assert_eq!(out[1][40], 0.5);
        assert_eq!(out[0][60], 0.0, "after its end");
    }

    #[test]
    fn the_result_becomes_a_new_playlist_keeping_the_rest() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut().edit("Setup", |p| {
            let s = p.add_source(Source {
                path: PathBuf::from("/bass.wav"),
                channels: 1,
                sample_rate: 48_000,
                frames: 100_000,
            });
            let t = p.add_track("Bass");
            p.add_clip(t, s, 0);
            track = Some(t);
        });
        let track = track.unwrap();
        doc.apply_groove(
            vec![Grooved {
                track,
                path: PathBuf::from("/bass (groove) 1.wav"),
                channels: 1,
                frames: 48_000,
            }],
            24_000,
            false,
        );
        let p = doc.project();
        let t = p.track(track).unwrap();
        assert_eq!(t.alternates.len(), 1, "the original kept");
        assert_eq!(t.playlist, "Bass groove");
        let clips: Vec<&Clip> = t.clips.iter().collect();
        assert_eq!(clips.len(), 3, "before, grooved, after: {clips:?}");
        assert_eq!((clips[1].start, clips[1].length), (24_000, 48_000));
        assert_eq!(
            p.sources[&clips[1].source].path,
            PathBuf::from("/bass (groove) 1.wav")
        );
        assert_eq!(clips[2].start, 72_000);
        let ids: std::collections::HashSet<_> = t
            .clips
            .iter()
            .chain(t.alternates[0].clips.iter())
            .map(|c| c.id)
            .collect();
        assert_eq!(ids.len(), 4, "every clip has its own id");
        assert!(doc.history().is_dirty());
    }
}
