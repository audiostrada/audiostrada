//! MIDI clips for agents: their notes in beats from the clip's start, read and written, new
//! clips, and which key plays which drum on a Strada-Drums track.

use audiostrada_core::model::Note;
use audiostrada_core::{ClipId, Project, TrackId, edit, routing};
use serde_json::{Value, json};

use crate::document::Document;

/// A note as agents give it: beats from the clip's start.
struct Spec {
    beat: f64,
    length: f64,
    key: u8,
    velocity: u8,
    channel: Option<u8>,
    probability: u8,
}

fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Which key plays which drum, when `track` plays Strada-Drums.
fn drum_map(project: &Project, track: TrackId) -> Option<Value> {
    let t = project.track(track)?;
    routing::flatten(&t.inserts)
        .iter()
        .any(|i| i.effect == "drums")
        .then(|| {
            json!(
                audiostrada_dsp::drums::VOICES
                    .iter()
                    .map(|(_, name, key)| json!({"key": key, "drum": name}))
                    .collect::<Vec<_>>()
            )
        })
}

fn midi_clip(project: &Project, id: u64) -> Result<(audiostrada_core::Clip, TrackId), String> {
    let clip = edit::clip(project, ClipId(id))
        .ok_or("no such clip (see inspect_project with detail full)")?;
    if !clip.is_midi() {
        return Err("this clip is audio, not MIDI".into());
    }
    let track = edit::track_of_clip(project, ClipId(id)).ok_or("no such clip")?;
    Ok((clip.clone(), track))
}

pub fn read(doc: &Document, id: u64) -> Result<Value, String> {
    let project = doc.project();
    let (clip, track) = midi_clip(project, id)?;
    let (tempo, rate) = (&project.tempo, project.sample_rate);
    let first = tempo.sample_to_beat(clip.start, rate);
    let origin = clip.start.saturating_sub(clip.offset);
    let mut notes: Vec<&Note> = clip.notes.iter().flat_map(|n| n.iter()).collect();
    notes.sort_by_key(|n| (n.start, n.key));
    let shown: Vec<Value> = notes
        .iter()
        .filter(|n| {
            let at = origin + n.start;
            at >= clip.start && at < clip.end()
        })
        .map(|n| {
            let at = origin + n.start;
            let beat = tempo.sample_to_beat(at, rate) - first;
            let length = tempo.sample_to_beat(at + n.length, rate) - tempo.sample_to_beat(at, rate);
            let mut v = json!([round(beat), round(length), n.key, n.velocity]);
            if n.probability < 100 {
                v.as_array_mut()
                    .expect("array")
                    .push(json!({"probability": n.probability}));
            }
            v
        })
        .collect();
    let mut out = json!({
        "clip": id,
        "track": project.track(track).map(|t| t.name.clone()),
        "starts_at_beat": round(first),
        "length_beats": round(tempo.sample_to_beat(clip.end(), rate) - first),
        "channel": notes.first().map_or(0, |n| n.channel),
        "notes_format": "[beat from the clip's start, length in beats, key (60 = C4), velocity 1-127]",
        "notes": shown,
    });
    if let Some(map) = drum_map(project, track) {
        out["drum_map"] = map;
    }
    Ok(out)
}

fn specs(value: Option<&Value>) -> Result<Vec<Spec>, String> {
    let list = value
        .and_then(Value::as_array)
        .ok_or("`notes` is needed: [{beat, length, key, velocity}]")?;
    list.iter()
        .map(|n| {
            // Objects, or the arrays read_midi_clip gives.
            let get = |name: &str, index: usize| {
                n.get(name).or_else(|| n.get(index)).and_then(Value::as_f64)
            };
            let beat = get("beat", 0)
                .filter(|b| b.is_finite() && *b >= 0.0)
                .ok_or("each note needs a `beat` of 0 or more")?;
            let length = get("length", 1).unwrap_or(0.25).max(1.0 / 64.0);
            let key = get("key", 2)
                .filter(|k| (0.0..=127.0).contains(k))
                .ok_or("each note needs a `key` from 0 to 127")? as u8;
            let velocity = get("velocity", 3).unwrap_or(100.0).clamp(1.0, 127.0) as u8;
            Ok(Spec {
                beat,
                length,
                key,
                velocity,
                channel: n
                    .get("channel")
                    .and_then(Value::as_u64)
                    .map(|c| c.min(15) as u8),
                probability: n
                    .get("probability")
                    .and_then(Value::as_u64)
                    .map_or(100, |p| p.clamp(1, 100) as u8),
            })
        })
        .collect()
}

/// The notes of `specs` in the content of a clip starting at `start` with `origin`.
fn notes(project: &Project, start: u64, origin: u64, specs: &[Spec], channel: u8) -> Vec<Note> {
    let (tempo, rate) = (&project.tempo, project.sample_rate);
    let first = tempo.sample_to_beat(start, rate);
    specs
        .iter()
        .map(|s| {
            let at = tempo.beat_to_sample(first + s.beat, rate);
            let end = tempo.beat_to_sample(first + s.beat + s.length, rate);
            Note {
                start: at.saturating_sub(origin),
                length: end.saturating_sub(at).max(1),
                key: s.key,
                velocity: s.velocity,
                channel: s.channel.unwrap_or(channel),
                probability: s.probability,
                muted: false,
            }
        })
        .collect()
}

