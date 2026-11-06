//! The song for agents: the transport (play, stop, locate, loop), its structure (markers as
//! sections, the tempo map), MIDI transformations, and analyses of the whole: section by
//! section, its key, and against a reference recording.

use audiostrada_core::routing::Channel;
use audiostrada_core::tempo::{TempoMap, TempoPoint, TimeSignature};
use audiostrada_core::{ClipId, edit};
use serde_json::{Value, json};

use crate::control::{loaded, seconds_to_samples, tool, track_ref};
use crate::document::Document;

pub fn definitions() -> Vec<Value> {
    let track = json!({"description": "track id or name, or \"master\""});
    let time = json!({"description": "seconds, or {\"samples\": n}"});
    vec![
        tool(
            "transport",
            "Play, stop, move the playhead, or set the loop: action play, stop, locate (`at`), loop (`start`, `end`: plays that stretch over and over) or loop_off.",
            json!({"action": {"type": "string", "enum": ["play", "stop", "locate", "loop", "loop_off"]}, "at": time, "start": time, "end": time}),
            &["action"],
        ),
        tool(
            "sections",
            "The song's sections: its markers, each from its place to the next (or the end), in seconds and bars.",
            json!({}),
            &[],
        ),
        tool(
            "set_marker",
            "Rename, move or delete a marker (by id or name); markers name the sections (Intro, Verse, Chorus…).",
            json!({"marker": {"description": "id or name"}, "name": {"type": "string"}, "at": time, "delete": {"type": "boolean"}}),
            &["marker"],
        ),
        tool(
            "set_tempo_map",
            "Tempo and time signature changes over the song: tempos [{beat, bpm}] (beat 0 first), signatures [{bar (0-based), numerator, denominator}]. Clips keep their place in seconds.",
            json!({"tempos": {"type": "array"}, "signatures": {"type": "array"}}),
            &["tempos"],
        ),
        tool(
            "transform_midi",
            "Transform the notes of MIDI clips: quantize (grid in beats, e.g. 0.25 for sixteenths; strength 0-1; swing 0-1 delays the off-beats), transpose (semitones), velocity {scale, add, set}, humanize {timing_ms, velocity}, length {set (beats) or scale}. One undo step.",
            json!({"clips": {"type": "array", "items": {"type": "integer"}}, "quantize": {"type": "number"}, "strength": {"type": "number"}, "swing": {"type": "number"}, "transpose": {"type": "integer"}, "velocity": {"type": "object"}, "humanize": {"type": "object"}, "length": {"type": "object"}}),
            &["clips"],
        ),
        tool(
            "analyze_sections",
            "Loudness, peaks and frequency balance of each section (between markers) of a track or the master: how the song builds and where it drops.",
            json!({"track": track}),
            &[],
        ),
        tool(
            "detect_key",
            "The most likely key of a track or the master (or a stretch of it), from the pitches it plays.",
            json!({"track": track, "start": time, "end": time}),
            &[],
        ),
        tool(
            "compare_to_reference",
            "Compare the master (or a track) with a reference recording (an audio file): loudness, peaks, dynamics, stereo width and balance per band, in figures to match.",
            json!({"path": {"type": "string"}, "track": track, "start": time, "end": time}),
            &["path"],
        ),
    ]
}

fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

/// The markers in order: (id, name, start, end).
fn sections(doc: &Document) -> Vec<(u64, String, u64, u64)> {
    let project = doc.project();
    let mut markers: Vec<_> = project.markers.iter().collect();
    markers.sort_by_key(|m| m.position);
    let end = project.length();
    markers
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let next = markers.get(i + 1).map_or(end, |n| n.position);
            (m.id.0, m.name.clone(), m.position, next.max(m.position))
        })
        .collect()
}

fn note_clips(doc: &Document, args: &Value) -> Result<Vec<ClipId>, String> {
    let ids = crate::clip_tools::ids(args)?;
    for id in &ids {
        match edit::clip(doc.project(), *id) {
            Some(c) if c.is_midi() => {}
            Some(_) => return Err(format!("clip {} is audio, not MIDI", id.0)),
            None => return Err(format!("no clip {}", id.0)),
        }
    }
    Ok(ids)
}

fn target(doc: &Document, args: &Value) -> Result<Channel, String> {
    match args.get("track") {
        Some(v) => track_ref(doc, Some(v)),
        None => Ok(Channel::Master),
    }
}

