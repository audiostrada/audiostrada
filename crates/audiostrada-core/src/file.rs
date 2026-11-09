//! Project files: versioned JSON, atomic saves, media paths relative to the project folder.
//!
//! A project lives in its own folder: `Name/Name.strada`, with imported media in
//! `Name/Audio Files/` and peak caches in `Name/Peaks/`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Project, SourceId};

pub const EXTENSION: &str = "strada";
const FORMAT: &str = "audiostrada-project";
/// Version written by this build. Older files are migrated on load.
pub const VERSION: u32 = 3;
const AUDIO_DIR: &str = "Audio Files";
const PEAKS_DIR: &str = "Peaks";
const VERSIONS_DIR: &str = "Versions";
/// Versions of the project file kept, the newest.
const VERSIONS_KEPT: usize = 30;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("not an AudioStrada project: {0}")]
    Format(String),
    #[error(
        "project saved by a newer AudioStrada (format {found}, this version reads up to {VERSION})"
    )]
    TooNew { found: u32 },
}

#[derive(Serialize, Deserialize)]
struct Envelope<P> {
    format: String,
    version: u32,
    project: P,
}

/// Locations derived from a project file path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectPaths {
    pub file: PathBuf,
}

impl ProjectPaths {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// `parent/name/name.strada`, for a new project.
    pub fn in_new_folder(parent: &Path, name: &str) -> Self {
        Self::new(parent.join(name).join(format!("{name}.{EXTENSION}")))
    }

    pub fn dir(&self) -> &Path {
        self.file.parent().unwrap_or(Path::new("."))
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.dir().join(AUDIO_DIR)
    }

    /// Amp models and impulse responses used by built-in effects.
    pub fn models_dir(&self) -> PathBuf {
        self.dir().join("Models")
    }

    pub fn peaks_dir(&self) -> PathBuf {
        self.dir().join(PEAKS_DIR)
    }

    /// Earlier states of the project file, one per save.
    pub fn versions_dir(&self) -> PathBuf {
        self.dir().join(VERSIONS_DIR)
    }

    pub fn autosave(&self) -> PathBuf {
        let mut name = self.file.file_name().unwrap_or_default().to_os_string();
        name.push(".autosave");
        self.file.with_file_name(name)
    }

    pub fn backup(&self) -> PathBuf {
        let mut name = self.file.file_name().unwrap_or_default().to_os_string();
        name.push(".bak");
        self.file.with_file_name(name)
    }

    /// Absolute path of a media file referenced by the project.
    pub fn resolve(&self, media: &Path) -> PathBuf {
        if media.is_absolute() {
            media.to_path_buf()
        } else {
            self.dir().join(media)
        }
    }

    /// Path to store in the project: relative when the file is inside the project folder.
    pub fn relativize(&self, media: &Path) -> PathBuf {
        media
            .strip_prefix(self.dir())
            .map_or_else(|_| media.to_path_buf(), Path::to_path_buf)
    }
}

/// Writes `bytes` to `path` through a temporary file and a rename, so a crash never leaves a
/// truncated file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Whether `path` names a project file.
pub fn is_project(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == EXTENSION)
}

pub fn to_json(project: &Project) -> Result<Vec<u8>, ProjectError> {
    let envelope = Envelope {
        format: FORMAT.to_owned(),
        version: VERSION,
        project,
    };
    serde_json::to_vec_pretty(&envelope).map_err(|e| ProjectError::Format(e.to_string()))
}

pub fn from_json(bytes: &[u8]) -> Result<Project, ProjectError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ProjectError::Format(e.to_string()))?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(ProjectError::Format("missing format marker".into()));
    }
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProjectError::Format("missing version".into()))? as u32;
    if version > VERSION {
        return Err(ProjectError::TooNew { found: version });
    }
    let mut value = migrate(value, version)?;
    // Saved before Strada-Amp's models were renumbered.
    crate::legacy::upgrade_amp_rigs(&mut value);
    let envelope: Envelope<Project> =
        serde_json::from_value(value).map_err(|e| ProjectError::Format(e.to_string()))?;
    let mut project = envelope.project;
    if version < 3 {
        crate::legacy::folders_to_buses(&mut project);
    }
    if !(8_000..=768_000).contains(&project.sample_rate) {
        return Err(ProjectError::Format(format!(
            "invalid sample rate {}",
            project.sample_rate
        )));
    }
    if project.highest_id() > 1 << 48 || project.peek_id() > 1 << 48 {
        return Err(ProjectError::Format("invalid object ids".into()));
    }
    project.repair();
    project.rebase_midi();
    Ok(project)
}

