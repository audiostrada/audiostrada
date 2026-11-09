//! Inspector: properties of the selected clip and track, playlists and comping.

use audiostrada_core::{ClipId, FadeShape, Project, TrackId, TrackKind, edit, routing};
use eframe::egui::{self, RichText, Ui};

use crate::document::Document;
use crate::i18n::tr;
use crate::mixer::{end_gesture, format_time};
use crate::trf;

fn shape_name(shape: FadeShape) -> &'static str {
    match shape {
        FadeShape::EqualPower => tr("Equal power"),
        FadeShape::Linear => tr("Linear"),
        FadeShape::Fast => tr("Fast"),
        FadeShape::Slow => tr("Slow"),
        FadeShape::SCurve => tr("S-curve"),
    }
}

pub fn show(ui: &mut Ui, doc: &mut Document, selection: &[ClipId]) {
    let track = doc.focused_track();
    let project = doc.project().clone();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let clips: Vec<ClipId> = selection
                .iter()
                .copied()
                .filter(|id| edit::clip(&project, *id).is_some())
                .collect();
            match clips.as_slice() {
                [] => {
                    ui.weak(tr("Select a clip to see its properties."));
                }
                [id] => clip(ui, doc, &project, *id),
                many => {
                    ui.label(trf!("{count} clips selected", count = many.len()));
                }
            }
            ui.separator();
            match track.filter(|t| project.track(*t).is_some()) {
                Some(id) => track_section(ui, doc, &project, id),
                None => {
                    ui.weak(tr("Click a track header to see the track."));
                }
            }
        });
}

fn text_field(
    ui: &mut Ui,
    doc: &mut Document,
    value: &str,
    label: &str,
    key: String,
    set: impl FnOnce(&mut Project, String),
) {
    let mut text = value.to_owned();
    let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(160.0));
    if response.changed() {
        doc.history_mut().edit_merged(label, &key, |p| set(p, text));
    }
    if response.lost_focus() {
        doc.history_mut().end_gesture();
    }
}

