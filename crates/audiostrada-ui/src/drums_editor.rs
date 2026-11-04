//! The drum machine's editor: small pads lit while their voice sounds, played with the mouse
//! and selected to show their settings; the kit's accent, drive and volume; and ready-made
//! rhythms written into a MIDI clip on the track, to edit in the piano roll.

use audiostrada_core::Note;
use audiostrada_core::routing::Channel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use audiostrada_dsp::drums::{LAYOUT, VOICES};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::effect_editors::{Params, named_knob, param_knob, segmented};
use crate::i18n::tr;
use crate::theme;

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

/// The dark screen behind the pads and the pad's editor.
const SCREEN: Color32 = Color32::from_rgb(16, 19, 25);

/// The color of a pad's family: kicks, snares and claps, hats, toms, cymbals, percussion.
fn family(pad: usize) -> Color32 {
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
fn waveform(painter: &egui::Painter, rect: Rect, peaks: &[f32], color: Color32) {
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
    let p = theme::current(ui);
    let mut pressed = None;
    // The pads, four by four on the dark screen, each with its sound drawn.
    egui::Frame::new()
        .fill(SCREEN)
        .corner_radius(10.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            let gap = 8.0;
            let width = ((ui.available_width() - 3.0 * gap) / 4.0).floor();
            let height = 74.0;
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
    editor(ui, params, state.selected.min(VOICES.len() - 1), meters);
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        panel(ui, tr("Kit"), |ui| {
            ui.horizontal(|ui| {
                for id in ["accent", "drive", "room", "crush", "volume"] {
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

/// The pad being edited: its sound large, its character or its file, and its settings.
fn editor(ui: &mut Ui, params: &mut Params, pad: usize, meters: &[f32]) {
    let (id, name, _) = VOICES[pad];
    let color = family(pad);
    let file = pad_file(params, pad);
    egui::Frame::new()
        .fill(SCREEN)
        .corner_radius(10.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            let selection = ui.visuals().selection;
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().selection = selection;
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                // The sound, wide, under the pad's name.
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr(name)).size(20.0).strong().color(color));
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
                    let width = (ui.available_width() - 330.0).max(260.0);
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 96.0), Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 8.0, Color32::from_rgb(24, 28, 36));
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
                        let (doc, channel, insert) =
                            (&mut *params.doc, params.channel, params.insert);
                        crate::mixer::load_button(ui, doc, channel, insert, slot);
                        if file.is_some() && ui.button(tr("Remove")).clicked() {
                            let _ = params.doc.set_effect_file(
                                params.channel,
                                params.insert,
                                slot.id,
                                None,
                            );
                        }
                    });
                });
                ui.add_space(12.0);
                ui.horizontal_centered(|ui| {
                    for (field, label) in [
                        ("level", tr("Level")),
                        ("pan", tr("Pan")),
                        ("tune", tr("Tune")),
                        ("decay", tr("Decay")),
                        ("tone", tr("Tone")),
                    ] {
                        named_knob(ui, params, &format!("{id}_{field}"), label, 44.0, color);
                    }
                });
            });
        });
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
                ui.label(RichText::new(title).size(11.5).color(p.weak).strong());
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
