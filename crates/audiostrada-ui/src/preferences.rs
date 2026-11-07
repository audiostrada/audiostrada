//! Preferences window: general options, audio settings and the keyboard shortcut editor.

use eframe::egui::{self, Event, Key, RichText};

use audiostrada_plugins::Format;
use audiostrada_plugins::scan::Database;

use crate::actions::Action;
use crate::host::AudioHost;
use crate::i18n::tr;
use crate::keymap::{Keymap, Preset, Shortcut};
use crate::palette::fuzzy_score;
use crate::settings::{AudioSettingsPanel, SettingsChange, WARN};
use crate::trf;
use crate::ui_prefs::{ThemeChoice, UiPrefs};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    General,
    Audio,
    Keyboard,
    Plugins,
    Midi,
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
        plugins: (&Database, bool),
    ) -> PrefsOutcome {
        let mut outcome = PrefsOutcome::default();
        if !self.open {
            self.recording = None;
            return outcome;
        }
        if let Some(action) = self.recording {
            self.capture(ctx, prefs, action, &mut outcome);
        }
        let mut open = self.open;
        egui::Window::new(tr("Preferences"))
            .id(egui::Id::unique("preferences"))
            .open(&mut open)
            .default_size([700.0, 640.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::General, tr("General"));
                    ui.selectable_value(&mut self.tab, Tab::Audio, tr("Audio"));
                    ui.selectable_value(&mut self.tab, Tab::Keyboard, tr("Keyboard"));
                    ui.selectable_value(&mut self.tab, Tab::Plugins, tr("Plugins"));
                    ui.selectable_value(&mut self.tab, Tab::Midi, tr("MIDI"));
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                    Tab::General => outcome.ui_changed |= general(ui, prefs),
                    Tab::Audio => outcome.audio = audio.show(ui, host),
                    Tab::Keyboard => outcome.ui_changed |= self.keyboard(ui, prefs, mac),
                    Tab::Plugins => plugins_tab(ui, prefs, plugins, &mut outcome),
                    Tab::Midi => self.midi(ui, prefs, &mut outcome),
                });
            });
        self.open = open;
        outcome
    }

    /// Inputs currently open, shown as available.
    pub fn set_midi_open(&mut self, open: Vec<String>) {
        self.midi_open = open;
    }

    fn midi(&mut self, ui: &mut egui::Ui, prefs: &mut UiPrefs, outcome: &mut PrefsOutcome) {
        let open = &self.midi_open;
        let ports = self
            .midi_ports
            .get_or_insert_with(audiostrada_midi::devices::input_names);
        ui.label(RichText::new(tr("Inputs")).strong());
        if ports.is_empty() {
            ui.weak(tr("No MIDI input found."));
        }
        for name in ports.iter() {
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
                    ui.weak(tr("unavailable"));
                }
            });
        }
        if ui.button(tr("Refresh")).clicked() {
            self.midi_ports = None;
            outcome.midi_changed = true;
        }
        ui.add_space(8.0);
        ui.weak(tr(
            "Notes from every enabled input play the armed instrument tracks. Right-click a \
             fader, knob or plugin parameter and choose MIDI learn to map a controller.",
        ));
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

    fn keyboard(&mut self, ui: &mut egui::Ui, prefs: &mut UiPrefs, mac: bool) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(tr("Preset"));
            let current = prefs.keymap.preset;
            egui::ComboBox::from_id_salt("keymap-preset")
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
            ui.add_space(12.0);
            ui.label(tr("Search"));
            ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(220.0));
        });
        if let Some(notice) = &self.notice {
            ui.colored_label(WARN, notice);
        }
        ui.add_space(6.0);
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
        changed
    }
}

