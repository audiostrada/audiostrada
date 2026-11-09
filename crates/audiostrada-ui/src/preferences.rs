//! Preferences window: general options, audio settings and the keyboard shortcut editor.

use eframe::egui::{self, Color32, Event, Key, Rect, RichText, Sense, Stroke, pos2, vec2};

use audiostrada_plugins::Format;
use audiostrada_plugins::scan::{Database, Progress};

use crate::actions::Action;
use crate::host::AudioHost;
use crate::i18n::tr;
use crate::keymap::{Keymap, Preset, Shortcut};
use crate::palette::fuzzy_score;
use crate::settings::{AudioSettingsPanel, SettingsChange, WARN};
use crate::theme;
use crate::trf;
use crate::ui_prefs::{ThemeChoice, UiPrefs};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Appearance,
    General,
    Audio,
    Recording,
    Midi,
    Keyboard,
    Plugins,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Appearance,
        Tab::General,
        Tab::Audio,
        Tab::Midi,
        Tab::Recording,
        Tab::Keyboard,
        Tab::Plugins,
    ];

    fn title(self) -> &'static str {
        match self {
            Tab::Appearance => tr("Appearance"),
            Tab::General => tr("General"),
            Tab::Audio => tr("Audio"),
            Tab::Recording => tr("Recording"),
            Tab::Midi => tr("MIDI"),
            Tab::Keyboard => tr("Keyboard shortcuts"),
            Tab::Plugins => tr("Plugins"),
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Tab::Appearance => tr("Language, theme and the size of the interface."),
            Tab::General => tr("How playback stops, windows and new versions."),
            Tab::Audio => tr("The audio interface, its sample rate and its buffer."),
            Tab::Recording => tr("What is heard while recording, and where the takes land."),
            Tab::Midi => tr("Keyboards and controllers that play and control AudioStrada."),
            Tab::Keyboard => tr("Every command and its keys. Change records a new key."),
            Tab::Plugins => tr("Third-party plugins: where they are found and how they run."),
        }
    }

    /// The color of its tile in the sidebar.
    fn color(self) -> Color32 {
        match self {
            Tab::Appearance => Color32::from_rgb(132, 96, 232),
            Tab::General => Color32::from_rgb(124, 132, 146),
            Tab::Audio => Color32::from_rgb(46, 132, 238),
            Tab::Recording => Color32::from_rgb(226, 72, 72),
            Tab::Midi => Color32::from_rgb(28, 166, 156),
            Tab::Keyboard => Color32::from_rgb(96, 108, 138),
            Tab::Plugins => Color32::from_rgb(58, 168, 92),
        }
    }

    /// A small space before it in the sidebar, between groups.
    fn starts_group(self) -> bool {
        matches!(self, Tab::Audio | Tab::Keyboard)
    }
}

#[derive(Debug, Default)]
pub struct PreferencesWindow {
    open: bool,
    tab: Tab,
    search: String,
    recording: Option<Action>,
    notice: Option<String>,
    midi_ports: Option<Vec<String>>,
    midi_open: Vec<String>,
    /// The custom theme's dialog is open.
    custom_open: bool,
}

/// Changes made in the window this frame.
#[derive(Debug, Default, PartialEq)]
pub struct PrefsOutcome {
    pub ui_changed: bool,
    pub audio: SettingsChange,
    /// A plugin scan was requested; `true` rescans every file.
    pub rescan: Option<bool>,
    /// The set of enabled MIDI inputs changed.
    pub midi_changed: bool,
}

/// Width of the sidebar, and of the names of the settings on a page.
const SIDEBAR: f32 = 212.0;
const NAMES: f32 = 230.0;

impl PreferencesWindow {
    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn open_keyboard(&mut self) {
        self.open = true;
        self.tab = Tab::Keyboard;
    }

    pub fn open_audio(&mut self) {
        self.open = true;
        self.tab = Tab::Audio;
    }

    /// Inputs currently open, shown as available.
    pub fn set_midi_open(&mut self, open: Vec<String>) {
        self.midi_open = open;
    }

