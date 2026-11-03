//! Projects in tabs: one plays and is edited, the others wait in the background with the
//! state of their panels. Changing tabs moves the audio stream to the project shown.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use eframe::egui;

use crate::document::Document;
use crate::i18n::tr;
use crate::mixer::MixerView;
use crate::piano_roll::PianoRoll;
use crate::timeline::TimelineView;
use crate::{AudioStradaApp, Pending, RecoveryOffer};

/// A project open in a background tab, with its panels as they were left.
pub(crate) struct Parked {
    doc: Document,
    mixer: MixerView,
    timeline: TimelineView,
    piano: PianoRoll,
}

/// What was clicked in the tab bar.
enum TabClick {
    Show(usize),
    Close(usize),
    New,
}

impl AudioStradaApp {
    fn parked(&self, doc: Document) -> Parked {
        Parked {
            doc,
            mixer: MixerView::new(Rc::clone(&self.doc)),
            timeline: TimelineView::new(Rc::clone(&self.doc)),
            piano: PianoRoll::new(Rc::clone(&self.doc)),
        }
    }

    /// Reads the project of tab `i`.
    fn with_tab<R>(&self, i: usize, read: impl FnOnce(&Document) -> R) -> Option<R> {
        match self.tabs.get(i)? {
            Some(parked) => Some(read(&parked.doc)),
            None => Some(read(&self.doc.borrow())),
        }
    }

    /// Swaps the project and panels shown with those of `parked`.
    fn exchange(&mut self, parked: &mut Parked) {
        std::mem::swap(&mut *self.doc.borrow_mut(), &mut parked.doc);
        let mut views = self.views.borrow_mut();
        std::mem::swap(&mut views.mixer, &mut parked.mixer);
        std::mem::swap(&mut views.timeline, &mut parked.timeline);
        std::mem::swap(&mut views.piano, &mut parked.piano);
        // The clipboards stay with what is shown, to paste into another project.
        views.timeline.keep_clipboard(&mut parked.timeline);
        views.piano.keep_clipboard(&mut parked.piano);
        views.timeline.header_width = self.prefs.header_width;
    }

    /// Sends the current project to the background; its plugin windows open again on return.
    fn park_current(&mut self) {
        let windows = self.doc.borrow_mut().park();
        self.views.borrow_mut().mixer.park(windows);
    }

    /// False, with a message, while the current project cannot be left.
    fn can_leave(&self) -> bool {
        let mut doc = self.doc.borrow_mut();
        if doc.recording().is_some() {
            doc.notify(tr("Stop recording before changing tabs").to_owned());
            return false;
        }
        true
    }

    /// Shows tab `to`, the current project going to the background.
    pub(crate) fn switch_tab(&mut self, to: usize) {
        if to == self.current || to >= self.tabs.len() || !self.can_leave() {
            return;
        }
        self.host.stop();
        self.park_current();
        let Some(mut parked) = self.tabs[to].take() else {
            return;
        };
        self.exchange(&mut parked);
        self.tabs[self.current] = Some(parked);
        self.current = to;
        self.restart();
    }

    /// The next or previous tab, around the ends.
    pub(crate) fn cycle_tab(&mut self, step: isize) {
        let n = self.tabs.len() as isize;
        self.switch_tab((self.current as isize + step).rem_euclid(n) as usize);
    }

    /// Shows `doc` in a new tab after the current one, or in place of a blank project.
    pub(crate) fn add_tab(&mut self, doc: Document) {
        if self.doc.borrow().is_blank() {
            self.doc.borrow_mut().discard_autosave();
            self.replace(doc);
            return;
        }
        if !self.can_leave() {
            return;
        }
        let mut doc = doc;
        doc.set_plugins(self.doc.borrow().player().plugins().clone());
        let mut parked = self.parked(doc);
        self.host.stop();
        self.park_current();
        self.exchange(&mut parked);
        self.tabs[self.current] = Some(parked);
        self.current += 1;
        self.tabs.insert(self.current, None);
        self.restart();
    }

    /// Closes the current project and shows tab `to` (an index before the removal).
    fn leave_tab(&mut self, to: usize) {
        self.host.stop();
        self.park_current();
        let Some(mut parked) = self.tabs.get_mut(to).and_then(Option::take) else {
            return;
        };
        self.exchange(&mut parked);
        drop(parked);
        self.tabs.remove(self.current);
        self.current = if to > self.current { to - 1 } else { to };
        self.restart();
    }

    /// Closes the current tab once its changes were saved or dropped.
    pub(crate) fn close_current_tab(&mut self) {
        self.doc.borrow_mut().discard_autosave();
        if self.tabs.len() == 1 {
            self.replace(Document::new(self.dirs.clone()));
            return;
        }
        let to = if self.current + 1 < self.tabs.len() {
            self.current + 1
        } else {
            self.current - 1
        };
        self.leave_tab(to);
    }