/// Upgrades a parsed file from `version` to [`VERSION`], one step at a time.
fn migrate(value: Value, version: u32) -> Result<Value, ProjectError> {
    match version {
        VERSION => Ok(value),
        0 => Err(ProjectError::Format("invalid version 0".into())),
        1 => migrate(crate::legacy::v1_to_v2(value), 2),
        // Folders become buses once the project is read (`legacy::folders_to_buses`).
        2 => Ok(value),
        v => Err(ProjectError::Format(format!(
            "no migration from version {v}"
        ))),
    }
}

/// Copies `file` into the project's audio folder under a free name; returns the path to store
/// in the project (relative). A file already in the folder is used as is.
pub fn import_media(paths: &ProjectPaths, file: &Path) -> Result<PathBuf, ProjectError> {
    let audio_dir = paths.audio_dir();
    if file.starts_with(&audio_dir) {
        return Ok(paths.relativize(file));
    }
    fs::create_dir_all(&audio_dir)?;
    let name = file
        .file_name()
        .ok_or_else(|| ProjectError::Format(format!("not a file: {}", file.display())))?;
    let size = fs::metadata(file)?.len();
    let mut target = audio_dir.join(name);
    let mut n = 2;
    while target.exists() {
        if fs::metadata(&target).map(|m| m.len()).ok() == Some(size) {
            return Ok(paths.relativize(&target));
        }
        let stem = Path::new(name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let ext = Path::new(name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        target = audio_dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    fs::copy(file, &target)?;
    Ok(paths.relativize(&target))
}

/// Copies every source located outside `paths`' folder into its audio folder (media of an
/// untitled project, or of the previous location for "Save As"). Returns the new paths.
pub fn collect_media(
    project: &Project,
    paths: &ProjectPaths,
    previous: Option<&ProjectPaths>,
) -> Result<Vec<(SourceId, PathBuf)>, ProjectError> {
    let mut moved = Vec::new();
    for (id, source) in &project.sources {
        let current = previous.map_or_else(|| source.path.clone(), |p| p.resolve(&source.path));
        if current.is_relative() || current.starts_with(paths.dir()) {
            continue;
        }
        moved.push((*id, import_media(paths, &current)?));
    }
    Ok(moved)
}

/// Saves the project, keeping the previous file as a `.bak` backup and as a numbered version.
pub fn save(project: &Project, paths: &ProjectPaths) -> Result<(), ProjectError> {
    let bytes = to_json(project)?;
    if paths.file.exists() {
        fs::copy(&paths.file, paths.backup())?;
        keep_version(paths)?;
    }
    write_atomic(&paths.file, &bytes)?;
    Ok(())
}

/// Saved versions of the project, the newest first, with their number.
pub fn versions(paths: &ProjectPaths) -> Vec<(u32, PathBuf)> {
    let stem = paths.file.file_stem().unwrap_or_default().to_string_lossy();
    let prefix = format!("{stem} v");
    let mut found: Vec<(u32, PathBuf)> = fs::read_dir(paths.versions_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let number = name.strip_prefix(&prefix)?.parse().ok()?;
            is_project(&path).then_some((number, path))
        })
        .collect();
    found.sort_by_key(|v| std::cmp::Reverse(v.0));
    found
}

/// Copies the project file as the next version unless it is the newest one already, and
/// drops the oldest beyond what is kept.
fn keep_version(paths: &ProjectPaths) -> std::io::Result<()> {
    let current = fs::read(&paths.file)?;
    let existing = versions(paths);
    if let Some((_, newest)) = existing.first()
        && fs::read(newest).is_ok_and(|b| b == current)
    {
        return Ok(());
    }
    let number = existing.first().map_or(1, |v| v.0 + 1);
    let stem = paths.file.file_stem().unwrap_or_default().to_string_lossy();
    let target = paths
        .versions_dir()
        .join(format!("{stem} v{number}.{EXTENSION}"));
    write_atomic(&target, &current)?;
    for (_, old) in existing.iter().skip(VERSIONS_KEPT - 1) {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Project, ProjectError> {
    from_json(&fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = crate::scratch::dir(&format!("core-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sample_project() -> Project {
        let mut p = Project::new("Song", 44_100);
        let s = p.add_source(Source {
            path: PathBuf::from("Audio Files/bass.flac"),
            channels: 1,
            sample_rate: 44_100,
            frames: 1000,
        });
        let t = p.add_track("Bass");
        p.add_clip(t, s, 500);
        p.track_mut(t).unwrap().gain_db = -3.5;
        p.loop_range = Some((0, 2000));
        p
    }

    #[test]
    fn a_loop_kept_off_survives_and_older_loops_still_play() {
        let mut p = sample_project();
        let older = to_json(&p).unwrap();
        assert!(
            !String::from_utf8_lossy(&older).contains("loop_off"),
            "nothing written while it is on"
        );
        assert_eq!(from_json(&older).unwrap().looping(), Some((0, 2000)));
        p.loop_off = true;
        p.loop_fixed = true;
        let back = from_json(&to_json(&p).unwrap()).unwrap();
        assert_eq!((back.loop_range, back.looping()), (Some((0, 2000)), None));
        assert!(back.loop_fixed);
    }

    #[test]
    fn json_round_trip() {
        let p = sample_project();
        let back = from_json(&to_json(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let mut q = back;
        assert_ne!(
            q.add_track("new"),
            p.tracks[0].id,
            "id counter survives the round trip"
        );
    }

    #[test]
    fn rejects_foreign_and_newer_files() {
        assert!(matches!(from_json(b"{}"), Err(ProjectError::Format(_))));
        assert!(matches!(
            from_json(b"not json"),
            Err(ProjectError::Format(_))
        ));
        let newer = br#"{"format":"audiostrada-project","version":99,"project":{}}"#;
        assert!(matches!(
            from_json(newer),
            Err(ProjectError::TooNew { found: 99 })
        ));
    }

    #[test]
    fn saves_keep_numbered_versions() {
        let dir = temp_dir("versions");
        let paths = ProjectPaths::in_new_folder(&dir, "Song");
        let mut project = sample_project();
        save(&project, &paths).unwrap();
        assert!(versions(&paths).is_empty(), "nothing before the first save");
        project.name = "Two".into();
        save(&project, &paths).unwrap();
        save(&project, &paths).unwrap();
        // Each save keeps the file it replaces.
        let kept = versions(&paths);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].0, 2);
        assert_eq!(load(&kept[0].1).unwrap().name, "Two");
        assert_eq!(load(&kept[1].1).unwrap().name, "Song");
        for n in 0..40 {
            project.name = format!("n{n}");
            save(&project, &paths).unwrap();
        }
        assert_eq!(versions(&paths).len(), VERSIONS_KEPT);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_keeps_a_backup_and_loads_back() {
        let dir = temp_dir("save");
        let paths = ProjectPaths::in_new_folder(&dir, "Song");
        let mut p = sample_project();
        save(&p, &paths).unwrap();
        p.name = "Renamed".into();
        save(&p, &paths).unwrap();
        assert_eq!(load(&paths.file).unwrap().name, "Renamed");
        assert_eq!(load(&paths.backup()).unwrap().name, "Song");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn module_data_survives_a_build_without_the_module() {
        let dir = temp_dir("extensions");
        let paths = ProjectPaths::in_new_folder(&dir, "Song");
        let mut p = sample_project();
        let data = serde_json::json!({"lyrics": [{"line": "Hello", "at": 3840}], "v": 2});
        p.extensions.insert("studio".into(), data.clone());
        save(&p, &paths).unwrap();
        let mut back = load(&paths.file).unwrap();
        back.name = "Edited".into();
        save(&back, &paths).unwrap();
        assert_eq!(load(&paths.file).unwrap().extensions["studio"], data);
        assert!(
            !String::from_utf8(to_json(&sample_project()).unwrap())
                .unwrap()
                .contains("extensions")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn media_is_copied_into_the_project() {
        let dir = temp_dir("import");
        fs::create_dir_all(&dir).unwrap();
        let outside = dir.join("take.wav");
        fs::write(&outside, b"audio").unwrap();
        let paths = ProjectPaths::in_new_folder(&dir, "Song");
        let first = import_media(&paths, &outside).unwrap();
        assert_eq!(first, PathBuf::from("Audio Files/take.wav"));
        assert_eq!(
            import_media(&paths, &outside).unwrap(),
            first,
            "same file reused"
        );
        fs::write(&outside, b"other audio").unwrap();
        assert_eq!(
            import_media(&paths, &outside).unwrap(),
            PathBuf::from("Audio Files/take (2).wav")
        );

        let mut p = Project::default();
        let id = p.add_source(Source {
            path: outside.clone(),
            channels: 1,
            sample_rate: 48_000,
            frames: 1,
        });
        let moved = collect_media(&p, &paths, None).unwrap();
        assert_eq!(moved, vec![(id, PathBuf::from("Audio Files/take (2).wav"))]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn media_paths_are_relative_inside_the_project() {
        let paths = ProjectPaths::new("/music/Song/Song.strada");
        assert_eq!(paths.audio_dir(), PathBuf::from("/music/Song/Audio Files"));
        assert_eq!(
            paths.relativize(Path::new("/music/Song/Audio Files/a.wav")),
            PathBuf::from("Audio Files/a.wav")
        );
        assert_eq!(
            paths.relativize(Path::new("/elsewhere/b.wav")),
            PathBuf::from("/elsewhere/b.wav")
        );
        assert_eq!(
            paths.resolve(Path::new("Audio Files/a.wav")),
            PathBuf::from("/music/Song/Audio Files/a.wav")
        );
        assert_eq!(
            paths.autosave(),
            PathBuf::from("/music/Song/Song.strada.autosave")
        );
    }
}
