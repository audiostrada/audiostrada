//! The drum machine's editor: pads lit while their voice sounds, played with the mouse and
//! selected to edit; under them, tabs for the pad's sound, the pattern and the kit's effects.

use audiostrada_core::Note;
use audiostrada_core::routing::Channel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use audiostrada_dsp::drums::{LAYOUT, VOICES};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::effect_editors::{Params, choice_box, named_knob, segmented};
use crate::i18n::tr;
use crate::theme;

/// What the drum machine's editor remembers between frames.
#[derive(Debug, Default)]
pub struct DrumsState {
    /// The pad held with the mouse, and the track it plays.
    held: Option<(audiostrada_core::TrackId, u8)>,
    /// The voice whose settings are shown.
    selected: usize,
    sequencer: crate::drum_sequencer::SequencerState,
    tab: Tab,
}

/// What the lower half of the editor shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Pad,
    Pattern,
    Effects,
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
        name: "Trap: bounce",
        bpm: 140,
        swing: 50.0,
        lines: &[
            ("kick", "X.....x...x.....X.x.....x..x...."),
            ("clap", "........X...............X......."),
            ("snare", "........X...............X......."),
            ("closed", "x.x.x.x.x.x.rrrrx.x.x.x.x.rrx.x."),
            ("open", "......................x........."),
        ],
    },
    Rhythm {
        name: "Trap: rage",
        bpm: 150,
        swing: 50.0,
        lines: &[
            ("kick", "X..x..x...X.....X..x..x...x.x..."),
            ("snare", "........X...............X......."),
            ("clap", "........X...............X......."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.rrrrrrrr"),
            ("open", "..x.......x.......x.......x....."),
        ],
    },
    Rhythm {
        name: "Trap: plugg",
        bpm: 145,
        swing: 50.0,
        lines: &[
            ("kick", "X.......x.x.....X.........x....."),
            ("clap", "........X...............X......."),
            ("rim", "..........o.........o.......o..."),
            ("closed", "x...x...x...x...x...x...x.x.x.x."),
        ],
    },
    Rhythm {
        name: "Drill: New York",
        bpm: 142,
        swing: 50.0,
        lines: &[
            ("kick", "X.....x...x.....X..x...x..x....."),
            ("snare", "........X......x........X......."),
            ("closed", "x..x..x.x..x..x.x..x..x.x..rrx.."),
        ],
    },
    Rhythm {
        name: "Hip hop: West coast",
        bpm: 94,
        swing: 56.0,
        lines: &[
            ("kick", "X.......X.x.....X.......X..x...."),
            ("snare", "....X.......X.......X.......X..."),
            ("clap", "....X.......X.......X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.x.x."),
        ],
    },
    Rhythm {
        name: "Jersey club",
        bpm: 140,
        swing: 50.0,
        lines: &[
            ("kick", "X..X..X.X...X.X.X..X..X.X...X.X."),
            ("clap", "....X.......X.......X.......X..."),
            ("closed", "..x...x...x...x...x...x...x...x."),
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
        name: "Phonk",
        bpm: 130,
        swing: 50.0,
        lines: &[
            ("kick", "X......x..X.....X......x..X..x.."),
            ("snare", "....X.......X.......X.......X..."),
            ("closed", "x.x.x.x.x.x.x.x.x.x.x.x.x.x.rrx."),
            ("cowbell", "x..x..x.x..x..x.x..x..x...x.x.x."),
        ],
    },
    Rhythm {
        name: "UK garage",
        bpm: 132,
        swing: 62.0,
        lines: &[
            ("kick", "X......x..X.....X.........X....."),
            ("snare", "....X.......X.......X.......X..."),
            ("rim", "..x.....x.x...x...x.....x.x...x."),
            ("closed", "..x...x...x...x...x...x...x...x."),
        ],
    },
    Rhythm {
        name: "Afrobeats",
        bpm: 104,
        swing: 54.0,
        lines: &[
            ("kick", "X.....x.X.....x."),
            ("rim", "...x..x....x..x."),
            ("snare", "....X.......X..."),
            ("closed", "x.xxx.xxx.xxx.xx"),
            ("tambourine", "..x...x...x...x."),
        ],
    },
    Rhythm {
        name: "Amapiano",
        bpm: 113,
        swing: 56.0,
        lines: &[
            ("kick", "X...X...X...X..."),
            ("clap", "....x.......x..."),
            ("tambourine", "xxoxxxoxxxoxxxox"),
            ("rim", "..x..x....x..x.."),
            ("low_tom", "X.....x...x.X.......x...x.X...x."),
        ],
    },
    Rhythm {
        name: "Dancehall",
        bpm: 98,
        swing: 52.0,
        lines: &[
            ("kick", "X.....x.X.....x."),
            ("rim", "...x..x....x..x."),
            ("snare", "....X.......X..."),
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

/// A factory kit: every pad in `character` (0 for 808, 1 for 909), spread in stereo, the kicks
/// mostly dry and the delay fed by the accents only, then `values`.
fn kit(name: &str, character: f32, values: &[(&str, f32)]) -> crate::presets::Preset {
    let mut kit = crate::presets::preset(
        name,
        &[
            ("closed_pan", -20.0),
            ("open_pan", -20.0),
            ("high_tom_pan", -25.0),
            ("low_tom_pan", 25.0),
            ("cymbal_pan", -30.0),
            ("ride_pan", 30.0),
            ("tambourine_pan", 15.0),
            ("clave_pan", -15.0),
            ("cowbell_pan", 15.0),
            ("kick_reverb", 30.0),
            ("kick2_reverb", 20.0),
        ],
    );
    for (id, _, _) in VOICES {
        kit.params.insert(format!("{id}_model"), character);
        let accent = matches!(
            id,
            "rim" | "clap" | "open" | "cowbell" | "clave" | "tambourine" | "snare2"
        );
        kit.params
            .insert(format!("{id}_delay"), if accent { 100.0 } else { 0.0 });
    }
    for (key, value) in values {
        kit.params.insert((*key).to_owned(), *value);
    }
    kit
}

/// Ready-made kits: the two classic machines, then by style.
pub(crate) fn factory() -> Vec<crate::presets::Preset> {
    vec![
        kit(tr("808: Classic"), 0.0, &[("volume", -7.1)]),
        kit(
            tr("808: Boom"),
            0.0,
            &[
                ("kick_decay", 320.0),
                ("kick_tune", -3.0),
                ("kick_level", 90.0),
                ("kick_tone", 25.0),
                ("kick_pitch", 7.0),
                ("kick_pitch_decay", 25.0),
                ("kick2_decay", 380.0),
                ("kick2_tune", -5.0),
                ("snare_tone", 40.0),
                ("clap_decay", 120.0),
                ("drive", 25.0),
                ("comp", 25.0),
                ("room", 10.0),
                ("volume", -13.3),
            ],
        ),
        kit(
            tr("808: Electro"),
            0.0,
            &[
                ("kick_decay", 70.0),
                ("kick_tone", 80.0),
                ("kick_tune", 2.0),
                ("kick_pitch", 5.0),
                ("kick_pitch_decay", 15.0),
                ("snare_tone", 70.0),
                ("snare_decay", 70.0),
                ("closed_decay", 50.0),
                ("open_decay", 60.0),
                ("cowbell_level", 90.0),
                ("clave_level", 90.0),
                ("tambourine_level", 70.0),
                ("drive", 10.0),
                ("comp", 20.0),
                ("delay", 12.0),
                ("delay_note", 3.0),
                ("volume", -7.3),
            ],
        ),
        kit(
            tr("808: Miami bass"),
            0.0,
            &[
                ("kick_decay", 400.0),
                ("kick_tune", -2.0),
                ("kick_level", 92.0),
                ("kick_pitch", 5.0),
                ("kick_pitch_decay", 30.0),
                ("snare_tone", 75.0),
                ("snare_tune", 2.0),
                ("clap_level", 90.0),
                ("cowbell_level", 85.0),
                ("clave_level", 85.0),
                ("closed_tone", 70.0),
                ("low", 3.0),
                ("high", 2.0),
                ("drive", 15.0),
                ("comp", 30.0),
                ("volume", -14.3),
            ],
        ),
        kit(
            tr("909: House"),
            1.0,
            &[
                ("kick_decay", 90.0),
                ("kick_tune", -1.0),
                ("open_decay", 85.0),
                ("open_level", 75.0),
                ("clap_decay", 110.0),
                ("ride_level", 75.0),
                ("room", 18.0),
                ("drive", 10.0),
                ("comp", 20.0),
                ("volume", -7.7),
            ],
        ),
        kit(
            tr("909: Techno"),
            1.0,
            &[
                ("kick_decay", 120.0),
                ("kick_tune", -2.0),
                ("kick_tone", 60.0),
                ("kick2_decay", 70.0),
                ("kick2_tune", 2.0),
                ("rim_level", 85.0),
                ("closed_tone", 60.0),
                ("closed_decay", 80.0),
                ("clap_decay", 130.0),
                ("snare_tone", 40.0),
                ("room", 15.0),
                ("drive", 30.0),
                ("comp", 30.0),
                ("volume", -9.5),
            ],
        ),
        kit(
            tr("909: Hard"),
            1.0,
            &[
                ("kick_decay", 140.0),
                ("kick_tone", 75.0),
                ("kick_tune", -1.0),
                ("kick_pitch", 9.0),
                ("kick_pitch_decay", 18.0),
                ("snare_tone", 70.0),
                ("clap_level", 90.0),
                ("closed_tone", 70.0),
                ("open_decay", 60.0),
                ("drive", 55.0),
                ("crush", 12.0),
                ("comp", 40.0),
                ("room", 8.0),
                ("volume", -10.1),
            ],
        ),
        kit(
            tr("Trap: Atlanta"),
            0.0,
            &[
                ("kick_decay", 140.0),
                ("kick_tone", 60.0),
                ("kick_pitch", 12.0),
                ("kick_pitch_decay", 20.0),
                ("kick2_decay", 400.0),
                ("kick2_tune", -4.0),
                ("kick2_level", 95.0),
                ("kick2_tone", 30.0),
                ("snare_tone", 85.0),
                ("snare_tune", 4.0),
                ("snare_decay", 70.0),
                ("clap_level", 90.0),
                ("clap_decay", 120.0),
                ("clap_tune", 1.0),
                ("closed_tone", 85.0),
                ("closed_decay", 50.0),
                ("closed_level", 75.0),
                ("open_decay", 70.0),
                ("rim_tune", 3.0),
                ("kick_reverb", 0.0),
                ("drive", 18.0),
                ("comp", 30.0),
                ("high", 2.0),
                ("room", 12.0),
                ("room_size", 30.0),
                ("volume", -8.7),
            ],
        ),
        kit(
            tr("Trap: Dark"),
            0.0,
            &[
                ("kick_tune", -2.0),
                ("kick_decay", 120.0),
                ("kick2_decay", 400.0),
                ("kick2_tune", -6.0),
                ("kick2_tone", 10.0),
                ("snare_model", 1.0),
                ("snare_tune", -2.0),
                ("snare_tone", 40.0),
                ("snare_decay", 120.0),
                ("clap_decay", 150.0),
                ("closed_tone", 55.0),
                ("closed_high_cut", 9000.0),
                ("open_tone", 50.0),
                ("snare_delay", 100.0),
                ("room", 25.0),
                ("room_size", 70.0),
                ("delay", 10.0),
                ("low", 2.0),
                ("high", -3.0),
                ("comp", 25.0),
                ("volume", -6.7),
            ],
        ),
        kit(
            tr("Trap: Rage"),
            0.0,
            &[
                ("kick_tone", 85.0),
                ("kick_pitch", 12.0),
                ("kick_pitch_decay", 15.0),
                ("kick2_decay", 300.0),
                ("kick2_tune", -3.0),
                ("kick2_level", 95.0),
                ("snare_model", 1.0),
                ("snare_tune", 5.0),
                ("snare_tone", 90.0),
                ("clap_level", 95.0),
                ("closed_tone", 95.0),
                ("closed_decay", 45.0),
                ("open_tone", 85.0),
                ("drive", 55.0),
                ("crush", 10.0),
                ("comp", 60.0),
                ("high", 3.0),
                ("room", 8.0),
                ("volume", -10.7),
            ],
        ),
        kit(
            tr("Trap: Plugg"),
            0.0,
            &[
                ("kick_level", 70.0),
                ("kick_tone", 30.0),
                ("kick2_decay", 260.0),
                ("kick2_tune", -2.0),
                ("kick2_tone", 20.0),
                ("snare_tone", 60.0),
                ("snare_decay", 90.0),
                ("snare_high_cut", 7000.0),
                ("closed_tone", 60.0),
                ("closed_level", 65.0),
                ("closed_decay", 70.0),
                ("rim_tune", 5.0),
                ("rim_level", 85.0),
                ("room", 30.0),
                ("room_size", 60.0),
                ("delay", 20.0),
                ("delay_note", 6.0),
                ("crush", 8.0),
                ("high", -2.0),
                ("comp", 20.0),
                ("volume", -5.1),
            ],
        ),
        kit(
            tr("Drill: UK"),
            0.0,
            &[
                ("kick_decay", 80.0),
                ("kick_tone", 70.0),
                ("kick2_decay", 300.0),
                ("kick2_tune", -1.0),
                ("kick2_level", 92.0),
                ("snare_model", 1.0),
                ("snare_tune", 6.0),
                ("snare_tone", 85.0),
                ("snare_decay", 60.0),
                ("rim_tune", 7.0),
                ("rim_level", 85.0),
                ("closed_tone", 80.0),
                ("closed_decay", 45.0),
                ("low_tom_tune", 7.0),
                ("low_tom_decay", 60.0),
                ("kick_reverb", 0.0),
                ("drive", 15.0),
                ("comp", 25.0),
                ("room", 10.0),
                ("volume", -7.6),
            ],
        ),
        kit(
            tr("Trap: Hard 808"),
            0.0,
            &[
                ("kick_decay", 110.0),
                ("kick_tone", 70.0),
                ("kick_pitch", 14.0),
                ("kick_pitch_decay", 18.0),
                ("kick2_decay", 400.0),
                ("kick2_tune", -5.0),
                ("kick2_level", 100.0),
                ("kick2_tone", 45.0),
                ("snare_tone", 80.0),
                ("snare_tune", 3.0),
                ("snare_decay", 80.0),
                ("clap_level", 95.0),
                ("clap_decay", 140.0),
                ("closed_tone", 90.0),
                ("closed_decay", 40.0),
                ("closed_level", 70.0),
                ("open_decay", 80.0),
                ("kick_reverb", 0.0),
                ("kick2_reverb", 0.0),
                ("drive", 45.0),
                ("comp", 35.0),
                ("low", 2.0),
                ("room", 8.0),
                ("volume", -9.4),
            ],
        ),
        kit(
            tr("Trap: Memphis"),
            0.0,
            &[
                ("kick_decay", 160.0),
                ("kick_tune", -2.0),
                ("kick2_decay", 400.0),
                ("kick2_tune", -3.0),
                ("kick2_level", 90.0),
                ("snare_tone", 60.0),
                ("snare_decay", 90.0),
                ("clap_level", 85.0),
                ("closed_tone", 60.0),
                ("closed_decay", 60.0),
                ("cowbell_level", 95.0),
                ("cowbell_tune", -2.0),
                ("kick_reverb", 0.0),
                ("drive", 30.0),
                ("crush", 18.0),
                ("comp", 30.0),
                ("room", 14.0),
                ("volume", -9.8),
            ],
        ),
        kit(
            tr("Drill: Brooklyn"),
            0.0,
            &[
                ("kick_decay", 90.0),
                ("kick_tone", 80.0),
                ("kick_pitch", 10.0),
                ("kick2_decay", 400.0),
                ("kick2_tune", -2.0),
                ("kick2_level", 96.0),
                ("kick2_tone", 40.0),
                ("snare_model", 1.0),
                ("snare_tune", 4.0),
                ("snare_tone", 90.0),
                ("snare_decay", 75.0),
                ("rim_tune", 5.0),
                ("rim_level", 80.0),
                ("closed_tone", 85.0),
                ("closed_decay", 40.0),
                ("kick_reverb", 0.0),
                ("drive", 35.0),
                ("comp", 30.0),
                ("high", 2.0),
                ("room", 10.0),
                ("volume", -9.0),
            ],
        ),
        kit(
            tr("Hip hop: Jersey club"),
            0.0,
            &[
                ("kick_decay", 120.0),
                ("kick_tone", 75.0),
                ("kick_pitch", 10.0),
                ("kick_pitch_decay", 15.0),
                ("clap_level", 100.0),
                ("clap_decay", 150.0),
                ("clap_tune", 2.0),
                ("snare_tone", 75.0),
                ("closed_tone", 85.0),
                ("closed_decay", 40.0),
                ("open_decay", 50.0),
                ("kick_reverb", 0.0),
                ("drive", 20.0),
                ("comp", 35.0),
                ("high", 2.0),
                ("room", 14.0),
                ("volume", -9.0),
            ],
        ),
        kit(
            tr("Drill: New York"),
            0.0,
            &[
                ("kick_tone", 75.0),
                ("kick_pitch", 9.0),
                ("kick_pitch_decay", 20.0),
                ("kick2_decay", 380.0),
                ("kick2_tune", -3.0),
                ("kick2_level", 95.0),
                ("snare_model", 1.0),
                ("snare_tune", 3.0),
                ("snare_tone", 75.0),
                ("clap_level", 95.0),
                ("closed_tone", 85.0),
                ("drive", 40.0),
                ("crush", 6.0),
                ("comp", 45.0),
                ("high", 2.0),
                ("volume", -9.1),
            ],
        ),
        kit(
            tr("Hip hop: Boom bap"),
            0.0,
            &[
                ("kick_decay", 120.0),
                ("kick_tone", 45.0),
                ("kick_level", 95.0),
                ("kick_pitch", 5.0),
                ("kick_pitch_decay", 20.0),
                ("snare_model", 1.0),
                ("snare_tune", -2.0),
                ("snare_tone", 45.0),
                ("snare_decay", 110.0),
                ("closed_tone", 45.0),
                ("closed_decay", 90.0),
                ("drive", 30.0),
                ("crush", 25.0),
                ("comp", 40.0),
                ("high", -3.0),
                ("room", 10.0),
                ("volume", -9.7),
            ],
        ),
        kit(
            tr("Hip hop: West coast"),
            0.0,
            &[
                ("kick_decay", 200.0),
                ("kick_tune", -1.0),
                ("kick_level", 90.0),
                ("clap_level", 90.0),
                ("snare_tone", 65.0),
                ("closed_tone", 70.0),
                ("tambourine_level", 75.0),
                ("open_decay", 80.0),
                ("high", 2.0),
                ("room", 15.0),
                ("comp", 20.0),
                ("volume", -8.4),
            ],
        ),
        kit(
            tr("Hip hop: Phonk"),
            0.0,
            &[
                ("kick2_decay", 300.0),
                ("kick2_tune", -3.0),
                ("kick2_level", 95.0),
                ("kick_tone", 70.0),
                ("cowbell_level", 100.0),
                ("cowbell_decay", 200.0),
                ("cowbell_tone", 60.0),
                ("cowbell_pan", 0.0),
                ("snare_model", 1.0),
                ("snare_tone", 70.0),
                ("closed_tone", 80.0),
                ("delay", 25.0),
                ("delay_note", 5.0),
                ("drive", 45.0),
                ("crush", 15.0),
                ("comp", 40.0),
                ("volume", -9.0),
            ],
        ),
        kit(
            tr("Lo-fi: Dusty"),
            0.0,
            &[
                ("kick_tone", 20.0),
                ("kick_decay", 110.0),
                ("snare_model", 1.0),
                ("snare_tone", 35.0),
                ("snare_tune", -2.0),
                ("closed_tone", 35.0),
                ("closed_high_cut", 6000.0),
                ("open_tone", 30.0),
                ("open_high_cut", 6000.0),
                ("drive", 30.0),
                ("crush", 45.0),
                ("comp", 35.0),
                ("low", 2.0),
                ("high", -5.0),
                ("room", 18.0),
                ("volume", -9.4),
            ],
        ),
        kit(
            tr("Lo-fi: Late night"),
            0.0,
            &[
                ("kick_level", 85.0),
                ("kick_tone", 30.0),
                ("kick_decay", 140.0),
                ("snare_tone", 40.0),
                ("snare_decay", 120.0),
                ("snare_high_cut", 5000.0),
                ("rim_level", 85.0),
                ("rim_tune", -2.0),
                ("closed_tone", 40.0),
                ("closed_level", 65.0),
                ("closed_high_cut", 7000.0),
                ("crush", 25.0),
                ("drive", 15.0),
                ("comp", 25.0),
                ("high", -4.0),
                ("room", 35.0),
                ("room_size", 65.0),
                ("delay", 18.0),
                ("delay_note", 6.0),
                ("volume", -8.9),
            ],
        ),
        kit(
            tr("Afro: Afrobeats"),
            0.0,
            &[
                ("kick_decay", 110.0),
                ("kick_tone", 55.0),
                ("rim_tune", 3.0),
                ("rim_level", 85.0),
                ("clave_level", 80.0),
                ("tambourine_level", 75.0),
                ("closed_model", 1.0),
                ("closed_tone", 60.0),
                ("closed_decay", 70.0),
                ("closed_level", 65.0),
                ("low_tom_tune", 3.0),
                ("mid_tom_tune", 3.0),
                ("high_tom_tune", 3.0),
                ("snare_model", 1.0),
                ("snare_tune", 4.0),
                ("snare_tone", 60.0),
                ("snare_decay", 70.0),
                ("room", 15.0),
                ("comp", 20.0),
                ("delay", 10.0),
                ("volume", -6.6),
            ],
        ),
        kit(
            tr("Afro: Amapiano"),
            1.0,
            &[
                ("kick_decay", 70.0),
                ("kick_level", 85.0),
                ("low_tom_model", 0.0),
                ("low_tom_tune", -7.0),
                ("low_tom_decay", 280.0),
                ("low_tom_tone", 30.0),
                ("low_tom_level", 95.0),
                ("low_tom_pitch", 12.0),
                ("low_tom_pitch_decay", 40.0),
                ("low_tom_pan", 0.0),
                ("low_tom_reverb", 30.0),
                ("mid_tom_model", 0.0),
                ("mid_tom_tune", -2.0),
                ("mid_tom_decay", 220.0),
                ("mid_tom_pitch", 12.0),
                ("mid_tom_pitch_decay", 40.0),
                ("mid_tom_pan", 0.0),
                ("tambourine_level", 80.0),
                ("clap_decay", 90.0),
                ("rim_level", 85.0),
                ("delay", 18.0),
                ("delay_note", 6.0),
                ("room", 20.0),
                ("room_size", 55.0),
                ("comp", 25.0),
                ("volume", -8.2),
            ],
        ),
        kit(
            tr("Afro: Afro house"),
            1.0,
            &[
                ("kick_decay", 95.0),
                ("kick_tone", 45.0),
                ("low_tom_tune", 2.0),
                ("mid_tom_tune", 2.0),
                ("high_tom_tune", 2.0),
                ("low_tom_decay", 80.0),
                ("mid_tom_decay", 80.0),
                ("high_tom_decay", 80.0),
                ("rim_level", 80.0),
                ("clave_level", 75.0),
                ("ride_level", 70.0),
                ("closed_tone", 55.0),
                ("tambourine_level", 70.0),
                ("room", 22.0),
                ("room_size", 50.0),
                ("delay", 12.0),
                ("delay_note", 3.0),
                ("comp", 25.0),
                ("volume", -7.5),
            ],
        ),
        kit(
            tr("Latin: Reggaeton"),
            0.0,
            &[
                ("kick_decay", 150.0),
                ("kick_tone", 55.0),
                ("kick_level", 95.0),
                ("snare_model", 1.0),
                ("snare_tune", 3.0),
                ("snare_tone", 75.0),
                ("snare_decay", 60.0),
                ("rim_level", 85.0),
                ("closed_tone", 70.0),
                ("comp", 30.0),
                ("room", 10.0),
                ("volume", -8.1),
            ],
        ),
        kit(
            tr("Latin: Dancehall"),
            0.0,
            &[
                ("kick_decay", 120.0),
                ("kick_tone", 60.0),
                ("rim_level", 95.0),
                ("rim_tune", 2.0),
                ("snare_model", 1.0),
                ("snare_tune", 4.0),
                ("snare_tone", 70.0),
                ("closed_tone", 70.0),
                ("clave_level", 80.0),
                ("delay", 10.0),
                ("delay_note", 6.0),
                ("comp", 25.0),
                ("room", 12.0),
                ("volume", -7.6),
            ],
        ),
        kit(
            tr("House: Deep"),
            1.0,
            &[
                ("kick_decay", 90.0),
                ("kick_tone", 40.0),
                ("kick_level", 90.0),
                ("rim_model", 0.0),
                ("rim_level", 85.0),
                ("clap_decay", 120.0),
                ("closed_tone", 55.0),
                ("closed_decay", 70.0),
                ("open_decay", 80.0),
                ("tambourine_level", 60.0),
                ("low", 1.0),
                ("high", -2.0),
                ("room", 20.0),
                ("room_size", 50.0),
                ("delay", 15.0),
                ("comp", 20.0),
                ("volume", -6.8),
            ],
        ),
        kit(
            tr("House: Tech house"),
            1.0,
            &[
                ("kick_decay", 85.0),
                ("kick_tone", 70.0),
                ("kick_pitch", 7.0),
                ("kick_pitch_decay", 20.0),
                ("clap_level", 90.0),
                ("clap_decay", 100.0),
                ("closed_tone", 75.0),
                ("closed_decay", 50.0),
                ("open_decay", 60.0),
                ("open_tone", 70.0),
                ("rim_tune", 3.0),
                ("comp", 45.0),
                ("drive", 15.0),
                ("delay", 12.0),
                ("delay_note", 3.0),
                ("room", 12.0),
                ("volume", -9.0),
            ],
        ),
        kit(
            tr("House: UK garage"),
            1.0,
            &[
                ("kick_decay", 70.0),
                ("kick_tone", 60.0),
                ("rim_level", 90.0),
                ("rim_tune", 3.0),
                ("snare_tone", 70.0),
                ("snare_tune", 3.0),
                ("closed_decay", 60.0),
                ("closed_tone", 75.0),
                ("open_decay", 55.0),
                ("delay", 12.0),
                ("delay_note", 3.0),
                ("comp", 30.0),
                ("room", 12.0),
                ("volume", -8.3),
            ],
        ),
        kit(
            tr("Techno: Berlin"),
            1.0,
            &[
                ("kick_decay", 130.0),
                ("kick_tone", 60.0),
                ("kick_reverb", 100.0),
                ("rim_level", 85.0),
                ("closed_tone", 65.0),
                ("clap_decay", 120.0),
                ("room", 35.0),
                ("room_size", 75.0),
                ("low", 2.0),
                ("drive", 25.0),
                ("comp", 50.0),
                ("volume", -9.4),
            ],
        ),
        kit(
            tr("Techno: Industrial"),
            1.0,
            &[
                ("kick_decay", 120.0),
                ("kick_tone", 85.0),
                ("kick_pitch", 12.0),
                ("kick_pitch_decay", 15.0),
                ("snare_tone", 90.0),
                ("clap_level", 90.0),
                ("closed_tone", 90.0),
                ("rim_tune", -3.0),
                ("drive", 70.0),
                ("crush", 20.0),
                ("comp", 55.0),
                ("room", 20.0),
                ("room_size", 85.0),
                ("high", 2.0),
                ("volume", -10.1),
            ],
        ),
        kit(
            tr("Techno: Minimal"),
            1.0,
            &[
                ("kick_decay", 45.0),
                ("snare_decay", 55.0),
                ("clap_decay", 50.0),
                ("open_decay", 45.0),
                ("cymbal_decay", 40.0),
                ("low_tom_decay", 55.0),
                ("mid_tom_decay", 55.0),
                ("high_tom_decay", 55.0),
                ("ride_decay", 50.0),
                ("closed_decay", 70.0),
                ("rim_level", 85.0),
                ("delay", 10.0),
                ("delay_note", 3.0),
                ("volume", -8.4),
            ],
        ),
        kit(
            tr("Electro: Breaks"),
            0.0,
            &[
                ("kick_decay", 90.0),
                ("kick_tone", 70.0),
                ("kick_pitch", 7.0),
                ("kick_pitch_decay", 20.0),
                ("snare_model", 1.0),
                ("snare_tune", 2.0),
                ("snare_tone", 70.0),
                ("closed_model", 1.0),
                ("drive", 20.0),
                ("comp", 35.0),
                ("room", 12.0),
                ("volume", -8.1),
            ],
        ),
        kit(
            tr("Electro: Synthwave"),
            1.0,
            &[
                ("kick_model", 0.0),
                ("kick_decay", 110.0),
                ("kick_reverb", 10.0),
                ("snare_tune", -2.0),
                ("snare_tone", 55.0),
                ("snare_decay", 110.0),
                ("low_tom_decay", 140.0),
                ("mid_tom_decay", 140.0),
                ("high_tom_decay", 140.0),
                ("low_tom_tone", 60.0),
                ("mid_tom_tone", 60.0),
                ("high_tom_tone", 60.0),
                ("room", 45.0),
                ("room_size", 85.0),
                ("comp", 50.0),
                ("high", 1.0),
                ("volume", -8.9),
            ],
        ),
        kit(
            tr("Electro: Hyperpop"),
            0.0,
            &[
                ("kick_decay", 160.0),
                ("kick_tone", 90.0),
                ("kick_pitch", 12.0),
                ("kick_pitch_decay", 15.0),
                ("snare_model", 1.0),
                ("snare_tune", 7.0),
                ("snare_tone", 95.0),
                ("clap_tune", 3.0),
                ("closed_tone", 95.0),
                ("crush", 25.0),
                ("drive", 50.0),
                ("comp", 60.0),
                ("high", 4.0),
                ("volume", -11.0),
            ],
        ),
        kit(
            tr("Drum and bass: Liquid"),
            1.0,
            &[
                ("kick_decay", 70.0),
                ("kick_tone", 60.0),
                ("snare_tune", 4.0),
                ("snare_tone", 72.0),
                ("snare_decay", 80.0),
                ("snare2_tune", 6.0),
                ("ride_level", 70.0),
                ("closed_tone", 70.0),
                ("room", 22.0),
                ("room_size", 55.0),
                ("comp", 25.0),
                ("volume", -9.8),
            ],
        ),
        kit(
            tr("Drum and bass: Neuro"),
            1.0,
            &[
                ("kick_decay", 60.0),
                ("kick_tone", 85.0),
                ("kick_pitch", 12.0),
                ("kick_pitch_decay", 12.0),
                ("snare_tune", 3.0),
                ("snare_tone", 90.0),
                ("snare_decay", 70.0),
                ("drive", 45.0),
                ("crush", 8.0),
                ("comp", 60.0),
                ("low", 2.0),
                ("high", 2.0),
                ("volume", -12.0),
            ],
        ),
    ]
}

/// The dark screen behind the pads and the pad's editor.
pub(crate) const SCREEN: Color32 = Color32::from_rgb(16, 19, 25);

/// The color of a pad's family: kicks, snares and claps, hats, toms, cymbals, percussion.
pub(crate) fn family(pad: usize) -> Color32 {
    use audiostrada_dsp::drums::{KINDS, Kind};
    match KINDS[pad] {
        Kind::Kick => Color32::from_rgb(232, 86, 72),
        Kind::Snare | Kind::Clap | Kind::Rim => Color32::from_rgb(238, 146, 62),
        Kind::Closed | Kind::Open => Color32::from_rgb(232, 202, 76),
        Kind::Tom(_) => Color32::from_rgb(112, 196, 104),
        Kind::Crash | Kind::Ride => Color32::from_rgb(82, 184, 220),
        _ => Color32::from_rgb(168, 126, 228),
    }
}

/// The settings that shape pad `pad`'s sound, to know when its drawing is stale.
fn signature(params: &Params, pad: usize, file: Option<&std::path::Path>) -> u64 {
    use std::hash::{Hash, Hasher};
    let (id, _, _) = VOICES[pad];
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for field in ["tune", "decay", "tone", "model"] {
        params.get(&format!("{id}_{field}")).to_bits().hash(&mut h);
    }
    params.get("drive").to_bits().hash(&mut h);
    params.get("crush").to_bits().hash(&mut h);
    file.hash(&mut h);
    h.finish()
}

/// Peaks of pad `pad`'s sound for drawing, normalized, rendered again when its settings change.
fn shape(params: &Params, pad: usize, file: Option<&std::path::Path>) -> Arc<Vec<f32>> {
    type Cache = HashMap<(usize, u64), Arc<Vec<f32>>>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let key = (pad, signature(params, pad, file));
    let Ok(mut cache) = CACHE.lock() else {
        return Arc::default();
    };
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(peaks) = cache.get(&key) {
        return Arc::clone(peaks);
    }
    let sound = match file {
        Some(path) => audiostrada_media::load(path)
            .map(|audio| audio.channel(0).to_vec())
            .unwrap_or_default(),
        None => audiostrada_dsp::drums::preview(&params.values, pad, 2.0),
    };
    // The silence at the end is left out, so that short and long hits both fill the cell.
    let end = sound
        .iter()
        .rposition(|s| s.abs() > 2e-3)
        .map_or(0, |i| i + 1);
    let bins = 160;
    let per = (end / bins).max(1);
    let mut peaks: Vec<f32> = sound[..end]
        .chunks(per)
        .map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs())))
        .collect();
    let top = peaks.iter().fold(1e-6f32, |m, v| m.max(*v));
    for v in &mut peaks {
        *v /= top;
    }
    let peaks = Arc::new(peaks);
    cache.retain(|(p, _), _| *p != pad);
    cache.insert(key, Arc::clone(&peaks));
    peaks
}

/// A sound's peaks as a mirrored waveform in `rect`.
pub(crate) fn waveform(painter: &egui::Painter, rect: Rect, peaks: &[f32], color: Color32) {
    if peaks.is_empty() {
        return;
    }
    let n = peaks.len();
    let mid = rect.center().y;
    let half = rect.height() / 2.0;
    let top: Vec<Pos2> = peaks
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                rect.left() + i as f32 / (n - 1).max(1) as f32 * rect.width(),
                mid - v.sqrt() * half,
            )
        })
        .collect();
    for pair in top.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let quad = vec![a, b, pos2(b.x, 2.0 * mid - b.y), pos2(a.x, 2.0 * mid - a.y)];
        painter.add(Shape::convex_polygon(
            quad,
            color.gamma_multiply(0.55),
            Stroke::NONE,
        ));
    }
    painter.add(Shape::line(top.clone(), Stroke::new(1.2, color)));
    let bottom: Vec<Pos2> = top.iter().map(|q| pos2(q.x, 2.0 * mid - q.y)).collect();
    painter.add(Shape::line(bottom, Stroke::new(1.2, color)));
}

