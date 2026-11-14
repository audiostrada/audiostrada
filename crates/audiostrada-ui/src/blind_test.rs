//! Blind test of the console colors, as a duel: two takes of the mix, 1 and 2, one through a
//! console and one without, drawn at random; the listener says which one goes through it.
//! The test starts with the console pushed all the way, where anyone hears it, and gets
//! subtler level after level: it ends on the lowest amount still heard, told for the console
//! tested and kept, console by console, with the application's settings.
//!
//! What is heard is switched in the player only: nothing goes into the project or its history,
//! and closing the window gives the consoles back to the project.

use audiostrada_core::console::ConsoleModel;
use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, vec2};

use crate::console_ui;
use crate::dialog;
use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

/// The amounts played, level after level, %: from the console pushed all the way down to a
/// subtle one.
const LEVELS: [f32; 6] = [100.0, 75.0, 50.0, 35.0, 25.0, 15.0];
/// Right answers that pass a level, and misses at a level that end the test.
const TO_PASS: usize = 3;
const MISSES: usize = 2;

/// The consoles tested, in the order they are offered.
const CONSOLES: [ConsoleModel; 4] = [
    ConsoleModel::Imperial,
    ConsoleModel::Iron,
    ConsoleModel::Discrete,
    ConsoleModel::Velvet,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// The console chosen, heard freely with and without.
    Setup,
    Duel,
    Done,
}

#[derive(Clone, Copy, Debug)]
struct Test {
    model: ConsoleModel,
    stage: Stage,
    level: usize,
    /// Right answers and misses at this level.
    right: usize,
    wrong: usize,
    /// Take 2 goes through the console (else take 1).
    two: bool,
    /// The take heard, 1 or 2; before the duel, 1 is with the console and 2 without.
    hearing: u8,
    /// The answer before: right or not, the take that went through the console, its amount.
    last: Option<(bool, u8, f32)>,
    /// Levels passed.
    passed: usize,
    /// Right answers, and answers in all.
    answers: (usize, usize),
}

impl Test {
    fn new(model: ConsoleModel) -> Self {
        Self {
            model,
            stage: Stage::Setup,
            level: 0,
            right: 0,
            wrong: 0,
            two: coin(),
            hearing: 1,
            last: None,
            passed: 0,
            answers: (0, 0),
        }
    }

    fn start(&mut self) {
        *self = Self {
            stage: Stage::Duel,
            ..Self::new(self.model)
        };
    }

    /// Answers that `take` goes through the console, and draws the next duel.
    fn answer(&mut self, take: u8) {
        if self.stage != Stage::Duel {
            return;
        }
        let through = if self.two { 2 } else { 1 };
        let right = take == through;
        self.last = Some((right, through, LEVELS[self.level]));
        self.answers.1 += 1;
        if right {
            self.answers.0 += 1;
            self.right += 1;
        } else {
            self.wrong += 1;
        }
        if self.right >= TO_PASS {
            self.passed = self.level + 1;
            self.level += 1;
            (self.right, self.wrong) = (0, 0);
            if self.level >= LEVELS.len() {
                self.stage = Stage::Done;
            }
        } else if self.wrong >= MISSES {
            self.stage = Stage::Done;
        }
        self.two = coin();
        self.hearing = 1;
    }

    /// The lowest amount heard, once the test is over: `None` when even the first was not.
    fn heard_from(&self) -> Option<f32> {
        self.passed.checked_sub(1).map(|k| LEVELS[k])
    }

    /// The amount of the console played now, or `None` for the mix without any.
    fn playing(&self) -> Option<f32> {
        match self.stage {
            Stage::Setup => (self.hearing == 1).then_some(LEVELS[0]),
            Stage::Duel => ((self.hearing == 2) == self.two).then_some(LEVELS[self.level]),
            Stage::Done => None,
        }
    }
}

/// Heads or tails, from the system's randomness.
fn coin() -> bool {
    let mut byte = [0u8; 1];
    if getrandom::fill(&mut byte).is_err() {
        // Without it, the clock's last bit.
        return std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .is_ok_and(|d| d.subsec_nanos() & 1 == 1);
    }
    byte[0] & 1 == 1
}

/// What was found for each console tested: the lowest amount heard, or `None` when it was
/// not heard at all. Kept with the application's settings: they are the listener's.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Results(Vec<(ConsoleModel, Option<f32>)>);

