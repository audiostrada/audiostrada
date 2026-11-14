//! Favorite effects and presets, and the effects added lately, kept in `favorites.json`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How many effects "Recent" keeps.
const RECENT: usize = 12;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Favorites {
    /// Effect keys.
    pub effects: BTreeSet<String>,
    /// Preset names, by effect key.
    pub presets: BTreeMap<String, BTreeSet<String>>,
    /// Effect keys, the latest first.
    pub recent: Vec<String>,
}

impl Favorites {
    pub fn preset(&self, effect: &str, name: &str) -> bool {
        self.presets.get(effect).is_some_and(|s| s.contains(name))
    }

    pub fn toggle_preset(&mut self, effect: &str, name: &str) {
        let set = self.presets.entry(effect.to_owned()).or_default();
        if !set.remove(name) {
            set.insert(name.to_owned());
        }
        if set.is_empty() {
            self.presets.remove(effect);
        }
    }

    pub fn toggle_effect(&mut self, key: &str) {
        if !self.effects.remove(key) {
            self.effects.insert(key.to_owned());
        }
    }

    /// An effect was just added.
    pub fn used(&mut self, key: &str) {
        self.recent.retain(|k| k != key);
        self.recent.insert(0, key.to_owned());
        self.recent.truncate(RECENT);
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Favorites>> = const { RefCell::new(None) };
}

fn file() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("audiostrada").join("favorites.json"))
}

fn load() -> Favorites {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Reads the favorites, loaded once.
pub fn with<R>(f: impl FnOnce(&Favorites) -> R) -> R {
    CURRENT.with_borrow_mut(|c| f(c.get_or_insert_with(load)))
}

/// Changes the favorites and saves them.
pub fn change(f: impl FnOnce(&mut Favorites)) {
    CURRENT.with_borrow_mut(|c| {
        let favorites = c.get_or_insert_with(load);
        f(favorites);
        if let Some(file) = file() {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(favorites) {
                let _ = std::fs::write(file, json);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_toggle_and_recent_effects_come_first() {
        let mut f = Favorites::default();
        f.toggle_preset("compressor", "Vocal: Gentle");
        assert!(f.preset("compressor", "Vocal: Gentle"));
        f.toggle_preset("compressor", "Vocal: Gentle");
        assert!(f.presets.is_empty());
        for key in ["eq", "reverb", "eq"] {
            f.used(key);
        }
        assert_eq!(f.recent, ["eq", "reverb"]);
        for i in 0..20 {
            f.used(&i.to_string());
        }
        assert_eq!(f.recent.len(), RECENT);
        assert_eq!(f.recent[0], "19");
    }
}