/// The file pad `pad` plays, if any.
fn pad_file(params: &Params, pad: usize) -> Option<std::path::PathBuf> {
    let project = params.doc.project();
    audiostrada_core::routing::find_insert(project, params.channel, params.insert)
        .and_then(|i| i.files.get(VOICES[pad].0).cloned())
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut DrumsState) {
    let mut pressed = None;
    // The pads, four by four on the dark screen, each with its sound drawn.
    egui::Frame::new()
        .fill(SCREEN)
        .corner_radius(10.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            let gap = 8.0;
            let width = ((ui.available_width() - 3.0 * gap) / 4.0).floor();
            let height = 58.0;
            for row in LAYOUT.chunks(4) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for &pad in row {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
                        let file = pad_file(params, pad);
                        cell(
                            ui,
                            params,
                            rect,
                            pad,
                            file.as_deref(),
                            meters,
                            state.selected == pad,
                        );
                        if response.is_pointer_button_down_on() {
                            // Lower on the pad plays louder, as a harder hit.
                            let depth = response
                                .interact_pointer_pos()
                                .map_or(0.7, |pos| (pos.y - rect.top()) / rect.height());
                            pressed =
                                Some((VOICES[pad].2, (40.0 + 87.0 * depth.clamp(0.0, 1.0)) as u8));
                            state.selected = pad;
                        }
                        response.on_hover_text(tr(
                            "Click to play and edit it; lower on the pad plays louder",
                        ));
                    }
                });
                ui.add_space(gap - ui.spacing().item_spacing.y);
            }
        });
    ui.add_space(8.0);
    tabs(ui, &mut state.tab, state.selected);
    ui.add_space(8.0);
    screen(ui, |ui| match state.tab {
        Tab::Pad => editor(ui, params, state.selected.min(VOICES.len() - 1), meters),
        Tab::Pattern => crate::drum_sequencer::show(
            ui,
            params,
            &mut state.sequencer,
            &mut state.selected,
            meters,
        ),
        Tab::Effects => effects(ui, params),
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

/// One pad: its family's color lighting up as it sounds, its name, its sound drawn, its
/// character or its file.
fn cell(
    ui: &Ui,
    params: &Params,
    rect: Rect,
    pad: usize,
    file: Option<&std::path::Path>,
    meters: &[f32],
    selected: bool,
) {
    let painter = ui.painter_at(rect.expand(3.0));
    let color = family(pad);
    let glow = meters
        .get(pad)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        .sqrt();
    let fill = theme::mix(Color32::from_rgb(30, 35, 44), color, 0.22 + 0.6 * glow);
    painter.rect_filled(rect, 7.0, fill);
    // A bright edge on top, as on a lit pad.
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 3.0)),
        egui::CornerRadius {
            nw: 7,
            ne: 7,
            sw: 0,
            se: 0,
        },
        color.gamma_multiply(0.6 + 0.4 * glow),
    );
    if selected {
        painter.rect_stroke(
            rect,
            7.0,
            Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Outside,
        );
    }
    let (id, name, key) = VOICES[pad];
    let ink = Color32::from_rgb(236, 240, 246);
    painter.text(
        rect.left_top() + vec2(9.0, 9.0),
        Align2::LEFT_TOP,
        tr(name),
        theme::strong(12.5),
        ink,
    );
    let badge = match file {
        Some(path) => path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
        None => {
            let model = params.get(&format!("{id}_model")).round().max(0.0) as usize;
            tr(audiostrada_dsp::drums::MODELS[model.min(2)]).to_owned()
        }
    };
    painter.text(
        rect.right_top() + vec2(-8.0, 10.0),
        Align2::RIGHT_TOP,
        badge,
        FontId::proportional(10.0),
        ink.gamma_multiply(0.7),
    );
    painter.text(
        rect.right_bottom() + vec2(-7.0, -5.0),
        Align2::RIGHT_BOTTOM,
        key.to_string(),
        FontId::monospace(9.0),
        ink.gamma_multiply(0.45),
    );
    let wave = Rect::from_min_max(
        rect.left_top() + vec2(9.0, 30.0),
        rect.right_bottom() - vec2(28.0, 8.0),
    );
    waveform(
        &painter,
        wave,
        &shape(params, pad, file),
        ink.gamma_multiply(0.85),
    );
}