impl Results {
    fn path() -> Option<std::path::PathBuf> {
        crate::config_dir().map(|d| d.join("audiostrada").join("blind_test.json"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn of(&self, model: ConsoleModel) -> Option<Option<f32>> {
        self.0.iter().find(|r| r.0 == model).map(|r| r.1)
    }

    fn keep(&mut self, model: ConsoleModel, heard: Option<f32>) {
        self.0.retain(|r| r.0 != model);
        self.0.push((model, heard));
        if let Some(path) = Self::path()
            && let Ok(json) = serde_json::to_vec_pretty(self)
        {
            if let Some(folder) = path.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = std::fs::write(path, json);
        }
    }
}

fn open_id() -> egui::Id {
    egui::Id::unique("blind-test-window")
}

fn test_id() -> egui::Id {
    egui::Id::unique("blind-test")
}

fn results_id() -> egui::Id {
    egui::Id::unique("blind-test-results")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(open_id(), true);
        d.remove::<Test>(test_id());
        d.insert_temp(results_id(), Results::load());
    });
    // Screenshots: `AUDIOSTRADA_SCREENSHOT_VIEW=duel` or `result` opens the test under way.
    if let Ok(view) = std::env::var("AUDIOSTRADA_SCREENSHOT_VIEW") {
        let mut test = Test::new(ConsoleModel::Iron);
        test.start();
        let answers: &[bool] = match view.as_str() {
            "duel" => &[true, true, true, true, false, true],
            "result" => &[
                true, true, true, true, true, true, true, false, true, true, true, false, false,
            ],
            _ => &[],
        };
        for &right in answers {
            let through = if test.two { 2 } else { 1 };
            test.answer(if right { through } else { 3 - through });
        }
        if !answers.is_empty() {
            ctx.data_mut(|d| d.insert_temp(test_id(), test));
        }
    }
}

/// The console the test opens on: the project's, else the first offered.
fn first(doc: &Document) -> ConsoleModel {
    let model = doc.project().console.model;
    if model == ConsoleModel::Clean {
        CONSOLES[0]
    } else {
        model
    }
}

/// What the actions band asked for.
#[derive(Default)]
struct Asked {
    answer: Option<u8>,
    start: bool,
    setup: bool,
    close: bool,
}

/// Shows the test; true when it asks to play or pause.
pub fn show(ctx: &egui::Context, doc: &mut Document) -> bool {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return false;
    }
    let mut test = ctx
        .data(|d| d.get_temp::<Test>(test_id()))
        .unwrap_or_else(|| Test::new(first(doc)));
    let mut results = ctx
        .data(|d| d.get_temp::<Results>(results_id()))
        .unwrap_or_default();
    let was = test.stage;
    let project = doc.project().console;
    let playing = doc.player().engine().playing();
    let mut asked = Asked::default();
    let mut play = false;
    // The takes on the keys 1 and 2, while nothing is being typed.
    if test.stage != Stage::Done && !ctx.egui_wants_keyboard_input() {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Num1) {
                test.hearing = 1;
            }
            if i.key_pressed(egui::Key::Num2) {
                test.hearing = 2;
            }
        });
    }
    dialog::Dialog::new("blind-test", tr("Console blind test"))
        .detail(tr(
            "Which take goes through the console? From easy to subtle",
        ))
        .width(560.0)
        .show(ctx, &mut open, |ui, part| match part {
            dialog::Part::Body => match test.stage {
                Stage::Setup => setup(ui, &mut test, project.model, &results),
                Stage::Duel => duel(ui, &mut test),
                Stage::Done => verdict(ui, &test, project, &results),
            },
            dialog::Part::Actions => {
                let label = if playing { tr("Stop") } else { tr("Listen") };
                play = dialog::secondary(ui, label).clicked();
                dialog::buttons(ui, |ui| match test.stage {
                    Stage::Setup => {
                        asked.start = dialog::primary(ui, tr("Start the test"), true).clicked();
                        asked.close = dialog::secondary(ui, tr("Close")).clicked();
                    }
                    Stage::Duel => {
                        // 2 given first: it lands at the right, 1 at its left.
                        if dialog::primary(ui, tr("It is take 2"), true).clicked() {
                            asked.answer = Some(2);
                        }
                        if dialog::primary(ui, tr("It is take 1"), true).clicked() {
                            asked.answer = Some(1);
                        }
                    }
                    Stage::Done => {
                        asked.start = dialog::primary(ui, tr("Again"), true).clicked();
                        asked.setup = dialog::secondary(ui, tr("Another console")).clicked();
                        asked.close = dialog::secondary(ui, tr("Close")).clicked();
                    }
                });
            }
        });
    if let Some(take) = asked.answer {
        test.answer(take);
    }
    if asked.start {
        test.start();
    }
    if asked.setup {
        test = Test::new(test.model);
    }
    if test.stage == Stage::Done && was != Stage::Done {
        results.keep(test.model, test.heard_from());
    }
    let open = open && !asked.close;
    let project_off = doc.project().console_off;
    let player = doc.player_mut();
    if open && test.stage != Stage::Done {
        // The console tested runs all along; the dry take only does not hear it.
        let amount = test.playing();
        let level = match test.stage {
            Stage::Duel => LEVELS[test.level],
            _ => LEVELS[0],
        };
        player.set_console_blind(Some(amount.is_none()), project_off);
        player.set_console_test(Some((test.model.index(), level)));
    } else {
        player.set_console_blind(None, project_off);
        player.set_console_test(None);
    }
    ctx.data_mut(|d| {
        d.insert_temp(open_id(), open);
        d.insert_temp(test_id(), test);
        d.insert_temp(results_id(), results);
    });
    play
}

