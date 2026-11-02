//! The track menu, shared by the timeline headers and the mixer strips. Color, duplicate and
//! delete act on every selected track when the clicked one is part of the selection.

use audiostrada_core::{TrackId, TrackKind, edit, routing};
use eframe::egui::{self, Color32, Ui};

use crate::actions::Action;
use crate::document::{Document, SelectMode};
use crate::i18n::tr;
use crate::timeline::TRACK_COLORS;
use crate::trf;

/// Commands the menu leaves to the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Action(Action),
    /// Export this track to a file.
    Export(TrackId),
}

/// The tracks a command applies to: the selection if `id` is in it, else `id` alone.
fn targets(doc: &mut Document, id: TrackId) -> Vec<TrackId> {
    if !doc.is_track_selected(id) {
        doc.select_track(id, SelectMode::Only);
    }
    doc.selected_tracks().to_vec()
}

pub fn show(ui: &mut Ui, doc: &mut Document, id: TrackId) -> Vec<Request> {
    let mut requests = Vec::new();
    let project = doc.project().clone();
    let Some(track) = project.track(id) else {
        ui.close();
        return requests;
    };
    let tracks = targets(doc, id);
    let mut name = track.name.clone();
    if ui
        .add(egui::TextEdit::singleline(&mut name).desired_width(160.0))
        .changed()
    {
        doc.history_mut()
            .edit_merged(tr("Rename track"), &format!("track-name:{}", id.0), |p| {
                if let Some(t) = p.track_mut(id) {
                    t.name = name;
                }
            });
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for color in TRACK_COLORS {
            let fill = Color32::from_rgb(color[0], color[1], color[2]);
            let button = egui::Button::new("")
                .fill(fill)
                .min_size(egui::vec2(16.0, 16.0))
                .stroke(if track.color == color {
                    egui::Stroke::new(2.0, Color32::WHITE)
                } else {
                    egui::Stroke::NONE
                });
            if ui.add(button).clicked() {
                doc.history_mut().edit(tr("Track color"), |p| {
                    for &t in &tracks {
                        if let Some(t) = p.track_mut(t) {
                            t.color = color;
                        }
                    }
                });
            }
        }
    });
    if tracks.len() > 1 {
        ui.weak(trf!("{count} tracks selected", count = tracks.len()));
    }
    crate::mixer::group_submenu(ui, doc, id);
    if track.kind != TrackKind::Vca {
        midi_out_submenu(ui, doc, id, track.midi_out.as_deref());
    }
    ui.separator();
    if track.kind != TrackKind::Vca {
        let mut stereo = track.stereo;
        if ui
            .checkbox(&mut stereo, tr("Stereo track"))
            .on_hover_text(tr(
                "Records two inputs side by side (In 1-2); stereo files play in stereo either way",
            ))
            .changed()
        {
            doc.history_mut().edit(tr("Stereo track"), |p| {
                for &t in &tracks {
                    if let Some(t) = p.track_mut(t).filter(|t| t.kind == TrackKind::Audio) {
                        t.stereo = stereo;
                    }
                }
            });
        }
        if tracks.len() == 2
            && ui
                .button(tr("Convert to a stereo track"))
                .on_hover_text(tr(
                    "The two tracks become one stereo track: left and right are told by the \
                     names (L, R, Left, Right), else the upper track is the left one",
                ))
                .clicked()
        {
            if let Err(e) = doc.make_stereo(tracks[0], tracks[1]) {
                doc.notify(trf!("Cannot convert to a stereo track: {e}", e = e));
            }
            ui.close();
        }
    }
    if track.kind != TrackKind::Vca {
        crate::mixer::save_chain_menu(
            ui,
            doc,
            (audiostrada_core::routing::Channel::Track(id), false),
            &track.name,
        );
    }
    ui.separator();
    if ui.button(tr("Add a track above")).clicked() {
        crate::add_tracks::add_one(doc, false, Some((id, false)));
        ui.close();
    }
    if ui.button(tr("Add a track below")).clicked() {
        crate::add_tracks::add_one(doc, false, Some((id, true)));
        ui.close();
    }
    if ui.button(tr("Add tracks…")).clicked() {
        crate::add_tracks::open(ui.ctx(), Some((id, true)));
        ui.close();
    }
    ui.separator();
    if track.kind != TrackKind::Vca && ui.button(tr("Import audio file…")).clicked() {
        doc.select_track(id, SelectMode::Only);
        requests.push(Request::Action(Action::ImportAudio));
        ui.close();
    }
    let many = tracks.len() > 1;
    let duplicate = if many {
        tr("Duplicate tracks")
    } else {
        tr("Duplicate track")
    };
    if ui.button(duplicate).clicked() {
        let mut copies = Vec::new();
        doc.history_mut().edit(tr("Duplicate track"), |p| {
            for &t in &tracks {
                copies.extend(edit::duplicate_track(p, t));
            }
        });
        doc.set_track_selection(copies);
        ui.close();
    }
    let delete = if many {
        tr("Delete tracks")
    } else {
        tr("Delete track")
    };
    if ui.button(delete).clicked() {
        doc.history_mut().edit(tr("Delete track"), |p| {
            for &t in &tracks {
                p.remove_track(t);
            }
            p.prune_sources();
        });
        ui.close();
    }
    if track.kind == TrackKind::Vca {
        return requests;
    }
    ui.separator();
    let has_audio = !track.clips.is_empty() || routing::has_children(&project, id);
    if track.frozen.is_some() {
        if ui.button(tr("Unfreeze")).clicked() {
            doc.unfreeze(id);
            ui.close();
        }
    } else if ui
        .add_enabled(has_audio, egui::Button::new(tr("Freeze")))
        .on_hover_text(tr(
            "Render with the inserts and play that instead, to save processing",
        ))
        .clicked()
    {
        if let Err(e) = doc.freeze(id) {
            doc.notify(trf!("Cannot freeze: {e}", e = e));
        }
        ui.close();
    }
    if ui
        .add_enabled(has_audio, egui::Button::new(tr("Bounce to a new track")))
        .on_hover_text(tr(
            "Render with the inserts onto a new track and mute this one",
        ))
        .clicked()
    {
        if let Err(e) = doc.bounce(id) {
            doc.notify(trf!("Cannot bounce: {e}", e = e));
        }
        ui.close();
    }
    if ui.button(tr("Export track…")).clicked() {
        requests.push(Request::Export(id));
        ui.close();
    }
    ui.separator();
    if ui.button(tr("New playlist")).clicked() {
        doc.history_mut()
            .edit(tr("New playlist"), |p| edit::new_playlist(p, id));
        ui.close();
    }
    let takes = track.show_takes;
    if ui
        .add_enabled(
            !track.alternates.is_empty() || takes,
            egui::Button::new(if takes {
                tr("Hide take lanes")
            } else {
                tr("Show take lanes")
            }),
        )
        .on_hover_text(tr(
            "Show the other playlists under the track; drag on one to comp",
        ))
        .clicked()
    {
        doc.history_mut().modify_unrecorded(|p| {
            if let Some(t) = p.track_mut(id) {
                t.show_takes = !takes;
            }
        });
        ui.close();
    }
    let shown = track.show_automation;
    if ui
        .button(if shown {
            tr("Hide automation")
        } else {
            tr("Show automation")
        })
        .clicked()
    {
        doc.history_mut().modify_unrecorded(|p| {
            if let Some(t) = p.track_mut(id) {
                t.show_automation = !shown;
            }
        });
        ui.close();
    }
    requests
}

