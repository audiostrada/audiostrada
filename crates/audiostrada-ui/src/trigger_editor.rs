//! Strada-Trigger's editor: the drum and its sound as cards, the track's hits drawn live under
//! a threshold to drag, then the detection and sound knobs, and two faders: close mics to
//! room, and the track to the trigger.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use audiostrada_dsp::trigger::{self, BUILT_IN, KICKS, SLOTS, SNARES};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};

use crate::document::Document;
use crate::drums_editor::{MODULE, family, modules, screen, waveform};
use crate::effect_editors::Params;
use crate::i18n::tr;
use crate::theme;
use audiostrada_core::TrackId;
use audiostrada_core::routing::Channel;
use std::collections::BTreeMap;

/// What the editor remembers between frames: the detection lately and the hits found.
#[derive(Debug, Default)]
pub struct TriggerState {
    /// Time, detection and input level (linear), oldest first.
    levels: VecDeque<(f64, f32, f32)>,
    /// Time and strength (0 to 1) of each hit.
    hits: VecDeque<(f64, f32)>,
    count: Option<f32>,
    /// The sounds of one's own, read once from the settings.
    own: Option<Vec<OwnSound>>,
    /// The sound being added, in its dialog.
    draft: Option<Draft>,
    /// What the last tuning found, for which sound, and the tune it set.
    note: Option<String>,
    matched: Option<(Tuned, f32)>,
}

/// The drum, the sound and its file a tuning was made for.
type Tuned = (usize, usize, Option<PathBuf>);

/// A sound of one's own: its name, its file and its drum (0 kick, 1 snare).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct OwnSound {
    name: String,
    path: PathBuf,
    kind: usize,
}

#[derive(Debug, Default)]
struct Draft {
    name: String,
    path: Option<PathBuf>,
    kind: usize,
}

/// Where the sounds of one's own are listed, for every project.
fn own_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::config_dir().map(|d| d.join("audiostrada").join("trigger_sounds.json"))
}

fn read_own() -> Vec<OwnSound> {
    own_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_own(list: &[OwnSound]) {
    let Some(file) = own_file() else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(file, text);
    }
}

/// Seconds of history drawn.
const SPAN: f64 = 4.0;
/// The display's range, in dB.
const FLOOR: f32 = -60.0;
const INK: Color32 = Color32::from_rgb(236, 240, 246);

/// A sound's short description, shown under its name.
fn subtitle(kind: usize, sound: usize) -> &'static str {
    let list: &[&str] = if kind == 0 {
        &[
            "20-inch, clean",
            "20-inch, rough",
            "24-inch, damped",
            "24-inch, open and long",
            "Vintage, stereo mix",
            "Two mics and room",
        ]
    } else {
        &[
            "13-inch, center",
            "14-inch, center",
            "14-inch, rimshot",
            "Short and dry",
            "Long ring",
            "Top, bottom and room",
            "Top, bottom and room",
        ]
    };
    list.get(sound).copied().unwrap_or("")
}

/// Where a sound comes from, for its tooltip and the credits.
fn source(kind: usize, sound: usize) -> &'static str {
    match (kind, sound) {
        (0, 0..=1) | (1, 0..=1) => "Karoryfer Samples, Unruly Drums (CC0)",
        (0, 2..=3) | (1, 2) => "Karoryfer Samples, Big Rusty Drums (CC0)",
        (0, 4) | (1, 3) => "SM Drums (public domain)",
        (1, 4) => "Karoryfer Samples, Swirly Drums (CC0)",
        (0, 5) | (1, 5) => "DrumGizmo, MuldjordKit (CC BY 4.0)",
        _ => "DrumGizmo, DRSKit (CC BY 4.0)",
    }
}

fn tint(kind: usize) -> Color32 {
    family(if kind == 0 { 0 } else { 2 })
}

fn db(linear: f32) -> f32 {
    20.0 * linear.max(1e-6).log10()
}