    /// True while waiting for a key to bind, so global shortcuts must not run.
    pub fn capturing_keys(&self) -> bool {
        self.open && self.recording.is_some()
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        prefs: &mut UiPrefs,
        audio: &mut AudioSettingsPanel,
        host: &AudioHost,
        mac: bool,
        plugins: (&Database, Option<&Progress>),
    ) -> PrefsOutcome {
        let mut outcome = PrefsOutcome::default();
        if !self.open {
            self.recording = None;
            self.custom_open = false;
            return outcome;
        }
        if let Some(action) = self.recording {
            self.capture(ctx, prefs, action, &mut outcome);
        }
        let mut open = self.open;
        let frame = egui::Frame::window(&ctx.global_style()).inner_margin(0);
        egui::Window::new(tr("Preferences"))
            .id(egui::Id::unique("preferences"))
            .open(&mut open)
            .frame(frame)
            // The title keeps its margins: the frame has none so the sidebar reaches its edges.
            .title_frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 10)))
            .default_size([900.0, 640.0])
            .min_size([760.0, 480.0])
            .collapsible(false)
            .show(ctx, |ui| {
                let height = ui.available_height().max(480.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.sidebar(ui, height);
                    ui.vertical(|ui| {
                        ui.set_min_height(height);
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: 28,
                                right: 20,
                                top: 22,
                                bottom: 12,
                            })
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                page_header(ui, self.tab);
                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_max_width(ui.available_width() - 8.0);
                                        self.page(
                                            ui,
                                            prefs,
                                            audio,
                                            host,
                                            mac,
                                            plugins,
                                            &mut outcome,
                                        );
                                    });
                            });
                    });
                });
            });
        self.open = open;
        outcome.ui_changed |= self.custom_dialog(ctx, prefs);
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn page(
        &mut self,
        ui: &mut egui::Ui,
        prefs: &mut UiPrefs,
        audio: &mut AudioSettingsPanel,
        host: &AudioHost,
        mac: bool,
        plugins: (&Database, Option<&Progress>),
        outcome: &mut PrefsOutcome,
    ) {
        match self.tab {
            Tab::Appearance => outcome.ui_changed |= self.appearance(ui, prefs),
            Tab::General => outcome.ui_changed |= general(ui, prefs),
            Tab::Audio => card(ui, None, |ui| outcome.audio = audio.show(ui, host)),
            Tab::Recording => outcome.ui_changed |= recording(ui, prefs),
            Tab::Midi => self.midi(ui, prefs, outcome),
            Tab::Keyboard => outcome.ui_changed |= self.keyboard(ui, prefs, mac),
            Tab::Plugins => plugins_tab(ui, prefs, plugins, outcome),
        }
    }

    /// The pages, each with its colored tile, as in a system's settings.
    fn sidebar(&mut self, ui: &mut egui::Ui, height: f32) {
        let p = theme::current(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(SIDEBAR, height), Sense::hover());
        let radius = (p.look.radius + 2).min(10);
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius {
                sw: radius,
                ..Default::default()
            },
            theme::mix(p.panel, p.base, if p.dark { 0.45 } else { 0.25 }),
        );
        ui.painter().vline(
            rect.right() - 0.5,
            rect.y_range(),
            Stroke::new(1.0, theme::mix(p.border, p.panel, 0.4)),
        );
        let mut y = rect.top() + 16.0;
        for tab in Tab::ALL {
            if tab.starts_group() {
                y += 12.0;
            }
            let item = Rect::from_min_size(pos2(rect.left() + 10.0, y), vec2(SIDEBAR - 20.0, 34.0));
            let response = ui.interact(
                item,
                ui.scope_id().with(("prefs-tab", tab as u8)),
                Sense::click(),
            );
            let selected = self.tab == tab;
            let fill = if selected {
                Some(p.accent.gamma_multiply(if p.dark { 0.28 } else { 0.18 }))
            } else if response.hovered() {
                Some(p.hover)
            } else {
                None
            };
            if let Some(fill) = fill {
                ui.painter().rect_filled(item, 7.0, fill);
            }
            let tile =
                Rect::from_center_size(pos2(item.left() + 19.0, item.center().y), vec2(24.0, 24.0));
            ui.painter().rect_filled(tile, 6.0, tab.color());
            tab_icon(ui.painter(), tile.shrink(5.0), tab);
            let font = if selected {
                theme::strong(13.0)
            } else {
                egui::FontId::proportional(13.0)
            };
            ui.painter().text(
                pos2(tile.right() + 11.0, item.center().y),
                egui::Align2::LEFT_CENTER,
                tab.title(),
                font,
                p.text,
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !selected
            {
                self.tab = tab;
                self.recording = None;
            }
            y += 38.0;
        }
        ui.painter().text(
            pos2(rect.left() + 20.0, rect.bottom() - 16.0),
            egui::Align2::LEFT_BOTTOM,
            format!("AudioStrada {}", env!("CARGO_PKG_VERSION")),
            egui::FontId::proportional(11.0),
            p.weak,
        );
    }

    fn appearance(&mut self, ui: &mut egui::Ui, prefs: &mut UiPrefs) -> bool {
        let mut changed = false;
        card(ui, Some(tr("Theme")), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                for theme in ThemeChoice::ALL {
                    let palette = theme.palette(&prefs.custom);
                    if theme_card(ui, theme.name(), palette, prefs.theme == theme).clicked() {
                        if theme == ThemeChoice::Custom {
                            self.custom_open = true;
                        }
                        prefs.theme = theme;
                        changed = true;
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        prefs.theme == ThemeChoice::Custom,
                        egui::Button::new(tr("Customize…")),
                    )
                    .on_disabled_hover_text(tr("Choose the Custom theme to make your own"))
                    .clicked()
                {
                    self.custom_open = true;
                }
                ui.label(
                    RichText::new(tr("Your own colors, kept readable whatever you choose."))
                        .small()
                        .color(theme::current(ui).weak),
                );
            });
        });
        card(ui, Some(tr("Interface")), |ui| {
            row(ui, tr("Language"), None, |ui| {
                egui::ComboBox::from_id_salt("language")
                    .width(200.0)
                    .selected_text(prefs.language.name())
                    .show_ui(ui, |ui| {
                        for language in crate::i18n::Language::ALL {
                            changed |= ui
                                .selectable_value(&mut prefs.language, language, language.name())
                                .changed();
                        }
                    });
            });
            divider(ui);
            row(
                ui,
                tr("Interface scale"),
                Some(tr("The size of everything, applied when you press Apply.")),
                |ui| {
                    ui.horizontal(|ui| {
                        // Applied on demand: the window would otherwise resize under the slider.
                        let id = egui::Id::unique("pending-scale");
                        // The value being chosen, forgotten when the scale changed elsewhere.
                        let mut scale = ui
                            .data(|d| d.get_temp::<(f32, f32)>(id))
                            .filter(|&(_, from)| from == prefs.scale)
                            .map_or(prefs.scale, |(scale, _)| scale);
                        ui.add(
                            egui::Slider::new(&mut scale, 0.75..=2.0)
                                .step_by(0.05)
                                .suffix("×"),
                        );
                        let pending = (scale - prefs.scale).abs() > 1e-3;
                        if ui
                            .add_enabled(pending, egui::Button::new(tr("Apply")))
                            .clicked()
                        {
                            prefs.scale = scale;
                            changed = true;
                        }
                        if pending && ui.button(tr("Cancel")).clicked() {
                            scale = prefs.scale;
                        }
                        ui.data_mut(|d| d.insert_temp(id, (scale, prefs.scale)));
                    });
                },
            );
            divider(ui);
            row(
                ui,
                tr("Master"),
                Some(tr("Where the master strip sits in the mixer.")),
                |ui| {
                    changed |= segmented(
                        ui,
                        &mut prefs.master_left,
                        &[(true, tr("At the left")), (false, tr("At the right"))],
                    );
                },
            );
            divider(ui);
            row(
                ui,
                tr("Mixer"),
                Some(tr("Narrow strips: a click opens one full width.")),
                |ui| {
                    changed |= ui
                        .checkbox(&mut prefs.mixer_compact, tr("Compact mixer"))
                        .changed();
                },
            );
            divider(ui);
            row(
                ui,
                tr("Timeline"),
                Some(tr(
                    "The master at the top of the timeline, to draw its automation.",
                )),
                |ui| {
                    changed |= ui
                        .checkbox(
                            &mut prefs.timeline_master,
                            tr("Master track in the timeline"),
                        )
                        .changed();
                },
            );
            divider(ui);
            row(
                ui,
                tr("Transport"),
                Some(tr("Where the transport bar sits, and its size.")),
                |ui| {
                    ui.horizontal(|ui| {
                        changed |= segmented(
                            ui,
                            &mut prefs.transport_bottom,
                            &[(false, tr("At the top")), (true, tr("At the bottom"))],
                        );
                        ui.add_space(12.0);
                        changed |= ui
                            .checkbox(&mut prefs.transport_large, tr("Large"))
                            .changed();
                    });
                },
            );
        });
        changed
    }

    /// The custom theme's dialog: a live picture of the theme beside its settings.
    fn custom_dialog(&mut self, ctx: &egui::Context, prefs: &mut UiPrefs) -> bool {
        if !self.custom_open {
            return false;
        }
        let mut changed = false;
        let mut done = false;
        let p = theme::current_for(ctx);
        let frame = egui::Frame::window(&ctx.global_style())
            .inner_margin(egui::Margin::same(24))
            .corner_radius((p.look.radius + 4).min(12));
        let modal = egui::Modal::new(egui::Id::unique("custom-theme"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(640.0);
                ui.label(RichText::new(tr("Custom theme")).font(theme::strong(18.0)));
                ui.label(
                    RichText::new(tr(
                        "Choose a background, an accent and a contrast: every other color \
                         follows, and nothing can become unreadable.",
                    ))
                    .color(p.weak),
                );
                ui.add_space(16.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        let palette = prefs.custom.palette();
                        theme_card(ui, tr("Preview"), palette, false);
                        ui.add_space(10.0);
                        swatches(ui, &palette);
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        changed |= custom_form(ui, &mut prefs.custom);
                    });
                });
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let done_button = egui::Button::new(
                            RichText::new(tr("Done"))
                                .strong()
                                .color(theme::text_on(p.accent)),
                        )
                        .fill(p.accent)
                        .min_size(vec2(96.0, 28.0));
                        if ui.add(done_button).clicked() {
                            done = true;
                        }
                    });
                });
            });
        if done || modal.should_close() {
            self.custom_open = false;
        }
        if changed {
            prefs.theme = ThemeChoice::Custom;
        }
        changed
    }

    /// Binds the next key press to `action`; Escape cancels.
    fn capture(
        &mut self,
        ctx: &egui::Context,
        prefs: &mut UiPrefs,
        action: Action,
        outcome: &mut PrefsOutcome,
    ) {
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        self.recording = None;
        if key == Key::Escape && !modifiers.any() {
            return;
        }
        let shortcut = Shortcut::new(modifiers.command, modifiers.shift, modifiers.alt, key);
        let label = shortcut.label(false);
        if let Some(previous) = prefs.keymap.bind(shortcut, action) {
            self.notice = Some(trf!(
                "{shortcut} was used by \"{previous}\"; it now runs \"{action}\".",
                shortcut = label,
                previous = previous.label(),
                action = action.label()
            ));
        } else {
            self.notice = None;
        }
        outcome.ui_changed = true;
    }

    fn midi(&mut self, ui: &mut egui::Ui, prefs: &mut UiPrefs, outcome: &mut PrefsOutcome) {
        let open = self.midi_open.clone();
        let mut refresh = false;
        let ports = self
            .midi_ports
            .get_or_insert_with(audiostrada_midi::devices::input_names)
            .clone();
        card(ui, Some(tr("Inputs")), |ui| {
            if ports.is_empty() {
                ui.label(RichText::new(tr("No MIDI input found.")).color(theme::current(ui).weak));
            }
            for (i, name) in ports.iter().enumerate() {
                if i > 0 {
                    divider(ui);
                }
                let mut enabled = !prefs.midi_disabled.contains(name);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut enabled, name).changed() {
                        if enabled {
                            prefs.midi_disabled.retain(|n| n != name);
                        } else {
                            prefs.midi_disabled.push(name.clone());
                        }
                        outcome.ui_changed = true;
                        outcome.midi_changed = true;
                    }
                    if enabled && !open.contains(name) {
                        ui.label(RichText::new(tr("unavailable")).small().color(WARN));
                    }
                });
            }
            ui.add_space(8.0);
            refresh = ui.button(tr("Refresh")).clicked();
        });
        if refresh {
            self.midi_ports = None;
            outcome.midi_changed = true;
        }
        hint(
            ui,
            tr(
                "Notes from every enabled input play the armed instrument tracks. Right-click a \
                 fader, knob or plugin parameter and choose MIDI learn to map a controller.",
            ),
        );
    }

    fn keyboard(&mut self, ui: &mut egui::Ui, prefs: &mut UiPrefs, mac: bool) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Preset")).strong());
            let current = prefs.keymap.preset;
            egui::ComboBox::from_id_salt("keymap-preset")
                .width(140.0)
                .selected_text(current.name())
                .show_ui(ui, |ui| {
                    for preset in Preset::ALL {
                        if ui
                            .selectable_label(current == preset, preset.name())
                            .clicked()
                            && current != preset
                        {
                            prefs.keymap = Keymap::preset(preset);
                            self.notice = Some(trf!(
                                "Shortcuts reset to the {preset} preset.",
                                preset = preset.name()
                            ));
                            changed = true;
                        }
                    }
                });
            ui.add_space(16.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text(tr("Search a command"))
                    .desired_width(260.0),
            );
        });
        if let Some(notice) = &self.notice {
            ui.add_space(4.0);
            ui.colored_label(WARN, notice);
        }
        ui.add_space(12.0);
        card(ui, None, |ui| {
            egui::Grid::new("shortcuts")
                .num_columns(4)
                .striped(true)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    for &action in Action::ALL {
                        let category = tr(action.category());
                        let shown = format!("{category} {}", action.label());
                        let english = format!("{} {}", action.category(), action.name());
                        if fuzzy_score(&self.search, &shown).is_none()
                            && fuzzy_score(&self.search, &english).is_none()
                        {
                            continue;
                        }
                        ui.label(RichText::new(category).weak());
                        ui.label(action.label());
                        let labels: Vec<String> = prefs
                            .keymap
                            .shortcuts_for(action)
                            .map(|s| s.label(mac))
                            .collect();
                        if self.recording == Some(action) {
                            ui.colored_label(WARN, tr("Press a key… (Esc cancels)"));
                        } else {
                            ui.monospace(if labels.is_empty() {
                                "—".to_owned()
                            } else {
                                labels.join(", ")
                            });
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button(tr("Change")).clicked() {
                                prefs.keymap.clear(action);
                                self.recording = Some(action);
                                changed = true;
                            }
                            if ui.small_button(tr("Add")).clicked() {
                                self.recording = Some(action);
                            }
                            if ui.small_button(tr("Clear")).clicked() {
                                prefs.keymap.clear(action);
                                changed = true;
                            }
                            if ui.small_button(tr("Reset")).clicked() {
                                prefs.keymap.reset(action);
                                changed = true;
                            }
                        });
                        ui.end_row();
                    }
                });
        });
        changed
    }
}

