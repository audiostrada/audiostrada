//! Exporting mixes and stems, and freezing tracks: renders run a slice at a time from
//! `Document::update`, then a background thread normalizes, resamples and writes each file.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

use audiostrada_core::{ClipId, Frozen, Project, Source, TrackId, TrackKind, routing};
use audiostrada_engine::export::{OfflineRender, Rendered};
use audiostrada_media::{FileFormat, encode, resample};
use serde::{Deserialize, Serialize};

use super::Document;
use crate::i18n::tr;
use crate::trf;

/// Time spent rendering per interface frame.
const SLICE_FRAMES: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Normalize {
    #[default]
    Off,
    /// Peak level in dBFS.
    Peak(f32),
    /// Integrated loudness in LUFS.
    Loudness(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportRange {
    #[default]
    Project,
    Loop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportSettings {
    pub format: FileFormat,
    /// `None` keeps the stream rate.
    pub sample_rate: Option<u32>,
    pub dither: bool,
    pub normalize: Normalize,
    pub range: ExportRange,
    /// Seconds rendered after the end, for reverb and delay tails.
    pub tail: f32,
    pub mix: bool,
    /// One file per audio track (soloed, with the buses it feeds).
    pub stems: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: FileFormat::Wav24,
            sample_rate: None,
            dither: true,
            normalize: Normalize::Off,
            range: ExportRange::Project,
            tail: 2.0,
            mix: true,
            stems: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobKind {
    Mix,
    Freeze(TrackId),
    /// Rendered onto a new track below this one, which is then muted.
    Bounce(TrackId),
    /// What a track plays in a range, replacing its clips there with one.
    Consolidate {
        track: TrackId,
        start: u64,
        end: u64,
    },
    /// Two tracks rendered side by side, replaced by one stereo track.
    Stereo {
        left: TrackId,
        right: TrackId,
        start: u64,
    },
    /// A clip through an effect (or reversed), replacing its audio.
    Process {
        clip: ClipId,
        reverse: bool,
        tail: bool,
    },
}

/// An offline process for clips (AudioSuite).
#[derive(Clone, Debug, PartialEq)]
pub enum Process {
    /// A built-in effect with its settings; `tail` keeps what rings past the clip.
    Effect {
        id: String,
        params: std::collections::BTreeMap<String, f32>,
        tail: bool,
    },
    Reverse,
    /// To this peak level, in dBFS.
    Normalize(f32),
}

impl Process {
    pub fn name(&self) -> String {
        match self {
            Process::Effect { id, .. } => {
                audiostrada_dsp::info(id).map_or_else(|| id.clone(), |i| i.name.to_owned())
            }
            Process::Reverse => "Reverse".into(),
            Process::Normalize(_) => "Normalize".into(),
        }
    }
}

/// A copy of `project` playing only `clips` of `track`, through `inserts` only, at unity.
fn clips_only(
    project: &Project,
    track: TrackId,
    keep: impl Fn(&audiostrada_core::Clip) -> bool,
    inserts: Vec<audiostrada_core::Insert>,
) -> Option<Project> {
    let mut copy = isolate(project, track)?;
    copy.tracks.retain(|t| t.id == track);
    let t = std::sync::Arc::make_mut(copy.tracks.first_mut()?);
    t.clips.retain(keep);
    t.inserts = inserts;
    t.automation.clear();
    t.frozen = None;
    Some(copy)
}

/// What `left` and `right` play (clips only, at unity), hard left and hard right.
fn stereo_pair(project: &Project, left: TrackId, right: TrackId) -> Project {
    let mut copy = project.clone();
    copy.tracks.retain(|t| t.id == left || t.id == right);
    for t in &mut copy.tracks {
        let t = std::sync::Arc::make_mut(t);
        // The pan law adds 3 dB at the sides.
        t.gain_db = -3.0103;
        t.pan = if t.id == left { -1.0 } else { 1.0 };
        t.mute = false;
        t.solo = false;
        t.sends.clear();
        t.parent = None;
        t.vca = None;
        t.master_send = true;
        t.inserts.clear();
        t.automation.clear();
        t.frozen = None;
        t.fx_off = false;
    }
    copy.master.gain_db = 0.0;
    copy.master.inserts.clear();
    copy.master.automation.clear();
    copy
}

/// The name of a stereo track made of `left` and `right`: what their names share once the
/// side is taken away, else both names.
fn stereo_name(project: &Project, left: TrackId, right: TrackId) -> String {
    let name = |id| project.track(id).map_or("", |t| t.name.as_str());
    let (l, r) = (name(left), name(right));
    match (name_side(l), name_side(r)) {
        (Some((_, a)), Some((_, b))) if !a.is_empty() && a.eq_ignore_ascii_case(&b) => a,
        _ => format!("{l} / {r}"),
    }
}

/// The side a track name tells (`Overhead L`, `OH_Right`, `Room (G)`...), and the rest of
/// the name: true for left.
pub(super) fn name_side(name: &str) -> Option<(bool, String)> {
    let trimmed = name.trim().trim_end_matches(')');
    for (words, left) in [
        (["left", "gauche", "l", "g"], true),
        (["right", "droite", "r", "d"], false),
    ] {
        for word in words {
            let Some(at) = trimmed.len().checked_sub(word.len()) else {
                continue;
            };
            if !trimmed.is_char_boundary(at) || !trimmed[at..].eq_ignore_ascii_case(word) {
                continue;
            }
            let base = &trimmed[..at];
            if base.is_empty() || base.ends_with([' ', '_', '-', '.', '(']) {
                let base = base.trim_end_matches([' ', '_', '-', '.', '(']);
                return Some((left, base.to_owned()));
            }
        }
    }
    None
}

#[derive(Debug)]
struct Job {
    settings: ExportSettings,
    project: Project,
    output: PathBuf,
    kind: JobKind,
    start: u64,
    end: u64,
}

#[derive(Debug)]
struct Finished {
    kind: JobKind,
    path: PathBuf,
    frames: u64,
    rate: u32,
}

/// An export waiting in the render queue, with the project as it was when queued.
#[derive(Debug)]
pub(super) struct QueuedExport {
    label: String,
    jobs: Vec<Job>,
}

#[derive(Debug)]
pub(super) struct Exporting {
    pending: VecDeque<Job>,
    current: Option<Rendering>,
    writing: Vec<JoinHandle<Result<Finished, String>>>,
    total: usize,
    done: usize,
    written: usize,
    errors: Vec<String>,
    freeze: bool,
}

/// A render running on its own thread, at full speed, then written to its file.
#[derive(Debug)]
struct Rendering {
    /// The player holding the render's plugins, updated here meanwhile.
    player: audiostrada_engine::project::ProjectPlayer,
    path: PathBuf,
    /// Fraction rendered, as the bits of an `f32`.
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<Result<Finished, String>>,
}

impl Rendering {
    fn start(render: OfflineRender, job: Job) -> Self {
        let (player, mut render) = render.split();
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared, stop) = (Arc::clone(&progress), Arc::clone(&cancel));
        let path = job.output.clone();
        let handle = std::thread::spawn(move || {
            while !render.render(SLICE_FRAMES) {
                if stop.load(Ordering::Relaxed) {
                    return Err(tr("Export cancelled").into());
                }
                shared.store(render.progress().to_bits(), Ordering::Relaxed);
            }
            write(render.finish(), &job.settings, &job.output, job.kind)
        });
        Self {
            player,
            path,
            progress,
            cancel,
            handle,
        }
    }

    fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

fn file_name(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    clean.trim().to_owned()
}

/// Normalizes, resamples and writes a render (on a background thread).
fn write(
    rendered: Rendered,
    settings: &ExportSettings,
    path: &Path,
    kind: JobKind,
) -> Result<Finished, String> {
    let Rendered {
        mut audio,
        peak,
        loudness,
    } = rendered;
    let gain = match settings.normalize {
        Normalize::Off => 1.0,
        Normalize::Peak(db) if peak > 0.0 => audiostrada_dsp::db_to_gain(db) / peak,
        Normalize::Loudness(target) => {
            loudness.map_or(1.0, |l| audiostrada_dsp::db_to_gain(target - l as f32))
        }
        Normalize::Peak(_) => 1.0,
    };
    if (gain - 1.0).abs() > 1e-6 {
        let channels = (0..audio.channels())
            .map(|ch| audio.channel(ch).iter().map(|s| s * gain).collect())
            .collect();
        audio = audiostrada_media::AudioData::new(channels, audio.sample_rate());
    }
    if let Some(rate) = settings.sample_rate
        && rate != audio.sample_rate()
    {
        audio = resample(&audio, rate).map_err(|e| e.to_string())?;
    }
    encode(
        path,
        &audio,
        settings.format,
        settings.dither && settings.format.bits().is_some(),
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Finished {
        kind,
        path: path.to_path_buf(),
        frames: audio.frames() as u64,
        rate: audio.sample_rate(),
    })
}

/// A copy of the project where only `track` (with its folder content) is heard, after its
/// inserts and before its fader.
fn isolate(project: &Project, track: TrackId) -> Option<Project> {
    let mut copy = project.clone();
    let inside = routing::subtree(project, track);
    let mut members: Vec<std::sync::Arc<audiostrada_core::Track>> = copy
        .tracks
        .iter()
        .filter(|t| t.id != track && inside.contains(&t.id))
        .cloned()
        .collect();
    for m in &mut members {
        let m = std::sync::Arc::make_mut(m);
        m.sends.retain(|s| inside.contains(&s.target));
        m.vca = None;
    }
    let mut t = copy.track(track)?.clone();
    t.gain_db = 0.0;
    t.pan = 0.0;
    t.mute = false;
    t.solo = false;
    t.sends.clear();
    t.parent = None;
    t.vca = None;
    t.master_send = true;
    t.automation.retain(|l| {
        matches!(
            l.target,
            audiostrada_core::automation::AutomationTarget::Param { .. }
        )
    });
    copy.tracks = std::iter::once(std::sync::Arc::new(t))
        .chain(members)
        .collect();
    copy.master.gain_db = 0.0;
    copy.master.inserts.clear();
    copy.master.automation.clear();
    Some(copy)
}

impl Document {
    fn export_range(&self, settings: &ExportSettings) -> (u64, u64) {
        let project = self.project();
        match (settings.range, project.loop_range) {
            (ExportRange::Loop, Some((a, b))) => (a, b),
            _ => (0, project.length()),
        }
    }

    /// Starts exporting to `target` (the mix file; stems go next to it).
    pub fn start_export(&mut self, settings: ExportSettings, target: &Path) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("an export is already running").into());
        }
        let jobs = self.export_jobs(settings, target)?;
        self.begin(jobs.into(), false);
        Ok(())
    }

    /// Adds an export to the render queue, keeping the project as it is now.
    pub fn queue_export(&mut self, settings: ExportSettings, target: &Path) -> Result<(), String> {
        let label = format!(
            "{} ({}{})",
            target
                .with_extension(settings.format.extension())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            settings.format.name(),
            if settings.stems { tr(", stems") } else { "" },
        );
        let jobs = self.export_jobs(settings, target)?;
        self.render_queue.push(QueuedExport { label, jobs });
        Ok(())
    }

    /// The queued exports: what each writes and its number of files.
    pub fn queued_exports(&self) -> Vec<(String, usize)> {
        self.render_queue
            .iter()
            .map(|q| (q.label.clone(), q.jobs.len()))
            .collect()
    }

    pub fn remove_queued_export(&mut self, index: usize) {
        if index < self.render_queue.len() {
            self.render_queue.remove(index);
        }
    }

    /// Renders every queued export, one after the other.
    pub fn render_queue(&mut self) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("an export is already running").into());
        }
        if self.render_queue.is_empty() {
            return Err(tr("the render queue is empty").into());
        }
        let jobs = std::mem::take(&mut self.render_queue)
            .into_iter()
            .flat_map(|q| q.jobs)
            .collect();
        self.begin(jobs, false);
        Ok(())
    }

    fn begin(&mut self, pending: VecDeque<Job>, freeze: bool) {
        self.exporting = Some(Exporting {
            total: pending.len(),
            pending,
            current: None,
            writing: Vec::new(),
            done: 0,
            written: 0,
            errors: Vec::new(),
            freeze,
        });
    }

    /// The renders an export to `target` needs: the mix and/or one per stem.
    fn export_jobs(&mut self, settings: ExportSettings, target: &Path) -> Result<Vec<Job>, String> {
        self.capture_plugin_states();
        let project = self.project().clone();
        let (start, end) = self.export_range(&settings);
        if end <= start {
            return Err(tr("there is nothing to export").into());
        }
        let ext = settings.format.extension();
        let target = target.with_extension(ext);
        let dir = target.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = target
            .file_stem()
            .map_or_else(|| "Mix".to_owned(), |s| s.to_string_lossy().into_owned());
        let mut pending = Vec::new();
        if settings.mix {
            pending.push(Job {
                settings: settings.clone(),
                project: project.clone(),
                output: target.clone(),
                kind: JobKind::Mix,
                start,
                end,
            });
        }
        if settings.stems {
            for track in project
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Audio && !t.clips.is_empty())
            {
                let mut soloed = project.clone();
                for t in &mut soloed.tracks {
                    std::sync::Arc::make_mut(t).solo = t.id == track.id;
                }
                pending.push(Job {
                    settings: settings.clone(),
                    project: soloed,
                    output: dir.join(format!("{stem} - {}.{ext}", file_name(&track.name))),
                    kind: JobKind::Mix,
                    start,
                    end,
                });
            }
        }
        if pending.is_empty() {
            return Err(tr("choose the mix, the stems or both").into());
        }
        Ok(pending)
    }

    /// Exports one track, soloed with the buses it feeds, to `target`.
    pub fn export_track(
        &mut self,
        track: TrackId,
        settings: ExportSettings,
        target: &Path,
    ) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("an export is already running").into());
        }
        self.capture_plugin_states();
        let (start, end) = self.export_range(&settings);
        if end <= start {
            return Err(tr("there is nothing to export").into());
        }
        let mut soloed = self.project().clone();
        for t in &mut soloed.tracks {
            std::sync::Arc::make_mut(t).solo = t.id == track;
        }
        let output = target.with_extension(settings.format.extension());
        let job = Job {
            settings,
            project: soloed,
            output,
            kind: JobKind::Mix,
            start,
            end,
        };
        self.begin([job].into(), false);
        Ok(())
    }

    /// Renders a track with its inserts to a file and plays that instead (freeze).
    pub fn freeze(&mut self, track: TrackId) -> Result<(), String> {
        self.render_track(track, JobKind::Freeze(track), "frozen")
    }

    /// Renders a track with its inserts onto a new track and mutes the original.
    pub fn bounce(&mut self, track: TrackId) -> Result<(), String> {
        self.render_track(track, JobKind::Bounce(track), "bounce")
    }

    fn render_track(&mut self, track: TrackId, kind: JobKind, label: &str) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("wait for the current export to finish").into());
        }
        let name = self
            .project()
            .track(track)
            .ok_or(tr("unknown track"))?
            .name
            .clone();
        self.capture_plugin_states();
        let project = isolate(self.project(), track).ok_or(tr("unknown track"))?;
        let dir = match (&self.paths, &self.dirs.recordings) {
            (Some(paths), _) => paths.audio_dir(),
            (None, Some(dir)) => dir.clone(),
            (None, None) => std::env::temp_dir().join("audiostrada-recordings"),
        };
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let output = (1..)
            .map(|n| dir.join(format!("{} ({label} {n}).wav", file_name(&name))))
            .find(|p| !p.exists())
            .unwrap_or_else(|| dir.join(format!("{label}.wav")));
        let end = project.length();
        let settings = ExportSettings {
            format: FileFormat::Wav32Float,
            dither: false,
            normalize: Normalize::Off,
            sample_rate: None,
            tail: 2.0,
            ..ExportSettings::default()
        };
        let job = Job {
            settings,
            project,
            output,
            kind,
            start: 0,
            end,
        };
        self.begin([job].into(), true);
        Ok(())
    }

    /// Consolidate: renders what each of `spans` (track, start, end) plays (no effects, level
    /// or pan) to a file that replaces the clips there.
    pub fn consolidate(&mut self, spans: &[(TrackId, u64, u64)]) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("wait for the current render to finish").into());
        }
        let mut jobs = std::collections::VecDeque::new();
        for &(track, start, end) in spans.iter().filter(|s| s.2 > s.1) {
            let name = self
                .project()
                .track(track)
                .ok_or(tr("unknown track"))?
                .name
                .clone();
            let project = clips_only(self.project(), track, |_| true, Vec::new())
                .ok_or(tr("unknown track"))?;
            let output = self.render_file(&format!("{name} (consolidated)"))?;
            jobs.push_back(Job {
                settings: render_settings(0.0, Normalize::Off),
                project,
                output,
                kind: JobKind::Consolidate { track, start, end },
                start,
                end,
            });
        }
        if jobs.is_empty() {
            return Err(tr("nothing to consolidate").into());
        }
        self.begin(jobs, true);
        Ok(())
    }

    /// Makes one stereo track of two: told left and right by their names, else the upper one
    /// is the left one. Their clips are rendered side by side (no effects, level or pan); the
    /// left track keeps its settings and takes the result, the right one goes.
    pub fn make_stereo(&mut self, a: TrackId, b: TrackId) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("wait for the current render to finish").into());
        }
        let project = self.project();
        let track = |id| project.track(id).ok_or(tr("unknown track"));
        let (ta, tb) = (track(a)?, track(b)?);
        if a == b || ta.kind != TrackKind::Audio || tb.kind != TrackKind::Audio {
            return Err(tr("select two audio tracks").into());
        }
        if routing::has_children(project, a) || routing::has_children(project, b) {
            return Err(tr("folder tracks cannot be converted").into());
        }
        let index = |id| project.tracks.iter().position(|t| t.id == id);
        let (left, right) = match (name_side(&ta.name), name_side(&tb.name)) {
            (Some((true, _)), Some((false, _))) => (a, b),
            (Some((false, _)), Some((true, _))) => (b, a),
            _ if index(a) <= index(b) => (a, b),
            _ => (b, a),
        };
        let clips = [ta, tb]
            .into_iter()
            .flat_map(|t| t.clips.iter().filter(|c| c.notes.is_none()));
        let (start, end) = clips.fold((u64::MAX, 0), |(s, e), c| (s.min(c.start), e.max(c.end())));
        if start >= end {
            return Err(tr("the tracks hold no audio").into());
        }
        let name = stereo_name(project, left, right);
        let pair = stereo_pair(project, left, right);
        let output = self.render_file(&format!("{name} (stereo)"))?;
        self.begin(
            std::collections::VecDeque::from([Job {
                settings: render_settings(0.0, Normalize::Off),
                project: pair,
                output,
                kind: JobKind::Stereo { left, right, start },
                start,
                end,
            }]),
            true,
        );
        Ok(())
    }

    fn apply_stereo(&mut self, left: TrackId, right: TrackId, start: u64, finished: &Finished) {
        let source = self.rendered_source(finished);
        let name = stereo_name(self.project(), left, right);
        self.history.edit(tr("Convert to a stereo track"), |p| {
            let source = p.add_source(source);
            if let Some(t) = p.track_mut(left) {
                t.name = name;
                t.clips.clear();
                t.alternates.clear();
                t.stereo = true;
                t.pan = 0.0;
                t.frozen = None;
            }
            p.add_clip(left, source, start);
            p.remove_track(right);
            p.prune_sources();
        });
        self.messages.push(tr("Converted to a stereo track").into());
    }

    /// AudioSuite: renders each clip through `process` and puts the result in its place.
    pub fn process_clips(&mut self, clips: &[ClipId], process: &Process) -> Result<(), String> {
        if self.exporting.is_some() {
            return Err(tr("wait for the current render to finish").into());
        }
        self.capture_plugin_states();
        let mut jobs = std::collections::VecDeque::new();
        for &id in clips {
            let project = self.project();
            let Some(clip) =
                audiostrada_core::edit::clip(project, id).filter(|c| c.notes.is_none())
            else {
                continue;
            };
            let Some(track) = project
                .tracks
                .iter()
                .find(|t| t.clips.iter().any(|c| c.id == id))
                .map(|t| t.id)
            else {
                continue;
            };
            let (inserts, normalize, tail) = match process {
                Process::Effect {
                    id: effect,
                    params,
                    tail,
                } => {
                    let mut scratch = project.clone();
                    let insert = routing::add_insert(
                        &mut scratch,
                        routing::Channel::Track(track),
                        effect,
                        params.clone(),
                    )
                    .and_then(|i| {
                        routing::inserts(&scratch, routing::Channel::Track(track))
                            .iter()
                            .find(|x| x.id == i)
                            .cloned()
                    })
                    .ok_or(tr("unknown effect"))?;
                    (vec![insert], Normalize::Off, *tail)
                }
                Process::Reverse => (Vec::new(), Normalize::Off, false),
                Process::Normalize(db) => (Vec::new(), Normalize::Peak(*db), false),
            };
            let (start, end) = (clip.start, clip.end());
            let name = format!("{} ({})", clip.name, process.name());
            let project =
                clips_only(project, track, |c| c.id == id, inserts).ok_or(tr("unknown track"))?;
            let output = self.render_file(&name)?;
            jobs.push_back(Job {
                settings: render_settings(if tail { 10.0 } else { 0.0 }, normalize),
                project,
                output,
                kind: JobKind::Process {
                    clip: id,
                    reverse: *process == Process::Reverse,
                    tail,
                },
                start,
                end,
            });
        }
        if jobs.is_empty() {
            return Err(tr("select audio clips to process").into());
        }
        self.begin(jobs, true);
        Ok(())
    }

    /// A new file for rendered audio, in the project's audio folder.
    pub(super) fn render_file(&self, name: &str) -> Result<PathBuf, String> {
        let dir = match (&self.paths, &self.dirs.recordings) {
            (Some(paths), _) => paths.audio_dir(),
            (None, Some(dir)) => dir.clone(),
            (None, None) => std::env::temp_dir().join("audiostrada-recordings"),
        };
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok((1..)
            .map(|n| dir.join(format!("{} {n}.wav", file_name(name))))
            .find(|p| !p.exists())
            .unwrap_or_else(|| dir.join("render.wav")))
    }

    fn apply_consolidate(&mut self, track: TrackId, start: u64, end: u64, finished: &Finished) {
        let source = self.rendered_source(finished);
        self.history.edit(tr("Consolidate"), |p| {
            audiostrada_core::edit::delete_range(p, &[track], start, end, false);
            let source = p.add_source(source);
            if let Some(id) = p.add_clip(track, source, start)
                && let Some(c) = audiostrada_core::edit::clip_mut(p, id)
            {
                c.length = c.length.min(end - start);
            }
        });
        self.messages.push(tr("Clips consolidated").into());
    }

    fn apply_process(&mut self, clip: ClipId, reverse: bool, tail: bool, finished: &Finished) {
        if reverse && let Err(e) = reverse_file(&finished.path) {
            self.messages
                .push(trf!("Cannot reverse: {error}", error = e));
            return;
        }
        let source = self.rendered_source(finished);
        let rate = self.project().sample_rate;
        self.history.edit(tr("Process clip"), |p| {
            let length = source.frames_at(rate);
            let source = p.add_source(source);
            if let Some(c) = audiostrada_core::edit::clip_mut(p, clip) {
                // Gain, fades, stretch and pitch are in the rendered audio now.
                c.source = source;
                c.offset = 0;
                c.stretch = 1.0;
                c.pitch = 0.0;
                c.gain_db = 0.0;
                c.fade_in = 0;
                c.fade_out = 0;
                if tail {
                    c.length = length;
                }
            }
        });
    }

    pub fn unfreeze(&mut self, track: TrackId) {
        self.history.edit(tr("Unfreeze track"), |p| {
            if let Some(t) = p.track_mut(track) {
                t.frozen = None;
            }
            p.prune_sources();
        });
    }

    /// Progress of the export (0 to 1) and what is being done.
    pub fn export_progress(&self) -> Option<(f32, String)> {
        let e = self.exporting.as_ref()?;
        let current = e.current.as_ref().map_or(0.0, Rendering::progress);
        let fraction = (e.done as f32 + current) / e.total.max(1) as f32;
        let what = match &e.current {
            Some(Rendering { path, .. }) => trf!(
                "Rendering {file}",
                file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            None => tr("Writing files").into(),
        };
        Some((fraction.min(1.0), what))
    }

    pub fn cancel_export(&mut self) {
        if let Some(e) = self.exporting.take() {
            if let Some(current) = &e.current {
                current.cancel.store(true, Ordering::Relaxed);
            }
            self.messages.push(tr("Export cancelled").into());
        }
    }

    /// Starts the next render when the previous one is done and collects the results; the
    /// renders themselves run on their own thread.
    pub(super) fn poll_export(&mut self) {
        let Some(mut e) = self.exporting.take() else {
            return;
        };
        if let Some(current) = &mut e.current {
            current.player.update();
        }
        if e.current.as_ref().is_some_and(|r| r.handle.is_finished())
            && let Some(done) = e.current.take()
        {
            e.writing.push(done.handle);
            e.done += 1;
        }
        // A render started while media decode would hear silence where they play.
        if e.current.is_none()
            && self.media.loading() == 0
            && let Some(job) = e.pending.pop_front()
        {
            let render = OfflineRender::new(
                &job.project,
                self.paths.as_ref(),
                &self.media,
                self.player.plugins().clone(),
                job.start,
                job.end,
                f64::from(job.settings.tail),
            );
            e.current = Some(Rendering::start(render, job));
        }
        let (finished, running): (Vec<_>, Vec<_>) =
            e.writing.into_iter().partition(|h| h.is_finished());
        e.writing = running;
        for handle in finished {
            match handle.join() {
                Ok(Ok(f)) => {
                    e.written += 1;
                    match f.kind {
                        JobKind::Freeze(track) => self.apply_freeze(track, &f),
                        JobKind::Bounce(track) => self.apply_bounce(track, &f),
                        JobKind::Consolidate { track, start, end } => {
                            self.apply_consolidate(track, start, end, &f)
                        }
                        JobKind::Stereo { left, right, start } => {
                            self.apply_stereo(left, right, start, &f)
                        }
                        JobKind::Process {
                            clip,
                            reverse,
                            tail,
                        } => self.apply_process(clip, reverse, tail, &f),
                        JobKind::Mix => {}
                    }
                }
                Ok(Err(error)) => e.errors.push(error),
                Err(_) => e.errors.push(tr("the export thread crashed").into()),
            }
        }
        if e.current.is_some() || !e.pending.is_empty() || !e.writing.is_empty() {
            self.exporting = Some(e);
            return;
        }
        self.messages.extend(
            e.errors
                .iter()
                .map(|err| trf!("Export failed: {error}", error = err)),
        );
        if e.written > 0 && !e.freeze {
            self.messages
                .push(trf!("Exported {count} file(s)", count = e.written));
        }
    }

    fn rendered_source(&self, finished: &Finished) -> Source {
        let stored = self
            .paths
            .as_ref()
            .map_or_else(|| finished.path.clone(), |p| p.relativize(&finished.path));
        Source {
            path: stored,
            channels: 2,
            sample_rate: finished.rate,
            frames: finished.frames,
        }
    }

    fn apply_bounce(&mut self, track: TrackId, finished: &Finished) {
        let source = self.rendered_source(finished);
        self.history.edit(tr("Bounce track"), |p| {
            let Some(original) = p.track(track).cloned() else {
                return;
            };
            let source = p.add_source(source);
            let new = p.add_track(&format!("{} (bounce)", original.name));
            p.add_clip(new, source, 0);
            if let Some(t) = p.track_mut(new) {
                t.color = original.color;
                t.parent = original.parent;
                t.stereo = true;
            }
            if let Some(t) = p.track_mut(track) {
                t.mute = true;
            }
            let last = routing::subtree(p, track)
                .iter()
                .filter_map(|id| p.tracks.iter().position(|t| t.id == *id))
                .max();
            if let (Some(last), Some(from)) = (last, p.tracks.iter().position(|t| t.id == new)) {
                let moved = p.tracks.remove(from);
                p.tracks.insert((last + 1).min(p.tracks.len()), moved);
            }
        });
        self.messages
            .push(tr("Track bounced to a new track").into());
    }

    fn apply_freeze(&mut self, track: TrackId, finished: &Finished) {
        let source = self.rendered_source(finished);
        self.history.edit(tr("Freeze track"), |p| {
            let source = p.add_source(source);
            if let Some(t) = p.track_mut(track) {
                t.frozen = Some(Frozen { source, start: 0 });
            }
        });
        self.messages.push(tr("Track frozen").into());
    }
}

/// Settings of an internal render: float WAV at the project rate.
fn render_settings(tail: f32, normalize: Normalize) -> ExportSettings {
    ExportSettings {
        format: FileFormat::Wav32Float,
        dither: false,
        normalize,
        sample_rate: None,
        tail,
        ..ExportSettings::default()
    }
}

/// Reverses a WAV file in place.
fn reverse_file(path: &std::path::Path) -> Result<(), String> {
    let data = audiostrada_media::load(path).map_err(|e| e.to_string())?;
    let channels: Vec<Vec<f32>> = (0..data.channels())
        .map(|ch| data.channel(ch).iter().rev().copied().collect())
        .collect();
    let reversed = audiostrada_media::AudioData::new(channels, data.sample_rate());
    audiostrada_media::write_wav(path, &reversed).map_err(|e| e.to_string())
}