pub fn call(
    doc: &mut Document,
    live: bool,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let rate = doc.project().sample_rate;
    let time = |k: &str| seconds_to_samples(args.get(k), rate);
    let secs = |s: u64| round(s as f64 / f64::from(rate), 3);
    let result = (|| -> Result<Value, String> {
        match name {
            "transport" => {
                let action = args.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "play" => doc.player_mut().engine_mut().play(),
                    "stop" => doc.player_mut().engine_mut().stop(),
                    "locate" => doc.player_mut().locate(time("at").ok_or("`at` is needed")?),
                    "loop" => {
                        let (start, end) = (
                            time("start").ok_or("`start` is needed")?,
                            time("end").ok_or("`end` is needed")?,
                        );
                        if end <= start {
                            return Err("`end` must come after `start`".into());
                        }
                        doc.history_mut().edit("Loop", |p| {
                            p.loop_range = Some((start, end));
                            p.loop_off = false;
                        });
                        doc.player_mut().locate(start);
                    }
                    "loop_off" => {
                        doc.history_mut().edit("Loop off", |p| p.loop_off = true);
                    }
                    _ => return Err("`action`: play, stop, locate, loop or loop_off".into()),
                }
                let player = doc.player();
                Ok(
                    json!({"playing": player.engine().playing(), "position": secs(player.position()), "heard": live}),
                )
            }
            "sections" => {
                let project = doc.project();
                let (tempo, rate) = (&project.tempo, project.sample_rate);
                let bar = |s: u64| {
                    let beat = tempo.sample_to_beat(s, rate);
                    round(beat / 4.0 + 1.0, 2)
                };
                let list = sections(doc);
                if list.is_empty() {
                    return Ok(
                        json!({"sections": [], "note": "no markers: add_marker names where sections start"}),
                    );
                }
                Ok(
                    json!({"sections": list.iter().map(|(id, name, a, b)| json!({"marker": id, "name": name, "start": secs(*a), "end": secs(*b), "bars": [bar(*a), bar(*b)]})).collect::<Vec<_>>()}),
                )
            }
            "set_marker" => {
                let wanted = args.get("marker").ok_or("`marker` is needed")?;
                let project = doc.project();
                let found = project
                    .markers
                    .iter()
                    .find(|m| {
                        wanted.as_u64() == Some(m.id.0)
                            || wanted
                                .as_str()
                                .is_some_and(|n| m.name.eq_ignore_ascii_case(n))
                    })
                    .map(|m| m.id)
                    .ok_or(format!("no marker {wanted}"))?;
                let (name, at) = (
                    args.get("name").and_then(Value::as_str).map(str::to_owned),
                    time("at"),
                );
                let delete = args.get("delete").and_then(Value::as_bool) == Some(true);
                doc.history_mut().edit("Marker", |p| {
                    if delete {
                        edit::remove_marker(p, found);
                        return;
                    }
                    if let Some(m) = p.markers.iter_mut().find(|m| m.id == found) {
                        if let Some(n) = &name {
                            m.name = n.clone();
                        }
                        if let Some(a) = at {
                            m.position = a;
                        }
                    }
                    p.markers.sort_by_key(|m| (m.position, m.id));
                });
                Ok(json!("ok"))
            }
            "set_tempo_map" => {
                let mut tempos = Vec::new();
                for t in args
                    .get("tempos")
                    .and_then(Value::as_array)
                    .ok_or("`tempos` is needed")?
                {
                    let beat = t
                        .get("beat")
                        .and_then(Value::as_f64)
                        .filter(|b| *b >= 0.0)
                        .ok_or("each tempo needs a `beat`")?;
                    let bpm = t
                        .get("bpm")
                        .and_then(Value::as_f64)
                        .filter(|b| (10.0..=999.0).contains(b))
                        .ok_or("each tempo needs a `bpm` from 10 to 999")?;
                    tempos.push(TempoPoint { beat, bpm });
                }
                let mut signatures = Vec::new();
                for s in args
                    .get("signatures")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let bar = s.get("bar").and_then(Value::as_u64).unwrap_or(0) as u32;
                    let numerator = s
                        .get("numerator")
                        .and_then(Value::as_u64)
                        .unwrap_or(4)
                        .clamp(1, 32) as u32;
                    let denominator =
                        s.get("denominator").and_then(Value::as_u64).unwrap_or(4) as u32;
                    if ![1, 2, 4, 8, 16].contains(&denominator) {
                        return Err("a `denominator` is 1, 2, 4, 8 or 16".into());
                    }
                    signatures.push(TimeSignature {
                        bar,
                        numerator,
                        denominator,
                    });
                }
                if signatures.is_empty() {
                    signatures = doc.project().tempo.signatures().to_vec();
                }
                if tempos.is_empty() {
                    return Err("`tempos` is empty".into());
                }
                let map = TempoMap::new(tempos, signatures);
                doc.history_mut()
                    .edit("Tempo map", |p| edit::set_tempo(p, map));
                Ok(json!("ok"))
            }
            "transform_midi" => {
                let ids = note_clips(doc, args)?;
                let grid = args
                    .get("quantize")
                    .and_then(Value::as_f64)
                    .filter(|g| *g > 0.0);
                let strength = args.get("strength").and_then(Value::as_f64).unwrap_or(1.0);
                let swing = args
                    .get("swing")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let transpose = args.get("transpose").and_then(Value::as_i64).unwrap_or(0);
                let velocity = args.get("velocity").cloned().unwrap_or(json!({}));
                let humanize = args.get("humanize").cloned().unwrap_or(json!({}));
                let length = args.get("length").cloned().unwrap_or(json!({}));
                let f = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
                let mut seed = 0x2545_f491u32;
                let mut random = move || {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as f64 / u32::MAX as f64 * 2.0 - 1.0
                };
                let mut changed = 0;
                doc.history_mut().edit("Transform notes", |p| {
                    let (tempo, rate) = (p.tempo.clone(), p.sample_rate);
                    for id in &ids {
                        let Some(c) = edit::clip(p, *id).cloned() else {
                            continue;
                        };
                        let origin = c.start.saturating_sub(c.offset);
                        let first = tempo.sample_to_beat(c.start, rate);
                        let Some(clip) = edit::clip_mut(p, *id) else {
                            continue;
                        };
                        let Some(notes) = clip.notes.as_mut() else {
                            continue;
                        };
                        let notes = std::sync::Arc::make_mut(notes);
                        if let Some(g) = grid {
                            edit::quantize(notes, origin, Some(g), &tempo, rate, strength);
                            if swing > 0.0 {
                                for n in notes.iter_mut() {
                                    let beat = tempo.sample_to_beat(origin + n.start, rate) - first;
                                    let step = (beat / g).round() as i64;
                                    if step % 2 == 1 {
                                        let at = tempo
                                            .beat_to_sample(first + beat + swing * g / 2.0, rate);
                                        n.start = at.saturating_sub(origin);
                                    }
                                }
                            }
                        }
                        for n in notes.iter_mut() {
                            n.key = (i64::from(n.key) + transpose).clamp(0, 127) as u8;
                            let mut v = f64::from(n.velocity);
                            if let Some(s) = f(&velocity, "scale") {
                                v *= s;
                            }
                            if let Some(a) = f(&velocity, "add") {
                                v += a;
                            }
                            if let Some(s) = f(&velocity, "set") {
                                v = s;
                            }
                            if let Some(h) = f(&humanize, "velocity") {
                                v += h * random();
                            }
                            n.velocity = v.round().clamp(1.0, 127.0) as u8;
                            if let Some(ms) = f(&humanize, "timing_ms") {
                                let shift = (ms / 1000.0 * f64::from(rate) * random()) as i64;
                                n.start = (n.start as i64 + shift).max(0) as u64;
                            }
                            let at = origin + n.start;
                            let beat = tempo.sample_to_beat(at, rate);
                            let beats = tempo.sample_to_beat(at + n.length, rate) - beat;
                            let new = f(&length, "set")
                                .or_else(|| f(&length, "scale").map(|s| beats * s));
                            if let Some(b) = new {
                                n.length = tempo
                                    .beat_to_sample(beat + b.max(1.0 / 64.0), rate)
                                    .saturating_sub(at)
                                    .max(1);
                            }
                            changed += 1;
                        }
                        notes.sort_by_key(|n| (n.start, n.key));
                    }
                });
                Ok(json!({"notes_changed": changed}))
            }
            "analyze_sections" => {
                loaded(doc, live)?;
                let channel = target(doc, args)?;
                let list = sections(doc);
                if list.is_empty() {
                    return Err(
                        "no markers: add markers where the sections start, then try again".into(),
                    );
                }
                let (from, to) = (list[0].2, list.last().map_or(0, |s| s.3));
                let h = crate::listening::hear(doc, channel, Some(from), Some(to))?;
                let out: Vec<Value> = list
                    .iter()
                    .map(|(_, name, a, b)| {
                        let scale = f64::from(h.rate) / f64::from(rate);
                        let (i, j) = (((a - from) as f64 * scale) as usize, (((b - from) as f64 * scale) as usize).min(h.left.len()));
                        let r = audiostrada_dsp::report::report(&h.left[i.min(j)..j], &h.right[i.min(j)..j], h.rate);
                        json!({
                            "section": name,
                            "start": secs(*a),
                            "integrated_lufs": r.integrated.map(|v| round(f64::from(v), 1)),
                            "short_term_max_lufs": r.short_term_max.map(|v| round(f64::from(v), 1)),
                            "true_peak_dbtp": round(f64::from(r.true_peak), 1),
                            "crest_db": round(f64::from(r.crest), 1),
                            "side_to_mid_db": r.side_to_mid.map(|v| round(f64::from(v), 1)),
                            "bands_db_of_total": audiostrada_dsp::report::BANDS.iter().zip(r.bands).map(|((n, _, _), v)| (n.to_string(), json!(round(f64::from(v), 1)))).collect::<serde_json::Map<_, _>>(),
                        })
                    })
                    .collect();
                Ok(json!({"target": h.name, "sections": out}))
            }
            "detect_key" => {
                loaded(doc, live)?;
                let channel = target(doc, args)?;
                let h = crate::listening::hear(doc, channel, time("start"), time("end"))?;
                match audiostrada_dsp::report::key(&h.left, &h.right, h.rate) {
                    Some((key, margin)) => Ok(
                        json!({"target": h.name, "key": key, "confidence": if margin > 0.08 { "clear" } else if margin > 0.03 { "likely" } else { "uncertain (relative major and minor, or a modal song)" }}),
                    ),
                    None => Err("nothing tonal in that range".into()),
                }
            }
            "compare_to_reference" => {
                loaded(doc, live)?;
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or("`path` is needed")?;
                let reference = audiostrada_media::load(std::path::Path::new(path))
                    .map_err(|e| format!("cannot read {path}: {e}"))?;
                let left = reference.channel(0);
                let right = if reference.channels() > 1 {
                    reference.channel(1)
                } else {
                    left
                };
                let theirs = audiostrada_dsp::report::report(left, right, reference.sample_rate());
                let channel = target(doc, args)?;
                let h = crate::listening::hear(doc, channel, time("start"), time("end"))?;
                let ours = audiostrada_dsp::report::report(&h.left, &h.right, h.rate);
                let mut out = crate::listening::compare(&theirs, &ours, ["reference", &h.name]);
                out["note"] = json!(
                    "b_minus_a: positive means the project has more of it than the reference"
                );
                out["reference"] =
                    crate::listening::describe(&theirs, path, true, 0.0)["loudness"].clone();
                Ok(out)
            }
            _ => Err(String::new()),
        }
    })();
    match result {
        Err(e) if e.is_empty() => None,
        r => Some(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;

    #[test]
    fn midi_is_quantized_swung_transposed_and_markers_make_sections() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut()
            .edit("Setup", |p| track = Some(p.add_track("Keys")));
        let track = track.unwrap();
        let made = crate::midi_tools::add_clip(
            &mut doc,
            track,
            0,
            1.0,
            Some(&json!([[0.03, 0.25, 60, 100], [0.52, 0.25, 62, 100]])),
        )
        .unwrap();
        let id = made["clip"].as_u64().unwrap();
        call(&mut doc, false, "transform_midi", &json!({"clips": [id], "quantize": 0.5, "swing": 0.5, "transpose": 12, "velocity": {"set": 90}})).unwrap().unwrap();
        let notes = crate::midi_tools::read(&doc, id).unwrap()["notes"].clone();
        assert_eq!(notes[0], json!([0.0, 0.25, 72, 90]));
        assert_eq!(notes[1][0], json!(0.625), "{notes}");
        doc.history_mut().edit("Markers", |p| {
            edit::add_marker(p, 0);
            edit::add_marker(p, 96_000);
        });
        let s = call(&mut doc, false, "sections", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(s["sections"].as_array().unwrap().len(), 2);
        let first = s["sections"][0]["marker"].clone();
        call(
            &mut doc,
            false,
            "set_marker",
            &json!({"marker": first, "name": "Intro"}),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            call(&mut doc, false, "sections", &json!({}))
                .unwrap()
                .unwrap()["sections"][0]["name"],
            "Intro"
        );
        assert!(call(&mut doc, false, "nope", &json!({})).is_none());
    }
}
