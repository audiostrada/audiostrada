//! Project versions: the project file as it was at each save, to look back and restore one.

use std::time::SystemTime;

use eframe::egui::{self, Align2, RichText, vec2};

use crate::document::Document;
use crate::i18n::tr;
use crate::trf;

fn open_id() -> egui::Id {
    egui::Id::unique("versions-window")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(open_id(), true));
}

/// "just now", "12 min ago", "3 h ago", "yesterday", "5 days ago".
fn age(modified: SystemTime) -> String {
    let seconds = SystemTime::now()
        .duration_since(modified)
        .map_or(0, |d| d.as_secs());
    match seconds {
        0..60 => tr("just now").into(),
        60..3600 => trf!("{minutes} min ago", minutes = seconds / 60),
        3600..86_400 => trf!("{hours} h ago", hours = seconds / 3600),
        86_400..172_800 => tr("yesterday").into(),
        _ => trf!("{days} days ago", days = seconds / 86_400),
    }
}

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    let versions = doc.versions();
    let mut restore = None;
    egui::Window::new(tr("Project versions"))
        .id(egui::Id::unique("versions-window-frame"))
        .open(&mut open)
        .collapsible(false)
        .default_width(380.0)
        .pivot(Align2::CENTER_TOP)
        .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
        .show(ctx, |ui| {
            ui.weak(tr(
                "Each save keeps the file it replaces (the last 30). Restoring one is a \
                 single step: Ctrl+Z comes back.",
            ));
            ui.separator();
            if doc.paths().is_none() {
                ui.label(tr("Save the project once to start keeping versions."));
                return;
            }
            if versions.is_empty() {
                ui.label(tr(
                    "No earlier version yet: they appear from the second save.",
                ));
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for (number, path) in &versions {
                        ui.horizontal(|ui| {
                            let when = std::fs::metadata(path)
                                .and_then(|m| m.modified())
                                .map_or_else(|_| "?".to_owned(), age);
                            ui.label(
                                RichText::new(trf!("Version {number}", number = number)).strong(),
                            );
                            ui.weak(when);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button(tr("Restore")).clicked() {
                                        restore = Some(path.clone());
                                    }
                                },
                            );
                        });
                    }
                });
        });
    if let Some(path) = restore
        && let Err(e) = doc.restore_version(&path)
    {
        doc.notify(trf!("Cannot restore this version: {error}", error = e));
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}
