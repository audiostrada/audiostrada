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
    include_str!("../../audiostrada-ui/assets/i18n/fr/app.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/mixer.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/timeline.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/effects.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/amp.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/windows.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/messages.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/piano.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/drums.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/dialogs.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/dialogs-a.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/dialogs-b.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/dialogs-c.txt"),
    include_str!("../../audiostrada-ui/assets/i18n/fr/audio-editor.txt"),
];

static FRENCH_CATALOG: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| parse(FRENCH_FILES).0);

/// Texts of a program sharing these catalogs (the plugins), looked up after them.
static MORE_FRENCH: std::sync::OnceLock<HashMap<&'static str, &'static str>> =
    std::sync::OnceLock::new();

/// Adds catalog files of the same format, for texts AudioStrada does not have; once.
pub fn add_french(files: &[&'static str]) {
    let _ = MORE_FRENCH.set(parse(files).0);
}

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

/// The French catalog and the texts it translates twice differently, for AudioStrada's check
/// that every text has its French.
pub fn french_catalog() -> (HashMap<&'static str, &'static str>, Vec<&'static str>) {
    parse(FRENCH_FILES)
}

/// The entries of catalog files, and the keys given two different translations.
pub fn parse_catalog(
    files: &[&'static str],
) -> (HashMap<&'static str, &'static str>, Vec<&'static str>) {
    parse(files)
}

/// How `text` is written as a catalog key.
pub fn catalog_key(text: &str) -> String {
    escape(text)
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
    let key = escape(text);
    FRENCH_CATALOG
        .get(key.as_str())
        .or_else(|| MORE_FRENCH.get()?.get(key.as_str()))
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
