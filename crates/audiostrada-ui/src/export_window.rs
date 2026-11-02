//! Export window: format, rate, dither, normalization, range, stems, the render queue and
//! progress.

use audiostrada_media::FileFormat;
use eframe::egui::{self, RichText};

use crate::document::{Document, ExportRange, ExportSettings, Normalize};
use crate::i18n::tr;
use crate::trf;

const RATES: [Option<u32>; 5] = [None, Some(44_100), Some(48_000), Some(88_200), Some(96_000)];

fn rate_name(rate: Option<u32>) -> String {
    match rate {
        None => tr("Stream rate").to_owned(),
        Some(r) => format!("{} kHz", f64::from(r) / 1000.0),
    }
}

fn format_name(format: FileFormat) -> &'static str {
    match format {
        FileFormat::Wav16 => tr("WAV 16-bit"),
        FileFormat::Wav24 => tr("WAV 24-bit"),
        FileFormat::Wav32Float => tr("WAV 32-bit float"),
        FileFormat::Flac16 => tr("FLAC 16-bit"),
        FileFormat::Flac24 => tr("FLAC 24-bit"),
        _ => format.name(),
    }
}

#[derive(Debug, Default)]
pub struct ExportWindow {
    open: bool,
    error: Option<String>,
}

impl ExportWindow {
    pub fn open(&mut self) {
        self.open = true;
        self.error = None;
    }