/// The dark screen filling the width, with light controls whatever the theme.
pub(crate) fn screen(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(SCREEN)
        .corner_radius(10.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            let selection = ui.visuals().selection;
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().selection = selection;
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// The tabs over the lower half, as wide as the editor.
fn tabs(ui: &mut Ui, tab: &mut Tab, pad: usize) {
    let p = theme::current(ui);
    let pad_name = tr(VOICES[pad.min(VOICES.len() - 1)].1);
    let list = [
        (
            Tab::Pad,
            pad_name,
            tr("The selected pad's sound: tune, envelope, filter and sends"),
        ),
        (
            Tab::Pattern,
            tr("Pattern"),
            tr("Steps of the MIDI clip under the playhead"),
        ),
        (
            Tab::Effects,
            tr("Kit effects"),
            tr("Compression, color, EQ, delay and reverb of the whole kit"),
        ),
    ];
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, theme::mix(p.panel, p.base, 0.5));
    let width = rect.width() / list.len() as f32;
    for (k, (which, label, hint)) in list.into_iter().enumerate() {
        let cell = Rect::from_min_size(
            pos2(rect.left() + k as f32 * width, rect.top()),
            vec2(width, rect.height()),
        )
        .shrink(3.0);
        let response = ui
            .interact(cell, ui.scope_id().with(("drums-tab", k)), Sense::click())
            .on_hover_text(hint);
        let on = *tab == which;
        let fill = if on {
            SCREEN
        } else if response.hovered() {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(cell, 6.0, fill);
        let ink = match (on, which) {
            (true, Tab::Pad) => family(pad),
            (true, _) => Color32::from_rgb(236, 240, 246),
            (false, _) => p.weak,
        };
        painter.text(
            cell.center(),
            Align2::CENTER_CENTER,
            label,
            theme::strong(13.0),
            ink,
        );
        if response.clicked() {
            *tab = which;
        }
    }
}

/// Modules side by side across the width, each as wide as its knobs, titled in `tint`; the
/// place of each control, in order, from its top to the bottom of what it drew.
pub(crate) fn modules(
    ui: &mut Ui,
    params: &mut Params,
    list: &[(&str, Vec<(String, &str)>)],
    tint: Color32,
    size: f32,
) -> Vec<Rect> {
    let p = theme::current(ui);
    let gap = 8.0;
    let mut places = Vec::new();
    let full = ui.available_width();
    let count: usize = list.iter().map(|m| m.1.len()).sum();
    let unit = (full - gap * (list.len().max(1) - 1) as f32) / count.max(1) as f32;
    let top = ui.cursor().min;
    let backs: Vec<_> = list.iter().map(|_| ui.painter().add(Shape::Noop)).collect();
    let (mut x, mut bottom) = (top.x, top.y);
    let mut rects = Vec::new();
    for (title, knobs) in list {
        let width = unit * knobs.len() as f32;
        let rect = Rect::from_min_size(pos2(x, top.y), vec2(width, 0.0));
        let inner = rect.shrink2(vec2(8.0, 0.0)).translate(vec2(0.0, 8.0));
        let builder = egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(inner.min, vec2(inner.width(), 400.0)))
            .layout(egui::Layout::top_down(egui::Align::Min));
        let used = ui
            .scope_builder(builder, |ui| {
                ui.label(RichText::new(*title).strong().size(12.0).color(tint));
                let row = ui.cursor().min;
                let cell = inner.width() / knobs.len() as f32;
                let mut low = row.y;
                for (k, (id, label)) in knobs.iter().enumerate() {
                    let place = Rect::from_min_size(
                        pos2(row.x + k as f32 * cell, row.y),
                        vec2(cell, 200.0),
                    );
                    let builder = egui::UiBuilder::new()
                        .max_rect(place)
                        .layout(egui::Layout::top_down(egui::Align::Center));
                    let choice = params.param(id).is_some_and(|p| !p.choices.is_empty());
                    let toggle = params.param(id).is_some_and(|p| p.toggle);
                    let shown = ui
                        .scope_builder(builder, |ui| {
                            if toggle {
                                // A switch in a knob's place, level with the knobs' centers.
                                ui.add_space(size / 2.0 + 2.0);
                                let on = params.get(id) >= 0.5;
                                let response = crate::effect_editors::pill_switch(ui, on, label);
                                let changed =
                                    response.clicked().then_some(if on { 0.0 } else { 1.0 });
                                params.set(id, changed, &response);
                                if changed.is_some() {
                                    params.doc.history_mut().end_gesture();
                                }
                            } else if choice {
                                // A list in a knob's place, level with the knobs' centers.
                                ui.label(RichText::new(*label).size(10.5).color(p.weak));
                                ui.add_space(size / 2.0 - 12.0);
                                choice_box(ui, params, id);
                            } else {
                                named_knob(ui, params, id, label, size, tint);
                            }
                        })
                        .response
                        .rect;
                    low = low.max(shown.bottom());
                    places.push(Rect::from_min_max(
                        place.min,
                        pos2(place.right(), shown.bottom()),
                    ));
                }
                ui.allocate_rect(
                    Rect::from_min_max(row, pos2(row.x + inner.width(), low)),
                    Sense::hover(),
                );
            })
            .response
            .rect;
        bottom = bottom.max(used.bottom() + 8.0);
        rects.push(rect);
        x += width + gap;
    }
    for (rect, back) in rects.into_iter().zip(backs) {
        let rect = Rect::from_min_max(rect.min, pos2(rect.right(), bottom));
        ui.painter()
            .set(back, Shape::rect_filled(rect, 8.0, MODULE));
    }
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + full, bottom)),
        Sense::hover(),
    );
    places
}

