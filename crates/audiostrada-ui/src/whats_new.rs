//! The What's new window: the notes of the versions installed since the last start. They live in
//! `assets/whats-new` (`## <version>` sections, newest first); the release workflow puts the
//! English ones on the GitHub release.

use eframe::egui;

use crate::dialog;
use crate::i18n::tr;
use crate::trf;
use crate::updates::newer;

const ENGLISH: &str = include_str!("../assets/whats-new/en.md");
const FRENCH: &str = include_str!("../assets/whats-new/fr.md");
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Default)]
pub struct WhatsNew {
    open: bool,
    /// The notes after this version only; every version's when `None`.
    since: Option<Option<String>>,
}

impl WhatsNew {
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Every version's notes (the Help menu).
    pub fn open(&mut self) {
        self.open = true;
        self.since = None;
    }

    /// At the start after an update from `seen` (`None`: a version that did not note it).
    pub fn after_update(&mut self, seen: Option<&str>) {
        if !between(notes(), seen, CURRENT).is_empty() {
            self.open = true;
            self.since = Some(seen.map(str::to_owned));
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let list = match &self.since {
            None => sections(notes()),
            Some(seen) => between(notes(), seen.as_deref(), CURRENT),
        };
        let mut open = true;
        let mut close = false;
        dialog::Dialog::new("whats-new", tr("What's new"))
            .detail(trf!("AudioStrada {version}", version = CURRENT))
            .width(520.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    egui::ScrollArea::vertical()
                        .max_height(420.0)
                        .show(ui, |ui| {
                            for (version, text) in &list {
                                let title = trf!("Version {version}", version = version);
                                dialog::section(ui, &title, |ui| paragraphs(ui, text));
                            }
                        });
                }
                dialog::Part::Actions => dialog::buttons(ui, |ui| {
                    close = dialog::primary(ui, tr("Close"), true).clicked();
                }),
            });
        if !open || close {
            self.open = false;
        }
    }
}

fn notes() -> &'static str {
    if crate::i18n::is_french() {
        FRENCH
    } else {
        ENGLISH
    }
}

/// (version, notes) of each `## version` section, in the file's order.
fn sections(notes: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut section: Option<(&str, usize)> = None;
    let mut offset = 0;
    for line in notes.split_inclusive('\n') {
        if let Some(version) = line.trim_end().strip_prefix("## ") {
            if let Some((v, from)) = section.take() {
                out.push((v, notes[from..offset].trim()));
            }
            section = Some((version.trim(), offset + line.len()));
        }
        offset += line.len();
    }
    if let Some((v, from)) = section {
        out.push((v, notes[from..].trim()));
    }
    out
}

/// The sections after `seen` up to `current`; only `current`'s without `seen`.
fn between<'a>(notes: &'a str, seen: Option<&str>, current: &str) -> Vec<(&'a str, &'a str)> {
    sections(notes)
        .into_iter()
        .filter(|(v, _)| !newer(v, current) && seen.map_or(*v == current, |s| newer(v, s)))
        .collect()
}

/// Paragraphs and `- ` bullets, their lines joined.
fn paragraphs(ui: &mut egui::Ui, text: &str) {
    for block in text.split("\n\n") {
        let mut items: Vec<String> = Vec::new();
        let mut bullets = false;
        for line in block.lines().map(str::trim) {
            if let Some(item) = line.strip_prefix("- ") {
                bullets = true;
                items.push(format!("•  {item}"));
            } else if let Some(last) = items.last_mut() {
                last.push(' ');
                last.push_str(line);
            } else {
                items.push(line.to_owned());
            }
        }
        for item in items.iter().filter(|i| !i.is_empty()) {
            ui.label(item);
        }
        if !bullets {
            ui.add_space(2.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str =
        "## 0.3.0\n\n- Three\n\n## 0.2.0\n\nTwo,\non two lines.\n\n## 0.1.0\n\nOne\n";

    #[test]
    fn notes_split_into_versions() {
        let list = sections(NOTES);
        assert_eq!(
            list,
            vec![
                ("0.3.0", "- Three"),
                ("0.2.0", "Two,\non two lines."),
                ("0.1.0", "One")
            ]
        );
    }

    #[test]
    fn an_update_shows_the_versions_since_the_last_one_seen() {
        let versions = |seen, current| {
            between(NOTES, seen, current)
                .into_iter()
                .map(|(v, _)| v)
                .collect::<Vec<_>>()
        };
        assert_eq!(versions(Some("0.1.0"), "0.3.0"), ["0.3.0", "0.2.0"]);
        assert_eq!(versions(Some("0.1.0"), "0.2.0"), ["0.2.0"]);
        assert_eq!(versions(None, "0.2.0"), ["0.2.0"]);
        assert!(versions(Some("0.3.0"), "0.3.0").is_empty());
    }

    #[test]
    fn every_version_has_its_notes_in_both_languages() {
        let versions = |notes| {
            sections(notes)
                .into_iter()
                .map(|(v, _)| v)
                .collect::<Vec<_>>()
        };
        assert_eq!(versions(ENGLISH), versions(FRENCH));
        assert!(
            versions(ENGLISH).contains(&CURRENT),
            "no notes for {CURRENT}"
        );
    }
}
