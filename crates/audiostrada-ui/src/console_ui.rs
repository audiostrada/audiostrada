//! The console color: its picker in the mixer's bar, each track's own, and gain staging to the
//! consoles' working level.

use audiostrada_core::TrackId;
use audiostrada_core::console::{Console, ConsoleModel};
use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

/// Working level of the consoles: what gain staging aims at.
pub const WORKING_DB: f32 = -18.0;

pub fn color(model: ConsoleModel) -> Color32 {
    match model {
        ConsoleModel::Clean => Color32::from_gray(150),
        ConsoleModel::Iron => Color32::from_rgb(228, 156, 60),
        ConsoleModel::Imperial => Color32::from_rgb(143, 168, 162),
        ConsoleModel::Discrete => Color32::from_rgb(79, 163, 224),
        ConsoleModel::Velvet => Color32::from_rgb(186, 98, 146),
    }
}

/// The consoles in the order they are offered: the brightest first.
const SHOWN: [ConsoleModel; 5] = [
    ConsoleModel::Clean,
    ConsoleModel::Imperial,
    ConsoleModel::Iron,
    ConsoleModel::Discrete,
    ConsoleModel::Velvet,
];

pub fn name(model: ConsoleModel) -> &'static str {
    match model {
        ConsoleModel::Clean => tr("Neutral"),
        other => other.name(),
    }
}

fn hint(model: ConsoleModel) -> &'static str {
    match model {
        ConsoleModel::Clean => tr("No color: the mix as it is"),
        ConsoleModel::Iron => tr("Transformers: round lows, a silky top"),
        ConsoleModel::Imperial => tr("Tight and precise, a touch of air"),
        ConsoleModel::Discrete => tr("Discrete amplifiers: punch and attack"),
        ConsoleModel::Velvet => tr("Smooth and warm, very low distortion"),
    }
}

/// What a console is inspired by.
fn inspiration(model: ConsoleModel) -> Option<&'static str> {
    match model {
        ConsoleModel::Clean => None,
        ConsoleModel::Iron => Some(tr("Inspired by Neve consoles")),
        ConsoleModel::Imperial => Some(tr("Inspired by SSL consoles")),
        ConsoleModel::Discrete => Some(tr("Inspired by API consoles")),
        ConsoleModel::Velvet => Some(tr("Inspired by Harrison consoles")),
    }
}

/// A console's character, and what inspired it.
fn description(model: ConsoleModel) -> String {
    match inspiration(model) {
        Some(source) => format!("{}\n{source}", hint(model)),
        None => hint(model).to_owned(),
    }
}

/// The 2nd to 6th harmonics of a console at an amount, worked out once.
fn fingerprint(ui: &Ui, model: ConsoleModel, amount: f32) -> [f32; 5] {
    let key = egui::Id::unique(("console-fingerprint", model, (amount / 5.0).round() as i32));
    if let Some(h) = ui.data(|d| d.get_temp::<[f32; 5]>(key)) {
        return h;
    }
    let h = audiostrada_dsp::console::harmonics(model.index(), amount, -12.0);
    ui.data_mut(|d| d.insert_temp(key, h));
    h
}

/// Label of the mixer's console button.
pub fn button_label(doc: &Document) -> String {
    let console = doc.project().console;
    trf!("Console: {name}", name = name(console.model))
}

/// The console of the project: a card per console with its harmonics, its amount, its hiss,
/// a comparison without it and gain staging.
pub fn console_menu(ui: &mut Ui, doc: &mut Document) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| menu_contents(ui, doc));
}

/// The console's on/off switch: every color off at once, at the same level (the color keeps
/// the working level), the settings kept.
pub fn power_switch(ui: &mut Ui, doc: &mut Document) -> egui::Response {
    let p = theme::current(ui);
    let off = doc.project().console_off;
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    let fill = if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(rect, 4.0, fill);
    theme::paint_power(ui.painter(), rect.shrink(3.0), theme::power_ink(!off, &p));
    let response = response.on_hover_text(if off {
        tr("Console colors off: click to hear them again")
    } else {
        tr("Console colors on: click to switch them all off, at the same level")
    });
    if response.clicked() {
        let label = if off {
            tr("Console colors on")
        } else {
            tr("Console colors off")
        };
        doc.history_mut().edit(label, |p| p.console_off = !off);
    }
    response
}