/// The panels inside the screen.
pub(crate) const MODULE: Color32 = Color32::from_rgb(25, 29, 38);

/// The pad being edited: its sound large with its character or file and its main knobs, then
/// its envelope, pitch, filter and sends.
fn editor(ui: &mut Ui, params: &mut Params, pad: usize, meters: &[f32]) {
    let (id, name, _) = VOICES[pad];
    let color = family(pad);
    let file = pad_file(params, pad);
    let side = 260.0;
    ui.horizontal_top(|ui| {
        // The left column takes what the right one, the gap and the two spacings leave.
        let wide = ui.available_width() - side - 12.0 - 2.0 * ui.spacing().item_spacing.x;
        ui.vertical(|ui| {
            ui.set_width(wide);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr(name)).size(19.0).strong().color(color));
                ui.add_space(8.0);
                if file.is_none() {
                    segmented(
                        ui,
                        params,
                        &format!("{id}_model"),
                        &[
                            "Deep and long, the classic analog machine",
                            "Punchy and bright, made for house and techno",
                            "Modeled skins, wires and cymbals: closer to a real kit",
                        ],
                    );
                }
            });
            let (rect, _) = ui.allocate_exact_size(vec2(wide, 84.0), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 8.0, MODULE);
            let glow = meters.get(pad).copied().unwrap_or(0.0).min(1.0);
            waveform(
                &painter,
                rect.shrink2(vec2(10.0, 8.0)),
                &shape(params, pad, file.as_deref()),
                color.gamma_multiply(0.7 + 0.3 * glow),
            );
            ui.horizontal(|ui| {
                let slot = &audiostrada_dsp::drums::SLOTS[pad];
                match &file {
                    Some(path) => {
                        let shown = path
                            .file_name()
                            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                        ui.label(RichText::new(shown).size(11.0))
                            .on_hover_text(path.display().to_string());
                        if let Some(Err(e)) =
                            params.doc.player().effect_file(params.insert, slot.id)
                        {
                            ui.colored_label(crate::settings::ERROR, e);
                        }
                    }
                    None => {
                        ui.weak(tr("Synthesized: load a sample to play it instead"));
                    }
                }
                let (doc, channel, insert) = (&mut *params.doc, params.channel, params.insert);
                crate::mixer::load_button(ui, doc, channel, insert, slot);
                if file.is_some() && ui.button(tr("Remove")).clicked() {
                    let _ =
                        params
                            .doc
                            .set_effect_file(params.channel, params.insert, slot.id, None);
                }
            });
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width(side);
            modules(
                ui,
                params,
                &[(
                    tr("Sound"),
                    vec![
                        (format!("{id}_level"), tr("Level")),
                        (format!("{id}_pan"), tr("Pan")),
                        (format!("{id}_tune"), tr("Tune")),
                    ],
                )],
                color,
                46.0,
            );
        });
    });
    ui.add_space(10.0);
    let field = |f: &str| format!("{id}_{f}");
    modules(
        ui,
        params,
        &[
            (
                tr("Envelope"),
                vec![
                    (field("attack"), tr("Attack")),
                    (field("decay"), tr("Decay")),
                ],
            ),
            (
                tr("Pitch"),
                vec![
                    (field("pitch"), tr("Amount")),
                    (field("pitch_decay"), tr("Length")),
                ],
            ),
            (
                tr("Filter"),
                vec![
                    (field("tone"), tr("Tone")),
                    (field("low_cut"), tr("Low cut")),
                    (field("high_cut"), tr("High cut")),
                ],
            ),
            (
                tr("Sends"),
                vec![
                    (field("delay"), tr("Delay")),
                    (field("reverb"), tr("Reverb")),
                ],
            ),
        ],
        color,
        44.0,
    );
}

