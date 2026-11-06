//! Library of amp and pedal captures (.nam) and cabinet impulse responses (.wav) found in the
//! user's folders, described by gear type with their maker, model and author, for the Amp
//! editor. Folders are remembered in the configuration folder.

use std::path::{Path, PathBuf};

use crate::trf;

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub family: &'static str,
    pub title: String,
    pub detail: String,
    /// NAM's estimate of the capture's gain, 0 to 1.
    pub gain: Option<f32>,
}

/// `name` made safe as a file or folder name, `fallback` when nothing is left.
pub(crate) fn file_name(name: &str, fallback: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.');
    if clean.is_empty() {
        fallback.to_owned()
    } else {
        clean.to_owned()
    }
}

/// Where downloaded captures go: the library folder in the configuration folder.
fn download_root() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("audiostrada").join("nam"))
}

/// The download folder, added to the library folders if needed.
pub(crate) fn library_root() -> Option<PathBuf> {
    let root = download_root()?;
    let mut dirs = load_dirs();
    if !dirs.contains(&root) {
        dirs.insert(0, root.clone());
        save_dirs(&dirs);
    }
    Some(root)
}

pub(crate) fn add_dir(dir: PathBuf) {
    let mut dirs = load_dirs();
    if !dirs.contains(&dir) {
        dirs.push(dir);
        save_dirs(&dirs);
    }
}

fn settings_file() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("audiostrada").join("nam-folders.json"))
}

/// The remembered folders, or the usual places for captures.
pub(crate) fn load_dirs() -> Vec<PathBuf> {
    if let Some(saved) = settings_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str::<Vec<PathBuf>>(&t).ok())
    {
        return saved;
    }
    let mut dirs = Vec::new();
    if let Some(config) = crate::config_dir() {
        dirs.push(config.join("audiostrada").join("nam"));
    }
    for base in [dirs::document_dir(), dirs::home_dir()]
        .into_iter()
        .flatten()
    {
        for name in ["NAM", "Neural Amp Modeler", "TONE3000"] {
            let dir = base.join(name);
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }
    dirs
}

fn save_dirs(dirs: &[PathBuf]) {
    if let Some(file) = settings_file() {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(dirs) {
            let _ = std::fs::write(file, text);
        }
    }
}

/// Family of a capture from NAM's and TONE3000's gear type words.
fn family(gear_type: Option<&str>) -> &'static str {
    match gear_type
        .unwrap_or("")
        .to_lowercase()
        .replace('-', "_")
        .as_str()
    {
        "amp" => "Amps",
        "amp_cab" | "amp_pedal_cab" | "pedal_amp" | "full_rig" => "Full rigs",
        "preamp" => "Preamps",
        "pedal" => "Pedals",
        "studio" | "outboard" => "Outboard",
        "cab" | "ir" => "Cabinets (IR)",
        _ => "Other",
    }
}

/// What a .nam file says about itself (only its metadata block is read).
pub fn describe(path: &Path, text: &str) -> Option<Entry> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let meta = value.get("metadata");
    let field = |key: &str| {
        meta.and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let gear = [field("gear_make"), field("gear_model")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    let name = field("name");
    let title = if gear.is_empty() {
        name.clone().unwrap_or_else(|| stem.clone())
    } else {
        gear
    };
    let mut detail: Vec<String> = Vec::new();
    if let Some(n) = name.filter(|n| *n != title) {
        detail.push(n);
    }
    if let Some(by) = field("modeled_by") {
        detail.push(trf!("by {name}", name = by));
    }
    if detail.is_empty() {
        detail.push(stem);
    }
    Some(Entry {
        path: path.to_path_buf(),
        family: family(field("gear_type").as_deref()),
        title,
        detail: detail.join(" · "),
        gain: meta
            .and_then(|m| m.get("gain"))
            .and_then(|v| v.as_f64())
            .map(|g| g as f32),
    })
}

pub(crate) fn scan(dirs: Vec<PathBuf>) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = dirs.into_iter().map(|d| (d, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in read.flatten() {
            let path = item.path();
            if path.is_dir() {
                let hidden = item.file_name().to_string_lossy().starts_with('.');
                if depth < 8 && !hidden {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            match ext.as_str() {
                "nam" => {
                    if let Some(entry) = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|t| describe(&path, &t))
                    {
                        entries.push(entry);
                    }
                }
                "wav" => entries.push(Entry {
                    title: path
                        .file_stem()
                        .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                    detail: path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                    path,
                    family: "Cabinets (IR)",
                    gain: None,
                }),
                _ => {}
            }
        }
    }
    entries.sort_by_key(|e| e.title.to_lowercase());
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_metadata_of_a_capture() {
        let text = r#"{"version": "0.5.4", "metadata": {"name": "Crunch 7", "modeled_by": "Sam",
            "gear_make": "Marshall", "gear_model": "JCM800", "gear_type": "amp", "gain": 0.6},
            "architecture": "WaveNet", "config": {}, "weights": []}"#;
        let entry = describe(Path::new("/x/crunch.nam"), text).unwrap();
        assert_eq!(entry.title, "Marshall JCM800");
        assert_eq!(entry.detail, "Crunch 7 · by Sam");
        assert_eq!(entry.family, "Amps");
        assert_eq!(entry.gain, Some(0.6));
        let bare = describe(Path::new("/x/my pedal.nam"), r#"{"weights": []}"#).unwrap();
        assert_eq!((bare.title.as_str(), bare.family), ("my pedal", "Other"));
        assert_eq!(family(Some("amp-cab")), "Full rigs");
        assert_eq!(family(Some("pedal")), "Pedals");
    }

    #[test]
    fn names_become_safe_file_names() {
        assert_eq!(
            file_name("Fender / Twin: \"clean\"", "model"),
            "Fender _ Twin_ _clean_"
        );
        assert_eq!(file_name(" ..Plexi (2). ", "model"), "Plexi (2)");
        assert_eq!(file_name("...", "Captures"), "Captures");
    }
}
