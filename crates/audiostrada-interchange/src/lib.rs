//! Projects of other DAWs: DAWproject (Bitwig, Studio One, Cubase…) read and written, Reaper's
//! `.rpp` read. An imported project becomes an AudioStrada project in a folder of its own next
//! to the file, its media copied in or referenced where they are.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use audiostrada_core::automation::{AutomationLane, AutomationPoint, AutomationTarget, CurveShape};
use audiostrada_core::file::{self, ProjectPaths};
use audiostrada_core::{Project, Source, SourceId, TrackId};

pub mod dawproject;
mod reaper;
mod vstpreset;
mod zip;

/// Something of the other project AudioStrada could not take as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Warning {
    /// An effect or instrument of the other DAW, left out.
    Left(String),
    /// A plugin kept without its settings.
    NoSettings(String),
    /// A media file that was not found.
    Missing(PathBuf),
}

#[derive(Debug)]
pub struct Imported {
    /// The AudioStrada project written.
    pub file: PathBuf,
    pub warnings: Vec<Warning>,
}

/// A file this module reads.
pub fn is_foreign(path: &Path) -> bool {
    matches!(extension(path).as_str(), "dawproject" | "rpp")
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Reads a project of another DAW and writes it as an AudioStrada project, in a new folder
/// next to it.
pub fn import(path: &Path) -> Result<Imported, String> {
    let stem = path.file_stem().map_or_else(
        || "Imported".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut name = stem.clone();
    let mut n = 2;
    while parent.join(&name).exists() {
        name = format!("{stem} {n}");
        n += 1;
    }
    let paths = ProjectPaths::in_new_folder(parent, &name);
    std::fs::create_dir_all(paths.audio_dir()).map_err(|e| e.to_string())?;
    let result = match extension(path).as_str() {
        "dawproject" => dawproject::import(path, &paths, &name),
        "rpp" => reaper::import(path, &paths, &name),
        other => Err(format!("unknown project format .{other}")),
    };
    let (project, warnings) = match result {
        Ok(done) => done,
        Err(e) => {
            let _ = std::fs::remove_dir_all(paths.dir());
            return Err(e);
        }
    };
    file::save(&project, &paths).map_err(|e| e.to_string())?;
    Ok(Imported {
        file: paths.file,
        warnings,
    })
}

/// A project being built from another one.
struct Build {
    project: Project,
    paths: ProjectPaths,
    warnings: Vec<Warning>,
    sources: BTreeMap<PathBuf, SourceId>,
}

impl Build {
    fn new(paths: &ProjectPaths, name: &str, rate: u32) -> Self {
        Self {
            project: Project::new(name, rate),
            paths: paths.clone(),
            warnings: Vec::new(),
            sources: BTreeMap::new(),
        }
    }

    /// The source of the audio file at `path`, read once.
    fn source(&mut self, path: &Path) -> Option<SourceId> {
        if let Some(&id) = self.sources.get(path) {
            return Some(id);
        }
        let Ok((channels, sample_rate, frames)) = audiostrada_media::probe(path) else {
            self.warn(Warning::Missing(path.to_path_buf()));
            return None;
        };
        let id = self.project.add_source(Source {
            path: self.paths.relativize(path),
            channels,
            sample_rate,
            frames,
        });
        self.sources.insert(path.to_path_buf(), id);
        Some(id)
    }

    /// Writes `data` into the project's audio folder as `name` (made unique) and gives its path.
    fn store(&self, name: &str, data: &[u8]) -> Option<PathBuf> {
        let base = Path::new(name).file_name()?.to_string_lossy().into_owned();
        let dir = self.paths.audio_dir();
        let mut target = dir.join(&base);
        let mut n = 2;
        while target.exists() {
            let p = Path::new(&base);
            let stem = p.file_stem().unwrap_or_default().to_string_lossy();
            let ext = p
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            target = dir.join(format!("{stem} {n}{ext}"));
            n += 1;
        }
        std::fs::write(&target, data).ok()?;
        Some(target)
    }

    fn warn(&mut self, warning: Warning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// Adds automation points to a track's (or the master's) lane for `target`.
    fn automate(
        &mut self,
        track: Option<TrackId>,
        target: AutomationTarget,
        points: Vec<(u64, f32, bool)>,
    ) {
        if points.is_empty() {
            return;
        }
        let lanes = match track {
            Some(t) => match self.project.track_mut(t) {
                Some(t) => &mut t.automation,
                None => return,
            },
            None => &mut self.project.master.automation,
        };
        let mut lane = AutomationLane::new(target);
        for (time, value, step) in points {
            let shape = if step {
                CurveShape::Step
            } else {
                CurveShape::Linear
            };
            lane.points.retain(|p| p.time != time);
            lane.points.push(AutomationPoint::new(time, value, shape));
        }
        lane.points.sort_by_key(|p| p.time);
        lanes.push(lane);
    }
}

/// Linear gain to decibels.
fn db(gain: f64) -> f32 {
    if gain <= 1e-6 {
        -144.0
    } else {
        (20.0 * gain.log10()) as f32
    }
}

fn gain(db: f32) -> f64 {
    if db <= -144.0 {
        0.0
    } else {
        10f64.powf(f64::from(db) / 20.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_core::routing::Channel;
    use audiostrada_core::tempo::{TempoMap, TempoPoint, TimeSignature};
    use audiostrada_core::{Marker, Note, TrackKind, TrackSend};

    #[test]
    fn a_project_exported_as_dawproject_comes_back_the_same() {
        let dir = audiostrada_core::scratch::dir("dawproject-round-trip");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("tone.wav");
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        audiostrada_media::write_wav(
            &wav,
            &audiostrada_media::AudioData::new(vec![tone.clone(), tone], 48_000),
        )
        .unwrap();

        let mut p = Project::new("Song", 48_000);
        p.tempo = TempoMap::new(
            vec![TempoPoint {
                beat: 0.0,
                bpm: 100.0,
            }],
            vec![TimeSignature {
                bar: 0,
                numerator: 3,
                denominator: 4,
            }],
        );
        let bus = p.add_track("Drums");
        let kick = p.add_track("Kick");
        let keys = p.add_track("Keys");
        let verb = p.add_track("Reverb");
        let source = p.add_source(Source {
            path: wav.clone(),
            channels: 2,
            sample_rate: 48_000,
            frames: 48_000,
        });
        p.add_clip(kick, source, 96_000);
        p.add_midi_clip(
            keys,
            48_000,
            96_000,
            vec![Note {
                start: 24_000,
                length: 12_000,
                key: 60,
                velocity: 100,
                ..Note::default()
            }],
        );
        audiostrada_core::routing::add_insert(
            &mut p,
            Channel::Track(keys),
            "synth",
            BTreeMap::new(),
        );
        for (id, kind) in [(bus, TrackKind::Bus), (verb, TrackKind::Bus)] {
            p.track_mut(id).unwrap().kind = kind;
        }
        let k = p.track_mut(kick).unwrap();
        k.parent = Some(bus);
        k.gain_db = -6.0;
        k.pan = -0.5;
        k.sends.push(TrackSend {
            target: verb,
            gain_db: -12.0,
            pre_fader: false,
            mute: false,
        });
        let mut lane = AutomationLane::new(AutomationTarget::Gain);
        lane.points = vec![
            AutomationPoint::new(0, 0.0, CurveShape::Linear),
            AutomationPoint::new(48_000, -12.0, CurveShape::Linear),
        ];
        k.automation.push(lane);
        let id = p.new_marker_id();
        p.markers.push(Marker {
            id,
            position: 144_000,
            name: "Chorus".into(),
        });

        let out = dir.join("song.dawproject");
        let warnings = dawproject::export(&p, None, &out).unwrap();
        assert_eq!(warnings, [Warning::Left("synth".into())]);
        let imported = import(&out).unwrap();
        let q = file::load(&imported.file).unwrap();

        assert_eq!(q.name, "Song");
        assert!((q.tempo.bpm_at_beat(0.0) - 100.0).abs() < 1e-6);
        assert_eq!(q.tempo.signatures()[0].numerator, 3);
        let names: Vec<&str> = q.tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Drums", "Kick", "Keys", "Reverb"]);
        let by = |n: &str| q.tracks.iter().find(|t| t.name == n).unwrap();
        let (qbus, qkick, qkeys, qverb) = (by("Drums"), by("Kick"), by("Keys"), by("Reverb"));
        assert_eq!(qbus.kind, TrackKind::Bus);
        assert_eq!(qkick.parent, Some(qbus.id));
        assert!((qkick.gain_db + 6.0).abs() < 0.01 && (qkick.pan + 0.5).abs() < 1e-4);
        assert_eq!(qkick.sends.len(), 1);
        assert_eq!(qkick.sends[0].target, qverb.id);
        assert!((qkick.sends[0].gain_db + 12.0).abs() < 0.01);
        let clip = &qkick.clips[0];
        assert_eq!((clip.start, clip.length), (96_000, 48_000));
        let s = &q.sources[&clip.source];
        assert_eq!((s.channels, s.frames), (2, 48_000));
        let notes = qkeys.clips[0].notes.as_ref().unwrap();
        assert_eq!(
            (qkeys.clips[0].start, notes[0].start, notes[0].key),
            (48_000, 24_000, 60)
        );
        assert_eq!(qkeys.inserts[0].effect, "synth");
        let gain = &qkick.automation[0];
        assert_eq!(gain.target, AutomationTarget::Gain);
        assert!((gain.points[1].value + 12.0).abs() < 0.01);
        assert_eq!(q.markers[0].position, 144_000);
    }
}