/// The title of a page and what it holds.
fn page_header(ui: &mut egui::Ui, tab: Tab) {
    let p = theme::current(ui);
    ui.label(RichText::new(tab.title()).font(theme::strong(22.0)));
    ui.label(RichText::new(tab.subtitle()).color(p.weak));
    ui.add_space(18.0);
}

/// A group of settings on a card, its title above it.
fn card(ui: &mut egui::Ui, title: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::current(ui);
    if let Some(title) = title {
        ui.label(
            RichText::new(title.to_uppercase())
                .size(10.5)
                .strong()
                .color(p.weak),
        );
        ui.add_space(4.0);
    }
    let (fill, edge) = theme::card(&p);
    let edge = if p.dark {
        Stroke::new(1.0, theme::mix(p.border, p.panel, 0.25))
    } else {
        edge
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(edge)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            add(ui);
        });
    ui.add_space(18.0);
}

/// One setting: its name and what it does at the left, its control at the right.
fn row(ui: &mut egui::Ui, name: &str, help: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::current(ui);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(NAMES);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(name).strong());
            if let Some(help) = help {
                ui.label(RichText::new(help).small().color(p.weak));
            }
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            add(ui);
        });
    });
}

/// A hairline between the settings of a card.
fn divider(ui: &mut egui::Ui) {
    let p = theme::current(ui);
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(
            1.0,
            theme::mix(p.border, p.panel, if p.dark { 0.35 } else { 0.6 }),
        ),
    );
    ui.add_space(4.0);
}

