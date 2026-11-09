//! The AI assistants window: switching the control channel on or off, adding AudioStrada to
//! Claude Desktop, Claude Code, Cursor or Gemini CLI, and what assistants did lately.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::SystemTime;

use eframe::egui::{self, Align2, Color32, RichText, vec2};

use crate::i18n::tr;
use crate::integrations::{self, Client, State};
use crate::theme;
use crate::trf;

/// What the window asks the application to do.
pub enum Change {
    Start(u16),
    Stop,
}

/// What Claude Code's `claude` command answered: the addition (true) or removal asked, and
/// whether AudioStrada is in its servers.
type ClaudeAnswer = (Option<(bool, Result<(), String>)>, State);

#[derive(Default)]
pub struct AssistantsWindow {
    open: bool,
    /// Each assistant: found here, and whether AudioStrada is in its servers.
    states: Vec<(Client, bool, State)>,
    message: Option<(bool, String)>,
    port: u16,
    /// The `claude` command takes a second or more: it runs off the interface thread.
    claude: Option<Receiver<ClaudeAnswer>>,
}

impl AssistantsWindow {
    pub fn open(&mut self) {
        self.open = true;
        self.states.clear();
        self.message = None;
    }

    fn refresh(&mut self, ctx: &egui::Context, port: u16) {
        self.states = Client::ALL
            .iter()
            .map(|c| {
                let state = match c {
                    Client::ClaudeCode => State::NotAdded,
                    _ => integrations::state(*c, port),
                };
                (*c, c.found(), state)
            })
            .collect();
        self.ask_claude(ctx, port, None);
    }

