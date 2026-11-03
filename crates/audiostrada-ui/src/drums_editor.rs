//! The drum machine's editor: small pads lit while their voice sounds, played with the mouse
//! and selected to show their settings; the kit's accent, drive and volume; and ready-made
//! rhythms written into a MIDI clip on the track, to edit in the piano roll.

use audiostrada_core::Note;
use audiostrada_core::routing::Channel;
use audiostrada_dsp::drums::VOICES;
use eframe::egui::{self, Align2, FontId, RichText, Sense, Ui, vec2};

use crate::effect_editors::{Params, named_knob, param_knob};
use crate::i18n::tr;
use crate::theme;

const COLUMNS: usize = 6;
const PAD_HEIGHT: f32 = 44.0;

/// What the drum machine's editor remembers between frames.
#[derive(Debug)]
pub struct DrumsState {
    /// The pad held with the mouse, and the track it plays.
    held: Option<(audiostrada_core::TrackId, u8)>,
    /// The voice whose settings are shown.
    selected: usize,
    /// The rhythm chosen, and how many bars to write.
    rhythm: usize,
    bars: u32,
}

impl Default for DrumsState {
    fn default() -> Self {
        Self {
            held: None,
            selected: 0,
            rhythm: 0,
            bars: 4,
        }
    }
}

impl DrumsState {
    /// Lets go of the pad held when the window closes.
    pub fn release(&mut self, doc: &crate::document::Document) {
        if let Some((track, key)) = self.held.take() {
            doc.player().play_key(track, [0x89, key, 0]);
        }
    }
}

/// A ready-made rhythm: sixteenth-note steps per voice, a bar per 16 steps. `X` is accented,
/// `x` normal, `o` a ghost note, `r` a roll (two thirty-second notes).
pub(crate) struct Rhythm {
    pub name: &'static str,
    pub bpm: u32,
    /// 50 plays straight, 66 swings the off sixteenths to triplets.
    pub swing: f32,
    pub lines: &'static [(&'static str, &'static str)],
}

pub(crate) const RHYTHMS: &[Rhythm] = &[
    Rhythm {
        name: "Hip hop: boom bap",
        bpm: 90,
        swing: 58.0,
        lines: &[
            ("kick", "X......x..x.....X......x..x..x.."),
            ("snare", "....X.......X.......X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.o."),
        ],
    },
    Rhythm {
        name: "Hip hop: lo-fi",
        bpm: 80,
        swing: 62.0,
        lines: &[
            ("kick", "X......x.x......X.......x.x....."),
            ("snare", "....X.......X..o....X.......X..."),
            ("rim", "..............o...............o."),
            ("closed", "x.o.x.o.x.o.x.o.x.o.x.o.x.o.x.xo"),
        ],
    },
    Rhythm {
        name: "Trap",
        bpm: 140,
        swing: 50.0,
        lines: &[
            ("kick", "X......x...x....X.x.......x....."),
            ("clap", "........X...............X......."),
            ("snare", "........X...............X......."),
            ("closed", "x.x.x.x.rrx.x.x.x.x.x.x.x.rrrrx."),
            ("open", "..............x................."),
        ],
    },
    Rhythm {
        name: "Drill",
        bpm: 142,
        swing: 50.0,
        lines: &[
            ("kick", "X.........x.....X..x......x....."),
            ("snare", "......X........x......X........."),
            ("closed", "x..x..x.x..x..x.x..x..rrx..x..x."),
        ],
    },
    Rhythm {
        name: "House",
        bpm: 124,
        swing: 52.0,
        lines: &[
            ("kick", "X...x...X...x..."),
            ("clap", "....X.......X..."),
            ("closed", "o...o...o...o..."),
            ("open", "..x...x...x...x."),
        ],
    },
    Rhythm {
        name: "Techno",
        bpm: 130,
        swing: 50.0,
        lines: &[
            ("kick", "X...x...X...x..."),
            ("closed", "..x...x...x...x."),
            ("rim", "...x..x....x..x."),
            ("clap", "....x.......x..."),
        ],
    },
    Rhythm {
        name: "Electro: 808 funk",
        bpm: 118,
        swing: 54.0,
        lines: &[
            ("kick", "X.....x...x.....X.....x..x..x..."),
            ("snare", "....X.......X.......X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x."),
            ("cowbell", "...x......x........x......x..x.."),
            ("clave", "x..x..x...x.x...x..x..x...x.x..."),
        ],
    },
    Rhythm {
        name: "Reggaeton: dembow",
        bpm: 95,
        swing: 50.0,
        lines: &[
            ("kick", "X...x...X...x..."),
            ("snare", "...x..x....x..x."),
            ("closed", "x.x.x.x.x.x.x.x."),
        ],
    },
    Rhythm {
        name: "Funk",
        bpm: 100,
        swing: 54.0,
        lines: &[
            ("kick", "X..x..x...x..x..X..x......x..x.."),
            ("snare", "....X..o.o..X..o....X..o.o..X.o."),
            ("closed", "xoxoxoxoxoxoxoxoxoxoxoxoxoxoxoxo"),
            ("open", "..............x...............x."),
        ],
    },
    Rhythm {
        name: "Rock",
        bpm: 110,
        swing: 50.0,
        lines: &[
            ("kick", "X.......x.x.....X.......x.x....."),
            ("snare", "....X.......X.......X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x."),
            ("cymbal", "X..............................."),
        ],
    },
    Rhythm {
        name: "Drum and bass",
        bpm: 172,
        swing: 50.0,
        lines: &[
            ("kick", "X.........x.....X.........x....."),
            ("snare", "....X.......X.......X..o....X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x."),
        ],
    },
];

