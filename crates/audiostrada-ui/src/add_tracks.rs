//! Adding audio tracks: below the selection, beside a track, or several at once from a dialog.

use audiostrada_core::{Project, TrackId, routing};
use eframe::egui::{self, Color32, RichText};

use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::timeline::TRACK_COLORS;
use crate::trf;

/// Where new tracks go: beside a track (after it when true).
pub(crate) type Place = Option<(TrackId, bool)>;

/// Below the last selected track (in the order of the tracks), else at the end.
pub(crate) fn below_selection(doc: &Document) -> Place {
    let project = doc.project();
    doc.selected_tracks()
        .iter()
        .filter_map(|&id| project.tracks.iter().position(|t| t.id == id))
        .max()
        .map(|i| (project.tracks[i].id, true))
}

/// Moves tracks just added (at the end) to their place.
pub(crate) fn place(project: &mut Project, ids: &[TrackId], at: Place) {
    if let Some((target, after)) = at {
        routing::move_tracks(project, ids, target, after);
    }
}

/// Adds one audio track at `at` and selects it.
pub(crate) fn add_one(doc: &mut Document, stereo: bool, at: Place) {
    let n = doc.project().tracks.len() + 1;
    let mut added = None;
    doc.history_mut().edit(tr("Add track"), |p| {
        let id = p.add_track(&trf!("Audio {n}", n = n));
        if let Some(t) = p.track_mut(id) {
            t.stereo = stereo;
        }
        place(p, &[id], at);
        added = Some(id);
    });
    if let Some(id) = added {
        doc.set_track_selection(vec![id]);
    }
}

#[derive(Clone, Debug)]
struct Dialog {
    viewport: egui::ViewportId,
    count: u32,
    name: String,
    stereo: bool,
    /// One color for all, else each its own.
    color: Option<[u8; 3]>,
    at: Place,
    /// Where `at` is, else at the end.
    beside: bool,
    focused: bool,
}

fn id() -> egui::Id {
    egui::Id::unique("add-tracks-dialog")
}

fn last_id() -> egui::Id {
    egui::Id::unique("add-tracks-last")
}

/// Opens the dialog in the window being drawn, the new tracks going at `at` by default.
pub(crate) fn open(ctx: &egui::Context, at: Place) {
    let last = ctx.data(|d| d.get_temp::<Dialog>(last_id()));
    let dialog = Dialog {
        viewport: ctx.viewport_id(),
        at,
        beside: at.is_some(),
        focused: false,
        ..last.unwrap_or(Dialog {
            viewport: ctx.viewport_id(),
            count: 4,
            name: String::new(),
            stereo: false,
            color: None,
            at: None,
            beside: false,
            focused: false,
        })
    };
    ctx.data_mut(|d| d.insert_temp(id(), dialog));
}

/// The names of the new tracks: numbered when there are several.
fn names(dialog: &Dialog, existing: usize) -> Vec<String> {
    let base = dialog.name.trim();
    (0..dialog.count as usize)
        .map(|i| match (base.is_empty(), dialog.count) {
            (true, _) => trf!("Audio {n}", n = existing + i + 1),
            (false, 1) => base.to_owned(),
            (false, _) => format!("{base} {}", i + 1),
        })
        .collect()
}

fn add(doc: &mut Document, dialog: &Dialog) {
    let names = names(dialog, doc.project().tracks.len());
    let at = dialog.at.filter(|_| dialog.beside);
    let mut ids = Vec::new();
    let label = if names.len() == 1 {
        tr("Add track").to_owned()
    } else {
        trf!("Add {count} tracks", count = names.len())
    };
    doc.history_mut().edit(&label, |p| {
        for name in &names {
            let id = p.add_track(name);
            if let Some(t) = p.track_mut(id) {
                t.stereo = dialog.stereo;
                if let Some(color) = dialog.color {
                    t.color = color;
                }
            }
            ids.push(id);
        }
        place(p, &ids, at);
    });
    doc.set_track_selection(ids);
}

