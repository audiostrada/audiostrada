//! The Amp's capture library: the NAM captures of the library folders grouped into packs (the
//! captures of one piece of gear), with the names, categories and favourites the user gave
//! them, and the import of captures into AudioStrada's own library folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::i18n::tr;
use crate::nam_library::Entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Amp,
    FullRig,
    /// Pedals and outboard (preamps, compressors, EQs): they go on the pedalboard.
    #[serde(alias = "Pedal")]
    Effect,
    /// Cabinet impulse responses.
    Cab,
    Other,
}

/// Where captures of a category are loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Amp,
    Pedalboard,
    Cab,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Amp,
        Category::FullRig,
        Category::Effect,
        Category::Cab,
        Category::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Amp => tr("Amps"),
            Category::FullRig => tr("Amps + cab"),
            Category::Effect => tr("Effects"),
            Category::Cab => tr("Cabs (IR)"),
            Category::Other => tr("Other"),
        }
    }

    pub fn from_family(family: &str) -> Category {
        match family {
            "Amps" | "Preamps" => Category::Amp,
            "Full rigs" => Category::FullRig,
            "Pedals" | "Outboard" => Category::Effect,
            "Cabinets (IR)" => Category::Cab,
            _ => Category::Other,
        }
    }

    /// From a TONE3000 gear type.
    pub fn from_gear(gear: &str) -> Category {
        match gear {
            "amp" => Category::Amp,
            "amp-cab" | "full-rig" => Category::FullRig,
            "pedal" | "outboard" => Category::Effect,
            "cab" | "ir" => Category::Cab,
            _ => Category::Other,
        }
    }

    pub fn place(self) -> Place {
        match self {
            Category::Effect => Place::Pedalboard,
            Category::Cab => Place::Cab,
            _ => Place::Amp,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pack {
    pub path: PathBuf,
    pub title: String,
    pub detail: String,
    pub category: Category,
    pub favorite: bool,
    pub count: usize,
    /// Highest gain the captures report, from 0 to 1.
    pub gain: Option<f32>,
    /// Inside AudioStrada's library folder (it can be deleted, not only hidden).
    pub owned: bool,
}

/// What the user changed about a pack.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub name: Option<String>,
    pub category: Option<Category>,
    pub favorite: bool,
    pub hidden: bool,
}

fn notes_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("captures.json"))
}

fn load_notes() -> BTreeMap<PathBuf, Note> {
    notes_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_notes(notes: &BTreeMap<PathBuf, Note>) {
    if let Some(file) = notes_file() {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(notes) {
            let _ = std::fs::write(file, text);
        }
    }
}

/// Changes what is noted about a pack, and the cached library with it.
pub fn note(path: &Path, change: impl FnOnce(&mut Note)) {
    let mut notes = load_notes();
    let note = notes.entry(path.to_path_buf()).or_default();
    change(note);
    let note = note.clone();
    save_notes(&notes);
    if let Ok(mut cached) = PACKS.lock()
        && let Some(packs) = cached.as_mut()
    {
        let mut list = packs.as_ref().clone();
        list.retain(|p| p.path != path || !note.hidden);
        for p in list.iter_mut().filter(|p| p.path == path) {
            apply(p, &note);
        }
        *packs = Arc::new(list);
    }
}

fn apply(pack: &mut Pack, note: &Note) {
    if let Some(name) = &note.name {
        pack.title = name.clone();
    }
    if let Some(category) = note.category {
        pack.category = category;
    }
    pack.favorite = note.favorite;
}

/// Groups captures into packs: a folder of captures of one piece of gear (or a TONE3000
/// download, one tone per folder) is one pack, other captures are packs of their own.
fn group(entries: Vec<Entry>, root: Option<&Path>) -> Vec<Pack> {
    let mut by_dir: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
    let mut packs = Vec::new();
    let is = |e: &Entry, ext: &str| {
        e.path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case(ext))
    };
    for e in entries.iter().filter(|e| is(e, "wav")) {
        packs.push(Pack {
            path: e.path.clone(),
            title: e.title.clone(),
            detail: e.detail.clone(),
            category: Category::Cab,
            favorite: false,
            count: 1,
            gain: None,
            owned: root.is_some_and(|r| e.path.starts_with(r)),
        });
    }
    for e in entries.into_iter().filter(|e| is(e, "nam")) {
        let dir = e.path.parent().map(PathBuf::from).unwrap_or_default();
        by_dir.entry(dir).or_default().push(e);
    }
    for (dir, files) in by_dir {
        let downloaded = dir
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "TONE3000" || n == "Imported");
        let same = downloaded || files.iter().all(|f| f.title == files[0].title);
        let make = |path: PathBuf, entries: &[Entry]| {
            let first = &entries[0];
            // A download is named after its tone; the captures' own words can be anything.
            let title = if downloaded
                || (entries.len() > 1 && entries.iter().any(|e| e.title != first.title))
            {
                path.file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            } else {
                first.title.clone()
            };
            Pack {
                gain: entries
                    .iter()
                    .filter_map(|e| e.gain)
                    .fold(None, |m: Option<f32>, g| Some(m.map_or(g, |m| m.max(g))))
                    .map(|g| g / 10.0),
                category: Category::from_family(first.family),
                favorite: false,
                count: entries.len(),
                detail: first.detail.clone(),
                owned: root.is_some_and(|r| path.starts_with(r)),
                title,
                path,
            }
        };
        if same && files.len() > 1 {
            packs.push(make(dir, &files));
        } else {
            for f in &files {
                packs.push(make(f.path.clone(), std::slice::from_ref(f)));
            }
        }
    }
    packs
}

