//! Periodic autosave and recovery after a crash.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::file::{self, ProjectError, ProjectPaths};
use crate::history::History;

#[derive(Debug)]
pub struct Autosaver {
    interval: Duration,
    last_attempt: Instant,
    saved_revision: u64,
}

impl Autosaver {
    pub fn new(interval: Duration, revision: u64) -> Self {
        Self {
            interval,
            last_attempt: Instant::now(),
            saved_revision: revision,
        }
    }

    /// Writes `target` when the project changed since the last (auto)save and the interval
    /// elapsed. Returns the outcome when a write was attempted.
    pub fn tick(&mut self, history: &History, target: &Path) -> Option<Result<(), ProjectError>> {
        if history.revision() == self.saved_revision || self.last_attempt.elapsed() < self.interval
        {
            return None;
        }
        self.last_attempt = Instant::now();
        let result = file::to_json(history.project())
            .and_then(|bytes| Ok(file::write_atomic(target, &bytes)?));
        if result.is_ok() {
            self.saved_revision = history.revision();
        }
        Some(result)
    }

    /// Call after a manual save, so unchanged projects are not autosaved again.
    pub fn saved(&mut self, revision: u64) {
        self.saved_revision = revision;
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The autosave of a project, if it holds changes newer than the saved file.
pub fn recovery_for(paths: &ProjectPaths) -> Option<PathBuf> {
    let autosave = paths.autosave();
    let auto_time = modified(&autosave)?;
    match modified(&paths.file) {
        Some(file_time) if file_time >= auto_time => None,
        _ => Some(autosave),
    }
}

/// Removes the autosave of a project (after a save, or when the user declines recovery).
pub fn discard(path: &Path) {
    if let Err(e) = fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("cannot remove autosave {}: {e}", path.display());
    }
}

/// Autosaves of untitled projects left in `dir`, newest first.
pub fn untitled_recoveries(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| file::is_project(p))
        .filter_map(|p| Some((modified(&p)?, p)))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = crate::scratch::dir(&format!("autosave-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn autosaves_only_changed_projects_after_the_interval() {
        let dir = temp_dir("tick");
        let target = dir.join("a.strada.autosave");
        let mut history = History::new(Project::default());
        let mut saver = Autosaver::new(Duration::ZERO, history.revision());
        assert!(saver.tick(&history, &target).is_none(), "unchanged project");
        history.edit("Add", |p| {
            p.add_track("a");
        });
        assert!(matches!(saver.tick(&history, &target), Some(Ok(()))));
        assert!(saver.tick(&history, &target).is_none(), "already autosaved");
        assert_eq!(file::load(&target).unwrap().tracks.len(), 1);

        let mut slow = Autosaver::new(Duration::from_secs(3600), 0);
        assert!(
            slow.tick(&history, &target).is_none(),
            "interval not elapsed"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn recovery_requires_a_newer_autosave() {
        let dir = temp_dir("recovery");
        let paths = ProjectPaths::new(dir.join("Song.strada"));
        assert_eq!(recovery_for(&paths), None);
        file::save(&Project::default(), &paths).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(
            paths.autosave(),
            file::to_json(&Project::new("Newer", 48_000)).unwrap(),
        )
        .unwrap();
        assert_eq!(recovery_for(&paths), Some(paths.autosave()));
        std::thread::sleep(Duration::from_millis(20));
        file::save(&Project::default(), &paths).unwrap();
        assert_eq!(recovery_for(&paths), None, "saved after the autosave");
        discard(&paths.autosave());
        assert!(!paths.autosave().exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn untitled_recoveries_are_listed_newest_first() {
        let dir = temp_dir("untitled");
        fs::write(dir.join("old.strada"), b"{}").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(dir.join("new.strada"), b"{}").unwrap();
        fs::write(dir.join("ignored.txt"), b"").unwrap();
        let found = untitled_recoveries(&dir);
        assert_eq!(found, vec![dir.join("new.strada"), dir.join("old.strada")]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
