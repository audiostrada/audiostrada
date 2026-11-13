//! Adding audio tracks: below the selection, beside a track, or several at once from a dialog.

use audiostrada_core::{Project, TrackId, routing};
use eframe::egui::{self, Color32, RichText};

use crate::dialog;
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
    /// A ready-made set of [`SETS`], else the tracks set by hand.
    set: Option<usize>,
    /// The first input of the device given to the first track, the next ones following.
    first_input: Option<u16>,
    armed: bool,
}

/// A ready-made set of tracks: its name, what it is for, and each track with whether it is
/// stereo.
type Set = (&'static str, &'static str, &'static [(&'static str, bool)]);

const SETS: [Set; 4] = [
    (
        "Drum kit",
        "A miked kit: one track per microphone, the overheads and the room in stereo",
        &[
            ("Kick", false),
            ("Snare", false),
            ("Hi-hat", false),
            ("Tom 1", false),
            ("Tom 2", false),
            ("Floor tom", false),
            ("Overheads", true),
            ("Room", true),
        ],
    ),
    (
        "Guitar",
        "The direct signal and the miked amp, recorded together",
        &[("Guitar DI", false), ("Guitar amp", false)],
    ),
    (
        "Bass guitar",
        "The direct signal and the miked amp, recorded together",
        &[("Bass DI", false), ("Bass amp", false)],
    ),
    (
        "Vocals",
        "A lead vocal, its double and a harmony",
        &[("Lead vocal", false), ("Double", false), ("Harmony", false)],
    ),
];

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
            set: None,
            first_input: None,
            armed: false,
        })
    };
    ctx.data_mut(|d| d.insert_temp(id(), dialog));
}

/// The new tracks, named and stereo or not: numbered when there are several.
fn names(dialog: &Dialog, existing: usize) -> Vec<(String, bool)> {
    if let Some((_, _, tracks)) = dialog.set.and_then(|k| SETS.get(k)) {
        return tracks
            .iter()
            .map(|&(name, stereo)| (tr(name).to_owned(), stereo))
            .collect();
    }
    let base = dialog.name.trim();
    (0..dialog.count as usize)
        .map(|i| match (base.is_empty(), dialog.count) {
            (true, _) => trf!("Audio {n}", n = existing + i + 1),
            (false, 1) => base.to_owned(),
            (false, _) => format!("{base} {}", i + 1),
        })
        .map(|name| (name, dialog.stereo))
        .collect()
}

/// The inputs of the new tracks, one after the other from the first one chosen.
fn inputs(dialog: &Dialog, tracks: &[(String, bool)]) -> Vec<u16> {
    let mut next = dialog.first_input.unwrap_or(0);
    tracks
        .iter()
        .map(|&(_, stereo)| {
            let input = next;
            if dialog.first_input.is_some() {
                next += if stereo { 2 } else { 1 };
            }
            input
        })
        .collect()
}