/// A note under the cards.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::current(ui).weak));
}

/// Choices side by side, as one control; the chosen one lit in the accent.
fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = theme::current(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for &(option, name) in options {
            let on = *value == option;
            let text = if on {
                RichText::new(name).strong().color(theme::text_on(p.accent))
            } else {
                RichText::new(name)
            };
            let mut button = egui::Button::new(text).min_size(vec2(64.0, 24.0));
            if on {
                button = button.fill(p.accent);
            }
            if ui.add(button).clicked() && !on {
                *value = option;
                changed = true;
            }
        }
    });
    changed
}

/// The settings of the custom theme, aligned in two columns.
fn custom_form(ui: &mut egui::Ui, custom: &mut crate::ui_prefs::CustomTheme) -> bool {
    use crate::theme::{CapStyle, MeterStyle};
    let p = theme::current(ui);
    let mut changed = false;
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02X}{g:02X}{b:02X}");
    let color_row = |ui: &mut egui::Ui, rgb: &mut [u8; 3]| -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            let button = ui.color_edit_button_srgb(rgb);
            changed |= button.changed();
            ui.label(RichText::new(hex(*rgb)).monospace().color(p.weak));
        });
        changed
    };
    egui::Grid::new("custom-theme")
        .num_columns(2)
        .min_col_width(110.0)
        .spacing([18.0, 14.0])
        .show(ui, |ui| {
            ui.label(RichText::new(tr("Start from")).strong());
            ui.horizontal(|ui| {
                for theme in [ThemeChoice::Dark, ThemeChoice::Studio, ThemeChoice::Light] {
                    if ui
                        .add(egui::Button::new(theme.name()).min_size(vec2(64.0, 24.0)))
                        .clicked()
                    {
                        let base = crate::ui_prefs::CustomTheme::default();
                        *custom = crate::ui_prefs::CustomTheme::from_palette(theme.palette(&base));
                        changed = true;
                    }
                }
            });
            ui.end_row();
            ui.label(RichText::new(tr("Background")).strong());
            changed |= color_row(ui, &mut custom.background);
            ui.end_row();
            ui.label(RichText::new(tr("Accent")).strong());
            changed |= color_row(ui, &mut custom.accent);
            ui.end_row();
            ui.label(RichText::new(tr("Contrast")).strong());
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Soft")).small().color(p.weak));
                changed |= ui
                    .add(egui::Slider::new(&mut custom.contrast, 0.0..=1.0).show_value(false))
                    .changed();
                ui.label(RichText::new(tr("Strong")).small().color(p.weak));
            });
            ui.end_row();
            ui.label(RichText::new(tr("Corners")).strong());
            changed |= ui
                .add(egui::Slider::new(&mut custom.radius, 0..=10).suffix(" px"))
                .changed();
            ui.end_row();
            ui.label(RichText::new(tr("Meters")).strong());
            changed |= segmented(
                ui,
                &mut custom.meter,
                &[
                    (MeterStyle::Gradient, tr("Gradient")),
                    (MeterStyle::Segments, tr("Lights")),
                    (MeterStyle::Glow, tr("Glow")),
                ],
            );
            ui.end_row();
            ui.label(RichText::new(tr("Fader caps")).strong());
            changed |= segmented(
                ui,
                &mut custom.cap,
                &[
                    (CapStyle::Modern, tr("Flat")),
                    (CapStyle::Classic, tr("Ridged")),
                ],
            );
            ui.end_row();
        });
    // What was corrected to stay readable, said once.
    let palette = custom.palette();
    let color = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
    let mut notes = Vec::new();
    if palette.panel != color(custom.background) {
        notes.push(tr(
            "The background is kept clearly dark or clearly light, and lightly colored.",
        ));
    }
    if palette.accent != color(custom.accent) {
        notes.push(tr(
            "The accent is adjusted to stand out from the background and from the record red.",
        ));
    }
    if !notes.is_empty() {
        ui.add_space(10.0);
        for note in notes {
            ui.label(RichText::new(note).small().color(p.weak));
        }
    }
    changed
}