/// The notes of `bars` bars of a rhythm, from a bar start, in samples of `samples_per_beat`.
pub(crate) fn rhythm_notes(rhythm: &Rhythm, bars: u32, samples_per_beat: f64) -> Vec<Note> {
    let step = samples_per_beat / 4.0;
    let delay = step * f64::from((rhythm.swing / 100.0 * 2.0 - 1.0).clamp(0.0, 0.5));
    let length = (step / 2.0) as u64;
    let mut notes = Vec::new();
    for (voice, steps) in rhythm.lines {
        let Some(key) = VOICES.iter().find(|v| v.0 == *voice).map(|v| v.2) else {
            continue;
        };
        let pattern: Vec<char> = steps.chars().collect();
        for index in 0..(bars as usize * 16) {
            let at = index as f64 * step + if index % 2 == 1 { delay } else { 0.0 };
            let mut hit = |start: f64, velocity: u8| {
                notes.push(Note {
                    start: start as u64,
                    length,
                    key,
                    velocity,
                    channel: 9,
                    ..Default::default()
                })
            };
            match pattern[index % pattern.len()] {
                'X' => hit(at, 120),
                'x' => hit(at, 95),
                'o' => hit(at, 55),
                'r' => {
                    hit(at, 85);
                    hit(at + step / 2.0, 70);
                }
                _ => {}
            }
        }
    }
    notes.sort_by_key(|n| (n.start, n.key));
    notes
}

