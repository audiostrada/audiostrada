//! Translations. The English text is the key: `tr("Add track")` gives it in the chosen
//! language, `trf!("{n} tracks", n = 3)` fills named values in. A text missing from a catalog
//! stays in English.
//!
//! Catalogs are text files in `assets/i18n/<language>/`, one per part of the interface: an
//! English line, its translation on the next, a blank line between entries; `\n` stands for a
//! line break and lines starting with `#` are comments.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    /// The system's language when AudioStrada has it, else English.
    #[default]
    Auto,
    English,
    French,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Auto, Language::English, Language::French];

    /// The name shown in the language list, in that language.
    pub fn name(self) -> &'static str {
        match self {
            Language::Auto => tr("Automatic"),
            Language::English => "English",
            Language::French => "Français",
        }
    }
}

const ENGLISH: u8 = 0;
const FRENCH: u8 = 1;

static CURRENT: AtomicU8 = AtomicU8::new(ENGLISH);

/// French catalog files, one per part of the interface.
const FRENCH_FILES: &[&str] = &[
    include_str!("../assets/i18n/fr/app.txt"),
    include_str!("../assets/i18n/fr/mixer.txt"),
    include_str!("../assets/i18n/fr/timeline.txt"),
    include_str!("../assets/i18n/fr/effects.txt"),
    include_str!("../assets/i18n/fr/amp.txt"),
    include_str!("../assets/i18n/fr/windows.txt"),
    include_str!("../assets/i18n/fr/messages.txt"),
    include_str!("../assets/i18n/fr/piano.txt"),
];

static FRENCH_CATALOG: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| parse(FRENCH_FILES).0);

/// Entries of catalog files, and the keys given two different translations.
fn parse(files: &[&'static str]) -> (HashMap<&'static str, &'static str>, Vec<&'static str>) {
    let mut map = HashMap::new();
    let mut conflicts = Vec::new();
    for file in files {
        let lines: Vec<&'static str> = file.lines().filter(|l| !l.starts_with('#')).collect();
        for entry in lines.split(|l| l.trim().is_empty()) {
            if let [key, value] = entry
                && let Some(old) = map.insert(*key, *value)
                && old != *value
            {
                conflicts.push(*key);
            }
        }
    }
    (map, conflicts)
}

/// Chooses the language of the interface.
pub fn set_language(language: Language) {
    let french = match language {
        Language::French => true,
        Language::English => false,
        Language::Auto => ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
            .iter()
            .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
            .is_some_and(|l| l.starts_with("fr")),
    };
    CURRENT.store(if french { FRENCH } else { ENGLISH }, Ordering::Relaxed);
}

/// `text` in the chosen language.
pub fn tr(text: &'static str) -> &'static str {
    if CURRENT.load(Ordering::Relaxed) == ENGLISH {
        return text;
    }
    french(text).unwrap_or(text)
}

fn french(text: &str) -> Option<&'static str> {
    FRENCH_CATALOG
        .get(escape(text).as_str())
        .map(|t| unescape(t))
}

/// Catalog keys write line breaks as `\n`.
fn escape(text: &str) -> String {
    text.replace('\n', "\\n")
}

fn unescape(text: &'static str) -> &'static str {
    if !text.contains("\\n") {
        return text;
    }
    static CACHE: LazyLock<std::sync::Mutex<HashMap<&'static str, &'static str>>> =
        LazyLock::new(Default::default);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(text)
        .or_insert_with(|| Box::leak(text.replace("\\n", "\n").into_boxed_str()))
}

/// Whether a message reports a failure, in English or in French ("Cannot…", "Impossible
/// de…", "… failed", "Échec…").
pub fn is_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["cannot", "impossible"]
        .iter()
        .any(|w| lower.starts_with(w))
        || ["failed", "error", "échec", "échoué", "erreur"]
            .iter()
            .any(|w| lower.contains(w))
}

/// Puts `values` in place of their `{name}` in a translated text.
pub fn fill(text: &str, values: &[(&str, String)]) -> String {
    let mut out = text.to_owned();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// A translated text with named values: `trf!("{n} tracks", n = count)`.
#[macro_export]
macro_rules! trf {
    ($text:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::fill(
            $crate::i18n::tr($text),
            &[$((stringify!($name), ($value).to_string())),*],
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texts given to `tr` and `trf!` in the sources.
    fn keys_in_sources() -> Vec<(String, String)> {
        let mut keys = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("i18n.rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
                for start in ["tr(", "trf!("] {
                    for (at, _) in source.match_indices(start) {
                        // The text may be on the next line, as rustfmt puts long ones.
                        let after = source[at + start.len()..].trim_start();
                        let Some(rest) = after.strip_prefix('"') else {
                            continue;
                        };
                        if source[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        let mut key = String::new();
                        let mut chars = rest.chars();
                        while let Some(c) = chars.next() {
                            match c {
                                '"' => break,
                                '\\' => match chars.next() {
                                    Some('n') => key.push_str("\\n"),
                                    Some('\n') => {
                                        // A line continuation: the next line's indent is dropped.
                                        let tail: String = chars.clone().collect();
                                        let skip = tail.len() - tail.trim_start().len();
                                        for _ in 0..skip {
                                            chars.next();
                                        }
                                    }
                                    Some(other) => key.push(other),
                                    None => {}
                                },
                                c => key.push(c),
                            }
                        }
                        let file = path.file_name().unwrap().to_string_lossy().into_owned();
                        keys.push((file, key));
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn every_text_has_its_french() {
        let (catalog, conflicts) = parse(FRENCH_FILES);
        assert!(
            conflicts.is_empty(),
            "translated twice differently: {conflicts:?}"
        );
        let mut missing: Vec<String> = keys_in_sources()
            .into_iter()
            .filter(|(_, key)| !catalog.contains_key(key.as_str()))
            .map(|(file, key)| format!("{file}: {key}"))
            .collect();
        for info in audiostrada_dsp::EFFECTS {
            let texts = [info.name, info.description, info.category]
                .into_iter()
                .chain(info.params.iter().map(|p| p.name))
                .chain(info.params.iter().flat_map(|p| p.choices.iter().copied()));
            for text in texts {
                if !catalog.contains_key(escape(text).as_str()) {
                    missing.push(format!("{}: {text}", info.id));
                }
            }
        }
        for action in crate::actions::Action::ALL {
            for text in [action.name(), action.category()] {
                if !catalog.contains_key(text) {
                    missing.push(format!("action: {text}"));
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "{} without French:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    #[test]
    fn texts_are_looked_up_and_filled() {
        assert_eq!(french("Automatic"), Some("Automatique"));
        assert_eq!(french("not in any catalog"), None);
        assert_eq!(fill("{n} pistes", &[("n", "3".into())]), "3 pistes");
    }

    #[test]
    fn errors_are_told_from_messages() {
        assert!(is_error("Cannot load x"));
        assert!(is_error("Impossible de charger x"));
        assert!(is_error("Export failed"));
        assert!(is_error("Échec de l'export"));
        assert!(!is_error("Project saved"));
        assert_eq!(unescape("a\\nb"), "a\nb");
        assert_eq!(escape("a\nb"), "a\\nb");
    }
}
