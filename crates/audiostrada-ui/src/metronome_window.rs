//! Metronome window: sound, clicks per beat, accents, count-in, with a picture of the bar
//! that shows what will be heard.

use audiostrada_core::{ClickSound, MetronomeSettings};
use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::dialog;
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
    let (beats, unit) = (signature.numerator, signature.denominator);
    let mut close = false;
    dialog::Dialog::new("metronome", tr("Metronome"))
        .detail(trf!(
            "Clicks on the beat of {beats}/{unit}, the time signature at the playhead; never \
             exported",
            beats = beats,
            unit = unit
        ))
        .width(640.0)
        .show(ctx, &mut open, |ui, part| match part {
            dialog::Part::Body => body(ui, &mut click, beats, unit),
            dialog::Part::Actions => {
                let weak = theme::current(ui).weak;
                ui.label(RichText::new(summary(&click)).size(11.5).color(weak));
                dialog::buttons(ui, |ui| {
                    close = dialog::secondary(ui, tr("Close")).clicked();
                    if dialog::secondary(ui, tr("Default settings"))
                        .on_hover_text(tr(
                            "Back to the settings of a new project, the click kept on or off",
                        ))
                        .clicked()
                    {
                        click = MetronomeSettings {
                            enabled: click.enabled,
                            ..MetronomeSettings::default()
                        };
                    }
                });
            }
        });
    if click != before {
        doc.set_metronome(|m| *m = click);
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), open && !close));
}

/// What will be heard, in a line: on or off, sound, clicks per beat, count-in.
fn summary(click: &MetronomeSettings) -> String {
    if !click.enabled {
        return tr("The click is off").to_owned();
    }
    let mut parts = vec![sound_name(click.sound).to_owned()];
    if click.subdivision > 1 {
        parts.push(trf!("{count} per beat", count = click.subdivision));
    }
    if click.recording_only {
        parts.push(tr("while recording").to_owned());
    }
    if click.count_in > 0 {
        parts.push(trf!("count-in {bars}", bars = bars(click.count_in)));
    }
    parts.join(" · ")
}

fn bars(count: u32) -> &'static str {
    match count {
        1 => tr("1 bar"),
        2 => tr("2 bars"),
        _ => tr("4 bars"),
    }
}

fn body(ui: &mut Ui, click: &mut MetronomeSettings, beats: u32, unit: u32) {
    bar_picture(ui, click, beats, unit);
    ui.add_space(4.0);
    dialog::section(ui, tr("Sound"), |ui| {
        dialog::field(ui, tr("Click"), |ui| {
            dialog::segmented(
                ui,
                &mut click.enabled,
                &[(true, tr("On"), true), (false, tr("Off"), true)],
            );
        });
        dialog::field(ui, tr("Volume"), |ui| {
            ui.add(egui::Slider::new(&mut click.gain_db, -40.0..=6.0).suffix(" dB"));
        });
        dialog::field(ui, tr("Sound"), |ui| {
            egui::ComboBox::from_id_salt("metronome-sound")
                .width(180.0)
                .selected_text(sound_name(click.sound))
                .show_ui(ui, |ui| {
                    for sound in ClickSound::ALL {
                        ui.selectable_value(&mut click.sound, sound, sound_name(sound));
                    }
                });
        });
    });
    dialog::section(ui, tr("Rhythm"), |ui| {
        let compound = unit == 8 && beats.is_multiple_of(3) && beats > 3;
        dialog::field(ui, tr("Clicks per beat"), |ui| {
            dialog::segmented(
                ui,
                &mut click.subdivision,
                &[
                    (1, "1", true),
                    (2, "2", true),
                    (3, "3", true),
                    (4, "4", true),
                ],
            );
        });
        dialog::hint(
            ui,
            match click.subdivision {
                2 => tr("Eighths: a softer click between the beats"),
                3 => tr("Triplets"),
                4 => tr("Sixteenths"),
                _ => tr("One click per beat"),
            },
        );
        if compound {
            dialog::field(ui, tr("Beat"), |ui| {
                dialog::segmented(
                    ui,
                    &mut click.compound,
                    &[
                        (true, tr("Dotted quarters"), true),
                        (false, tr("Eighths"), true),
                    ],
                );
            });
            dialog::hint(
                ui,
                &trf!(
                    "In {beats}/{unit}, dotted quarters give {count} beats per bar.",
                    beats = beats,
                    unit = unit,
                    count = beats / 3
                ),
            );
        }
    });
    dialog::section(ui, tr("Accents"), |ui| {
        dialog::field(ui, tr("First beat"), |ui| {
            ui.checkbox(&mut click.accent, tr("Accent the first beat of the bar"));
        });
        dialog::field(ui, tr("Groups"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut click.groups)
                    .hint_text(tr("none"))
                    .desired_width(80.0),
            );
            for &pattern in suggestions(beats) {
                if ui.small_button(pattern).clicked() {
                    click.groups = pattern.to_owned();
                }
            }
            if !click.groups.is_empty() && ui.small_button(tr("Clear")).clicked() {
                click.groups.clear();
            }
        });
        if !click.groups.is_empty() && click.group_starts(beats) == 0 {
            ui.colored_label(
                theme::RECORD,
                trf!(
                    "The groups must add up to {beats} beats, the bar of {beats}/{unit}.",
                    beats = beats,
                    unit = unit
                ),
            );
        } else {
            dialog::hint(
                ui,
                tr("Groups accent the first beat of each group, for odd meters (7/8 as 2+2+3)."),
            );
        }
    });
    dialog::section(ui, tr("Recording"), |ui| {
        dialog::field(ui, tr("Clicks"), |ui| {
            dialog::segmented(
                ui,
                &mut click.recording_only,
                &[
                    (false, tr("Playing and recording"), true),
                    (true, tr("Only while recording"), true),
                ],
            );
        });
        let counts = [
            (0, tr("Off"), true),
            (1, tr("1 bar"), true),
            (2, tr("2 bars"), true),
            (4, tr("4 bars"), true),
        ];
        dialog::field(ui, tr("Count-in"), |ui| {
            dialog::segmented(ui, &mut click.count_in, &counts);
        });
        dialog::field(ui, tr("Pre-roll"), |ui| {
            // Its own ids: the same choices as the count-in.
            ui.push_id("pre-roll", |ui| {
                dialog::segmented(ui, &mut click.pre_roll, &counts);
            });
        });
        dialog::hint(
            ui,
            tr(
                "The count-in clicks bars before recording starts; the pre-roll plays bars of \
                 the song before the count-in, counted from the punch-in when there is one.",
            ),
        );
    });
    dialog::hint(ui, tr("Change the time signature in the transport bar."));
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