    /// Closes tab `i`, asking first when its project has unsaved changes.
    pub(crate) fn close_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i == self.current {
            if self.can_leave() {
                self.request(ctx, Pending::CloseTab);
            }
            return;
        }
        let clean = matches!(&self.tabs.get(i), Some(Some(p)) if !p.doc.history().is_dirty());
        if clean {
            if let Some(Some(mut parked)) = (i < self.tabs.len()).then(|| self.tabs.remove(i)) {
                parked.doc.discard_autosave();
            }
            if i < self.current {
                self.current -= 1;
            }
        } else {
            self.switch_tab(i);
            if self.current == i {
                self.request(ctx, Pending::CloseTab);
            }
        }
    }

    /// Tab whose project is saved in `file`.
    pub(crate) fn tab_of(&self, file: &Path) -> Option<usize> {
        (0..self.tabs.len()).find(|&i| {
            self.with_tab(i, |d| d.paths().is_some_and(|p| p.file == file))
                .unwrap_or(false)
        })
    }

    pub(crate) fn any_unsaved(&self) -> bool {
        (0..self.tabs.len()).any(|i| self.with_tab(i, |d| d.history().is_dirty()) == Some(true))
    }

    /// While quitting, the current project being dealt with: closes the background projects
    /// without changes, then shows the next one with changes. Returns false when none is left.
    pub(crate) fn quit_next(&mut self) -> bool {
        self.doc.borrow_mut().discard_autosave();
        let mut i = 0;
        while i < self.tabs.len() {
            if let Some(parked) = &mut self.tabs[i]
                && !parked.doc.history().is_dirty()
            {
                parked.doc.discard_autosave();
                self.tabs.remove(i);
                if i < self.current {
                    self.current -= 1;
                }
            } else {
                i += 1;
            }
        }
        match self.tabs.iter().position(Option::is_some) {
            Some(next) => {
                self.leave_tab(next);
                true
            }
            None => false,
        }
    }

    /// Notes the projects open in tabs, to reopen them at the next start.
    pub(crate) fn remember_tabs(&mut self) {
        let mut files = Vec::new();
        let mut shown = 0;
        for i in 0..self.tabs.len() {
            let file = self
                .with_tab(i, |d| d.paths().map(|p| p.file.clone()))
                .flatten();
            if let Some(file) = file {
                if i == self.current {
                    shown = files.len();
                }
                files.push(file);
            }
        }
        self.prefs.tabs = files;
        self.prefs.tab = shown;
    }

    /// Reopens in the background the projects that were in tabs at the last exit; the one that
    /// was shown takes the place of a blank project.
    pub(crate) fn restore_tabs(&mut self) {
        let shown_file = self.doc.borrow().paths().map(|p| p.file.clone());
        let plugins = self.doc.borrow().player().plugins().clone();
        let files: Vec<PathBuf> = self.prefs.tabs.clone();
        let mut shown = None;
        for (i, file) in files.iter().enumerate() {
            if shown_file.as_ref() == Some(file)
                || !file.exists()
                || RecoveryOffer::find(Some(file), &self.dirs).is_some()
            {
                continue;
            }
            match Document::open(file, self.dirs.clone()) {
                Ok(mut doc) => {
                    doc.set_plugins(plugins.clone());
                    let parked = self.parked(doc);
                    self.tabs.push(Some(parked));
                    if i == self.prefs.tab || shown.is_none() {
                        shown = Some(self.tabs.len() - 1);
                    }
                }
                Err(e) => tracing::warn!("cannot reopen {}: {e}", file.display()),
            }
        }
        if let Some(shown) = shown
            && self.recovery.is_none()
            && self.doc.borrow().is_blank()
        {
            self.doc.borrow_mut().discard_autosave();
            self.leave_tab(shown);
        }
    }

    /// The tabs, shown when more than one project is open.
    pub(crate) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut clicked = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for i in 0..self.tabs.len() {
                let Some(title) = self.with_tab(i, Document::title) else {
                    continue;
                };
                let response = ui.selectable_label(i == self.current, &title);
                if response.clicked() {
                    clicked = Some(TabClick::Show(i));
                }
                if response.middle_clicked() {
                    clicked = Some(TabClick::Close(i));
                }
                response.context_menu(|ui| {
                    if ui.button(tr("Close tab")).clicked() {
                        clicked = Some(TabClick::Close(i));
                        ui.close();
                    }
                });
                if ui
                    .small_button("×")
                    .on_hover_text(tr("Close tab"))
                    .clicked()
                {
                    clicked = Some(TabClick::Close(i));
                }
                ui.add_space(8.0);
            }
            if ui
                .small_button("+")
                .on_hover_text(tr("New project tab"))
                .clicked()
            {
                clicked = Some(TabClick::New);
            }
        });
        match clicked {
            Some(TabClick::Show(i)) => self.switch_tab(i),
            Some(TabClick::Close(i)) => self.close_tab(ui.ctx(), i),
            Some(TabClick::New) => self.add_tab(Document::new(self.dirs.clone())),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use audiostrada_core::{ClipId, Project, ProjectPaths, file};

    use super::*;
    use crate::document::AppDirs;
    use crate::{AudioHost, Hooks, ui_prefs::UiPrefs};

    fn app() -> AudioStradaApp {
        let doc = Rc::new(RefCell::new(Document::new(AppDirs::default())));
        let hooks = Hooks {
            audio_changed: Box::new(|_| {}),
            recent_changed: Box::new(|_| {}),
            ui_changed: Box::new(|_| {}),
            plugin_scan: Box::new(|_, _| std::thread::spawn(Default::default)),
        };
        AudioStradaApp::new(
            AudioHost::new(Box::new(|_| unreachable!())),
            doc,
            AppDirs::default(),
            UiPrefs::default(),
            Vec::new(),
            None,
            hooks,
        )
    }

    /// A new project with one track, with unsaved changes.
    fn edited(track: &str) -> Document {
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().edit("Add", |p| {
            p.add_track(track);
        });
        doc
    }

    /// A project saved in `dir`, opened without changes.
    fn saved(dir: &Path, name: &str) -> (PathBuf, Document) {
        let paths = ProjectPaths::in_new_folder(dir, name);
        std::fs::create_dir_all(paths.dir()).unwrap();
        let mut project = Project::new(name, 48_000);
        project.add_track(name);
        file::save(&project, &paths).unwrap();
        let doc = Document::open(&paths.file, AppDirs::default()).unwrap();
        (paths.file, doc)
    }

    fn first_track(app: &AudioStradaApp) -> String {
        app.doc.borrow().project().tracks[0].name.clone()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("audiostrada-tabs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn each_tab_keeps_its_project_and_panels() {
        let mut app = app();
        app.add_tab(edited("Blank replaced"));
        assert_eq!(app.tabs.len(), 1, "a blank project gives its place");
        app.add_tab(edited("B"));
        assert_eq!((app.tabs.len(), app.current), (2, 1));
        assert_eq!(first_track(&app), "B");
        app.views.borrow_mut().timeline.selection = vec![ClipId(7)];
        app.switch_tab(0);
        assert_eq!(first_track(&app), "Blank replaced");
        assert!(app.views.borrow().timeline.selection.is_empty());
        app.cycle_tab(1);
        assert_eq!(first_track(&app), "B");
        assert_eq!(app.views.borrow().timeline.selection, [ClipId(7)]);
        app.cycle_tab(1);
        assert_eq!(app.current, 0, "around the ends");
    }

    #[test]
    fn closing_asks_only_for_unsaved_changes() {
        let dir = scratch("close");
        let ctx = egui::Context::default();
        let mut app = app();
        app.add_tab(edited("A"));
        let (file, clean) = saved(&dir, "Clean");
        app.add_tab(clean);
        app.add_tab(edited("C"));
        assert_eq!(app.tab_of(&file), Some(1));
        app.close_tab(&ctx, 1);
        assert_eq!(
            (app.tabs.len(), app.current),
            (2, 1),
            "closed in the background"
        );
        assert_eq!(app.tab_of(&file), None);
        app.close_tab(&ctx, 1);
        assert!(matches!(app.pending, Some(Pending::CloseTab)));
        app.pending = None;
        app.perform(&ctx, Pending::CloseTab);
        assert_eq!((app.tabs.len(), app.current), (1, 0));
        assert_eq!(first_track(&app), "A");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quitting_goes_through_the_projects_with_changes() {
        let dir = scratch("quit");
        let mut app = app();
        app.add_tab(edited("A"));
        app.add_tab(saved(&dir, "Clean").1);
        app.add_tab(edited("C"));
        assert!(app.any_unsaved());
        // C was dealt with: the clean project closes, A asks next.
        assert!(app.quit_next());
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(first_track(&app), "A");
        assert!(!app.quit_next());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tabs_of_the_last_exit_come_back() {
        let dir = scratch("restore");
        let mut app = app();
        let (one, doc) = saved(&dir, "One");
        app.add_tab(doc);
        let (two, doc) = saved(&dir, "Two");
        app.add_tab(doc);
        app.add_tab(edited("Untitled"));
        app.switch_tab(1);
        app.remember_tabs();
        assert_eq!(app.prefs.tabs, [one.clone(), two.clone()]);
        assert_eq!(app.prefs.tab, 1);

        let mut next = self::app();
        next.prefs.tabs = app.prefs.tabs.clone();
        next.prefs.tab = 1;
        next.restore_tabs();
        assert_eq!(next.tabs.len(), 2);
        assert_eq!(first_track(&next), "Two");
        assert_eq!(next.tab_of(&one), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