pub fn show(ui: &mut Ui, params: &mut Params, meters: &[f32], state: &mut TriggerState) {
    let kind = (params.get("kind").round() as usize).min(1);
    let color = tint(kind);
    record(ui, meters, state);
    screen(ui, |ui| {
        kind_switch(ui, params, kind);
        ui.add_space(10.0);
        cards(ui, params, kind, state);
        ui.add_space(12.0);
        display(ui, params, state, color);
        ui.add_space(12.0);
        follow_track(ui, params, kind, state);
        let knob = |id: &str, label: &'static str| (id.to_owned(), tr(label));
        let places = modules(
            ui,
            params,
            &[
                (
                    tr("Detection"),
                    vec![
                        knob("threshold", "Threshold"),
                        knob("tolerance", "Tolerance"),
                        knob("dynamics", "Dynamics"),
                    ],
                ),
                (
                    tr("Sound"),
                    vec![
                        knob("tune", "Tune"),
                        knob("match", "On the track"),
                        knob("decay", "Decay"),
                        knob("level", "Level"),
                    ],
                ),
            ],
            color,
            46.0,
        );
        // What the tuning found, small under its switch, in the room the knobs leave there.
        if params.get("match") >= 0.5
            && let (Some(note), Some(place)) = (&state.note, places.get(4))
        {
            let galley = ui.painter().layout(
                note.clone(),
                FontId::proportional(10.0),
                INK.gamma_multiply(0.6),
                place.width() - 4.0,
            );
            let at = pos2(
                place.center().x - galley.size().x / 2.0,
                place.bottom() + 4.0,
            );
            ui.painter().galley(at, galley, INK);
        }
        ui.add_space(8.0);
        modules(
            ui,
            params,
            &[
                (tr("Compression"), vec![knob("comp", "Amount")]),
                (
                    tr("EQ"),
                    vec![
                        knob("low", "Bass"),
                        knob("mid", "Mid range"),
                        knob("high", "Treble"),
                    ],
                ),
                (tr("Drive"), vec![knob("drive", "Amount")]),
            ],
            color,
            46.0,
        );
        ui.add_space(14.0);
        fader(
            ui,
            params,
            "room",
            (tr("Close mics"), tr("Room mics")),
            color,
            30.0,
        );
        ui.add_space(12.0);
        fader(
            ui,
            params,
            "amount",
            (tr("Track"), tr("Triggered sound")),
            color,
            44.0,
        );
    });
    add_dialog(ui, params, state);
    ui.ctx().request_repaint();
}

/// While the switch is on, the sound stays tuned to the track's drum, again whenever the
/// sound changes; turning the tune by hand lets it go.
fn follow_track(ui: &Ui, params: &mut Params, kind: usize, state: &mut TriggerState) {
    if params.get("match") < 0.5 {
        state.matched = None;
        return;
    }
    let id = if kind == 0 { "kick" } else { "snare" };
    let file =
        audiostrada_core::routing::find_insert(params.doc.project(), params.channel, params.insert)
            .and_then(|i| i.files.get(SLOTS[kind].id).cloned());
    let key = (kind, params.get(id).round() as usize, file);
    let response = ui.interact(Rect::NOTHING, ui.scope_id().with("match"), Sense::hover());
    match &state.matched {
        Some((done, tune)) if *done == key => {
            if (params.get("tune") - tune).abs() > 0.05 {
                params.set("match", Some(0.0), &response);
                params.doc.history_mut().end_gesture();
                state.matched = None;
            }
        }
        _ => {
            let tune = match match_pitch(params, kind) {
                Ok((semitones, note)) => {
                    params.set("tune", Some(semitones), &response);
                    params.doc.history_mut().end_gesture();
                    state.note = Some(note);
                    semitones
                }
                Err(note) => {
                    state.note = Some(note);
                    params.get("tune")
                }
            };
            state.matched = Some((key, tune));
        }
    }
}

/// The track's audio where its clips play, mono, up to `seconds`, at the files' rate.
fn track_audio(doc: &Document, track: TrackId, seconds: f32) -> Option<(Vec<f32>, u32)> {
    let project = doc.project();
    let track = project.track(track)?;
    let mut out = Vec::new();
    let mut rate = None;
    for clip in track.clips.iter().filter(|c| !c.muted && c.notes.is_none()) {
        let Some(path) = audiostrada_engine::project::clip_media(project, doc.paths(), clip) else {
            continue;
        };
        let Some(audio) = doc.media().audio(&path) else {
            continue;
        };
        let file_rate = audio.sample_rate();
        if *rate.get_or_insert(file_rate) != file_rate {
            continue;
        }
        let ratio = f64::from(file_rate) / f64::from(project.sample_rate.max(1));
        let start = ((clip.offset as f64 * ratio) as usize).min(audio.frames());
        let end = (start + (clip.length as f64 * ratio) as usize).min(audio.frames());
        let channels = audio.channels().max(1);
        out.extend(
            (start..end)
                .map(|i| (0..channels).map(|c| audio.channel(c)[i]).sum::<f32>() / channels as f32),
        );
        if out.len() as f32 > seconds * file_rate as f32 {
            break;
        }
    }
    Some((out, rate?))
}

