//! Control by agents and scripts: tools that read and change the open project, a Model Context
//! Protocol server (JSON-RPC over stdio) that offers them, and a local channel to reach a
//! running AudioStrada.
//!
//! Positions and lengths are in seconds, or `{"samples": n}`. Tracks are named by id or name,
//! "master" for the master.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use audiostrada_core::edit::{self, StripMode};
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::tempo::TempoMap;
use audiostrada_core::{ClipId, InsertId, ProjectPaths, TrackId, TrackKind};
use audiostrada_media::FileFormat;
use serde_json::{Value, json};

use crate::document::{AppDirs, Document, ExportSettings, Normalize, Process, StripSettings};
use crate::i18n::tr;
use crate::trf;

/// Longest a headless render or import may take.
const WAIT: Duration = Duration::from_secs(600);

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": properties, "required": required},
    })
}

/// The tools, with their parameters (JSON Schema).
pub fn tool_definitions(live: bool) -> Vec<Value> {
    let track = json!({"description": "track id or name, or \"master\""});
    let time = json!({"description": "seconds, or {\"samples\": n}"});
    let mut tools = vec![
        tool(
            "inspect_project",
            "The project as JSON: tempo, tracks (clips, inserts, sends, level), master, markers.",
            json!({"detail": {"type": "string", "enum": ["summary", "full"]}}),
            &[],
        ),
        tool(
            "list_effects",
            "Built-in effects and instruments with their parameters (id, range, default).",
            json!({}),
            &[],
        ),
        tool(
            "import_audio",
            "Import audio files (WAV, AIFF, FLAC, MP3, OGG, AAC/M4A, ALAC, CAF, audio of MP4/MOV/WebM), each on a new track, or on `track` at `at`.",
            json!({"paths": {"type": "array", "items": {"type": "string"}}, "track": track, "at": time}),
            &["paths"],
        ),
        tool(
            "add_track",
            "Add a track: audio, instrument (MIDI with the built-in synth) or vca.",
            json!({"name": {"type": "string"}, "kind": {"type": "string", "enum": ["audio", "instrument", "vca"]}}),
            &[],
        ),
        tool(
            "set_track",
            "Change a track's name, level (dB), pan (-1 to 1), mute or solo.",
            json!({"track": track, "name": {"type": "string"}, "gain_db": {"type": "number"}, "pan": {"type": "number"}, "mute": {"type": "boolean"}, "solo": {"type": "boolean"}}),
            &["track"],
        ),
        tool(
            "delete_track",
            "Delete a track.",
            json!({"track": track}),
            &["track"],
        ),
        tool(
            "add_insert",
            "Add a built-in effect (see list_effects) at the end of a track's inserts, with optional parameters. Returns its id.",
            json!({"track": track, "effect": {"type": "string"}, "params": {"type": "object"}}),
            &["track", "effect"],
        ),
        tool(
            "set_insert",
            "Change an insert's parameters or bypass it.",
            json!({"track": track, "insert": {"type": "integer"}, "params": {"type": "object"}, "bypass": {"type": "boolean"}}),
            &["track", "insert"],
        ),
        tool(
            "remove_insert",
            "Remove an insert.",
            json!({"track": track, "insert": {"type": "integer"}}),
            &["track", "insert"],
        ),
        tool(
            "add_send",
            "Send a track to another (a bus).",
            json!({"from": track, "to": track}),
            &["from", "to"],
        ),
        tool(
            "set_tempo",
            "Set a constant tempo and time signature.",
            json!({"bpm": {"type": "number"}, "numerator": {"type": "integer"}, "denominator": {"type": "integer"}}),
            &["bpm"],
        ),
        tool(
            "add_marker",
            "Add a marker.",
            json!({"at": time, "name": {"type": "string"}}),
            &["at"],
        ),
        tool(
            "split",
            "Split the clips under a position (on one track, or all).",
            json!({"at": time, "track": track}),
            &["at"],
        ),
        tool(
            "delete_range",
            "Remove what tracks play between two positions; `ripple` closes the gap.",
            json!({"start": time, "end": time, "tracks": {"type": "array"}, "ripple": {"type": "boolean"}}),
            &["start", "end"],
        ),
        tool(
            "strip_silence",
            "Remove (or keep only, or cut at) the silences of a track's clips.",
            json!({"track": track, "threshold_db": {"type": "number"}, "min_silence_ms": {"type": "number"}, "pad_before_ms": {"type": "number"}, "pad_after_ms": {"type": "number"}, "mode": {"type": "string", "enum": ["strip", "extract", "separate"]}}),
            &["track"],
        ),
        tool(
            "heal",
            "Join clips of a track cut from the same audio that are still in place.",
            json!({"track": track, "start": time, "end": time}),
            &["track"],
        ),
        tool(
            "beat_detective",
            "Find the hits of tracks (a drum kit: all together, so they stay in phase), cut there and, with a grid (bar, 1/2, 1/4, 1/8, 1/16, 1/32), move the slices toward it by strength (0 to 1).",
            json!({"tracks": {"type": "array"}, "start": time, "end": time, "sensitivity": {"type": "number"}, "grid": {"type": "string"}, "strength": {"type": "number"}}),
            &["tracks"],
        ),
        tool(
            "consolidate",
            "Render what a track plays between two positions to one new clip.",
            json!({"track": track, "start": time, "end": time}),
            &["track", "start", "end"],
        ),
        tool(
            "process_clips",
            "Render a track's clips through a process: \"normalize\" (to peak_db), \"reverse\", or a built-in effect id with params.",
            json!({"track": track, "process": {"type": "string"}, "params": {"type": "object"}, "peak_db": {"type": "number"}, "tail": {"type": "boolean"}}),
            &["track", "process"],
        ),
        tool(
            "export_mix",
            "Render the mix (and stems) to a file: wav16, wav24, wav32, flac16, flac24, mp3 or opus; optional loudness (LUFS) or peak (dB) normalization.",
            json!({"path": {"type": "string"}, "format": {"type": "string"}, "lufs": {"type": "number"}, "peak_db": {"type": "number"}, "stems": {"type": "boolean"}}),
            &["path"],
        ),
        tool("undo", "Undo the last change.", json!({}), &[]),
        tool("redo", "Redo the last undone change.", json!({}), &[]),
        tool(
            "move_clips",
            "Move clips (audio or MIDI) by `by` seconds (negative goes earlier), or so the earliest starts at `to`, and/or onto another `track`. As on the timeline, a clip moved over another covers it (the part underneath goes; undo brings it back) and clips that come to overlap crossfade.",
            json!({"clips": {"type": "array", "items": {"type": "integer"}}, "by": {"description": "signed seconds, or {\"samples\": n}"}, "to": time, "track": track}),
            &["clips"],
        ),
        tool(
            "trim_clip",
            "Trim a clip: `start` moves its start (its end stays), `end` moves its end, within the material it has.",
            json!({"clip": {"type": "integer"}, "start": time, "end": time}),
            &["clip"],
        ),
        tool(
            "duplicate_clips",
            "Duplicate clips right after themselves, or starting at `at`.",
            json!({"clips": {"type": "array", "items": {"type": "integer"}}, "at": time}),
            &["clips"],
        ),
        tool(
            "delete_clips",
            "Delete clips; `ripple` pulls the later clips of their tracks back by the gap.",
            json!({"clips": {"type": "array", "items": {"type": "integer"}}, "ripple": {"type": "boolean"}}),
            &["clips"],
        ),
        tool(
            "set_clip",
            "Change clips: gain_db, fade_in and fade_out (seconds), fade_shape (equal_power, linear, fast, slow, s_curve), muted, name, stretch_to (new length in seconds, audio stretched without changing its pitch), pitch (semitones, audio).",
            json!({"clips": {"type": "array", "items": {"type": "integer"}}, "gain_db": {"type": "number"}, "fade_in": {"type": "number"}, "fade_out": {"type": "number"}, "fade_shape": {"type": "string", "enum": ["equal_power", "linear", "fast", "slow", "s_curve"]}, "muted": {"type": "boolean"}, "name": {"type": "string"}, "stretch_to": {"type": "number"}, "pitch": {"type": "number"}}),
            &["clips"],
        ),
        tool(
            "set_crossfade",
            "Set the crossfade between two neighbouring clips of a track (`left` then `right`) to `length` seconds; 0 removes it.",
            json!({"left": {"type": "integer"}, "right": {"type": "integer"}, "length": {"type": "number"}}),
            &["left", "right", "length"],
        ),
        tool(
            "read_midi_clip",
            "The notes of a MIDI clip, in beats from the clip's start (the clip's place and length too); on a Strada-Drums track, which key plays which drum. Clip ids are in inspect_project with detail \"full\".",
            json!({"clip": {"type": "integer"}}),
            &["clip"],
        ),
        tool(
            "write_midi_clip",
            "Write the notes of a MIDI clip: `replace` (the default) swaps the notes it shows for these, `add` keeps them. Each note: {beat (from the clip's start), length (beats, default 0.25), key (60 = C4; drums: see read_midi_clip), velocity (1-127, default 100), probability (percent, optional)}, or [beat, length, key, velocity] as read_midi_clip gives. The clip lengthens to its last note. One undo step.",
            json!({"clip": {"type": "integer"}, "notes": {"type": "array"}, "mode": {"type": "string", "enum": ["replace", "add"]}}),
            &["clip", "notes"],
        ),
        tool(
            "add_midi_clip",
            "Add a MIDI clip on a track at `at`, `bars` long (default 4), with notes as in write_midi_clip (optional).",
            json!({"track": track, "at": time, "bars": {"type": "number"}, "notes": {"type": "array"}}),
            &["track", "at"],
        ),
        tool(
            "export_midi_file",
            "Write the notes of a track (or of every track) to a standard MIDI file at `path`.",
            json!({"path": {"type": "string"}, "track": track}),
            &["path"],
        ),
        tool(
            "import_midi_file",
            "Import a standard MIDI file: one instrument track per track of the file, at the playhead.",
            json!({"path": {"type": "string"}}),
            &["path"],
        ),
        tool(
            "analyze_audio",
            "Listen to a track (alone, after its effects) or the master (the mix), rendered offline: loudness (LUFS, true peak, range), dynamics over time, stereo image, share of energy per frequency band, resonances, rhythm, and notes on problems. Use it before and after changing the mix.",
            json!({"track": track, "start": time, "end": time}),
            &[],
        ),
        tool(
            "render_spectrum",
            "A picture of the frequency spectrum (20 Hz to 20 kHz) of up to three tracks or the master, rendered offline, drawn over each other: see which frequencies dominate, where sounds overlap, resonances.",
            json!({"tracks": {"type": "array", "items": track, "maxItems": 3, "description": "default: the master"}, "start": time, "end": time}),
            &[],
        ),
        tool(
            "render_spectrogram",
            "A picture of how the frequencies of a track or the master change over time (lighter is louder).",
            json!({"track": track, "start": time, "end": time}),
            &[],
        ),
        tool(
            "render_waveform",
            "A picture of the waveform of a track or the master (clipping in orange) above its short-term loudness over time.",
            json!({"track": track, "start": time, "end": time}),
            &[],
        ),
        tool(
            "compare_audio",
            "Compare two tracks (or a track and the master), each rendered offline: differences of loudness, peaks and frequency balance, and the frequency ranges where they compete (masking).",
            json!({"a": track, "b": track, "start": time, "end": time}),
            &["a", "b"],
        ),
    ];
    if live {
        tools.push(tool(
            "list_actions",
            "Commands of the menus and shortcuts, by name.",
            json!({}),
            &[],
        ));
        tools.push(tool(
            "run_action",
            "Run a menu command by name, as if chosen in the menu (see list_actions).",
            json!({"name": {"type": "string"}}),
            &["name"],
        ));
    } else {
        tools.push(tool(
            "new_project",
            "Start an empty project.",
            json!({"name": {"type": "string"}, "sample_rate": {"type": "integer"}}),
            &[],
        ));
        tools.push(tool(
            "open_project",
            "Open a .strada project.",
            json!({"path": {"type": "string"}}),
            &["path"],
        ));
    }
    tools.push(tool(
        "save_project",
        "Save the project, to `path` (a .strada file) the first time.",
        json!({"path": {"type": "string"}}),
        &[],
    ));
    tools
}

