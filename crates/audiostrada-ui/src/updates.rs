//! New versions of the official builds, checked once a day when the user agreed. Stores update
//! their copies themselves; elsewhere the update window brings the new version.

use std::sync::mpsc::{Receiver, channel};

use serde::Deserialize;

use crate::ui_prefs::UiPrefs;

/// What `releases/latest.json` on the web site describes.
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub version: String,
    /// Release date (YYYY-MM-DD), compared with the license's updates.
    pub date: String,
    /// Download page.
    pub url: String,
}

/// Official builds look for new versions, unless Flathub or the Snap Store installed them.
pub fn offered() -> bool {
    audiostrada_license::OFFICIAL && install() != Install::Store
}

/// How this copy was installed, which decides how a new version reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    /// The Windows installer.
    Windows,
    /// The macOS disk image.
    MacOs,
    /// The .deb package; true when it added AudioStrada's APT repository.
    Deb(bool),
    /// The .rpm package; true when it added AudioStrada's RPM repository.
    Rpm(bool),
    AppImage,
    /// Flathub or the Snap Store, which update it themselves.
    Store,
    /// Built from source, or unknown.
    Other,
}

pub fn install() -> Install {
    use std::path::Path;
    if cfg!(windows) {
        return Install::Windows;
    }
    if cfg!(target_os = "macos") {
        return Install::MacOs;
    }
    let var = |name| std::env::var_os(name).is_some();
    if var("FLATPAK_ID") || var("SNAP") {
        return Install::Store;
    }
    if var("APPIMAGE") {
        return Install::AppImage;
    }
    let packaged = std::env::current_exe().is_ok_and(|exe| exe.starts_with("/usr/"));
    if packaged && Path::new("/var/lib/dpkg/info/audiostrada.list").exists() {
        return Install::Deb(Path::new("/etc/apt/sources.list.d/audiostrada.sources").exists());
    }
    if packaged && Path::new("/var/lib/rpm").exists() {
        return Install::Rpm(Path::new("/etc/yum.repos.d/audiostrada.repo").exists());
    }
    Install::Other
}

impl Install {
    /// This copy's installer among the latest release's files, as the site's `/get/` names it.
    pub fn file(self) -> Option<&'static str> {
        match self {
            Install::Windows => Some("windows"),
            Install::MacOs => Some("macos"),
            Install::Deb(_) => Some("deb"),
            Install::Rpm(_) => Some("rpm"),
            Install::AppImage => Some("appimage"),
            Install::Store | Install::Other => None,
        }
    }
}

impl Install {
    /// The system brings new versions by itself, whatever the license covers.
    pub fn automatic(self) -> bool {
        matches!(
            self,
            Install::Deb(true) | Install::Rpm(true) | Install::Store
        )
    }

    /// The command that keeps this version when the system updates.
    pub fn hold_command(self) -> Option<&'static str> {
        match self {
            Install::Deb(true) => Some("sudo apt-mark hold audiostrada"),
            Install::Rpm(true) => Some("sudo dnf versionlock add audiostrada"),
            Install::Store if flatpak() => Some("flatpak mask com.audiostrada.AudioStrada"),
            Install::Store => Some("sudo snap refresh --hold audiostrada"),
            _ => None,
        }
    }

    /// The command that goes back to `version` and keeps it; Flathub has none by version.
    pub fn back_command(self, version: &str) -> Option<String> {
        match self {
            Install::Deb(true) => Some(format!(
                "sudo apt install --allow-downgrades audiostrada={version}-1 && sudo apt-mark \
                 hold audiostrada"
            )),
            Install::Rpm(true) => Some(format!(
                "sudo dnf downgrade audiostrada-{version} && sudo dnf versionlock add audiostrada"
            )),
            // The Snap Store keeps the previous version on the computer.
            Install::Store if !flatpak() => {
                Some("sudo snap revert audiostrada && sudo snap refresh --hold audiostrada".into())
            }
            _ => None,
        }
    }
}

fn flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some()
}

