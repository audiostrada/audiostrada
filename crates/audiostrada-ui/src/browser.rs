//! File browser: navigate folders, drag audio files onto the timeline or double-click them.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, Ui};

use crate::dialogs::AUDIO_EXTENSIONS;
use crate::document::Document;
use crate::i18n::tr;
use crate::theme::{self, Icon};

/// Drag-and-drop payload of a file dragged from the browser.
#[derive(Clone, Debug)]
pub struct DraggedFile(pub PathBuf);

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    name: String,
    folder: bool,
}

#[derive(Debug)]
pub struct Browser {
    dir: PathBuf,
    entries: Vec<Entry>,
    listed: Option<PathBuf>,
    selected: Option<PathBuf>,
    filter: String,
    error: Option<String>,
    /// Plays a file as soon as it is selected.
    auto_play: bool,
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

impl Browser {
    pub fn new() -> Self {
        Self {
            dir: dirs::audio_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("/")),
            entries: Vec::new(),
            listed: None,
            selected: None,
            filter: String::new(),
            error: None,
            auto_play: false,
        }
    }

    fn enter(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.selected = None;
        self.filter.clear();
    }

    fn list(&mut self) {
        self.listed = Some(self.dir.clone());
        self.entries.clear();
        self.error = None;
        let read = match std::fs::read_dir(&self.dir) {
            Ok(read) => read,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let folder = path.is_dir();
            if folder || is_audio(&path) {
                self.entries.push(Entry { path, name, folder });
            }
        }
        self.entries
            .sort_by_key(|e| (!e.folder, e.name.to_lowercase()));
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document) {
        if self.listed.as_ref() != Some(&self.dir) {
            self.list();
        }
        ui.horizontal(|ui| {
            let parent = self.dir.parent().map(Path::to_path_buf);
            if ui
                .add_enabled(parent.is_some(), egui::Button::new("⬆"))
                .on_hover_text(tr("Parent folder"))
                .clicked()
                && let Some(parent) = parent
            {
                self.enter(parent);
            }
            if ui.button("🏠").on_hover_text(tr("Home folder")).clicked()
                && let Some(home) = dirs::home_dir()
            {
                self.enter(home);
            }
            if ui.button("⟳").on_hover_text(tr("Refresh")).clicked() {
                self.listed = None;
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(tr("Filter files"))
                    .desired_width(f32::INFINITY),
            );
        });
        ui.label(RichText::new(self.dir.display().to_string()).weak().small());
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().warn_fg_color, error);
        }
        ui.separator();
        let mut enter = None;
        let filter = self.filter.to_lowercase();
        let footer = 28.0;
        egui::ScrollArea::vertical()
            .max_height((ui.available_height() - footer).max(40.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &self.entries {
                    if !filter.is_empty() && !entry.name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    if entry.folder {
                        if ui
                            .selectable_label(false, format!("📁 {}", entry.name))
                            .clicked()
                        {
                            enter = Some(entry.path.clone());
                        }
                        continue;
                    }
                    let selected = self.selected.as_ref() == Some(&entry.path);
                    let playing = doc.previewing() == Some(entry.path.as_path());
                    let id = egui::Id::unique(("browser-file", &entry.path));
                    let label = ui
                        .horizontal(|ui| {
                            let icon = if playing { Icon::Stop } else { Icon::Play };
                            let (rect, play) = ui
                                .allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
                            let color = if playing {
                                theme::PLAY
                            } else {
                                theme::current(ui).weak
                            };
                            theme::paint_icon(ui.painter(), rect.shrink(4.0), icon, color);
                            if play.on_hover_text(tr("Listen")).clicked() {
                                doc.preview((!playing).then_some(entry.path.as_path()));
                            }
                            ui.dnd_drag_source(id, DraggedFile(entry.path.clone()), |ui| {
                                ui.selectable_label(selected, &entry.name)
                            })
                            .inner
                        })
                        .inner;
                    if label.clicked() {
                        self.selected = Some(entry.path.clone());
                        if self.auto_play {
                            doc.preview(Some(&entry.path));
                        }
                    }
                    if label.double_clicked() {
                        doc.import(std::slice::from_ref(&entry.path));
                    }
                    label.on_hover_text(tr(
                        "Drag onto the timeline, or double-click to add a track",
                    ));
                }
                if self.entries.is_empty() && self.error.is_none() {
                    ui.weak(tr("No audio files here"));
                }
            });
        if let Some(dir) = enter {
            self.enter(dir);
        }
        ui.horizontal(|ui| {
            let selected = self.selected.clone();
            if ui
                .add_enabled(selected.is_some(), egui::Button::new(tr("Import")))
                .clicked()
                && let Some(path) = selected
            {
                doc.import(&[path]);
            }
            ui.checkbox(&mut self.auto_play, tr("Play on select"));
            if doc.previewing().is_some() && ui.button(tr("Stop")).clicked() {
                doc.preview(None);
            }
        });
    }
}
