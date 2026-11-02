//! Audio configuration panel: backend, devices, rate, buffer, test tone, meters, diagnostics.

use std::time::Instant;

use audiostrada_io::diagnostics::{self, Check, CheckStatus};
use audiostrada_io::tools::gain_to_db;
use audiostrada_io::{BackendKind, StreamConfig, compiled_backends};
use eframe::egui::{self, Color32, ComboBox, RichText, Sense, Ui, vec2};

use crate::host::AudioHost;
use crate::i18n::tr;
use crate::trf;

const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const BUFFERS: [u32; 7] = [32, 64, 128, 256, 512, 1024, 2048];
pub(crate) const METER_FLOOR_DB: f32 = -60.0;
const METER_FALL_DB_PER_S: f32 = 24.0;

pub(crate) const OK: Color32 = Color32::from_rgb(70, 180, 90);
pub(crate) const WARN: Color32 = Color32::from_rgb(230, 170, 40);
pub(crate) const ERROR: Color32 = Color32::from_rgb(220, 70, 60);

/// What the user changed this frame.
#[derive(Debug, Default, PartialEq)]
pub enum SettingsChange {
    #[default]
    None,
    Backend(BackendKind),
    Config(StreamConfig),
}

#[derive(Debug)]
pub struct AudioSettingsPanel {
    input_levels: Vec<f32>,
    output_levels: Vec<f32>,
    last_frame: Instant,
    checks: Option<Vec<Check>>,
}

impl Default for AudioSettingsPanel {
    fn default() -> Self {
        Self {
            input_levels: Vec::new(),
            output_levels: Vec::new(),
            last_frame: Instant::now(),
            checks: None,
        }
    }
}

fn status_color(status: CheckStatus) -> Color32 {
    match status {
        CheckStatus::Ok => OK,
        CheckStatus::Warning => WARN,
        CheckStatus::Error => ERROR,
        CheckStatus::Unknown => Color32::GRAY,
    }
}

