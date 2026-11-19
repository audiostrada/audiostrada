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
}
