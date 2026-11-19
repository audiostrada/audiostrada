//! The update window: brings the version the update check found, the way this copy was
//! installed. On Windows and macOS it downloads the installer, checks it against the release's
//! SHA-256 sums and, when this copy is signed, checks that the same publisher signed it, then
//! starts it. The AppImage is replaced (or installed) the same way, then started again. The
//! Linux packages come with the system's updates once their repository is added.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use eframe::egui;
use sha2::{Digest, Sha256};

use crate::dialog;
use crate::i18n::tr;
use crate::theme;
use crate::trf;
use crate::updates::{Install, Release};

/// What the application does once the window is answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The installer runs, or the new AppImage once AudioStrada has closed: it closes, asking
    /// to save the projects.
    Quit,
}

#[derive(Debug, Default)]
struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Idle,
    Downloading(Arc<Progress>, Receiver<Result<PathBuf, String>>),
    /// Downloaded and checked.
    Ready(PathBuf),
    Failed(String),
}

#[derive(Debug, Default)]
pub struct UpdateWindow {
    open: bool,
    state: State,
}

impl UpdateWindow {
    pub fn open(&mut self) {
        self.open = true;
    }

    /// `coverage` says whether the license covers the new version.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        release: &Release,
        coverage: &str,
    ) -> Option<Outcome> {
        if !self.open {
            return None;
        }
        if let State::Downloading(_, rx) = &self.state {
            match rx.try_recv() {
                Ok(Ok(path)) => self.state = State::Ready(path),
                Ok(Err(error)) => self.state = State::Failed(error),
                Err(TryRecvError::Disconnected) => {
                    self.state = State::Failed(tr("The download stopped").into())
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(250)),
            }
        }
        let install = crate::updates::install();
        let title = trf!("Version {version} available", version = release.version);
        let mut open = true;
        let mut close = false;
        let mut download = false;
        let mut browser = false;
        let mut run = false;
        dialog::Dialog::new("update", &title)
            .detail(coverage)
            .width(500.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => self.body(ui, install),
                dialog::Part::Actions => dialog::buttons(ui, |ui| match (&self.state, install) {
                    (_, Install::Deb(true) | Install::Rpm(true)) => {
                        close = dialog::primary(ui, tr("Close"), true).clicked();
                    }
                    (_, Install::Deb(false) | Install::Rpm(false)) => {
                        browser = dialog::primary(ui, tr("Download"), true).clicked();
                        close = dialog::secondary(ui, tr("Later")).clicked();
                    }
                    (_, Install::Store | Install::Other) => {
                        browser = dialog::primary(ui, tr("Open the download page"), true).clicked();
                        close = dialog::secondary(ui, tr("Later")).clicked();
                    }
                    (State::Downloading(..), _) => {
                        close = dialog::secondary(ui, tr("Cancel")).clicked();
                    }
                    (State::Ready(_), _) => {
                        let text = match install {
                            Install::Windows => tr("Install now"),
                            Install::AppImage => tr("Update and restart"),
                            _ => tr("Open it and quit"),
                        };
                        run = dialog::primary(ui, text, true).clicked();
                        close = dialog::secondary(ui, tr("Later")).clicked();
                    }
                    (State::Idle | State::Failed(_), _) => {
                        let text = if matches!(self.state, State::Failed(_)) {
                            tr("Try again")
                        } else {
                            tr("Download and install")
                        };
                        download = dialog::primary(ui, text, true).clicked();
                        browser = dialog::secondary(ui, tr("Download in the browser")).clicked();
                    }
                }),
            });
        if download && let Some(file) = install.file() {
            self.start(file, release.version.clone());
        }
        if browser {
            let url = match install.file() {
                Some(file) => audiostrada_license::link(&format!("get/{file}")),
                None => release.url.clone(),
            };
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        let mut outcome = None;
        if run && let State::Ready(path) = &self.state {
            let started = if install == Install::AppImage {
                crate::appimage::update(path).map(crate::appimage::restart_with)
            } else {
                launch(path)
            };
            match started {
                Ok(()) => {
                    outcome = Some(Outcome::Quit);
                    close = true;
                }
                Err(error) => self.state = State::Failed(error),
            }
        }
        if !open || close {
            self.open = false;
            if let State::Downloading(progress, _) = &self.state {
                progress.cancel.store(true, Ordering::Relaxed);
                self.state = State::Idle;
            }
        }
        outcome
    }

    fn body(&self, ui: &mut egui::Ui, install: Install) {
        let command = match install {
            Install::Deb(true) => Some("sudo apt update && sudo apt upgrade"),
            Install::Rpm(true) => Some("sudo dnf upgrade --refresh audiostrada"),
            _ => None,
        };
        if let Some(command) = command {
            dialog::hint(
                ui,
                tr(
                    "Your system installs it with its other updates (the software updater), or in \
                    a terminal:",
                ),
            );
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(command).monospace());
                if ui.small_button(tr("Copy")).clicked() {
                    ui.ctx().copy_text(command.to_owned());
                }
            });
            return;
        }
        let hint = match install {
            Install::Deb(_) | Install::Rpm(_) => tr(
                "The new package also adds AudioStrada's package repository: later versions then \
                 come with your system's updates.",
            ),
            Install::AppImage => match crate::appimage::state() {
                Some(crate::appimage::State::Loose) => tr(
                    "AudioStrada downloads the new AppImage and checks it, then installs it: in \
                     your programs, the applications menu, and for .strada files. It then \
                     starts again, asking to save your projects. You can delete this copy.",
                ),
                _ => tr(
                    "AudioStrada downloads the new AppImage and checks it, replaces this one with \
                     it, then starts again, asking to save your projects.",
                ),
            },
            Install::Windows => tr(
                "AudioStrada downloads the installer and checks it. It then closes, asking to save \
                 your projects, and the installer starts.",
            ),
            Install::MacOs => tr(
                "AudioStrada downloads the disk image and checks it. In its window, drag \
                 AudioStrada onto Applications, replacing this version, then start it again.",
            ),
            Install::Store | Install::Other => tr("The download page has every package."),
        };
        dialog::hint(ui, hint);
        match &self.state {
            State::Downloading(progress, _) => {
                let done = progress.done.load(Ordering::Relaxed) as f32 / 1e6;
                let total = progress.total.load(Ordering::Relaxed) as f32 / 1e6;
                let text = trf!(
                    "{done} / {total} MB",
                    done = format!("{done:.0}"),
                    total = format!("{total:.0}")
                );
                let fraction = if total > 0.0 { done / total } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).text(text));
            }
            State::Ready(_) => {
                ui.label(tr("Downloaded and checked."));
            }
            State::Failed(error) => {
                ui.colored_label(theme::RECORD, error);
            }
            State::Idle => {}
        }
    }

    fn start(&mut self, file: &'static str, version: String) {
        let progress = Arc::new(Progress::default());
        let (tx, rx) = channel();
        let shared = progress.clone();
        std::thread::spawn(move || {
            let _ = tx.send(fetch(file, &version, &shared));
        });
        self.state = State::Downloading(progress, rx);
    }
}