fn seconds_to_samples(value: Option<&Value>, rate: u32) -> Option<u64> {
    let v = value?;
    if let Some(n) = v.get("samples").and_then(Value::as_u64) {
        return Some(n);
    }
    v.as_f64()
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| (s * f64::from(rate)).round() as u64)
}

fn track_ref(doc: &Document, value: Option<&Value>) -> Result<Channel, String> {
    let value = value.ok_or("a track is needed")?;
    if value
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("master"))
    {
        return Ok(Channel::Master);
    }
    let project = doc.project();
    let found = match value {
        Value::Number(n) => n
            .as_u64()
            .and_then(|id| project.tracks.iter().find(|t| t.id.0 == id)),
        Value::String(name) => project.tracks.iter().find(|t| t.name == *name).or_else(|| {
            project
                .tracks
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
        }),
        _ => None,
    };
    found
        .map(|t| Channel::Track(t.id))
        .ok_or_else(|| format!("no track {value}"))
}

fn track_id(doc: &Document, value: Option<&Value>) -> Result<TrackId, String> {
    match track_ref(doc, value)? {
        Channel::Track(t) => Ok(t),
        Channel::Master => Err("the master has no clips".into()),
    }
}

fn track_clips(doc: &Document, track: TrackId) -> Vec<ClipId> {
    doc.project()
        .track(track)
        .map(|t| t.clips.iter().map(|c| c.id).collect())
        .unwrap_or_default()
}

fn params_map(value: Option<&Value>) -> std::collections::BTreeMap<String, f32> {
    value
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_f64().map(|v| (k.clone(), v as f32)))
                .collect()
        })
        .unwrap_or_default()
}

