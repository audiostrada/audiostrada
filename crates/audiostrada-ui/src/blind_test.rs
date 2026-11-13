//! Blind test of the console colors (ABX): A is the mix through its consoles, B the mix without
//! them, X one of the two drawn at random. Ten rounds tell whether the color is heard or only
//! believed: by guessing, 9 right out of 10 happens about once in a hundred tests.
//!
//! What is heard is switched in the player only: nothing goes into the project or its history,
//! and closing the window gives the consoles back to the project.

use eframe::egui::{self, RichText};

use crate::dialog;
use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

/// Rounds of a test.
const ROUNDS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hearing {
    A,
    B,
    X,
}

#[derive(Clone, Copy, Debug)]
struct Test {
    /// Answers given, and how many were right.
    round: usize,
    right: usize,
    /// X is B, the mix without its consoles.
    x_is_b: bool,
    hearing: Hearing,
    /// Each round answered: right or not.
    answers: [Option<bool>; ROUNDS],
}

impl Test {
    fn new() -> Self {
        Self {
            round: 0,
            right: 0,
            x_is_b: coin(),
            hearing: Hearing::X,
            answers: [None; ROUNDS],
        }
    }

    /// The answer of the round before, shown once answered.
    fn last(&self) -> Option<bool> {
        self.round.checked_sub(1).and_then(|r| self.answers[r])
    }

    /// Answers that X is B or not, and draws the next X, heard straight away.
    fn answer(&mut self, is_b: bool) {
        if self.done() {
            return;
        }
        let right = is_b == self.x_is_b;
        self.answers[self.round] = Some(right);
        self.right += usize::from(right);
        self.round += 1;
        self.x_is_b = coin();
        self.hearing = Hearing::X;
    }

    fn done(&self) -> bool {
        self.round >= ROUNDS
    }

