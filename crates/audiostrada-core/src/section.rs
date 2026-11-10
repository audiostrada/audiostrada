//! Edits of a section of the whole project in time: removing it (what follows moves back to
//! close the gap), clearing it (the gap stays, silent) or keeping only it (crop). Every track
//! and playlist follows; when time goes, so do the automation, markers, tempo and signature
//! changes, and the loop and punch ranges.

use crate::automation::{AutomationLane, AutomationPoint, CurveShape, value_at};
use crate::edit::{MIN_LENGTH, clamp_fades};
use crate::model::{Clip, ClipId, Project};
use crate::tempo::{TempoMap, TempoPoint, TimeSignature};

/// Fades put on the edges a cut leaves in audio, so that they do not click.
const DECLICK_SECONDS: f64 = 0.005;

/// Removes `start..end`: everything after it moves back by its length.
pub fn remove(project: &mut Project, start: u64, end: u64) {
    if end <= start {
        return;
    }
    cut(project, start, end, true);
    let shift = |at: u64| {
        if at >= end {
            at - (end - start)
        } else {
            at.min(start)
        }
    };
    for lane in lanes(project) {
        close_lane(&mut lane.points, start, end);
    }
    project
        .markers
        .retain(|m| m.position < start || m.position >= end);
    for m in &mut project.markers {
        m.position = shift(m.position);
    }
    remove_tempo(project, start, end);
    map_ranges(project, shift);
}

/// Empties `start..end` of every clip; the time stays, silent.
pub fn clear(project: &mut Project, start: u64, end: u64) {
    if end > start {
        cut(project, start, end, false);
    }
}

/// Keeps only `start..end`, which then starts the project.
pub fn crop(project: &mut Project, start: u64, end: u64) {
    if end <= start {
        return;
    }
    cut(project, end, u64::MAX, false);
    for lane in lanes(project) {
        end_lane(&mut lane.points, end);
    }
    project.markers.retain(|m| m.position <= end);
    map_ranges(project, |at| at.min(end));
    remove(project, 0, start);
}

/// Every automation lane of the tracks and of the master.
fn lanes(project: &mut Project) -> impl Iterator<Item = &mut AutomationLane> {
    project
        .tracks
        .iter_mut()
        .flat_map(|t| std::sync::Arc::make_mut(t).automation.iter_mut())
        .chain(project.master.automation.iter_mut())
}

/// Cuts the clips of every track and playlist at `start` and `end` and drops what lies
/// between; with `close`, what follows moves back.
fn cut(project: &mut Project, start: u64, end: u64, close: bool) {
    let declick = (DECLICK_SECONDS * f64::from(project.sample_rate)) as u64;
    let mut next = project.peek_id();
    for track in &mut project.tracks {
        let track = std::sync::Arc::make_mut(track);
        let playlists = std::iter::once(&mut track.clips)
            .chain(track.alternates.iter_mut().map(|p| &mut p.clips))
            .chain(track.takes.iter_mut().map(|p| &mut p.clips));
        for clips in playlists {
            cut_clips(clips, start, end, close, declick, &mut next);
        }
    }
    project.reserve_ids(next);
}

fn cut_clips(
    clips: &mut Vec<Clip>,
    start: u64,
    end: u64,
    close: bool,
    declick: u64,
    next: &mut u64,
) {
    let length = end - start;
    let mut kept = Vec::with_capacity(clips.len() + 1);
    for clip in std::mem::take(clips) {
        if clip.end() <= start {
            kept.push(clip);
            continue;
        }
        if clip.start >= end {
            let mut clip = clip;
            if close {
                clip.start -= length;
            }
            kept.push(clip);
            continue;
        }
        let audio = !clip.is_midi();
        if clip.start < start && start - clip.start >= MIN_LENGTH {
            let mut left = clip.clone();
            left.length = start - clip.start;
            if audio {
                left.fade_out = declick;
            }
            clamp_fades(&mut left);
            kept.push(left);
        }
        if clip.end() > end && clip.end() - end >= MIN_LENGTH {
            let mut right = clip.clone();
            if clip.start < start {
                right.id = ClipId(*next);
                *next += 1;
            }
            right.offset += end - clip.start;
            right.length = clip.end() - end;
            right.start = if close { start } else { end };
            if audio {
                right.fade_in = declick;
            }
            clamp_fades(&mut right);
            kept.push(right);
        }
    }
    kept.sort_by_key(|c| (c.start, c.id));
    *clips = kept;
}