/// Ready-made kits.
pub(crate) fn factory() -> Vec<crate::presets::Preset> {
    use crate::presets::preset;
    vec![
        preset(tr("Kit: classic"), &[]),
        preset(
            tr("Kit: boom (long kick)"),
            &[
                ("kick_decay", 320.0),
                ("kick_tune", -3.0),
                ("kick_level", 90.0),
                ("kick_tone", 20.0),
                ("drive", 25.0),
            ],
        ),
        preset(
            tr("Kit: trap"),
            &[
                ("kick_decay", 260.0),
                ("kick_tune", -2.0),
                ("kick_level", 90.0),
                ("snare_tone", 80.0),
                ("snare_tune", 3.0),
                ("clap_decay", 140.0),
                ("closed_tone", 85.0),
                ("closed_decay", 60.0),
                ("open_decay", 70.0),
                ("drive", 15.0),
            ],
        ),
        preset(
            tr("Kit: electro"),
            &[
                ("kick_decay", 70.0),
                ("kick_tone", 80.0),
                ("kick_tune", 2.0),
                ("snare_tone", 70.0),
                ("snare_decay", 70.0),
                ("closed_decay", 50.0),
                ("open_decay", 60.0),
                ("cowbell_level", 90.0),
                ("clave_level", 90.0),
            ],
        ),
        preset(
            tr("Kit: tight and dry"),
            &[
                ("kick_decay", 45.0),
                ("snare_decay", 55.0),
                ("clap_decay", 50.0),
                ("open_decay", 45.0),
                ("cymbal_decay", 40.0),
                ("low_tom_decay", 55.0),
                ("mid_tom_decay", 55.0),
                ("high_tom_decay", 55.0),
            ],
        ),
        preset(
            tr("Kit: dusty lo-fi"),
            &[
                ("kick_tone", 10.0),
                ("snare_tone", 25.0),
                ("closed_tone", 15.0),
                ("open_tone", 15.0),
                ("cymbal_tone", 10.0),
                ("drive", 55.0),
                ("volume", -9.0),
            ],
        ),
        preset(
            tr("Kit: tuned up"),
            &[
                ("kick_tune", 5.0),
                ("snare_tune", 5.0),
                ("low_tom_tune", 5.0),
                ("mid_tom_tune", 5.0),
                ("high_tom_tune", 5.0),
                ("closed_tune", 3.0),
                ("open_tune", 3.0),
            ],
        ),
    ]
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut DrumsState) {
    let p = theme::current(ui);
    let gap = ui.spacing().item_spacing.x;
    let width = ((ui.available_width() - (COLUMNS - 1) as f32 * gap) / COLUMNS as f32).max(80.0);
    let mut pressed = None;
    for (row, voices) in VOICES.chunks(COLUMNS).enumerate() {
        ui.horizontal(|ui| {
            for (column, (_, name, key)) in voices.iter().enumerate() {
                let index = row * COLUMNS + column;
                let glow = meters.get(index).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, PAD_HEIGHT), Sense::click_and_drag());
                let fill = theme::mix(p.base, p.accent, 0.12 + 0.88 * glow.sqrt());
                ui.painter().rect_filled(rect, 6.0, fill);
                if index == state.selected {
                    ui.painter().rect_stroke(
                        rect,
                        6.0,
                        egui::Stroke::new(2.0, p.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.painter().text(
                    rect.left_top() + vec2(8.0, 7.0),
                    Align2::LEFT_TOP,
                    tr(name),
                    theme::strong(12.0),
                    if glow > 0.3 {
                        theme::text_on(p.accent)
                    } else {
                        p.text
                    },
                );
                ui.painter().text(
                    rect.right_bottom() + vec2(-6.0, -4.0),
                    Align2::RIGHT_BOTTOM,
                    key.to_string(),
                    FontId::monospace(9.0),
                    p.weak,
                );
                if response.is_pointer_button_down_on() {
                    // Lower on the pad plays louder, as a harder hit.
                    let depth = response
                        .interact_pointer_pos()
                        .map_or(0.7, |pos| (pos.y - rect.top()) / rect.height());
                    pressed = Some((*key, (40.0 + 87.0 * depth.clamp(0.0, 1.0)) as u8));
                    state.selected = index;
                }
                response.on_hover_text(tr(
                    "Click to play and show its settings; lower on the pad plays louder",
                ));
            }
        });
    }
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        let (id, name, _) = VOICES[state.selected.min(VOICES.len() - 1)];
        panel(ui, tr(name), |ui| {
            ui.horizontal(|ui| {
                for (field, label) in ["level", "tune", "decay", "tone"].into_iter().zip([
                    tr("Level"),
                    tr("Tune"),
                    tr("Decay"),
                    tr("Tone"),
                ]) {
                    named_knob(ui, params, &format!("{id}_{field}"), label, 38.0, p.accent);
                }
            });
        });
        panel(ui, tr("Kit"), |ui| {
            ui.horizontal(|ui| {
                for id in ["accent", "drive", "volume"] {
                    param_knob(ui, params, id, 38.0, p.accent);
                }
            });
        });
        panel(ui, tr("Rhythms"), |ui| rhythms(ui, params, state));
    });

    // The mouse plays one pad at a time.
    let Channel::Track(track) = params.channel else {
        return;
    };
    let doc = &*params.doc;
    if pressed.map(|(key, _)| key) != state.held.map(|(_, key)| key) {
        state.release(doc);
        if let Some((key, velocity)) = pressed {
            doc.player().play_key(track, [0x99, key, velocity]);
            state.held = Some((track, key));
        }
    }
    if meters.iter().any(|m| *m > 0.0) || state.held.is_some() {
        ui.ctx().request_repaint();
    }
}

/// A titled panel.
fn panel(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let p = theme::current(ui);
    egui::Frame::new()
        .fill(theme::mix(p.panel, p.base, 0.4))
        .corner_radius(8.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title.to_uppercase())
                        .size(9.5)
                        .color(p.weak)
                        .strong(),
                );
                ui.add_space(2.0);
                add(ui);
            });
        });
}