/// The colors a palette is made of, in small squares.
fn swatches(ui: &mut egui::Ui, p: &theme::Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for color in [p.base, p.panel, p.surface, p.text, p.weak, p.accent] {
            let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, color);
            ui.painter().rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, theme::mix(p.border, p.panel, 0.3)),
                egui::StrokeKind::Inside,
            );
        }
    });
}

/// The glyph of a page, in white on its tile.
fn tab_icon(painter: &egui::Painter, r: Rect, tab: Tab) {
    let ink = Color32::WHITE;
    let stroke = Stroke::new(1.6, ink);
    let c = r.center();
    let s = r.width() / 2.0;
    match tab {
        Tab::Appearance => {
            painter.circle_stroke(c, s * 0.85, stroke);
            let half: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    c + vec2(a.cos(), a.sin()) * s * 0.85
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(half, ink, Stroke::NONE));
        }
        Tab::General => {
            painter.circle_stroke(c, s * 0.45, stroke);
            for k in 0..8 {
                let a = std::f32::consts::TAU * k as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                painter.line_segment([c + d * s * 0.62, c + d * s * 0.95], stroke);
            }
        }
        Tab::Audio => {
            let body = [
                c + vec2(-s * 0.9, -s * 0.3),
                c + vec2(-s * 0.45, -s * 0.3),
                c + vec2(s * 0.1, -s * 0.8),
                c + vec2(s * 0.1, s * 0.8),
                c + vec2(-s * 0.45, s * 0.3),
                c + vec2(-s * 0.9, s * 0.3),
            ];
            painter.add(egui::Shape::convex_polygon(
                body.to_vec(),
                ink,
                Stroke::NONE,
            ));
            let arc: Vec<egui::Pos2> = (0..=8)
                .map(|i| {
                    let a = -0.8 + 1.6 * i as f32 / 8.0;
                    c + vec2(s * 0.1, 0.0) + vec2(a.cos(), a.sin()) * s * 0.7
                })
                .collect();
            painter.add(egui::Shape::line(arc, stroke));
        }
        Tab::Recording => {
            painter.circle_stroke(c, s * 0.9, stroke);
            painter.circle_filled(c, s * 0.5, ink);
        }
        Tab::Midi => {
            painter.circle_stroke(c, s * 0.9, stroke);
            for k in 0..5 {
                let a = std::f32::consts::PI * (0.15 + 0.7 * k as f32 / 4.0);
                painter.circle_filled(c - vec2(a.cos(), a.sin()) * s * 0.5, 1.4, ink);
            }
        }
        Tab::Keyboard => {
            let body = Rect::from_center_size(c, vec2(s * 1.9, s * 1.3));
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Middle);
            for k in 0..3 {
                let x = body.left() + body.width() * (0.27 + 0.23 * k as f32);
                painter.circle_filled(pos2(x, body.top() + body.height() * 0.35), 1.2, ink);
            }
            painter.line_segment(
                [
                    pos2(
                        body.left() + body.width() * 0.28,
                        body.top() + body.height() * 0.7,
                    ),
                    pos2(
                        body.right() - body.width() * 0.28,
                        body.top() + body.height() * 0.7,
                    ),
                ],
                stroke,
            );
        }
        Tab::Plugins => {
            let body = Rect::from_center_size(c + vec2(0.0, s * 0.15), vec2(s * 1.1, s * 0.9));
            painter.rect_filled(body, 2.0, ink);
            for dx in [-0.3, 0.3] {
                painter.line_segment(
                    [
                        pos2(c.x + dx * s, body.top()),
                        pos2(c.x + dx * s, body.top() - s * 0.45),
                    ],
                    stroke,
                );
            }
            painter.line_segment([pos2(c.x, body.bottom()), pos2(c.x, c.y + s)], stroke);
        }
    }
}