/// Waits for imports, media loading and renders to finish.
fn wait_idle(doc: &mut Document) -> Result<(), String> {
    let end = Instant::now() + WAIT;
    while doc.busy() || doc.export_progress().is_some() {
        doc.update();
        if Instant::now() > end {
            return Err("timed out waiting for media or a render".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    doc.update();
    Ok(())
}

/// Lets imports, media loading and renders finish, failing on their errors (headless use).
fn settle(doc: &mut Document) -> Result<(), String> {
    wait_idle(doc)?;
    let errors: Vec<String> = doc
        .take_messages()
        .into_iter()
        .filter(|m| crate::i18n::is_error(m))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// The project as JSON.
pub fn inspect(doc: &Document, full: bool) -> Value {
    let project = doc.project();
    let rate = f64::from(project.sample_rate.max(1));
    let secs = |s: u64| (s as f64 / rate * 1000.0).round() / 1000.0;
    let inserts = |list: &[audiostrada_core::Insert]| -> Vec<Value> {
        list.iter()
            .map(|i| {
                let mut v = json!({"id": i.id.0, "effect": i.effect, "bypass": i.bypass});
                if full {
                    v["params"] = json!(i.params);
                }
                v
            })
            .collect()
    };
    let tempo = project.tempo.tempos().first().map_or(120.0, |t| t.bpm);
    let signature = project.tempo.signatures().first().copied();
    let tracks: Vec<Value> = project
        .tracks
        .iter()
        .map(|t| {
            let kind = match t.kind {
                TrackKind::Vca => "vca",
                TrackKind::Audio if doc.is_instrument_track(t.id) => "instrument",
                TrackKind::Audio => "audio",
            };
            let mut v = json!({
                "id": t.id.0,
                "name": t.name,
                "kind": kind,
                "gain_db": t.gain_db,
                "pan": t.pan,
                "mute": t.mute,
                "solo": t.solo,
                "parent": t.parent.map(|p| p.0),
                "clips": t.clips.len(),
                "inserts": inserts(&t.inserts),
                "sends": t.sends.iter().map(|s| json!({"to": s.target.0, "gain_db": s.gain_db})).collect::<Vec<_>>(),
            });
            if full {
                v["clips"] = json!(t
                    .clips
                    .iter()
                    .map(|c| json!({
                        "id": c.id.0,
                        "name": c.name,
                        "start": secs(c.start),
                        "length": secs(c.length),
                        "gain_db": c.gain_db,
                        "fade_in": secs(c.fade_in),
                        "fade_out": secs(c.fade_out),
                        "muted": c.muted,
                        "midi": c.notes.is_some(),
                    }))
                    .collect::<Vec<_>>());
            }
            v
        })
        .collect();
    json!({
        "name": project.name,
        "file": doc.paths().map(|p| p.file.display().to_string()),
        "sample_rate": project.sample_rate,
        "tempo": tempo,
        "signature": signature.map(|s| format!("{}/{}", s.numerator, s.denominator)),
        "length": secs(project.length()),
        "tracks": tracks,
        "master": {"gain_db": project.master.gain_db, "inserts": inserts(&project.master.inserts)},
        "markers": project.markers.iter().map(|m| json!({"name": m.name, "at": secs(m.position)})).collect::<Vec<_>>(),
        "can_undo": doc.history().undo_label(),
    })
}

fn effects() -> Value {
    json!(audiostrada_dsp::EFFECTS
        .iter()
        .map(|e| json!({
            "id": e.id,
            "name": e.name,
            "category": e.category,
            "instrument": e.instrument,
            "description": e.description,
            "params": e.params.iter().map(|p| {
                let mut v = json!({"id": p.id, "name": p.name, "min": p.min, "max": p.max, "default": p.default, "unit": p.unit});
                if !p.choices.is_empty() {
                    v["choices"] = json!(p.choices);
                }
                v
            }).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>())
}

/// Lets the media load before an analysis: live, its messages stay for the window to show.
fn loaded(doc: &mut Document, live: bool) -> Result<(), String> {
    if live {
        wait_idle(doc).or(Ok(()))
    } else {
        settle(doc)
    }
}

fn format(name: Option<&str>) -> Result<FileFormat, String> {
    Ok(match name.unwrap_or("wav24") {
        "wav16" => FileFormat::Wav16,
        "wav24" | "wav" => FileFormat::Wav24,
        "wav32" => FileFormat::Wav32Float,
        "flac16" => FileFormat::Flac16,
        "flac24" | "flac" => FileFormat::Flac24,
        "mp3" => FileFormat::Mp3,
        "opus" => FileFormat::Opus,
        other => return Err(format!("unknown format {other}")),
    })
}

/// Runs a tool on `doc`. `live` is a running AudioStrada: renders and imports go on in the
/// background there, while headless calls wait for them.
pub fn call_tool(
    doc: &mut Document,
    live: bool,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let rate = doc.project().sample_rate;
    let arg = |k: &str| args.get(k);
    let string = |k: &str| args.get(k).and_then(Value::as_str);
    let time = |k: &str| seconds_to_samples(args.get(k), rate);
    let wait = |doc: &mut Document| if live { Ok(()) } else { settle(doc) };
    match name {
        "inspect_project" => Ok(inspect(doc, string("detail") == Some("full"))),
        "list_effects" => Ok(effects()),
        "import_audio" => {
            let paths: Vec<PathBuf> = arg("paths")
                .and_then(Value::as_array)
                .ok_or("`paths` is a list of files")?
                .iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect();
            if let Some(missing) = paths.iter().find(|p| !p.exists()) {
                return Err(format!("no file {}", missing.display()));
            }
            match arg("track") {
                Some(t) => {
                    let track = track_id(doc, Some(t))?;
                    doc.import_at(&paths, Some(track), time("at").unwrap_or(0));
                }
                None => doc.import(&paths),
            }
            wait(doc)?;
            Ok(inspect(doc, false)["tracks"].clone())
        }
        "add_track" => {
            let kind = string("kind").unwrap_or("audio");
            let n = doc.project().tracks.len() + 1;
            let mut created = None;
            doc.history_mut().edit(tr("Add track"), |p| {
                created = Some(match kind {
                    "vca" => routing::add_vca(p, string("name").unwrap_or("VCA")),
                    "instrument" => {
                        let t =
                            p.add_track(string("name").unwrap_or(&trf!("Instrument {n}", n = n)));
                        routing::add_insert(p, Channel::Track(t), "synth", Default::default());
                        t
                    }
                    _ => p.add_track(string("name").unwrap_or(&trf!("Audio {n}", n = n))),
                });
            });
            Ok(json!({"id": created.map(|t| t.0)}))
        }
        "set_track" => {
            let channel = track_ref(doc, arg("track"))?;
            let gain = arg("gain_db")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .map(|v| {
                    (v as f32).clamp(
                        audiostrada_core::model::MIN_GAIN_DB,
                        audiostrada_core::model::MAX_TRACK_GAIN_DB,
                    )
                });
            let pan = arg("pan")
                .and_then(Value::as_f64)
                .map(|v| (v as f32).clamp(-1.0, 1.0));
            let mute = arg("mute").and_then(Value::as_bool);
            let solo = arg("solo").and_then(Value::as_bool);
            let name = string("name").map(str::to_owned);
            doc.history_mut()
                .edit(tr("Change track"), |p| match channel {
                    Channel::Master => {
                        if let Some(g) = gain {
                            p.master.gain_db = g;
                        }
                    }
                    Channel::Track(id) => {
                        if let Some(t) = p.track_mut(id) {
                            if let Some(g) = gain {
                                t.gain_db = g;
                            }
                            if let Some(v) = pan {
                                t.pan = v;
                            }
                            if let Some(v) = mute {
                                t.mute = v;
                            }
                            if let Some(v) = solo {
                                t.solo = v;
                            }
                            if let Some(n) = name {
                                t.name = n;
                            }
                        }
                    }
                });
            Ok(json!("ok"))
        }
        "delete_track" => {
            let id = track_id(doc, arg("track"))?;
            doc.history_mut().edit(tr("Delete track"), |p| {
                p.remove_track(id);
                p.prune_sources();
            });
            Ok(json!("ok"))
        }
        "add_insert" => {
            let channel = track_ref(doc, arg("track"))?;
            let effect = string("effect").ok_or("`effect` is needed")?;
            if audiostrada_dsp::info(effect).is_none() {
                return Err(format!("no built-in effect {effect} (see list_effects)"));
            }
            let params = params_map(arg("params"));
            let mut added = None;
            doc.history_mut().edit(tr("Add insert"), |p| {
                added = routing::add_insert(p, channel, effect, params);
            });
            added
                .map(|i| json!({"insert": i.0}))
                .ok_or_else(|| "cannot add it there".into())
        }
        "set_insert" | "remove_insert" => {
            let channel = track_ref(doc, arg("track"))?;
            let insert = InsertId(
                arg("insert")
                    .and_then(Value::as_u64)
                    .ok_or("`insert` id is needed")?,
            );
            if routing::find_insert(doc.project(), channel, insert).is_none() {
                return Err(format!("no insert {} there", insert.0));
            }
            if name == "remove_insert" {
                doc.history_mut().edit(tr("Remove insert"), |p| {
                    routing::remove_insert(p, channel, insert)
                });
                return Ok(json!("ok"));
            }
            let params = params_map(arg("params"));
            let bypass = arg("bypass").and_then(Value::as_bool);
            doc.history_mut().edit(tr("Change insert"), |p| {
                for (k, v) in &params {
                    routing::set_insert_param(p, channel, insert, k, *v);
                }
                if let Some(b) = bypass {
                    routing::set_bypass(p, channel, insert, b);
                }
            });
            Ok(json!("ok"))
        }
        "add_send" => {
            let from = track_id(doc, arg("from"))?;
            let to = track_id(doc, arg("to"))?;
            let mut result = Ok(());
            doc.history_mut()
                .edit(tr("Add send"), |p| result = routing::add_send(p, from, to));
            result.map_err(|e| e.to_string())?;
            Ok(json!("ok"))
        }
        "set_tempo" => {
            let bpm = arg("bpm")
                .and_then(Value::as_f64)
                .filter(|b| (10.0..=999.0).contains(b));
            let bpm = bpm.ok_or("`bpm` between 10 and 999")?;
            let numerator = arg("numerator")
                .and_then(Value::as_u64)
                .unwrap_or(4)
                .clamp(1, 32) as u32;
            let denominator = arg("denominator").and_then(Value::as_u64).unwrap_or(4) as u32;
            if ![1, 2, 4, 8, 16].contains(&denominator) {
                return Err("`denominator` is 1, 2, 4, 8 or 16".into());
            }
            doc.history_mut().edit(tr("Tempo"), |p| {
                edit::set_tempo(p, TempoMap::constant(bpm, numerator, denominator))
            });
            Ok(json!("ok"))
        }
        "add_marker" => {
            let at = time("at").ok_or("`at` is needed")?;
            let name = string("name").map(str::to_owned);
            doc.history_mut().edit(tr("Add marker"), |p| {
                let id = edit::add_marker(p, at);
                if let (Some(n), Some(m)) = (name, p.markers.iter_mut().find(|m| m.id == id)) {
                    m.name = n;
                }
            });
            Ok(json!("ok"))
        }
        "split" => {
            let at = time("at").ok_or("`at` is needed")?;
            let ids: Vec<ClipId> = match arg("track") {
                Some(t) => {
                    let track = track_id(doc, Some(t))?;
                    let ids = track_clips(doc, track);
                    // No ids would split every track.
                    if ids.is_empty() {
                        return Ok(json!({"new_clips": 0}));
                    }
                    ids
                }
                None => Vec::new(),
            };
            let mut created = Vec::new();
            doc.history_mut()
                .edit(tr("Split"), |p| created = edit::split(p, at, &ids));
            Ok(json!({"new_clips": created.len()}))
        }
        "delete_range" => {
            let (start, end) = (time("start").ok_or("`start`")?, time("end").ok_or("`end`")?);
            let tracks: Vec<TrackId> = match arg("tracks").and_then(Value::as_array) {
                Some(list) => list
                    .iter()
                    .map(|t| track_id(doc, Some(t)))
                    .collect::<Result<_, _>>()?,
                None => doc.project().tracks.iter().map(|t| t.id).collect(),
            };
            let ripple = arg("ripple").and_then(Value::as_bool).unwrap_or(false);
            doc.history_mut().edit(tr("Delete range"), |p| {
                edit::delete_range(p, &tracks, start, end, ripple)
            });
            Ok(json!("ok"))
        }
        "strip_silence" => {
            let track = track_id(doc, arg("track"))?;
            let number = |k: &str, default: f32| {
                arg(k).and_then(Value::as_f64).map_or(default, |v| v as f32)
            };
            let base = StripSettings::default();
            let settings = StripSettings {
                threshold_db: number("threshold_db", base.threshold_db),
                min_silence_ms: number("min_silence_ms", base.min_silence_ms),
                pad_before_ms: number("pad_before_ms", base.pad_before_ms),
                pad_after_ms: number("pad_after_ms", base.pad_after_ms),
            };
            let mode = match string("mode").unwrap_or("strip") {
                "extract" => StripMode::Extract,
                "separate" => StripMode::Separate,
                _ => StripMode::Strip,
            };
            loaded(doc, live)?;
            let clips: Vec<ClipId> = track_clips(doc, track);
            let found = doc.find_sound(&clips, &settings);
            let left = doc.strip_silence(&found, mode);
            Ok(json!({"clips": left.len()}))
        }
        "heal" => {
            let track = track_id(doc, arg("track"))?;
            let start = time("start").unwrap_or(0);
            let end = time("end").unwrap_or(u64::MAX);
            Ok(json!({"joined": doc.heal(&[track], start, end)}))
        }
        "beat_detective" => {
            let tracks: Vec<TrackId> = arg("tracks")
                .and_then(Value::as_array)
                .ok_or("`tracks` is a list")?
                .iter()
                .map(|t| track_id(doc, Some(t)))
                .collect::<Result<_, _>>()?;
            let start = time("start").unwrap_or(0);
            let end = time("end").unwrap_or(doc.project().length());
            let sensitivity = arg("sensitivity").and_then(Value::as_f64).unwrap_or(0.5) as f32;
            loaded(doc, live)?;
            let hits = doc.find_hits(&tracks, start, end, sensitivity);
            let conform = match string("grid") {
                None => None,
                Some(grid) => {
                    let beats = match grid {
                        "bar" => None,
                        "1/2" => Some(2.0),
                        "1/4" => Some(1.0),
                        "1/8" => Some(0.5),
                        "1/16" => Some(0.25),
                        "1/32" => Some(0.125),
                        other => return Err(format!("unknown grid {other}")),
                    };
                    Some((
                        beats,
                        arg("strength").and_then(Value::as_f64).unwrap_or(1.0),
                    ))
                }
            };
            doc.beat_detective(&tracks, start, end, &hits, conform);
            Ok(json!({"hits": hits.len()}))
        }
        "consolidate" => {
            let track = track_id(doc, arg("track"))?;
            let (start, end) = (time("start").ok_or("`start`")?, time("end").ok_or("`end`")?);
            doc.consolidate(&[(track, start, end)])?;
            wait(doc)?;
            Ok(json!("ok"))
        }
        "process_clips" => {
            let track = track_id(doc, arg("track"))?;
            let process = match string("process").ok_or("`process` is needed")? {
                "normalize" => {
                    Process::Normalize(arg("peak_db").and_then(Value::as_f64).unwrap_or(-1.0) as f32)
                }
                "reverse" => Process::Reverse,
                effect if audiostrada_dsp::info(effect).is_some() => Process::Effect {
                    id: effect.to_owned(),
                    params: params_map(arg("params")),
                    tail: arg("tail").and_then(Value::as_bool).unwrap_or(false),
                },
                other => return Err(format!("unknown process {other}")),
            };
            let clips: Vec<ClipId> = track_clips(doc, track);
            doc.process_clips(&clips, &process)?;
            wait(doc)?;
            Ok(json!("ok"))
        }
        "export_mix" => {
            let path = PathBuf::from(string("path").ok_or("`path` is needed")?);
            let normalize = match (
                arg("lufs").and_then(Value::as_f64),
                arg("peak_db").and_then(Value::as_f64),
            ) {
                (Some(l), _) => Normalize::Loudness(l as f32),
                (None, Some(p)) => Normalize::Peak(p as f32),
                _ => Normalize::Off,
            };
            let settings = ExportSettings {
                format: format(string("format"))?,
                normalize,
                stems: arg("stems").and_then(Value::as_bool).unwrap_or(false),
                ..ExportSettings::default()
            };
            let written = path.with_extension(settings.format.extension());
            loaded(doc, live)?;
            doc.start_export(settings, &path)?;
            wait(doc)?;
            Ok(json!({"file": written.display().to_string()}))
        }
        "move_clips" => {
            let ids = crate::clip_tools::ids(args)?;
            let target = match arg("track") {
                Some(v) => match track_ref(doc, Some(v))? {
                    Channel::Track(t) => Some(t),
                    Channel::Master => return Err("clips go on tracks, not the master".into()),
                },
                None => None,
            };
            let by = crate::clip_tools::signed(arg("by"), rate);
            crate::clip_tools::move_clips(doc, &ids, by, time("to"), target)
        }
        "trim_clip" => {
            let ids = crate::clip_tools::ids(args)?;
            crate::clip_tools::trim(doc, ids[0], time("start"), time("end"))
        }
        "duplicate_clips" => {
            let ids = crate::clip_tools::ids(args)?;
            crate::clip_tools::duplicate(doc, &ids, time("at"))
        }
        "delete_clips" => {
            let ids = crate::clip_tools::ids(args)?;
            let ripple = arg("ripple").and_then(Value::as_bool).unwrap_or(false);
            crate::clip_tools::delete(doc, &ids, ripple)
        }
        "set_clip" => {
            let ids = crate::clip_tools::ids(args)?;
            crate::clip_tools::set(doc, &ids, args)
        }
        "set_crossfade" => {
            let id = |k: &str| {
                arg(k)
                    .and_then(Value::as_u64)
                    .map(ClipId)
                    .ok_or(format!("`{k}` is needed"))
            };
            let length = time("length").ok_or("`length` is needed")?;
            crate::clip_tools::crossfade(doc, id("left")?, id("right")?, length)
        }
        "read_midi_clip" => {
            let id = arg("clip")
                .and_then(Value::as_u64)
                .ok_or("`clip` is needed")?;
            crate::midi_tools::read(doc, id)
        }
        "write_midi_clip" => {
            let id = arg("clip")
                .and_then(Value::as_u64)
                .ok_or("`clip` is needed")?;
            crate::midi_tools::write(doc, id, arg("notes"), string("mode") == Some("add"))
        }
        "add_midi_clip" => {
            let Channel::Track(track) = track_ref(doc, arg("track"))? else {
                return Err("MIDI clips go on a track, not the master".into());
            };
            let at = time("at").ok_or("`at` is needed")?;
            let bars = arg("bars").and_then(Value::as_f64).unwrap_or(4.0);
            crate::midi_tools::add_clip(doc, track, at, bars, arg("notes"))
        }
        "export_midi_file" => {
            let path = PathBuf::from(string("path").ok_or("`path` is needed")?);
            let track = match arg("track") {
                Some(v) => match track_ref(doc, Some(v))? {
                    Channel::Track(t) => Some(t),
                    Channel::Master => None,
                },
                None => None,
            };
            doc.export_midi(&path, track)?;
            Ok(json!({"file": path.display().to_string()}))
        }
        "import_midi_file" => {
            let path = PathBuf::from(string("path").ok_or("`path` is needed")?);
            let tracks = doc.import_midi(&path)?;
            Ok(json!({"tracks_added": tracks}))
        }
        "analyze_audio" => {
            loaded(doc, live)?;
            let target = match arg("track") {
                Some(v) => track_ref(doc, Some(v))?,
                None => Channel::Master,
            };
            let start = time("start");
            let (report, name) = crate::listening::render(doc, target, start, time("end"))?;
            let from = start.unwrap_or(0) as f64 / f64::from(rate);
            let mut out =
                crate::listening::describe(&report, &name, target == Channel::Master, from);
            let soloed: Vec<String> = doc
                .project()
                .tracks
                .iter()
                .filter(|t| t.solo)
                .map(|t| t.name.clone())
                .collect();
            if target == Channel::Master
                && !soloed.is_empty()
                && let Some(notes) = out["notes"].as_array_mut()
            {
                notes.insert(
                    0,
                    json!(format!(
                        "only the soloed tracks are heard: {}",
                        soloed.join(", ")
                    )),
                );
            }
            Ok(out)
        }
        "render_spectrum" => {
            loaded(doc, live)?;
            let targets: Vec<Channel> = match arg("tracks").and_then(Value::as_array) {
                Some(list) if !list.is_empty() => list
                    .iter()
                    .take(3)
                    .map(|v| track_ref(doc, Some(v)))
                    .collect::<Result<_, _>>()?,
                _ => vec![Channel::Master],
            };
            let mut heard = Vec::new();
            for t in targets {
                let h = crate::listening::hear(doc, t, time("start"), time("end"))?;
                let r = audiostrada_dsp::report::report(&h.left, &h.right, h.rate);
                heard.push((h, r));
            }
            let span = heard[0].0.span();
            let sounds: Vec<(String, &audiostrada_dsp::report::Report)> =
                heard.iter().map(|(h, r)| (h.name.clone(), r)).collect();
            let png = crate::plots::spectrum(&sounds, &span);
            let text = format!(
                "Spectrum of {}, {span}, smoothed over a sixth of an octave, in dB under the loudest point; {}",
                sounds
                    .iter()
                    .map(|s| s.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if sounds.len() > 1 {
                    "blue, orange and green in that order"
                } else {
                    "blue"
                }
            );
            Ok(crate::listening::picture(&png, text))
        }
        "render_spectrogram" => {
            loaded(doc, live)?;
            let target = match arg("track") {
                Some(v) => track_ref(doc, Some(v))?,
                None => Channel::Master,
            };
            let h = crate::listening::hear(doc, target, time("start"), time("end"))?;
            let (cols, rows, low, high) = (360, 160, 30.0, 16_000.0);
            let cells = audiostrada_dsp::report::spectrogram(
                &h.left, &h.right, h.rate, cols, rows, low, high,
            );
            let grid = crate::plots::Cells {
                values: &cells,
                columns: cols,
                rows,
                low,
                high,
            };
            let png = crate::plots::spectrogram(&h.name, &grid, h.from, h.seconds(), &h.span());
            Ok(crate::listening::picture(
                &png,
                format!(
                    "Spectrogram of {}, {}: time across, 30 Hz to 16 kHz up in octaves, lighter is louder (72 dB of range)",
                    h.name,
                    h.span()
                ),
            ))
        }
        "render_waveform" => {
            loaded(doc, live)?;
            let target = match arg("track") {
                Some(v) => track_ref(doc, Some(v))?,
                None => Channel::Master,
            };
            let h = crate::listening::hear(doc, target, time("start"), time("end"))?;
            let env = audiostrada_dsp::report::envelope(&h.left, &h.right, 1092);
            let lufs = audiostrada_dsp::report::loudness_curve(&h.left, &h.right, h.rate, 364);
            let reference = (target == Channel::Master).then_some(-14.0);
            let png = crate::plots::waveform(
                &h.name,
                &env,
                &lufs,
                h.from,
                h.seconds(),
                reference,
                &h.span(),
            );
            Ok(crate::listening::picture(
                &png,
                format!(
                    "Waveform of {} (blue, orange where it reaches full scale) and its short-term loudness (green), {}",
                    h.name,
                    h.span()
                ),
            ))
        }
        "compare_audio" => {
            loaded(doc, live)?;
            let (a, b) = (track_ref(doc, arg("a"))?, track_ref(doc, arg("b"))?);
            let (start, end) = (time("start"), time("end"));
            let (ra, na) = crate::listening::render(doc, a, start, end)?;
            let (rb, nb) = crate::listening::render(doc, b, start, end)?;
            Ok(crate::listening::compare(&ra, &rb, [&na, &nb]))
        }
        "undo" => Ok(json!({"undone": doc.history_mut().undo()})),
        "redo" => Ok(json!({"redone": doc.history_mut().redo()})),
        "new_project" if !live => {
            let mut fresh = Document::new(doc.dirs().clone());
            let name = string("name").unwrap_or("Untitled").to_owned();
            let rate = arg("sample_rate").and_then(Value::as_u64).unwrap_or(48_000) as u32;
            fresh
                .history_mut()
                .replace(audiostrada_core::Project::new(&name, rate), false);
            *doc = fresh;
            Ok(json!("ok"))
        }
        "open_project" if !live => {
            let path = Path::new(string("path").ok_or("`path` is needed")?);
            *doc = Document::open(path, doc.dirs().clone()).map_err(|e| e.to_string())?;
            settle(doc)?;
            Ok(inspect(doc, false))
        }
        "save_project" => {
            match string("path") {
                Some(path) => {
                    let path =
                        PathBuf::from(path).with_extension(audiostrada_core::file::EXTENSION);
                    doc.save_as(ProjectPaths::new(path))
                        .map_err(|e| e.to_string())?
                }
                None if doc.paths().is_some() => doc.save().map_err(|e| e.to_string())?,
                None => return Err("the project has no file yet: give a `path`".into()),
            }
            Ok(json!({"file": doc.paths().map(|p| p.file.display().to_string())}))
        }
        other => Err(format!("unknown tool {other}")),
    }
}

const INSTRUCTIONS: &str = "AudioStrada is a digital audio workstation. Inspect the project with \
inspect_project, add tracks, import audio, add built-in effects (list_effects) and change the \
mix, edit clips (split, strip_silence, heal, consolidate, process_clips) and export the mix. \
Hear what you change with analyze_audio (a track or the master) and compare_audio (masking). \
Read and write the notes of MIDI clips with read_midi_clip and write_midi_clip (drum maps included). \
Arrange with move_clips, trim_clip, duplicate_clips, delete_clips, set_clip and set_crossfade. \
Positions are seconds or {\"samples\": n}; tracks are named by id or name, or \"master\".";

/// What answers MCP tool calls: a document here, or a running AudioStrada.
pub trait Backend {
    fn tools(&self) -> Vec<Value>;
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String>;
}

/// A project of its own, without a window.
pub struct Headless(pub Document);

impl std::fmt::Debug for Headless {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Headless")
    }
}

impl Headless {
    pub fn new() -> Self {
        Self(Document::new(AppDirs::default()))
    }
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for Headless {
    fn tools(&self) -> Vec<Value> {
        tool_definitions(false)
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        call_tool(&mut self.0, false, name, args)
    }
}

fn result(value: Result<Value, String>) -> Value {
    match value {
        Ok(v) if v.get("image_png").is_some() => json!({
            "content": [
                {"type": "image", "data": v["image_png"], "mimeType": "image/png"},
                {"type": "text", "text": v["text"].as_str().unwrap_or("")},
            ],
            "isError": false,
        }),
        Ok(v) => {
            let text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
            json!({"content": [{"type": "text", "text": text}], "isError": false})
        }
        Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
    }
}

/// Answers one JSON-RPC message (`None` for notifications).
pub fn handle(backend: &mut dyn Backend, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(json!({}));
    let answer: Result<Value, (i64, String)> = match method {
        "initialize" => {
            const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| VERSIONS.contains(v))
                .unwrap_or(VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "audiostrada", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": backend.tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(result(backend.call(name, &args)))
        }
        "" => Err((-32600, "invalid request".into())),
        other => Err((-32601, format!("method not found: {other}"))),
    };
    Some(match answer {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, m)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}})
        }
    })
}

/// Serves MCP over newline-delimited JSON until the input ends.
pub fn serve(
    backend: &mut dyn Backend,
    input: impl BufRead,
    mut out: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.iter().filter_map(|m| handle(backend, m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(message) => handle(backend, &message),
            Err(e) => Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}),
            ),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// A tool call from the control channel, answered on the UI thread.
#[derive(Debug)]
pub struct Request {
    pub tool: String,
    pub args: Value,
    pub reply: std::sync::mpsc::Sender<Result<Value, String>>,
}

/// Where the control channel's token is kept (readable by the user only).
fn token_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("control-token"))
}

fn write_token(token: &str) -> Result<(), String> {
    let file = token_file().ok_or("no configuration folder")?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&file).map_err(|e| e.to_string())?;
    out.write_all(token.as_bytes()).map_err(|e| e.to_string())
}

