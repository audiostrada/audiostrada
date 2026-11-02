//! Archiving: the media a project uses gathered into its folder, the project packed with all
//! of them into one .tar.gz to send or keep, and the versions kept at each save restored.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use audiostrada_core::file::{self, ProjectPaths};
use audiostrada_core::{Insert, Project};

use super::Document;
use crate::i18n::tr;
use crate::trf;

/// Where each file the project uses goes in a self-contained project folder.
struct Plan {
    /// The project with its paths relative to that folder.
    project: Project,
    /// Absolute path of each file (or capture pack folder) and its place in the folder.
    files: Vec<(PathBuf, PathBuf)>,
}

/// Plans the project folder: files inside it stay, the others go to `Audio Files` (audio) or
/// `Models` (amp captures, impulses), renamed when two different files share a name.
fn plan(project: &Project, paths: &ProjectPaths) -> Plan {
    let mut out = project.clone();
    let mut files: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut taken: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut place = |stored: &Path, folder: &Path| -> PathBuf {
        let absolute = paths.resolve(stored);
        if let Some((_, placed)) = files.iter().find(|(from, _)| *from == absolute) {
            return placed.clone();
        }
        let placed = if absolute.starts_with(paths.dir()) {
            paths.relativize(&absolute)
        } else {
            let name = absolute.file_name().unwrap_or_default();
            let stem = Path::new(name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let ext = Path::new(name)
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let mut placed = folder.join(name);
            let mut n = 2;
            while taken.get(&placed).is_some_and(|other| *other != absolute)
                || paths.dir().join(&placed).exists()
            {
                placed = folder.join(format!("{stem} ({n}){ext}"));
                n += 1;
            }
            placed
        };
        taken.insert(placed.clone(), absolute.clone());
        files.push((absolute, placed.clone()));
        placed
    };
    let audio = paths.relativize(&paths.audio_dir());
    let models = paths.relativize(&paths.models_dir());
    for source in out.sources.values_mut() {
        source.path = place(&source.path, &audio);
    }
    if let Some(reference) = &mut out.reference {
        reference.path = place(&reference.path, &audio);
    }
    let mut effect_files = |inserts: &mut Vec<Insert>| {
        for insert in inserts {
            for file in insert.files.values_mut() {
                *file = place(file, &models);
            }
        }
    };
    for track in &mut out.tracks {
        effect_files(&mut std::sync::Arc::make_mut(track).inserts);
    }
    effect_files(&mut out.master.inserts);
    Plan {
        project: out,
        files,
    }
}

/// Copies a file, or a capture pack folder with what it holds.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)?.flatten() {
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Packs the project file and the files it uses under `root/` in a gzipped tar.
fn pack(
    target: &Path,
    root: &str,
    project: &[u8],
    files: &[(PathBuf, PathBuf)],
) -> std::io::Result<()> {
    let out = std::fs::File::create(target)?;
    let gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    tar.follow_symlinks(true);
    let mut header = tar::Header::new_gnu();
    header.set_size(project.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    );
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("{root}/{root}.{}", file::EXTENSION),
        project,
    )?;
    for (from, to) in files {
        let name = Path::new(root).join(to);
        if from.is_dir() {
            tar.append_dir_all(&name, from)?;
        } else {
            tar.append_path_with_name(from, &name)?;
        }
    }
    tar.into_inner()?.finish()?.sync_all()
}

impl Document {
    /// Copies the files the project uses from outside its folder into it (audio to
    /// `Audio Files`, amp captures and impulses to `Models`), so the folder holds everything.
    /// Returns how many were copied.
    pub fn collect_media(&mut self) -> Result<usize, String> {
        let paths = self.paths.clone().ok_or(tr("save the project first"))?;
        let plan = plan(self.project(), &paths);
        let mut copied = 0;
        for (from, to) in &plan.files {
            let target = paths.dir().join(to);
            if *from != target && !target.exists() {
                copy(from, &target).map_err(|e| format!("{}: {e}", from.display()))?;
                copied += 1;
            }
        }
        if copied > 0 {
            self.history
                .edit(tr("Collect media"), |p| *p = plan.project.clone());
        }
        Ok(copied)
    }

