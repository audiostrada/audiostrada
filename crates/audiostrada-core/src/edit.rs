//! Editing operations on a project: move, trim, split, delete, duplicate, fades, markers,
//! playlists and grid snapping. Each is meant to run inside one `History::edit`.
//!
//! Overlaps on a track follow simple rules, as in Reaper: a clip moved or trimmed across the
//! edge of another one crossfades with it, over exactly their common part, and the crossfade
//! follows the overlap as it grows, shrinks or goes; a clip covering another one, or covered by
//! it, replaces the covered part.

use crate::model::{
    Clip, ClipId, MAX_CLIP_GAIN_DB, MIN_GAIN_DB, Marker, MarkerId, Note, Playlist, Project,
    SourceId, Track, TrackId, TrackKind,
};
use crate::tempo::TempoMap;

/// Copies a track (clips, playlists, inserts, sends, automation) just below it, or below its
/// folder content; returns the copy.
pub fn duplicate_track(project: &mut Project, id: TrackId) -> Option<TrackId> {
    let mut copy = project.track(id)?.clone();
    let last = crate::routing::subtree(project, id)
        .iter()
        .filter_map(|t| project.tracks.iter().position(|x| x.id == *t))
        .max()?;
    copy.id = project.new_track_id();
    copy.name = format!("{} (copy)", copy.name);
    copy.armed = false;
    copy.solo = false;
    for clip in copy
        .clips
        .iter_mut()
        .chain(copy.alternates.iter_mut().flat_map(|p| p.clips.iter_mut()))
        .chain(copy.takes.iter_mut().flat_map(|p| p.clips.iter_mut()))
    {
        clip.id = project.new_clip_id();
    }
    let mut renamed = std::collections::HashMap::new();
    for insert in &mut copy.inserts {
        let fresh = project.new_insert_id();
        renamed.insert(insert.id, fresh);
        insert.id = fresh;
    }
    for lane in &mut copy.automation {
        if let crate::automation::AutomationTarget::Param { insert, .. } = &mut lane.target
            && let Some(fresh) = renamed.get(insert)
        {
            *insert = *fresh;
        }
    }
    let new = copy.id;
    project.tracks.insert(last + 1, std::sync::Arc::new(copy));
    Some(new)
}

/// Splits the clips of `tracks` at both edges of `start..end`; returns the clips inside.
pub fn split_range(project: &mut Project, tracks: &[TrackId], start: u64, end: u64) -> Vec<ClipId> {
    let on_tracks = |p: &Project| -> Vec<ClipId> {
        p.tracks
            .iter()
            .filter(|t| tracks.contains(&t.id))
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect()
    };
    for at in [start, end] {
        let ids = on_tracks(project);
        if !ids.is_empty() {
            split(project, at, &ids);
        }
    }
    project
        .tracks
        .iter()
        .filter(|t| tracks.contains(&t.id))
        .flat_map(|t| t.clips.iter())
        .filter(|c| c.start >= start && c.end() <= end)
        .map(|c| c.id)
        .collect()
}

/// Removes what `tracks` play within `start..end`; with `ripple`, later clips of those tracks
/// move back to close the gap.
pub fn delete_range(project: &mut Project, tracks: &[TrackId], start: u64, end: u64, ripple: bool) {
    if end <= start {
        return;
    }
    let inside = split_range(project, tracks, start, end);
    delete_clips(project, &inside, false);
    if ripple {
        let later: Vec<ClipId> = project
            .tracks
            .iter()
            .filter(|t| tracks.contains(&t.id))
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.start >= end)
            .map(|c| c.id)
            .collect();
        move_clips(project, &later, -((end - start) as i64), 0);
    }
}

/// Opens `length` samples of silence at `at` on every track, moving later clips, markers and
/// automation points.
pub fn insert_silence(project: &mut Project, at: u64, length: u64) {
    if length == 0 {
        return;
    }
    let all: Vec<TrackId> = project.tracks.iter().map(|t| t.id).collect();
    split_range(project, &all, at, at);
    for id in all {
        if let Some(t) = project.track_mut(id) {
            for c in t.clips.iter_mut().filter(|c| c.start >= at) {
                c.start += length;
            }
            for lane in &mut t.automation {
                for p in lane.points.iter_mut().filter(|p| p.time >= at) {
                    p.time += length;
                }
            }
        }
    }
    for m in project.markers.iter_mut().filter(|m| m.position >= at) {
        m.position += length;
    }
}

/// Shortest clip a trim can leave, in samples.
pub const MIN_LENGTH: u64 = 16;

/// Track index and position of a clip.
pub fn locate(project: &Project, id: ClipId) -> Option<(usize, usize)> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(t, track)| track.clips.iter().position(|c| c.id == id).map(|c| (t, c)))
}

pub fn clip(project: &Project, id: ClipId) -> Option<&Clip> {
    locate(project, id).map(|(t, c)| &project.tracks[t].clips[c])
}

pub fn clip_mut(project: &mut Project, id: ClipId) -> Option<&mut Clip> {
    let (t, c) = locate(project, id)?;
    let track_id = project.tracks[t].id;
    project.track_mut(track_id).map(|track| &mut track.clips[c])
}

/// The track holding a clip.
pub fn track_of_clip(project: &Project, id: ClipId) -> Option<TrackId> {
    locate(project, id).map(|(t, _)| project.tracks[t].id)
}

/// Lengthens a MIDI clip to the bar after its last note, so pasted notes are heard; never
/// shortens it.
pub fn fit_clip_to_notes(project: &mut Project, id: ClipId) {
    let (rate, tempo) = (project.sample_rate, project.tempo.clone());
    let Some(c) = clip_mut(project, id) else {
        return;
    };
    let origin = c.start.saturating_sub(c.offset);
    let Some(last) = c
        .notes
        .as_ref()
        .and_then(|n| n.iter().map(|n| n.start + n.length).max())
    else {
        return;
    };
    if origin + last <= c.end() {
        return;
    }
    let beat = tempo.sample_to_beat(origin + last, rate);
    let bbt = tempo.beat_to_bbt(beat);
    let mut bar = tempo.bbt_to_beat(crate::tempo::BarBeatTick {
        bar: bbt.bar,
        beat: 1,
        tick: 0,
    });
    if bar < beat {
        bar = tempo.bbt_to_beat(crate::tempo::BarBeatTick {
            bar: bbt.bar + 1,
            beat: 1,
            tick: 0,
        });
    }
    c.length = tempo.beat_to_sample(bar, rate).saturating_sub(c.start);
}

/// Source length available to a clip, in project samples, stretch included.
fn source_length(project: &Project, clip: &Clip) -> u64 {
    project.sources.get(&clip.source).map_or(u64::MAX, |s| {
        (s.frames_at(project.sample_rate) as f64 * clip.stretch).round() as u64
    })
}

/// Stretches a clip to `length` samples from its start, keeping its pitch; the part of the
/// source shown stays the same.
pub fn stretch_clip(project: &mut Project, id: ClipId, length: u64) {
    if let Some(c) = clip_mut(project, id)
        && !c.is_midi()
        && c.length > 0
    {
        let factor = length.max(MIN_LENGTH) as f64 / c.length as f64;
        let stretch = (c.stretch * factor).clamp(0.1, 10.0);
        let factor = stretch / c.stretch;
        c.stretch = stretch;
        c.offset = (c.offset as f64 * factor).round() as u64;
        c.length = (c.length as f64 * factor).round().max(MIN_LENGTH as f64) as u64;
        c.fade_in = (c.fade_in as f64 * factor) as u64;
        c.fade_out = (c.fade_out as f64 * factor) as u64;
        clamp_fades(c);
    }
}

/// Transposes clips by `semitones` (absolute).
pub fn set_pitch(project: &mut Project, ids: &[ClipId], semitones: f32) {
    for id in ids {
        if let Some(c) = clip_mut(project, *id)
            && !c.is_midi()
        {
            c.pitch = semitones.clamp(-24.0, 24.0);
        }
    }
}

fn sort_track(project: &mut Project, track: TrackId) {
    if let Some(t) = project.track_mut(track) {
        t.clips.sort_by_key(|c| (c.start, c.id));
    }
}

pub(crate) fn clamp_fades(clip: &mut Clip) {
    clip.fade_in = clip.fade_in.min(clip.length);
    clip.fade_out = clip.fade_out.min(clip.length - clip.fade_in);
}

/// Cuts `clip` at `at` (strictly inside it) and returns the right part, with the id `id`.
fn split_off(clip: &mut Clip, at: u64, id: ClipId) -> Clip {
    let cut = at - clip.start;
    let mut right = clip.clone();
    right.id = id;
    right.start = at;
    right.offset += cut;
    right.length = clip.length - cut;
    right.fade_in = 0;
    clip.length = cut;
    clip.fade_out = 0;
    clamp_fades(clip);
    clamp_fades(&mut right);
    right
}

/// Applies the overlap rules for the clips in `placed` on `track`.
pub fn resolve_overlaps(project: &mut Project, track: TrackId, placed: &[ClipId]) {
    let mut next_id = project.peek_id();
    let Some(t) = project.track_mut(track) else {
        return;
    };
    let placed_clips: Vec<Clip> = t
        .clips
        .iter()
        .filter(|c| placed.contains(&c.id))
        .cloned()
        .collect();
    for p in &placed_clips {
        let mut keep = Vec::with_capacity(t.clips.len() + 1);
        for mut o in std::mem::take(&mut t.clips) {
            let overlaps = o.start < p.end() && p.start < o.end();
            if placed.contains(&o.id) || !overlaps {
                keep.push(o);
            } else if p.start <= o.start && o.end() <= p.end() {
                // Fully covered: replaced.
            } else if o.start <= p.start && p.end() <= o.end() {
                // Covers the placed clip: keep what lies on each side.
                if o.end() > p.end() {
                    keep.push(split_off(&mut o, p.end(), ClipId(next_id)));
                    next_id = next_id.saturating_add(1);
                }
                if p.start > o.start {
                    o.length = p.start - o.start;
                    o.fade_out = 0;
                    clamp_fades(&mut o);
                    keep.push(o);
                }
            } else {
                keep.push(o);
            }
        }
        t.clips = keep;
    }
    // Partial overlaps crossfade over their common part.
    for p in &placed_clips {
        let Some(pi) = t.clips.iter().position(|c| c.id == p.id) else {
            continue;
        };
        for oi in 0..t.clips.len() {
            if oi == pi || placed.contains(&t.clips[oi].id) {
                continue;
            }
            let (o_start, o_end) = (t.clips[oi].start, t.clips[oi].end());
            let (p_start, p_end) = (t.clips[pi].start, t.clips[pi].end());
            if o_start < p_start && o_end > p_start && o_end < p_end {
                let overlap = o_end - p_start;
                t.clips[oi].fade_out = overlap;
                t.clips[pi].fade_in = overlap;
            } else if o_start > p_start && o_start < p_end && o_end > p_end {
                let overlap = p_end - o_start;
                t.clips[oi].fade_in = overlap;
                t.clips[pi].fade_out = overlap;
            }
        }
    }
    t.clips.iter_mut().for_each(clamp_fades);
    t.clips.sort_by_key(|c| (c.start, c.id));
    project.reserve_ids(next_id);
}

