//! New versions of the official builds, checked once a day when the user agreed. Nothing is
//! installed: the user is pointed to the download page.

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
        if self.done || !audiostrada_license::OFFICIAL || prefs.check_updates != Some(true) {
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
    serde_json::from_str(&text).ok()
}

/// `version` comes after `current` (dotted numbers, as 1.2.10).
fn newer(version: &str, current: &str) -> bool {
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