fn scan(dirs: Vec<PathBuf>) -> Vec<Pack> {
    let root = crate::nam_library::library_root();
    let mut packs = group(crate::nam_library::scan(dirs), root.as_deref());
    let notes = load_notes();
    packs.retain(|p| !notes.get(&p.path).is_some_and(|n| n.hidden));
    for p in &mut packs {
        if let Some(note) = notes.get(&p.path) {
            apply(p, note);
        }
    }
    packs.sort_by_key(|p| p.title.to_lowercase());
    packs
}

static PACKS: Mutex<Option<Arc<Vec<Pack>>>> = Mutex::new(None);
static SCANNING: Mutex<bool> = Mutex::new(false);

/// The library's packs, scanned in the background the first time (or after `rescan`).
pub fn packs(ctx: &egui::Context, rescan: bool) -> Option<Arc<Vec<Pack>>> {
    let cached = PACKS.lock().ok().and_then(|p| p.clone());
    if cached.is_some() && !rescan {
        return cached;
    }
    let mut scanning = SCANNING.lock().ok()?;
    if !*scanning {
        *scanning = true;
        let ctx = ctx.clone();
        let dirs = crate::nam_library::load_dirs();
        let _ = std::thread::Builder::new()
            .name("capture-library".into())
            .spawn(move || {
                let found = Arc::new(scan(dirs));
                if let Ok(mut p) = PACKS.lock() {
                    *p = Some(found);
                }
                if let Ok(mut s) = SCANNING.lock() {
                    *s = false;
                }
                ctx.request_repaint();
            });
    }
    cached
}

/// The category of downloaded captures, from their files.
pub fn category_of(folder: &Path) -> Option<Category> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    if files
        .iter()
        .any(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
    {
        return Some(Category::Cab);
    }
    files.iter().find_map(|f| {
        let text = std::fs::read_to_string(f).ok()?;
        crate::nam_library::describe(f, &text).map(|e| Category::from_family(e.family))
    })
}

/// The library as last scanned, without starting a scan.
pub fn cached() -> Option<Arc<Vec<Pack>>> {
    PACKS.lock().ok().and_then(|p| p.clone())
}

/// Forgets the scanned library, so the next look scans it again.
pub fn invalidate() {
    if let Ok(mut p) = PACKS.lock() {
        *p = None;
    }
}

/// Copies .nam files into the library (`Imported/<gear>`), one folder per piece of gear;
/// returns how many packs of each category were added.
pub fn import(files: &[PathBuf]) -> Result<Vec<(String, Category)>, String> {
    let root = crate::nam_library::library_root().ok_or(tr("no configuration folder"))?;
    let entries: Vec<Entry> = files
        .iter()
        .filter_map(|f| {
            let text = std::fs::read_to_string(f).ok()?;
            crate::nam_library::describe(f, &text)
        })
        .collect();
    if entries.is_empty() {
        return Err(tr("no NAM captures among these files").into());
    }
    let mut by_gear: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for e in &entries {
        by_gear.entry(e.title.clone()).or_default().push(e);
    }
    let mut added = Vec::new();
    for (title, group) in by_gear {
        let folder = root
            .join("Imported")
            .join(crate::nam_library::file_name(&title, "Captures"));
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        for e in &group {
            if let Some(name) = e.path.file_name() {
                std::fs::copy(&e.path, folder.join(name)).map_err(|e| e.to_string())?;
            }
        }
        added.push((title, Category::from_family(group[0].family)));
    }
    invalidate();
    Ok(added)
}