/// The control channel while it listens; dropping it closes the port.
#[derive(Debug)]
pub struct Server {
    pub requests: std::sync::mpsc::Receiver<Request>,
    pub port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Server {
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        // Wakes the listener up, which then sees it should stop.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Listens on `127.0.0.1:port` for tool calls: one JSON object per line,
/// `{"token": ..., "tool": ..., "arguments": {...}}`, answered with `{"ok": ...}` or
/// `{"error": ...}`. The token is written to a file only this user can read.
pub fn listen(port: u16) -> Result<Server, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    write_token(&token)?;
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let sender = sender.clone();
                let token = token.clone();
                let _ = std::thread::Builder::new()
                    .name("control-client".into())
                    .spawn(move || answer_client(stream, &token, &sender));
            }
        })
        .map_err(|e| e.to_string())?;
    tracing::info!(port, "control channel listening on 127.0.0.1");
    Ok(Server {
        requests: receiver,
        port,
        stop,
    })
}

fn answer_client(
    stream: std::net::TcpStream,
    token: &str,
    sender: &std::sync::mpsc::Sender<Request>,
) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    for line in std::io::BufReader::new(stream).lines() {
        let Ok(line) = line else {
            return;
        };
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(m) if m.get("token").and_then(Value::as_str) != Some(token) => {
                json!({"error": "wrong token"})
            }
            Ok(m) => {
                let (reply, answer) = std::sync::mpsc::channel();
                let request = Request {
                    tool: m
                        .get("tool")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    args: m.get("arguments").cloned().unwrap_or(json!({})),
                    reply,
                };
                if sender.send(request).is_err() {
                    return;
                }
                match answer.recv_timeout(WAIT) {
                    Ok(Ok(v)) => json!({"ok": v}),
                    Ok(Err(e)) => json!({"error": e}),
                    Err(_) => json!({"error": "AudioStrada did not answer"}),
                }
            }
            Err(e) => json!({"error": format!("bad request: {e}")}),
        };
        if writeln!(out, "{reply}").is_err() {
            return;
        }
    }
}

