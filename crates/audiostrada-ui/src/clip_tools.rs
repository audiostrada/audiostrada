//! Clips for agents, audio or MIDI, as on the timeline: moved (in time or to another track),
//! trimmed, duplicated, deleted, their gain, fades, mute, name, length and pitch set, and the
//! crossfade between two of them. Every call is one undo step; the files never change.

use audiostrada_core::model::FadeShape;
use audiostrada_core::{ClipId, TrackId, edit};
use serde_json::{Value, json};

use crate::document::Document;

/// Clip ids from `"clips": [..]` or `"clip": n`.
pub fn ids(args: &Value) -> Result<Vec<ClipId>, String> {
    let list: Vec<u64> = match (
        args.get("clips").and_then(Value::as_array),
        args.get("clip").and_then(Value::as_u64),
    ) {
        (Some(list), _) => list.iter().filter_map(Value::as_u64).collect(),
        (None, Some(one)) => vec![one],
        _ => {
            return Err(
                "`clips` (ids, from inspect_project with detail \"full\") is needed".into(),
            );
        }
    };
    if list.is_empty() {
        return Err("no clip given".into());
    }
    Ok(list.into_iter().map(ClipId).collect())
}

fn check(doc: &Document, ids: &[ClipId]) -> Result<(), String> {
    match ids
        .iter()
        .find(|id| edit::clip(doc.project(), **id).is_none())
    {
        Some(missing) => Err(format!("no clip {}", missing.0)),
        None => Ok(()),
    }
}

/// Seconds, or `{"samples": n}`, negative allowed.
pub fn signed(value: Option<&Value>, rate: u32) -> Option<i64> {
    let v = value?;
    if let Some(n) = v.get("samples").and_then(Value::as_i64) {
        return Some(n);
    }
    v.as_f64()
        .filter(|s| s.is_finite())
        .map(|s| (s * f64::from(rate)).round() as i64)
}

fn describe(doc: &Document, ids: &[ClipId]) -> Value {
    let project = doc.project();
    let rate = f64::from(project.sample_rate);
    let secs = |s: u64| (s as f64 / rate * 1000.0).round() / 1000.0;
    json!(ids
        .iter()
        .filter_map(|id| {
            let c = edit::clip(project, *id)?;
            let track = edit::track_of_clip(project, *id).and_then(|t| project.track(t)).map(|t| t.name.clone());
            Some(json!({"id": id.0, "track": track, "start": secs(c.start), "length": secs(c.length), "gain_db": c.gain_db, "muted": c.muted}))
        })
        .collect::<Vec<_>>())
}

/// Moves clips by `by` (signed), or so the earliest starts `to`, and to `track`.
pub fn move_clips(
    doc: &mut Document,
    ids: &[ClipId],
    by: Option<i64>,
    to: Option<u64>,
    track: Option<TrackId>,
) -> Result<Value, String> {
    check(doc, ids)?;
    let project = doc.project();
    let earliest = ids
        .iter()
        .filter_map(|id| edit::clip(project, *id))
        .map(|c| c.start)
        .min()
        .unwrap_or(0);
    let delta = match (by, to) {
        (Some(d), _) => d,
        (None, Some(t)) => t as i64 - earliest as i64,
        (None, None) => 0,
    };
    let offset = match track {
        Some(target) => {
            let first = edit::track_of_clip(project, ids[0]).ok_or("no such clip")?;
            let index = |t: TrackId| project.tracks.iter().position(|x| x.id == t);
            index(target).ok_or("no such track")? as isize
                - index(first).ok_or("no such track")? as isize
        }
        None => 0,
    };
    if delta == 0 && offset == 0 {
        return Err("give `by`, `to` or `track`".into());
    }
    doc.history_mut()
        .edit("Move clips", |p| edit::move_clips(p, ids, delta, offset));
    Ok(json!({"clips": describe(doc, ids)}))
}

pub fn trim(
    doc: &mut Document,
    id: ClipId,
    start: Option<u64>,
    end: Option<u64>,
) -> Result<Value, String> {
    check(doc, &[id])?;
    if start.is_none() && end.is_none() {
        return Err("give `start` (new start, end kept) or `end` (new end)".into());
    }
    doc.history_mut().edit("Trim clip", |p| {
        if let Some(s) = start {
            edit::trim_start(p, id, s);
        }
        if let Some(e) = end {
            edit::trim_end(p, id, e);
        }
    });
    Ok(json!({"clips": describe(doc, &[id]), "note": "trimmed within the material the clip has"}))
}

pub fn duplicate(doc: &mut Document, ids: &[ClipId], at: Option<u64>) -> Result<Value, String> {
    check(doc, ids)?;
    let mut made = Vec::new();
    doc.history_mut().edit("Duplicate clips", |p| {
        made = edit::duplicate(p, ids);
        if let Some(at) = at {
            let first = made
                .iter()
                .filter_map(|id| edit::clip(p, *id))
                .map(|c| c.start)
                .min()
                .unwrap_or(at);
            edit::move_clips(p, &made, at as i64 - first as i64, 0);
        }
    });
    Ok(json!({"clips": describe(doc, &made)}))
}