/// Takes `start..end` out of an automation curve: the value at `start` holds until the cut,
/// where the curve goes on from its value at `end`.
fn close_lane(points: &mut Vec<AutomationPoint>, start: u64, end: u64) {
    if points.is_empty() {
        return;
    }
    let (before, after) = (value_at(points, start), value_at(points, end));
    let mut kept: Vec<AutomationPoint> =
        points.iter().filter(|p| p.time < start).copied().collect();
    if let (Some(value), Some(last)) = (before, start.checked_sub(1))
        && kept.last().is_none_or(|p| p.time < last)
        && !kept.is_empty()
    {
        kept.push(AutomationPoint::new(last, value, CurveShape::Linear));
    }
    if let Some(value) = after {
        kept.push(AutomationPoint::new(start, value, CurveShape::Linear));
    }
    for p in points.iter().filter(|p| p.time >= end) {
        let moved = AutomationPoint {
            time: p.time - (end - start),
            ..*p
        };
        match kept.last_mut() {
            Some(last) if last.time == moved.time => *last = moved,
            _ => kept.push(moved),
        }
    }
    *points = kept;
}

/// Ends an automation curve at `end`, holding the value it had there.
fn end_lane(points: &mut Vec<AutomationPoint>, end: u64) {
    if !points.iter().any(|p| p.time > end) {
        return;
    }
    let value = value_at(points, end);
    points.retain(|p| p.time <= end);
    if let Some(value) = value
        && points.last().is_none_or(|p| p.time < end)
    {
        points.push(AutomationPoint::new(end, value, CurveShape::Linear));
    }
}

/// Takes `start..end` out of the tempo map: the changes after it move back by the beats it
/// held (the signatures by its bars), the tempo in force at `end` starting at the cut.
fn remove_tempo(project: &mut Project, start: u64, end: u64) {
    let tempo = &project.tempo;
    if tempo.tempos().len() <= 1 && tempo.signatures().len() <= 1 {
        return;
    }
    let rate = project.sample_rate;
    let (from, to) = (
        tempo.sample_to_beat(start, rate),
        tempo.sample_to_beat(end, rate),
    );
    let beats = to - from;
    let mut tempos: Vec<TempoPoint> = tempo
        .tempos()
        .iter()
        .filter(|t| t.beat < from)
        .copied()
        .collect();
    let bpm = tempo.bpm_at_beat(to);
    if tempos.last().is_none_or(|t| t.bpm != bpm) {
        tempos.push(TempoPoint { beat: from, bpm });
    }
    tempos.extend(
        tempo
            .tempos()
            .iter()
            .filter(|t| t.beat > to + 1e-9)
            .map(|t| TempoPoint {
                beat: t.beat - beats,
                bpm: t.bpm,
            }),
    );
    let bar = |beat: f64| tempo.beat_to_bbt(beat).bar.saturating_sub(1);
    let (first, last) = (bar(from), bar(to));
    let mut signatures: Vec<TimeSignature> = tempo
        .signatures()
        .iter()
        .filter(|s| s.bar <= first)
        .copied()
        .collect();
    let at_end = tempo.signature_at_beat(to);
    let same = |a: &TimeSignature, b: &TimeSignature| {
        a.numerator == b.numerator && a.denominator == b.denominator
    };
    if signatures.last().is_none_or(|s| !same(s, &at_end)) {
        signatures.retain(|s| s.bar < first);
        signatures.push(TimeSignature {
            bar: first,
            ..at_end
        });
    }
    signatures.extend(
        tempo
            .signatures()
            .iter()
            .filter(|s| s.bar > last)
            .map(|s| TimeSignature {
                bar: s.bar - (last - first),
                ..*s
            }),
    );
    project.tempo = TempoMap::new(tempos, signatures);
}

