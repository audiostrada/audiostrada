//! Blind test of the console colors (ABX): A is the mix through its consoles, B the mix without
//! them, X one of the two drawn at random. Ten rounds tell whether the color is heard or only
//! believed: by guessing, 9 right out of 10 happens about once in a hundred tests.
//!
//! What is heard is switched in the player only: nothing goes into the project or its history,
//! and closing the window gives the consoles back to the project.

use eframe::egui::{self, Align2, RichText, vec2};

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
    /// The answer of the round before: right or not, shown once answered.
    last: Option<bool>,
}

impl Test {
    fn new() -> Self {
        Self {
            round: 0,
            right: 0,
            x_is_b: coin(),
            hearing: Hearing::X,
            last: None,
        }
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

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    let mut test = ctx
        .data(|d| d.get_temp::<Test>(test_id()))
        .unwrap_or_else(Test::new);
    egui::Window::new(tr("Console blind test"))
        .id(egui::Id::unique("blind-test-frame"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(400.0)
        .pivot(Align2::CENTER_TOP)
        .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
        .show(ctx, |ui| contents(ui, doc, &mut test));
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

fn contents(ui: &mut egui::Ui, doc: &Document, test: &mut Test) {
    let p = theme::current(ui);
    ui.weak(tr(
        "A is the mix through its consoles, B the mix without them, X one of the two. Play the \
         song, listen to A, B and X as often as you like, then say which one X is.",
    ));
    ui.separator();
    if !colored(doc) {
        ui.label(tr(
            "No console colors the mix: choose one in the Console menu first.",
        ));
        return;
    }
    if test.done() {
        results(ui, test);
        return;
    }
    ui.label(trf!(
        "Round {round} of {rounds}",
        round = test.round + 1,
        rounds = ROUNDS
    ));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let big = |text: &str| RichText::new(text).size(18.0).strong();
        for (hearing, label) in [(Hearing::A, "A"), (Hearing::B, "B"), (Hearing::X, "X")] {
            if ui
                .add_sized(
                    [80.0, 40.0],
                    egui::Button::new(big(label)).selected(test.hearing == hearing),
                )
                .clicked()
            {
                test.hearing = hearing;
            }
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let mut answer = None;
        if theme::primary_button(ui, tr("X is A")).clicked() {
            answer = Some(false);
        }
        if theme::primary_button(ui, tr("X is B")).clicked() {
            answer = Some(true);
        }
        if let Some(is_b) = answer {
            let right = is_b == test.x_is_b;
            test.right += usize::from(right);
            test.round += 1;
            test.last = Some(right);
            // A new X, heard straight away.
            test.x_is_b = coin();
            test.hearing = Hearing::X;
        }
    });
    if let Some(right) = test.last {
        ui.add_space(4.0);
        if right {
            ui.colored_label(p.accent, tr("Right."));
        } else {
            ui.weak(tr("Wrong."));
        }
    }
    ui.add_space(4.0);
    ui.weak(tr(
        "A and B are at the same level: the consoles keep the working level (-18 dBFS).",
    ));
}

fn results(ui: &mut egui::Ui, test: &mut Test) {
    let p = theme::current(ui);
    let chance = chance(test.right, ROUNDS) * 100.0;
    ui.label(
        RichText::new(trf!(
            "{right} right out of {rounds}",
            right = test.right,
            rounds = ROUNDS
        ))
        .size(16.0)
        .strong(),
    );
    let odds = trf!(
        "By guessing, {chance} % of tests do as well.",
        chance = format!("{chance:.1}")
    );
    if chance <= 5.0 {
        ui.colored_label(p.accent, tr("You hear the consoles."));
    } else {
        ui.label(tr(
            "Not told apart for sure: at this level and amount, the color is subtle.",
        ));
    }
    ui.weak(odds);
    ui.add_space(8.0);
    if theme::primary_button(ui, tr("Again")).clicked() {
        *test = Test::new();
    }
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
