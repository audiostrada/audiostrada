//! Export window: format, rate, dither, normalization, range, stems, the render queue and
//! progress.

use audiostrada_media::FileFormat;
use eframe::egui::{self, RichText};

use crate::dialog;
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
        let progress = doc.export_progress();
        if !self.open && progress.is_none() {
            return false;
        }
        let before = settings.clone();
        let mut open = true;
        let name = doc.project().name.clone();
        let mut asked = Asked::default();
        let error = self.error.clone();
        let queued = doc.queued_exports();
        dialog::Dialog::new("export", tr("Export"))
            .detail(trf!(
                "{project}: the mix and the stems to files",
                project = name.as_str()
            ))
            .width(560.0)
            .show(ctx, &mut open, |ui, part| match (part, &progress) {
                (dialog::Part::Body, Some((fraction, what))) => {
                    dialog::section(ui, tr("Rendering"), |ui| {
                        ui.label(what);
                        ui.add(egui::ProgressBar::new(*fraction).show_percentage());
                    });
                }
                (dialog::Part::Body, None) => {
                    body(ui, doc, settings, &queued, &mut asked, error.as_deref());
                }
                (dialog::Part::Actions, Some(_)) => {
                    dialog::buttons(ui, |ui| {
                        asked.cancel = dialog::secondary(ui, tr("Cancel")).clicked();
                    });
                }
                (dialog::Part::Actions, None) => {
                    let weak = crate::theme::current(ui).weak;
                    ui.label(RichText::new(summary(doc, settings)).size(11.5).color(weak));
                    dialog::buttons(ui, |ui| {
                        let ready = settings.mix || settings.stems;
                        asked.export = dialog::primary(ui, tr("Export…"), ready)
                            .on_hover_text(tr("Choose where, then render now"))
                            .clicked()
                            && ready;
                        asked.queue = dialog::secondary(ui, tr("Add to queue…"))
                            .on_hover_text(tr(
                                "Keep these settings and the project as it is now, to render \
                                 later with other exports",
                            ))
                            .clicked()
                            && ready;
                    });
                }
            });
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
        if let Some(path) = crate::dialogs::ask(ctx, "export", asked.export, dialog(tr("Export"))) {
            self.error = doc.start_export(settings.clone(), &path).err();
        }
        let queue_dialog = dialog(tr("Add to the render queue"));
        if let Some(path) = crate::dialogs::ask(ctx, "export-queue", asked.queue, queue_dialog) {
            self.error = doc.queue_export(settings.clone(), &path).err();
        }
        if let Some(i) = asked.remove {
            doc.remove_queued_export(i);
        }
        if asked.render {
            self.error = doc.render_queue().err();
        }
        if asked.cancel {
            doc.cancel_export();
        }
        if !open {
            self.open = false;
            if progress.is_some() {
                doc.cancel_export();
            }
        }
        *settings != before
    }
}

/// What the window's buttons asked for.
#[derive(Default)]
struct Asked {
    export: bool,
    queue: bool,
    cancel: bool,
    render: bool,
    remove: Option<usize>,
}

/// The settings that cover most needs, in one click.
const PRESETS: [(&str, &str); 4] = [
    (
        "Streaming",
        "WAV 24-bit at -14 LUFS: Spotify, Apple Music, YouTube",
    ),
    ("CD", "WAV 16-bit, 44.1 kHz, dithered, peaks at -1 dBFS"),
    (
        "Mastering",
        "WAV 32-bit float, untouched: for a mastering engineer",
    ),
    (
        "Stems",
        "One WAV 24-bit file per track, untouched, for a remix or a mix elsewhere",
    ),
];

fn apply(settings: &mut ExportSettings, preset: usize) {
    let (format, rate, dither, normalize, mix, stems) = match preset {
        0 => (
            FileFormat::Wav24,
            None,
            true,
            Normalize::Loudness(-14.0),
            true,
            false,
        ),
        1 => (
            FileFormat::Wav16,
            Some(44_100),
            true,
            Normalize::Peak(-1.0),
            true,
            false,
        ),
        2 => (
            FileFormat::Wav32Float,
            None,
            false,
            Normalize::Off,
            true,
            false,
        ),
        _ => (FileFormat::Wav24, None, true, Normalize::Off, false, true),
    };
    *settings = ExportSettings {
        format,
        sample_rate: rate,
        dither,
        normalize,
        mix,
        stems,
        ..settings.clone()
    };
}

/// The preset matching the settings, if one does.
fn preset_of(settings: &ExportSettings) -> Option<usize> {
    (0..PRESETS.len()).find(|&k| {
        let mut probe = settings.clone();
        apply(&mut probe, k);
        probe == *settings
    })
}

/// What will be written: format, rate, length and files.
fn summary(doc: &Document, settings: &ExportSettings) -> String {
    let project = doc.project();
    let rate = f64::from(project.sample_rate.max(1));
    let samples = match (settings.range, project.loop_range) {
        (ExportRange::Loop, Some((a, b))) => b.saturating_sub(a),
        _ => project.length(),
    };
    let seconds = samples as f64 / rate + f64::from(settings.tail);
    let length = format!("{}:{:02}", (seconds / 60.0) as u64, seconds as u64 % 60);
    let stems = project
        .tracks
        .iter()
        .filter(|t| t.kind == audiostrada_core::TrackKind::Audio)
        .count();
    let files = usize::from(settings.mix) + if settings.stems { stems } else { 0 };
    format!(
        "{} · {} · {length} · {}",
        format_name(settings.format),
        rate_name(settings.sample_rate),
        trf!("{files} files", files = files)
    )
}

