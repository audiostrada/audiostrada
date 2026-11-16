//! Checks that every interface text, AudioStrada's and its editors', has its French.

use crate::i18n::{catalog_key, french_catalog};

/// Texts given to `tr` and `trf!` in the sources.
fn keys_in_sources() -> Vec<(String, String)> {
    let mut keys = Vec::new();
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut stack = vec![
        crate_dir.join("src"),
        crate_dir.join("../audiostrada-editors/src"),
    ];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs")
                || path.ends_with("i18n.rs")
                || path.ends_with("i18n_check.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
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
    let (catalog, conflicts) = french_catalog();
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
            if !catalog.contains_key(catalog_key(text).as_str()) {
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