impl AudioSettingsPanel {
    /// Draws the panel and returns the change requested by the user, applied by the caller.
    pub fn show(&mut self, ui: &mut Ui, host: &AudioHost) -> SettingsChange {
        let mut change = SettingsChange::None;
        let mut config = host.config().clone();

        egui::Grid::new("audio-settings")
            .num_columns(2)
            .spacing([24.0, 10.0])
            .min_col_width(110.0)
            .show(ui, |ui| {
                ui.label(tr("Backend"));
                let current = host.kind();
                ComboBox::from_id_salt("backend")
                    .width(320.0)
                    .selected_text(current.map_or(tr("none"), |k| k.name()))
                    .show_ui(ui, |ui| {
                        for &kind in compiled_backends() {
                            if ui
                                .selectable_label(current == Some(kind), kind.name())
                                .clicked()
                                && current != Some(kind)
                            {
                                change = SettingsChange::Backend(kind);
                            }
                        }
                    });
                ui.end_row();

                ui.label(tr("Input"));
                self.device_combo(ui, host, true, &mut config.input_device);
                ui.end_row();

                ui.label(tr("Output"));
                self.device_combo(ui, host, false, &mut config.output_device);
                ui.end_row();

                ui.label(tr("Sample rate"));
                let rate_text = config
                    .sample_rate
                    .map_or(tr("Device default").to_owned(), |r| format!("{r} Hz"));
                ComboBox::from_id_salt("rate")
                    .width(320.0)
                    .selected_text(rate_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut config.sample_rate, None, tr("Device default"));
                        for rate in RATES {
                            ui.selectable_value(
                                &mut config.sample_rate,
                                Some(rate),
                                format!("{rate} Hz"),
                            );
                        }
                    });
                ui.end_row();

                ui.label(tr("Buffer size"));
                let rate = host
                    .stream()
                    .map(|s| s.info().sample_rate)
                    .or(config.sample_rate)
                    .unwrap_or(48_000);
                let buffer = |frames: u32| {
                    let ms = f64::from(frames) * 1000.0 / f64::from(rate);
                    trf!(
                        "{frames} frames ({ms} ms)",
                        frames = frames,
                        ms = format!("{ms:.1}")
                    )
                };
                ComboBox::from_id_salt("buffer")
                    .width(320.0)
                    .selected_text(buffer(config.buffer_size))
                    .show_ui(ui, |ui| {
                        for frames in BUFFERS {
                            ui.selectable_value(&mut config.buffer_size, frames, buffer(frames));
                        }
                    });
                ui.end_row();

                ui.label(tr("Channels"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut config.inputs)
                            .range(0..=64)
                            .suffix(tr(" in")),
                    );
                    ui.add(
                        egui::DragValue::new(&mut config.outputs)
                            .range(0..=64)
                            .suffix(tr(" out")),
                    );
                });
                ui.end_row();
            });

        if change == SettingsChange::None && config != *host.config() {
            change = SettingsChange::Config(config);
        }

        ui.add_space(8.0);
        self.status(ui, host);
        ui.add_space(8.0);
        self.meters(ui, host);
        ui.add_space(12.0);
        self.diagnostics(ui);
        change
    }

    fn device_combo(
        &self,
        ui: &mut Ui,
        host: &AudioHost,
        input: bool,
        selected: &mut Option<String>,
    ) {
        let devices: Vec<_> = host
            .devices()
            .iter()
            .filter(|d| {
                (if input { d.inputs > 0 } else { d.outputs > 0 }) || d.inputs + d.outputs == 0
            })
            .collect();
        let default_name = devices
            .iter()
            .find(|d| {
                if input {
                    d.is_default_input
                } else {
                    d.is_default_output
                }
            })
            .map(|d| d.name.as_str());
        let text = match selected {
            Some(id) => devices
                .iter()
                .find(|d| d.id == *id)
                .map_or(id.clone(), |d| d.name.clone()),
            None => trf!(
                "System default ({device})",
                device = default_name.unwrap_or(tr("none"))
            ),
        };
        ComboBox::from_id_salt(if input {
            "input-device"
        } else {
            "output-device"
        })
        .width(320.0)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, None, tr("System default"));
            for device in devices {
                let channels = if input { device.inputs } else { device.outputs };
                let label = if channels > 0 {
                    format!("{} ({channels} ch)", device.name)
                } else {
                    device.name.clone()
                };
                ui.selectable_value(selected, Some(device.id.clone()), label);
            }
        });
    }

    fn status(&self, ui: &mut Ui, host: &AudioHost) {
        ui.horizontal(|ui| match (host.stream(), host.error()) {
            (Some(stream), _) => {
                let info = stream.info();
                let status = stream.status();
                let (color, state) = if status.failed() {
                    (ERROR, tr("Stopped (device error)"))
                } else if status.panicked() {
                    (ERROR, tr("Processing error"))
                } else {
                    (OK, tr("Running"))
                };
                ui.colored_label(color, RichText::new(format!("● {state}")).strong());
                let frames = match status.current_buffer_size() {
                    0 => info.buffer_size,
                    n => n,
                };
                let rate = match status.current_sample_rate() {
                    0 => info.sample_rate,
                    r => r,
                };
                let ms = f64::from(frames) * 1000.0 / f64::from(rate.max(1));
                ui.label(trf!(
                    "{rate} Hz, {frames} frames ({ms} ms)",
                    rate = rate,
                    frames = frames,
                    ms = format!("{ms:.1}")
                ));
                let xruns = status.xruns();
                ui.colored_label(
                    if xruns == 0 { Color32::GRAY } else { WARN },
                    trf!("{count} dropouts", count = xruns),
                );
                if status.alloc_violations() > 0 {
                    ui.colored_label(
                        WARN,
                        trf!(
                            "{count} allocations in audio thread",
                            count = status.alloc_violations()
                        ),
                    );
                }
            }
            (None, Some(error)) => {
                ui.colored_label(ERROR, RichText::new(format!("● {error}")).strong());
            }
            (None, None) => {
                ui.colored_label(Color32::GRAY, format!("● {}", tr("Stopped")));
            }
        });
        if let Some(stream) = host.stream() {
            ui.weak(&stream.info().device);
        }
    }

    fn meters(&mut self, ui: &mut Ui, host: &AudioHost) {
        let now = Instant::now();
        let fall = now.duration_since(self.last_frame).as_secs_f32() * METER_FALL_DB_PER_S;
        self.last_frame = now;
        let update = |levels: &mut Vec<f32>, meters: &audiostrada_io::tools::Meters| {
            levels.resize(meters.channels(), METER_FLOOR_DB);
            for (ch, level) in levels.iter_mut().enumerate() {
                let peak = gain_to_db(meters.take_peak(ch)).max(METER_FLOOR_DB);
                *level = peak.max(*level - fall);
            }
        };
        update(&mut self.input_levels, host.input_meters());
        update(&mut self.output_levels, host.output_meters());

        ui.horizontal(|ui| {
            let mut tone = host.tone();
            if ui
                .toggle_value(&mut tone, tr("🔊 Test tone (440 Hz, -18 dBFS)"))
                .changed()
            {
                host.set_tone(tone);
            }
        });
        ui.add_space(4.0);
        for (label, levels) in [("In", &self.input_levels), ("Out", &self.output_levels)] {
            for (ch, &db) in levels.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [48.0, 14.0],
                        egui::Label::new(format!("{label} {}", ch + 1)),
                    );
                    meter(ui, db, 260.0, 10.0);
                    ui.monospace(if db <= METER_FLOOR_DB {
                        "  -∞ dB".to_owned()
                    } else {
                        format!("{db:5.1} dB")
                    });
                });
            }
        }
    }

    fn diagnostics(&mut self, ui: &mut Ui) {
        ui.collapsing(tr("System check"), |ui| {
            if self.checks.is_none() || ui.button(tr("Run again")).clicked() {
                self.checks = Some(diagnostics::run());
            }
            let Some(checks) = &self.checks else { return };
            if checks.is_empty() {
                ui.weak(tr("No checks for this platform yet."));
            }
            for check in checks {
                ui.horizontal(|ui| {
                    ui.colored_label(status_color(check.status), "●");
                    ui.label(RichText::new(check.name).strong());
                    ui.label(&check.detail);
                });
                if let Some(fix) = check
                    .fix
                    .as_ref()
                    .filter(|_| check.status != CheckStatus::Ok)
                {
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.monospace(fix);
                        });
                        if ui.small_button(tr("Copy")).clicked() {
                            ui.ctx().copy_text(fix.clone());
                        }
                    });
                }
            }
        });
    }
}

/// Horizontal level bar from -60 to 0 dBFS.
pub(crate) fn meter(ui: &mut Ui, db: f32, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let fraction = ((db - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0);
    if fraction > 0.0 {
        let mut filled = rect;
        filled.set_width(rect.width() * fraction);
        let color = if db > -3.0 {
            ERROR
        } else if db > -12.0 {
            WARN
        } else {
            OK
        };
        painter.rect_filled(filled, 2.0, color);
    }
}