pub fn delete(doc: &mut Document, ids: &[ClipId], ripple: bool) -> Result<Value, String> {
    check(doc, ids)?;
    doc.history_mut()
        .edit("Delete clips", |p| edit::delete_clips(p, ids, ripple));
    Ok(json!({"deleted": ids.len()}))
}

fn shape(name: &str) -> Result<FadeShape, String> {
    Ok(match name {
        "equal_power" => FadeShape::EqualPower,
        "linear" => FadeShape::Linear,
        "fast" => FadeShape::Fast,
        "slow" => FadeShape::Slow,
        "s_curve" => FadeShape::SCurve,
        other => {
            return Err(format!(
                "unknown fade shape {other}: equal_power, linear, fast, slow, s_curve"
            ));
        }
    })
}

/// Gain, fades, mute, name, length (stretching audio) and pitch of clips.
pub fn set(doc: &mut Document, ids: &[ClipId], args: &Value) -> Result<Value, String> {
    check(doc, ids)?;
    let rate = doc.project().sample_rate;
    let samples = |k: &str| {
        args.get(k)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|s| (s * f64::from(rate)).round() as u64)
    };
    let gain = args
        .get("gain_db")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .map(|v| v as f32);
    let (fade_in, fade_out) = (samples("fade_in"), samples("fade_out"));
    let fade_shape = args
        .get("fade_shape")
        .and_then(Value::as_str)
        .map(shape)
        .transpose()?;
    let muted = args.get("muted").and_then(Value::as_bool);
    let name = args.get("name").and_then(Value::as_str).map(str::to_owned);
    let length = samples("stretch_to");
    let pitch = args
        .get("pitch")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .map(|v| (v as f32).clamp(-24.0, 24.0));
    doc.history_mut().edit("Change clips", |p| {
        for &id in ids {
            if let Some(g) = gain {
                edit::set_clip_gain(p, id, g);
            }
            if fade_in.is_some() || fade_out.is_some() {
                let c = edit::clip(p, id).cloned();
                if let Some(c) = c {
                    edit::set_fades(
                        p,
                        id,
                        fade_in.unwrap_or(c.fade_in),
                        fade_out.unwrap_or(c.fade_out),
                    );
                }
            }
            if let Some(l) = length {
                edit::stretch_clip(p, id, l);
            }
            if let Some(c) = edit::clip_mut(p, id) {
                if let Some(s) = fade_shape {
                    c.fade_shape = s;
                    c.fade_out_shape = None;
                }
                if let Some(m) = muted {
                    c.muted = m;
                }
                if let Some(n) = &name {
                    c.name = n.clone();
                }
            }
        }
        if let Some(st) = pitch {
            edit::set_pitch(p, ids, st);
        }
    });
    Ok(json!({"clips": describe(doc, ids)}))
}

pub fn crossfade(
    doc: &mut Document,
    left: ClipId,
    right: ClipId,
    length: u64,
) -> Result<Value, String> {
    check(doc, &[left, right])?;
    let project = doc.project();
    if edit::track_of_clip(project, left) != edit::track_of_clip(project, right) {
        return Err("the two clips must be on the same track, `left` before `right`".into());
    }
    doc.history_mut()
        .edit("Crossfade", |p| edit::set_crossfade(p, left, right, length));
    Ok(json!({"clips": describe(doc, &[left, right])}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;

    #[test]
    fn clips_move_trim_duplicate_change_and_go() {
        let mut doc = Document::new(AppDirs::default());
        let (mut a, mut b) = (None, None);
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("Keys");
            let u = p.add_track("Other");
            a = p.add_midi_clip(t, 48_000, 96_000, vec![]);
            b = Some(u);
        });
        let (a, other) = (a.unwrap(), b.unwrap());
        move_clips(&mut doc, &[a], Some(-24_000), None, None).unwrap();
        assert_eq!(edit::clip(doc.project(), a).unwrap().start, 24_000);
        move_clips(&mut doc, &[a], None, Some(0), Some(other)).unwrap();
        assert_eq!(edit::track_of_clip(doc.project(), a), Some(other));
        assert_eq!(edit::clip(doc.project(), a).unwrap().start, 0);
        trim(&mut doc, a, Some(12_000), None).unwrap();
        assert_eq!(edit::clip(doc.project(), a).unwrap().start, 12_000);
        let copies = duplicate(&mut doc, &[a], Some(200_000)).unwrap();
        let copy = ClipId(copies["clips"][0]["id"].as_u64().unwrap());
        assert_eq!(edit::clip(doc.project(), copy).unwrap().start, 200_000);
        set(
            &mut doc,
            &[copy],
            &json!({"gain_db": -6.0, "muted": true, "name": "Echo"}),
        )
        .unwrap();
        let c = edit::clip(doc.project(), copy).unwrap();
        assert_eq!((c.gain_db, c.muted, c.name.as_str()), (-6.0, true, "Echo"));
        assert!(set(&mut doc, &[copy], &json!({"fade_shape": "wobbly"})).is_err());
        delete(&mut doc, &[copy], false).unwrap();
        assert!(edit::clip(doc.project(), copy).is_none());
        assert!(move_clips(&mut doc, &[ClipId(999_999)], Some(1), None, None).is_err());
        // Each call is one step: undoing brings the deleted copy back.
        doc.history_mut().undo();
        assert!(edit::clip(doc.project(), copy).is_some());
    }
}
