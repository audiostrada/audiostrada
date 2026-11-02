//! Reference track window: the song to compare with, its loudness against the mix's, and the
//! switch between hearing the mix and the reference.

use eframe::egui::{self, Align2, RichText, vec2};

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

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    egui::Window::new(tr("Reference track"))
        .id(egui::Id::unique("reference-window-frame"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .pivot(Align2::CENTER_TOP)
        .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
        .show(ctx, |ui| {
            let p = theme::current(ui);
            ui.weak(tr(
                "A finished song to compare your mix with: heard instead of the mix, at the \
                 same place in the song, never exported.",
            ));
            ui.separator();
            let reference = doc.project().reference.clone();
            ui.horizontal(|ui| {
                let name = reference.as_ref().map_or_else(
                    || tr("No reference").to_owned(),
                    |r| {
                        r.path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                    },
                );
                ui.label(RichText::new(name).strong());
                let open = ui.button(tr("Choose…")).clicked();
                if let Some(file) =
                    dialogs::ask(ui.ctx(), "reference", open, dialogs::pick_audio_file)
                {
                    doc.set_reference(Some(&file));
                }
                if reference.is_some() && ui.button(tr("Remove")).clicked() {
                    doc.set_reference(None);
                }
            });
            let Some(reference) = reference else {
                return;
            };
            ui.add_space(6.0);
            egui::Grid::new("reference-loudness")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label(tr("Reference"));
                    ui.monospace(lufs(doc.reference_loudness()));
                    ui.end_row();
                    ui.label(tr("Mix"));
                    ui.monospace(lufs(doc.mix_loudness())).on_hover_text(tr(
                        "Integrated since the meters were reset, else short-term",
                    ));
                    ui.end_row();
                });
            let mut matched = reference.matched;
            if ui
                .checkbox(&mut matched, tr("Match the mix's loudness"))
                .on_hover_text(tr(
                    "Plays the reference as loud as the mix, so you compare the sound and \
                     not the level",
                ))
                .changed()
            {
                doc.set_reference_matched(matched);
            }
            if matched {
                ui.weak(trf!(
                    "The reference plays {gain} dB",
                    gain = format!("{:+.1}", doc.reference_gain())
                ));
            }
            ui.add_space(8.0);
            let listening = doc.player().reference_listening();
            ui.horizontal(|ui| {
                let big = |text: &str| RichText::new(text).size(15.0).strong();
                if ui
                    .add_sized(
                        [120.0, 34.0],
                        egui::Button::new(big(tr("Mix"))).selected(!listening),
                    )
                    .clicked()
                {
                    doc.player_mut().set_reference_listening(false);
                }
                if ui
                    .add_sized(
                        [120.0, 34.0],
                        egui::Button::new(big(tr("Reference"))).selected(listening),
                    )
                    .clicked()
                {
                    doc.player_mut().set_reference_listening(true);
                }
            });
            if listening {
                ui.colored_label(p.accent, tr("You hear the reference."));
            }
        });
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}