/// Downloads the latest release's `file` and checks it; its path once checked.
fn fetch(file: &str, version: &str, progress: &Progress) -> Result<PathBuf, String> {
    let unreachable = || tr("Cannot reach the web site").to_owned();
    let agent = crate::tone3000::agent_with_timeout(Duration::from_secs(3600));
    let get = |page: &str| {
        agent
            .get(&audiostrada_license::link(page))
            .call()
            .ok()
            .filter(|r| r.status().is_success())
    };
    let sums = get("get/checksums")
        .ok_or_else(unreachable)?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let response = get(&format!("get/{file}")).ok_or_else(unreachable)?;
    let total = response.body().content_length().unwrap_or(0);
    progress.total.store(total, Ordering::Relaxed);
    let dir = std::env::temp_dir().join("AudioStrada-update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let partial = dir.join("download.part");
    let mut out = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0; 1 << 16];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err(tr("Download cancelled").into());
        }
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buffer[..n]);
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    drop(out);
    let hash = audiostrada_license::to_hex(&hasher.finalize());
    let name = listed(&sums, &hash)
        .filter(|name| name.contains(version) && !name.contains(['/', '\\']))
        .ok_or_else(|| tr("The download does not match the release: it was not kept"))?;
    let path = dir.join(name);
    std::fs::rename(&partial, &path).map_err(|e| e.to_string())?;
    if !signed_like_this_copy(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(
            tr("The installer is not signed by AudioStrada's publisher: it was not kept").into(),
        );
    }
    Ok(path)
}

/// The file name `SHA256SUMS` gives to `hash`.
fn listed<'a>(sums: &'a str, hash: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (sum, name) = line.split_once(char::is_whitespace)?;
        sum.eq_ignore_ascii_case(hash)
            .then(|| name.trim().trim_start_matches('*'))
    })
}

/// Whether the same publisher signed `installer` and this copy; true when this copy is not
/// signed, the checksums being then the only check.
fn signed_like_this_copy(installer: &Path) -> bool {
    let Some(publisher) = std::env::current_exe().ok().and_then(|exe| signer(&exe)) else {
        return true;
    };
    signer(installer).as_deref() == Some(publisher.as_str())
}

/// Who signed `path`, when its signature holds: the team on macOS.
#[cfg(target_os = "macos")]
fn signer(path: &Path) -> Option<String> {
    use std::process::Command;
    let valid = Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(path)
        .status()
        .ok()?
        .success();
    if !valid {
        return None;
    }
    let out = Command::new("codesign")
        .arg("-dv")
        .arg(path)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find_map(|l| l.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_owned)
}

/// Who signed `path`, when its signature holds: the certificate's subject on Windows.
#[cfg(windows)]
fn signer(path: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // The path goes through the environment: no quoting to get wrong.
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$s = Get-AuthenticodeSignature -LiteralPath $env:AUDIOSTRADA_SIGNED; \
             if ($s.Status -eq 'Valid') { $s.SignerCertificate.Subject }",
        ])
        .env("AUDIOSTRADA_SIGNED", path)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let subject = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!subject.is_empty()).then_some(subject)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn signer(_: &Path) -> Option<String> {
    None
}

/// Starts the installer (Windows asks for the administrator's rights) or opens the disk image.
fn launch(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        let file = HSTRING::from(path);
        // SAFETY: the strings live through the call.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(file.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Above 32 means it started.
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(tr("Cannot start the installer").into())
        }
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_download_is_known_by_its_checksum() {
        let sums = "AB12  AudioStrada-1.0.0-x64-setup.exe\ncd34 *AudioStrada-1.0.0.dmg\n";
        assert_eq!(
            listed(sums, "ab12"),
            Some("AudioStrada-1.0.0-x64-setup.exe")
        );
        assert_eq!(listed(sums, "CD34"), Some("AudioStrada-1.0.0.dmg"));
        assert_eq!(listed(sums, "ef56"), None);
    }
}
