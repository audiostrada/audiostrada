//! When the system installs new versions by itself (the package repositories, Flathub, the Snap
//! Store), a license whose updates end would land in evaluation mode with the next one. A month
//! before, AudioStrada says so once and how to keep this version; after such an update, how to go
//! back to the last version the license covers.

use std::sync::mpsc::{Receiver, channel};

use audiostrada_license::{License, Status};
use eframe::egui;

use crate::dialog;
use crate::i18n::tr;
use crate::trf;
use crate::ui_prefs::UiPrefs;
use crate::updates::{self, Install};

/// Days before the end of the updates when the warning shows.
const NOTICE_DAYS: i64 = 30;

#[derive(Debug, Default)]
pub struct UpdatesEnd {
    pending: Option<Receiver<Option<String>>>,
    /// The last version the license covers, once looked up.
    last_covered: Option<Option<String>>,
}

impl UpdatesEnd {
    /// Shows the warning or the way back when due; true when `prefs` changed.
    pub fn show(&mut self, ctx: &egui::Context, status: &Status, prefs: &mut UiPrefs) -> bool {
        let install = updates::install();
        if !audiostrada_license::OFFICIAL || !install.automatic() {
            return false;
        }
        match status {
            Status::Licensed(license) if ending(license, &audiostrada_license::today()) => {
                if prefs.updates_end_noted.as_deref() == Some(license.updates_until.as_str()) {
                    return false;
                }
                if warning(ctx, license, install) {
                    prefs.updates_end_noted = Some(license.updates_until.clone());
                    return true;
                }
                false
            }
            Status::Lapsed(license) => {
                let version = env!("CARGO_PKG_VERSION");
                if prefs.lapsed_noted.as_deref() == Some(version) {
                    return false;
                }
                if self.way_back(ctx, license, install) {
                    prefs.lapsed_noted = Some(version.to_owned());
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Says what came with the system's update; true once closed.
    fn way_back(&mut self, ctx: &egui::Context, license: &License, install: Install) -> bool {
        self.look_up(ctx, license);
        let title = trf!(
            "Version {version} came after your updates",
            version = env!("CARGO_PKG_VERSION")
        );
        let detail = trf!(
            "Your updates ended on {date}: this version runs in evaluation mode",
            date = license.updates_until
        );
        let mut open = true;
        let mut ok = false;
        dialog::Dialog::new("updates-lapsed", &title)
            .detail(detail)
            .width(520.0)
            .modal()
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    ui.label(tr(
                        "Your system installed it with its other updates. Renew your license to \
                         unlock it, or go back to the last version your license covers.",
                    ));
                    match &self.last_covered {
                        None => {
                            dialog::hint(ui, tr("Looking for the last version covered…"));
                        }
                        Some(Some(version)) => match install.back_command(version) {
                            Some(command) => {
                                dialog::hint(
                                    ui,
                                    &trf!(
                                        "Version {version} in a terminal, kept at the next \
                                         updates:",
                                        version = version
                                    ),
                                );
                                command_line(ui, &command);
                            }
                            None => dialog::hint(
                                ui,
                                &trf!(
                                    "Your license covers version {version}: the manual tells \
                                     how to go back to it.",
                                    version = version
                                ),
                            ),
                        },
                        Some(None) => dialog::hint(
                            ui,
                            tr("The manual tells how to go back to an earlier version."),
                        ),
                    }
                }
                dialog::Part::Actions => dialog::buttons(ui, |ui| {
                    if dialog::primary(ui, tr("Renew"), true).clicked() {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(audiostrada_license::link("account")));
                    }
                    ok = dialog::secondary(ui, tr("Close")).clicked();
                }),
            });
        !open || ok
    }

    fn look_up(&mut self, ctx: &egui::Context, license: &License) {
        if self.last_covered.is_some() {
            return;
        }
        match &self.pending {
            None => {
                let (tx, rx) = channel();
                let until = license.updates_until.clone();
                std::thread::spawn(move || {
                    let found =
                        updates::published().and_then(|list| updates::last_covered(&list, &until));
                    let _ = tx.send(found);
                });
                self.pending = Some(rx);
            }
            Some(rx) => match rx.try_recv() {
                Ok(found) => self.last_covered = Some(found),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.last_covered = Some(None),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            },
        }
        if self.last_covered.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

/// The updates end within the notice, and are not for life.
fn ending(license: &License, today: &str) -> bool {
    !license.is_lifetime()
        && audiostrada_license::days_after(&license.updates_until, -NOTICE_DAYS)
            .is_some_and(|from| today >= from.as_str())
}

/// The month's notice; true once closed.
fn warning(ctx: &egui::Context, license: &License, install: Install) -> bool {
    let title = trf!("Your updates end on {date}", date = license.updates_until);
    let mut open = true;
    let mut ok = false;
    dialog::Dialog::new("updates-ending", &title)
        .detail(tr("Your system installs new versions by itself"))
        .width(520.0)
        .modal()
        .show(ctx, &mut open, |ui, part| match part {
            dialog::Part::Body => {
                ui.label(trf!(
                    "Versions released after {date} will run in evaluation mode. Renew your \
                     license, or keep this version when your system updates:",
                    date = license.updates_until
                ));
                if let Some(command) = install.hold_command() {
                    command_line(ui, command);
                }
                dialog::hint(
                    ui,
                    tr("The manual tells how to let the updates in again after renewing."),
                );
            }
            dialog::Part::Actions => dialog::buttons(ui, |ui| {
                if dialog::primary(ui, tr("Renew"), true).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(audiostrada_license::link("account")));
                }
                ok = dialog::secondary(ui, tr("Close")).clicked();
            }),
        });
    !open || ok
}

fn command_line(ui: &mut egui::Ui, command: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(command).monospace());
        if ui.small_button(tr("Copy")).clicked() {
            ui.ctx().copy_text(command.to_owned());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn license(until: &str) -> License {
        License {
            id: "a".into(),
            name: "b".into(),
            email: "c".into(),
            updates_until: until.into(),
            issued: "2026-01-01".into(),
            plan: audiostrada_license::Plan::Personal,
        }
    }

    #[test]
    fn the_end_of_the_updates_is_told_a_month_before() {
        assert!(!ending(&license("2027-03-01"), "2027-01-29"));
        assert!(ending(&license("2027-03-01"), "2027-01-30"));
        assert!(ending(&license("2027-03-01"), "2027-03-01"));
        assert!(!ending(&license("9999-12-31"), "9999-12-30"));
    }
}
