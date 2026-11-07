//! Headphone mixes: a column per mix with its outputs, its volume, "more me", the click and a
//! level per track; and the control room's Dim and Mono.

use audiostrada_core::model::MIN_GAIN_DB;
use audiostrada_core::{CueMix, Project, TrackId, TrackKind};
use eframe::egui::{self, Align2, RichText, Ui, vec2};

use crate::document::Document;
use crate::i18n::tr;
use crate::trf;

fn open_id() -> egui::Id {
    egui::Id::unique("cue-window")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(open_id(), true));
}

/// "Out 3-4" for the pair starting at `first` (from 0).
pub fn pair_label(first: u16) -> String {
    trf!(
        "Out {first}-{second}",
        first = first + 1,
        second = first + 2
    )
}

/// The first output pair after the main one that no mix uses, if the interface has one.
pub fn free_pair(project: &Project, outputs: usize) -> Option<u16> {
    (2..outputs.saturating_sub(1) as u16)
        .step_by(2)
        .find(|o| project.cues.iter().all(|c| c.output != *o))
}

/// A new mix on the next free pair, starting from the main mix's levels.
pub fn add_cue(doc: &mut Document) {
    let Some(output) = free_pair(doc.project(), doc.stream_outputs()) else {
        return;
    };
    let name = trf!("Headphones {n}", n = doc.project().cues.len() + 1);
    doc.history_mut().edit(tr("Add a headphone mix"), |p| {
        let mut cue = CueMix {
            name,
            output,
            ..CueMix::default()
        };
        copy_main(p, &mut cue);
        p.cues.push(cue);
    });
}

/// The track levels of the main mix, muted tracks left out.
fn copy_main(project: &Project, cue: &mut CueMix) {
    cue.levels = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .map(|t| (t.id, if t.mute { MIN_GAIN_DB } else { t.gain_db }))
        .collect();
}

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    egui::Window::new(tr("Headphone mixes"))
        .id(egui::Id::unique("cue-window-frame"))
        .open(&mut open)
        .collapsible(false)
        .default_size(vec2(760.0, 520.0))
        .pivot(Align2::CENTER_TOP)
        .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
        .show(ctx, |ui| contents(ui, doc));
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The window's contents, also shown in the recording mode's panel.
pub fn contents(ui: &mut Ui, doc: &mut Document) {
    ui.horizontal(|ui| {
        let (mut dim, mut mono) = doc.player().control_room();
        ui.label(RichText::new(tr("Control room")).strong());
        let a = ui.toggle_value(&mut dim, tr("Dim")).on_hover_text(tr(
            "Lowers the main output by 20 dB; the headphones stay as they are",
        ));
        let b = ui
            .toggle_value(&mut mono, tr("Mono"))
            .on_hover_text(tr("Both speakers play the sum of left and right"));
        if a.changed() || b.changed() {
            doc.player_mut().set_control_room(dim, mono);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let free = free_pair(doc.project(), doc.stream_outputs()).is_some();
            let add = ui
                .add_enabled(free, egui::Button::new(tr("Add a headphone mix")))
                .on_disabled_hover_text(tr(
                    "Each mix needs a free pair of outputs on the audio interface; the first \
                     pair is the main output",
                ));
            if add.clicked() {
                add_cue(doc);
            }
        });
    });
    ui.separator();
    if doc.project().cues.is_empty() {
        ui.label(
            RichText::new(tr(
                "No headphone mix: the musicians hear the main mix. Add one for each pair of \
                 outputs that feeds headphones.",
            ))
            .weak(),
        );
        return;
    }
    egui::ScrollArea::both().show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for index in 0..doc.project().cues.len() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(200.0);
                    ui.vertical(|ui| column(ui, doc, index));
                });
            }
        });
    });
}