/// The drum a track's name says, when it says one.
fn kind_from_name(name: &str) -> Option<usize> {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |list: &[&str]| words.iter().any(|w| list.contains(w));
    let kick = lower.contains("kick")
        || lower.contains("grosse")
        || has(&["kik", "kck", "bd", "bassdrum", "gc", "kd"]);
    let snare =
        lower.contains("snare") || lower.contains("claire") || has(&["sn", "snr", "sd", "cc"]);
    match (kick, snare) {
        (true, false) => Some(0),
        (false, true) => Some(1),
        _ => None,
    }
}

fn guess_from_audio(doc: &Document, track: TrackId) -> Option<usize> {
    let (audio, rate) = track_audio(doc, track, 30.0)?;
    trigger::guess_kind(&audio, rate)
}

/// The settings of a trigger put on a track: its drum, from the track's name or its sound.
pub(crate) fn first_settings(doc: &Document, channel: Channel) -> BTreeMap<String, f32> {
    let Channel::Track(t) = channel else {
        return BTreeMap::new();
    };
    let named = doc.project().track(t).and_then(|t| kind_from_name(&t.name));
    named
        .or_else(|| guess_from_audio(doc, t))
        .map(|k| BTreeMap::from([("kind".to_owned(), k as f32)]))
        .unwrap_or_default()
}

/// Tunes the sound to the pitch the track's drum rings at, by the nearest octave.
fn match_pitch(params: &Params, kind: usize) -> Result<(f32, String), String> {
    let Channel::Track(t) = params.channel else {
        return Err(tr("Only on a track").to_owned());
    };
    let (audio, rate) =
        track_audio(params.doc, t, 30.0).ok_or_else(|| tr("No audio on this track yet"))?;
    let target = trigger::fundamental(&audio, rate, kind)
        .ok_or_else(|| tr("No clear pitch on the track"))?;
    let id = if kind == 0 { "kick" } else { "snare" };
    let sound = params.get(id).round() as usize;
    let own = if sound < BUILT_IN[kind] {
        trigger::sound_fundamental(kind, sound)
    } else {
        let slot = SLOTS[kind].id;
        audiostrada_core::routing::find_insert(params.doc.project(), params.channel, params.insert)
            .and_then(|i| i.files.get(slot).cloned())
            .and_then(|file| audiostrada_media::load(&params.doc.resolve(&file)).ok())
            .and_then(|a| trigger::fundamental(a.channel(0), a.sample_rate(), kind))
    }
    .ok_or_else(|| tr("No clear pitch in the sound"))?;
    let mut semitones = 12.0 * (target / own).log2();
    // Drums sound right a few semitones away, not an octave: the nearest one is kept.
    while semitones > 6.0 {
        semitones -= 12.0;
    }
    while semitones < -6.0 {
        semitones += 12.0;
    }
    let semitones = (semitones * 10.0).round() / 10.0;
    let note = crate::trf!(
        "Track {track} Hz, sound {sound} Hz: {shift} st",
        track = format!("{target:.0}"),
        sound = format!("{own:.0}"),
        shift = format!("{semitones:+.1}")
    );
    Ok((semitones, note))
}

/// Keeps the levels and the hits of the last seconds.
fn record(ui: &Ui, meters: &[f32], state: &mut TriggerState) {
    let now = ui.input(|i| i.time);
    let [input, detection, count, velocity] = [0, 1, 2, 3].map(|k| meters.get(k).copied());
    if let (Some(input), Some(detection)) = (input, detection) {
        state.levels.push_back((now, detection, input));
    }
    if let (Some(count), Some(velocity)) = (count, velocity) {
        if state.count.is_some_and(|c| c != count) {
            state.hits.push_back((now, velocity));
        }
        state.count = Some(count);
    }
    while state.levels.front().is_some_and(|l| now - l.0 > SPAN + 0.2) {
        state.levels.pop_front();
    }
    while state.hits.front().is_some_and(|h| now - h.0 > SPAN + 0.2) {
        state.hits.pop_front();
    }
}

