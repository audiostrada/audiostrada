//! Writes a track's preparation (see `audiostrada_dsp::prepare`) into the project: settings of
//! its channel strip, the effects added after it (a Strada-EQ for its resonances, a de-esser,
//! a transient shaper), a ride of the strip's output, its pan, its console color and its send
//! to the shared space bus; takes it back out, or bypasses it for a comparison.

use crate::automation::{self, AutomationPoint, AutomationTarget};
use crate::console::Console;
use crate::model::{Bypassed, Headroom, InsertId, Prepared, Project, TrackId, TrackKind};
use crate::routing::{self, Channel};

/// The `instrument` marking the space bus the preparation made.
pub const SPACE: &str = "space";

/// An effect added after the channel strip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Added {
    pub effect: String,
    pub params: Vec<(String, f32)>,
    /// Its output gain, dB: what keeps the level it had going in.
    pub gain_db: f32,
    /// Part of bringing out (else of the cleaning).
    pub out: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preparation {
    pub instrument: String,
    pub clean: bool,
    pub bring_out: bool,
    /// Channel strip settings, by parameter id.
    pub strip: Vec<(String, f32)>,
    /// Added to the strip's output gain, dB: what keeps the level it had going in.
    pub strip_gain: f32,
    /// The effects, the cleaning's then bringing out's, in their order after the strip.
    pub inserts: Vec<Added>,
    /// Gain of the strip's output (project samples, dB); empty, no ride.
    pub ride: Vec<(u64, f32)>,
    /// The track leads with an instrument: the strip goes after it.
    pub after_first: bool,
    /// Bringing out: the track's pan, its console color, its send (dB) to the space bus.
    pub pan: Option<f32>,
    pub console: Option<Console>,
    pub space: Option<(TrackId, f32)>,
    /// The track's input gain (dB), set first to bring it to the working level.
    pub trim: Option<f32>,
}

/// The space bus the preparation made, if there is one.
pub fn space_bus(project: &Project) -> Option<TrackId> {
    project
        .tracks
        .iter()
        .find(|t| {
            t.kind == TrackKind::Bus && t.prepared.as_ref().is_some_and(|p| p.instrument == SPACE)
        })
        .map(|t| t.id)
}

/// The space bus, made with its reverb (`reverb`: its settings) when there is none yet.
pub fn ensure_space_bus(project: &mut Project, name: &str, reverb: &[(&str, f32)]) -> TrackId {
    if let Some(bus) = space_bus(project) {
        return bus;
    }
    let bus = routing::add_bus(project, name);
    if let Some(t) = project.track_mut(bus) {
        t.prepared = Some(Prepared {
            instrument: SPACE.to_owned(),
            ..Default::default()
        });
    }
    let params = reverb.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
    routing::add_insert(project, Channel::Track(bus), "reverb", params);
    bus
}

/// Removes the space bus when nothing sends to it any more.
pub fn tidy_space(project: &mut Project) {
    if let Some(bus) = space_bus(project)
        && !project
            .tracks
            .iter()
            .any(|t| t.sends.iter().any(|s| s.target == bus))
    {
        project.remove_track(bus);
    }
}

/// A bus the Express Mix groups tracks into.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The family it holds, as an instrument id: its bus is prepared as such.
    pub family: String,
    pub name: String,
    /// Its tracks, in their order.
    pub tracks: Vec<TrackId>,
}