/// Shows the dialog when it was opened from the window being drawn.
pub(crate) fn dialog(ctx: &egui::Context, doc: &mut Document) {
    let Some(mut dialog) = ctx.data(|d| d.get_temp::<Dialog>(id())) else {
        return;
    };
    if dialog.viewport != ctx.viewport_id() {
        return;
    }
    let beside_name = dialog
        .at
        .and_then(|(t, _)| doc.project().track(t))
        .map(|t| t.name.clone());
    let mut done = false;
    // The Enter that chose the command in the palette must not add the tracks.
    let first = !dialog.focused;
    let modal = egui::Modal::new(egui::Id::unique("add-tracks-modal")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading(tr("Add audio tracks"));
        ui.add_space(8.0);
        egui::Grid::new("add-tracks-grid")
            .num_columns(2)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label(tr("Number"));
                let count = ui.add(
                    egui::DragValue::new(&mut dialog.count)
                        .range(1..=64)
                        .speed(0.1),
                );
                if !dialog.focused {
                    count.request_focus();
                    dialog.focused = true;
                }
                ui.end_row();

                ui.label(tr("Channels"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut dialog.stereo, false, tr("Mono"));
                    ui.selectable_value(&mut dialog.stereo, true, tr("Stereo"));
                });
                ui.end_row();

                ui.label(tr("Name"));
                ui.vertical(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .hint_text(tr("Audio"))
                            .desired_width(f32::INFINITY),
                    );
                    let names = names(&dialog, doc.project().tracks.len());
                    let preview = match names.as_slice() {
                        [one] => one.clone(),
                        [first, .., last] => format!("{first} … {last}"),
                        [] => String::new(),
                    };
                    ui.label(RichText::new(preview).weak().size(11.0));
                });
                ui.end_row();

                ui.label(tr("Color"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
                    if ui
                        .selectable_label(dialog.color.is_none(), tr("Varied"))
                        .on_hover_text(tr("Each track takes the next color"))
                        .clicked()
                    {
                        dialog.color = None;
                    }
                    for color in TRACK_COLORS {
                        let fill = Color32::from_rgb(color[0], color[1], color[2]);
                        let chosen = dialog.color == Some(color);
                        let swatch = egui::Button::new("")
                            .fill(fill)
                            .min_size(egui::vec2(15.0, 15.0))
                            .stroke(if chosen {
                                egui::Stroke::new(2.0, theme::current(ui).text)
                            } else {
                                egui::Stroke::NONE
                            });
                        if ui.add(swatch).clicked() {
                            dialog.color = Some(color);
                        }
                    }
                });
                ui.end_row();

                if let Some(name) = &beside_name {
                    let after = dialog.at.is_some_and(|(_, after)| after);
                    ui.label(tr("Place"));
                    ui.vertical(|ui| {
                        let label = if after {
                            trf!("Below {name}", name = name)
                        } else {
                            trf!("Above {name}", name = name)
                        };
                        ui.radio_value(&mut dialog.beside, true, label);
                        ui.radio_value(&mut dialog.beside, false, tr("After the last track"));
                    });
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        ui.separator();
        let row = egui::vec2(ui.available_width(), 26.0);
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let enter = !first && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if theme::primary_button(ui, tr("Add")).clicked() || enter {
                    add(doc, &dialog);
                    done = true;
                }
                if ui.button(tr("Cancel")).clicked() {
                    done = true;
                }
            },
        );
    });
    if done || modal.should_close() {
        ctx.data_mut(|d| {
            d.remove::<Dialog>(id());
            d.insert_temp(last_id(), dialog);
        });
    } else {
        ctx.data_mut(|d| d.insert_temp(id(), dialog));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;

    #[test]
    fn tracks_go_below_the_selection_or_beside_a_track() {
        let mut doc = Document::new(AppDirs::default());
        let mut ids = Vec::new();
        doc.history_mut().edit("Setup", |p| {
            ids = ["A", "B", "C"].map(|n| p.add_track(n)).to_vec();
        });
        let names = |doc: &Document| -> Vec<String> {
            doc.project()
                .tracks
                .iter()
                .map(|t| t.name.clone())
                .collect()
        };
        doc.set_track_selection(vec![ids[0]]);
        let at = below_selection(&doc);
        add_one(&mut doc, false, at);
        assert_eq!(names(&doc), ["A", "Audio 4", "B", "C"]);
        add_one(&mut doc, false, Some((ids[2], false)));
        assert_eq!(names(&doc), ["A", "Audio 4", "B", "Audio 5", "C"]);

        let dialog = Dialog {
            viewport: egui::ViewportId::ROOT,
            count: 3,
            name: "Gtr".into(),
            stereo: true,
            color: Some(TRACK_COLORS[2]),
            at: Some((ids[1], true)),
            beside: true,
            focused: true,
        };
        add(&mut doc, &dialog);
        assert_eq!(
            names(&doc),
            [
                "A", "Audio 4", "B", "Gtr 1", "Gtr 2", "Gtr 3", "Audio 5", "C"
            ]
        );
        let added = &doc.project().tracks[3..6];
        assert!(added.iter().all(|t| t.stereo && t.color == TRACK_COLORS[2]));
        assert_eq!(doc.selected_tracks().len(), 3);
    }
}