fn clip(ui: &mut Ui, doc: &mut Document, project: &Project, id: ClipId) {
    let Some(c) = edit::clip(project, id).cloned() else {
        return;
    };
    let rate = project.sample_rate;
    let ms = |samples: u64| samples as f64 * 1000.0 / f64::from(rate);
    let samples = |ms: f64| (ms.max(0.0) * f64::from(rate) / 1000.0).round() as u64;
    ui.heading(tr("Clip"));
    egui::Grid::new("clip-properties")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(tr("Name"));
            text_field(
                ui,
                doc,
                &c.name,
                tr("Rename clip"),
                format!("clip-name:{}", id.0),
                |p, name| {
                    if let Some((t, i)) = edit::locate(p, id) {
                        std::sync::Arc::make_mut(&mut p.tracks[t]).clips[i].name = name;
                    }
                },
            );
            ui.end_row();
            ui.label(tr("Start"));
            let bbt = project
                .tempo
                .beat_to_bbt(project.tempo.sample_to_beat(c.start, rate));
            ui.label(format!(
                "{}   {}.{}.{:03}",
                format_time(c.start, rate),
                bbt.bar,
                bbt.beat,
                bbt.tick
            ));
            ui.end_row();
            ui.label(tr("Length"));
            ui.label(format_time(c.length, rate));
            ui.end_row();
            ui.label(tr("Gain"));
            let mut gain = c.gain_db;
            let response = ui.add(
                egui::Slider::new(&mut gain, -36.0..=12.0)
                    .suffix(" dB")
                    .fixed_decimals(1),
            );
            if response.changed() {
                doc.history_mut()
                    .edit_merged(tr("Clip gain"), &format!("clip-gain:{}", id.0), |p| {
                        edit::set_clip_gain(p, id, gain)
                    });
            }
            end_gesture(doc.history_mut(), &response);
            ui.end_row();
            for (label, fade_in) in [(tr("Fade in"), true), (tr("Fade out"), false)] {
                ui.label(label);
                let mut value = ms(if fade_in { c.fade_in } else { c.fade_out });
                let response = ui.add(
                    egui::DragValue::new(&mut value)
                        .range(0.0..=60_000.0)
                        .speed(1.0)
                        .suffix(" ms"),
                );
                if response.changed() {
                    let (fi, fo) = if fade_in {
                        (samples(value), c.fade_out)
                    } else {
                        (c.fade_in, samples(value))
                    };
                    doc.history_mut().edit_merged(
                        label,
                        &format!("fade:{fade_in}:{}", id.0),
                        |p| edit::set_fades(p, id, fi, fo),
                    );
                }
                end_gesture(doc.history_mut(), &response);
                ui.end_row();
            }
            for fade_in in [true, false] {
                ui.label(if fade_in {
                    tr("Fade-in shape")
                } else {
                    tr("Fade-out shape")
                });
                let current = if fade_in { c.fade_shape } else { c.out_shape() };
                egui::ComboBox::from_id_salt(("fade-shape", fade_in, id.0))
                    .selected_text(shape_name(current))
                    .show_ui(ui, |ui| {
                        for shape in FadeShape::ALL {
                            if ui
                                .selectable_label(current == shape, shape_name(shape))
                                .clicked()
                                && current != shape
                            {
                                doc.history_mut().edit(tr("Fade shape"), |p| {
                                    if let Some(m) = edit::clip_mut(p, id) {
                                        if fade_in {
                                            let out = m.out_shape();
                                            m.fade_shape = shape;
                                            m.fade_out_shape = Some(out);
                                        } else {
                                            m.fade_out_shape = Some(shape);
                                        }
                                    }
                                });
                            }
                        }
                    });
                ui.end_row();
            }
            if !c.is_midi() {
                ui.label(tr("Stretch"));
                let mut percent = c.stretch * 100.0;
                let response = ui
                    .add(
                        egui::DragValue::new(&mut percent)
                            .range(10.0..=1000.0)
                            .speed(0.5)
                            .fixed_decimals(1)
                            .suffix(" %"),
                    )
                    .on_hover_text(tr(
                        "Length relative to the original, pitch kept. Alt-drag the clip's right edge too",
                    ));
                if response.changed() {
                    let base = c.length as f64 / c.stretch;
                    doc.history_mut().edit_merged(
                        tr("Stretch clip"),
                        &format!("stretch:{}", id.0),
                        |p| edit::stretch_clip(p, id, (base * percent / 100.0).round() as u64),
                    );
                }
                end_gesture(doc.history_mut(), &response);
                ui.end_row();
                ui.label(tr("Pitch"));
                let mut pitch = c.pitch;
                let response = ui
                    .add(
                        egui::DragValue::new(&mut pitch)
                            .range(-24.0..=24.0)
                            .speed(0.05)
                            .fixed_decimals(2)
                            .suffix(format!(" {}", tr("st"))),
                    )
                    .on_hover_text(tr("Transposition in semitones, length kept"));
                if response.changed() {
                    doc.history_mut().edit_merged(
                        tr("Transpose clip"),
                        &format!("pitch:{}", id.0),
                        |p| edit::set_pitch(p, &[id], pitch),
                    );
                }
                end_gesture(doc.history_mut(), &response);
                ui.end_row();
            }
            if let Some(source) = project.sources.get(&c.source) {
                ui.label(tr("File"));
                ui.label(
                    RichText::new(source.path.display().to_string())
                        .weak()
                        .small(),
                );
                ui.end_row();
            }
        });
}

