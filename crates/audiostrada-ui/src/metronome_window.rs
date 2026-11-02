//! Metronome window: sound, clicks per beat, accents, count-in, with a picture of the bar
//! that shows what will be heard.

use audiostrada_core::{ClickSound, MetronomeSettings};
use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::document::Document;
use crate::i18n::tr;
use crate::theme;
use crate::trf;

fn open_id() -> egui::Id {
    egui::Id::unique("metronome-window")
}

pub fn open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(open_id(), true));
}

fn sound_name(sound: ClickSound) -> &'static str {
    match sound {
        ClickSound::Beep => tr("Beep"),
        ClickSound::Woodblock => tr("Wood block"),
        ClickSound::Clave => tr("Clave"),
        ClickSound::Cowbell => tr("Cowbell"),
        ClickSound::HiHat => tr("Hi-hat"),
    }
}

pub fn show(ctx: &egui::Context, doc: &mut Document) {
    let mut open = ctx.data(|d| d.get_temp::<bool>(open_id())).unwrap_or(false);
    if !open {
        return;
    }
    let mut click = doc.project().metronome.clone();
    let before = click.clone();
    let project = doc.project();
    let rate = project.sample_rate;
    let beat = project.tempo.sample_to_beat(doc.player().position(), rate);
    let signature = project.tempo.signature_at_beat(beat);
    egui::Window::new(tr("Metronome"))
        .id(egui::Id::unique("metronome-window-frame"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .pivot(Align2::CENTER_TOP)
        .default_pos(ctx.content_rect().center_top() + vec2(0.0, 90.0))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Click")).strong());
                ui.selectable_value(&mut click.enabled, true, tr("On"));
                ui.selectable_value(&mut click.enabled, false, tr("Off"));
                ui.add_space(16.0);
                ui.label(tr("Volume"));
                ui.add(egui::Slider::new(&mut click.gain_db, -40.0..=6.0).suffix(" dB"));
            });
            bar_picture(ui, &click, signature.numerator, signature.denominator);
            section(ui, tr("Sound"));
            ui.horizontal_wrapped(|ui| {
                for sound in ClickSound::ALL {
                    ui.selectable_value(&mut click.sound, sound, sound_name(sound));
                }
            });
            section(ui, tr("Clicks per beat"));
            ui.horizontal(|ui| {
                for (n, text, hint) in [
                    (1, "1", tr("One click per beat")),
                    (2, "2", tr("Eighths: a softer click between the beats")),
                    (3, "3", tr("Triplets")),
                    (4, "4", tr("Sixteenths")),
                ] {
                    ui.selectable_value(&mut click.subdivision, n, text)
                        .on_hover_text(hint);
                }
            });
            section(ui, tr("Accents"));
            ui.checkbox(&mut click.accent, tr("Accent the first beat of the bar"));
            let compound = signature.denominator == 8
                && signature.numerator.is_multiple_of(3)
                && signature.numerator > 3;
            ui.add_enabled_ui(compound, |ui| {
                ui.checkbox(
                    &mut click.compound,
                    tr("Beat in dotted quarters (6/8, 9/8, 12/8: 2, 3 or 4 beats per bar)"),
                );
            });
            ui.horizontal(|ui| {
                ui.label(tr("Groups"));
                ui.add(
                    egui::TextEdit::singleline(&mut click.groups)
                        .hint_text(tr("none"))
                        .desired_width(80.0),
                );
                for &pattern in suggestions(signature.numerator) {
                    if ui.small_button(pattern).clicked() {
                        click.groups = pattern.to_owned();
                    }
                }
                if !click.groups.is_empty() && ui.small_button(tr("Clear")).clicked() {
                    click.groups.clear();
                }
            });
            if !click.groups.is_empty() && click.group_starts(signature.numerator) == 0 {
                ui.colored_label(
                    theme::RECORD,
                    trf!(
                        "The groups must add up to {beats} beats, the bar of {beats}/{unit}.",
                        beats = signature.numerator,
                        unit = signature.denominator
                    ),
                );
            } else {
                ui.weak(tr(
                    "Groups accent the first beat of each group, for odd meters (7/8 as 2+2+3).",
                ));
            }
            section(ui, tr("When"));
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut click.recording_only,
                    false,
                    tr("Playing and recording"),
                );
                ui.radio_value(&mut click.recording_only, true, tr("Only while recording"));
            });
            ui.horizontal(|ui| {
                ui.label(tr("Count-in before recording"));
                for (bars, text) in [
                    (0, tr("Off")),
                    (1, tr("1 bar")),
                    (2, tr("2 bars")),
                    (4, tr("4 bars")),
                ] {
                    ui.selectable_value(&mut click.count_in, bars, text);
                }
            });
            ui.add_space(4.0);
            ui.weak(trf!(
                "Time signature at the playhead: {beats}/{unit} (change it in the transport bar).",
                beats = signature.numerator,
                unit = signature.denominator
            ));
        });
    if click != before {
        doc.set_metronome(|m| *m = click);
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

fn section(ui: &mut Ui, title: &str) {
    theme::section(ui, title);
}

/// Usual groupings of odd meters.
fn suggestions(numerator: u32) -> &'static [&'static str] {
    match numerator {
        5 => &["3+2", "2+3"],
        7 => &["2+2+3", "3+2+2", "2+3+2"],
        8 => &["3+3+2"],
        9 => &["2+2+2+3"],
        10 => &["3+3+2+2"],
        11 => &["3+3+3+2", "2+2+3+2+2"],
        13 => &["3+3+3+2+2"],
        _ => &[],
    }
}

/// One bar as dots: big for the first beat, medium for group starts and beats, small for the
/// softer clicks in between, none where nothing sounds.
fn bar_picture(ui: &mut Ui, click: &MetronomeSettings, numerator: u32, denominator: u32) {
    let p = theme::current(ui);
    let numerator = numerator.max(1);
    let compound =
        click.compound && denominator == 8 && numerator.is_multiple_of(3) && numerator > 3;
    let per_unit = if compound {
        1
    } else {
        click.subdivision.clamp(1, 4)
    };
    let groups = click.group_starts(numerator);
    let ticks = numerator * per_unit;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 54.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, p.base);
    painter.text(
        rect.left_top() + vec2(8.0, 4.0),
        Align2::LEFT_TOP,
        format!("{numerator}/{denominator}"),
        FontId::proportional(10.0),
        p.weak,
    );
    let step = (rect.width() - 40.0) / ticks as f32;
    for tick in 0..ticks {
        let unit = tick / per_unit;
        let on_unit = tick % per_unit == 0;
        let (radius, color) = if compound {
            match (unit, unit % 3 == 0) {
                (0, _) if click.accent => (9.0, p.accent),
                (_, true) | (0, _) => (6.5, p.text),
                _ if click.subdivision > 1 => (3.5, p.weak),
                _ => (2.0, p.border),
            }
        } else if !on_unit {
            (3.5, p.weak)
        } else if unit == 0 && click.accent {
            (9.0, p.accent)
        } else if unit > 0 && unit < 32 && groups & (1 << unit) != 0 {
            (7.5, Color32::from_rgb(240, 180, 60))
        } else {
            (6.0, p.text)
        };
        let center = pos2(
            rect.left() + 28.0 + step * (tick as f32 + 0.5),
            rect.center().y + 4.0,
        );
        painter.circle_filled(center, radius, color);
        if on_unit {
            painter.line_segment(
                [center + vec2(0.0, 13.0), center + vec2(0.0, 16.0)],
                Stroke::new(1.0, p.border),
            );
        }
    }
}
