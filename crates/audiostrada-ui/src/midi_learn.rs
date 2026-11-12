//! The MIDI controllers learned on effect parameters, kept in `midi_learn.json` for every
//! project: a controller learned on Strada-Synth's first macro turns that macro wherever a
//! Strada-Synth plays, on the armed tracks (else on the tracks selected).

use std::cell::RefCell;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A controller and the parameter of a kind of effect it turns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    /// MIDI channel (0 to 15) and controller number.
    pub channel: u8,
    pub controller: u8,
    /// The effect's key (`synth`, `clap:org.example.reverb`...) and the parameter's id.
    pub effect: String,
    pub param: String,
    /// The values 0 and 127 of the controller give.
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub learned: Vec<Learned>,
}

impl Store {
    /// Learns a controller: it turns this parameter only, and the parameter has this
    /// controller only.
    pub fn learn(&mut self, learned: Learned) {
        self.forget_controller(learned.channel, learned.controller);
        self.forget(&learned.effect, &learned.param);
        self.learned.push(learned);
    }

    pub fn forget(&mut self, effect: &str, param: &str) {
        self.learned
            .retain(|l| !(l.effect == effect && l.param == param));
    }

    pub fn forget_controller(&mut self, channel: u8, controller: u8) {
        self.learned
            .retain(|l| !(l.channel == channel && l.controller == controller));
    }

    /// The controller of a parameter: (channel, number).
    pub fn of(&self, effect: &str, param: &str) -> Option<(u8, u8)> {
        self.learned
            .iter()
            .find(|l| l.effect == effect && l.param == param)
            .map(|l| (l.channel, l.controller))
    }

    /// What a controller turns.
    pub fn turned_by(&self, channel: u8, controller: u8) -> Vec<Learned> {
        self.learned
            .iter()
            .filter(|l| l.channel == channel && l.controller == controller)
            .cloned()
            .collect()
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Store>> = const { RefCell::new(None) };
}

fn file() -> Option<PathBuf> {
    // Tests keep theirs in memory, each on its own thread.
    if cfg!(test) {
        return None;
    }
    crate::config_dir().map(|d| d.join("audiostrada").join("midi_learn.json"))
}

fn load() -> Store {
    file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Reads what was learned, loaded once.
pub fn with<R>(f: impl FnOnce(&Store) -> R) -> R {
    CURRENT.with_borrow_mut(|c| f(c.get_or_insert_with(load)))
}

/// Changes what was learned and saves it.
pub fn change(f: impl FnOnce(&mut Store)) {
    CURRENT.with_borrow_mut(|c| {
        let store = c.get_or_insert_with(load);
        f(store);
        if let Some(file) = file() {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(store) {
                let _ = std::fs::write(file, json);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn learned(controller: u8, param: &str) -> Learned {
        Learned {
            channel: 0,
            controller,
            effect: "synth".into(),
            param: param.into(),
            min: 0.0,
            max: 100.0,
        }
    }

    #[test]
    fn a_controller_turns_one_parameter_and_a_parameter_has_one_controller() {
        let mut store = Store::default();
        store.learn(learned(20, "macro1"));
        store.learn(learned(21, "macro2"));
        assert_eq!(store.of("synth", "macro1"), Some((0, 20)));
        // CC 20 learned again elsewhere leaves macro 1.
        store.learn(learned(20, "macro3"));
        assert_eq!(store.of("synth", "macro1"), None);
        assert_eq!(store.turned_by(0, 20)[0].param, "macro3");
        // Macro 2 learned again keeps one controller.
        store.learn(learned(22, "macro2"));
        assert!(store.turned_by(0, 21).is_empty());
        store.forget("synth", "macro2");
        assert_eq!(store.learned.len(), 1);
    }
}