/// Makes the buses of `groups`, one after the other in their order where the first of their
/// tracks was, each followed by its tracks; their ids.
pub fn make_groups(project: &mut Project, groups: &[Group]) -> Vec<TrackId> {
    let before: Vec<TrackId> = project.tracks.iter().map(|t| t.id).collect();
    let members: Vec<TrackId> = groups.iter().flat_map(|g| g.tracks.clone()).collect();
    let Some(anchor) = before.iter().position(|t| members.contains(t)) else {
        return Vec::new();
    };
    let mut buses = Vec::new();
    for g in groups {
        let bus = routing::add_bus(project, &g.name);
        let color = g
            .tracks
            .first()
            .and_then(|t| project.track(*t))
            .map(|t| t.color);
        if let Some(t) = project.track_mut(bus) {
            if let Some(color) = color {
                t.color = color;
            }
            t.prepared = Some(Prepared {
                instrument: g.family.clone(),
                ..Default::default()
            });
        }
        for &id in &g.tracks {
            if let Some(t) = project.track_mut(id) {
                t.parent = Some(bus);
                t.master_send = true;
            }
        }
        buses.push(bus);
    }
    let block: Vec<TrackId> = groups
        .iter()
        .zip(&buses)
        .flat_map(|(g, &bus)| std::iter::once(bus).chain(g.tracks.iter().copied()))
        .collect();
    let tracks = std::mem::take(&mut project.tracks);
    let (mut grouped, others): (Vec<_>, Vec<_>) =
        tracks.into_iter().partition(|t| block.contains(&t.id));
    grouped.sort_by_key(|t| block.iter().position(|id| *id == t.id));
    let at = others
        .iter()
        .filter(|t| before.iter().position(|id| *id == t.id) < Some(anchor))
        .count();
    let mut others = others;
    let tail = others.split_off(at);
    project.tracks = others.into_iter().chain(grouped).chain(tail).collect();
    routing::normalize_order(project);
    buses
}

fn reset(clean: bool) -> &'static [(&'static str, f32)] {
    if clean {
        &[("filters", 0.0), ("gate", 0.0)]
    } else {
        &[
            ("eq", 0.0),
            ("low_gain", 0.0),
            ("lmf_gain", 0.0),
            ("hmf_gain", 0.0),
            ("high_gain", 0.0),
            ("comp", 0.0),
        ]
    }
}

fn ride_target(project: &Project, track: TrackId) -> Option<AutomationTarget> {
    routing::strip(project, Channel::Track(track)).map(|s| AutomationTarget::Param {
        insert: s.id,
        param: "output".to_owned(),
    })
}

fn strip_mut(project: &mut Project, track: TrackId) -> Option<&mut crate::model::Insert> {
    project
        .track_mut(track)?
        .inserts
        .iter_mut()
        .find(|i| i.effect == routing::STRIP)
}

/// Undoes the parts named of what was prepared before: the modules they set, the effects
/// added (unless kept: they are about to be set again), the ride, the pan, the send, the
/// console color (unless `console` is false: it is left as it is), and the gain on the strip.
fn take_back(
    project: &mut Project,
    track: TrackId,
    (clean, bring_out): (bool, bool),
    (keep, keep_out): (bool, bool),
    console: bool,
) {
    let channel = Channel::Track(track);
    let Some(done) = project.track(track).and_then(|t| t.prepared.clone()) else {
        return;
    };
    let strip = routing::strip(project, channel).map(|s| s.id);
    for (part, did) in [
        (true, clean && done.cleaned),
        (false, bring_out && done.brought_out),
    ] {
        if !did {
            continue;
        }
        if let Some(id) = strip {
            for &(param, value) in reset(part) {
                routing::set_insert_param(project, channel, id, param, value);
            }
        }
        let (added, kept) = if part {
            (&done.inserts, keep)
        } else {
            (&done.out_inserts, keep_out)
        };
        if !kept {
            for &id in added {
                routing::remove_insert(project, channel, id);
            }
        }
        if part {
            continue;
        }
        if let Some(target) = ride_target(project, track)
            && let Some(t) = project.track_mut(track)
        {
            t.automation.retain(|l| l.target != target);
        }
        let bus = space_bus(project);
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        if let Some(pan) = done.pan {
            t.pan = pan;
        }
        if done.space {
            t.sends.retain(|s| Some(s.target) != bus);
        }
        if done.console_set && console {
            t.console = done.console;
        }
    }
    if let Some(s) = strip_mut(project, track) {
        s.gain_db -= done.strip_gain;
    }
    let cleaned = done.cleaned && !clean;
    let brought_out = done.brought_out && !bring_out;
    let console_kept = brought_out || (bring_out && !console && done.console_set);
    if let Some(t) = project.track_mut(track) {
        t.prepared = (cleaned || brought_out || console_kept).then(|| Prepared {
            cleaned,
            brought_out,
            inserts: if cleaned { done.inserts } else { Vec::new() },
            out_inserts: if brought_out {
                done.out_inserts
            } else {
                Vec::new()
            },
            strip_gain: 0.0,
            pan: done.pan.filter(|_| brought_out),
            console_set: console_kept && done.console_set,
            space: done.space && brought_out,
            bypassed: None,
            ..done
        });
    }
}