fn menu_contents(ui: &mut Ui, doc: &mut Document) {
    let p = theme::current(ui);
    let console = doc.project().console;
    ui.set_width(5.0 * 112.0);
    ui.horizontal(|ui| {
        power_switch(ui, doc);
        ui.label(RichText::new(tr("Console color")).size(15.0).strong());
    });
    ui.add_space(2.0);
    ui.label(
        RichText::new(tr(
            "The character of an analog console on every track (its preamp), the buses and the \
             master (their summing). A track can have its own.",
        ))
        .size(10.5)
        .color(p.weak),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for model in SHOWN {
            if card(ui, model, console.model == model, console.amount).clicked() {
                doc.history_mut()
                    .edit(tr("Console color"), |p| p.console.model = model);
            }
        }
    });
    ui.add_space(8.0);
    // Nothing to set on the neutral console.
    ui.add_enabled_ui(console.model != ConsoleModel::Clean, |ui| {
        console_options(ui, doc, console)
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Blind test")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tr("Start…")).clicked() {
                crate::blind_test::open(ui.ctx());
            }
        });
    });
    ui.label(
        RichText::new(tr(
            "Do you hear the consoles? The mix with and without them, one of the two at random: \
             say which.",
        ))
        .size(10.5)
        .color(p.weak),
    );
    ui.separator();
    let done_key = egui::Id::unique("console-gain-staging");
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr("Gain staging")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::primary_button(ui, tr("Apply")).clicked() {
                let count = stage_gains(doc, WORKING_DB);
                ui.data_mut(|d| d.insert_temp(done_key, count));
            }
            if let Some(count) = ui.data(|d| d.get_temp::<usize>(done_key)) {
                ui.label(
                    RichText::new(trf!("{count} track(s) set", count = count))
                        .size(10.5)
                        .color(p.weak),
                );
            }
        });
    });
    ui.label(
        RichText::new(tr(
            "A console's character depends on the level it gets. Sets each audio track's input \
             gain so it plays around -18 dBFS (the selected tracks, or all of them).",
        ))
        .size(10.5)
        .color(p.weak),
    );
}

/// The amount and the hiss of the project's console.
fn console_options(ui: &mut Ui, doc: &mut Document, console: Console) {
    let mut amount = console.amount;
    ui.horizontal(|ui| {
        ui.label(tr("Amount"));
        let response = ui
            .add(egui::Slider::new(&mut amount, 0.0..=100.0).suffix(" %"))
            .on_hover_text(tr(
                "50 %: the console as it is. 0 %: softer, 100 %: pushed harder, more harmonics; \
                 always at the same level",
            ));
        if response.changed() {
            doc.history_mut()
                .edit_merged(tr("Console amount"), "console-amount", |p| {
                    p.console.amount = amount
                });
        }
    });
    let mut noise = doc.project().console_noise;
    if ui
        .checkbox(&mut noise, tr("Hiss and crosstalk"))
        .on_hover_text(tr(
            "Each channel's hiss, and the sides of the buses leaking a little into each other",
        ))
        .changed()
    {
        doc.history_mut()
            .edit(tr("Console hiss"), |p| p.console_noise = noise);
    }
}