fn column(ui: &mut Ui, doc: &mut Document, index: usize) {
    let Some(mut cue) = doc.project().cues.get(index).cloned() else {
        return;
    };
    let before = cue.clone();
    let tracks: Vec<(TrackId, String)> = doc
        .project()
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .map(|t| (t.id, t.name.clone()))
        .collect();
    let mut remove = false;
    let mut copy = false;
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut cue.name).desired_width(140.0));
        if ui
            .small_button("✕")
            .on_hover_text(tr("Remove this mix"))
            .clicked()
        {
            remove = true;
        }
    });
    let outputs = doc.stream_outputs().max(4) as u16;
    egui::ComboBox::from_id_salt(("cue-output", index))
        .selected_text(pair_label(cue.output))
        .show_ui(ui, |ui| {
            for first in (2..outputs.saturating_sub(1)).step_by(2) {
                ui.selectable_value(&mut cue.output, first, pair_label(first));
            }
        });
    ui.checkbox(&mut cue.follow_main, tr("Plays the main mix"));
    ui.horizontal(|ui| {
        ui.label(tr("Volume"));
        ui.add(egui::Slider::new(&mut cue.gain_db, -40.0..=12.0).suffix(" dB"));
    });
    ui.separator();
    // "More me" first: what a musician asks for most.
    ui.label(RichText::new(tr("More me")).strong());
    let more = cue
        .more_me
        .and_then(|id| tracks.iter().find(|t| t.0 == id))
        .map_or_else(|| tr("None").to_owned(), |t| t.1.clone());
    egui::ComboBox::from_id_salt(("cue-more-me", index))
        .selected_text(more)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut cue.more_me, None, tr("None"));
            for (id, name) in &tracks {
                ui.selectable_value(&mut cue.more_me, Some(*id), name);
            }
        });
    ui.add_enabled(
        cue.more_me.is_some(),
        egui::Slider::new(&mut cue.more_me_db, 0.0..=12.0).suffix(" dB"),
    );
    ui.horizontal(|ui| {
        let mut click = cue.click_db.is_some();
        if ui.checkbox(&mut click, tr("Click")).changed() {
            cue.click_db = click.then_some(0.0);
        }
        if let Some(db) = &mut cue.click_db {
            ui.add(egui::Slider::new(db, -40.0..=12.0).suffix(" dB"));
        }
    });
    ui.separator();
    if !cue.follow_main {
        if ui
            .button(tr("Copy the main mix"))
            .on_hover_text(tr("Every track at its level in the main mix"))
            .clicked()
        {
            copy = true;
        }
        for (id, name) in &tracks {
            let mut db = cue.levels.get(id).copied().unwrap_or(0.0).max(-60.0);
            ui.add(egui::Label::new(RichText::new(name).small()).truncate());
            if ui
                .add(egui::Slider::new(&mut db, -60.0..=12.0).suffix(" dB"))
                .changed()
            {
                cue.levels
                    .insert(*id, if db <= -60.0 { MIN_GAIN_DB } else { db });
            }
        }
    }
    if remove {
        doc.history_mut().edit(tr("Remove a headphone mix"), |p| {
            if index < p.cues.len() {
                p.cues.remove(index);
            }
        });
    } else if copy {
        doc.history_mut().edit(tr("Copy the main mix"), |p| {
            if let Some(cue) = p.cues.get(index).cloned() {
                let mut cue = cue;
                copy_main(p, &mut cue);
                p.cues[index] = cue;
            }
        });
    } else if cue != before {
        let key = format!("cue-{index}");
        doc.history_mut()
            .edit_merged(tr("Headphone mix"), &key, |p| {
                if let Some(c) = p.cues.get_mut(index) {
                    *c = cue;
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_take_the_free_pairs_after_the_main_one() {
        let mut p = Project::new("Test", 48_000);
        assert_eq!(free_pair(&p, 2), None, "stereo interface: nothing free");
        assert_eq!(free_pair(&p, 6), Some(2));
        p.cues.push(CueMix {
            output: 2,
            ..CueMix::default()
        });
        assert_eq!(free_pair(&p, 6), Some(4));
        p.cues.push(CueMix {
            output: 4,
            ..CueMix::default()
        });
        assert_eq!(free_pair(&p, 6), None);
        assert_eq!(pair_label(2), "Out 3-4");
    }
}