/// A running AudioStrada, reached through its control channel when a tool is called: the
/// assistant can start before AudioStrada, and keeps working when it is restarted.
struct Remote {
    port: u16,
    link: Option<Link>,
}

struct Link {
    reader: std::io::BufReader<std::net::TcpStream>,
    writer: std::net::TcpStream,
    token: String,
}

const NOT_RUNNING: &str = "AudioStrada is not open, or AI assistants are off in it (Help, AI assistants): open AudioStrada and switch them on, then try again";

impl Remote {
    fn connect(&self) -> Result<Link, String> {
        let token = token_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .ok_or(NOT_RUNNING)?;
        let writer = std::net::TcpStream::connect(("127.0.0.1", self.port))
            .map_err(|_| NOT_RUNNING.to_owned())?;
        let reader = std::io::BufReader::new(writer.try_clone().map_err(|e| e.to_string())?);
        Ok(Link {
            reader,
            writer,
            token: token.trim().to_owned(),
        })
    }

    fn exchange(link: &mut Link, request: &Value) -> Result<String, String> {
        writeln!(link.writer, "{request}").map_err(|e| e.to_string())?;
        let mut line = String::new();
        link.reader
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Err("closed".into());
        }
        Ok(line)
    }
}

impl Backend for Remote {
    fn tools(&self) -> Vec<Value> {
        tool_definitions(true)
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let mut request = json!({"tool": name, "arguments": args});
        // A link that broke (AudioStrada restarted) is made again once.
        let mut line = None;
        for _ in 0..2 {
            if self.link.is_none() {
                self.link = Some(self.connect()?);
            }
            let link = self.link.as_mut().expect("linked");
            request["token"] = json!(link.token);
            match Self::exchange(link, &request) {
                Ok(l) => {
                    line = Some(l);
                    break;
                }
                Err(_) => self.link = None,
            }
        }
        let line = line.ok_or(NOT_RUNNING)?;
        let reply: Value =
            serde_json::from_str(&line).map_err(|e| format!("AudioStrada closed: {e}"))?;
        if reply.get("error").and_then(Value::as_str) == Some("wrong token") {
            self.link = None;
        }
        match (reply.get("ok"), reply.get("error")) {
            (Some(v), _) => Ok(v.clone()),
            (_, Some(e)) => Err(e.as_str().unwrap_or("error").to_owned()),
            _ => Err("unexpected answer".into()),
        }
    }
}