/// The kit's effects, in the order the sound goes through them.
fn effects(ui: &mut Ui, params: &mut Params) {
    let tint = theme::current(ui).accent;
    let knob = |id: &str, label: &'static str| (id.to_owned(), tr(label));
    modules(
        ui,
        params,
        &[
            (
                tr("Dynamics"),
                vec![knob("accent", "Accent"), knob("comp", "Compression")],
            ),
            (
                tr("Color"),
                vec![knob("drive", "Drive"), knob("crush", "Crush")],
            ),
            (tr("EQ"), vec![knob("low", "Bass"), knob("high", "Treble")]),
        ],
        tint,
        52.0,
    );
    ui.add_space(8.0);
    modules(
        ui,
        params,
        &[
            (
                tr("Delay"),
                vec![
                    knob("delay_note", "Time"),
                    knob("delay_feedback", "Feedback"),
                    knob("delay", "Return"),
                ],
            ),
            (
                tr("Reverb"),
                vec![knob("room_size", "Size"), knob("room", "Return")],
            ),
            (tr("Output"), vec![knob("volume", "Volume")]),
        ],
        tint,
        52.0,
    );
    ui.add_space(4.0);
    ui.weak(tr(
        "Each pad sends to the delay and the reverb: set how much in its Sends",
    ));
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_pad_and_tab_draws() {
        for tab in [Tab::Pad, Tab::Pattern, Tab::Effects] {
            crate::effect_editors::draw_for_test("drums", &[], VOICES.len(), |frame, state| {
                state.drums.tab = tab;
                state.drums.selected = frame;
            });
        }
    }

    use super::*;

    #[test]
    fn kits_have_matching_levels() {
        use audiostrada_dsp::MidiEvent;
        let info = audiostrada_dsp::info("drums").unwrap();
        let mut report = Vec::new();
        for kit in factory() {
            let mut drums = audiostrada_dsp::create("drums").unwrap();
            for (i, p) in info.params.iter().enumerate() {
                drums.set_param(i, kit.params.get(p.id).copied().unwrap_or(p.default));
            }
            drums.prepare(48_000, 500);
            // Two bars at 120 bpm: kick on one and three, snare on two and four, hats in eighths,
            // a rim, a clap, an open hat and a crash with the kick: every pad a beat uses.
            // Drums are heard by their hits: every kit peaks at the same level on the same beat.
            let mut top = 0.0f32;
            let (mut energy, mut count) = (0.0f64, 0usize);
            for eighth in 0..16 {
                let mut hits = vec![42];
                match eighth % 8 {
                    0 => hits.push(36),
                    2 | 6 => hits.push(38),
                    3 => hits.push(37),
                    5 => hits.push(39),
                    4 => hits.push(36),
                    7 => hits = vec![46],
                    _ => {}
                }
                if eighth == 8 {
                    hits.push(49);
                }
                for key in hits {
                    drums.midi(&MidiEvent::note_on(0, 9, key, 100));
                }
                for _ in 0..24 {
                    let (mut l, mut r) = (vec![0.0f32; 500], vec![0.0f32; 500]);
                    drums.process(&mut l, &mut r);
                    top = l.iter().chain(&r).fold(top, |m, s| m.max(s.abs()));
                    energy += l.iter().chain(&r).map(|s| f64::from(s * s)).sum::<f64>();
                    count += 1000;
                }
            }
            let loud = 10.0 * (energy / count as f64).max(1e-12).log10();
            report.push((kit.name.clone(), 20.0 * top.max(1e-9).log10(), loud));
        }
        for (name, top, loud) in &report {
            eprintln!("{name}: peak {top:.1} dB loudness {loud:.1} dB");
        }
        // As loud as each other, unless a kit's peaks would pass -3 dBFS first.
        for (name, top, loud) in &report {
            assert!(*top < -2.5, "{name}: peak {top:.1} dB");
            assert!(
                *loud > -27.0 && (*loud < -19.0 || *top > -3.6),
                "{name}: {loud:.1} dB"
            );
        }
    }

    /// Writes each kit playing a rhythm of its style into `KITS_DIR`, to listen to them.
    #[test]
    #[ignore = "writes audio files for listening"]
    fn render_kits_for_listening() {
        use audiostrada_dsp::MidiEvent;
        let Some(dir) = std::env::var_os("KITS_DIR") else {
            return;
        };
        let info = audiostrada_dsp::info("drums").unwrap();
        for kit in factory() {
            let category = kit.name.split(':').next().unwrap_or_default();
            let style = match category.trim() {
                "Trap" => "Trap",
                "Drill" => "Drill",
                "Afro" if kit.name.contains("Amapiano") => "Amapiano",
                "Afro" => "Afrobeats",
                "Latin" if kit.name.contains("Dancehall") => "Dancehall",
                "Latin" => "Reggaeton: dembow",
                "House" if kit.name.contains("garage") => "UK garage",
                "House" | "808" | "909" if !kit.name.contains("Hard") => "House",
                "Techno" | "909" => "Techno",
                "Hip hop" if kit.name.contains("Phonk") => "Phonk",
                "Hip hop" | "Lo-fi" => "Hip hop: boom bap",
                "Drum and bass" => "Drum and bass",
                _ => "Electro: 808 funk",
            };
            let rhythm = RHYTHMS.iter().find(|r| r.name == style).unwrap();
            let beat = 48_000.0 * 60.0 / f64::from(rhythm.bpm);
            let mut notes = rhythm_notes(rhythm, 2, beat);
            // A fill on the toms and a crash at the end, to hear the whole kit.
            let end = (8.0 * beat) as u64;
            for (k, key) in [50u8, 50, 47, 47, 45, 45, 43, 43].into_iter().enumerate() {
                notes.push(Note {
                    start: end + (k as f64 * beat / 4.0) as u64,
                    length: 100,
                    key,
                    velocity: 110,
                    channel: 9,
                    ..Default::default()
                });
            }
            notes.push(Note {
                start: end + (2.0 * beat) as u64,
                length: 100,
                key: 49,
                velocity: 120,
                channel: 9,
                ..Default::default()
            });
            notes.push(Note {
                start: end + (2.0 * beat) as u64,
                length: 100,
                key: 36,
                velocity: 120,
                channel: 9,
                ..Default::default()
            });
            notes.sort_by_key(|n| n.start);
            let mut drums = audiostrada_dsp::create("drums").unwrap();
            for (i, p) in info.params.iter().enumerate() {
                drums.set_param(i, kit.params.get(p.id).copied().unwrap_or(p.default));
            }
            drums.set_tempo(f64::from(rhythm.bpm));
            drums.prepare(48_000, 64);
            let total = end as usize + (4.0 * beat) as usize;
            let mut pcm = Vec::with_capacity(total * 4);
            let mut next = 0;
            for at in (0..total).step_by(64) {
                while next < notes.len() && notes[next].start < (at + 64) as u64 {
                    let n = &notes[next];
                    drums.midi(&MidiEvent::note_on(0, 9, n.key, n.velocity));
                    next += 1;
                }
                let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
                drums.process(&mut l, &mut r);
                for (a, b) in l.iter().zip(&r) {
                    for s in [a, b] {
                        pcm.extend_from_slice(
                            &((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes(),
                        );
                    }
                }
            }
            let mut wav = Vec::new();
            let size = pcm.len() as u32;
            for chunk in [
                &b"RIFF"[..],
                &(36 + size).to_le_bytes(),
                b"WAVEfmt ",
                &16u32.to_le_bytes(),
                &1u16.to_le_bytes(),
                &2u16.to_le_bytes(),
                &48_000u32.to_le_bytes(),
                &(48_000u32 * 4).to_le_bytes(),
                &4u16.to_le_bytes(),
                &16u16.to_le_bytes(),
                b"data",
                &size.to_le_bytes(),
            ] {
                wav.extend_from_slice(chunk);
            }
            wav.extend(pcm);
            let file = kit.name.replace([':', ' '], "_").replace("__", "_");
            std::fs::write(std::path::Path::new(&dir).join(format!("{file}.wav")), wav).unwrap();
        }
    }

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
}