fn general(ui: &mut egui::Ui, prefs: &mut UiPrefs) -> bool {
    let mut changed = false;
    card(ui, Some(tr("Transport")), |ui| {
        row(
            ui,
            tr("Play key"),
            Some(tr("What stopping playback does to the playhead.")),
            |ui| {
                changed |= ui
                    .radio_value(
                        &mut prefs.return_on_stop,
                        true,
                        tr("Stop goes back to where playback started"),
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut prefs.return_on_stop,
                        false,
                        tr("Stop stays where it is (pause)"),
                    )
                    .changed();
            },
        );
    });
    if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        card(ui, Some(tr("Windows")), |ui| {
            row(
                ui,
                tr("Several screens"),
                Some(tr(
                    "Under Wayland the system places the windows, so they all open on the same \
                     screen, and effect and plugin windows can fall behind AudioStrada. In X11 mode \
                     (XWayland) AudioStrada puts them back where they were and keeps effect and plugin \
                     windows above it, but with a fractional display scale (125 %, 150 %) they \
                     may look blurry. On by default. Takes effect when AudioStrada starts again.",
                )),
                |ui| {
                    changed |= ui
                        .checkbox(
                            &mut prefs.x11_windows,
                            tr("Put each window back on its screen (X11 mode)"),
                        )
                        .changed();
                },
            );
        });
    }
    if audiostrada_license::OFFICIAL {
        card(ui, Some(tr("Updates")), |ui| {
            row(ui, tr("New versions"), None, |ui| {
                let mut check = prefs.check_updates == Some(true);
                if ui.checkbox(&mut check, tr("Check once a day")).changed() {
                    prefs.check_updates = Some(check);
                    changed = true;
                }
            });
        });
    }
    changed
}

fn recording(ui: &mut egui::Ui, prefs: &mut UiPrefs) -> bool {
    let mut changed = false;
    card(ui, Some(tr("Input monitoring")), |ui| {
        row(
            ui,
            tr("Armed tracks"),
            Some(tr(
                "For the tracks whose speaker button is on Auto. The button can also make a track \
                 always hear its input, armed or not, or never.",
            )),
            |ui| {
                changed |= ui
                    .radio_value(
                        &mut prefs.auto_monitoring,
                        false,
                        tr("Armed tracks always hear their input"),
                    )
                    .changed();
                changed |= ui
                    .radio_value(
                        &mut prefs.auto_monitoring,
                        true,
                        tr("Automatic: input while stopped or recording, the track while playing"),
                    )
                    .changed();
            },
        );
    });
    card(ui, Some(tr("Takes")), |ui| {
        row(
            ui,
            tr("Recording latency offset"),
            Some(tr(
                "Recorded clips are moved earlier by this amount. Measure it with `audiostrada latency`.",
            )),
            |ui| {
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut prefs.record_offset)
                            .range(-48_000..=48_000)
                            .suffix(tr(" samples")),
                    )
                    .changed();
            },
        );
    });
    changed
}