/// Kick or snare, as two wide pills, with what the delay is.
fn kind_switch(ui: &mut Ui, params: &mut Params, kind: usize) {
    ui.horizontal(|ui| {
        for (k, label) in [(0, tr("Kick")), (1, tr("Snare"))] {
            let on = kind == k;
            let (rect, response) = ui.allocate_exact_size(vec2(150.0, 32.0), Sense::click());
            let color = tint(k);
            let fill = if on {
                color.gamma_multiply(0.85)
            } else if response.hovered() {
                theme::mix(MODULE, color, 0.25)
            } else {
                MODULE
            };
            ui.painter().rect_filled(rect, 16.0, fill);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                theme::strong(14.0),
                if on {
                    Color32::from_rgb(20, 16, 14)
                } else {
                    INK
                },
            );
            let changed = (response.clicked() && !on).then_some(k as f32);
            params.set("kind", changed, &response);
            if changed.is_some() {
                params.doc.history_mut().end_gesture();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(tr(
                    "Plays 20 ms ahead: the mix delays the rest to stay in time",
                ))
                .size(11.0)
                .color(INK.gamma_multiply(0.45)),
            );
        });
    });
}

/// What a card stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Card {
    BuiltIn(usize),
    /// A sound of one's own, by its place in the list.
    Own(usize),
    /// The file the project plays, not in one's list.
    Loaded,
    Add,
}

/// The sounds of the drum as cards across the width, then one's own and a card to add one:
/// click one to choose it and hear it.
fn cards(ui: &mut Ui, params: &mut Params, kind: usize, state: &mut TriggerState) {
    let (id, names): (&str, &[&str]) = if kind == 0 {
        ("kick", &KICKS)
    } else {
        ("snare", &SNARES)
    };
    let own = state.own.get_or_insert_with(read_own).clone();
    let chosen = params.get(id).round() as usize;
    let slot = SLOTS[kind].id;
    let loaded =
        audiostrada_core::routing::find_insert(params.doc.project(), params.channel, params.insert)
            .and_then(|i| i.files.get(slot).cloned());
    let loaded_name = loaded
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_os_string());
    let is_loaded = |o: &OwnSound| o.path.file_name().map(|n| n.to_os_string()) == loaded_name;
    let mut list: Vec<Card> = (0..BUILT_IN[kind]).map(Card::BuiltIn).collect();
    list.extend(
        own.iter()
            .enumerate()
            .filter(|(_, o)| o.kind == kind)
            .map(|(k, _)| Card::Own(k)),
    );
    if loaded.is_some() && !own.iter().any(|o| o.kind == kind && is_loaded(o)) {
        list.push(Card::Loaded);
    }
    list.push(Card::Add);
    let color = tint(kind);
    let gap = 8.0;
    let per_row = list.len().min(8);
    let width = ((ui.available_width() - gap * (per_row - 1) as f32) / per_row as f32).floor();
    let mut remove = None;
    for row in list.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &card in row {
                let (rect, response) = ui.allocate_exact_size(vec2(width, 78.0), Sense::click());
                let on = match card {
                    Card::BuiltIn(k) => k == chosen,
                    Card::Own(k) => chosen == BUILT_IN[kind] && is_loaded(&own[k]),
                    Card::Loaded => chosen == BUILT_IN[kind],
                    Card::Add => false,
                };
                let painter = ui.painter_at(rect.expand(3.0));
                let strength = if on {
                    0.32
                } else if response.hovered() {
                    0.16
                } else {
                    0.06
                };
                if card == Card::Add {
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        8.0,
                        Stroke::new(1.5, INK.gamma_multiply(0.25 + strength)),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        rect.center() - vec2(0.0, 8.0),
                        Align2::CENTER_CENTER,
                        "+",
                        FontId::proportional(24.0),
                        INK.gamma_multiply(0.7),
                    );
                    painter.text(
                        rect.center() + vec2(0.0, 16.0),
                        Align2::CENTER_CENTER,
                        tr("Your sound"),
                        FontId::proportional(11.0),
                        INK.gamma_multiply(0.6),
                    );
                    let response = response.on_hover_text(tr(
                        "Add a sound of your own: a file with one hit, named as you like",
                    ));
                    if response.clicked() {
                        state.draft = Some(Draft {
                            kind,
                            ..Draft::default()
                        });
                    }
                    continue;
                }
                painter.rect_filled(rect, 8.0, theme::mix(MODULE, color, strength));
                if on {
                    painter.rect_stroke(
                        rect,
                        8.0,
                        Stroke::new(2.0, color),
                        egui::StrokeKind::Outside,
                    );
                }
                let (name, subtitle, shape) = match card {
                    Card::BuiltIn(k) => (
                        tr(names[k]).to_owned(),
                        tr(subtitle(kind, k)),
                        Arc::new(trigger::shape(kind, k, 80)),
                    ),
                    Card::Own(k) => (
                        own[k].name.clone(),
                        tr("Your sound"),
                        file_shape(&own[k].path),
                    ),
                    _ => (
                        loaded
                            .as_ref()
                            .and_then(|p| p.file_stem())
                            .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
                        tr("File of this project"),
                        file_shape(&project_path(params, loaded.as_deref())),
                    ),
                };
                painter.text(
                    rect.left_top() + vec2(9.0, 8.0),
                    Align2::LEFT_TOP,
                    name,
                    theme::strong(13.0),
                    INK,
                );
                painter.text(
                    rect.left_top() + vec2(9.0, 26.0),
                    Align2::LEFT_TOP,
                    subtitle,
                    FontId::proportional(10.5),
                    INK.gamma_multiply(0.6),
                );
                let wave = Rect::from_min_max(
                    rect.left_top() + vec2(9.0, 44.0),
                    rect.right_bottom() - vec2(9.0, 7.0),
                );
                waveform(
                    &painter,
                    wave,
                    &shape,
                    if on { color } else { INK.gamma_multiply(0.55) },
                );
                let hint = match card {
                    Card::BuiltIn(k) => {
                        format!(
                            "{}\n{}",
                            tr("Click to choose it and hear it"),
                            source(kind, k)
                        )
                    }
                    Card::Own(k) => format!(
                        "{}\n{}",
                        tr("Click to choose it and hear it; right-click to take it off the list"),
                        own[k].path.display()
                    ),
                    _ => tr("Click to choose it and hear it").to_owned(),
                };
                let response = response.on_hover_text(hint);
                if let Card::Own(k) = card {
                    response.context_menu(|ui| {
                        if ui.button(tr("Take off the list")).clicked() {
                            remove = Some(k);
                            ui.close();
                        }
                    });
                }
                let value = match card {
                    Card::BuiltIn(k) => k,
                    _ => BUILT_IN[kind],
                };
                let changed = (response.clicked() && !on).then_some(value as f32);
                if changed.is_some()
                    && let Card::Own(k) = card
                    && let Err(e) = params.doc.set_effect_file(
                        params.channel,
                        params.insert,
                        slot,
                        Some(&own[k].path),
                    )
                {
                    params.doc.notify(e);
                }
                params.set(id, changed, &response);
                if changed.is_some() {
                    params.doc.history_mut().end_gesture();
                }
                if response.clicked() {
                    match card {
                        Card::BuiltIn(k) => audition(params, kind, k),
                        Card::Own(k) => play_file(params, &own[k].path),
                        _ => {
                            let path = project_path(params, loaded.as_deref());
                            play_file(params, &path);
                        }
                    }
                }
            }
        });
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }
    if let Some(k) = remove
        && let Some(list) = &mut state.own
    {
        list.remove(k);
        write_own(list);
    }
}