fn plugins_tab(
    ui: &mut egui::Ui,
    prefs: &mut UiPrefs,
    (database, scanning): (&Database, bool),
    outcome: &mut PrefsOutcome,
) {
    let enabled = audiostrada_plugins::enabled();
    if ui
        .checkbox(
            &mut prefs.disable_plugins,
            tr("Disable third-party plugins (CLAP, VST3, VST2, LV2, AU)"),
        )
        .on_hover_text(tr(
            "They are neither scanned nor loaded; their inserts keep their settings. Applies at \
             the next start. For one session: audiostrada --disable-third-party-plugins",
        ))
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
        ui.weak(tr("Restart AudioStrada to apply."));
    }
    ui.add_space(4.0);
    let count = database.plugins().count();
    ui.horizontal(|ui| {
        ui.label(trf!("{count} plugin(s) found", count = count));
        if scanning {
            ui.spinner();
            ui.weak(tr("scanning…"));
        }
    });
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
    });
    ui.add_space(4.0);
    if ui
        .checkbox(
            &mut prefs.plugin_sandbox,
            tr("Run each plugin in its own process (sandbox)"),
        )
        .on_hover_text(tr(
            "A plugin that crashes or hangs is stopped without taking AudioStrada down; it can be \
             reloaded from its insert menu. Applies to plugins loaded from now on.",
        ))
        .changed()
    {
        prefs.apply_sandbox();
        outcome.ui_changed = true;
    }
    ui.add_space(8.0);
    ui.label(RichText::new(tr("Standard folders")).strong());
    for format in Format::ALL {
        for dir in audiostrada_plugins::scan::default_dirs(format) {
            ui.weak(format!("{}  {}", format.name(), dir.display()));
        }
    }
    ui.add_space(8.0);
    ui.label(RichText::new(tr("Additional folders")).strong());
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
    let failures: Vec<_> = database.failures().collect();
    if !failures.is_empty() {
        ui.add_space(8.0);
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
}