/// Neighbours partly overlapping: each pair, its common part.
fn crossfaded_pairs(clips: &[Clip]) -> Vec<(ClipId, ClipId, u64)> {
    let mut sorted: Vec<&Clip> = clips.iter().collect();
    sorted.sort_by_key(|c| (c.start, c.id));
    sorted
        .windows(2)
        .filter(|w| w[0].end() > w[1].start && w[0].end() < w[1].end() && w[0].start < w[1].start)
        .map(|w| (w[0].id, w[1].id, w[0].end() - w[1].start))
        .collect()
}

/// Reaper's crossfades on `track`: neighbours partly overlapping fade over exactly their
/// common part; a pair that crossfaded in `before` and no longer meets loses those fades.
pub fn update_crossfades(project: &mut Project, track: TrackId, before: &[Clip]) {
    for (a, b, overlap) in crossfaded_pairs(before) {
        let meets = match (locate(project, a), locate(project, b)) {
            (Some((ta, _)), Some((tb, _))) if ta == tb => {
                let (ca, cb) = (clip(project, a), clip(project, b));
                ca.zip(cb)
                    .is_some_and(|(x, y)| x.end() > y.start && x.start < y.start)
            }
            _ => false,
        };
        if meets {
            continue;
        }
        if let Some(m) = clip_mut(project, a)
            && m.fade_out == overlap
        {
            m.fade_out = 0;
        }
        if let Some(m) = clip_mut(project, b)
            && m.fade_in == overlap
        {
            m.fade_in = 0;
        }
    }
    let Some(t) = project.track_mut(track) else {
        return;
    };
    t.clips.sort_by_key(|c| (c.start, c.id));
    for (a, b, overlap) in crossfaded_pairs(&t.clips) {
        for c in t.clips.iter_mut() {
            if c.id == a {
                c.fade_out = overlap;
            } else if c.id == b {
                c.fade_in = overlap;
            }
        }
    }
    t.clips.iter_mut().for_each(clamp_fades);
}

/// The crossfades of every track holding `ids` now or in `before` (their clips then), after
/// the clips were moved or trimmed.
pub fn crossfade_after(project: &mut Project, ids: &[ClipId], before: &[Clip]) {
    let mut tracks: Vec<TrackId> = Vec::new();
    for id in ids {
        if let Some((t, _)) = locate(project, *id) {
            let track = project.tracks[t].id;
            if !tracks.contains(&track) {
                tracks.push(track);
            }
        }
    }
    for c in before {
        if let Some((t, _)) = locate(project, c.id) {
            let track = project.tracks[t].id;
            if !tracks.contains(&track) {
                tracks.push(track);
            }
        }
    }
    for track in tracks {
        update_crossfades(project, track, before);
    }
}

/// The clips of the tracks holding `ids`, as they are before an edit.
pub fn clips_around(project: &Project, ids: &[ClipId]) -> Vec<Clip> {
    project
        .tracks
        .iter()
        .filter(|t| t.clips.iter().any(|c| ids.contains(&c.id)))
        .flat_map(|t| t.clips.iter().cloned())
        .collect()
}

/// Slides the crossfade between `left` and `right` by `delta` samples, keeping its length:
/// the end of one and the start of the other move together, within their sources.
pub fn move_crossfade(project: &mut Project, left: ClipId, right: ClipId, delta: i64) {
    let (Some(a), Some(b)) = (clip(project, left).cloned(), clip(project, right).cloned()) else {
        return;
    };
    let before = clips_around(project, &[left, right]);
    // As far as both edges can go: the material before the right clip and after the left one,
    // and each keeping some length.
    let back = b.offset.min(a.length.saturating_sub(MIN_LENGTH)) as i64;
    let after = source_length(project, &a).saturating_sub(a.offset + a.length);
    let forth = after.min(b.length.saturating_sub(MIN_LENGTH)) as i64;
    let delta = delta.clamp(-back, forth);
    trim_end(project, left, (a.end() as i64 + delta).max(0) as u64);
    trim_start(project, right, (b.start as i64 + delta).max(0) as u64);
    crossfade_after(project, &[left, right], &before);
}

/// Both sides of a crossfade take `shape`: the end of `left`, the start of `right`.
pub fn set_crossfade_shape(
    project: &mut Project,
    left: ClipId,
    right: ClipId,
    shape: crate::model::FadeShape,
) {
    if let Some(a) = clip_mut(project, left) {
        a.fade_out_shape = Some(shape);
    }
    if let Some(b) = clip_mut(project, right) {
        let out = b.out_shape();
        b.fade_shape = shape;
        b.fade_out_shape = Some(out);
    }
}

/// Moves clips by `delta` samples (never before 0) and `track_offset` tracks.
pub fn move_clips(project: &mut Project, ids: &[ClipId], delta: i64, track_offset: isize) {
    let before = clips_around(project, ids);
    let track_count = project.tracks.len() as isize;
    let mut destinations: Vec<TrackId> = Vec::new();
    for &id in ids {
        let Some((t, c)) = locate(project, id) else {
            continue;
        };
        let source_track = project.tracks[t].id;
        let target = (t as isize + track_offset).clamp(0, track_count - 1) as usize;
        let target = if project.tracks[target].kind == TrackKind::Audio {
            target
        } else {
            t
        };
        let target_track = project.tracks[target].id;
        let Some(mut moved) = project.track_mut(source_track).map(|tr| tr.clips.remove(c)) else {
            continue;
        };
        moved.start = (moved.start as i64 + delta).max(0) as u64;
        if let Some(tr) = project.track_mut(target_track) {
            tr.clips.push(moved);
        }
        if !destinations.contains(&target_track) {
            destinations.push(target_track);
        }
    }
    for track in destinations {
        sort_track(project, track);
        resolve_overlaps(project, track, ids);
    }
    crossfade_after(project, ids, &before);
}

/// Moves the start of a clip, keeping its end in place.
pub fn trim_start(project: &mut Project, id: ClipId, new_start: u64) {
    let Some(c) = clip(project, id).cloned() else {
        return;
    };
    let earliest = c.start.saturating_sub(c.offset);
    let new_start = new_start.clamp(earliest, c.end().saturating_sub(MIN_LENGTH).max(earliest));
    let track = project.tracks[locate(project, id).map_or(0, |l| l.0)].id;
    if let Some(m) = clip_mut(project, id) {
        let end = m.end();
        m.offset = (m.offset as i64 + new_start as i64 - m.start as i64).max(0) as u64;
        m.start = new_start;
        m.length = end - new_start;
        clamp_fades(m);
    }
    sort_track(project, track);
}

/// Moves the end of a clip, within the available source material.
pub fn trim_end(project: &mut Project, id: ClipId, new_end: u64) {
    let Some(c) = clip(project, id).cloned() else {
        return;
    };
    let latest = c.start + source_length(project, &c).saturating_sub(c.offset);
    let new_end = new_end.clamp(c.start + MIN_LENGTH, latest.max(c.start + MIN_LENGTH));
    if let Some(m) = clip_mut(project, id) {
        m.length = new_end - m.start;
        clamp_fades(m);
    }
}

pub fn set_fades(project: &mut Project, id: ClipId, fade_in: u64, fade_out: u64) {
    if let Some(m) = clip_mut(project, id) {
        m.fade_in = fade_in;
        m.fade_out = fade_out;
        clamp_fades(m);
    }
}

/// The crossfade between `left` and `right` (the next clip on its track) set to `length`,
/// centered where they meet: both edges move within their source, the fades cover the overlap.
pub fn set_crossfade(project: &mut Project, left: ClipId, right: ClipId, length: u64) {
    let (Some(a), Some(b)) = (clip(project, left).cloned(), clip(project, right).cloned()) else {
        return;
    };
    let center = (b.start + a.end().max(b.start)) / 2;
    let half = length / 2;
    trim_end(project, left, center + (length - half));
    trim_start(project, right, center.saturating_sub(half));
    let (Some(a), Some(b)) = (clip(project, left).cloned(), clip(project, right).cloned()) else {
        return;
    };
    let overlap = a.end().saturating_sub(b.start);
    set_fades(
        project,
        left,
        a.fade_in.min(a.length - overlap.min(a.length)),
        overlap,
    );
    set_fades(
        project,
        right,
        overlap,
        b.fade_out.min(b.length - overlap.min(b.length)),
    );
}

/// Moves the audio inside a clip by `delta` samples (later when positive), the clip staying
/// in place, within its source.
pub fn slip_content(project: &mut Project, id: ClipId, delta: i64) {
    let Some(c) = clip(project, id).cloned() else {
        return;
    };
    let latest = source_length(project, &c)
        .saturating_sub(c.length)
        .min(i64::MAX as u64) as i64;
    if let Some(m) = clip_mut(project, id) {
        m.offset = (c.offset as i64 - delta).clamp(0, latest.max(0)) as u64;
    }
}