/// A console's card: its color, name and character, and its harmonics as bars.
fn card(ui: &mut Ui, model: ConsoleModel, selected: bool, amount: f32) -> egui::Response {
    let p = theme::current(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(106.0, 176.0), Sense::click());
    let painter = ui.painter_at(rect.expand(2.0));
    let tint = color(model);
    let fill = if selected {
        tint.gamma_multiply(0.18)
    } else if response.hovered() {
        p.hover
    } else {
        p.surface
    };
    painter.rect_filled(rect, 8.0, fill);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 4.0)),
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            ..Default::default()
        },
        tint,
    );
    if selected {
        painter.rect_stroke(rect, 8.0, Stroke::new(2.0, tint), egui::StrokeKind::Inside);
    }
    // The name on two lines when it has two words.
    let ink = if selected {
        p.text
    } else {
        p.text.gamma_multiply(0.85)
    };
    let title = painter.layout(
        name(model).replacen(' ', "\n", 1),
        FontId::proportional(14.0),
        ink,
        rect.width() - 18.0,
    );
    let below = rect.top() + 12.0 + title.size().y + 4.0;
    painter.galley(pos2(rect.left() + 10.0, rect.top() + 12.0), title, ink);
    let galley = painter.layout(
        hint(model).to_owned(),
        FontId::proportional(9.5),
        p.weak,
        rect.width() - 18.0,
    );
    painter.galley(pos2(rect.left() + 10.0, below), galley, p.weak);
    // Harmonics 2 to 6, from -120 dB (nothing) to -50 dB (a full bar): a console at -12 dBFS
    // measures between -60 and -110 dB.
    let base = rect.bottom() - 40.0;
    if let Some(source) = inspiration(model) {
        let galley = painter.layout(
            source.to_owned(),
            FontId::proportional(8.5),
            p.weak.gamma_multiply(0.8),
            rect.width() - 18.0,
        );
        painter.galley(
            pos2(rect.left() + 10.0, base + 15.0),
            galley,
            p.weak.gamma_multiply(0.8),
        );
    }
    let h = fingerprint(ui, model, amount);
    for (k, db) in h.iter().enumerate() {
        let x = rect.left() + 12.0 + k as f32 * 18.0;
        let full = 34.0;
        let height = if model == ConsoleModel::Clean {
            1.0
        } else {
            (((db + 120.0) / 70.0).clamp(0.0, 1.0) * full).max(1.0)
        };
        painter.rect_filled(
            Rect::from_min_max(pos2(x, base - full), pos2(x + 10.0, base)),
            2.0,
            p.base,
        );
        painter.rect_filled(
            Rect::from_min_max(pos2(x, base - height), pos2(x + 10.0, base)),
            2.0,
            tint.gamma_multiply(if k % 2 == 0 { 1.0 } else { 0.7 }),
        );
        painter.text(
            pos2(x + 5.0, base + 2.0),
            Align2::CENTER_TOP,
            format!("{}", k + 2),
            FontId::proportional(8.0),
            p.weak,
        );
    }
    match inspiration(model) {
        Some(source) => response.on_hover_text(trf!(
            "{inspiration}. Bars: its harmonics, 2nd to 6th (even ones brighter), on a tone at \
             -12 dBFS",
            inspiration = source
        )),
        None => response,
    }
}

/// Which console a track plays through: the project's, or its own.
pub fn track_menu(ui: &mut Ui, doc: &mut Document, track: TrackId) {
    let project = doc.project();
    let shared = project.console;
    let own = project.track(track).and_then(|t| t.console);
    ui.label(RichText::new(tr("Console color")).strong());
    let follow = trf!("As the project ({name})", name = name(shared.model));
    if ui.radio(own.is_none(), follow).clicked() {
        doc.history_mut().edit(tr("Track console"), |p| {
            if let Some(t) = p.track_mut(track) {
                t.console = None;
            }
        });
    }
    for model in SHOWN {
        let on = own.is_some_and(|c| c.model == model);
        let text = RichText::new(name(model)).color(if on {
            color(model)
        } else {
            theme::current(ui).text
        });
        if ui
            .radio(on, text)
            .on_hover_text(description(model))
            .clicked()
        {
            let amount = own.map_or(shared.amount, |c| c.amount);
            doc.history_mut().edit(tr("Track console"), |p| {
                if let Some(t) = p.track_mut(track) {
                    t.console = Some(Console { model, amount });
                }
            });
        }
    }
    if let Some(mut console) = own.filter(|c| c.model != ConsoleModel::Clean) {
        ui.horizontal(|ui| {
            ui.label(tr("Amount"));
            if ui
                .add(egui::Slider::new(&mut console.amount, 0.0..=100.0).suffix(" %"))
                .changed()
            {
                doc.history_mut().edit_merged(
                    tr("Track console"),
                    &format!("console-amount:{}", track.0),
                    |p| {
                        if let Some(t) = p.track_mut(track) {
                            t.console = Some(console);
                        }
                    },
                );
            }
        });
    }
}

/// The console a channel plays through.
pub fn of_channel(doc: &Document, channel: audiostrada_core::routing::Channel) -> Console {
    match channel {
        audiostrada_core::routing::Channel::Track(t) => {
            audiostrada_core::console::of_track(doc.project(), t)
        }
        audiostrada_core::routing::Channel::Master => doc.project().console,
        audiostrada_core::routing::Channel::Clip(_) => Console::default(),
    }
}

