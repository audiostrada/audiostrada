//! Reference track window: the song to compare with, its loudness against the mix's, and the
//! switch between hearing the mix and the reference.

use eframe::egui::{self, RichText};

use crate::dialog;
use crate::dialogs;
use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

fn open_id() -> egui::Id {
    egui::Id::unique("reference-window")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(open_id(), true));
}

/// Hears the reference instead of the mix, or the mix again; without a reference, opens the
/// window to choose one.
pub fn toggle(ctx: &egui::Context, doc: &mut Document) {
    if doc.project().reference.is_some() {
        let on = !doc.player().reference_listening();
        doc.player_mut().set_reference_listening(on);
    } else {
        open(ctx);
    }
}

fn lufs(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| format!("{v:.1} LUFS"))
}

/// How much louder or softer the reference is than the mix.
fn difference(mix: f64, reference: f64) -> String {
    let delta = reference - mix;
    if delta.abs() < 0.5 {
        tr("The reference and the mix are about as loud.").to_owned()
    } else if delta > 0.0 {
        trf!(
            "The reference is {db} dB louder than the mix.",
            db = format!("{delta:.1}")
        )
    } else {
        trf!(
            "The reference is {db} dB softer than the mix.",
            db = format!("{:.1}", -delta)
        )
    }
}

/// What the window's buttons asked for.
#[derive(Default)]
struct Asked {
    choose: bool,
    remove: bool,
    listen: Option<bool>,
    matched: Option<bool>,
}

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    let reference = doc.project().reference.clone();
    let listening = doc.player().reference_listening();
    let mut asked = Asked::default();
    dialog::Dialog::new("reference", tr("Reference track"))
        .detail(tr(
            "A finished song to compare your mix with; never exported",
        ))
        .width(500.0)
        .show(ctx, &mut open, |ui, part| match part {
            dialog::Part::Body => body(ui, doc, reference.as_ref(), &mut asked),
            dialog::Part::Actions => {
                let p = theme::current(ui);
                if reference.is_some() {
                    let (text, color) = if listening {
                        (tr("You hear the reference"), p.accent)
                    } else {
                        (tr("You hear the mix"), p.weak)
                    };
                    ui.label(RichText::new(text).size(11.5).strong().color(color));
                }
                dialog::buttons(ui, |ui| match &reference {
                    None => {
                        asked.choose |= dialog::primary(ui, tr("Choose a song…"), true).clicked();
                    }
                    Some(_) => {
                        let text = if listening {
                            tr("Back to the mix")
                        } else {
                            tr("Hear the reference")
                        };
                        if dialog::primary(ui, text, true)
                            .on_hover_text(tr("Also with the REF button of the transport bar"))
                            .clicked()
                        {
                            asked.listen = Some(!listening);
                        }
                    }
                });
            }
        });
    if let Some(file) = dialogs::ask(ctx, "reference", asked.choose, dialogs::pick_audio_file) {
        doc.set_reference(Some(&file));
    }
    if asked.remove {
        doc.set_reference(None);
    }
    if let Some(matched) = asked.matched {
        doc.set_reference_matched(matched);
    }
    if let Some(on) = asked.listen {
        doc.player_mut().set_reference_listening(on);
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

fn body(
    ui: &mut egui::Ui,
    doc: &Document,
    reference: Option<&audiostrada_core::ReferenceTrack>,
    asked: &mut Asked,
) {
    let p = theme::current(ui);
    dialog::section(ui, tr("Song"), |ui| {
        dialog::field(ui, tr("File"), |ui| {
            let name = reference.map_or_else(
                || tr("None").to_owned(),
                |r| {
                    r.path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                },
            );
            let text = RichText::new(name).strong();
            let text = if reference.is_some() {
                text
            } else {
                text.color(p.weak)
            };
            let label = ui.add(egui::Label::new(text).truncate());
            if let Some(r) = reference {
                label.on_hover_text(r.path.display().to_string());
            }
        });
        if reference.is_some() {
            dialog::field(ui, "", |ui| {
                asked.choose |= ui.button(tr("Choose another…")).clicked();
                asked.remove |= ui.button(tr("Remove")).clicked();
            });
        }
        if reference.is_none() {
            dialog::hint(
                ui,
                tr("A song in the style you aim at, well mixed and mastered: WAV, FLAC or MP3."),
            );
        }
    });
    let Some(reference) = reference else {
        return;
    };
    dialog::section(ui, tr("Loudness"), |ui| {
        let (theirs, ours) = (doc.reference_loudness(), doc.mix_loudness());
        dialog::field(ui, tr("Reference"), |ui| {
            let text = theirs.map_or_else(|| tr("Measuring…").to_owned(), |v| lufs(Some(v)));
            ui.monospace(text);
        });
        dialog::field(ui, tr("Your mix"), |ui| {
            ui.monospace(lufs(ours)).on_hover_text(tr(
                "Integrated since the meters were reset, else short-term",
            ));
        });
        match (ours, theirs) {
            (Some(mix), Some(theirs)) => dialog::hint(ui, &difference(mix, theirs)),
            (None, _) => dialog::hint(ui, tr("Play the song to measure the mix.")),
            _ => {}
        }
        dialog::field(ui, tr("Match the level"), |ui| {
            let mut matched = reference.matched;
            if ui
                .checkbox(&mut matched, tr("Play the reference as loud as the mix"))
                .changed()
            {
                asked.matched = Some(matched);
            }
        });
        let hint = if reference.matched {
            trf!(
                "The reference plays {gain} dB, so you compare the sound and not the level.",
                gain = format!("{:+.1}", doc.reference_gain())
            )
        } else {
            tr("The reference plays at its own level.").to_owned()
        };
        dialog::hint(ui, &hint);
    });
}