/// Fades over a range on `tracks`, as its edges fall: over a clip's start a fade-in, over its
/// end a fade-out, over the meeting of two clips a crossfade.
pub fn fades_in_range(project: &mut Project, tracks: &[TrackId], start: u64, end: u64) {
    if end <= start {
        return;
    }
    let length = end - start;
    for &track in tracks {
        let Some(t) = project.track(track) else {
            continue;
        };
        let clips: Vec<Clip> = t.clips.clone();
        let mut joined: Vec<ClipId> = Vec::new();
        for pair in clips.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let meet = (a.end().min(b.end()) + b.start) / 2;
            if a.end() >= b.start && meet > start && meet < end {
                set_crossfade(project, a.id, b.id, length);
                joined.extend([a.id, b.id]);
            }
        }
        for c in &clips {
            let Some(now) = clip(project, c.id).cloned() else {
                continue;
            };
            let (mut fade_in, mut fade_out) = (now.fade_in, now.fade_out);
            let in_range = |x: u64| x >= start && x < end;
            let crossfaded = joined.contains(&c.id);
            if in_range(now.start) && now.end() > end && !crossfaded {
                fade_in = end - now.start;
            }
            if now.end() > start && now.end() <= end && now.start < start && !crossfaded {
                fade_out = now.end() - start;
            }
            set_fades(project, c.id, fade_in, fade_out);
        }
    }
}

/// Changes the tempo map. MIDI clips keep their place in bars and beats, with their notes and
/// controllers, as on ticks-based tracks; audio clips stay at their time.
pub fn set_tempo(project: &mut Project, tempo: TempoMap) {
    let rate = project.sample_rate;
    let old = std::mem::replace(&mut project.tempo, tempo);
    let new = &project.tempo;
    let map = |sample: u64| new.beat_to_sample(old.sample_to_beat(sample, rate), rate);
    for track in &mut project.tracks {
        let track = std::sync::Arc::make_mut(track);
        let alternates = track.alternates.iter_mut().flat_map(|p| p.clips.iter_mut());
        let takes = track.takes.iter_mut().flat_map(|p| p.clips.iter_mut());
        for clip in track.clips.iter_mut().chain(alternates).chain(takes) {
            if !clip.is_midi() {
                continue;
            }
            let origin = clip.start.saturating_sub(clip.offset);
            let (start, end, new_origin) = (map(clip.start), map(clip.end()), map(origin));
            let Some(notes) = clip.notes.as_mut() else {
                continue;
            };
            for note in std::sync::Arc::make_mut(notes) {
                let on = map(origin + note.start);
                let off = map(origin + note.start + note.length);
                note.start = on.saturating_sub(new_origin);
                note.length = off.saturating_sub(on).max(1);
            }
            if let Some(controls) = clip.controls.as_mut() {
                for point in std::sync::Arc::make_mut(controls) {
                    point.time = map(origin + point.time).saturating_sub(new_origin);
                }
            }
            clip.start = start;
            clip.length = end.saturating_sub(start).max(1);
            clip.offset = start.saturating_sub(new_origin);
            clamp_fades(clip);
        }
    }
}

pub fn set_clip_gain(project: &mut Project, id: ClipId, gain_db: f32) {
    if let Some(m) = clip_mut(project, id) {
        m.gain_db = gain_db.clamp(MIN_GAIN_DB, MAX_CLIP_GAIN_DB);
    }
}

/// Splits the given clips (or every clip when `ids` is empty) at `at`. Returns the new right
/// halves.
pub fn split(project: &mut Project, at: u64, ids: &[ClipId]) -> Vec<ClipId> {
    let mut created = Vec::new();
    let targets: Vec<(TrackId, ClipId)> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(move |c| (t.id, c)))
        .filter(|(_, c)| (ids.is_empty() || ids.contains(&c.id)) && c.start < at && at < c.end())
        .filter(|(_, c)| at - c.start >= MIN_LENGTH && c.end() - at >= MIN_LENGTH)
        .map(|(t, c)| (t, c.id))
        .collect();
    for (track, id) in targets {
        let new_id = project.new_clip_id();
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        let Some(c) = t.clips.iter_mut().find(|c| c.id == id) else {
            continue;
        };
        let right = split_off(c, at, new_id);
        t.clips.push(right);
        t.clips.sort_by_key(|c| (c.start, c.id));
        created.push(new_id);
    }
    created
}

/// Cuts every clip of `tracks` at each of `cuts` (sorted), in one pass: what [`split`] does
/// once per point, for many points.
pub fn split_at(project: &mut Project, tracks: &[TrackId], cuts: &[u64]) {
    for &track in tracks {
        let Some(clips) = project
            .track_mut(track)
            .map(|t| std::mem::take(&mut t.clips))
        else {
            continue;
        };
        let mut out = Vec::with_capacity(clips.len() + cuts.len());
        for mut piece in clips {
            let first = cuts.partition_point(|&at| at <= piece.start);
            for &at in &cuts[first..] {
                if at >= piece.end() {
                    break;
                }
                if at - piece.start < MIN_LENGTH || piece.end() - at < MIN_LENGTH {
                    continue;
                }
                let right = split_off(&mut piece, at, project.new_clip_id());
                out.push(std::mem::replace(&mut piece, right));
            }
            out.push(piece);
        }
        out.sort_by_key(|c| (c.start, c.id));
        if let Some(t) = project.track_mut(track) {
            t.clips = out;
        }
    }
}

/// Can `b`, which follows `a`, join it: the same audio still in its original place.
fn joinable(a: &Clip, b: &Clip) -> bool {
    a.notes.is_none()
        && b.notes.is_none()
        && a.source == b.source
        && a.stretch == b.stretch
        && a.pitch == b.pitch
        && a.gain_db == b.gain_db
        && a.muted == b.muted
        && b.start >= a.end()
        && i128::from(b.offset) - i128::from(b.start) == i128::from(a.offset) - i128::from(a.start)
}

/// Heal: joins neighbouring clips of `tracks` that still play the same audio in its original
/// place (cut apart and not moved), filling any gap between them, where they meet within
/// `start..end`. Returns how many joins were made.
pub fn heal(project: &mut Project, tracks: &[TrackId], start: u64, end: u64) -> usize {
    let mut healed = 0;
    for &track in tracks {
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        let mut clips = std::mem::take(&mut t.clips);
        clips.sort_by_key(|c| (c.start, c.id));
        let mut out: Vec<Clip> = Vec::with_capacity(clips.len());
        for c in clips {
            if let Some(prev) = out.last_mut()
                && joinable(prev, &c)
                && prev.end() >= start
                && c.start <= end
            {
                prev.length = c.end() - prev.start;
                prev.fade_out = c.fade_out;
                prev.fade_out_shape = c.fade_out_shape;
                healed += 1;
                continue;
            }
            out.push(c);
        }
        t.clips = out;
    }
    healed
}

/// What Strip Silence keeps of a clip once the silences are found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StripMode {
    /// The parts that hold sound.
    #[default]
    Strip,
    /// The silent parts.
    Extract,
    /// Every part: the clip is only cut at the edges of the silences.
    Separate,
}

/// Cuts `clip` at the edges of `audio` (timeline ranges that hold sound) and keeps the pieces
/// `mode` asks for. Returns the clips left.
pub fn strip_clip(
    project: &mut Project,
    clip: ClipId,
    audio: &[(u64, u64)],
    mode: StripMode,
) -> Vec<ClipId> {
    let Some(c) = self::clip(project, clip) else {
        return Vec::new();
    };
    let (from, to) = (c.start, c.end());
    let mut cuts: Vec<u64> = audio
        .iter()
        .flat_map(|&(a, b)| [a, b])
        .filter(|&at| at > from && at < to)
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut pieces = vec![clip];
    for at in cuts {
        let created = split(project, at, &pieces);
        pieces.extend(created);
    }
    let sounding = |c: &Clip| {
        let mid = c.start + c.length / 2;
        audio.iter().any(|&(a, b)| mid >= a && mid < b)
    };
    let remove: Vec<ClipId> = pieces
        .iter()
        .filter_map(|id| self::clip(project, *id))
        .filter(|c| match mode {
            StripMode::Strip => !sounding(c),
            StripMode::Extract => sounding(c),
            StripMode::Separate => false,
        })
        .map(|c| c.id)
        .collect();
    delete_clips(project, &remove, false);
    pieces.retain(|id| !remove.contains(id));
    pieces
}

/// Beat Detective's conform: moves the audio clips of `tracks` that start within `start..end`
/// toward `target(clip start)` by `strength` (0 to 1), then joins each to the next one (a gap
/// is filled with more of its audio, an overlap is cut) with fades of `fade` samples.
pub fn conform(
    project: &mut Project,
    tracks: &[TrackId],
    start: u64,
    end: u64,
    target: impl Fn(u64) -> u64,
    strength: f64,
    fade: u64,
) {
    let strength = strength.clamp(0.0, 1.0);
    for &track in tracks {
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        let mut moved = std::collections::HashSet::new();
        for c in t.clips.iter_mut().filter(|c| c.notes.is_none()) {
            if c.start < start || c.start >= end {
                continue;
            }
            let to = target(c.start) as f64;
            let new = (c.start as f64 + (to - c.start as f64) * strength)
                .round()
                .max(0.0) as u64;
            c.start = new;
            moved.insert(c.id);
        }
        t.clips.sort_by_key(|c| (c.start, c.id));
        let Some(t) = project.track(track) else {
            continue;
        };
        let audio: Vec<usize> = (0..t.clips.len())
            .filter(|&i| t.clips[i].notes.is_none())
            .collect();
        // Each moved slice ends where the next starts (as `trim_end` does), crossfaded.
        let joins: Vec<(usize, usize, u64)> = audio
            .windows(2)
            .filter(|w| moved.contains(&t.clips[w[0]].id) || moved.contains(&t.clips[w[1]].id))
            .map(|w| {
                let (a, b) = (&t.clips[w[0]], &t.clips[w[1]]);
                let latest = a.start + source_length(project, a).saturating_sub(a.offset);
                let end = b
                    .start
                    .clamp(a.start + MIN_LENGTH, latest.max(a.start + MIN_LENGTH));
                (w[0], w[1], end)
            })
            .collect();
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        for (a, b, end) in joins {
            let c = &mut t.clips[a];
            c.length = end - c.start;
            c.fade_out = fade;
            clamp_fades(c);
            let c = &mut t.clips[b];
            c.fade_in = fade;
            clamp_fades(c);
        }
    }
}

