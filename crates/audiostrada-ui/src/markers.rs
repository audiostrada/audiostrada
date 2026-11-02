//! Markers list: jump, rename and delete.

use audiostrada_core::edit;
use eframe::egui::{self, RichText, Ui};

use crate::document::Document;
use crate::i18n::tr;
use crate::mixer::format_time;

pub fn show(ui: &mut Ui, doc: &mut Document) {
    if ui.button(tr("Add marker at playhead")).clicked() {
        let position = doc.player().position();
        doc.history_mut().edit(tr("Add marker"), |p| {
            edit::add_marker(p, position);
        });
    }
    ui.separator();
    let project = doc.project().clone();
    let (rate, tempo) = (project.sample_rate, &project.tempo);
    if project.markers.is_empty() {
        ui.weak(tr(
            "No markers. Add one at the playhead with the button above or its shortcut.",
        ));
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("markers")
                .num_columns(4)
                .striped(true)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for marker in &project.markers {
                        let id = marker.id;
                        let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(marker.position, rate));
                        if ui
                            .button(RichText::new(format!("{}.{}", bbt.bar, bbt.beat)).monospace())
                            .on_hover_text(format_time(marker.position, rate))
                            .clicked()
                        {
                            doc.player_mut().locate(marker.position);
                        }
                        let mut name = marker.name.clone();
                        let response =
                            ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
                        if response.changed() {
                            doc.history_mut().edit_merged(
                                tr("Rename marker"),
                                &format!("marker:{}", id.0),
                                |p| {
                                    if let Some(m) = p.markers.iter_mut().find(|m| m.id == id) {
                                        m.name = name;
                                    }
                                },
                            );
                        }
                        if response.lost_focus() {
                            doc.history_mut().end_gesture();
                        }
                        ui.weak(format_time(marker.position, rate));
                        if ui
                            .small_button("✕")
                            .on_hover_text(tr("Delete marker"))
                            .clicked()
                        {
                            doc.history_mut()
                                .edit(tr("Delete marker"), |p| edit::remove_marker(p, id));
                        }
                        ui.end_row();
                    }
                });
        });
}