/// Output port the track's MIDI is sent to, for external instruments.
fn midi_out_submenu(ui: &mut Ui, doc: &mut Document, id: TrackId, current: Option<&str>) {
    ui.menu_button(tr("MIDI output"), |ui| {
        let mut choice = None;
        if ui.selectable_label(current.is_none(), tr("None")).clicked() {
            choice = Some(None);
        }
        // Listing ports opens a client: refreshed every two seconds while the menu is open.
        let key = egui::Id::unique("midi-output-ports");
        let now = ui.input(|i| i.time);
        let cached: Option<(f64, Vec<String>)> = ui.data(|d| d.get_temp(key));
        let mut ports = match cached {
            Some((at, ports)) if now - at < 2.0 => ports,
            _ => {
                let ports = audiostrada_midi::devices::output_names();
                ui.data_mut(|d| d.insert_temp(key, (now, ports.clone())));
                ports
            }
        };
        if let Some(c) = current
            && !ports.iter().any(|p| p == c)
        {
            ports.push(c.to_owned());
        }
        for port in ports {
            if ui
                .selectable_label(current == Some(port.as_str()), &port)
                .clicked()
            {
                choice = Some(Some(port));
            }
        }
        if let Some(port) = choice {
            doc.history_mut().edit(tr("MIDI output"), |p| {
                if let Some(t) = p.track_mut(id) {
                    t.midi_out = port;
                }
            });
            ui.close();
        }
    });
}