/// Copies of the clips `ids`, with their track.
fn with_tracks(project: &Project, ids: &[ClipId]) -> Vec<(TrackId, Clip)> {
    ids.iter()
        .filter_map(|&id| {
            locate(project, id)
                .map(|(t, c)| (project.tracks[t].id, project.tracks[t].clips[c].clone()))
        })
        .collect()
}

/// Removes clips. With `shuffle`, later clips on the same track move left to close the gap.
pub fn delete_clips(project: &mut Project, ids: &[ClipId], shuffle: bool) {
    let mut removed = with_tracks(project, ids);
    removed.sort_by_key(|(_, c)| std::cmp::Reverse(c.start));
    for (track, gone) in removed {
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        t.clips.retain(|c| c.id != gone.id);
        if shuffle {
            for c in t.clips.iter_mut().filter(|c| c.start >= gone.end()) {
                c.start -= gone.length;
            }
        }
    }
}

/// Copies clips right after the end of the selection, on the same tracks. Returns the copies.
pub fn duplicate(project: &mut Project, ids: &[ClipId]) -> Vec<ClipId> {
    let selected = with_tracks(project, ids);
    let (Some(first), Some(last)) = (
        selected.iter().map(|(_, c)| c.start).min(),
        selected.iter().map(|(_, c)| c.end()).max(),
    ) else {
        return Vec::new();
    };
    let span = last - first;
    let mut created = Vec::new();
    let mut tracks = Vec::new();
    for (track, mut copy) in selected {
        copy.id = project.new_clip_id();
        copy.start += span;
        created.push(copy.id);
        if let Some(t) = project.track_mut(track) {
            t.clips.push(copy);
        }
        if !tracks.contains(&track) {
            tracks.push(track);
        }
    }
    for track in tracks {
        sort_track(project, track);
        resolve_overlaps(project, track, &created);
    }
    created
}

pub fn add_marker(project: &mut Project, position: u64) -> MarkerId {
    let id = project.new_marker_id();
    let name = format!("Marker {}", project.markers.len() + 1);
    project.markers.push(Marker { id, position, name });
    project.markers.sort_by_key(|m| (m.position, m.id));
    id
}

pub fn remove_marker(project: &mut Project, id: MarkerId) {
    project.markers.retain(|m| m.id != id);
}

pub fn next_marker(project: &Project, position: u64) -> Option<u64> {
    project
        .markers
        .iter()
        .map(|m| m.position)
        .find(|&p| p > position)
}

pub fn previous_marker(project: &Project, position: u64) -> Option<u64> {
    project
        .markers
        .iter()
        .rev()
        .map(|m| m.position)
        .find(|&p| p < position)
}

/// Starts an empty playlist on a track, keeping the current one as an alternate.
pub fn new_playlist(project: &mut Project, track: TrackId) {
    let Some(t) = project.track_mut(track) else {
        return;
    };
    let name = format!("{}.{:02}", t.name, t.alternates.len() + 2);
    let previous = Playlist {
        name: std::mem::replace(&mut t.playlist, name),
        clips: std::mem::take(&mut t.clips),
    };
    t.alternates.push(previous);
}

/// Switches a track to its next playlist.
pub fn next_playlist(project: &mut Project, track: TrackId) {
    let Some(t) = project.track_mut(track) else {
        return;
    };
    if t.alternates.is_empty() {
        return;
    }
    let next = t.alternates.remove(0);
    let current = Playlist {
        name: std::mem::replace(&mut t.playlist, next.name),
        clips: std::mem::replace(&mut t.clips, next.clips),
    };
    t.alternates.push(current);
}

/// Replaces the controller changes of a MIDI clip.
pub fn set_controls(
    project: &mut Project,
    id: ClipId,
    mut points: Vec<crate::model::ControlPoint>,
) {
    points.sort_by_key(|p| (p.time, p.controller, p.channel));
    if let Some(c) = clip_mut(project, id)
        && c.is_midi()
    {
        c.controls = (!points.is_empty()).then(|| std::sync::Arc::new(points));
    }
}

/// Replaces the notes of a MIDI clip.
pub fn set_notes(project: &mut Project, id: ClipId, mut notes: Vec<Note>) {
    notes.sort_by_key(|n| (n.start, n.key));
    if let Some(c) = clip_mut(project, id)
        && c.notes.is_some()
    {
        c.notes = Some(std::sync::Arc::new(notes));
    }
}

/// Gives overlapping notes their own MIDI channel (2 to 16, the MPE member channels), so each
/// can be bent and shaped alone; the lowest free channel is taken, the oldest reused if all 15
/// are busy. Works on notes sorted by start.
pub fn spread_mpe_channels(notes: &mut [Note]) {
    let mut ends = [0u64; 15];
    for note in notes.iter_mut() {
        let free = (0..15).find(|&c| ends[c] <= note.start);
        let c = free.unwrap_or_else(|| (0..15).min_by_key(|&c| ends[c]).unwrap_or(0));
        ends[c] = note.start + note.length;
        note.channel = c as u8 + 1;
    }
}

/// Moves note starts towards the grid (`strength` 0 to 1); `origin` is the project time of the
/// clip content's start, so the grid follows the project, not the clip.
pub fn quantize(
    notes: &mut [Note],
    origin: u64,
    beats: Option<f64>,
    tempo: &TempoMap,
    rate: u32,
    strength: f64,
) {
    for note in notes.iter_mut() {
        let at = origin + note.start;
        let target = snap(at, beats, tempo, rate);
        let moved = at as f64 + (target as f64 - at as f64) * strength.clamp(0.0, 1.0);
        note.start = (moved.round() as u64).saturating_sub(origin);
    }
}

/// Makes the alternate playlist at `index` current; the current one takes its place.
pub fn select_playlist(project: &mut Project, track: TrackId, index: usize) {
    let Some(t) = project.track_mut(track) else {
        return;
    };
    if index >= t.alternates.len() {
        return;
    }
    let next = t.alternates.remove(index);
    let current = Playlist {
        name: std::mem::replace(&mut t.playlist, next.name),
        clips: std::mem::replace(&mut t.clips, next.clips),
    };
    t.alternates.insert(index, current);
}

/// Comping: copies a clip of an alternate playlist into the current one, replacing what it
/// covers there.
pub fn promote_take(
    project: &mut Project,
    track: TrackId,
    playlist: usize,
    clip: ClipId,
) -> Option<ClipId> {
    let t = project.track(track)?;
    let take = t
        .alternates
        .get(playlist)?
        .clips
        .iter()
        .find(|c| c.id == clip)?
        .clone();
    let alternates = t.alternates.clone();
    let id = place_take(project, track, take);
    if let Some(t) = project.track_mut(track) {
        t.alternates = alternates;
    }
    Some(id)
}