/// A published version, as `releases/index.json` on the web site lists them.
#[derive(Clone, Debug, Deserialize)]
pub struct Published {
    pub version: String,
    /// Release date (YYYY-MM-DD).
    pub date: String,
}

/// The latest version released by `until`, the last day of a license's updates.
pub fn last_covered(published: &[Published], until: &str) -> Option<String> {
    published
        .iter()
        .filter(|p| p.date.as_str() <= until)
        .map(|p| p.version.as_str())
        .reduce(|a, b| if newer(b, a) { b } else { a })
        .map(str::to_owned)
}

/// The versions the web site lists; `None` when it cannot be reached.
pub fn published() -> Option<Vec<Published>> {
    if cfg!(test) {
        return None;
    }
    let url = audiostrada_license::link("releases/index.json");
    let mut response = crate::tone3000::agent().get(&url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    serde_json::from_str(&response.body_mut().read_to_string().ok()?).ok()
}

#[derive(Debug, Default)]
pub struct Updates {
    pending: Option<Receiver<Option<Release>>>,
    /// Checked (or found not due) in this session.
    done: bool,
    /// A newer version than this one.
    pub found: Option<Release>,
}

impl Updates {
    /// Starts the daily check if it is due; true when `prefs` changed.
    pub fn start(&mut self, prefs: &mut UiPrefs) -> bool {
        if self.done || !offered() || prefs.check_updates != Some(true) {
            return false;
        }
        self.done = true;
        let today = audiostrada_license::today();
        if prefs.updates_checked.as_deref() == Some(today.as_str()) {
            return false;
        }
        prefs.updates_checked = Some(today);
        let (tx, rx) = channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(fetch());
        });
        true
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &self.pending
            && let Ok(release) = rx.try_recv()
        {
            self.pending = None;
            self.found = release.filter(|r| newer(&r.version, env!("CARGO_PKG_VERSION")));
        }
    }
}

fn fetch() -> Option<Release> {
    let url = audiostrada_license::link("releases/latest.json");
    let mut response = crate::tone3000::agent().get(&url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let text = response.body_mut().read_to_string().ok()?;
    let release: Release = serde_json::from_str(&text).ok()?;
    // Its page opens in the browser: only one of the site's.
    release
        .url
        .starts_with(&format!("{}/", audiostrada_license::SITE))
        .then_some(release)
}

/// `version` comes after `current` (dotted numbers, as 1.2.10).
pub fn newer(version: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-'])
            .map_while(|p| p.parse().ok())
            .collect()
    };
    parts(version) > parts(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("v1.10.0", "1.9.3"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn the_last_version_covered_is_the_latest_released_by_the_end_of_the_updates() {
        let published = |list: &[(&str, &str)]| -> Vec<Published> {
            list.iter()
                .map(|(version, date)| Published {
                    version: (*version).into(),
                    date: (*date).into(),
                })
                .collect()
        };
        let list = published(&[
            ("1.10.0", "2027-06-01"),
            ("1.9.0", "2027-03-01"),
            ("1.2.0", "2027-01-15"),
            ("1.0.0", "2026-12-01"),
        ]);
        assert_eq!(last_covered(&list, "2027-03-01").as_deref(), Some("1.9.0"));
        assert_eq!(last_covered(&list, "2027-12-31").as_deref(), Some("1.10.0"));
        assert_eq!(last_covered(&list, "2026-11-30"), None);
    }

    #[test]
    fn going_back_names_the_version_for_the_package_manager() {
        let deb = Install::Deb(true).back_command("1.2.0").unwrap();
        assert!(deb.contains("audiostrada=1.2.0-1") && deb.ends_with("apt-mark hold audiostrada"));
        let rpm = Install::Rpm(true).back_command("1.2.0").unwrap();
        assert!(rpm.starts_with("sudo dnf downgrade audiostrada-1.2.0 "));
        assert_eq!(Install::Deb(false).back_command("1.2.0"), None);
        assert!(!Install::Windows.automatic() && Install::Rpm(true).automatic());
    }
}
