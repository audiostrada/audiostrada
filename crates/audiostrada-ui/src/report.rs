//! Crash reports and feedback, sent to the web site, which mails them. A crash leaves a report
//! on disk, offered at the next start (never sent without the user's word); Help › Report a bug
//! or an idea writes one by hand.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, RichText, Ui};

use crate::dialog;
use crate::i18n::tr;
use crate::theme;
use crate::ui_prefs::UiPrefs;

/// The session's log, read for what led to a report.
static LOG: OnceLock<Option<PathBuf>> = OnceLock::new();
/// A report being written, so that a panic while writing it does not write another.
static WRITING: AtomicBool = AtomicBool::new(false);
/// What is kept of the log in a report, its end.
const LOG_TAIL: u64 = 48 * 1024;

/// Where reports go: the web site's address, or the one given when building
/// (`AUDIOSTRADA_REPORTS`, a variable of the official builds).
fn endpoint(kind: &str) -> String {
    // An unset GitHub variable comes in empty.
    let base = option_env!("AUDIOSTRADA_REPORTS")
        .filter(|b| !b.trim().is_empty())
        .map_or_else(
            || audiostrada_license::link("report"),
            |b| b.trim().trim_end_matches('/').to_owned(),
        );
    format!("{base}/{kind}")
}

fn crash_dir() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("audiostrada").join("crashes"))
}

/// Writes a report of every panic to the crashes folder, then lets the panic go on; `log` is
/// the session's log.
pub fn install(log: Option<PathBuf>) {
    let _ = LOG.set(log);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !WRITING.swap(true, Ordering::SeqCst) {
            write_crash(info);
            WRITING.store(false, Ordering::SeqCst);
        }
        previous(info);
    }));
}

/// The version, the build and the system, first in every report.
fn header() -> String {
    format!(
        "AudioStrada {} ({} build)\nSystem: {} {}\n",
        env!("CARGO_PKG_VERSION"),
        if audiostrada_license::OFFICIAL {
            "official"
        } else {
            "source"
        },
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// The end of the session's log, from a whole line.
fn log_tail() -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Some(Some(path)) = LOG.get() else {
        return String::new();
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |m| m.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let start = if length > LOG_TAIL {
        text.find('\n').map_or(0, |n| n + 1)
    } else {
        0
    };
    text[start..].to_owned()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn write_crash(info: &std::panic::PanicHookInfo<'_>) {
    let Some(dir) = crash_dir() else {
        return;
    };
    if cfg!(test) || std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let thread = std::thread::current();
    let text = format!(
        "{}Date: {} (Unix time)\nThread: {}\nPanic: {}\nAt: {}\n\nBacktrace:\n{}\n\nLog:\n{}",
        header(),
        now(),
        thread.name().unwrap_or("unnamed"),
        info.payload_as_str().unwrap_or("(no message)"),
        info.location()
            .map_or_else(|| "unknown".to_owned(), |l| l.to_string()),
        std::backtrace::Backtrace::force_capture(),
        log_tail(),
    );
    let _ = std::fs::write(dir.join(format!("crash-{}.txt", now())), text);
}

/// The reports of crashes not sent nor dismissed yet, oldest first.
fn pending() -> Vec<PathBuf> {
    let Some(dir) = crash_dir() else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    files
}

/// Sends a report (`kind`: crash or feedback) as JSON; the web site mails it.
fn post(kind: &str, body: serde_json::Value) -> Result<(), String> {
    // Tests never reach the network.
    if cfg!(test) {
        return Err("no network in tests".into());
    }
    let response = crate::tone3000::agent()
        .post(endpoint(kind))
        .content_type("application/json")
        .send(body.to_string())
        .map_err(|e| e.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("the server answered {}", response.status()))
    }
}

/// A report on its way, and how it went.
#[derive(Debug, Default)]
enum Sending {
    #[default]
    Idle,
    Busy(Receiver<Result<(), String>>),
    Done,
    Failed(String),
}

impl Sending {
    fn start(&mut self, kind: &'static str, body: serde_json::Value) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(post(kind, body));
        });
        *self = Sending::Busy(rx);
    }

    /// Moves on once the answer came; true while waiting.
    fn poll(&mut self) -> bool {
        if let Sending::Busy(rx) = self
            && let Ok(result) = rx.try_recv()
        {
            *self = match result {
                Ok(()) => Sending::Done,
                Err(e) => Sending::Failed(e),
            };
        }
        matches!(self, Sending::Busy(_))
    }
}