/// Deletes a pack copied into AudioStrada's library, or hides one from another folder.
pub fn remove(pack: &Pack) -> Result<(), String> {
    if pack.owned {
        let removed = if pack.path.is_dir() {
            std::fs::remove_dir_all(&pack.path)
        } else {
            std::fs::remove_file(&pack.path)
        };
        removed.map_err(|e| e.to_string())?;
        invalidate();
        Ok(())
    } else {
        note(&pack.path, |n| n.hidden = true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, title: &str, family: &'static str, gain: f32) -> Entry {
        Entry {
            path: PathBuf::from(path),
            family,
            title: title.into(),
            detail: String::new(),
            gain: Some(gain),
        }
    }

    #[test]
    fn families_and_gear_types_agree_on_categories() {
        for (family, gear, category) in [
            ("Amps", "amp", Category::Amp),
            ("Full rigs", "amp-cab", Category::FullRig),
            ("Pedals", "pedal", Category::Effect),
            ("Outboard", "outboard", Category::Effect),
            ("Cabinets (IR)", "ir", Category::Cab),
            ("Other", "unknown", Category::Other),
        ] {
            assert_eq!(Category::from_family(family), category, "{family}");
            assert_eq!(Category::from_gear(gear), category, "{gear}");
        }
        assert_eq!(Category::Effect.place(), Place::Pedalboard);
        assert_eq!(Category::Cab.place(), Place::Cab);
        assert_eq!(Category::FullRig.place(), Place::Amp);
    }

    #[test]
    fn captures_of_one_piece_of_gear_make_a_pack() {
        let packs = group(
            vec![
                entry("/lib/Plexi/a.nam", "Marshall 1959", "Amps", 2.0),
                entry("/lib/Plexi/b.nam", "Marshall 1959", "Amps", 8.0),
                entry("/lib/Loose/twin.nam", "Fender Twin", "Full rigs", 3.0),
                entry("/lib/Loose/ts9.nam", "Ibanez TS9", "Pedals", 6.0),
                entry("/lib/Loose/1073.nam", "Neve 1073", "Outboard", 0.0),
                entry("/lib/Loose/v30.wav", "V30 SM57", "Cabinets (IR)", 0.0),
                entry("/lib/TONE3000/Mixed/x.nam", "One", "Amps", 1.0),
                entry("/lib/TONE3000/Mixed/y.nam", "Two", "Amps", 1.0),
            ],
            Some(Path::new("/lib/TONE3000")),
        );
        assert_eq!(packs.len(), 6);
        let plexi = packs.iter().find(|p| p.title == "Marshall 1959").unwrap();
        assert_eq!(
            (plexi.count, plexi.category, plexi.gain),
            (2, Category::Amp, Some(0.8))
        );
        let twin = packs.iter().find(|p| p.title == "Fender Twin").unwrap();
        assert_eq!(twin.category, Category::FullRig);
        assert!(
            packs
                .iter()
                .any(|p| p.title == "Ibanez TS9" && p.category == Category::Effect)
        );
        let neve = packs.iter().find(|p| p.title == "Neve 1073").unwrap();
        assert_eq!(neve.category.place(), Place::Pedalboard);
        let v30 = packs.iter().find(|p| p.title == "V30 SM57").unwrap();
        assert_eq!(v30.category.place(), Place::Cab);
        let mixed = packs.iter().find(|p| p.title == "Mixed").unwrap();
        assert!(mixed.owned && mixed.count == 2);
    }

    #[test]
    fn notes_rename_recategorize_and_star() {
        let mut pack = group(vec![entry("/lib/a.nam", "Amp", "Amps", 1.0)], None).remove(0);
        apply(
            &mut pack,
            &Note {
                name: Some("My amp".into()),
                category: Some(Category::FullRig),
                favorite: true,
                hidden: false,
            },
        );
        assert_eq!(
            (pack.title.as_str(), pack.category, pack.favorite),
            ("My amp", Category::FullRig, true)
        );
    }
}