fn track_section(ui: &mut Ui, doc: &mut Document, project: &Project, id: TrackId) {
    let Some(track) = project.track(id) else {
        return;
    };
    ui.heading(tr("Track"));
    egui::Grid::new("track-properties")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(tr("Name"));
            text_field(
                ui,
                doc,
                &track.name,
                tr("Rename track"),
                format!("track-name:{}", id.0),
                |p, name| {
                    if let Some(t) = p.track_mut(id) {
                        t.name = name;
                    }
                },
            );
            ui.end_row();
            ui.label(tr("Color"));
            let mut color = track.color;
            let response = ui.color_edit_button_srgb(&mut color);
            if response.changed() {
                doc.history_mut().edit_merged(
                    tr("Track color"),
                    &format!("track-color:{}", id.0),
                    |p| {
                        if let Some(t) = p.track_mut(id) {
                            t.color = color;
                        }
                    },
                );
            }
            ui.end_row();
            if track.kind.carries_audio() {
                ui.label(tr("Output"));
                let output = ui.button(crate::mixer::output_label(project, track));
                let tracks = vec![id];
                egui::Popup::menu(&output).show(|ui| {
                    if let Err(e) = crate::mixer::output_menu(ui, doc, project, &tracks) {
                        doc.notify(e);
                    }
                });
                ui.end_row();
            }
            if track.kind != TrackKind::Audio {
                return;
            }
            ui.label(tr("Input"));
            ui.horizontal(|ui| {
                let mut input = track.input + 1;
                let mut stereo = track.stereo;
                let a = ui.add(
                    egui::DragValue::new(&mut input)
                        .range(1..=63)
                        .prefix(format!("{} ", tr("In"))),
                );
                let b = ui.checkbox(&mut stereo, tr("Stereo"));
                if a.changed() || b.changed() {
                    doc.history_mut().edit_merged(
                        tr("Track input"),
                        &format!("track-input:{}", id.0),
                        |p| {
                            if let Some(t) = p.track_mut(id) {
                                t.input = input - 1;
                                t.stereo = stereo;
                            }
                        },
                    );
                }
                end_gesture(doc.history_mut(), &a);
            });
            ui.end_row();
            if track.kind == TrackKind::Audio && !doc.is_instrument_track(id) {
                ui.label(tr("Record"));
                crate::recording::record_source(ui, doc, track);
                ui.end_row();
            }
            ui.label(tr("Playlist"));
            ui.label(RichText::new(&track.playlist).strong());
            ui.end_row();
        });
    ui.horizontal(|ui| {
        if ui.button(tr("Move up")).clicked() {
            doc.history_mut()
                .edit(tr("Move track"), |p| routing::move_track(p, id, true));
        }
        if ui.button(tr("Move down")).clicked() {
            doc.history_mut()
                .edit(tr("Move track"), |p| routing::move_track(p, id, false));
        }
    });
    if track.kind == TrackKind::Vca {
        return;
    }
    ui.horizontal(|ui| {
        if track.frozen.is_some() {
            ui.colored_label(crate::settings::WARN, tr("Frozen"));
            if ui.button(tr("Unfreeze")).on_hover_text(tr("Play the clips and inserts again")).clicked() {
                doc.unfreeze(id);
            }
        } else if ui
            .add_enabled(!track.clips.is_empty() || routing::has_children(project, id), egui::Button::new(tr("Freeze")))
            .on_hover_text(tr("Render the track with its inserts to a file and play that instead, to save processing"))
            .clicked()
            && let Err(e) = doc.freeze(id)
        {
            ui.colored_label(crate::settings::ERROR, e);
        }
    });
    ui.horizontal(|ui| {
        if ui.button(tr("New playlist")).clicked() {
            doc.history_mut()
                .edit(tr("New playlist"), |p| edit::new_playlist(p, id));
        }
        if ui
            .add_enabled(
                !track.alternates.is_empty(),
                egui::Button::new(tr("Next playlist")),
            )
            .clicked()
        {
            doc.history_mut()
                .edit(tr("Switch playlist"), |p| edit::next_playlist(p, id));
        }
    });
    if track.alternates.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Other playlists and takes")).strong());
    ui.weak(tr("Copy a clip into the current playlist to build a comp."));
    let rate = project.sample_rate;
    for (index, playlist) in track.alternates.iter().enumerate() {
        egui::CollapsingHeader::new(trf!(
            "{name}  ({count} clips)",
            name = playlist.name,
            count = playlist.clips.len()
        ))
        .id_salt(("playlist", id.0, index))
        .show(ui, |ui| {
            if ui.button(tr("Use this playlist")).clicked() {
                doc.history_mut().edit(tr("Switch playlist"), |p| {
                    edit::select_playlist(p, id, index)
                });
            }
            for c in &playlist.clips {
                ui.horizontal(|ui| {
                    ui.label(format!("{}  {}", format_time(c.start, rate), c.name));
                    if ui.small_button(tr("Copy to current")).clicked() {
                        doc.history_mut().edit(tr("Comp take"), |p| {
                            edit::promote_take(p, id, index, c.id);
                        });
                    }
                });
            }
        });
    }
}
