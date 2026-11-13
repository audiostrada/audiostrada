//! Project versions: the project file as it was at each save, to look back and restore one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, Align2, FontId, RichText, Sense, vec2};

use crate::dialog;
use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

fn open_id() -> egui::Id {
    egui::Id::unique("versions-window")
}

fn chosen_id() -> egui::Id {
    egui::Id::unique("versions-window-chosen")
}

fn summaries_id() -> egui::Id {
    egui::Id::unique("versions-window-summaries")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(open_id(), true);
        d.remove::<PathBuf>(chosen_id());
    });
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

/// What a version holds, read once per file: its date, tracks and length.
#[derive(Clone)]
struct Summary {
    modified: Option<SystemTime>,
    content: Option<String>,
}

fn summarize(path: &Path) -> Summary {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let content = audiostrada_core::file::load(path).ok().map(|project| {
        let seconds = project.length() / u64::from(project.sample_rate.max(1));
        format!(
            "{} · {}:{:02}",
            trf!("{count} tracks", count = project.tracks.len()),
            seconds / 60,
            seconds % 60
        )
    });
    Summary { modified, content }
}

/// The summaries of `versions`, read from the files not seen yet.
fn summaries(ctx: &egui::Context, versions: &[(u32, PathBuf)]) -> HashMap<PathBuf, Summary> {
    let mut known: HashMap<PathBuf, Summary> =
        ctx.data(|d| d.get_temp(summaries_id())).unwrap_or_default();
    known.retain(|path, _| versions.iter().any(|(_, p)| p == path));
    for (_, path) in versions {
        known.entry(path.clone()).or_insert_with(|| summarize(path));
    }
    ctx.data_mut(|d| d.insert_temp(summaries_id(), known.clone()));
    known
}

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    let saved = doc.paths().is_some();
    let versions = doc.versions();
    let known = summaries(ctx, &versions);
    let mut chosen = ctx
        .data(|d| d.get_temp::<PathBuf>(chosen_id()))
        .filter(|c| versions.iter().any(|(_, p)| p == c));
    let (mut restore, mut close) = (None, false);
    let detail = trf!(
        "{project}: the file as it was at each save",
        project = doc.project().name.as_str()
    );
    dialog::Dialog::new("versions", tr("Project versions"))
        .detail(detail)
        .width(540.0)
        .show(ctx, &mut open, |ui, part| match part {
            dialog::Part::Body => {
                if !saved {
                    dialog::section(ui, tr("Saved versions"), |ui| {
                        dialog::hint(ui, tr("Save the project once to start keeping versions."));
                    });
                    return;
                }
                dialog::section(ui, tr("Saved versions"), |ui| {
                    dialog::well(ui, |ui| {
                        if versions.is_empty() {
                            ui.label(tr(
                                "No earlier version yet: they appear from the second save.",
                            ));
                            return;
                        }
                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                for (number, path) in &versions {
                                    let summary = known.get(path);
                                    let picked = chosen.as_ref() == Some(path);
                                    let row = version_row(ui, *number, summary, picked);
                                    if row.clicked() {
                                        chosen = Some(path.clone());
                                    }
                                    if row.double_clicked() {
                                        restore = Some((*number, path.clone()));
                                    }
                                }
                            });
                    });
                    dialog::hint(
                        ui,
                        tr(
                            "Each save keeps the file it replaces (the last 30), in the project's \
                            Versions folder. Restoring is one step: Ctrl+Z brings back the \
                            project as it is now.",
                        ),
                    );
                });
            }
            dialog::Part::Actions => {
                let weak = theme::current(ui).weak;
                if saved {
                    let count = trf!("{count} versions kept", count = versions.len());
                    ui.label(RichText::new(count).size(11.5).color(weak));
                }
                dialog::buttons(ui, |ui| {
                    if !saved || versions.is_empty() {
                        close = dialog::secondary(ui, tr("Close")).clicked();
                        return;
                    }
                    let number = chosen
                        .as_ref()
                        .and_then(|c| versions.iter().find(|(_, p)| p == c))
                        .map(|(n, _)| *n);
                    let text = number.map_or_else(
                        || tr("Restore").to_owned(),
                        |n| trf!("Restore version {number}", number = n),
                    );
                    let button = dialog::primary(ui, &text, number.is_some());
                    let button = if number.is_some() {
                        button
                    } else {
                        button.on_hover_text(tr("Choose a version in the list first"))
                    };
                    if button.clicked()
                        && let (Some(n), Some(path)) = (number, chosen.clone())
                    {
                        restore = Some((n, path));
                    }
                    close = dialog::secondary(ui, tr("Close")).clicked();
                });
            }
        });
    if let Some((number, path)) = restore {
        match doc.restore_version(&path) {
            Ok(()) => doc.notify(trf!("Version {number} restored", number = number)),
            Err(e) => doc.notify(trf!("Cannot restore this version: {error}", error = e)),
        }
    }
    ctx.data_mut(|d| {
        d.insert_temp(open_id(), open && !close);
        match chosen {
            Some(c) => {
                d.insert_temp(chosen_id(), c);
            }
            None => d.remove::<PathBuf>(chosen_id()),
        }
    });
}

/// A version in the list: its number, how old it is and what it holds; lit when chosen.
fn version_row(
    ui: &mut egui::Ui,
    number: u32,
    summary: Option<&Summary>,
    picked: bool,
) -> egui::Response {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let painter = ui.painter();
    if picked {
        painter.rect_filled(rect, 4.0, theme::mix(p.surface, p.accent, 0.35));
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, p.surface);
    }
    let y = rect.center().y;
    painter.text(
        egui::pos2(rect.left() + 10.0, y),
        Align2::LEFT_CENTER,
        trf!("Version {number}", number = number),
        theme::strong(13.0),
        p.text,
    );
    let when = summary
        .and_then(|s| s.modified)
        .map_or_else(|| "?".to_owned(), age);
    painter.text(
        egui::pos2(rect.left() + 120.0, y),
        Align2::LEFT_CENTER,
        when,
        FontId::proportional(12.0),
        p.weak,
    );
    let content = summary.and_then(|s| s.content.clone()).unwrap_or_default();
    painter.text(
        egui::pos2(rect.right() - 10.0, y),
        Align2::RIGHT_CENTER,
        content,
        FontId::proportional(12.0),
        p.weak,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tr("Double-click to restore it"))
}