/// Of what was added for a part, the effects kept in place when the same ones come again.
fn reusable(
    project: &Project,
    track: TrackId,
    done: &[InsertId],
    wanted: &[&Added],
) -> Vec<InsertId> {
    let Some(t) = project.track(track) else {
        return Vec::new();
    };
    let same = !done.is_empty()
        && done.len() == wanted.len()
        && done.iter().zip(wanted).all(|(id, a)| {
            t.inserts
                .iter()
                .any(|i| i.id == *id && i.effect == a.effect)
        });
    if same { done.to_vec() } else { Vec::new() }
}

/// Places `wanted` one after the other from `after` (else the start of the chain), reusing
/// `reuse` when given; their ids.
fn place(
    project: &mut Project,
    track: TrackId,
    wanted: &[&Added],
    reuse: &[InsertId],
    mut after: Option<InsertId>,
) -> Option<Vec<InsertId>> {
    let channel = Channel::Track(track);
    let mut added = Vec::new();
    for (k, a) in wanted.iter().enumerate() {
        let id = match reuse.get(k) {
            Some(&id) => {
                // Bands it no longer uses are switched off.
                let stale: Vec<String> = project
                    .track(track)?
                    .inserts
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| {
                        i.params
                            .keys()
                            .filter(|k| {
                                k.ends_with("_used") && !a.params.iter().any(|(n, _)| n == *k)
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                for param in stale {
                    routing::set_insert_param(project, channel, id, &param, 0.0);
                }
                id
            }
            None => {
                let index = after
                    .and_then(|after| {
                        routing::inserts(project, channel)
                            .iter()
                            .position(|i| i.id == after)
                    })
                    .map_or(0, |i| i + 1);
                routing::add_insert_at(project, channel, (None, index), &a.effect)?
            }
        };
        for (param, value) in &a.params {
            routing::set_insert_param(project, channel, id, param, *value);
        }
        if let Some(i) = project
            .track_mut(track)?
            .inserts
            .iter_mut()
            .find(|i| i.id == id)
        {
            i.gain_db = a.gain_db;
        }
        added.push(id);
        after = Some(id);
    }
    Some(added)
}

/// Prepares `track`: what it did before for the same parts is replaced, never added to.
pub fn prepare(project: &mut Project, track: TrackId, p: &Preparation) -> Option<()> {
    let channel = Channel::Track(track);
    set_bypassed(project, track, false);
    let done = project.track(track)?.prepared.clone().unwrap_or_default();
    let done_trim = done.trim;
    let cleaning: Vec<&Added> = p.inserts.iter().filter(|a| !a.out).collect();
    let out: Vec<&Added> = p.inserts.iter().filter(|a| a.out).collect();
    // The same effects as last time stay in place, only their settings change: nothing is
    // rebuilt under a preparation heard while it is adjusted.
    let reuse = if done.cleaned && p.clean {
        reusable(project, track, &done.inserts, &cleaning)
    } else {
        Vec::new()
    };
    let reuse_out = if done.brought_out && p.bring_out {
        reusable(project, track, &done.out_inserts, &out)
    } else {
        Vec::new()
    };
    take_back(
        project,
        track,
        (p.clean, p.bring_out),
        (!reuse.is_empty(), !reuse_out.is_empty()),
        true,
    );
    let before = project.track(track)?.prepared.clone();
    let strip = routing::ensure_strip(project, channel, p.after_first)?;
    for (part, on) in [(true, p.clean), (false, p.bring_out)] {
        if on {
            for &(param, value) in reset(part) {
                routing::set_insert_param(project, channel, strip, param, value);
            }
        }
    }
    for (param, value) in &p.strip {
        routing::set_insert_param(project, channel, strip, param, *value);
    }
    if let Some(s) = strip_mut(project, track) {
        s.gain_db += p.strip_gain;
    }
    let inserts = if p.clean {
        place(project, track, &cleaning, &reuse, Some(strip))?
    } else {
        before
            .as_ref()
            .map(|b| b.inserts.clone())
            .unwrap_or_default()
    };
    let out_inserts = if p.bring_out {
        let after = inserts.last().copied().unwrap_or(strip);
        place(project, track, &out, &reuse_out, Some(after))?
    } else {
        before
            .as_ref()
            .map(|b| b.out_inserts.clone())
            .unwrap_or_default()
    };
    if !p.ride.is_empty()
        && let Some(target) = ride_target(project, track)
        && let Some(t) = project.track_mut(track)
    {
        let lane = automation::lane_mut(t, &target);
        lane.points = p
            .ride
            .iter()
            .map(|&(time, value)| AutomationPoint {
                time,
                value,
                shape: Default::default(),
                tension: 0.0,
            })
            .collect();
        lane.enabled = true;
    }
    let mut space = before.as_ref().is_some_and(|b| b.space) && !p.bring_out;
    if let Some((bus, db)) = p.space
        && match routing::add_send(project, track, bus) {
            Ok(()) => true,
            Err(e) => e == routing::RoutingError::Duplicate,
        }
        && let Some(send) = project
            .track_mut(track)?
            .sends
            .iter_mut()
            .find(|s| s.target == bus)
    {
        send.gain_db = db;
        send.mute = false;
        space = true;
    }
    let t = project.track_mut(track)?;
    let mut pan = before.as_ref().and_then(|b| b.pan).filter(|_| !p.bring_out);
    if let Some(to) = p.pan {
        pan = Some(t.pan);
        t.pan = to.clamp(-1.0, 1.0);
    }
    let (mut console_set, mut console) = before
        .as_ref()
        .map_or((false, None), |b| (b.console_set, b.console));
    if let Some(c) = p.console {
        if !console_set {
            console = t.console;
        }
        console_set = true;
        t.console = Some(c);
    }
    // Gain staging: the input gain it had first is kept, to come back to.
    let first_trim = before
        .as_ref()
        .and_then(|b| b.trim)
        .or(done_trim)
        .unwrap_or(t.trim_db);
    t.trim_db = p.trim.unwrap_or(first_trim);
    let trim = p.trim.map(|_| first_trim);
    t.prepared = Some(Prepared {
        instrument: p.instrument.clone(),
        cleaned: p.clean || before.as_ref().is_some_and(|b| b.cleaned),
        brought_out: p.bring_out || before.as_ref().is_some_and(|b| b.brought_out),
        inserts,
        out_inserts,
        strip_gain: p.strip_gain,
        pan,
        console_set,
        console,
        space,
        trim,
        bypassed: None,
    });
    Some(())
}

/// Takes the whole preparation of `track` back out (then [`tidy_space`] once every track is
/// done).
pub fn unprepare(project: &mut Project, track: TrackId) {
    set_bypassed(project, track, false);
    let trim = project
        .track(track)
        .and_then(|t| t.prepared.as_ref())
        .and_then(|p| p.trim);
    take_back(project, track, (true, true), (false, false), true);
    if let (Some(trim), Some(t)) = (trim, project.track_mut(track)) {
        t.trim_db = trim;
        if let Some(prepared) = &mut t.prepared {
            prepared.trim = None;
        }
    }
}

/// Keeps `db` of room before the master's processing, or takes it out (`None`): a
/// Strada-Utility first lowers the mix by `db`, a Strada-Max last (the one already there, else
/// a new one) brings its loudness back with its peaks under -1 dB. Set again, the same ones
/// are kept and only their gains change.
pub fn master_headroom(project: &mut Project, db: Option<f32>) {
    let channel = Channel::Master;
    let has = |project: &Project, id: InsertId| project.master.inserts.iter().any(|i| i.id == id);
    let mut kept = project
        .master
        .headroom
        .clone()
        .filter(|h| has(project, h.trim) && has(project, h.makeup));
    let Some(db) = db else {
        if let Some(h) = kept.take() {
            routing::remove_insert(project, channel, h.trim);
            match h.makeup_gain {
                Some(gain) => routing::set_insert_param(project, channel, h.makeup, "gain", gain),
                None => routing::remove_insert(project, channel, h.makeup),
            }
        }
        project.master.headroom = None;
        return;
    };
    let h = match kept {
        Some(h) => h,
        None => {
            let Some(trim) = routing::add_insert_at(project, channel, (None, 0), "utility") else {
                return;
            };
            let found = project
                .master
                .inserts
                .iter()
                .rev()
                .find(|i| i.effect == "maximizer" && !i.bypass)
                .map(|i| (i.id, i.params.get("gain").copied().unwrap_or(4.0)));
            let (makeup, makeup_gain) = match found {
                Some((id, gain)) => (id, Some(gain)),
                None => {
                    let params = [("ceiling".to_owned(), -1.0)].into_iter().collect();
                    let Some(id) = routing::add_insert(project, channel, "maximizer", params)
                    else {
                        return;
                    };
                    (id, None)
                }
            };
            Headroom {
                trim,
                makeup,
                makeup_gain,
            }
        }
    };
    routing::set_insert_param(project, channel, h.trim, "gain", -db);
    let base = h.makeup_gain.unwrap_or(0.0);
    routing::set_insert_param(
        project,
        channel,
        h.makeup,
        "gain",
        (base + db).clamp(0.0, 24.0),
    );
    project.master.headroom = Some(h);
}

/// Bypasses what the preparation of `track` did, to compare, or puts it back: the strip's
/// modules it set and its gain, the effects it added, the ride, the send to the space bus,
/// the pan and the console color.
pub fn set_bypassed(project: &mut Project, track: TrackId, on: bool) {
    let channel = Channel::Track(track);
    let Some(done) = project.track(track).and_then(|t| t.prepared.clone()) else {
        return;
    };
    if done.bypassed.is_some() == on || done.instrument == SPACE {
        return;
    }
    let strip = routing::strip(project, channel).map(|s| (s.id, s.params.clone()));
    let ride = ride_target(project, track);
    let bus = space_bus(project);
    let added: Vec<InsertId> = done
        .inserts
        .iter()
        .chain(&done.out_inserts)
        .copied()
        .collect();
    if on {
        let mut b = Bypassed::default();
        if let Some((id, params)) = &strip {
            let mut modules = Vec::new();
            if done.cleaned {
                modules.extend(["filters", "gate"]);
            }
            if done.brought_out {
                modules.extend(["eq", "comp"]);
            }
            for m in modules {
                let value = params.get(m).copied().unwrap_or(0.0);
                if value != 0.0 {
                    b.switches.push((m.to_owned(), value));
                    routing::set_insert_param(project, channel, *id, m, 0.0);
                }
            }
        }
        let inserts = project
            .track(track)
            .map(|t| t.inserts.clone())
            .unwrap_or_default();
        b.inserts = added
            .iter()
            .copied()
            .filter(|id| inserts.iter().any(|i| i.id == *id && !i.bypass))
            .collect();
        for &id in &b.inserts {
            routing::set_bypass(project, channel, id, true);
        }
        if let Some(s) = strip_mut(project, track) {
            s.gain_db -= done.strip_gain;
        }
        let Some(t) = project.track_mut(track) else {
            return;
        };
        if let Some(lane) = t
            .automation
            .iter_mut()
            .find(|l| Some(&l.target) == ride.as_ref())
            && lane.enabled
        {
            lane.enabled = false;
            b.ride = true;
        }
        if let Some(send) = t
            .sends
            .iter_mut()
            .find(|s| done.space && Some(s.target) == bus && !s.mute)
        {
            send.mute = true;
            b.send = true;
        }
        if let Some(pan) = done.pan {
            b.pan = t.pan;
            t.pan = pan;
        }
        if done.console_set {
            b.console = t.console;
            t.console = done.console;
        }
        if let Some(trim) = done.trim {
            b.trim = t.trim_db;
            t.trim_db = trim;
        }
        if let Some(prepared) = &mut t.prepared {
            prepared.bypassed = Some(b);
        }
    } else {
        let Some(b) = done.bypassed else {
            return;
        };
        if let Some((id, _)) = strip {
            for (m, value) in &b.switches {
                routing::set_insert_param(project, channel, id, m, *value);
            }
        }
        for &id in &b.inserts {
            routing::set_bypass(project, channel, id, false);
        }
        if let Some(s) = strip_mut(project, track) {
            s.gain_db += done.strip_gain;
        }
        let Some(t) = project.track_mut(track) else {
            return;
        };
        if b.ride
            && let Some(lane) = t
                .automation
                .iter_mut()
                .find(|l| Some(&l.target) == ride.as_ref())
        {
            lane.enabled = true;
        }
        if b.send
            && let Some(send) = t.sends.iter_mut().find(|s| Some(s.target) == bus)
        {
            send.mute = false;
        }
        if done.pan.is_some() {
            t.pan = b.pan;
        }
        if done.console_set {
            t.console = b.console;
        }
        if done.trim.is_some() {
            t.trim_db = b.trim;
        }
        if let Some(prepared) = &mut t.prepared {
            prepared.bypassed = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cleaning() -> Preparation {
        Preparation {
            instrument: "vocal_male".into(),
            clean: true,
            strip: vec![("filters".into(), 1.0), ("hpf".into(), 95.0)],
            strip_gain: 1.5,
            inserts: vec![
                Added {
                    effect: "eq".into(),
                    params: vec![("b1_used".into(), 1.0), ("b1_freq".into(), 3100.0)],
                    gain_db: 0.5,
                    out: false,
                },
                Added {
                    effect: "deesser".into(),
                    params: vec![("frequency".into(), 6000.0)],
                    gain_db: 0.0,
                    out: false,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn preparing_again_replaces_what_was_done() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Lead vox");
        prepare(&mut p, t, &cleaning()).unwrap();
        prepare(&mut p, t, &cleaning()).unwrap();
        let inserts = &p.track(t).unwrap().inserts;
        let effects: Vec<&str> = inserts.iter().map(|i| i.effect.as_str()).collect();
        assert_eq!(effects, [routing::STRIP, "eq", "deesser"]);
        assert_eq!(inserts[0].params["hpf"], 95.0);
        assert_eq!(inserts[0].gain_db, 1.5, "kept once, not twice");
        assert_eq!(inserts[1].params["b1_freq"], 3100.0);
        assert_eq!(inserts[1].gain_db, 0.5);
        let done = p.track(t).unwrap().prepared.clone().unwrap();
        assert!(done.cleaned && !done.brought_out);
        assert_eq!(done.inserts, [inserts[1].id, inserts[2].id]);
    }

    #[test]
    fn preparing_again_with_the_same_effects_keeps_them_in_place() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Lead vox");
        prepare(&mut p, t, &cleaning()).unwrap();
        let before: Vec<_> = p.track(t).unwrap().inserts.iter().map(|i| i.id).collect();
        let mut again = cleaning();
        again.inserts[0].params = vec![("b2_used".into(), 1.0), ("b2_freq".into(), 500.0)];
        prepare(&mut p, t, &again).unwrap();
        let inserts = &p.track(t).unwrap().inserts;
        let after: Vec<_> = inserts.iter().map(|i| i.id).collect();
        assert_eq!(before, after, "nothing is rebuilt");
        assert_eq!(inserts[1].params["b1_used"], 0.0, "the band left is off");
        assert_eq!(inserts[1].params["b2_freq"], 500.0);
    }

    #[test]
    fn bringing_out_keeps_the_cleaning_and_removing_takes_both_back() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Lead vox");
        prepare(&mut p, t, &cleaning()).unwrap();
        let out = Preparation {
            instrument: "vocal_male".into(),
            bring_out: true,
            strip: vec![("eq".into(), 1.0), ("hmf_gain".into(), 2.0)],
            strip_gain: -2.0,
            ride: vec![(0, 0.0), (48_000, 3.0)],
            ..Default::default()
        };
        prepare(&mut p, t, &out).unwrap();
        let track = p.track(t).unwrap();
        assert_eq!(track.inserts.len(), 3, "the cleaning's effects stay");
        assert_eq!(track.inserts[0].gain_db, -2.0, "the level measured last");
        assert_eq!(track.automation.len(), 1);
        let done = track.prepared.clone().unwrap();
        assert!(done.cleaned && done.brought_out);
        unprepare(&mut p, t);
        let track = p.track(t).unwrap();
        assert_eq!(track.inserts.len(), 1, "only the strip is left");
        let strip = &track.inserts[0];
        assert_eq!(
            (
                strip.params["filters"],
                strip.params["eq"],
                strip.params["hmf_gain"]
            ),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(strip.gain_db, 0.0);
        assert!(track.automation.is_empty());
        assert!(track.prepared.is_none());
    }

    fn bringing_out(p: &mut Project) -> Preparation {
        let bus = ensure_space_bus(p, "Space", &[("decay", 1.5)]);
        Preparation {
            instrument: "snare".into(),
            bring_out: true,
            strip: vec![("eq".into(), 1.0), ("hmf_gain".into(), 2.0)],
            strip_gain: -1.0,
            inserts: vec![Added {
                effect: "transient".into(),
                params: vec![("attack".into(), 25.0)],
                gain_db: 0.0,
                out: true,
            }],
            ride: vec![(0, 0.0), (48_000, 3.0)],
            pan: Some(0.3),
            console: Some(Console {
                model: crate::console::ConsoleModel::Discrete,
                amount: 50.0,
            }),
            space: Some((bus, -15.0)),
            ..Default::default()
        }
    }

    #[test]
    fn bringing_out_again_updates_the_pan_the_console_and_the_send_without_adding_any() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Snare");
        p.track_mut(t).unwrap().pan = -0.1;
        prepare(&mut p, t, &cleaning()).unwrap();
        let out = bringing_out(&mut p);
        prepare(&mut p, t, &out).unwrap();
        let mut again = out.clone();
        again.pan = Some(0.2);
        prepare(&mut p, t, &again).unwrap();
        let track = p.track(t).unwrap();
        let effects: Vec<&str> = track.inserts.iter().map(|i| i.effect.as_str()).collect();
        assert_eq!(effects, [routing::STRIP, "eq", "deesser", "transient"]);
        assert_eq!(track.pan, 0.2);
        assert_eq!(track.sends.len(), 1, "one send to the space bus");
        assert_eq!(track.sends[0].gain_db, -15.0);
        let done = track.prepared.clone().unwrap();
        assert_eq!(
            (done.pan, done.console, done.console_set),
            (Some(-0.1), None, true)
        );
        unprepare(&mut p, t);
        tidy_space(&mut p);
        let track = p.track(t).unwrap();
        assert_eq!(
            (track.pan, track.console, track.sends.len()),
            (-0.1, None, 0)
        );
        assert_eq!(track.inserts.len(), 1);
        assert_eq!(space_bus(&p), None, "the empty space bus is gone");
    }

    #[test]
    fn grouped_tracks_go_into_their_buses_in_the_order_asked_where_the_first_was() {
        let mut p = Project::new("t", 48_000);
        let intro = p.add_track("Intro");
        let vox = p.add_track("Vox");
        let kick = p.add_track("Kick");
        let gtr = p.add_track("Gtr");
        let snare = p.add_track("Snare");
        let gtr2 = p.add_track("Gtr 2");
        let groups = [
            Group {
                family: "drums".into(),
                name: "Drums".into(),
                tracks: vec![kick, snare],
            },
            Group {
                family: "electric_guitar".into(),
                name: "Guitars".into(),
                tracks: vec![gtr, gtr2],
            },
        ];
        let buses = make_groups(&mut p, &groups);
        let order: Vec<TrackId> = p.tracks.iter().map(|t| t.id).collect();
        assert_eq!(
            order,
            [intro, vox, buses[0], kick, snare, buses[1], gtr, gtr2],
            "where the first grouped track was, after the tracks before it"
        );
        assert_eq!(p.track(snare).unwrap().parent, Some(buses[0]));
        assert_eq!(p.track(gtr2).unwrap().parent, Some(buses[1]));
        let bus = p.track(buses[0]).unwrap();
        assert_eq!(bus.kind, TrackKind::Bus);
        assert_eq!(bus.prepared.as_ref().unwrap().instrument, "drums");
    }

    #[test]
    fn the_input_gain_set_comes_back_as_it_was() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Snare");
        p.track_mut(t).unwrap().trim_db = -2.0;
        let mut staged = cleaning();
        staged.trim = Some(6.0);
        prepare(&mut p, t, &staged).unwrap();
        prepare(&mut p, t, &staged).unwrap();
        assert_eq!(p.track(t).unwrap().trim_db, 6.0);
        set_bypassed(&mut p, t, true);
        assert_eq!(p.track(t).unwrap().trim_db, -2.0, "bypassed, as it was");
        set_bypassed(&mut p, t, false);
        assert_eq!(p.track(t).unwrap().trim_db, 6.0);
        unprepare(&mut p, t);
        assert_eq!(
            p.track(t).unwrap().trim_db,
            -2.0,
            "the first one, not the one before"
        );
    }

    #[test]
    fn a_bypassed_preparation_is_heard_as_before_and_comes_back_whole() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Snare");
        prepare(&mut p, t, &cleaning()).unwrap();
        let out = bringing_out(&mut p);
        prepare(&mut p, t, &out).unwrap();
        let prepared = p.track(t).unwrap().clone();
        set_bypassed(&mut p, t, true);
        let track = p.track(t).unwrap();
        let strip = &track.inserts[0];
        assert_eq!((strip.params["filters"], strip.params["eq"]), (0.0, 0.0));
        assert_eq!(strip.gain_db, 0.0);
        assert!(track.inserts[1..].iter().all(|i| i.bypass));
        assert!(track.sends[0].mute);
        assert!(!track.automation[0].enabled);
        assert_eq!((track.pan, track.console), (0.0, None));
        set_bypassed(&mut p, t, false);
        assert_eq!(p.track(t).unwrap(), &prepared);
        set_bypassed(&mut p, t, true);
        prepare(&mut p, t, &out).unwrap();
        let track = p.track(t).unwrap();
        assert!(track.prepared.as_ref().unwrap().bypassed.is_none());
        assert!(
            track.inserts.iter().all(|i| !i.bypass),
            "preparing again turns it back on"
        );
    }
}