/// A file the project keeps, as a path to read.
fn project_path(params: &Params, file: Option<&Path>) -> PathBuf {
    let Some(file) = file else {
        return PathBuf::new();
    };
    params.doc.resolve(file)
}

/// Peaks of a file's first channel, for its card, read once.
fn file_shape(path: &Path) -> Arc<Vec<f32>> {
    static CACHE: Mutex<Option<HashMap<PathBuf, Arc<Vec<f32>>>>> = Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return Arc::default();
    };
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(peaks) = cache.get(path) {
        return Arc::clone(peaks);
    }
    let samples = audiostrada_media::load(path)
        .map(|a| a.channel(0).to_vec())
        .unwrap_or_default();
    let top = samples.iter().fold(1e-6f32, |m, v| m.max(v.abs()));
    let end = samples
        .iter()
        .rposition(|v| v.abs() > top * 2e-3)
        .map_or(0, |i| i + 1);
    let per = (end / 80).max(1);
    let peaks: Vec<f32> = samples[..end]
        .chunks(per)
        .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())) / top)
        .collect();
    let peaks = Arc::new(peaks);
    cache.insert(path.to_path_buf(), Arc::clone(&peaks));
    peaks
}

/// Plays a file as it is, at the engine's rate.
fn play_file(params: &mut Params, path: &Path) {
    let Ok(audio) = audiostrada_media::load(path) else {
        return;
    };
    let rate = params.doc.player().engine().sample_rate();
    let step = f64::from(audio.sample_rate()) / f64::from(rate);
    let channels = (0..audio.channels().min(2))
        .map(|c| {
            let x = audio.channel(c);
            let n = (x.len() as f64 / step) as usize;
            (0..n)
                .map(|k| {
                    let at = k as f64 * step;
                    let i = at as usize;
                    let frac = (at - i as f64) as f32;
                    let a = x[i];
                    a + (x.get(i + 1).copied().unwrap_or(a) - a) * frac
                })
                .collect()
        })
        .collect();
    let data = audiostrada_media::AudioData::new(channels, rate);
    params.doc.player_mut().preview(Some(Arc::new(data)));
}