/// Two big buttons choosing what is heard; `labels` under their numbers when they are told.
fn takes(ui: &mut egui::Ui, test: &mut Test, labels: Option<[&str; 2]>) {
    let p = theme::current(ui);
    let tint = console_ui::color(test.model);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let width = (ui.available_width() - 12.0) / 2.0;
        for take in [1u8, 2] {
            let (rect, response) = ui.allocate_exact_size(vec2(width, 84.0), Sense::click());
            let on = test.hearing == take;
            let fill = if on {
                theme::mix(p.surface, tint, 0.35)
            } else if response.hovered() {
                p.hover
            } else {
                p.surface
            };
            let edge = if on { tint } else { p.border };
            let painter = ui.painter();
            painter.rect(
                rect,
                8.0,
                fill,
                Stroke::new(if on { 2.0 } else { 1.0 }, edge),
                StrokeKind::Inside,
            );
            let number = rect.center() - vec2(0.0, if labels.is_some() { 10.0 } else { 0.0 });
            painter.text(
                number,
                Align2::CENTER_CENTER,
                take.to_string(),
                FontId::proportional(34.0),
                if on { p.text } else { p.weak },
            );
            if let Some(labels) = labels {
                painter.text(
                    rect.center() + vec2(0.0, 24.0),
                    Align2::CENTER_CENTER,
                    labels[usize::from(take) - 1],
                    FontId::proportional(12.0),
                    if on { p.text } else { p.weak },
                );
            }
            if on {
                painter.text(
                    rect.right_top() + vec2(-10.0, 10.0),
                    Align2::RIGHT_TOP,
                    tr("heard"),
                    FontId::proportional(10.0),
                    tint,
                );
            }
            if response.clicked() {
                test.hearing = take;
            }
        }
    });
}

fn setup(ui: &mut egui::Ui, test: &mut Test, project: ConsoleModel, results: &Results) {
    dialog::section(ui, tr("Console tested"), |ui| {
        ui.horizontal(|ui| {
            for model in CONSOLES {
                let mut text = RichText::new(console_ui::name(model));
                if model == test.model {
                    text = text.strong();
                }
                if ui
                    .add(egui::Button::new(text).selected(model == test.model))
                    .on_hover_text(console_ui::hint(model))
                    .clicked()
                {
                    test.model = model;
                }
            }
        });
        if test.model == project {
            dialog::hint(ui, tr("The console of your project."));
        }
        if let Some(found) = results.of(test.model) {
            dialog::hint(ui, &trf!("Last time: {found}.", found = found_text(found)));
        }
    });
    dialog::section(ui, tr("Listen first"), |ui| {
        takes(
            ui,
            test,
            Some([tr("With the console, pushed all the way"), tr("Without it")]),
        );
        dialog::hint(
            ui,
            tr(
                "Play the song and switch between the two (keys 1 and 2) to hear what the \
                 console does. Then start: the two takes are shuffled, and it gets subtler as \
                 you find the right one.",
            ),
        );
    });
}

/// What was found, in words.
fn found_text(found: Option<f32>) -> String {
    match found {
        Some(amount) => trf!("heard from {amount} %", amount = amount as i32),
        None => tr("not heard").to_owned(),
    }
}