fn plugins_tab(
    ui: &mut egui::Ui,
    prefs: &mut UiPrefs,
    (database, scan): (&Database, Option<&Progress>),
    outcome: &mut PrefsOutcome,
) {
    let scanning = scan.is_some();
    let enabled = audiostrada_plugins::enabled();
    card(ui, Some(tr("Third-party plugins")), |ui| {
        row(
            ui,
            tr("Plugins"),
            Some(tr(
                "They are neither scanned nor loaded; their inserts keep their settings. Applies at \
                 the next start. For one session: audiostrada --disable-third-party-plugins",
            )),
            |ui| {
                if ui
                    .checkbox(
                        &mut prefs.disable_plugins,
                        tr("Disable third-party plugins (CLAP, VST3, VST2, LV2, AU)"),
                    )
                    .changed()
                {
                    outcome.ui_changed = true;
                }
                if !enabled {
                    ui.colored_label(
                        WARN,
                        tr("Third-party plugins are disabled for this session."),
                    );
                } else if prefs.disable_plugins {
                    ui.colored_label(WARN, tr("Restart AudioStrada to apply."));
                }
            },
        );
        divider(ui);
        row(
            ui,
            tr("Sandbox"),
            Some(tr(
                "A plugin that crashes or hangs is stopped without taking AudioStrada down; it can be \
                 reloaded from its insert menu. Applies to plugins loaded from now on.",
            )),
            |ui| {
                if ui
                    .checkbox(
                        &mut prefs.plugin_sandbox,
                        tr("Run each plugin in its own process (sandbox)"),
                    )
                    .changed()
                {
                    prefs.apply_sandbox();
                    outcome.ui_changed = true;
                }
            },
        );
    });
    card(ui, Some(tr("Scan")), |ui| {
        let count = database.plugins().count();
        row(
            ui,
            &trf!("{count} plugin(s) found", count = count),
            None,
            |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            enabled && !scanning,
                            egui::Button::new(tr("Scan for new plugins")),
                        )
                        .clicked()
                    {
                        outcome.rescan = Some(false);
                    }
                    if ui
                        .add_enabled(
                            enabled && !scanning,
                            egui::Button::new(tr("Rescan everything")),
                        )
                        .on_hover_text(tr("Forget the cached list and scan every plugin again"))
                        .clicked()
                    {
                        outcome.rescan = Some(true);
                    }
                    if let Some(scan) = scan {
                        ui.spinner();
                        ui.weak(crate::effect_browser::scan_status(scan));
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(250));
                    }
                });
            },
        );
        let failures: Vec<_> = database.failures().collect();
        if !failures.is_empty() {
            divider(ui);
            ui.colored_label(
                WARN,
                trf!(
                    "{count} file(s) could not be loaded",
                    count = failures.len()
                ),
            );
            for (path, reason) in failures {
                ui.label(RichText::new(format!("{}: {reason}", path.display())).small());
            }
        }
    });
    card(ui, Some(tr("Folders")), |ui| {
        let p = theme::current(ui);
        row(ui, tr("Standard folders"), None, |ui| {
            for format in Format::ALL {
                for dir in audiostrada_plugins::scan::default_dirs(format) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format.name()).small().strong().color(p.weak));
                        ui.label(
                            RichText::new(dir.display().to_string())
                                .small()
                                .color(p.weak),
                        );
                    });
                }
            }
        });
        divider(ui);
        row(ui, tr("Additional folders"), None, |ui| {
            let mut removed = None;
            for (i, dir) in prefs.plugin_dirs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(dir.display().to_string());
                    if ui.small_button(tr("Remove")).clicked() {
                        removed = Some(i);
                    }
                });
            }
            if let Some(i) = removed {
                prefs.plugin_dirs.remove(i);
                outcome.ui_changed = true;
            }
            let open = ui.button(tr("Add folder…")).clicked();
            if let Some(dir) = crate::dialogs::ask(ui.ctx(), "plugin-folder", open, || {
                rfd::FileDialog::new()
                    .set_title(tr("Plugin folder"))
                    .pick_folder()
            }) && !prefs.plugin_dirs.contains(&dir)
            {
                prefs.plugin_dirs.push(dir);
                outcome.ui_changed = true;
                outcome.rescan = Some(false);
            }
        });
    });
}