/// The dialog adding a sound of one's own: its name, its drum and its file, heard before
/// it is added.
fn add_dialog(ui: &Ui, params: &mut Params, state: &mut TriggerState) {
    let Some(draft) = &mut state.draft else {
        return;
    };
    let (mut cancel, mut add) = (false, false);
    egui::Modal::new(egui::Id::unique("trigger-add-sound")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(tr("Add a sound")).strong().size(16.0));
        ui.add_space(8.0);
        egui::Grid::new("trigger-add-grid")
            .num_columns(2)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                ui.label(tr("Name"));
                ui.add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .hint_text(tr("My kick"))
                        .desired_width(260.0),
                );
                ui.end_row();
                ui.label(tr("Drum"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut draft.kind, 0, tr("Kick"));
                    ui.selectable_value(&mut draft.kind, 1, tr("Snare"));
                });
                ui.end_row();
                ui.label(tr("File"));
                ui.horizontal(|ui| {
                    let open = ui.button(tr("Choose a file…")).clicked();
                    if let Some(file) =
                        crate::dialogs::ask(ui.ctx(), "trigger-own-file", open, || {
                            rfd::FileDialog::new()
                                .add_filter(tr("Audio"), SLOTS[0].extensions)
                                .pick_file()
                        })
                    {
                        if draft.name.trim().is_empty()
                            && let Some(stem) = file.file_stem()
                        {
                            draft.name = stem.to_string_lossy().into_owned();
                        }
                        draft.path = Some(file);
                    }
                    match &draft.path {
                        Some(path) => {
                            let name = path
                                .file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            ui.label(name).on_hover_text(path.display().to_string());
                            if ui.button(tr("Listen")).clicked() {
                                play_file(params, path);
                            }
                        }
                        None => {
                            ui.weak(tr("No file yet"));
                        }
                    }
                });
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.weak(tr(
            "One hit per file. The silence before it is taken off and its level follows the \
             track's hits. WAV, FLAC, AIFF, OGG or MP3.",
        ));
        theme::footer(ui, |ui| {
            let ready = draft.path.is_some() && !draft.name.trim().is_empty();
            if theme::primary_button_enabled(ui, ready, tr("Add")).clicked() {
                add = true;
            }
            if ui.button(tr("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.draft = None;
        return;
    }
    if !add {
        return;
    }
    let Some(draft) = state.draft.take() else {
        return;
    };
    let Some(path) = draft.path else {
        return;
    };
    let kind = draft.kind;
    let list = state.own.get_or_insert_with(read_own);
    list.push(OwnSound {
        name: draft.name.trim().to_owned(),
        path: path.clone(),
        kind,
    });
    write_own(list);
    if let Err(e) =
        params
            .doc
            .set_effect_file(params.channel, params.insert, SLOTS[kind].id, Some(&path))
    {
        params.doc.notify(e);
        return;
    }
    let response = ui.interact(
        Rect::NOTHING,
        ui.scope_id().with("trigger-add"),
        Sense::hover(),
    );
    params.set("kind", Some(kind as f32), &response);
    params.set(
        if kind == 0 { "kick" } else { "snare" },
        Some(BUILT_IN[kind] as f32),
        &response,
    );
    params.doc.history_mut().end_gesture();
}

/// Plays a hard hit of a sound, close to room as set.
fn audition(params: &mut Params, kind: usize, sound: usize) {
    let rate = params.doc.player().engine().sample_rate();
    let room = params.get("room") / 100.0;
    let hit = trigger::audition(kind, sound, 0.9, room, rate);
    let data = audiostrada_media::AudioData::new(vec![hit.clone(), hit], rate);
    params.doc.player_mut().preview(Some(Arc::new(data)));
}

/// The detection of the last seconds, the input behind it, the threshold as a line to drag,
/// and a mark at each hit as strong as it was.
fn display(ui: &mut Ui, params: &mut Params, state: &TriggerState, color: Color32) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 140.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, MODULE);
    let plot = rect.shrink2(vec2(10.0, 10.0));
    let y_of = |db: f32| plot.bottom() - (db - FLOOR) / -FLOOR * plot.height();
    for line in [-48.0, -36.0, -24.0, -12.0, 0.0] {
        let y = y_of(line);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, INK.gamma_multiply(0.06)),
        );
        painter.text(
            pos2(plot.left() + 2.0, y - 1.0),
            Align2::LEFT_BOTTOM,
            format!("{line:.0}"),
            FontId::monospace(9.0),
            INK.gamma_multiply(0.3),
        );
    }
    let now = ui.input(|i| i.time);
    let x_of = |t: f64| plot.right() - ((now - t) / SPAN) as f32 * plot.width();
    let threshold = params.get("threshold");
    // The input, faint, then the detection, filled, brighter over the threshold.
    let curve = |pick: fn(&(f64, f32, f32)) -> f32| -> Vec<Pos2> {
        state
            .levels
            .iter()
            .filter(|l| now - l.0 <= SPAN)
            .map(|l| pos2(x_of(l.0), y_of(db(pick(l)).max(FLOOR - 2.0))))
            .collect()
    };
    let input = curve(|l| l.2);
    let detection = curve(|l| l.1);
    fill(&painter, &input, plot.bottom(), INK.gamma_multiply(0.07));
    fill(
        &painter,
        &detection,
        plot.bottom(),
        color.gamma_multiply(0.28),
    );
    let heard = state.levels.iter().any(|l| l.1 > 1e-3 || l.2 > 1e-3);
    if heard && detection.len() > 1 {
        painter.add(Shape::line(
            detection.clone(),
            Stroke::new(1.5, color.gamma_multiply(0.9)),
        ));
    }
    // Each hit: a line from the bottom and a dot as high as it was strong.
    for &(t, strength) in &state.hits {
        if now - t > SPAN {
            continue;
        }
        let x = x_of(t);
        let age = ((now - t) / 0.25).min(1.0) as f32;
        let top = plot.bottom() - (0.2 + 0.8 * strength) * plot.height();
        painter.line_segment(
            [pos2(x, plot.bottom()), pos2(x, top)],
            Stroke::new(2.0, color.gamma_multiply(0.75)),
        );
        painter.circle_filled(pos2(x, top), 4.0 + 5.0 * (1.0 - age), color);
    }
    // The light of the last hit.
    if let Some(&(t, _)) = state.hits.back() {
        let glow = (1.0 - (now - t) / 0.3).max(0.0) as f32;
        if glow > 0.0 {
            painter.circle_filled(
                rect.right_top() + vec2(-18.0, 18.0),
                7.0,
                color.gamma_multiply(glow),
            );
        }
    }
    let y = y_of(threshold);
    let dashes = Shape::dashed_line(
        &[pos2(plot.left(), y), pos2(plot.right(), y)],
        Stroke::new(1.5, INK.gamma_multiply(0.85)),
        6.0,
        4.0,
    );
    painter.extend(dashes);
    painter.text(
        pos2(plot.right() - 30.0, y - 3.0),
        Align2::RIGHT_BOTTOM,
        format!("{} {threshold:.0} dB", tr("Threshold")),
        FontId::proportional(11.0),
        INK,
    );
    if state.levels.iter().all(|l| l.1 < 1e-3 && l.2 < 1e-3) {
        painter.text(
            pos2(rect.center().x, y_of(-50.0)),
            Align2::CENTER_CENTER,
            tr("Play the track: its hits show here, each one found is marked"),
            FontId::proportional(12.5),
            INK.gamma_multiply(0.45),
        );
    }
    let response = response.on_hover_text(tr(
        "Drag up or down to set the threshold: hits over the line play the sample",
    ));
    let changed = (response.dragged() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|pos| (FLOOR * (1.0 - (plot.bottom() - pos.y) / plot.height())).clamp(FLOOR, 0.0))
        .map(|v| v.round());
    params.set("threshold", changed, &response);
}