fn add(doc: &mut Document, dialog: &Dialog) {
    let names = names(dialog, doc.project().tracks.len());
    let inputs = inputs(dialog, &names);
    let at = dialog.at.filter(|_| dialog.beside);
    let mut ids = Vec::new();
    let label = if names.len() == 1 {
        tr("Add track").to_owned()
    } else {
        trf!("Add {count} tracks", count = names.len())
    };
    doc.history_mut().edit(&label, |p| {
        for ((name, stereo), input) in names.iter().zip(&inputs) {
            let id = p.add_track(name);
            if let Some(t) = p.track_mut(id) {
                t.stereo = *stereo;
                t.input = *input;
                t.armed = dialog.armed;
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

/// The first and the last of several names.
fn first_and_last(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        [first, .., last] => format!("{first} … {last}"),
        [] => String::new(),
    }
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
    let existing = doc.project().tracks.len();
    let device_inputs = doc.stream_inputs();
    let mut open = true;
    let (mut added, mut cancel) = (false, false);
    // The Enter that chose the command in the palette must not add the tracks.
    let first = !dialog.focused;
    dialog::Dialog::new("add-tracks", tr("Add audio tracks"))
        .detail(tr("Empty tracks to record on, or to drop audio files on"))
        .width(600.0)
        .modal()
        .show(ctx, &mut open, |ui, part| {
            let tracks = names(&dialog, existing);
            match part {
                dialog::Part::Body => body(
                    ui,
                    &mut dialog,
                    &tracks,
                    beside_name.as_deref(),
                    device_inputs,
                ),
                dialog::Part::Actions => {
                    let stereo = tracks.iter().filter(|t| t.1).count();
                    let text = match (tracks.len() - stereo, stereo) {
                        (mono, 0) => trf!("{count} mono track(s)", count = mono),
                        (0, stereo) => trf!("{count} stereo track(s)", count = stereo),
                        (mono, stereo) => trf!(
                            "{mono} mono and {stereo} stereo tracks",
                            mono = mono,
                            stereo = stereo
                        ),
                    };
                    let names: Vec<String> = tracks.iter().map(|t| t.0.clone()).collect();
                    let weak = theme::current(ui).weak;
                    ui.label(
                        RichText::new(format!("{text} · {}", first_and_last(&names)))
                            .size(11.5)
                            .color(weak),
                    );
                    dialog::buttons(ui, |ui| {
                        let enter = !first && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        added = dialog::primary(ui, tr("Add"), true).clicked() || enter;
                        cancel = dialog::secondary(ui, tr("Cancel")).clicked();
                    });
                }
            }
        });
    if added {
        add(doc, &dialog);
    }
    if added || cancel || !open {
        ctx.data_mut(|d| {
            d.remove::<Dialog>(id());
            d.insert_temp(last_id(), dialog);
        });
    } else {
        ctx.data_mut(|d| d.insert_temp(id(), dialog));
    }
}

fn body(
    ui: &mut egui::Ui,
    dialog: &mut Dialog,
    tracks: &[(String, bool)],
    beside_name: Option<&str>,
    device_inputs: usize,
) {
    dialog::section(ui, tr("Tracks"), |ui| {
        let mut choices: Vec<(Option<usize>, &str, bool)> = vec![(None, tr("Your own"), true)];
        choices.extend(
            SETS.iter()
                .enumerate()
                .map(|(k, s)| (Some(k), tr(s.0), true)),
        );
        dialog::segmented(ui, &mut dialog.set, &choices);
        match dialog.set.and_then(|k| SETS.get(k)) {
            Some(set) => {
                dialog::hint(ui, tr(set.1));
                dialog::well(ui, |ui| {
                    for (name, stereo) in tracks {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            ui.weak(if *stereo { tr("Stereo") } else { tr("Mono") });
                        });
                    }
                });
            }
            None => {
                dialog::field(ui, tr("Number"), |ui| {
                    let count = ui.add(
                        egui::DragValue::new(&mut dialog.count)
                            .range(1..=64)
                            .speed(0.1),
                    );
                    if !dialog.focused {
                        count.request_focus();
                    }
                });
                dialog::field(ui, tr("Name"), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.name)
                            .hint_text(tr("Audio"))
                            .desired_width(240.0),
                    );
                });
                let names: Vec<String> = tracks.iter().map(|t| t.0.clone()).collect();
                dialog::hint(ui, &trf!("Named {names}", names = first_and_last(&names)));
                dialog::field(ui, tr("Channels"), |ui| {
                    dialog::segmented(
                        ui,
                        &mut dialog.stereo,
                        &[(false, tr("Mono"), true), (true, tr("Stereo"), true)],
                    );
                });
            }
        }
        dialog.focused = true;
    });
    dialog::section(ui, tr("Recording"), |ui| {
        dialog::field(ui, tr("Inputs"), |ui| {
            let mut follow = dialog.first_input.is_some();
            if ui
                .checkbox(&mut follow, tr("One after the other, from"))
                .changed()
            {
                dialog.first_input = follow.then_some(0);
            }
            if let Some(first) = &mut dialog.first_input {
                let mut shown = *first + 1;
                ui.add(
                    egui::DragValue::new(&mut shown)
                        .range(1..=256)
                        .speed(0.1)
                        .custom_formatter(|v, _| crate::mixer::input_label(v as u16 - 1, false))
                        .custom_parser(|t| {
                            t.trim_start_matches(|c: char| !c.is_ascii_digit())
                                .parse()
                                .ok()
                        }),
                );
                *first = shown - 1;
            }
        });
        match dialog.first_input {
            Some(_) => {
                let inputs = inputs(dialog, tracks);
                let last = tracks
                    .last()
                    .zip(inputs.last())
                    .map_or(0, |(t, &i)| i + u16::from(t.1));
                let range = trf!(
                    "{first} to {last}",
                    first = crate::mixer::input_label(inputs.first().copied().unwrap_or(0), false),
                    last = crate::mixer::input_label(last, false)
                );
                if device_inputs > 0 && usize::from(last) >= device_inputs {
                    ui.colored_label(
                        crate::settings::WARN,
                        trf!(
                            "{range}: the audio device has {count} inputs",
                            range = range,
                            count = device_inputs
                        ),
                    );
                } else {
                    dialog::hint(ui, &range);
                }
            }
            None => dialog::hint(ui, tr("All on In 1; each can be changed later")),
        }
        dialog::field(ui, tr("Arm"), |ui| {
            ui.checkbox(&mut dialog.armed, tr("Ready to record"));
        });
    });
    dialog::section(ui, tr("Look"), |ui| {
        dialog::field(ui, tr("Color"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
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
                        .min_size(egui::vec2(14.0, 14.0))
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
        });
    });
    if let Some(name) = beside_name {
        let after = dialog.at.is_some_and(|(_, after)| after);
        let beside = if after {
            trf!("Below {name}", name = name)
        } else {
            trf!("Above {name}", name = name)
        };
        dialog::section(ui, tr("Place"), |ui| {
            dialog::segmented(
                ui,
                &mut dialog.beside,
                &[
                    (true, beside.as_str(), true),
                    (false, tr("After the last track"), true),
                ],
            );
        });
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
            set: None,
            first_input: Some(4),
            armed: true,
        };
        add(&mut doc, &dialog);
        assert_eq!(
            names(&doc),
            [
                "A", "Audio 4", "B", "Gtr 1", "Gtr 2", "Gtr 3", "Audio 5", "C"
            ]
        );
        let added = &doc.project().tracks[3..6];
        assert!(
            added
                .iter()
                .all(|t| t.stereo && t.armed && t.color == TRACK_COLORS[2])
        );
        let inputs: Vec<u16> = added.iter().map(|t| t.input).collect();
        assert_eq!(
            inputs,
            [4, 6, 8],
            "stereo tracks take their inputs in pairs"
        );
        assert_eq!(doc.selected_tracks().len(), 3);
    }
}