/// Picks a rhythm and writes it into a new MIDI clip at the playhead's bar.
fn rhythms(ui: &mut Ui, params: &mut Params, state: &mut DrumsState) {
    let rhythm = &RHYTHMS[state.rhythm.min(RHYTHMS.len() - 1)];
    egui::ComboBox::from_id_salt("drum-rhythm")
        .width(190.0)
        .selected_text(format!("{} · {} BPM", tr(rhythm.name), rhythm.bpm))
        .show_ui(ui, |ui| {
            for (i, r) in RHYTHMS.iter().enumerate() {
                ui.selectable_value(
                    &mut state.rhythm,
                    i,
                    format!("{} · {} BPM", tr(r.name), r.bpm),
                );
            }
        });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("drum-rhythm-bars")
            .width(70.0)
            .selected_text(crate::trf!("{n} bars", n = state.bars))
            .show_ui(ui, |ui| {
                for n in [1, 2, 4, 8] {
                    ui.selectable_value(&mut state.bars, n, crate::trf!("{n} bars", n = n));
                }
            });
        let Channel::Track(track) = params.channel else {
            return;
        };
        if ui
            .button(tr("Add to the piano roll"))
            .on_hover_text(tr(
                "Writes the rhythm into a new MIDI clip at the playhead, then opens it in the \
                 piano roll; the project tempo stays as it is",
            ))
            .clicked()
        {
            add_rhythm(params.doc, track, &RHYTHMS[state.rhythm], state.bars);
        }
    });
}

fn add_rhythm(
    doc: &mut crate::document::Document,
    track: audiostrada_core::TrackId,
    rhythm: &Rhythm,
    bars: u32,
) {
    let project = doc.project();
    let (rate, tempo) = (project.sample_rate, project.tempo.clone());
    let beat = tempo.sample_to_beat(doc.player().position(), rate);
    let bar = tempo.beat_to_bbt(beat);
    let bar_start = tempo.bbt_to_beat(audiostrada_core::tempo::BarBeatTick {
        bar: bar.bar,
        beat: 1,
        tick: 0,
    });
    let start = tempo.beat_to_sample(bar_start, rate);
    let beats = 4.0 * f64::from(bars);
    let end = tempo.beat_to_sample(bar_start + beats, rate);
    let samples_per_beat = (end - start) as f64 / beats;
    let notes = rhythm_notes(rhythm, bars, samples_per_beat);
    let mut created = None;
    doc.history_mut()
        .edit(&crate::trf!("Rhythm {name}", name = tr(rhythm.name)), |p| {
            created = p.add_midi_clip(track, start, end - start, notes);
            if let Some(id) = created {
                audiostrada_core::edit::resolve_overlaps(p, track, &[id]);
            }
        });
    if let Some(id) = created {
        doc.reveal_clip(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{AppDirs, Document};

    #[test]
    fn rhythms_are_well_formed() {
        for rhythm in RHYTHMS {
            for (voice, steps) in rhythm.lines {
                assert!(
                    VOICES.iter().any(|v| v.0 == *voice),
                    "{}: {voice}",
                    rhythm.name
                );
                assert_eq!(steps.len() % 16, 0, "{}: {voice}", rhythm.name);
                assert!(
                    steps.chars().all(|c| ".Xxor".contains(c)),
                    "{}",
                    rhythm.name
                );
            }
            assert!((50.0..=66.0).contains(&rhythm.swing));
        }
    }

    #[test]
    fn steps_become_notes_with_swing_and_rolls() {
        let rhythm = Rhythm {
            name: "Test",
            bpm: 120,
            swing: 60.0,
            lines: &[("kick", "X..............."), ("closed", ".x..............")],
        };
        let notes = rhythm_notes(&rhythm, 2, 1000.0);
        let kicks: Vec<u64> = notes
            .iter()
            .filter(|n| n.key == 36)
            .map(|n| n.start)
            .collect();
        assert_eq!(kicks, [0, 4000], "one bar repeats");
        let hat = notes.iter().find(|n| n.key == 42).unwrap();
        assert_eq!(hat.start, 250 + 50, "the off sixteenth swings");
        let roll = Rhythm {
            lines: &[("closed", "r...............")],
            ..rhythm
        };
        let starts: Vec<u64> = rhythm_notes(&roll, 1, 1000.0)
            .iter()
            .map(|n| n.start)
            .collect();
        assert_eq!(starts, [0, 125]);
    }

    #[test]
    fn a_rhythm_goes_into_a_clip_at_the_playhead_bar() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut()
            .edit("Setup", |p| track = Some(p.add_track("Drums")));
        let track = track.unwrap();
        add_rhythm(&mut doc, track, &RHYTHMS[0], 2);
        let clip = doc.project().tracks[0].clips.first().unwrap().clone();
        assert_eq!(clip.start, 0);
        let bar = doc
            .project()
            .tempo
            .beat_to_sample(4.0, doc.project().sample_rate);
        assert_eq!(clip.length, 2 * bar);
        assert!(clip.notes.as_ref().is_some_and(|n| !n.is_empty()));
        assert_eq!(doc.take_revealed_clip(), Some(clip.id));
    }
}
