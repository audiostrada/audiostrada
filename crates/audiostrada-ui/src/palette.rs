//! Command palette: type a few letters, run any action.

use eframe::egui::{self, Key, RichText};

use crate::actions::Action;
use crate::i18n::tr;
use crate::keymap::Keymap;

/// Score of `query` against `text`: every query character must appear in order. Matches at
/// word starts and consecutive runs score higher. `None` when it does not match.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut pos = 0;
    let mut previous: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = (pos..text.len()).find(|&i| text[i] == q)?;
        score += 1;
        if found == 0 || !text[found - 1].is_alphanumeric() {
            score += 5;
        }
        if previous == Some(found.wrapping_sub(1)) {
            score += 3;
        }
        score -= (found - pos) as i32 / 4;
        previous = Some(found);
        pos = found + 1;
    }
    Some(score)
}

#[derive(Debug, Default)]
pub struct CommandPalette {
    open: bool,
    query: String,
    selected: usize,
}

impl CommandPalette {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn matches(&self) -> Vec<Action> {
        let mut scored: Vec<(i32, Action)> = Action::ALL
            .iter()
            .filter_map(|&a| {
                // English names still match in another language.
                let shown = format!("{} {}", tr(a.category()), a.label());
                let english = format!("{} {}", a.category(), a.name());
                let score =
                    fuzzy_score(&self.query, &shown).max(fuzzy_score(&self.query, &english));
                score.map(|s| (s, a))
            })
            .collect();
        scored.sort_by_key(|&(s, a)| (std::cmp::Reverse(s), a.label()));
        scored.into_iter().map(|(_, a)| a).collect()
    }

    /// Shows the palette; returns the action chosen this frame.
    pub fn show(&mut self, ctx: &egui::Context, keymap: &Keymap, mac: bool) -> Option<Action> {
        if !self.open {
            return None;
        }
        let matches = self.matches();
        self.selected = self.selected.min(matches.len().saturating_sub(1));
        let (up, down, enter, escape) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
            )
        });
        if escape {
            self.open = false;
            return None;
        }
        if down {
            self.selected = (self.selected + 1).min(matches.len().saturating_sub(1));
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        let mut chosen = enter.then(|| matches.get(self.selected).copied()).flatten();
        egui::Modal::new(egui::Id::unique("command-palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(tr("Type a command…"))
                    .desired_width(f32::INFINITY),
            );
            edit.request_focus();
            if edit.changed() {
                self.selected = 0;
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, action) in matches.iter().enumerate().take(40) {
                        ui.horizontal(|ui| {
                            ui.set_min_width(ui.available_width());
                            let response = ui.selectable_label(i == self.selected, action.label());
                            ui.weak(tr(action.category()));
                            if i == self.selected {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                chosen = Some(*action);
                            }
                            if let Some(label) = keymap.label(*action, mac) {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| ui.label(RichText::new(label).monospace().weak()),
                                );
                            }
                        });
                    }
                    if matches.is_empty() {
                        ui.weak(tr("No matching command"));
                    }
                });
        });
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_prefers_word_starts() {
        assert!(fuzzy_score("spl", "Edit Split at playhead").is_some());
        assert!(fuzzy_score("xyz", "Edit Split at playhead").is_none());
        let word_start = fuzzy_score("sa", "File Save as").unwrap();
        let scattered = fuzzy_score("sa", "Transport Toggle loop as").unwrap_or(i32::MIN);
        assert!(word_start > scattered);
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("S P", "Split"), fuzzy_score("sp", "split"));
    }

    #[test]
    fn the_best_match_comes_first() {
        let mut palette = CommandPalette::default();
        assert!(!palette.is_open());
        palette.query = "old".into();
        palette.selected = 3;
        palette.open();
        assert!(palette.is_open());
        assert_eq!((palette.query.as_str(), palette.selected), ("", 0));
        assert_eq!(palette.matches().len(), Action::ALL.len());
        palette.query = "view zoom in".into();
        assert_eq!(palette.matches()[0], Action::ZoomIn);
        palette.query = "undo".into();
        assert_eq!(palette.matches()[0], Action::Undo);
        palette.query = "qqqq".into();
        assert!(palette.matches().is_empty());
    }

    #[test]
    fn escape_closes_and_enter_runs_the_selected_command() {
        use eframe::egui::{Event, Modifiers, RawInput};
        let key = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let ctx = egui::Context::default();
        let keymap = Keymap::default();
        let mut palette = CommandPalette::default();
        let frame = |palette: &mut CommandPalette, events| {
            let mut chosen = None;
            let input = RawInput {
                events,
                ..Default::default()
            };
            ctx.run_pass(input, |ctx| chosen = palette.show(ctx, &keymap, false))
                .drop_without_applying_deltas();
            chosen
        };
        assert_eq!(frame(&mut palette, vec![key(Key::Enter)]), None, "closed");
        palette.open();
        palette.query = "undo".into();
        frame(&mut palette, vec![]);
        frame(&mut palette, vec![key(Key::ArrowDown), key(Key::ArrowUp)]);
        assert_eq!(
            frame(&mut palette, vec![key(Key::Enter)]),
            Some(Action::Undo)
        );
        assert!(!palette.is_open());
        palette.open();
        assert_eq!(frame(&mut palette, vec![key(Key::Escape)]), None);
        assert!(!palette.is_open());
    }
}