/// Moves the loop and punch ranges; one left empty goes.
fn map_ranges(project: &mut Project, map: impl Fn(u64) -> u64) {
    for range in [&mut project.loop_range, &mut project.punch_range] {
        *range = range.map(|(a, b)| (map(a), map(b))).filter(|(a, b)| b > a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::AutomationTarget;
    use crate::model::Source;
    use std::path::PathBuf;

    /// One track with a clip over 0..10000 and another from 20000 to 30000, a marker at 5000
    /// and 25000, a volume ramp, and a loop over 4000..26000.
    fn project() -> Project {
        let mut p = Project::new("t", 48_000);
        let s = p.add_source(Source {
            path: PathBuf::from("a.wav"),
            channels: 1,
            sample_rate: 48_000,
            frames: 10_000,
        });
        let t = p.add_track("A");
        p.add_clip(t, s, 0);
        p.add_clip(t, s, 20_000);
        crate::edit::add_marker(&mut p, 5_000);
        crate::edit::add_marker(&mut p, 25_000);
        let track = p.track_mut(t).unwrap();
        let mut lane = AutomationLane::new(AutomationTarget::Gain);
        lane.add(0, 0.0, CurveShape::Linear);
        lane.add(30_000, 1.0, CurveShape::Linear);
        track.automation.push(lane);
        p.loop_range = Some((4_000, 26_000));
        p
    }

    fn clips(p: &Project) -> Vec<(u64, u64, u64)> {
        p.tracks[0]
            .clips
            .iter()
            .map(|c| (c.start, c.offset, c.length))
            .collect()
    }

    #[test]
    fn removing_closes_the_gap() {
        let mut p = project();
        remove(&mut p, 6_000, 22_000);
        assert_eq!(clips(&p), [(0, 0, 6_000), (6_000, 2_000, 8_000)]);
        let c = &p.tracks[0].clips;
        assert_eq!(
            (c[0].fade_out, c[1].fade_in),
            (240, 240),
            "no click at the cut"
        );
        let markers: Vec<u64> = p.markers.iter().map(|m| m.position).collect();
        assert_eq!(markers, [5_000, 9_000]);
        assert_eq!(p.loop_range, Some((4_000, 10_000)));
        let lane = &p.tracks[0].automation[0];
        assert_eq!(lane.value_at(5_999), Some(0.2));
        assert!((lane.value_at(6_000).unwrap() - 22_000.0 / 30_000.0).abs() < 1e-6);
        assert_eq!(lane.value_at(14_000), Some(1.0));
    }

    #[test]
    fn clearing_keeps_the_time() {
        let mut p = project();
        let markers = p.markers.clone();
        clear(&mut p, 6_000, 22_000);
        assert_eq!(clips(&p), [(0, 0, 6_000), (22_000, 2_000, 8_000)]);
        assert_eq!(p.markers, markers);
        assert_eq!(p.loop_range, Some((4_000, 26_000)));
        assert_eq!(p.tracks[0].automation[0].points.len(), 2);
    }

    #[test]
    fn cropping_keeps_only_the_selection() {
        let mut p = project();
        crop(&mut p, 5_000, 25_000);
        assert_eq!(clips(&p), [(0, 5_000, 5_000), (15_000, 0, 5_000)]);
        let markers: Vec<u64> = p.markers.iter().map(|m| m.position).collect();
        assert_eq!(markers, [0, 20_000]);
        assert_eq!(p.loop_range, Some((0, 20_000)));
        let lane = &p.tracks[0].automation[0];
        assert!((lane.value_at(0).unwrap() - 5_000.0 / 30_000.0).abs() < 1e-6);
        assert!((lane.value_at(20_000).unwrap() - 25_000.0 / 30_000.0).abs() < 1e-6);
        assert_eq!(lane.points.last().unwrap().time, 20_000);
        assert_eq!(p.length(), 20_000);
    }

    #[test]
    fn playlists_and_ids_follow() {
        let mut p = project();
        let t = p.tracks[0].id;
        crate::edit::new_playlist(&mut p, t);
        let s = *p.sources.keys().next().unwrap();
        p.add_clip(t, s, 0);
        remove(&mut p, 2_000, 4_000);
        let track = &p.tracks[0];
        assert_eq!(clips(&p), [(0, 0, 2_000), (2_000, 4_000, 6_000)]);
        let alternate: Vec<u64> = track.alternates[0].clips.iter().map(|c| c.start).collect();
        assert_eq!(alternate, [0, 2_000, 18_000]);
        let mut ids: Vec<ClipId> = track
            .clips
            .iter()
            .chain(&track.alternates[0].clips)
            .map(|c| c.id)
            .collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5, "every clip keeps its own id");
        assert!(p.peek_id() > p.highest_id());
    }

    #[test]
    fn tempo_changes_after_the_cut_move_back() {
        let mut p = project();
        // 120 then 90 BPM from beat 8 (4 s), 4/4 then 3/4 from bar 4.
        p.tempo = TempoMap::new(
            vec![
                TempoPoint {
                    beat: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    beat: 8.0,
                    bpm: 90.0,
                },
            ],
            vec![
                TimeSignature {
                    bar: 0,
                    numerator: 4,
                    denominator: 4,
                },
                TimeSignature {
                    bar: 4,
                    numerator: 3,
                    denominator: 4,
                },
            ],
        );
        // Takes out bar 2 (beats 4 to 8, 2 s).
        remove(&mut p, 96_000, 192_000);
        let tempos: Vec<(f64, f64)> = p.tempo.tempos().iter().map(|t| (t.beat, t.bpm)).collect();
        assert_eq!(tempos, [(0.0, 120.0), (4.0, 90.0)]);
        let bars: Vec<(u32, u32)> = p
            .tempo
            .signatures()
            .iter()
            .map(|s| (s.bar, s.numerator))
            .collect();
        assert_eq!(bars, [(0, 4), (3, 3)]);
    }
}
