//! Undo/redo by project snapshots. Snapshots share unchanged tracks, so they stay cheap.

use crate::model::Project;

const DEFAULT_LIMIT: usize = 200;

#[derive(Debug, Clone)]
struct Entry {
    label: String,
    /// State before the edit (undo) or after it (redo).
    project: Project,
    /// Consecutive edits with the same key merge into one step (e.g. dragging a fader).
    merge_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct History {
    current: Project,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    revision: u64,
    saved_revision: u64,
    /// Merging is only allowed while nothing else happened since the previous edit.
    can_merge: bool,
    limit: usize,
}

impl History {
    pub fn new(project: Project) -> Self {
        Self {
            current: project,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            saved_revision: 0,
            can_merge: false,
            limit: DEFAULT_LIMIT,
        }
    }

    pub fn project(&self) -> &Project {
        &self.current
    }

    /// Incremented by every change, including undo and redo.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Applies an undoable edit; returns false (and records nothing) if it changed nothing.
    pub fn edit(&mut self, label: &str, f: impl FnOnce(&mut Project)) -> bool {
        self.apply(label, None, f)
    }

    /// Like [`edit`](Self::edit), merging with the previous edit if it used the same key.
    pub fn edit_merged(&mut self, label: &str, key: &str, f: impl FnOnce(&mut Project)) -> bool {
        self.apply(label, Some(key), f)
    }

    fn apply(&mut self, label: &str, key: Option<&str>, f: impl FnOnce(&mut Project)) -> bool {
        let before = self.current.clone();
        f(&mut self.current);
        if self.current == before {
            return false;
        }
        let merge = self.can_merge
            && key.is_some()
            && self.undo.last().and_then(|e| e.merge_key.as_deref()) == key;
        if !merge {
            self.undo.push(Entry {
                label: label.to_owned(),
                project: before,
                merge_key: key.map(str::to_owned),
            });
            if self.undo.len() > self.limit {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.can_merge = key.is_some();
        self.revision += 1;
        true
    }

    /// Ends a merged gesture, so the next edit with the same key starts a new step.
    pub fn end_gesture(&mut self) {
        self.can_merge = false;
    }

    pub fn undo(&mut self) -> Option<String> {
        let entry = self.undo.pop()?;
        let after = std::mem::replace(&mut self.current, entry.project);
        self.redo.push(Entry {
            label: entry.label.clone(),
            project: after,
            merge_key: None,
        });
        self.can_merge = false;
        self.revision += 1;
        Some(entry.label)
    }

    /// Takes back the last edit and forgets it, without a redo step (a preview withdrawn).
    pub fn withdraw(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        self.current = entry.project;
        self.can_merge = false;
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> Option<String> {
        let entry = self.redo.pop()?;
        let before = std::mem::replace(&mut self.current, entry.project);
        self.undo.push(Entry {
            label: entry.label.clone(),
            project: before,
            merge_key: None,
        });
        self.can_merge = false;
        self.revision += 1;
        Some(entry.label)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// True if the project changed since it was last saved (or loaded).
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Replaces the project without recording an undo step (loading, recovery).
    pub fn replace(&mut self, project: Project, dirty: bool) {
        self.current = project;
        self.undo.clear();
        self.redo.clear();
        self.can_merge = false;
        self.revision += 1;
        if !dirty {
            self.saved_revision = self.revision;
        }
    }

    /// Changes the project outside of undo (e.g. updating media paths after a save).
    pub fn modify_unrecorded(&mut self, f: impl FnOnce(&mut Project)) {
        f(&mut self.current);
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_withdrawn_edit_leaves_no_redo() {
        let mut h = History::new(Project::default());
        h.edit("Add track", |p| {
            p.add_track("a");
        });
        h.edit("Preview", |p| {
            p.add_track("b");
        });
        assert!(h.withdraw());
        assert_eq!(h.project().tracks.len(), 1);
        assert_eq!(h.redo_label(), None);
        assert_eq!(h.undo_label(), Some("Add track"));
    }

    #[test]
    fn undo_and_redo_restore_states() {
        let mut h = History::new(Project::default());
        assert!(h.edit("Add track", |p| {
            p.add_track("a");
        }));
        let t = h.project().tracks[0].id;
        assert!(h.edit("Mute", |p| p.track_mut(t).unwrap().mute = true));
        assert_eq!(h.undo_label(), Some("Mute"));
        assert_eq!(h.undo().as_deref(), Some("Mute"));
        assert!(!h.project().track(t).unwrap().mute);
        assert_eq!(h.redo().as_deref(), Some("Mute"));
        assert!(h.project().track(t).unwrap().mute);
        assert_eq!(h.undo().as_deref(), Some("Mute"));
        assert_eq!(h.undo().as_deref(), Some("Add track"));
        assert!(h.project().tracks.is_empty());
        assert_eq!(h.undo(), None);
    }

    #[test]
    fn the_oldest_steps_are_dropped_beyond_the_limit() {
        let mut h = History::new(Project::default());
        h.limit = 2;
        for name in ["a", "b", "c"] {
            h.edit(name, |p| {
                p.add_track(name);
            });
        }
        assert_eq!(h.undo().as_deref(), Some("c"));
        assert_eq!(h.undo().as_deref(), Some("b"));
        assert_eq!(h.undo(), None);
        assert_eq!(h.project().tracks.len(), 1);
        let revision = h.revision();
        h.modify_unrecorded(|p| p.name = "Song".into());
        assert!(h.revision() > revision && h.is_dirty());
        assert_eq!(h.undo_label(), None, "not undoable");
    }

    #[test]
    fn no_op_edits_are_not_recorded() {
        let mut h = History::new(Project::default());
        assert!(!h.edit("Nothing", |_| {}));
        assert_eq!(h.undo_label(), None);
        assert!(!h.is_dirty());
    }

    #[test]
    fn gestures_merge_into_one_step() {
        let mut h = History::new(Project::default());
        h.edit("Add track", |p| {
            p.add_track("a");
        });
        let t = h.project().tracks[0].id;
        for db in [-1.0, -2.0, -3.0] {
            h.edit_merged("Gain", "gain:1", |p| p.track_mut(t).unwrap().gain_db = db);
        }
        h.end_gesture();
        h.edit_merged("Gain", "gain:1", |p| p.track_mut(t).unwrap().gain_db = -4.0);
        h.undo();
        assert_eq!(h.project().track(t).unwrap().gain_db, -3.0);
        h.undo();
        assert_eq!(h.project().track(t).unwrap().gain_db, 0.0);
        assert_eq!(h.undo_label(), Some("Add track"));
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = History::new(Project::default());
        h.edit("a", |p| {
            p.add_track("a");
        });
        h.undo();
        h.edit("b", |p| {
            p.add_track("b");
        });
        assert_eq!(h.redo_label(), None);
    }

    #[test]
    fn dirty_state_follows_saves_and_undo() {
        let mut h = History::new(Project::default());
        h.edit("a", |p| {
            p.add_track("a");
        });
        assert!(h.is_dirty());
        h.mark_saved();
        assert!(!h.is_dirty());
        h.undo();
        assert!(h.is_dirty());
        h.replace(Project::default(), false);
        assert!(!h.is_dirty());
        assert_eq!(h.undo_label(), None);
    }
}
