//! The License window: what this copy may do, entering a license, links to the store.

use audiostrada_license::Status;
use eframe::egui::{self, RichText};

use crate::dialog;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

#[derive(Debug, Default)]
pub struct LicenseWindow {
    open: bool,
    text: String,
    error: Option<String>,
    /// Removing the license was asked: the actions band asks to confirm.
    removing: bool,
}

impl LicenseWindow {
    pub fn open(&mut self) {
        self.open = true;
        self.error = None;
        self.removing = false;
    }

    /// Takes a license dropped on the window or opened from a file.
    pub fn offer(&mut self, text: String, status: &mut Status) {
        self.open = true;
        self.text = text;
        self.install(status);
    }

    fn install(&mut self, status: &mut Status) {
        match audiostrada_license::install(&self.text) {
            Ok(license) => {
                *status = audiostrada_license::status(Some(license));
                audiostrada_engine::evaluation::set_active(status.evaluation());
                self.text.clear();
                self.error = None;
            }
            Err(e) => self.error = Some(error_text(&e)),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, status: &mut Status) {
        if !self.open {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut activate = false;
        let mut remove = false;
        let unofficial = matches!(status, Status::Unofficial);
        let detail = match status {
            Status::Unofficial => tr("Built from source: nothing to unlock"),
            Status::Evaluation => tr("Evaluation: every feature works, with short dropouts"),
            Status::Licensed(_) => tr("Licensed: thank you for supporting AudioStrada"),
            Status::Lapsed(_) => tr("Updates ended: this version runs in evaluation mode"),
        };
        dialog::Dialog::new("license", tr("License"))
            .detail(detail)
            .width(540.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    dialog::section(ui, tr("This copy"), |ui| summary(ui, status));
                    if unofficial {
                        return;
                    }
                    dialog::section(ui, tr("Enter a license"), |ui| {
                        dialog::hint(
                            ui,
                            tr(
                                "Paste the license from the purchase email, or drop the file on \
                                AudioStrada.",
                            ),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut self.text)
                                .font(egui::TextStyle::Monospace)
                                .desired_rows(5)
                                .desired_width(f32::INFINITY)
                                .hint_text("-----BEGIN AUDIOSTRADA LICENSE-----"),
                        );
                        if let Some(error) = &self.error {
                            ui.colored_label(theme::RECORD, error);
                        }
                        let pick = ui.button(tr("Open a file…")).clicked();
                        if let Some(path) = crate::dialogs::ask(ui.ctx(), "license", pick, || {
                            rfd::FileDialog::new().pick_file()
                        }) {
                            match std::fs::read_to_string(&path) {
                                Ok(text) => {
                                    self.text = text;
                                    self.install(status);
                                }
                                Err(e) => self.error = Some(e.to_string()),
                            }
                        }
                    });
                    dialog::section(ui, tr("Subscription"), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (text, page) in [
                                (tr("Buy a license"), "buy"),
                                (tr("Manage my subscription"), "account"),
                                (tr("Find my license"), "recover"),
                            ] {
                                if ui.button(text).clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(
                                        audiostrada_license::link(page),
                                    ));
                                }
                            }
                        });
                        dialog::hint(ui, tr("These open the AudioStrada web site."));
                    });
                }
                dialog::Part::Actions => {
                    if self.removing {
                        let weak = theme::current(ui).weak;
                        ui.label(
                            RichText::new(tr("Remove the license from this computer?"))
                                .size(11.5)
                                .color(weak),
                        );
                        dialog::buttons(ui, |ui| {
                            remove = dialog::primary(ui, tr("Remove"), true).clicked();
                            if dialog::secondary(ui, tr("Keep it")).clicked() {
                                self.removing = false;
                            }
                        });
                        return;
                    }
                    if status.license().is_some()
                        && dialog::secondary(ui, tr("Remove the license"))
                            .on_hover_text(tr(
                                "This copy goes back to evaluation; the license can be entered \
                                 again later",
                            ))
                            .clicked()
                    {
                        self.removing = true;
                    }
                    dialog::buttons(ui, |ui| {
                        if unofficial {
                            close = dialog::primary(ui, tr("Close"), true).clicked();
                            return;
                        }
                        let ready = !self.text.trim().is_empty();
                        activate = dialog::primary(ui, tr("Activate"), ready)
                            .on_hover_text(tr("Check the license and unlock this copy"))
                            .clicked()
                            && ready;
                        close = dialog::secondary(ui, tr("Close")).clicked();
                    });
                }
            });
        if activate {
            self.install(status);
        }
        if remove {
            let _ = audiostrada_license::remove();
            *status = audiostrada_license::status(None);
            audiostrada_engine::evaluation::set_active(status.evaluation());
            self.removing = false;
        }
        self.open = open && !close;
    }
}

