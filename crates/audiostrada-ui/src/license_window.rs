//! The License window: what this copy may do, entering a license, links to the store.

use audiostrada_license::{License, Status};
use eframe::egui::{self, Align2, RichText, vec2};

use crate::i18n::tr;
use crate::theme;
use crate::trf;

#[derive(Debug, Default)]
pub struct LicenseWindow {
    open: bool,
    text: String,
    error: Option<String>,
}

impl LicenseWindow {
    pub fn open(&mut self) {
        self.open = true;
        self.error = None;
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
        egui::Window::new(tr("License"))
            .id(egui::Id::unique("license-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .pivot(Align2::CENTER_TOP)
            .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                summary(ui, status);
                if matches!(status, Status::Unofficial) {
                    return;
                }
                theme::section(ui, tr("Enter a license"));
                ui.weak(tr(
                    "Paste the license from the purchase email, or drop the file on AudioStrada.",
                ));
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
                ui.horizontal(|ui| {
                    let open = ui.button(tr("Open a file…")).clicked();
                    if let Some(path) = crate::dialogs::ask(ui.ctx(), "license", open, || {
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
                    if theme::primary_button_enabled(
                        ui,
                        !self.text.trim().is_empty(),
                        tr("Activate"),
                    )
                    .clicked()
                    {
                        self.install(status);
                    }
                });
                theme::section(ui, tr("Subscription"));
                ui.horizontal_wrapped(|ui| {
                    let link = |ui: &mut egui::Ui, text: &str, page: &str| {
                        if ui.link(text).clicked() {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab(audiostrada_license::link(page)));
                        }
                    };
                    link(ui, tr("Buy a license"), "buy");
                    link(ui, tr("Manage my subscription"), "account");
                    link(ui, tr("Find my license"), "recover");
                });
                if status.license().is_some() {
                    theme::footer(ui, |ui| {
                        if ui.button(tr("Remove the license")).clicked() {
                            let _ = audiostrada_license::remove();
                            *status = audiostrada_license::status(None);
                            audiostrada_engine::evaluation::set_active(status.evaluation());
                        }
                    });
                }
            });
        self.open = open;
    }
}

fn summary(ui: &mut egui::Ui, status: &Status) {
    let (title, detail) = match status {
        Status::Unofficial => (
            tr("Built from source"),
            tr("Nothing to unlock: every feature works, with no time limit.").to_owned(),
        ),
        Status::Licensed(license) if license.is_lifetime() => (
            tr("Licensed for life"),
            holder(license) + "\n" + tr("Every update included, for life."),
        ),
        Status::Licensed(license) => (
            tr("Licensed"),
            holder(license)
                + "\n"
                + &trf!("Updates included until {date}.", date = license.updates_until),
        ),
        Status::Lapsed(license) => (
            tr("Updates ended"),
            holder(license)
                + "\n"
                + &trf!(
                    "Your updates ended on {date}: this version runs in evaluation mode. Versions released before keep working.",
                    date = license.updates_until
                ),
        ),
        Status::Evaluation => (
            tr("Evaluation"),
            tr("Every feature works, export included. After 20 minutes, the sound drops out for a moment every 10 minutes. A license removes the dropouts and pays for the development.").to_owned(),
        ),
    };
    let color = if status.evaluation() {
        theme::RECORD
    } else {
        theme::current(ui).accent
    };
    ui.label(RichText::new(title).font(theme::strong(18.0)).color(color));
    ui.label(detail);
}

fn holder(license: &License) -> String {
    trf!(
        "{plan} of {name} ({email}).",
        plan = plan_name(license.plan),
        name = license.name,
        email = license.email
    )
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
    use audiostrada_license::Plan;

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