/// A small picture of a theme: a strip with its meter, fader cap and accent, rounded as the
/// theme rounds; the chosen one is outlined.
fn theme_card(
    ui: &mut egui::Ui,
    name: &str,
    p: crate::theme::Palette,
    chosen: bool,
) -> egui::Response {
    use crate::theme::{CapStyle, MeterStyle};
    use eframe::egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
    let (rect, response) = ui.allocate_exact_size(vec2(132.0, 92.0), Sense::click());
    let painter = ui.painter_at(rect.expand(2.0));
    let radius = f32::from(p.look.radius) + 2.0;
    painter.rect_filled(rect, radius, p.panel);
    let strip = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(46.0, 56.0));
    painter.rect_filled(strip, f32::from(p.look.radius), p.surface);
    // Meter.
    for (i, db) in [-8.0f32, -14.0].into_iter().enumerate() {
        let x = strip.left() + 30.0 + i as f32 * 6.0;
        let bar = Rect::from_min_max(
            pos2(x, strip.top() + 6.0),
            pos2(x + 5.0, strip.bottom() - 6.0),
        );
        painter.rect_filled(bar, 1.0, p.base);
        let lit = Rect::from_min_max(
            pos2(
                x,
                bar.bottom() - bar.height() * crate::widgets::meter_fraction(db),
            ),
            bar.max,
        );
        match p.look.meter {
            MeterStyle::Segments => crate::widgets::paint_meter_lights(&painter, bar, db),
            MeterStyle::Glow => {
                painter.rect_filled(
                    lit.expand2(vec2(1.5, 2.0)),
                    3.0,
                    Color32::from_rgba_unmultiplied(80, 200, 120, 50),
                );
                crate::widgets::paint_meter_bar(&painter, bar, lit);
            }
            MeterStyle::Gradient => crate::widgets::paint_meter_bar(&painter, bar, lit),
        }
    }
    // Fader.
    let track =
        Rect::from_center_size(pos2(strip.left() + 14.0, strip.center().y), vec2(3.0, 44.0));
    painter.rect_filled(track, 1.0, p.border);
    let cap = Rect::from_center_size(
        pos2(track.center().x, strip.center().y + 4.0),
        vec2(14.0, 18.0),
    );
    match p.look.cap {
        CapStyle::Modern => {
            painter.rect_filled(cap, f32::from(p.look.radius.min(3)), p.hover);
            painter.line_segment(
                [
                    pos2(cap.left() + 2.0, cap.center().y),
                    pos2(cap.right() - 2.0, cap.center().y),
                ],
                Stroke::new(1.5, p.text),
            );
        }
        CapStyle::Classic => {
            painter.rect_filled(
                cap,
                f32::from(p.look.radius.min(3)),
                Color32::from_gray(196),
            );
            for dy in [-5.0, -3.0, 3.0, 5.0] {
                painter.line_segment(
                    [
                        pos2(cap.left() + 1.0, cap.center().y + dy),
                        pos2(cap.right() - 1.0, cap.center().y + dy),
                    ],
                    Stroke::new(1.0, Color32::from_black_alpha(70)),
                );
            }
            painter.line_segment(
                [
                    pos2(cap.left(), cap.center().y),
                    pos2(cap.right(), cap.center().y),
                ],
                Stroke::new(1.5, Color32::from_gray(20)),
            );
        }
    }
    // Accent: a lit button and a line of text.
    let button = Rect::from_min_size(pos2(strip.right() + 10.0, strip.top()), vec2(56.0, 18.0));
    painter.rect_filled(button, f32::from(p.look.radius), p.accent);
    painter.text(
        button.center(),
        Align2::CENTER_CENTER,
        tr("Play"),
        FontId::proportional(10.0),
        Color32::WHITE,
    );
    let field = Rect::from_min_size(pos2(button.left(), button.bottom() + 6.0), vec2(56.0, 14.0));
    painter.rect_filled(field, f32::from(p.look.radius), p.base);
    painter.text(
        field.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        "-6.0",
        FontId::monospace(9.0),
        p.text,
    );
    painter.text(
        pos2(button.left(), field.bottom() + 6.0),
        Align2::LEFT_TOP,
        "Mix",
        FontId::proportional(9.5),
        p.weak,
    );
    painter.text(
        pos2(rect.left() + 10.0, rect.bottom() - 8.0),
        Align2::LEFT_BOTTOM,
        name,
        crate::theme::strong(11.5),
        p.text,
    );
    let outline = if chosen {
        Stroke::new(2.0, p.accent)
    } else if response.hovered() {
        Stroke::new(1.0, p.weak)
    } else {
        Stroke::new(1.0, p.border)
    };
    painter.rect_stroke(rect, radius, outline, StrokeKind::Outside);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, RawInput, Rect, vec2};

    #[test]
    fn every_tab_draws_on_every_system() {
        let mut host = AudioHost::new(Box::new(|_| {
            Box::new(
                |_: &mut audiostrada_io::AudioBuffers<'_>, _: &audiostrada_io::CallbackTime| {},
            )
        }));
        host.start(
            Some(audiostrada_io::BackendKind::Dummy),
            audiostrada_io::StreamConfig::default(),
        );
        let ctx = egui::Context::default();
        let mut prefs = UiPrefs::default();
        let mut audio = AudioSettingsPanel::default();
        let database = Database::default();
        for tab in Tab::ALL {
            for mac in [false, true] {
                let mut window = PreferencesWindow::default();
                window.open();
                window.tab = tab;
                window.set_midi_open(vec!["Keyboard".into()]);
                for _ in 0..2 {
                    let input = RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                        ..Default::default()
                    };
                    ctx.run_ui(input, |ui| {
                        let outcome = window.show(
                            ui.ctx(),
                            &mut prefs,
                            &mut audio,
                            &host,
                            mac,
                            (&database, Some(&Progress::default())),
                        );
                        assert!(!outcome.midi_changed);
                    })
                    .drop_without_applying_deltas();
                }
            }
        }
    }

    #[test]
    fn a_shortcut_is_recorded_from_the_next_key() {
        let ctx = egui::Context::default();
        let mut prefs = UiPrefs::default();
        let host = AudioHost::new(Box::new(|_| unreachable!("no stream in this test")));
        let mut audio = AudioSettingsPanel::default();
        let mut window = PreferencesWindow::default();
        window.open_keyboard();
        window.recording = Some(Action::ToggleMetronome);
        assert!(window.capturing_keys());
        let key = egui::Event::Key {
            key: egui::Key::J,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        let input = RawInput {
            events: vec![key],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            window.show(
                ui.ctx(),
                &mut prefs,
                &mut audio,
                &host,
                false,
                (&Database::default(), None),
            );
        })
        .drop_without_applying_deltas();
        assert!(!window.capturing_keys());
        assert_eq!(
            prefs
                .keymap
                .action_for(egui::Key::J, egui::Modifiers::COMMAND),
            Some(Action::ToggleMetronome)
        );
    }
}