/// MCP on stdio, driving the AudioStrada that listens on `port` (AI assistants switched on,
/// or started with `--control <port>`), reached at the first tool call.
pub fn serve_remote(port: u16) -> Result<(), String> {
    let mut remote = Remote { port, link: None };
    let stdin = std::io::stdin();
    serve(&mut remote, stdin.lock(), std::io::stdout()).map_err(|e| e.to_string())
}

/// Names of the actions, for `list_actions`.
pub fn actions() -> Value {
    json!(
        crate::actions::Action::ALL
            .iter()
            .map(|a| json!({"name": a.name(), "menu": a.category()}))
            .collect::<Vec<_>>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(backend: &mut Headless, name: &str, args: Value) -> Result<Value, String> {
        backend.call(name, &args)
    }

    #[test]
    fn tools_build_and_inspect_a_mix() {
        let mut b = Headless::new();
        let id = call(&mut b, "add_track", json!({"name": "Bass"})).unwrap()["id"].clone();
        call(&mut b, "add_track", json!({"name": "Bus"})).unwrap();
        call(
            &mut b,
            "set_track",
            json!({"track": "bass", "gain_db": -4.5, "pan": -0.3}),
        )
        .unwrap();
        let insert = call(&mut b, "add_insert", json!({"track": id, "effect": "eq"})).unwrap();
        let insert = insert["insert"].as_u64().unwrap();
        call(
            &mut b,
            "set_insert",
            json!({"track": "Bass", "insert": insert, "bypass": true}),
        )
        .unwrap();
        call(&mut b, "add_send", json!({"from": "Bass", "to": "Bus"})).unwrap();
        call(
            &mut b,
            "set_tempo",
            json!({"bpm": 90, "numerator": 6, "denominator": 8}),
        )
        .unwrap();
        call(
            &mut b,
            "add_marker",
            json!({"at": {"samples": 48_000}, "name": "Verse"}),
        )
        .unwrap();
        let project = call(&mut b, "inspect_project", json!({})).unwrap();
        let bass = &project["tracks"][0];
        assert_eq!(
            (bass["gain_db"].as_f64(), bass["pan"].as_f64()),
            (Some(-4.5), Some(-0.3f32 as f64))
        );
        assert_eq!(bass["inserts"][0]["bypass"], true);
        assert_eq!(bass["sends"].as_array().unwrap().len(), 1);
        assert_eq!(project["signature"], "6/8");
        assert_eq!(project["markers"][0]["at"], 1.0);
        assert!(
            call(
                &mut b,
                "add_insert",
                json!({"track": "Bass", "effect": "nope"})
            )
            .is_err()
        );
        assert!(call(&mut b, "set_track", json!({"track": "Nobody"})).is_err());
        assert_eq!(
            call(&mut b, "undo", json!({})).unwrap()["undone"],
            "Add marker"
        );
    }

    #[test]
    fn mcp_speaks_json_rpc() {
        let mut b = Headless::new();
        let input = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05"}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "nope"}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "nope"}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let mut out = Vec::new();
        serve(&mut b, input.as_bytes(), &mut out).unwrap();
        let replies: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 4, "no reply to the notification");
        assert_eq!(replies[0]["result"]["protocolVersion"], "2024-11-05");
        assert!(replies[1]["result"]["tools"].as_array().unwrap().len() > 20);
        assert_eq!(replies[2]["result"]["isError"], true);
        assert_eq!(replies[3]["error"]["code"], -32601);
    }

    #[test]
    fn json_rpc_batches_errors_and_pings() {
        let mut b = Headless::new();
        let input = format!(
            "{}\n\nnot json\n{}\n{}\n",
            json!([
                {"jsonrpc": "2.0", "id": 1, "method": "ping"},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
            ]),
            json!({"jsonrpc": "2.0", "id": 2}),
            json!([{"jsonrpc": "2.0", "method": "notifications/cancelled"}]),
        );
        let mut out = Vec::new();
        serve(&mut b, input.as_bytes(), &mut out).unwrap();
        let replies: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 3, "a batch of notifications gets no reply");
        assert_eq!(
            replies[0],
            json!([{"jsonrpc": "2.0", "id": 1, "result": {}}])
        );
        assert_eq!(replies[1]["error"]["code"], -32700);
        assert_eq!(replies[2]["error"]["code"], -32600);
        let unknown =
            json!({"id": 3, "method": "initialize", "params": {"protocolVersion": "1999"}});
        let reply = handle(&mut b, &unknown).unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn tools_differ_live_and_headless() {
        let names = |live| -> Vec<String> {
            tool_definitions(live)
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_owned())
                .collect()
        };
        let (live, headless) = (names(true), names(false));
        assert!(live.contains(&"run_action".into()) && !live.contains(&"open_project".into()));
        assert!(
            headless.contains(&"open_project".into()) && !headless.contains(&"run_action".into())
        );
        assert!(live.contains(&"save_project".into()) && headless.contains(&"save_project".into()));
        assert_eq!(
            actions().as_array().unwrap().len(),
            crate::actions::Action::ALL.len()
        );
        let effects = effects();
        assert!(effects.as_array().unwrap().iter().any(|e| e["id"] == "eq"));
    }

    #[test]
    fn arguments_are_read_leniently() {
        assert_eq!(seconds_to_samples(Some(&json!(1.5)), 48_000), Some(72_000));
        assert_eq!(
            seconds_to_samples(Some(&json!({"samples": 7})), 48_000),
            Some(7)
        );
        assert_eq!(seconds_to_samples(Some(&json!(-1.0)), 48_000), None);
        assert_eq!(seconds_to_samples(None, 48_000), None);
        assert_eq!(format(None), Ok(FileFormat::Wav24));
        assert_eq!(format(Some("flac")), Ok(FileFormat::Flac24));
        assert!(format(Some("aiff")).is_err());
        let params = params_map(Some(&json!({"gain": 2, "name": "x"})));
        assert_eq!(
            params.into_iter().collect::<Vec<_>>(),
            vec![("gain".into(), 2.0)]
        );

        let mut b = Headless::new();
        let id = call(&mut b, "add_track", json!({"name": "Kick"})).unwrap()["id"].as_u64();
        let doc = &b.0;
        let kick = Channel::Track(TrackId(id.unwrap()));
        assert_eq!(track_ref(doc, Some(&json!("KICK"))), Ok(kick));
        assert_eq!(track_ref(doc, Some(&json!(id))), Ok(kick));
        assert_eq!(track_ref(doc, Some(&json!("Master"))), Ok(Channel::Master));
        assert!(track_ref(doc, Some(&json!(true))).is_err());
        assert!(track_ref(doc, None).is_err());
        assert!(track_id(doc, Some(&json!("master"))).is_err());
    }

    #[test]
    fn tools_edit_tracks_clips_and_tempo() {
        let mut b = Headless::new();
        call(&mut b, "add_track", json!({"kind": "instrument"})).unwrap();
        call(&mut b, "add_track", json!({"kind": "vca"})).unwrap();
        call(&mut b, "add_track", json!({})).unwrap();
        let project = call(&mut b, "inspect_project", json!({"detail": "full"})).unwrap();
        let kinds: Vec<&str> = project["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["instrument", "vca", "audio"]);
        assert_eq!(project["tracks"][2]["name"], "Audio 3");
        assert_eq!(project["tracks"][0]["inserts"][0]["effect"], "synth");
        assert!(project["tracks"][0]["inserts"][0]["params"].is_object());

        let insert = project["tracks"][0]["inserts"][0]["id"].clone();
        call(
            &mut b,
            "remove_insert",
            json!({"track": "Instrument 1", "insert": insert}),
        )
        .unwrap();
        assert!(
            call(
                &mut b,
                "remove_insert",
                json!({"track": "Instrument 1", "insert": insert})
            )
            .is_err()
        );
        call(
            &mut b,
            "set_track",
            json!({"track": "master", "gain_db": -2.0}),
        )
        .unwrap();
        assert_eq!(b.0.project().master.gain_db, -2.0);

        assert!(call(&mut b, "set_tempo", json!({"bpm": 5})).is_err());
        assert!(call(&mut b, "set_tempo", json!({"bpm": 100, "denominator": 3})).is_err());
        call(&mut b, "set_tempo", json!({"bpm": 100})).unwrap();
        assert_eq!(
            call(&mut b, "inspect_project", json!({})).unwrap()["tempo"],
            100.0
        );

        // A track without clips splits nothing, not every track.
        let audio = b.0.project().tracks[2].id;
        b.0.history_mut().edit("Clip", |p| {
            let s = p.add_source(audiostrada_core::Source {
                path: "a.wav".into(),
                channels: 1,
                sample_rate: 48_000,
                frames: 48_000,
            });
            p.add_clip(audio, s, 0);
        });
        let split = |b: &mut Headless, args| call(b, "split", args).unwrap()["new_clips"].clone();
        assert_eq!(
            split(&mut b, json!({"at": 0.5, "track": "Instrument 1"})),
            0
        );
        assert_eq!(b.0.project().tracks[2].clips.len(), 1);
        assert_eq!(split(&mut b, json!({"at": 0.5})), 1);
        call(
            &mut b,
            "delete_range",
            json!({"start": 0.0, "end": 0.25, "tracks": ["Audio 3"]}),
        )
        .unwrap();
        assert_eq!(b.0.project().tracks[2].clips[0].start, 12_000);
        assert!(call(&mut b, "delete_range", json!({"start": 0.0})).is_err());
        assert_eq!(
            call(&mut b, "heal", json!({"track": "Audio 3"})).unwrap()["joined"],
            1
        );
        assert_eq!(b.0.project().tracks[2].clips.len(), 1);
        assert!(
            call(
                &mut b,
                "beat_detective",
                json!({"tracks": ["Audio 3"], "grid": "1/3"})
            )
            .is_err()
        );

        call(&mut b, "delete_track", json!({"track": "Audio 3"})).unwrap();
        assert_eq!(b.0.project().tracks.len(), 2);
        assert_eq!(
            call(&mut b, "undo", json!({})).unwrap()["undone"],
            "Delete track"
        );
        assert_eq!(
            call(&mut b, "redo", json!({})).unwrap()["redone"],
            "Delete track"
        );
        assert!(call(&mut b, "nope", json!({})).is_err());
    }

    #[test]
    fn projects_are_created_saved_and_opened() {
        let dir = std::env::temp_dir().join(format!("audiostrada-control-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut b = Headless::new();
        call(
            &mut b,
            "new_project",
            json!({"name": "Demo", "sample_rate": 44_100}),
        )
        .unwrap();
        assert_eq!(b.0.project().sample_rate, 44_100);
        assert!(
            call(&mut b, "save_project", json!({})).is_err(),
            "no file yet"
        );
        call(&mut b, "add_track", json!({"name": "Bass"})).unwrap();
        let saved = call(&mut b, "save_project", json!({"path": dir.join("demo")})).unwrap();
        let file = saved["file"].as_str().unwrap().to_owned();
        assert!(file.ends_with(audiostrada_core::file::EXTENSION));
        call(&mut b, "save_project", json!({})).unwrap();
        call(&mut b, "new_project", json!({})).unwrap();
        let opened = call(&mut b, "open_project", json!({"path": file})).unwrap();
        assert_eq!(opened["tracks"][0]["name"], "Bass");
        assert!(
            call(
                &mut b,
                "open_project",
                json!({"path": dir.join("none.strada")})
            )
            .is_err()
        );
        assert!(
            call(
                &mut b,
                "import_audio",
                json!({"paths": [dir.join("none.wav")]})
            )
            .is_err()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn agents_hear_a_track_and_the_mix() {
        use audiostrada_media::{AudioData, write_wav};
        let dir = std::env::temp_dir().join(format!("audiostrada-hear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tone = |freq: f32, gain: f32| -> Vec<f32> {
            (0..4 * 48_000)
                .map(|i| gain * (i as f32 * freq * std::f32::consts::TAU / 48_000.0).sin())
                .collect()
        };
        write_wav(
            &dir.join("low.wav"),
            &AudioData::new(vec![tone(80.0, 0.5)], 48_000),
        )
        .unwrap();
        write_wav(
            &dir.join("high.wav"),
            &AudioData::new(vec![tone(3_000.0, 0.1)], 48_000),
        )
        .unwrap();
        let mut b = Headless::new();
        call(
            &mut b,
            "import_audio",
            json!({"paths": [dir.join("low.wav"), dir.join("high.wav")]}),
        )
        .unwrap();
        let low = call(&mut b, "analyze_audio", json!({"track": "low"})).unwrap();
        assert!(
            low["spectrum"]["bands_db_of_total"]["bass 60-250 Hz"]
                .as_f64()
                .unwrap()
                > -1.0,
            "{low}"
        );
        assert!(low["loudness"]["integrated_lufs"].as_f64().is_some());
        let mix = call(&mut b, "analyze_audio", json!({})).unwrap();
        assert_eq!(mix["target"], "master");
        assert!(mix["seconds"].as_f64().unwrap() > 3.9);
        let both = call(&mut b, "compare_audio", json!({"a": "low", "b": "high"})).unwrap();
        assert!(
            both["b_minus_a"]["integrated_lu"].as_f64().is_some(),
            "{both}"
        );
        assert!(call(&mut b, "analyze_audio", json!({"track": "nope"})).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