fn general(ui: &mut egui::Ui, prefs: &mut UiPrefs) -> bool {
    let mut changed = false;
    crate::theme::section(ui, tr("Appearance"));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Language")).strong());
        egui::ComboBox::from_id_salt("language")
            .selected_text(prefs.language.name())
            .show_ui(ui, |ui| {
                for language in crate::i18n::Language::ALL {
                    changed |= ui
                        .selectable_value(&mut prefs.language, language, language.name())
                        .changed();
                }
            });
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr("Theme")).strong());
    for row in ThemeChoice::ALL.chunks(4) {
        ui.horizontal(|ui| {
            for &theme in row {
                let palette = theme.palette_with(prefs.accent);
                if theme_card(ui, theme.name(), palette, prefs.theme == theme).clicked() {
                    prefs.theme = theme;
                    changed = true;
                }
            }
        });
    }
    ui.horizontal(|ui| {
        let mut own = prefs.accent.is_some();
        if ui.checkbox(&mut own, tr("Own accent color")).changed() {
            let accent = prefs.theme.palette().accent;
            prefs.accent = own.then_some([accent.r(), accent.g(), accent.b()]);
            changed = true;
        }
        if let Some(accent) = &mut prefs.accent {
            changed |= ui.color_edit_button_srgb(accent).changed();
        }
    });
    crate::theme::section(ui, tr("Workspace"));
    egui::Grid::new("general").num_columns(2).spacing([24.0, 10.0]).show(ui, |ui| {
        ui.label(tr("Interface scale"));
        // Applied on demand: the window would otherwise resize under the slider being dragged.
        ui.horizontal(|ui| {
            let id = egui::Id::unique("pending-scale");
            // The value being chosen, forgotten when the scale changed elsewhere.
            let mut scale = ui
                .data(|d| d.get_temp::<(f32, f32)>(id))
                .filter(|&(_, from)| from == prefs.scale)
                .map_or(prefs.scale, |(scale, _)| scale);
            ui.add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05).suffix("×"));
            let pending = (scale - prefs.scale).abs() > 1e-3;
            if ui.add_enabled(pending, egui::Button::new(tr("Apply"))).clicked() {
                prefs.scale = scale;
                changed = true;
            }
            if pending && ui.button(tr("Cancel")).clicked() {
                scale = prefs.scale;
            }
            ui.data_mut(|d| d.insert_temp(id, (scale, prefs.scale)));
        });
        ui.end_row();
        ui.label(tr("Transport"));
        ui.horizontal(|ui| {
            changed |= ui.radio_value(&mut prefs.transport_bottom, false, tr("At the top")).changed();
            changed |= ui.radio_value(&mut prefs.transport_bottom, true, tr("At the bottom")).changed();
            ui.add_space(12.0);
            changed |= ui.checkbox(&mut prefs.transport_large, tr("Large")).changed();
        });
        ui.end_row();
        ui.label(tr("Recording latency offset"));
        ui.vertical(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut prefs.record_offset).range(-48_000..=48_000).suffix(tr(" samples")))
                .changed();
            ui.weak(tr("Recorded clips are moved earlier by this amount. Measure it with `audiostrada latency`."));
        });
        ui.end_row();
        ui.label(tr("Recording mode"));
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(tr("Audio buffer"));
                egui::ComboBox::from_id_salt("recording-buffer")
                    .selected_text(trf!("{frames} samples", frames = prefs.recording_buffer))
                    .show_ui(ui, |ui| {
                        for frames in [32, 64, 128, 256] {
                            changed |= ui
                                .selectable_value(&mut prefs.recording_buffer, frames, trf!("{frames} samples", frames = frames))
                                .changed();
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(tr("Bypass the effects of the master and folders adding more than"));
                changed |= ui
                    .add(egui::DragValue::new(&mut prefs.recording_bypass_ms).range(0.0..=50.0).speed(0.1).suffix(" ms"))
                    .changed();
            });
            changed |= ui.checkbox(&mut prefs.recording_offer, tr("Offer it when a track is armed")).changed();
            ui.weak(tr("A layout for recording musicians: the armed inputs, the timeline and the takes. Leaving it puts everything back."));
        });
        ui.end_row();
        ui.label(tr("Input monitoring"));
        ui.vertical(|ui| {
            changed |= ui.radio_value(&mut prefs.auto_monitoring, false, tr("Armed tracks always hear their input")).changed();
            changed |= ui
                .radio_value(&mut prefs.auto_monitoring, true, tr("Automatic: input while stopped or recording, the track while playing"))
                .changed();
            ui.weak(tr("For the tracks whose speaker button is on Auto. The button can also make a track always hear its input, armed or not, or never."));
        });
        ui.end_row();
        ui.label(tr("Play key"));
        ui.vertical(|ui| {
            changed |= ui.radio_value(&mut prefs.return_on_stop, true, tr("Stop goes back to where playback started")).changed();
            changed |= ui.radio_value(&mut prefs.return_on_stop, false, tr("Stop stays where it is (pause)")).changed();
        });
        ui.end_row();
        if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
            ui.label(tr("Several screens"));
            ui.vertical(|ui| {
                changed |= ui
                    .checkbox(&mut prefs.x11_windows, tr("Put each window back on its screen (X11 mode)"))
                    .changed();
                ui.weak(tr(
                    "Under Wayland the system places the windows, so they all open on the same \
                     screen, and effect and plugin windows can fall behind AudioStrada. In X11 mode \
                     (XWayland) AudioStrada puts them back where they were and keeps effect and plugin \
                     windows above it, but with a fractional display scale (125 %, 150 %) they \
                     may look blurry. On by default. Takes effect when AudioStrada starts again.",
                ));
            });
            ui.end_row();
        }
        if audiostrada_license::OFFICIAL {
            ui.label(tr("New versions"));
            let mut check = prefs.check_updates == Some(true);
            if ui.checkbox(&mut check, tr("Check once a day")).changed() {
                prefs.check_updates = Some(check);
                changed = true;
            }
            ui.end_row();
        }
    });
    changed
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
        for tab in [
            Tab::General,
            Tab::Audio,
            Tab::Keyboard,
            Tab::Plugins,
            Tab::Midi,
        ] {
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
                            (&database, true),
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
                (&Database::default(), false),
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