/// Swipe comping: the current playlist plays the alternate at `playlist` between `start` and
/// `end`, silence included. Returns the clips copied in.
pub fn comp_range(
    project: &mut Project,
    track: TrackId,
    playlist: usize,
    start: u64,
    end: u64,
) -> Vec<ClipId> {
    let Some(alternate) = project
        .track(track)
        .and_then(|t| t.alternates.get(playlist))
        .map(|a| a.clips.clone())
    else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    // Cut the take with the usual split rules on a scratch copy of the track.
    let mut scratch = project.clone();
    if let Some(t) = scratch.track_mut(track) {
        t.clips = alternate;
    }
    let inside = split_range(&mut scratch, &[track], start, end);
    let pieces: Vec<Clip> = scratch
        .track(track)
        .map(|t| {
            t.clips
                .iter()
                .filter(|c| inside.contains(&c.id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    delete_range(project, &[track], start, end, false);
    let mut ids = Vec::new();
    for mut piece in pieces {
        piece.id = project.new_clip_id();
        ids.push(piece.id);
        if let Some(t) = project.track_mut(track) {
            t.clips.push(piece);
        }
    }
    if let Some(t) = project.track_mut(track) {
        t.clips.sort_by_key(|c| (c.start, c.id));
    }
    ids
}

/// Name of the take lane holding what a track played before its first take.
pub const BEFORE_TAKES: &str = "Before recording";

/// Length of the crossfades at the edges of what is comped in.
const COMP_CROSSFADE_SECONDS: f64 = 0.01;

/// Records an audio take on a track: it replaces what it covers in the track's clips (the
/// comp), and is kept whole in a take lane of its own, from which any part of it can be comped
/// back at any time, the latest take as the others. What the first take covered gets a lane
/// too. The lanes show once there is a choice. Returns the take's id in the comp.
pub fn record_take(project: &mut Project, track: TrackId, take: Clip) -> ClipId {
    let Some(t) = project.track(track) else {
        return ClipId(0);
    };
    let covered: Vec<Clip> = t
        .clips
        .iter()
        .filter(|c| c.start < take.end() && take.start < c.end())
        .cloned()
        .collect();
    let first = t.takes.is_empty();
    // The comp plays the take where it was recorded; the playlists are left as they were.
    let alternates = t.alternates.clone();
    let id = place_take(project, track, take);
    let placed = clip(project, id).cloned();
    let mut next = project.peek_id();
    if let Some(t) = project.track_mut(track) {
        t.alternates = alternates;
        if first && !covered.is_empty() {
            let mut before = covered;
            for c in &mut before {
                c.id = ClipId(next);
                next += 1;
            }
            t.takes.push(Playlist {
                name: BEFORE_TAKES.to_owned(),
                clips: before,
            });
        }
        if let Some(mut lane) = placed {
            lane.id = ClipId(next);
            next += 1;
            let number = t.takes.iter().filter(|l| l.name != BEFORE_TAKES).count() + 1;
            t.takes.push(Playlist {
                name: format!("Take {number}"),
                clips: vec![lane],
            });
        }
        if t.takes.len() >= 2 {
            t.show_takes = true;
        }
    }
    project.reserve_ids(next);
    id
}

/// Comps `start..end` of take lane `lane` into the track: the take plays there instead of what
/// the comp had, silence included, with short crossfades across both edges so that no cut
/// clicks. The lanes are left untouched: anything can be comped back. Returns the clips copied
/// in.
pub fn comp_take(
    project: &mut Project,
    track: TrackId,
    lane: usize,
    start: u64,
    end: u64,
) -> Vec<ClipId> {
    let Some(take) = project
        .track(track)
        .and_then(|t| t.takes.get(lane))
        .map(|l| l.clips.clone())
    else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    let full = (COMP_CROSSFADE_SECONDS * f64::from(project.sample_rate)) as u64;
    // Half of each crossfade on either side of an edge; none in a range too short for it.
    let half = if end - start > 2 * full { full / 2 } else { 0 };
    let (from, to) = (start.saturating_sub(half), end + half);
    // The take, cut with the usual split rules on a scratch copy of the track.
    let mut scratch = project.clone();
    if let Some(t) = scratch.track_mut(track) {
        t.clips = take;
    }
    let inside = split_range(&mut scratch, &[track], from, to);
    let pieces: Vec<Clip> = scratch
        .track(track)
        .map(|t| {
            t.clips
                .iter()
                .filter(|c| inside.contains(&c.id))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    // What the comp played across each edge goes on under the take's crossfade.
    let across = |at: u64| -> Vec<(SourceId, i128)> {
        project
            .track(track)
            .map(|t| {
                t.clips
                    .iter()
                    .filter(|c| !c.is_midi() && c.start < at && at < c.end())
                    .map(|c| (c.source, i128::from(c.start) - i128::from(c.offset)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let (before, after) = (across(from), across(to));
    // The comp makes room over the whole range, crossfades included.
    delete_range(project, &[track], from, to, false);
    if half > 0 {
        let frames: std::collections::BTreeMap<SourceId, u64> = project
            .sources
            .iter()
            .map(|(id, s)| (*id, s.frames))
            .collect();
        if let Some(t) = project.track_mut(track) {
            for c in &mut t.clips {
                let line = (c.source, i128::from(c.start) - i128::from(c.offset));
                let room = frames.get(&c.source).copied().unwrap_or(u64::MAX);
                // Its material goes on past the edge (unstretched, still within its file).
                if c.end() == from
                    && before.contains(&line)
                    && c.stretch == 1.0
                    && c.offset + c.length + 2 * half <= room
                {
                    c.length += 2 * half;
                    c.fade_out = 2 * half;
                    clamp_fades(c);
                }
                if c.start == to
                    && after.contains(&line)
                    && c.stretch == 1.0
                    && c.offset >= 2 * half
                {
                    c.start -= 2 * half;
                    c.offset -= 2 * half;
                    c.length += 2 * half;
                    c.fade_in = 2 * half;
                    clamp_fades(c);
                }
            }
        }
    }
    let mut ids = Vec::new();
    for mut piece in pieces {
        if half > 0 {
            if piece.start == from {
                piece.fade_in = piece.fade_in.max(2 * half);
            }
            if piece.end() == to {
                piece.fade_out = piece.fade_out.max(2 * half);
            }
            clamp_fades(&mut piece);
        }
        piece.id = project.new_clip_id();
        ids.push(piece.id);
        if let Some(t) = project.track_mut(track) {
            t.clips.push(piece);
        }
    }
    sort_track(project, track);
    ids
}

/// Comps a whole take lane into the track, over all it covers.
pub fn comp_whole_take(project: &mut Project, track: TrackId, lane: usize) -> Vec<ClipId> {
    let Some((start, end)) = project
        .track(track)
        .and_then(|t| t.takes.get(lane))
        .and_then(|l| {
            let start = l.clips.iter().map(|c| c.start).min()?;
            let end = l.clips.iter().map(Clip::end).max()?;
            Some((start, end))
        })
    else {
        return Vec::new();
    };
    comp_take(project, track, lane, start, end)
}

/// Removes a take lane; what the comp took from it stays.
pub fn remove_take(project: &mut Project, track: TrackId, lane: usize) {
    if let Some(t) = project.track_mut(track)
        && lane < t.takes.len()
    {
        t.takes.remove(lane);
        if t.takes.is_empty() {
            t.show_takes = false;
        }
    }
}

/// Whether a clip of the comp plays a lane's clip at its place: the same material, aligned.
fn plays(comp: &Clip, lane: &Clip) -> bool {
    !comp.is_midi()
        && comp.source == lane.source
        && comp.stretch == lane.stretch
        && comp.pitch == lane.pitch
        && i128::from(comp.start) - i128::from(comp.offset)
            == i128::from(lane.start) - i128::from(lane.offset)
        && comp.start.max(lane.start) < comp.end().min(lane.end())
}

/// What the comp plays of each take lane: per lane, the ranges of it the track's clips play.
pub fn comp_sources(track: &Track) -> Vec<Vec<(u64, u64)>> {
    track
        .takes
        .iter()
        .map(|lane| {
            let mut ranges = Vec::new();
            for c in &track.clips {
                for l in lane.clips.iter().filter(|l| plays(c, l)) {
                    ranges.push((c.start.max(l.start), c.end().min(l.end())));
                }
            }
            ranges.sort_unstable();
            ranges
        })
        .collect()
}

/// The take lane a clip of the comp comes from, if any.
pub fn take_of(track: &Track, clip: &Clip) -> Option<usize> {
    track
        .takes
        .iter()
        .position(|lane| lane.clips.iter().any(|l| plays(clip, l)))
}

/// Places a recorded clip, keeping what it covers in a new playlist ("take") and replacing the
/// covered material in the current one.
pub fn place_take(project: &mut Project, track: TrackId, mut take: Clip) -> ClipId {
    take.id = project.new_clip_id();
    let id = take.id;
    let mut next_id = project.peek_id();
    let covers_something = project.track(track).is_some_and(|t| {
        t.clips
            .iter()
            .any(|c| c.start < take.end() && take.start < c.end())
    });
    if let Some(t) = project.track_mut(track) {
        if covers_something {
            let name = format!("{} take {}", t.name, t.alternates.len() + 1);
            t.alternates.push(Playlist {
                name,
                clips: t.clips.clone(),
            });
        }
        t.clips.push(take);
        t.clips.sort_by_key(|c| (c.start, c.id));
    }
    // Recorded material replaces what it covers, even at its edges (no crossfade).
    let Some(t) = project.track_mut(track) else {
        return id;
    };
    let placed = t.clips.iter().find(|c| c.id == id).cloned();
    if let Some(p) = placed {
        let mut kept = Vec::new();
        for mut o in std::mem::take(&mut t.clips) {
            if o.id == id || o.end() <= p.start || o.start >= p.end() {
                kept.push(o);
            } else if o.start < p.start && o.end() > p.end() {
                let right = split_off(&mut o, p.end(), ClipId(next_id));
                next_id = next_id.saturating_add(1);
                o.length = p.start - o.start;
                clamp_fades(&mut o);
                kept.push(o);
                kept.push(right);
            } else if o.start < p.start {
                o.length = p.start - o.start;
                clamp_fades(&mut o);
                kept.push(o);
            } else if o.end() > p.end() {
                let cut = p.end() - o.start;
                o.offset += cut;
                o.start = p.end();
                o.length -= cut;
                clamp_fades(&mut o);
                kept.push(o);
            }
        }
        t.clips = kept;
    }
    project.reserve_ids(next_id);
    sort_track(project, track);
    id
}

/// Nearest grid line to `position`. `beats` is the grid step in quarter notes, or `None` for
/// whole bars.
pub fn snap(position: u64, beats: Option<f64>, tempo: &TempoMap, rate: u32) -> u64 {
    let beat = tempo.sample_to_beat(position, rate);
    let snapped = match beats {
        Some(step) if step > 0.0 => (beat / step).round() * step,
        _ => {
            let bbt = tempo.beat_to_bbt(beat);
            let bar_start = tempo.bbt_to_beat(crate::tempo::BarBeatTick {
                bar: bbt.bar,
                beat: 1,
                tick: 0,
            });
            let next_bar = tempo.bbt_to_beat(crate::tempo::BarBeatTick {
                bar: bbt.bar + 1,
                beat: 1,
                tick: 0,
            });
            if beat - bar_start < next_bar - beat {
                bar_start
            } else {
                next_bar
            }
        }
    };
    tempo.beat_to_sample(snapped.max(0.0), rate)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::Source;

    fn project() -> (Project, TrackId, Vec<ClipId>) {
        let mut p = Project::new("t", 48_000);
        let s = p.add_source(Source {
            path: PathBuf::from("a.wav"),
            channels: 1,
            sample_rate: 48_000,
            frames: 10_000,
        });
        let t = p.add_track("a");
        let a = p.add_clip(t, s, 0).unwrap();
        let b = p.add_clip(t, s, 20_000).unwrap();
        (p, t, vec![a, b])
    }

    #[test]
    fn heal_joins_what_a_split_cut_apart() {
        let (mut p, t, ids) = project();
        // Cut the first clip in three, take out the middle: the outer pieces still line up.
        let right = split(&mut p, 3000, &[ids[0]]);
        let last = split(&mut p, 6000, &right);
        delete_clips(&mut p, &right, false);
        assert_eq!(clips(&p, t).len(), 3);
        // A moved clip does not heal.
        let moved = split(&mut p, 25_000, &[ids[1]]);
        move_clips(&mut p, &moved, 100, 0);
        assert_eq!(heal(&mut p, &[t], 0, 40_000), 1);
        let after = clips(&p, t);
        assert_eq!(after[0].0, 0);
        assert_eq!(
            after[0].1, 10_000,
            "the gap is filled with the original audio"
        );
        assert_eq!(after.len(), 3);
        assert!(last.len() == 1);
    }

    #[test]
    fn strip_keeps_extracts_or_separates() {
        let audio = [(1000, 3000), (6000, 8000)];
        for (mode, lengths) in [
            (StripMode::Strip, vec![2000, 2000]),
            (StripMode::Extract, vec![1000, 3000, 2000]),
            (StripMode::Separate, vec![1000, 2000, 3000, 2000, 2000]),
        ] {
            let (mut p, t, ids) = project();
            strip_clip(&mut p, ids[0], &audio, mode);
            let left: Vec<u64> = clips(&p, t)
                .iter()
                .filter(|c| c.0 < 10_000)
                .map(|c| c.1)
                .collect();
            assert_eq!(left, lengths, "{mode:?}");
        }
    }

    #[test]
    fn split_at_cuts_every_clip_in_one_pass() {
        let (mut p, t, _) = project();
        // Too close to an edge or to the previous cut, or between clips: skipped.
        split_at(&mut p, &[t], &[5, 3000, 3010, 6000, 9995, 15_000, 25_000]);
        let c = clips(&p, t);
        assert_eq!(
            c.iter().map(|c| (c.0, c.1, c.2)).collect::<Vec<_>>(),
            [
                (0, 3000, 0),
                (3000, 3000, 3000),
                (6000, 4000, 6000),
                (20_000, 5000, 0),
                (25_000, 5000, 5000)
            ]
        );
    }

    #[test]
    fn conform_moves_slices_and_joins_them() {
        let (mut p, t, ids) = project();
        // Slices at 0, 2900 and 6100 of the first clip; the grid is every 3000 samples.
        let right = split(&mut p, 2900, &[ids[0]]);
        split(&mut p, 6100, &right);
        conform(
            &mut p,
            &[t],
            0,
            10_000,
            |at| (at + 1500) / 3000 * 3000,
            1.0,
            32,
        );
        let c = clips(&p, t);
        assert_eq!(
            c.iter().map(|c| c.0).take(3).collect::<Vec<_>>(),
            [0, 3000, 6000]
        );
        // Each slice now reaches the next: the first grew (its source goes on), the second
        // was cut where the third starts.
        assert_eq!((c[0].1, c[1].1), (3000, 3000));
        assert_eq!((c[0].4, c[1].3), (32, 32), "joined with fades");
        // Half strength goes half way.
        let (mut p, t, ids) = project();
        split(&mut p, 2900, &[ids[0]]);
        conform(
            &mut p,
            &[t],
            0,
            10_000,
            |at| (at + 1500) / 3000 * 3000,
            0.5,
            0,
        );
        assert_eq!(clips(&p, t)[1].0, 2950);
    }

    #[test]
    fn ranges_delete_and_insert_time() {
        let (mut p, t, _) = project();
        let other = p.add_track("b");
        let s = *p.sources.keys().next().unwrap();
        p.add_clip(other, s, 0);
        // Clips at 0..10000 and 20000..30000 on `t`, 0..10000 on `other`.
        delete_range(&mut p, &[t], 4000, 24_000, false);
        assert_eq!(
            clips(&p, t).iter().map(|c| (c.0, c.1)).collect::<Vec<_>>(),
            vec![(0, 4000), (24_000, 6000)]
        );
        assert_eq!(
            p.track(other).unwrap().clips[0].length,
            10_000,
            "other track untouched"
        );
        delete_range(&mut p, &[t], 4000, 24_000, true);
        assert_eq!(clips(&p, t)[1].0, 4000, "ripple closes the gap");
        let m = add_marker(&mut p, 5000);
        let lane = crate::automation::lane_mut(
            p.track_mut(t).unwrap(),
            &crate::automation::AutomationTarget::Gain,
        );
        lane.add(1000, 0.0, crate::automation::CurveShape::Linear);
        lane.add(2000, -6.0, crate::automation::CurveShape::Linear);
        insert_silence(&mut p, 2000, 1000);
        let points = &p.track(t).unwrap().automation[0].points;
        assert_eq!((points[0].time, points[1].time), (1000, 3000));
        assert_eq!(
            clips(&p, t).iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![0, 3000, 5000]
        );
        assert_eq!(p.markers.iter().find(|x| x.id == m).unwrap().position, 6000);
        assert_eq!(
            p.track(other).unwrap().clips.len(),
            2,
            "split on every track"
        );
    }

    #[test]
    fn stretching_scales_the_clip_window() {
        let (mut p, t, ids) = project();
        trim_start(&mut p, ids[0], 1000);
        stretch_clip(&mut p, ids[0], 18_000);
        let c = &p.track(t).unwrap().clips[0];
        assert_eq!((c.stretch, c.offset, c.length), (2.0, 2000, 18_000));
        assert_eq!(c.variant(), Some((2.0, 0.0)));
        // The stretched source is twice as long: trimming can reach its new end.
        trim_end(&mut p, ids[0], 1000 + 18_000 + 5000);
        assert_eq!(p.track(t).unwrap().clips[0].length, 18_000);
        set_pitch(&mut p, &ids, 3.0);
        assert_eq!(p.track(t).unwrap().clips[1].pitch, 3.0);
        let path = std::path::Path::new("/a/b.wav");
        let v = crate::model::variant_path(path, (1.5, -2.0));
        assert_eq!(
            crate::model::parse_variant(&v),
            (path.to_path_buf(), Some((1.5, -2.0)))
        );
        assert_eq!(
            crate::model::parse_variant(path),
            (path.to_path_buf(), None)
        );
    }

    #[test]
    fn duplicated_track_gets_fresh_ids() {
        let (mut p, t, ids) = project();
        let insert = crate::routing::add_insert(
            &mut p,
            crate::routing::Channel::Track(t),
            "eq",
            Default::default(),
        )
        .unwrap();
        p.track_mut(t)
            .unwrap()
            .automation
            .push(crate::automation::AutomationLane::new(
                crate::automation::AutomationTarget::Param {
                    insert,
                    param: "gain".into(),
                },
            ));
        let other = p.add_track("b");
        let copy = duplicate_track(&mut p, t).unwrap();
        let order: Vec<_> = p.tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, [t, copy, other]);
        let c = p.track(copy).unwrap();
        assert_eq!(c.name, "a (copy)");
        assert_eq!(c.clips.len(), 2);
        assert!(c.clips.iter().all(|clip| !ids.contains(&clip.id)));
        assert_ne!(c.inserts[0].id, insert);
        assert!(matches!(
            &c.automation[0].target,
            crate::automation::AutomationTarget::Param { insert: i, .. } if *i == c.inserts[0].id
        ));
    }

    fn clips(p: &Project, t: TrackId) -> Vec<(u64, u64, u64, u64, u64)> {
        p.track(t)
            .unwrap()
            .clips
            .iter()
            .map(|c| (c.start, c.length, c.offset, c.fade_in, c.fade_out))
            .collect()
    }

    #[test]
    fn moving_across_an_edge_crossfades() {
        let (mut p, t, ids) = project();
        move_clips(&mut p, &[ids[1]], -12_000, 0);
        assert_eq!(
            clips(&p, t),
            vec![(0, 10_000, 0, 0, 2_000), (8_000, 10_000, 0, 2_000, 0)]
        );
    }

    #[test]
    fn crossfades_follow_the_overlap_as_in_reaper() {
        let (mut p, t, ids) = project();
        // Moved in, then back a little, then away: the crossfade grows, shrinks and goes.
        move_clips(&mut p, &[ids[1]], -12_000, 0);
        move_clips(&mut p, &[ids[1]], 1_000, 0);
        assert_eq!(
            clips(&p, t),
            vec![(0, 10_000, 0, 0, 1_000), (9_000, 10_000, 0, 1_000, 0)]
        );
        move_clips(&mut p, &[ids[1]], 5_000, 0);
        assert_eq!(
            clips(&p, t),
            vec![(0, 10_000, 0, 0, 0), (14_000, 10_000, 0, 0, 0)]
        );
        // Trimmed into its neighbour, a clip crossfades with it.
        trim_end(&mut p, ids[0], 6_000);
        move_clips(&mut p, &[ids[1]], -6_000, 0);
        let before = clips_around(&p, &[ids[0]]);
        trim_end(&mut p, ids[0], 10_000);
        crossfade_after(&mut p, &[ids[0]], &before);
        assert_eq!(
            clips(&p, t),
            vec![(0, 10_000, 0, 0, 2_000), (8_000, 10_000, 0, 2_000, 0)]
        );
        // Slid, the crossfade keeps its length, as far as both clips have material.
        let before = clips_around(&p, &[ids[1]]);
        trim_start(&mut p, ids[1], 9_000);
        crossfade_after(&mut p, &[ids[1]], &before);
        move_crossfade(&mut p, ids[0], ids[1], -1_500);
        assert_eq!(
            clips(&p, t),
            vec![(0, 9_000, 0, 0, 1_000), (8_000, 10_000, 0, 1_000, 0)]
        );
    }

    #[test]
    fn covering_a_clip_replaces_it() {
        let (mut p, t, ids) = project();
        trim_end(&mut p, ids[0], 3_000);
        move_clips(&mut p, &[ids[0]], 24_000, 0);
        // The short clip lands inside the second one, which is split around it.
        assert_eq!(
            clips(&p, t),
            vec![
                (20_000, 4_000, 0, 0, 0),
                (24_000, 3_000, 0, 0, 0),
                (27_000, 3_000, 7_000, 0, 0)
            ]
        );
        // Dropped at the very start of a clip, it replaces the beginning (no full-length fade).
        move_clips(&mut p, &[ids[0]], -4_000, 0);
        let all = clips(&p, t);
        assert!(all.contains(&(20_000, 3_000, 0, 0, 0)));
        assert!(
            all.iter().all(|c| c.3 == 0 && c.4 == 0),
            "no crossfade: {all:?}"
        );
        // Moving it fully over the remaining pieces removes what it covers.
        let unique: std::collections::HashSet<_> =
            p.track(t).unwrap().clips.iter().map(|c| c.id).collect();
        assert_eq!(unique.len(), p.track(t).unwrap().clips.len());
    }

    #[test]
    fn moves_between_tracks_and_clamps_at_zero() {
        let (mut p, t, ids) = project();
        let u = p.add_track("b");
        move_clips(&mut p, &[ids[1]], -50_000, 1);
        assert_eq!(p.track(t).unwrap().clips.len(), 1);
        assert_eq!(p.track(u).unwrap().clips[0].start, 0);
    }

    #[test]
    fn trims_stay_within_source_material() {
        let (mut p, t, ids) = project();
        trim_start(&mut p, ids[1], 22_000);
        assert_eq!(clips(&p, t)[1], (22_000, 8_000, 2_000, 0, 0));
        trim_start(&mut p, ids[1], 0);
        assert_eq!(
            clips(&p, t)[1],
            (20_000, 10_000, 0, 0, 0),
            "cannot reveal before the source start"
        );
        trim_end(&mut p, ids[1], 100_000);
        assert_eq!(
            clips(&p, t)[1].1,
            10_000,
            "cannot extend past the source end"
        );
        trim_end(&mut p, ids[1], 20_001);
        assert_eq!(clips(&p, t)[1].1, MIN_LENGTH);
    }

    #[test]
    fn split_and_delete_with_shuffle() {
        let (mut p, t, ids) = project();
        let right = split(&mut p, 4_000, &[]);
        assert_eq!(right.len(), 1);
        assert_eq!(
            clips(&p, t),
            vec![
                (0, 4_000, 0, 0, 0),
                (4_000, 6_000, 4_000, 0, 0),
                (20_000, 10_000, 0, 0, 0)
            ]
        );
        delete_clips(&mut p, &[ids[0]], true);
        assert_eq!(
            clips(&p, t),
            vec![(0, 6_000, 4_000, 0, 0), (16_000, 10_000, 0, 0, 0)]
        );
        delete_clips(&mut p, &[right[0]], false);
        assert_eq!(clips(&p, t), vec![(16_000, 10_000, 0, 0, 0)]);
    }

    #[test]
    fn duplicate_places_copies_after_the_selection() {
        let (mut p, t, ids) = project();
        let copies = duplicate(&mut p, &ids);
        assert_eq!(copies.len(), 2);
        let starts: Vec<u64> = clips(&p, t).iter().map(|c| c.0).collect();
        assert_eq!(starts, vec![0, 20_000, 30_000, 50_000]);
    }

    #[test]
    fn fades_are_clamped_to_the_clip() {
        let (mut p, _, ids) = project();
        set_fades(&mut p, ids[0], 8_000, 8_000);
        let c = clip(&p, ids[0]).unwrap();
        assert_eq!((c.fade_in, c.fade_out), (8_000, 2_000));
    }

    #[test]
    fn markers_navigation() {
        let (mut p, _, _) = project();
        add_marker(&mut p, 30_000);
        let m = add_marker(&mut p, 10_000);
        assert_eq!(p.markers[0].id, m);
        assert_eq!(next_marker(&p, 10_000), Some(30_000));
        assert_eq!(previous_marker(&p, 30_000), Some(10_000));
        assert_eq!(next_marker(&p, 30_000), None);
        remove_marker(&mut p, m);
        assert_eq!(p.markers.len(), 1);
    }

    #[test]
    fn playlists_rotate() {
        let (mut p, t, _) = project();
        new_playlist(&mut p, t);
        assert!(p.track(t).unwrap().clips.is_empty());
        assert_eq!(p.track(t).unwrap().playlist, "a.02");
        next_playlist(&mut p, t);
        assert_eq!(p.track(t).unwrap().clips.len(), 2);
        assert_eq!(p.track(t).unwrap().playlist, "Main");
    }

    #[test]
    fn playlists_can_be_selected_and_comped() {
        let (mut p, t, ids) = project();
        let mut take = clip(&p, ids[0]).unwrap().clone();
        take.start = 0;
        take.length = 30_000;
        place_take(&mut p, t, take);
        assert_eq!(p.track(t).unwrap().clips.len(), 1);
        let original = ids[1];
        let id = promote_take(&mut p, t, 0, original).unwrap();
        let track = p.track(t).unwrap();
        assert_eq!(track.alternates.len(), 1, "comping keeps the playlists");
        assert_eq!(
            track.clips.len(),
            2,
            "take trimmed before the promoted clip"
        );
        assert!(track.clips.iter().any(|c| c.id == id && c.id != original));
        select_playlist(&mut p, t, 0);
        let track = p.track(t).unwrap();
        assert_eq!(track.clips.len(), 2);
        assert_eq!(track.playlist, "a take 1");
        assert_eq!(track.alternates[0].name, "Main");
        assert!(promote_take(&mut p, t, 5, original).is_none());
    }

    #[test]
    fn swipe_comping_takes_a_range_of_an_alternate() {
        let (mut p, t, ids) = project();
        let original: Vec<(u64, u64)> = p
            .track(t)
            .unwrap()
            .clips
            .iter()
            .map(|c| (c.start, c.end()))
            .collect();
        let mut take = clip(&p, ids[0]).unwrap().clone();
        take.start = 0;
        take.length = 30_000;
        place_take(&mut p, t, take);
        // Back to the original material between 5000 and 12000 only.
        let pieces = comp_range(&mut p, t, 0, 5000, 12_000);
        let track = p.track(t).unwrap();
        assert_eq!(track.alternates.len(), 1);
        assert!(!pieces.is_empty());
        for c in track.clips.iter().filter(|c| pieces.contains(&c.id)) {
            assert!(
                c.start >= 5000 && c.end() <= 12_000,
                "{}..{}",
                c.start,
                c.end()
            );
            assert!(
                original.iter().any(|&(a, b)| a <= c.start && c.end() <= b),
                "comes from the alternate"
            );
        }
        let others: Vec<&Clip> = track
            .clips
            .iter()
            .filter(|c| !pieces.contains(&c.id))
            .collect();
        assert!(others.iter().all(|c| c.end() <= 5000 || c.start >= 12_000));
        assert!(others.iter().any(|c| c.end() == 5000));
        assert!(others.iter().any(|c| c.start == 12_000));
        assert!(comp_range(&mut p, t, 3, 0, 10).is_empty());
    }

    #[test]
    fn takes_replace_covered_material_and_keep_the_previous_version() {
        let (mut p, t, ids) = project();
        let mut take = clip(&p, ids[0]).unwrap().clone();
        take.start = 5_000;
        take.length = 20_000;
        take.offset = 0;
        let id = place_take(&mut p, t, take);
        let track = p.track(t).unwrap();
        assert_eq!(track.alternates.len(), 1);
        assert_eq!(track.alternates[0].clips.len(), 2, "previous version kept");
        assert_eq!(
            clips(&p, t),
            vec![
                (0, 5_000, 0, 0, 0),
                (5_000, 20_000, 0, 0, 0),
                (25_000, 5_000, 5_000, 0, 0)
            ]
        );
        assert!(p.track(t).unwrap().clips.iter().any(|c| c.id == id));
        let unique: std::collections::HashSet<_> =
            p.track(t).unwrap().clips.iter().map(|c| c.id).collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn a_take_inside_a_clip_keeps_the_fades_within_both_sides() {
        let (mut p, t, ids) = project();
        set_fades(&mut p, ids[0], 4_000, 6_000);
        let mut take = clip(&p, ids[0]).unwrap().clone();
        (take.start, take.length, take.fade_in, take.fade_out) = (5_000, 4_000, 0, 0);
        place_take(&mut p, t, take);
        assert_eq!(
            clips(&p, t)[..3],
            [
                (0, 5_000, 0, 4_000, 0),
                (5_000, 4_000, 0, 0, 0),
                (9_000, 1_000, 9_000, 0, 1_000)
            ]
        );
        assert!(p.peek_id() > p.highest_id());
    }

    #[test]
    fn clips_grow_to_the_bar_after_their_last_note() {
        let (mut p, t, _) = project();
        p.tempo = TempoMap::constant(120.0, 4, 4);
        let note = |start| Note {
            start,
            length: 6000,
            key: 60,
            velocity: 100,
            channel: 0,
            ..Default::default()
        };
        let id = p.add_midi_clip(t, 96_000, 96_000, vec![note(0)]).unwrap();
        fit_clip_to_notes(&mut p, id);
        assert_eq!(clip(&p, id).unwrap().length, 96_000, "already fits");
        set_notes(&mut p, id, vec![note(0), note(100_000)]);
        fit_clip_to_notes(&mut p, id);
        assert_eq!(
            clip(&p, id).unwrap().length,
            192_000,
            "to the end of the next bar"
        );
        assert_eq!(track_of_clip(&p, id), Some(t));
    }

    #[test]
    fn midi_clips_keep_their_beats_when_the_tempo_changes() {
        let (mut p, t, audio) = project();
        // 120 BPM at 48 kHz: a beat is 24 000 samples.
        p.tempo = TempoMap::constant(120.0, 4, 4);
        let note = |start, length| Note {
            start,
            length,
            key: 36,
            velocity: 100,
            channel: 9,
            ..Default::default()
        };
        let id = p
            .add_midi_clip(t, 96_000, 96_000, vec![note(0, 6000), note(24_000, 6000)])
            .unwrap();
        set_controls(
            &mut p,
            id,
            vec![crate::model::ControlPoint {
                time: 48_000,
                controller: 1,
                value: 0.5,
                channel: 0,
            }],
        );
        let audio_start = clip(&p, audio[1]).unwrap().start;
        set_tempo(&mut p, TempoMap::constant(60.0, 4, 4));
        let c = clip(&p, id).unwrap();
        assert_eq!((c.start, c.length, c.offset), (192_000, 192_000, 0));
        let notes = c.notes.as_deref().unwrap();
        assert_eq!(
            notes
                .iter()
                .map(|n| (n.start, n.length))
                .collect::<Vec<_>>(),
            [(0, 12_000), (48_000, 12_000)]
        );
        assert_eq!(c.controls.as_deref().unwrap()[0].time, 96_000);
        assert_eq!(
            clip(&p, audio[1]).unwrap().start,
            audio_start,
            "audio stays"
        );
    }

    #[test]
    fn midi_notes_edit_and_quantize() {
        let (mut p, t, _) = project();
        let id = p.add_midi_clip(t, 48_000, 96_000, Vec::new()).unwrap();
        let note = |start| Note {
            start,
            length: 1000,
            key: 60,
            velocity: 100,
            channel: 0,
            ..Default::default()
        };
        set_notes(&mut p, id, vec![note(25_000), note(11_000)]);
        let notes = clip(&p, id).unwrap().notes.clone().unwrap();
        assert_eq!(notes[0].start, 11_000, "sorted");
        let mut copy = (*notes).clone();
        // 120 BPM at 48 kHz: beats every 24000 samples; the clip content starts at 48000.
        quantize(
            &mut copy,
            48_000,
            Some(1.0),
            &TempoMap::constant(120.0, 4, 4),
            48_000,
            1.0,
        );
        assert_eq!(
            copy.iter().map(|n| n.start).collect::<Vec<_>>(),
            vec![0, 24_000]
        );
        let control = |time, value| crate::model::ControlPoint {
            time,
            controller: 1,
            value,
            channel: 0,
        };
        set_controls(&mut p, id, vec![control(900, 1.0), control(100, 0.0)]);
        let controls = clip(&p, id).unwrap().controls.clone().unwrap();
        assert_eq!((controls[0].time, controls[1].time), (100, 900));
        set_controls(&mut p, id, Vec::new());
        assert!(clip(&p, id).unwrap().controls.is_none());
        set_clip_gain(&mut p, id, 99.0);
        assert_eq!(clip(&p, id).unwrap().gain_db, 24.0);
        let halves = split(&mut p, 96_000, &[id]);
        assert_eq!(halves.len(), 1);
        assert!(
            clip(&p, halves[0]).unwrap().is_midi(),
            "split keeps the notes"
        );
    }

    #[test]
    fn snapping_to_beats_and_bars() {
        let tempo = TempoMap::constant(120.0, 4, 4);
        // 120 BPM at 48 kHz: one beat is 24000 samples, one bar 96000.
        assert_eq!(snap(13_000, Some(1.0), &tempo, 48_000), 24_000);
        assert_eq!(snap(11_000, Some(1.0), &tempo, 48_000), 0);
        assert_eq!(snap(13_000, Some(0.25), &tempo, 48_000), 12_000);
        assert_eq!(snap(50_000, None, &tempo, 48_000), 96_000);
        assert_eq!(snap(40_000, None, &tempo, 48_000), 0);
    }

    #[test]
    fn overlapping_notes_get_their_own_mpe_channel() {
        let note = |start, length| Note {
            start,
            length,
            key: 60,
            velocity: 100,
            channel: 0,
            ..Default::default()
        };
        let mut notes = vec![note(0, 100), note(50, 100), note(120, 10), note(200, 10)];
        spread_mpe_channels(&mut notes);
        let channels: Vec<u8> = notes.iter().map(|n| n.channel).collect();
        assert_eq!(channels, vec![1, 2, 1, 1]);
        let mut chord: Vec<Note> = (0..20).map(|_| note(0, 10)).collect();
        spread_mpe_channels(&mut chord);
        assert!(chord.iter().all(|n| (1..=15).contains(&n.channel)));
    }

    #[test]
    fn crossfades_slips_and_range_fades() {
        let (mut p, t, ids) = project();
        // Butted at 8 000, then crossfaded over 2 000 around the cut.
        trim_end(&mut p, ids[0], 8_000);
        if let Some(c) = clip_mut(&mut p, ids[1]) {
            (c.start, c.offset, c.length) = (8_000, 2_000, 8_000);
        }
        set_crossfade(&mut p, ids[0], ids[1], 2_000);
        let all = clips(&p, t);
        assert_eq!(
            all[0].0 + all[0].1,
            9_000,
            "left ends 1 000 past the cut: {all:?}"
        );
        assert_eq!(all[1].0, 7_000);
        assert_eq!((all[0].4, all[1].3), (2_000, 2_000));
        // The audio slides inside the clip, within the source.
        let (mut p, t, ids) = project();
        trim_start(&mut p, ids[0], 2_000);
        slip_content(&mut p, ids[0], 500);
        assert_eq!(clips(&p, t)[0].2, 1_500);
        slip_content(&mut p, ids[0], 10_000);
        assert_eq!(clips(&p, t)[0].2, 0);
        slip_content(&mut p, ids[0], -50_000);
        assert_eq!(clips(&p, t)[0].2, 2_000, "no further than the source's end");
        // A range over a clip's end fades it out, over a start fades it in.
        let (mut p, t, _) = project();
        fades_in_range(&mut p, &[t], 9_000, 21_000);
        let all = clips(&p, t);
        assert_eq!((all[0].4, all[1].3), (1_000, 1_000));
    }

    /// A loop recorded three times over a clip: each pass in a lane of its own, the clip it
    /// covered in a lane too, the comp playing the last pass.
    fn three_takes() -> (Project, TrackId, Vec<ClipId>) {
        let (mut p, t, ids) = project();
        let source = clip(&p, ids[0]).unwrap().clone();
        let mut comp = Vec::new();
        for pass in 0..3u64 {
            let mut take = source.clone();
            // Each take records into a file of its own.
            take.source = p.add_source(Source {
                path: PathBuf::from(format!("take{pass}.wav")),
                channels: 1,
                sample_rate: 48_000,
                frames: 40_000,
            });
            (take.start, take.length, take.offset) = (0, 30_000, pass * 1000);
            (take.fade_in, take.fade_out) = (0, 0);
            comp.push(record_take(&mut p, t, take));
        }
        (p, t, comp)
    }

    #[test]
    fn every_take_keeps_a_lane_of_its_own() {
        let (p, t, comp) = three_takes();
        let track = p.track(t).unwrap();
        let names: Vec<&str> = track.takes.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, [BEFORE_TAKES, "Take 1", "Take 2", "Take 3"]);
        assert!(track.show_takes, "the lanes show once there is a choice");
        assert!(track.alternates.is_empty(), "the playlists are left alone");
        assert_eq!(track.takes[0].clips.len(), 2, "what the first take covered");
        assert!(track.takes[1..].iter().all(|l| l.clips.len() == 1));
        assert_eq!(track.clips.len(), 1);
        assert_eq!(track.clips[0].id, comp[2], "the comp plays the last take");
        assert_eq!(take_of(track, &track.clips[0]), Some(3));
        assert_eq!(comp_sources(track)[3], [(0, 30_000)]);
        assert!(comp_sources(track)[1].is_empty());
        let mut ids: Vec<ClipId> = track
            .clips
            .iter()
            .chain(track.takes.iter().flat_map(|l| &l.clips))
            .map(|c| c.id)
            .collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "every clip has its own id");
        assert!(p.highest_id() >= ids[count - 1].0);
    }

    #[test]
    fn comping_a_range_crossfades_and_loses_no_take() {
        let (mut p, t, _) = three_takes();
        let rate = u64::from(p.sample_rate);
        let half = (COMP_CROSSFADE_SECONDS * rate as f64) as u64 / 2;
        // The second take between 10000 and 20000, over the third.
        let pieces = comp_take(&mut p, t, 2, 10_000, 20_000);
        let track = p.track(t).unwrap();
        assert_eq!(pieces.len(), 1);
        let piece = track.clips.iter().find(|c| c.id == pieces[0]).unwrap();
        assert_eq!((piece.start, piece.end()), (10_000 - half, 20_000 + half));
        assert_eq!((piece.fade_in, piece.fade_out), (2 * half, 2 * half));
        assert_eq!(take_of(track, piece), Some(2));
        // The third take fades out and back in under it.
        let left = track
            .clips
            .iter()
            .find(|c| c.end() == 10_000 + half)
            .unwrap();
        let right = track
            .clips
            .iter()
            .find(|c| c.start == 20_000 - half)
            .unwrap();
        assert_eq!((left.fade_out, right.fade_in), (2 * half, 2 * half));
        assert_eq!(take_of(track, left), Some(3));
        assert_eq!(take_of(track, right), Some(3));
        let sources = comp_sources(track);
        assert_eq!(sources[2], [(10_000 - half, 20_000 + half)]);
        assert_eq!(sources[3], [(0, 10_000 + half), (20_000 - half, 30_000)]);
        // Every take is still whole in its lane.
        assert!(track.takes[1..].iter().all(|l| l.clips[0].length == 30_000));
        // Back to the third take there: comped from its lane.
        comp_take(&mut p, t, 3, 10_000, 20_000);
        let track = p.track(t).unwrap();
        assert!(comp_sources(track)[2].is_empty());
        assert!(!comp_sources(track)[3].is_empty());
        // A whole take, and the original material.
        comp_whole_take(&mut p, t, 1);
        assert_eq!(comp_sources(p.track(t).unwrap())[1], [(0, 30_000)]);
        comp_whole_take(&mut p, t, 0);
        let track = p.track(t).unwrap();
        assert!(comp_sources(track)[1].is_empty());
        assert!(!comp_sources(track)[0].is_empty());
        // Nothing for a lane that is not there, or an empty range.
        assert!(comp_take(&mut p, t, 9, 0, 100).is_empty());
        assert!(comp_take(&mut p, t, 1, 500, 500).is_empty());
    }

    #[test]
    fn a_short_range_is_comped_without_crossfades_and_lanes_can_go() {
        let (mut p, t, _) = three_takes();
        let pieces = comp_take(&mut p, t, 1, 1000, 1100);
        let track = p.track(t).unwrap();
        let piece = track.clips.iter().find(|c| c.id == pieces[0]).unwrap();
        assert_eq!((piece.start, piece.end()), (1000, 1100));
        remove_take(&mut p, t, 1);
        let track = p.track(t).unwrap();
        assert_eq!(track.takes.len(), 3);
        assert!(
            track.clips.iter().any(|c| c.start == 1000),
            "what was comped stays"
        );
        for _ in 0..3 {
            remove_take(&mut p, t, 0);
        }
        assert!(!p.track(t).unwrap().show_takes);
        remove_take(&mut p, t, 0);
    }
}