/// An email address, roughly: something before and after an @, a dot in the domain.
fn looks_like_email(text: &str) -> bool {
    let text = text.trim();
    text.split_once('@')
        .is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !text.contains(' '))
}

/// The crash reports found at start, offered one window for all.
#[derive(Debug, Default)]
pub struct CrashOffer {
    files: Vec<PathBuf>,
    looked: bool,
    comment: String,
    email: String,
    always: bool,
    sending: Sending,
}

impl CrashOffer {
    /// Shows the offer while there are reports, sent by themselves once the user chose so;
    /// true when that choice was just made (the preferences to save).
    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut UiPrefs) -> bool {
        if !self.looked {
            self.looked = true;
            self.files = pending();
            self.always = prefs.send_crash_reports == Some(true);
            if self.always && !self.files.is_empty() {
                self.send();
            }
        }
        if self.files.is_empty() {
            return false;
        }
        if self.sending.poll() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if matches!(self.sending, Sending::Done) {
            forget(&self.files);
            self.files.clear();
            return false;
        }
        if prefs.send_crash_reports == Some(true) && matches!(self.sending, Sending::Busy(_)) {
            return false;
        }
        let mut open = true;
        let (mut send, mut dismiss) = (false, false);
        let count = self.files.len();
        let report = std::fs::read_to_string(&self.files[count - 1]).unwrap_or_default();
        dialog::Dialog::new("crash-report", tr("AudioStrada closed unexpectedly"))
            .detail(tr("A report can help fix it: nothing is sent without you"))
            .width(620.0)
            .modal()
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => self.body(ui, &report, count),
                dialog::Part::Actions => {
                    if let Sending::Failed(e) = &self.sending {
                        ui.label(
                            RichText::new(crate::trf!("Not sent: {error}", error = e))
                                .size(11.5)
                                .color(crate::settings::WARN),
                        );
                    }
                    dialog::buttons(ui, |ui| {
                        let busy = matches!(self.sending, Sending::Busy(_));
                        send = dialog::primary(ui, tr("Send the report"), !busy).clicked();
                        dismiss = dialog::secondary(ui, tr("Don't send")).clicked();
                    });
                }
            });
        if send {
            if self.always {
                prefs.send_crash_reports = Some(true);
            }
            self.send();
        }
        if dismiss || !open {
            forget(&self.files);
            self.files.clear();
        }
        send && self.always
    }

    fn body(&mut self, ui: &mut Ui, report: &str, count: usize) {
        let p = theme::current(ui);
        ui.label(if count > 1 {
            crate::trf!(
                "AudioStrada stopped on an error {count} times. The reports hold the version, \
                 the system, where the error happened and the end of the log: no audio, no \
                 project.",
                count = count
            )
        } else {
            tr(
                "AudioStrada stopped on an error. The report holds the version, the system, where \
                the error happened and the end of the log: no audio, no project.",
            )
            .to_owned()
        });
        ui.add_space(6.0);
        dialog::field(ui, tr("What were you doing?"), |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.comment)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text(tr("Optional: what you did just before")),
            );
        });
        dialog::field(ui, tr("Your email"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.email)
                    .desired_width(f32::INFINITY)
                    .hint_text(tr("Optional, to be answered")),
            );
        });
        ui.checkbox(
            &mut self.always,
            tr("Always send crash reports without asking"),
        );
        ui.add_space(4.0);
        ui.collapsing(tr("What is sent"), |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(report).monospace().size(10.5).color(p.weak));
                });
        });
    }

    fn send(&mut self) {
        let reports: Vec<String> = self
            .files
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .collect();
        let email = looks_like_email(&self.email).then(|| self.email.trim().to_owned());
        self.sending.start(
            "crash",
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "reports": reports,
                "comment": self.comment.trim(),
                "email": email,
            }),
        );
    }
}

/// Removes the reports sent or dismissed.
fn forget(files: &[PathBuf]) {
    for file in files {
        let _ = std::fs::remove_file(file);
    }
}

/// What a report written by hand is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Bug,
    Idea,
    Other,
}

impl Kind {
    fn id(self) -> &'static str {
        match self {
            Kind::Bug => "bug",
            Kind::Idea => "idea",
            Kind::Other => "other",
        }
    }
}