/// What this copy may do: the state in its color, then the license's details.
fn summary(ui: &mut egui::Ui, status: &Status) {
    let p = theme::current(ui);
    let (title, color) = match status {
        Status::Unofficial => (tr("Built from source"), p.accent),
        Status::Licensed(license) if license.is_lifetime() => (tr("Licensed for life"), p.accent),
        Status::Licensed(_) => (tr("Licensed"), p.accent),
        Status::Lapsed(_) => (tr("Updates ended"), theme::RECORD),
        Status::Evaluation => (tr("Evaluation"), theme::RECORD),
    };
    dialog::well(ui, |ui| {
        ui.label(RichText::new(title).font(theme::strong(17.0)).color(color));
        match status {
            Status::Unofficial => {
                ui.label(tr(
                    "Nothing to unlock: every feature works, with no time limit.",
                ));
            }
            Status::Evaluation => {
                ui.label(tr("Every feature works, export included. After 20 minutes, the sound drops out for a moment every 10 minutes. A license removes the dropouts and pays for the development."));
            }
            Status::Licensed(license) | Status::Lapsed(license) => {
                ui.add_space(4.0);
                dialog::field(ui, tr("License"), |ui| ui.label(plan_name(license.plan)));
                dialog::field(ui, tr("Licensed to"), |ui| {
                    ui.label(format!("{} ({})", license.name, license.email))
                });
                dialog::field(ui, tr("Updates"), |ui| {
                    let text = match status {
                        Status::Licensed(_) if license.is_lifetime() => {
                            tr("Every update included, for life.").to_owned()
                        }
                        Status::Licensed(_) => trf!(
                            "Updates included until {date}.",
                            date = license.updates_until
                        ),
                        _ => trf!("Ended on {date}", date = license.updates_until),
                    };
                    ui.label(text)
                });
                if matches!(status, Status::Lapsed(_)) {
                    dialog::hint(
                        ui,
                        tr(
                            "Versions released before your updates ended keep working without \
                            dropouts. Renew to unlock this one.",
                        ),
                    );
                }
            }
        }
    });
}

pub fn plan_name(plan: audiostrada_license::Plan) -> &'static str {
    match plan {
        audiostrada_license::Plan::Personal => tr("Personal license"),
        audiostrada_license::Plan::Commercial => tr("Commercial license"),
    }
}

fn error_text(error: &audiostrada_license::LicenseError) -> String {
    use audiostrada_license::LicenseError;
    match error {
        LicenseError::Format => tr(
            "This text is not a complete license: copy it whole, from the first line to the last.",
        )
        .to_owned(),
        LicenseError::Signature => tr("This license is not valid.").to_owned(),
        LicenseError::NoKey => tr("This build cannot check licenses.").to_owned(),
        LicenseError::Io(e) => trf!("Cannot save the license: {error}", error = e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_license::{License, Plan};

    fn license(until: &str, plan: Plan) -> License {
        License {
            id: "lic_1".into(),
            name: "Ada".into(),
            email: "ada@example.org".into(),
            updates_until: until.into(),
            issued: "2026-11-01".into(),
            plan,
        }
    }

    #[test]
    fn every_status_draws_and_a_wrong_text_is_refused() {
        let ctx = egui::Context::default();
        let statuses = [
            Status::Unofficial,
            Status::Evaluation,
            Status::Licensed(license("2027-11-01", Plan::Personal)),
            Status::Licensed(license("9999-12-31", Plan::Commercial)),
            Status::Lapsed(license("2025-01-01", Plan::Personal)),
        ];
        for mut status in statuses {
            let mut window = LicenseWindow::default();
            window.open();
            for _ in 0..2 {
                ctx.run_ui(egui::RawInput::default(), |ui| {
                    window.show(ui.ctx(), &mut status)
                })
                .drop_without_applying_deltas();
            }
            window.offer("not a license".into(), &mut status);
            assert!(window.error.is_some());
            ctx.run_ui(egui::RawInput::default(), |ui| {
                window.show(ui.ctx(), &mut status)
            })
            .drop_without_applying_deltas();
        }
        assert_ne!(plan_name(Plan::Personal), plan_name(Plan::Commercial));
    }
}