/// A curve filled down to `bottom`.
fn fill(painter: &egui::Painter, points: &[Pos2], bottom: f32, color: Color32) {
    if points.len() < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for p in points {
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(pos2(p.x, bottom), color);
        if i >= 2 {
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A wide fader between two ends, each end's share shown at its side.
fn fader(
    ui: &mut Ui,
    params: &mut Params,
    id: &str,
    (left, right): (&str, &str),
    color: Color32,
    height: f32,
) {
    let Some(param) = params.param(id) else {
        return;
    };
    let value = params.get(id);
    let t = (value - param.min) / (param.max - param.min);
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), height + 18.0),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect.expand(2.0));
    let labels = Rect::from_min_size(rect.min, vec2(rect.width(), 16.0));
    // The share of each end: both whole at the middle for the trigger, a crossfade for the room.
    let (a, b) = if id == "amount" {
        ((2.0 * (1.0 - t)).min(1.0), (2.0 * t).min(1.0))
    } else {
        (1.0 - t, t)
    };
    painter.text(
        labels.left_center(),
        Align2::LEFT_CENTER,
        format!("{left}  {:.0} %", a * 100.0),
        theme::strong(12.0),
        INK.gamma_multiply(0.45 + 0.55 * a),
    );
    painter.text(
        labels.right_center(),
        Align2::RIGHT_CENTER,
        format!("{:.0} %  {right}", b * 100.0),
        theme::strong(12.0),
        INK.gamma_multiply(0.45 + 0.55 * b),
    );
    let track = Rect::from_min_max(pos2(rect.left(), rect.top() + 20.0), rect.right_bottom());
    let r = track.height() / 2.0;
    painter.rect_filled(track, r, MODULE);
    let x = track.left() + r + t * (track.width() - 2.0 * r);
    painter.rect_filled(
        Rect::from_min_max(track.min, pos2(x, track.bottom())),
        r,
        color.gamma_multiply(0.35 + 0.4 * t),
    );
    painter.text(
        track.center(),
        Align2::CENTER_CENTER,
        if id == "amount" {
            tr("Blend")
        } else {
            tr("Room")
        },
        FontId::proportional(11.0),
        INK.gamma_multiply(0.35),
    );
    let knob = Rect::from_center_size(
        pos2(x, track.center().y),
        vec2(2.0 * r - 6.0, 2.0 * r - 6.0),
    );
    painter.rect_filled(knob, r, INK);
    painter.rect_stroke(knob, r, Stroke::new(2.0, color), egui::StrokeKind::Inside);
    let response = response.on_hover_text(tr("Drag, or click a point; double-click to reset"));
    let changed = if response.double_clicked() {
        Some(param.default)
    } else if response.dragged() || response.clicked() {
        response.interact_pointer_pos().map(|pos| {
            let t = ((pos.x - track.left() - r) / (track.width() - 2.0 * r)).clamp(0.0, 1.0);
            (param.min + t * (param.max - param.min)).round()
        })
    } else {
        None
    };
    params.set(id, changed, &response);
}