/// The levels as steps: passed, being played, to come; and the answers at this one.
fn progress(ui: &mut egui::Ui, test: &Test) {
    let p = theme::current(ui);
    let tint = console_ui::color(test.model);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let painter = ui.painter();
    let step = rect.width() / LEVELS.len() as f32;
    for (k, amount) in LEVELS.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            rect.min + vec2(k as f32 * step, 0.0),
            vec2(step - 6.0, 22.0),
        );
        let (fill, ink) = if k < test.passed {
            (theme::mix(p.surface, tint, 0.55), p.text)
        } else if k == test.level && test.stage == Stage::Duel {
            (p.surface, p.text)
        } else {
            (p.surface, p.weak)
        };
        let failed = test.stage == Stage::Done && k == test.level && k >= test.passed;
        let edge = if failed {
            theme::RECORD
        } else if k == test.level && test.stage == Stage::Duel {
            tint
        } else {
            p.border
        };
        painter.rect(cell, 5.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
        painter.text(
            cell.center(),
            Align2::CENTER_CENTER,
            format!("{} %", *amount as i32),
            FontId::proportional(11.5),
            ink,
        );
        // The answers at the level being played: right ones lit, misses hollow.
        if k == test.level && test.stage == Stage::Duel {
            for n in 0..TO_PASS {
                let center = cell.left_bottom() + vec2(8.0 + n as f32 * 11.0, 8.0);
                if n < test.right {
                    painter.circle_filled(center, 3.5, tint);
                } else {
                    painter.circle_stroke(center, 3.0, Stroke::new(1.0, p.border));
                }
            }
            for n in 0..test.wrong {
                let center = cell.right_bottom() + vec2(-8.0 - n as f32 * 11.0, 8.0);
                painter.circle_stroke(center, 3.0, Stroke::new(1.5, theme::RECORD));
            }
        }
    }
}

/// The answer before, told with the console and its amount.
fn reveal(ui: &mut egui::Ui, test: &Test) {
    let p = theme::current(ui);
    let Some((right, take, amount)) = test.last else {
        return;
    };
    let name = console_ui::name(test.model);
    let (text, color): (String, Color32) = if right {
        (
            trf!(
                "Right: take {take} went through {name} at {amount} %.",
                take = take,
                name = name,
                amount = amount as i32
            ),
            p.accent,
        )
    } else {
        (
            trf!(
                "Missed: take {take} went through {name} at {amount} %.",
                take = take,
                name = name,
                amount = amount as i32
            ),
            theme::RECORD,
        )
    };
    ui.label(RichText::new(text).color(color).strong());
}

fn duel(ui: &mut egui::Ui, test: &mut Test) {
    let name = console_ui::name(test.model);
    dialog::section(
        ui,
        &trf!(
            "Level {level} of {levels} · {name} at {amount} %",
            level = test.level + 1,
            levels = LEVELS.len(),
            name = name,
            amount = LEVELS[test.level] as i32
        ),
        |ui| {
            progress(ui, test);
            ui.add_space(4.0);
            reveal(ui, test);
        },
    );
    dialog::section(ui, tr("Which take goes through the console?"), |ui| {
        takes(ui, test, None);
        dialog::hint(
            ui,
            tr(
                "Play the song and switch between the two takes (keys 1 and 2) as often as \
                 you like, then answer. Both are as loud: only the color differs.",
            ),
        );
        dialog::hint(
            ui,
            &trf!(
                "{pass} right answers pass a level; {misses} misses at a level end the test.",
                pass = TO_PASS,
                misses = MISSES
            ),
        );
    });
}

