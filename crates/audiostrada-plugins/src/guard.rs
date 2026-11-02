//! Plugins that crashed AudioStrada while loading in its own process (sandbox off): the plugin
//! being loaded is written down, and found again at the next start if the load never ended.
//! Such plugins are refused in this process from then on; the sandbox still loads them.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::PluginDescriptor;

struct Guard {
    dir: PathBuf,
    /// Keys of the plugins that crashed a load.
    crashed: Vec<String>,
    /// The plugin that crashed the last run, until told to the user.
    report: Option<String>,
}

static GUARD: Mutex<Option<Guard>> = Mutex::new(None);

const LOADING: &str = "plugin-loading";
const CRASHED: &str = "plugins-crashed";

/// Keeps the guard's files in `dir`. Returns the name of the plugin that crashed the last run
/// while loading, if one did.
pub fn set_dir(dir: PathBuf) -> Option<String> {
    let mut crashed: Vec<String> = std::fs::read_to_string(dir.join(CRASHED))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let mut name = None;
    if let Ok(loading) = std::fs::read_to_string(dir.join(LOADING)) {
        let _ = std::fs::remove_file(dir.join(LOADING));
        let mut lines = loading.lines();
        if let Some(key) = lines.next().filter(|k| !k.is_empty()) {
            tracing::warn!("plugin {key} crashed AudioStrada while loading, now refused");
            if !crashed.iter().any(|c| c == key) {
                crashed.push(key.to_owned());
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join(CRASHED), crashed.join("\n"));
            }
            name = Some(lines.next().unwrap_or(key).to_owned());
        }
    }
    if let Ok(mut guard) = GUARD.lock() {
        *guard = Some(Guard {
            dir,
            crashed,
            report: name.clone(),
        });
    }
    name
}

/// The name of the plugin that crashed the last run while loading, once.
pub fn take_report() -> Option<String> {
    GUARD
        .lock()
        .ok()
        .and_then(|mut g| g.as_mut().and_then(|g| g.report.take()))
}

/// True when the plugin crashed AudioStrada while loading in its process.
pub fn crashed(descriptor: &PluginDescriptor) -> bool {
    let key = descriptor.key();
    GUARD
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|g| g.crashed.contains(&key)))
        .unwrap_or(false)
}

/// Marks `descriptor` as loading in this process until dropped; nothing when sandboxed.
#[must_use]
pub fn loading(descriptor: &PluginDescriptor) -> Loading {
    if crate::sandbox_host().is_some() {
        return Loading(None);
    }
    let Some(dir) = GUARD
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|g| g.dir.clone()))
    else {
        return Loading(None);
    };
    let file = dir.join(LOADING);
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&file, format!("{}\n{}", descriptor.key(), descriptor.name)));
    Loading(written.is_ok().then_some(file))
}

/// A load in progress, see [`loading`].
#[derive(Debug)]
pub struct Loading(Option<PathBuf>);

impl Drop for Loading {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let _ = std::fs::remove_file(file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Format;

    #[test]
    fn a_load_that_never_ended_is_refused_next_time() {
        let dir = std::env::temp_dir().join(format!("audiostrada-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let descriptor = PluginDescriptor {
            format: Format::Clap,
            id: "com.example.crash".into(),
            name: "Crash".into(),
            vendor: String::new(),
            version: String::new(),
            path: PathBuf::new(),
            instrument: false,
            tags: Vec::new(),
        };
        assert_eq!(set_dir(dir.clone()), None);
        drop(loading(&descriptor));
        assert_eq!(set_dir(dir.clone()), None, "a load that ended is forgotten");
        std::mem::forget(loading(&descriptor));
        assert_eq!(set_dir(dir.clone()).as_deref(), Some("Crash"));
        assert!(crashed(&descriptor));
        assert_eq!(take_report().as_deref(), Some("Crash"));
        assert_eq!(take_report(), None);
        assert_eq!(set_dir(dir.clone()), None, "reported once");
        assert!(crashed(&descriptor), "and still refused");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