    /// Packs the project with every file it uses into `target` (.tar.gz), in the background;
    /// the project itself is unchanged.
    pub fn archive(&mut self, target: PathBuf) -> Result<(), String> {
        if self.archiving.is_some() {
            return Err(tr("an archive is already being written").into());
        }
        let paths = self.paths.clone().ok_or(tr("save the project first"))?;
        self.capture_plugin_states();
        let plan = plan(self.project(), &paths);
        if let Some((missing, _)) = plan.files.iter().find(|(from, _)| !from.exists()) {
            return Err(trf!("missing file: {path}", path = missing.display()));
        }
        let bytes = file::to_json(&plan.project).map_err(|e| e.to_string())?;
        let root = paths.file.file_stem().map_or_else(
            || "Project".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        self.archiving = Some(std::thread::spawn(move || {
            pack(&target, &root, &bytes, &plan.files)
                .map(|()| target)
                .map_err(|e| e.to_string())
        }));
        Ok(())
    }

    pub fn archiving(&self) -> bool {
        self.archiving.is_some()
    }

    pub(super) fn poll_archive(&mut self) {
        if !self.archiving.as_ref().is_some_and(JoinHandle::is_finished) {
            return;
        }
        let message = match self.archiving.take().map(JoinHandle::join) {
            Some(Ok(Ok(path))) => trf!("Project archived to {path}", path = path.display()),
            Some(Ok(Err(e))) => trf!("Cannot archive the project: {error}", error = e),
            _ => tr("Cannot archive the project: the archive thread crashed").into(),
        };
        self.messages.push(message);
    }

    /// Versions of the project kept at each save, the newest first.
    pub fn versions(&self) -> Vec<(u32, PathBuf)> {
        self.paths.as_ref().map(file::versions).unwrap_or_default()
    }

    /// Brings back a saved version as one undoable step.
    pub fn restore_version(&mut self, path: &Path) -> Result<(), String> {
        let restored = file::load(path).map_err(|e| e.to_string())?;
        let label = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        self.history
            .edit(&trf!("Restore {version}", version = label), |p| {
                *p = restored
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audiostrada_core::routing::{self, Channel};
    use audiostrada_media::{AudioData, write_wav};

    use super::*;
    use crate::document::AppDirs;

    #[test]
    fn a_project_collects_and_archives_what_it_uses() {
        let dir = std::env::temp_dir().join(format!("audiostrada-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let outside = dir.join("elsewhere");
        std::fs::create_dir_all(outside.join("pack")).unwrap();
        let take = outside.join("take.wav");
        write_wav(&take, &AudioData::new(vec![vec![0.1; 4800]], 48_000)).unwrap();
        std::fs::write(outside.join("pack").join("a.nam"), b"capture").unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.save_as(ProjectPaths::in_new_folder(&dir, "Song"))
            .unwrap();
        // Imported after the save: the take stays outside, as a file dragged from elsewhere.
        doc.history_mut().edit("Setup", |p| {
            let s = p.add_source(audiostrada_core::Source {
                path: take.clone(),
                channels: 1,
                sample_rate: 48_000,
                frames: 4800,
            });
            let t = p.add_track("Take");
            p.add_clip(t, s, 0);
            let amp = routing::add_insert(p, Channel::Track(t), "amp_rig", Default::default());
            routing::set_insert_file(
                p,
                Channel::Track(t),
                amp.unwrap(),
                "amp",
                Some(outside.join("pack")),
            );
        });

        let archive = dir.join("Song.tar.gz");
        doc.archive(archive.clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while doc.archiving() && std::time::Instant::now() < deadline {
            doc.poll_archive();
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut names: Vec<String> = tar::Archive::new(flate2::read::GzDecoder::new(
            std::fs::File::open(&archive).unwrap(),
        ))
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with('/'))
        .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "Song/Audio Files/take.wav",
                "Song/Models/pack/a.nam",
                "Song/Song.strada"
            ]
        );
        assert!(
            doc.project().sources.values().all(|s| s.path == take),
            "unchanged"
        );

        assert_eq!(doc.collect_media().unwrap(), 2);
        let paths = doc.paths().unwrap().clone();
        assert!(paths.audio_dir().join("take.wav").exists());
        assert!(paths.models_dir().join("pack").join("a.nam").exists());
        assert!(
            doc.project()
                .sources
                .values()
                .all(|s| s.path == Path::new("Audio Files/take.wav"))
        );
        assert_eq!(doc.collect_media().unwrap(), 0, "nothing left outside");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn versions_are_restored_in_one_step() {
        let dir = std::env::temp_dir().join(format!("audiostrada-versions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = Document::new(AppDirs::default());
        doc.save_as(ProjectPaths::in_new_folder(&dir, "Song"))
            .unwrap();
        doc.history_mut().edit("Add", |p| {
            p.add_track("Bass");
        });
        doc.save().unwrap();
        doc.history_mut().edit("Add", |p| {
            p.add_track("Drums");
        });
        doc.save().unwrap();
        let versions = doc.versions();
        assert_eq!(versions.len(), 2);
        doc.restore_version(&versions[1].1).unwrap();
        assert!(
            doc.project().tracks.is_empty(),
            "the first save had no track"
        );
        doc.history_mut().undo();
        assert_eq!(doc.project().tracks.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