pub fn write(
    doc: &mut Document,
    id: u64,
    value: Option<&Value>,
    add: bool,
) -> Result<Value, String> {
    let specs = specs(value)?;
    let (clip, _) = midi_clip(doc.project(), id)?;
    let origin = clip.start.saturating_sub(clip.offset);
    let channel = clip
        .notes
        .as_ref()
        .and_then(|n| n.first())
        .map_or(0, |n| n.channel);
    let new = notes(doc.project(), clip.start, origin, &specs, channel);
    let count = new.len();
    doc.history_mut()
        .edit(if add { "Add notes" } else { "Replace notes" }, |p| {
            if let Some(c) = edit::clip_mut(p, ClipId(id)) {
                let list = std::sync::Arc::make_mut(c.notes.get_or_insert_with(Default::default));
                if !add {
                    // Notes hidden by a trim stay: only what the clip shows is replaced.
                    let (from, to) = (clip.offset, clip.offset + clip.length);
                    list.retain(|n| n.start < from || n.start >= to);
                }
                list.extend(new);
                list.sort_by_key(|n| (n.start, n.key));
            }
            edit::fit_clip_to_notes(p, ClipId(id));
        });
    Ok(json!({"clip": id, "notes_written": count}))
}

pub fn add_clip(
    doc: &mut Document,
    track: TrackId,
    start: u64,
    bars: f64,
    value: Option<&Value>,
) -> Result<Value, String> {
    let project = doc.project();
    let t = project.track(track).ok_or("no such track")?;
    if t.kind != audiostrada_core::TrackKind::Audio {
        return Err("MIDI clips go on audio or instrument tracks".into());
    }
    let specs = match value {
        Some(_) => specs(value)?,
        None => Vec::new(),
    };
    let (tempo, rate) = (&project.tempo, project.sample_rate);
    let first = tempo.sample_to_beat(start, rate);
    let signature = tempo.signature_at_beat(first);
    let beats_per_bar = f64::from(signature.numerator) * 4.0 / f64::from(signature.denominator);
    let end = tempo.beat_to_sample(first + bars.max(0.25) * beats_per_bar, rate);
    let channel = if drum_map(project, track).is_some() {
        9
    } else {
        0
    };
    let new = notes(project, start, start, &specs, channel);
    let mut made = None;
    doc.history_mut().edit("Add MIDI clip", |p| {
        made = p.add_midi_clip(track, start, end - start, new);
        if let Some(id) = made {
            edit::fit_clip_to_notes(p, id);
        }
    });
    let id = made.ok_or("the clip could not be added")?;
    Ok(json!({"clip": id.0, "notes_written": specs.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;

    #[test]
    fn notes_go_in_and_come_out_in_beats() {
        let mut doc = Document::new(AppDirs::default());
        let mut track = None;
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("Drums");
            routing::add_insert(p, routing::Channel::Track(t), "drums", Default::default());
            track = Some(t);
        });
        let track = track.unwrap();
        let rate = doc.project().sample_rate;
        let bar = doc.project().tempo.beat_to_sample(4.0, rate);
        let made = add_clip(&mut doc, track, bar, 1.0, Some(&json!([{"beat": 0, "length": 0.25, "key": 36, "velocity": 120}, {"beat": 1, "key": 38}]))).unwrap();
        let id = made["clip"].as_u64().unwrap();
        let read_back = read(&doc, id).unwrap();
        assert_eq!(
            read_back["notes"],
            json!([[0.0, 0.25, 36, 120], [1.0, 0.25, 38, 100]])
        );
        assert_eq!(read_back["length_beats"], json!(4.0));
        assert_eq!(read_back["channel"], json!(9));
        assert!(
            read_back["drum_map"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["key"] == 36)
        );
        // Replacing, in the arrays read gives; a note past the end lengthens the clip.
        write(
            &mut doc,
            id,
            Some(&json!([[0.5, 0.5, 42, 90], [6.0, 0.5, 46, 80]])),
            false,
        )
        .unwrap();
        let again = read(&doc, id).unwrap();
        assert_eq!(
            again["notes"],
            json!([[0.5, 0.5, 42, 90], [6.0, 0.5, 46, 80]])
        );
        assert!(again["length_beats"].as_f64().unwrap() >= 7.0);
        write(&mut doc, id, Some(&json!([{"beat": 2, "key": 36}])), true).unwrap();
        assert_eq!(
            read(&doc, id).unwrap()["notes"].as_array().unwrap().len(),
            3
        );
        assert!(write(&mut doc, id, Some(&json!([{"key": 36}])), false).is_err());
        assert!(read(&doc, 999_999).is_err());
    }
}