/// Help › Report a bug or an idea: a title, what happened or what is wished, an email to be
/// answered, the log if the user agrees.
#[derive(Debug, Default)]
pub struct Feedback {
    open: bool,
    kind: Kind,
    title: String,
    text: String,
    email: String,
    log: bool,
    sending: Sending,
}

impl Feedback {
    pub fn open(&mut self) {
        if !self.open {
            *self = Feedback {
                open: true,
                log: true,
                ..Default::default()
            };
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        if self.sending.poll() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut open = self.open;
        let mut close = false;
        let detail = tr("Straight to the people who make AudioStrada; we read everything");
        dialog::Dialog::new("feedback", tr("Report a bug or an idea"))
            .detail(detail)
            .width(600.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => self.body(ui),
                dialog::Part::Actions => {
                    let ready = !self.title.trim().is_empty() && !self.text.trim().is_empty();
                    match &self.sending {
                        Sending::Done => {
                            ui.label(
                                RichText::new(tr("Sent. Thank you!"))
                                    .size(11.5)
                                    .color(theme::current(ui).accent),
                            );
                        }
                        Sending::Failed(e) => {
                            ui.label(
                                RichText::new(crate::trf!("Not sent: {error}", error = e))
                                    .size(11.5)
                                    .color(crate::settings::WARN),
                            );
                        }
                        _ => {}
                    }
                    dialog::buttons(ui, |ui| {
                        if matches!(self.sending, Sending::Done) {
                            close = dialog::primary(ui, tr("Close"), true).clicked();
                            return;
                        }
                        let busy = matches!(self.sending, Sending::Busy(_));
                        if dialog::primary(ui, tr("Send the message"), ready && !busy).clicked() {
                            self.send();
                        }
                        close = dialog::secondary(ui, tr("Cancel")).clicked();
                    });
                }
            });
        self.open = open && !close;
    }

    fn body(&mut self, ui: &mut Ui) {
        dialog::field(ui, tr("About"), |ui| {
            dialog::segmented(
                ui,
                &mut self.kind,
                &[
                    (Kind::Bug, tr("A bug"), true),
                    (Kind::Idea, tr("An idea"), true),
                    (Kind::Other, tr("Something else"), true),
                ],
            );
        });
        let (title_hint, text_hint) = match self.kind {
            Kind::Bug => (
                tr("What goes wrong, in a few words"),
                tr("What you did, what you expected, what happened instead"),
            ),
            Kind::Idea => (
                tr("The feature, in a few words"),
                tr("What it would let you do, and how you picture it"),
            ),
            Kind::Other => (tr("A title"), tr("Your message")),
        };
        dialog::field(ui, tr("Title"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.title)
                    .desired_width(f32::INFINITY)
                    .hint_text(title_hint),
            );
        });
        dialog::field(ui, tr("Details"), |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_rows(6)
                    .desired_width(f32::INFINITY)
                    .hint_text(text_hint),
            );
        });
        dialog::field(ui, tr("Your email"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.email)
                    .desired_width(f32::INFINITY)
                    .hint_text(tr("Optional, to be answered")),
            );
        });
        ui.checkbox(
            &mut self.log,
            tr("Attach the version, the system and the end of the log"),
        );
        dialog::hint(
            ui,
            tr("Neither audio nor projects are ever sent. Sent through audiostrada.com."),
        );
    }

    fn send(&mut self) {
        let email = looks_like_email(&self.email).then(|| self.email.trim().to_owned());
        let details = self
            .log
            .then(|| format!("{}\nLog:\n{}", header(), log_tail()));
        self.sending.start(
            "feedback",
            serde_json::json!({
                "kind": self.kind.id(),
                "title": self.title.trim(),
                "text": self.text.trim(),
                "email": email,
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "details": details,
            }),
        );
    }
}

/// The crashes folder, for the tests.
#[cfg(test)]
pub(crate) fn crashes() -> Option<PathBuf> {
    crash_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_go_to_the_web_site_and_emails_are_checked_roughly() {
        assert!(endpoint("crash").ends_with("/report/crash"));
        assert!(looks_like_email(" ada@example.org "));
        assert!(!looks_like_email("ada"));
        assert!(!looks_like_email("ada@local"));
        assert!(!looks_like_email("a da@example.org"));
    }

    #[test]
    fn the_crashes_left_are_found_oldest_first_and_forgotten_once_answered() {
        let dir = crashes().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["crash-2.txt", "crash-1.txt", "note.md"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let found = pending();
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("crash-1.txt"));
        forget(&found);
        assert!(pending().is_empty());
    }
}