/// The DSP's index of the character a channel's strip takes.
pub fn character(doc: &Document, channel: audiostrada_core::routing::Channel) -> usize {
    let console = of_channel(doc, channel);
    if console.colors() {
        console.model.index()
    } else {
        0
    }
}

/// Sets the input gain of the selected audio tracks (all of them when none is) so they play
/// around `target` dBFS; returns how many were set.
pub fn stage_gains(doc: &mut Document, target: f32) -> usize {
    let project = doc.project().clone();
    let selected = doc.selected_tracks().to_vec();
    let tracks: Vec<TrackId> = project
        .tracks
        .iter()
        .filter(|t| selected.is_empty() || selected.contains(&t.id))
        .map(|t| t.id)
        .collect();
    let trims: Vec<(TrackId, f32)> = tracks
        .into_iter()
        .filter_map(|t| {
            let level = working_level(doc, &project, t)?;
            let trim = ((target - level) * 10.0).round() / 10.0;
            Some((t, trim.clamp(-24.0, 24.0)))
        })
        .collect();
    if !trims.is_empty() {
        doc.history_mut().edit(tr("Gain staging"), |p| {
            for &(t, trim) in &trims {
                if let Some(track) = p.track_mut(t) {
                    track.trim_db = trim;
                }
            }
        });
    }
    trims.len()
}

/// Average level of a track's audio where it plays (silences left out), with its clips'
/// gains, before its input gain: dBFS.
pub(crate) fn working_level(
    doc: &Document,
    project: &audiostrada_core::Project,
    id: TrackId,
) -> Option<f32> {
    const WINDOW: usize = 4_800;
    let track = project.track(id)?;
    let (mut power, mut windows) = (0.0f64, 0usize);
    for clip in track.clips.iter().filter(|c| !c.muted && c.notes.is_none()) {
        let Some(path) = audiostrada_engine::project::clip_media(project, doc.paths(), clip) else {
            continue;
        };
        let Some(audio) = doc.media().audio(&path) else {
            continue;
        };
        let ratio = f64::from(audio.sample_rate()) / f64::from(project.sample_rate.max(1));
        let start = ((clip.offset as f64 * ratio) as usize).min(audio.frames());
        let end = (start + (clip.length as f64 * ratio) as usize).min(audio.frames());
        let gain = f64::from(audiostrada_dsp::db_to_gain(clip.gain_db));
        let channels = audio.channels().max(1);
        let mut at = start;
        while at + WINDOW <= end {
            // Every 4th sample: plenty for a level.
            let mut sum = 0.0f64;
            for c in 0..channels {
                let samples = &audio.channel(c)[at..at + WINDOW];
                sum += samples
                    .iter()
                    .step_by(4)
                    .map(|&s| f64::from(s) * f64::from(s))
                    .sum::<f64>();
            }
            let mean = sum / (channels * WINDOW / 4) as f64 * gain * gain;
            // Below -60 dBFS: silence between the notes, left out.
            if mean > 1e-6 {
                power += mean;
                windows += 1;
            }
            at += WINDOW;
        }
    }
    (windows > 0).then(|| (10.0 * (power / windows as f64).log10()) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;
    use audiostrada_media::{AudioData, write_wav};
    use std::time::Duration;

    #[test]
    fn gain_staging_brings_tracks_to_the_working_level() {
        let dir = audiostrada_core::scratch::dir("staging");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let take = dir.join("quiet.wav");
        // A tone at -29 dBFS RMS, then as long a silence, left out of the level.
        let mut tone: Vec<f32> = (0..48_000)
            .map(|i| 0.05 * (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        tone.extend(std::iter::repeat_n(0.0, 48_000));
        write_wav(&take, &AudioData::new(vec![tone], 48_000)).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.import(std::slice::from_ref(&take));
        assert!(doc.wait_loaded(Duration::from_secs(10)));
        assert_eq!(stage_gains(&mut doc, WORKING_DB), 1);
        let trim = doc.project().tracks[0].trim_db;
        assert!((trim - 11.0).abs() < 0.3, "{trim}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_console_has_a_color_name_and_hint() {
        for model in ConsoleModel::ALL {
            assert!(!name(model).is_empty() && hint(model).len() > 10);
            assert_ne!(color(model), Color32::TRANSPARENT);
        }
    }
}