    /// The consoles off, for what is heard.
    fn off(&self) -> bool {
        match self.hearing {
            Hearing::A => false,
            Hearing::B => true,
            Hearing::X => self.x_is_b,
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

/// The chance of `right` answers or more out of `rounds` by guessing.
fn chance(right: usize, rounds: usize) -> f64 {
    let choose = |n: usize, k: usize| -> f64 {
        (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
    };
    (right..=rounds).map(|k| choose(rounds, k)).sum::<f64>() / 2f64.powi(rounds as i32)
}

fn open_id() -> egui::Id {
    egui::Id::unique("blind-test-window")
}

fn test_id() -> egui::Id {
    egui::Id::unique("blind-test")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(open_id(), true);
        d.insert_temp(test_id(), Test::new());
    });
}

/// Some console colors something in the project.
fn colored(doc: &Document) -> bool {
    let project = doc.project();
    project.console.colors()
        || project
            .tracks
            .iter()
            .any(|t| t.console.is_some_and(|c| c.colors()))
}

/// What the actions band asked for.
#[derive(Default)]
struct Asked {
    answer: Option<bool>,
    again: bool,
    close: bool,
}

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    let mut test = ctx
        .data(|d| d.get_temp::<Test>(test_id()))
        .unwrap_or_else(Test::new);
    let ready = colored(doc);
    let mut asked = Asked::default();
    dialog::Dialog::new("blind-test", tr("Console blind test"))
        .detail(trf!(
            "Whether you hear the consoles or only believe it: {rounds} rounds of A, B and X",
            rounds = ROUNDS
        ))
        .width(520.0)
        .show(ctx, &mut open, |ui, part| match part {
            dialog::Part::Body if !ready => {
                dialog::section(ui, tr("Nothing to compare"), |ui| {
                    ui.label(tr(
                        "No console colors the mix: choose one in the Console menu first.",
                    ));
                });
            }
            dialog::Part::Body if test.done() => results(ui, &test),
            dialog::Part::Body => contents(ui, &mut test),
            dialog::Part::Actions => {
                let weak = theme::current(ui).weak;
                if ready {
                    let text = if test.done() {
                        tr("Test finished").to_owned()
                    } else {
                        trf!(
                            "Round {round} of {rounds} · {right} right",
                            round = test.round + 1,
                            rounds = ROUNDS,
                            right = test.right
                        )
                    };
                    ui.label(RichText::new(text).size(11.5).color(weak));
                }
                dialog::buttons(ui, |ui| {
                    if !ready {
                        asked.close = dialog::secondary(ui, tr("Close")).clicked();
                    } else if test.done() {
                        asked.again = dialog::primary(ui, tr("Again"), true).clicked();
                        asked.close = dialog::secondary(ui, tr("Close")).clicked();
                    } else {
                        // B given first: it lands at the right, A at its left.
                        if dialog::primary(ui, tr("X is B"), true).clicked() {
                            asked.answer = Some(true);
                        }
                        if dialog::primary(ui, tr("X is A"), true).clicked() {
                            asked.answer = Some(false);
                        }
                    }
                });
            }
        });
    if let Some(is_b) = asked.answer {
        test.answer(is_b);
    }
    if asked.again {
        test = Test::new();
    }
    let open = open && !asked.close;
    let project_off = doc.project().console_off;
    if open {
        let off = colored(doc).then(|| test.off());
        doc.player_mut().set_console_blind(off, project_off);
        ctx.data_mut(|d| d.insert_temp(test_id(), test));
    } else {
        doc.player_mut().set_console_blind(None, project_off);
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

fn contents(ui: &mut egui::Ui, test: &mut Test) {
    let p = theme::current(ui);
    dialog::section(
        ui,
        &trf!(
            "Round {round} of {rounds}",
            round = test.round + 1,
            rounds = ROUNDS
        ),
        |ui| {
            rounds(ui, test);
            match test.last() {
                Some(true) => {
                    ui.colored_label(p.accent, tr("Right."));
                }
                Some(false) => {
                    ui.label(RichText::new(tr("Wrong.")).color(p.weak));
                }
                None => {}
            }
        },
    );
    dialog::section(ui, tr("Listen"), |ui| {
        ui.horizontal(|ui| {
            let big = |text: &str| RichText::new(text).size(20.0).strong();
            for (hearing, label) in [(Hearing::A, "A"), (Hearing::B, "B"), (Hearing::X, "X")] {
                if ui
                    .add_sized(
                        [96.0, 48.0],
                        egui::Button::new(big(label)).selected(test.hearing == hearing),
                    )
                    .clicked()
                {
                    test.hearing = hearing;
                }
            }
        });
        dialog::hint(
            ui,
            tr(
                "A is the mix through its consoles, B the mix without them, X one of the two. \
                Play the song, listen to A, B and X as often as you like, then say which one \
                X is.",
            ),
        );
        dialog::hint(
            ui,
            tr("A and B are at the same level: the consoles keep the working level (-18 dBFS)."),
        );
    });
}

/// The rounds as dots: lit when right, hollow when wrong, outlined for the one to answer.
fn rounds(ui: &mut egui::Ui, test: &Test) {
    let p = theme::current(ui);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
    let painter = ui.painter();
    for (k, answer) in test.answers.iter().enumerate() {
        let center = egui::pos2(rect.left() + 9.0 + k as f32 * 24.0, rect.center().y);
        match answer {
            Some(true) => {
                painter.circle_filled(center, 7.0, p.accent);
            }
            Some(false) => {
                painter.circle_stroke(center, 6.5, egui::Stroke::new(1.5, theme::RECORD));
            }
            None if k == test.round => {
                painter.circle_stroke(center, 6.5, egui::Stroke::new(1.5, p.text));
            }
            None => {
                painter.circle_filled(center, 3.0, p.border);
            }
        }
    }
}

fn results(ui: &mut egui::Ui, test: &Test) {
    let p = theme::current(ui);
    let chance = chance(test.right, ROUNDS) * 100.0;
    dialog::section(ui, tr("Result"), |ui| {
        ui.label(
            RichText::new(trf!(
                "{right} right out of {rounds}",
                right = test.right,
                rounds = ROUNDS
            ))
            .size(18.0)
            .strong(),
        );
        rounds(ui, test);
        if chance <= 5.0 {
            ui.colored_label(p.accent, tr("You hear the consoles."));
        } else {
            ui.label(tr(
                "Not told apart for sure: at this level and amount, the color is subtle.",
            ));
        }
        dialog::hint(
            ui,
            &trf!(
                "By guessing, {chance} % of tests do as well.",
                chance = format!("{chance:.1}")
            ),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chance_of_guessing_is_a_binomial_tail() {
        assert!((chance(0, 10) - 1.0).abs() < 1e-12);
        assert!((chance(10, 10) - 1.0 / 1024.0).abs() < 1e-12);
        // 9 or 10 right: 11 ways out of 1024.
        assert!((chance(9, 10) - 11.0 / 1024.0).abs() < 1e-12);
        assert!(chance(8, 10) < 0.06 && chance(7, 10) > 0.1);
    }

    #[test]
    fn x_is_a_or_b_and_each_comes_up() {
        let draws: Vec<bool> = (0..64).map(|_| coin()).collect();
        assert!(draws.contains(&true) && draws.contains(&false));
        let mut t = Test::new();
        t.hearing = Hearing::A;
        assert!(!t.off());
        t.hearing = Hearing::B;
        assert!(t.off());
        t.hearing = Hearing::X;
        assert_eq!(t.off(), t.x_is_b);
    }
}