fn body(
    ui: &mut egui::Ui,
    doc: &Document,
    settings: &mut ExportSettings,
    queued: &[(String, usize)],
    asked: &mut Asked,
    error: Option<&str>,
) {
    dialog::section(ui, tr("Ready-made settings"), |ui| {
        let mut chosen = preset_of(settings);
        let choices: Vec<(Option<usize>, &str, bool)> = PRESETS
            .iter()
            .enumerate()
            .map(|(k, (name, _))| (Some(k), tr(name), true))
            .collect();
        if dialog::segmented(ui, &mut chosen, &choices)
            && let Some(k) = chosen
        {
            apply(settings, k);
        }
        let hint = preset_of(settings).map_or(tr("Your own settings"), |k| tr(PRESETS[k].1));
        dialog::hint(ui, hint);
    });
    dialog::section(ui, tr("Files"), |ui| {
        dialog::field(ui, tr("The mix"), |ui| {
            ui.checkbox(&mut settings.mix, tr("One file of the whole mix"));
        });
        dialog::field(ui, tr("Stems"), |ui| {
            ui.checkbox(&mut settings.stems, tr("One file per track, soloed"));
        });
        if !settings.mix && !settings.stems {
            ui.colored_label(
                crate::settings::WARN,
                tr("Choose the mix, the stems or both."),
            );
        }
    });
    dialog::section(ui, tr("Format"), |ui| {
        dialog::field(ui, tr("File"), |ui| {
            egui::ComboBox::from_id_salt("export-format")
                .width(200.0)
                .selected_text(format_name(settings.format))
                .show_ui(ui, |ui| {
                    for format in FileFormat::ALL {
                        ui.selectable_value(&mut settings.format, format, format_name(format));
                    }
                });
        });
        dialog::field(ui, tr("Sample rate"), |ui| {
            let current = if RATES.contains(&settings.sample_rate) {
                rate_name(settings.sample_rate)
            } else {
                tr("Custom").to_owned()
            };
            egui::ComboBox::from_id_salt("export-rate")
                .width(200.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for rate in RATES {
                        ui.selectable_value(&mut settings.sample_rate, rate, rate_name(rate));
                    }
                });
        });
        dialog::field(ui, tr("Dither"), |ui| {
            ui.add_enabled(
                settings.format.bits().is_some(),
                egui::Checkbox::new(&mut settings.dither, tr("Triangular (TPDF)")),
            )
            .on_hover_text(tr(
                "Softens the last bits when they are cut: on for 16 and 24 bits",
            ));
        });
    });
    dialog::section(ui, tr("Level"), |ui| {
        let mut kind = match settings.normalize {
            Normalize::Off => 0,
            Normalize::Peak(_) => 1,
            Normalize::Loudness(_) => 2,
        };
        dialog::field(ui, tr("Normalize"), |ui| {
            dialog::segmented(
                ui,
                &mut kind,
                &[
                    (0, tr("Off"), true),
                    (1, tr("Peak"), true),
                    (2, tr("Loudness"), true),
                ],
            );
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
                dialog::field(ui, tr("Peaks at"), |ui| {
                    ui.add(
                        egui::DragValue::new(v)
                            .range(-24.0..=0.0)
                            .speed(0.1)
                            .suffix(" dBFS"),
                    );
                });
            }
            Normalize::Loudness(v) => {
                dialog::field(ui, tr("Loudness"), |ui| {
                    ui.add(
                        egui::DragValue::new(v)
                            .range(-40.0..=0.0)
                            .speed(0.1)
                            .suffix(" LUFS"),
                    );
                    for (target, name) in [(-14.0, "-14"), (-16.0, "-16"), (-23.0, "-23")] {
                        if ui.small_button(name).clicked() {
                            *v = target;
                        }
                    }
                });
                dialog::hint(
                    ui,
                    tr("Streaming services: about -14 LUFS; broadcast: -23 LUFS."),
                );
            }
            Normalize::Off => {}
        }
    });
    dialog::section(ui, tr("Range"), |ui| {
        let has_loop = doc.project().loop_range.is_some();
        dialog::field(ui, tr("From and to"), |ui| {
            dialog::segmented(
                ui,
                &mut settings.range,
                &[
                    (ExportRange::Project, tr("Whole project"), true),
                    (ExportRange::Loop, tr("Loop range"), has_loop),
                ],
            );
        });
        if !has_loop {
            dialog::hint(ui, tr("Set a loop range first to export only a part."));
        }
        dialog::field(ui, tr("Tail"), |ui| {
            ui.add(
                egui::DragValue::new(&mut settings.tail)
                    .range(0.0..=30.0)
                    .speed(0.1)
                    .suffix(" s"),
            )
            .on_hover_text(tr("Rendered after the end, for reverb and delay tails"));
        });
    });
    if let Some(error) = error {
        ui.colored_label(crate::settings::ERROR, error);
    }
    if !queued.is_empty() {
        dialog::section(ui, tr("Render queue"), |ui| {
            dialog::well(ui, |ui| {
                for (i, (label, files)) in queued.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        if *files > 1 {
                            ui.weak(trf!("{files} files", files = files));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(tr("Remove")).clicked() {
                                asked.remove = Some(i);
                            }
                        });
                    });
                }
            });
            if ui
                .button(
                    RichText::new(trf!("Render queue ({count})", count = queued.len())).strong(),
                )
                .clicked()
            {
                asked.render = true;
            }
        });
    }
}