fn verdict(
    ui: &mut egui::Ui,
    test: &Test,
    project: audiostrada_core::console::Console,
    results: &Results,
) {
    let p = theme::current(ui);
    let name = console_ui::name(test.model);
    dialog::section(ui, tr("Result"), |ui| {
        progress(ui, test);
        ui.add_space(4.0);
        reveal(ui, test);
        ui.add_space(6.0);
        let headline = match test.heard_from() {
            Some(amount) if test.passed == LEVELS.len() => trf!(
                "{name}: you hear it down to {amount} %, the subtlest level.",
                name = name,
                amount = amount as i32
            ),
            Some(amount) => trf!(
                "{name}: you hear it from {amount} %.",
                name = name,
                amount = amount as i32
            ),
            None => trf!("{name}: not heard, even pushed all the way.", name = name),
        };
        ui.label(RichText::new(headline).size(17.0).strong());
        // Against the project's own setting, when it plays this console.
        if project.model == test.model {
            let set = project.amount.round() as i32;
            let text = match test.heard_from() {
                Some(amount) if project.amount >= amount => {
                    trf!("Your mix is set to {amount} %: you hear it.", amount = set)
                }
                _ => trf!(
                    "Your mix is set to {amount} %: under what you hear. Push the amount, or \
                     set the gains so that the tracks play around -18 dBFS.",
                    amount = set
                ),
            };
            ui.label(text);
        }
        dialog::hint(
            ui,
            &trf!(
                "{right} right answers out of {asked}.",
                right = test.answers.0,
                asked = test.answers.1
            ),
        );
    });
    dialog::section(ui, tr("Your results"), |ui| {
        egui::Grid::new("blind-test-results")
            .num_columns(2)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                for model in CONSOLES {
                    let found = if model == test.model {
                        Some(test.heard_from())
                    } else {
                        results.of(model)
                    };
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 4.0, console_ui::color(model));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(console_ui::name(model)).strong());
                        match found {
                            Some(found) => ui.label(found_text(found)),
                            None => ui.label(RichText::new(tr("not tested yet")).color(p.weak)),
                        };
                    });
                    ui.end_row();
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers right or wrong, whatever was drawn.
    fn answer(t: &mut Test, right: bool) {
        let through = if t.two { 2 } else { 1 };
        t.answer(if right { through } else { 3 - through });
    }

    #[test]
    fn three_right_answers_pass_a_level_and_two_misses_end_the_test() {
        let mut t = Test::new(ConsoleModel::Iron);
        t.start();
        for _ in 0..TO_PASS {
            answer(&mut t, true);
        }
        assert_eq!((t.level, t.passed, t.stage), (1, 1, Stage::Duel));
        // A miss does not end it; the right answers of the level are kept.
        answer(&mut t, false);
        answer(&mut t, true);
        answer(&mut t, true);
        assert_eq!((t.level, t.right, t.wrong), (1, 2, 1));
        answer(&mut t, false);
        assert_eq!(t.stage, Stage::Done);
        assert_eq!(t.heard_from(), Some(LEVELS[0]), "the level passed");
        assert_eq!(t.answers, (5, 7));
        // Nothing heard at all.
        let mut t = Test::new(ConsoleModel::Iron);
        t.start();
        answer(&mut t, false);
        answer(&mut t, false);
        assert_eq!((t.stage, t.heard_from()), (Stage::Done, None));
    }

    #[test]
    fn every_level_passed_ends_on_the_subtlest() {
        let mut t = Test::new(ConsoleModel::Velvet);
        t.start();
        for _ in 0..TO_PASS * LEVELS.len() {
            answer(&mut t, true);
        }
        assert_eq!(t.stage, Stage::Done);
        assert_eq!(t.heard_from(), Some(LEVELS[LEVELS.len() - 1]));
    }

    #[test]
    fn one_take_plays_the_console_at_the_level_s_amount_and_the_other_none() {
        let draws: Vec<bool> = (0..64).map(|_| coin()).collect();
        assert!(draws.contains(&true) && draws.contains(&false));
        let mut t = Test::new(ConsoleModel::Imperial);
        // Before the duel: take 1 with the console pushed, take 2 without.
        assert_eq!(t.playing(), Some(LEVELS[0]));
        t.hearing = 2;
        assert_eq!(t.playing(), None);
        t.start();
        let through = if t.two { 2 } else { 1 };
        t.hearing = through;
        assert_eq!(t.playing(), Some(LEVELS[0]));
        t.hearing = 3 - through;
        assert_eq!(t.playing(), None);
    }

    #[test]
    fn the_results_keep_one_line_a_console() {
        let mut r = Results::default();
        r.keep(ConsoleModel::Iron, Some(50.0));
        r.keep(ConsoleModel::Velvet, None);
        r.keep(ConsoleModel::Iron, Some(35.0));
        assert_eq!(r.of(ConsoleModel::Iron), Some(Some(35.0)));
        assert_eq!(r.of(ConsoleModel::Velvet), Some(None));
        assert_eq!(r.of(ConsoleModel::Discrete), None);
        assert_eq!(Results::load(), r, "kept with the settings");
    }
}