/// Starting points: reinforce or replace, close or roomy.
pub(crate) fn factory() -> Vec<crate::presets::Preset> {
    use crate::presets::preset;
    vec![
        preset(
            tr("Kick: reinforce"),
            &[
                ("kind", 0.0),
                ("kick", 0.0),
                ("amount", 35.0),
                ("room", 15.0),
            ],
        ),
        preset(
            tr("Kick: replace"),
            &[
                ("kind", 0.0),
                ("kick", 2.0),
                ("amount", 100.0),
                ("room", 25.0),
            ],
        ),
        preset(
            tr("Kick: big and open"),
            &[
                ("kind", 0.0),
                ("kick", 3.0),
                ("amount", 60.0),
                ("room", 40.0),
            ],
        ),
        preset(
            tr("Kick: fast double pedal"),
            &[
                ("kind", 0.0),
                ("kick", 5.0),
                ("amount", 70.0),
                ("tolerance", 85.0),
                ("dynamics", 35.0),
                ("room", 10.0),
            ],
        ),
        preset(
            tr("Snare: reinforce"),
            &[
                ("kind", 1.0),
                ("snare", 1.0),
                ("amount", 35.0),
                ("room", 20.0),
            ],
        ),
        preset(
            tr("Snare: replace"),
            &[
                ("kind", 1.0),
                ("snare", 2.0),
                ("amount", 100.0),
                ("room", 30.0),
            ],
        ),
        preset(
            tr("Snare: in the room"),
            &[
                ("kind", 1.0),
                ("snare", 6.0),
                ("amount", 60.0),
                ("room", 75.0),
            ],
        ),
        preset(
            tr("Snare: tight and dry"),
            &[
                ("kind", 1.0),
                ("snare", 3.0),
                ("amount", 50.0),
                ("room", 0.0),
                ("decay", 60.0),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_names_tell_the_drum() {
        for (name, kind) in [
            ("01-KICK-consolidated", Some(0)),
            ("Kik in", Some(0)),
            ("BD", Some(0)),
            ("Grosse caisse", Some(0)),
            ("03-SN-consolidated", Some(1)),
            ("04-SN DO-consolidated", Some(1)),
            ("Snare top", Some(1)),
            ("Caisse claire", Some(1)),
            ("Overheads", None),
            ("Kick and snare", None),
        ] {
            assert_eq!(kind_from_name(name), kind, "{name}");
        }
    }
}