    /// Runs `claude` in the background: adding (true) or removing AudioStrada first if asked,
    /// then reading whether it is in Claude Code's servers.
    fn ask_claude(&mut self, ctx: &egui::Context, port: u16, change: Option<bool>) {
        if !Client::ClaudeCode.found() {
            self.claude = None;
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let done = change.map(|add| {
                let result = if add {
                    integrations::add(Client::ClaudeCode, port)
                } else {
                    integrations::remove(Client::ClaudeCode)
                };
                (add, result)
            });
            let _ = send.send((done, integrations::state(Client::ClaudeCode, port)));
            ctx.request_repaint();
        });
        self.claude = Some(receive);
    }

    /// Takes Claude Code's answer once it came.
    fn claude_answer(&mut self) {
        let Some(receive) = &self.claude else {
            return;
        };
        let (done, state) = match receive.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.claude = None;
                return;
            }
        };
        self.claude = None;
        if let Some(entry) = self.states.iter_mut().find(|s| s.0 == Client::ClaudeCode) {
            entry.2 = state;
        }
        if let Some((add, result)) = done {
            self.message = Some(outcome(Client::ClaudeCode, add, result));
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        listening: Option<u16>,
        port: u16,
        log: &VecDeque<(String, SystemTime)>,
    ) -> Option<Change> {
        if !self.open {
            return None;
        }
        if self.port == 0 {
            self.port = port;
        }
        if self.states.is_empty() {
            self.refresh(ctx, self.port);
        }
        self.claude_answer();
        let mut change = None;
        let mut open = true;
        egui::Window::new(tr("AI assistants"))
            .id(egui::Id::unique("assistants-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .pivot(Align2::CENTER_TOP)
            .default_pos(ctx.content_rect().center_top() + vec2(0.0, 70.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(tr(
                    "An AI assistant can hear, look at and change the open project: it measures the mix, finds what masks what, sets equalizers and levels. Every change it makes can be undone.",
                ));
                ui.add_space(4.0);
                change = self.switch(ui, listening);
                ui.separator();
                self.clients(ui);
                ui.separator();
                activity(ui, log);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(tr(
                        "AudioStrada answers only programs of this computer and this user. What the assistant reads (names, measurements, pictures) goes to its provider, like anything you ask it.",
                    ))
                    .small()
                    .weak(),
                );
            });
        self.open &= open;
        change
    }

    fn switch(&mut self, ui: &mut egui::Ui, listening: Option<u16>) -> Option<Change> {
        let mut change = None;
        ui.horizontal(|ui| {
            let mut on = listening.is_some();
            if ui
                .checkbox(
                    &mut on,
                    RichText::new(tr("Allow AI assistants to drive AudioStrada")).strong(),
                )
                .changed()
            {
                change = Some(if on {
                    Change::Start(self.port)
                } else {
                    Change::Stop
                });
            }
        });
        ui.horizontal(|ui| {
            match listening {
                Some(port) => ui.label(
                    RichText::new(trf!("On: listening on this computer, port {port}", port = port))
                        .color(theme::PLAY),
                ),
                None => ui.label(RichText::new(tr("Off: assistants cannot reach AudioStrada")).weak()),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let field = ui
                    .add(egui::DragValue::new(&mut self.port).range(1024..=65_535))
                    .on_hover_text(tr("Change it only if another program uses this port, then update the assistants below"));
                ui.label(RichText::new(tr("Port")).weak());
                if field.lost_focus() && listening.is_some_and(|p| p != self.port) {
                    change = Some(Change::Start(self.port));
                    self.refresh(ui.ctx(), self.port);
                }
            });
        });
        change
    }

    fn clients(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(tr("Connect an assistant")).strong());
        let port = self.port;
        let checking = self.claude.is_some();
        let mut job = None;
        egui::Grid::new("assistants-clients")
            .num_columns(3)
            .spacing(vec2(16.0, 8.0))
            .show(ui, |ui| {
                for (client, found, state) in &self.states {
                    ui.label(RichText::new(client.name()).strong());
                    if checking && *client == Client::ClaudeCode {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(tr("Checking…")).weak());
                        });
                        ui.label("");
                        ui.end_row();
                        continue;
                    }
                    let (text, color) = match (state, found) {
                        (State::Added, _) => (tr("Added"), theme::PLAY),
                        (State::Outdated, _) => {
                            (tr("Added for another port: update it"), theme::MUTE)
                        }
                        (State::Unknown(_), _) => (tr("Its settings cannot be read"), theme::MUTE),
                        (State::NotAdded, true) => {
                            (tr("Not added"), ui.visuals().weak_text_color())
                        }
                        (State::NotAdded, false) => (
                            tr("Not found on this computer"),
                            ui.visuals().weak_text_color(),
                        ),
                    };
                    let label = ui.label(RichText::new(text).color(color));
                    if let State::Unknown(e) = state {
                        label.on_hover_text(e);
                    }
                    ui.horizontal(|ui| {
                        let (add, remove) = match state {
                            State::Added => (None, true),
                            State::Outdated => (Some(tr("Update")), true),
                            _ => (Some(tr("Add")), false),
                        };
                        if let Some(add) = add
                            && ui
                                .add_enabled(
                                    *found || *client != Client::ClaudeCode,
                                    egui::Button::new(add),
                                )
                                .clicked()
                        {
                            job = Some((*client, true));
                        }
                        if remove && ui.button(tr("Remove")).clicked() {
                            job = Some((*client, false));
                        }
                    });
                    ui.end_row();
                }
            });
        match job {
            Some((Client::ClaudeCode, add)) => {
                self.message = None;
                self.ask_claude(ui.ctx(), port, Some(add));
            }
            Some((client, add)) => {
                let result = if add {
                    integrations::add(client, port)
                } else {
                    integrations::remove(client)
                };
                self.message = Some(outcome(client, add, result));
                if let Some(entry) = self.states.iter_mut().find(|s| s.0 == client) {
                    entry.2 = integrations::state(client, port);
                }
            }
            None => {}
        }
        if let Some((ok, text)) = &self.message {
            ui.label(RichText::new(text).color(if *ok { theme::PLAY } else { theme::MUTE }));
        }
        if !Client::ClaudeCode.found() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tr("Claude Code, in a terminal:")).weak());
                let line = integrations::claude_code_line(port);
                ui.code(&line);
                if ui.small_button(tr("Copy")).clicked() {
                    ui.ctx().copy_text(line);
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("Another assistant that speaks MCP:")).weak());
            if ui.small_button(tr("Copy its configuration")).clicked() {
                ui.ctx().copy_text(integrations::snippet(port));
                self.message = Some((
                    true,
                    tr("Copied: paste it into the assistant's MCP settings.").to_owned(),
                ));
            }
        });
        ui.label(
            RichText::new(tr(
                "ChatGPT only reaches MCP servers on the internet for now, not programs of this computer.",
            ))
            .small()
            .weak(),
        );
    }
}

/// The message after adding (true) or removing AudioStrada.
fn outcome(client: Client, add: bool, result: Result<(), String>) -> (bool, String) {
    match (result, add) {
        (Ok(()), true) => (
            true,
            trf!(
                "Added to {name}: restart it to load AudioStrada.",
                name = client.name()
            ),
        ),
        (Ok(()), false) => (true, trf!("Removed from {name}.", name = client.name())),
        (Err(e), _) => (false, e),
    }
}

fn activity(ui: &mut egui::Ui, log: &VecDeque<(String, SystemTime)>) {
    ui.label(RichText::new(tr("Lately")).strong());
    if log.is_empty() {
        ui.label(RichText::new(tr("No assistant has called AudioStrada since it started.")).weak());
        return;
    }
    for (tool, when) in log {
        let ago = when.elapsed().map(|d| d.as_secs()).unwrap_or(0);
        let ago = if ago < 60 {
            trf!("{n} s ago", n = ago)
        } else if ago < 3600 {
            trf!("{n} min ago", n = ago / 60)
        } else {
            trf!("{n} h ago", n = ago / 3600)
        };
        ui.horizontal(|ui| {
            ui.monospace(tool);
            ui.label(RichText::new(ago).weak().color(Color32::GRAY));
        });
    }
}
