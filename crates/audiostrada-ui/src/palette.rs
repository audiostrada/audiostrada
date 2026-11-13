//! Command palette: type a few letters, run any action.

use eframe::egui::{self, FontId, Key};

use crate::actions::Action;
use crate::dialog;
use crate::i18n::tr;
use crate::keymap::Keymap;
use crate::theme;

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

/// Where the characters of `query` are found in `text`, as [`fuzzy_score`] finds them.
fn positions(query: &str, text: &str) -> Vec<usize> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut out = Vec::new();
    let mut pos = 0;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let Some(found) = (pos..text.len()).find(|&i| text[i] == q) else {
            return Vec::new();
        };
        out.push(found);
        pos = found + 1;
    }
    out
}

/// The commands run last, shown first.
const RECENT: usize = 5;

#[derive(Debug, Default)]
pub struct CommandPalette {
    open: bool,
    query: String,
    selected: usize,
    recent: Vec<Action>,
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

    /// The commands matching what is typed, the best first; with nothing typed, the recent
    /// ones then all of them by category.
    fn matches(&self) -> Vec<Action> {
        if self.query.trim().is_empty() {
            let mut rest: Vec<Action> = Action::ALL
                .iter()
                .copied()
                .filter(|a| !self.recent.contains(a))
                .collect();
            rest.sort_by_key(|a| (tr(a.category()), a.label()));
            return self.recent.iter().copied().chain(rest).collect();
        }
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
        let mut open = true;
        let typed = !self.query.trim().is_empty();
        let recent = if typed { 0 } else { self.recent.len() };
        dialog::Dialog::new("command-palette", tr("Commands"))
            .detail(tr(
                "Every command of AudioStrada: type a few letters of its name",
            ))
            .width(620.0)
            .modal()
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text(tr("Type a command…"))
                            .font(FontId::proportional(16.0))
                            .margin(egui::vec2(10.0, 8.0))
                            .desired_width(f32::INFINITY),
                    );
                    edit.request_focus();
                    if edit.changed() {
                        self.selected = 0;
                    }
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(380.0)
                        .min_scrolled_height(380.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let mut category = "";
                            for (i, &action) in matches.iter().enumerate().take(400) {
                                // Grouped when nothing is typed: the recent ones, then by
                                // category.
                                if !typed {
                                    let group = if i < recent {
                                        tr("Recent")
                                    } else {
                                        tr(action.category())
                                    };
                                    if group != category {
                                        category = group;
                                        ui.add_space(if i == 0 { 0.0 } else { 6.0 });
                                        dialog::hint(ui, &group.to_uppercase());
                                    }
                                }
                                let response = row(
                                    ui,
                                    action,
                                    i == self.selected,
                                    &self.query,
                                    typed,
                                    keymap.label(action, mac),
                                );
                                if i == self.selected && (up || down) {
                                    response.scroll_to_me(None);
                                }
                                if response.hovered()
                                    && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                                {
                                    self.selected = i;
                                }
                                if response.clicked() {
                                    chosen = Some(action);
                                }
                            }
                            if matches.is_empty() {
                                ui.add_space(30.0);
                                ui.vertical_centered(|ui| {
                                    dialog::hint(ui, tr("No matching command"));
                                });
                            }
                        });
                }
                dialog::Part::Actions => {
                    let p = theme::current(ui);
                    let (rect, _) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                    let painter = ui.painter();
                    crate::effect_frame::key_hints(
                        painter,
                        &p,
                        rect.left_center(),
                        &[
                            (&["↑", "↓"], tr("Choose")),
                            (&["Enter"], tr("Run")),
                            (&["Esc"], tr("Close")),
                        ],
                    );
                    painter.text(
                        rect.right_center(),
                        egui::Align2::RIGHT_CENTER,
                        crate::trf!("{count} commands", count = matches.len()),
                        FontId::proportional(11.5),
                        p.weak,
                    );
                }
            });
        if !open {
            self.open = false;
        }
        if let Some(action) = chosen {
            self.open = false;
            self.recent.retain(|a| *a != action);
            self.recent.insert(0, action);
            self.recent.truncate(RECENT);
        }
        chosen
    }
}

/// A command of the list: its name with the letters typed lit, its category when searching,
/// its shortcut as key caps.
fn row(
    ui: &mut egui::Ui,
    action: Action,
    selected: bool,
    query: &str,
    typed: bool,
    keys: Option<String>,
) -> egui::Response {
    let p = theme::current(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 5.0, theme::mix(p.surface, p.accent, 0.35));
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            1.5,
            p.accent,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 5.0, p.hover);
    }
    let label = action.label();
    let lit = if typed {
        positions(query, label)
    } else {
        Vec::new()
    };
    let mut job = egui::text::LayoutJob::default();
    for (k, c) in label.chars().enumerate() {
        let on = lit.contains(&k);
        job.append(
            &c.to_string(),
            0.0,
            egui::TextFormat {
                font_id: FontId::proportional(13.5),
                color: if on { p.accent } else { p.text },
                underline: if on {
                    egui::Stroke::new(1.0, p.accent)
                } else {
                    egui::Stroke::NONE
                },
                ..Default::default()
            },
        );
    }
    let galley = painter.layout_job(job);
    let text = egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
    let width = galley.size().x;
    painter.galley(text, galley, p.text);
    if typed {
        painter.text(
            egui::pos2(text.x + width + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            tr(action.category()),
            FontId::proportional(11.5),
            p.weak,
        );
    }
    if let Some(keys) = keys {
        let caps: Vec<&str> = keys.split('+').collect();
        let total = crate::effect_frame::key_hints_width(&painter, &p, &[(&caps, "")]);
        crate::effect_frame::key_hints(
            &painter,
            &p,
            egui::pos2(rect.right() - total - 8.0, rect.center().y),
            &[(&caps, "")],
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