    /// Shows the window; returns true when the settings changed.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        doc: &mut Document,
        settings: &mut ExportSettings,
    ) -> bool {
        if !self.open && doc.export_progress().is_none() {
            return false;
        }
        let before = settings.clone();
        let mut open = true;
        egui::Window::new(tr("Export"))
            .id(egui::Id::unique("export-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                if let Some((fraction, what)) = doc.export_progress() {
                    ui.label(what);
                    ui.add(egui::ProgressBar::new(fraction).show_percentage());
                    if ui.button(tr("Cancel")).clicked() {
                        doc.cancel_export();
                    }
                    return;
                }
                egui::Grid::new("export-settings")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(tr("Format"));
                        egui::ComboBox::from_id_salt("export-format")
                            .selected_text(format_name(settings.format))
                            .show_ui(ui, |ui| {
                                for format in FileFormat::ALL {
                                    ui.selectable_value(
                                        &mut settings.format,
                                        format,
                                        format_name(format),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label(tr("Sample rate"));
                        let current = if RATES.contains(&settings.sample_rate) {
                            rate_name(settings.sample_rate)
                        } else {
                            tr("Custom").to_owned()
                        };
                        egui::ComboBox::from_id_salt("export-rate")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for rate in RATES {
                                    ui.selectable_value(
                                        &mut settings.sample_rate,
                                        rate,
                                        rate_name(rate),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label(tr("Dither"));
                        ui.add_enabled(
                            settings.format.bits().is_some(),
                            egui::Checkbox::new(&mut settings.dither, tr("Triangular (TPDF)")),
                        );
                        ui.end_row();
                        ui.label(tr("Normalize"));
                        ui.vertical(|ui| {
                            let mut kind = match settings.normalize {
                                Normalize::Off => 0,
                                Normalize::Peak(_) => 1,
                                Normalize::Loudness(_) => 2,
                            };
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut kind, 0, tr("Off"));
                                ui.selectable_value(&mut kind, 1, tr("Peak"));
                                ui.selectable_value(&mut kind, 2, tr("Loudness"));
                            });
                            settings.normalize = match (kind, settings.normalize) {
                                (1, Normalize::Peak(v)) => Normalize::Peak(v),
                                (1, _) => Normalize::Peak(-1.0),
                                (2, Normalize::Loudness(v)) => Normalize::Loudness(v),
                                (2, _) => Normalize::Loudness(-14.0),
                                _ => Normalize::Off,
                            };
                            match &mut settings.normalize {
                                Normalize::Peak(v) => {
                                    ui.add(
                                        egui::DragValue::new(v)
                                            .range(-24.0..=0.0)
                                            .speed(0.1)
                                            .suffix(" dBFS"),
                                    );
                                }
                                Normalize::Loudness(v) => {
                                    ui.add(
                                        egui::DragValue::new(v)
                                            .range(-40.0..=0.0)
                                            .speed(0.1)
                                            .suffix(" LUFS"),
                                    );
                                    ui.weak(tr(
                                        "Streaming services: about -14 LUFS; broadcast: -23 LUFS.",
                                    ));
                                }
                                Normalize::Off => {}
                            }
                        });
                        ui.end_row();
                        ui.label(tr("Range"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut settings.range,
                                ExportRange::Project,
                                tr("Whole project"),
                            );
                            let has_loop = doc.project().loop_range.is_some();
                            ui.add_enabled_ui(has_loop, |ui| {
                                ui.selectable_value(
                                    &mut settings.range,
                                    ExportRange::Loop,
                                    tr("Loop range"),
                                )
                                .on_disabled_hover_text(tr("Set a loop range first"));
                            });
                        });
                        ui.end_row();
                        ui.label(tr("Tail"));
                        ui.add(
                            egui::DragValue::new(&mut settings.tail)
                                .range(0.0..=30.0)
                                .speed(0.1)
                                .suffix(" s"),
                        );
                        ui.end_row();
                        ui.label(tr("Files"));
                        ui.vertical(|ui| {
                            ui.checkbox(&mut settings.mix, tr("Mix"));
                            ui.checkbox(
                                &mut settings.stems,
                                tr("Stems (one file per track, soloed)"),
                            );
                        });
                        ui.end_row();
                    });
                if let Some(error) = &self.error {
                    ui.colored_label(crate::settings::ERROR, error);
                }
                ui.separator();
                let name = doc.project().name.clone();
                let (ext, format) = (settings.format.extension(), format_name(settings.format));
                let dialog = |title: &'static str| {
                    let name = name.clone();
                    move || {
                        rfd::FileDialog::new()
                            .set_title(title)
                            .set_file_name(format!("{name}.{ext}"))
                            .add_filter(format, &[ext])
                            .save_file()
                    }
                };
                ui.horizontal(|ui| {
                    let export = crate::theme::primary_button(ui, tr("Export…")).clicked();
                    if let Some(path) =
                        crate::dialogs::ask(ui.ctx(), "export", export, dialog(tr("Export")))
                    {
                        self.error = doc.start_export(settings.clone(), &path).err();
                    }
                    let queue = ui
                        .button(tr("Add to queue…"))
                        .on_hover_text(tr(
                            "Keep these settings and the project as it is now, to render later \
                             with other exports",
                        ))
                        .clicked();
                    let dialog = dialog(tr("Add to the render queue"));
                    if let Some(path) = crate::dialogs::ask(ui.ctx(), "export-queue", queue, dialog)
                    {
                        self.error = doc.queue_export(settings.clone(), &path).err();
                    }
                });
                let queued = doc.queued_exports();
                if !queued.is_empty() {
                    ui.separator();
                    ui.label(RichText::new(tr("Render queue")).strong());
                    let mut removed = None;
                    for (i, (label, files)) in queued.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button(tr("Remove")).clicked() {
                                removed = Some(i);
                            }
                            ui.label(label);
                            if *files > 1 {
                                ui.weak(trf!("{files} files", files = files));
                            }
                        });
                    }
                    if let Some(i) = removed {
                        doc.remove_queued_export(i);
                    }
                    if ui
                        .button(
                            RichText::new(trf!("Render queue ({count})", count = queued.len()))
                                .strong(),
                        )
                        .clicked()
                    {
                        self.error = doc.render_queue().err();
                    }
                }
            });
        if !open {
            self.open = false;
        }
        *settings != before
    }
}
