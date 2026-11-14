//! Writes a track's preparation (see `audiostrada_dsp::prepare`) into the project: settings of
//! its channel strip, the effects added after it (a Strada-EQ for its resonances, a de-esser,
//! a transient shaper), a ride of the strip's output, its pan, its console color and its send
//! to the shared space bus; takes it back out, or bypasses it for a comparison.

use crate::automation::{self, AutomationPoint, AutomationTarget};
use crate::console::Console;
use crate::model::{Bypassed, Headroom, InsertId, Prepared, Project, TrackId, TrackKind};
use crate::routing::{self, Channel};

/// The `instrument` marking a space bus the preparation made: the hall, the first there was.
pub const SPACE: &str = "space";
/// The space buses a mix can have: a room, a plate, the hall and the echo.
pub const SPACES: [&str; 4] = ["space_room", "space_plate", SPACE, "space_echo"];

/// `instrument` marks a space bus.
pub fn is_space(instrument: &str) -> bool {
    SPACES.contains(&instrument)
}

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
    /// Bringing out: the track's pan, its console color, its send (dB) to the space bus and
    /// to the echo.
    pub pan: Option<f32>,
    pub console: Option<Console>,
    pub space: Option<(TrackId, f32)>,
    pub echo: Option<(TrackId, f32)>,
    /// The track's input gain (dB), set first to bring it to the working level.
    pub trim: Option<f32>,
    /// The track's fader (dB), when the balance is set.
    pub gain: Option<f32>,
    /// Channel strip modules set by hand: left as they are.
    pub hands: Vec<String>,
}

/// The space bus of a kind (one of [`SPACES`]) the preparation made, if there is one.
pub fn space_bus(project: &Project, kind: &str) -> Option<TrackId> {
    project
        .tracks
        .iter()
        .find(|t| {
            t.kind == TrackKind::Bus && t.prepared.as_ref().is_some_and(|p| p.instrument == kind)
        })
        .map(|t| t.id)
}

/// Every space bus the preparation made.
pub fn space_buses(project: &Project) -> Vec<TrackId> {
    project
        .tracks
        .iter()
        .filter(|t| {
            t.kind == TrackKind::Bus && t.prepared.as_ref().is_some_and(|p| is_space(&p.instrument))
        })
        .map(|t| t.id)
        .collect()
}

/// The space bus of a kind, made with its effect (`effect`, set to `settings`) when there is
/// none yet; one already there keeps its settings, which may have been changed by hand.
pub fn ensure_space_bus(
    project: &mut Project,
    kind: &str,
    name: &str,
    (effect, settings): (&str, &[(&str, f32)]),
) -> TrackId {
    if let Some(bus) = space_bus(project, kind) {
        return bus;
    }
    let bus = routing::add_bus(project, name);
    if let Some(t) = project.track_mut(bus) {
        t.prepared = Some(Prepared {
            instrument: kind.to_owned(),
            ..Default::default()
        });
    }
    let params = settings
        .iter()
        .map(|(k, v)| ((*k).to_owned(), *v))
        .collect();
    routing::add_insert(project, Channel::Track(bus), effect, params);
    bus
}

/// Removes the space buses nothing sends to any more.
pub fn tidy_space(project: &mut Project) {
    for bus in space_buses(project) {
        if !project
            .tracks
            .iter()
            .any(|t| t.sends.iter().any(|s| s.target == bus))
        {
            project.remove_track(bus);
        }
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

/// The channel strip modules a preparation sets.
pub const MODULES: [&str; 4] = ["filters", "gate", "eq", "comp"];

/// The module of a channel strip parameter, among [`MODULES`].
pub fn module_of(param: &str) -> Option<&'static str> {
    match param {
        "filters" | "hpf" | "lpf" | "slope" => Some("filters"),
        "gate" => Some("gate"),
        "eq" => Some("eq"),
        "comp" => Some("comp"),
        p if p.starts_with("gate_") => Some("gate"),
        p if p.starts_with("comp_") => Some("comp"),
        p if ["low_", "lmf_", "hmf_", "high_"]
            .iter()
            .any(|band| p.starts_with(band)) =>
        {
            Some("eq")
        }
        _ => None,
    }
}

/// The channel strip modules set by hand on a track with these effects: switched on where no
/// preparation was made, or changed since the preparation wrote them.
pub fn hands(inserts: &[crate::model::Insert], prepared: Option<&Prepared>) -> Vec<&'static str> {
    let Some(strip) = inserts.iter().find(|i| i.effect == routing::STRIP) else {
        return Vec::new();
    };
    let done = prepared.filter(|p| p.cleaned || p.brought_out);
    // Bypassed, its modules are off: what they were counts.
    let switched = done.and_then(|d| d.bypassed.as_ref());
    let value = |k: &str| {
        switched
            .and_then(|b| b.switches.iter().find(|s| s.0 == k).map(|s| s.1))
            .or_else(|| strip.params.get(k).copied())
    };
    let on = |m: &str| value(m).unwrap_or(0.0) != 0.0;
    MODULES
        .into_iter()
        .filter(|m| match done {
            None => on(m),
            Some(d) if d.hands.iter().any(|h| h == m) => true,
            // Prepared before this was kept: nothing tells.
            Some(d) if d.strip.is_empty() => false,
            Some(d) => d
                .strip
                .iter()
                .filter(|(k, _)| module_of(k) == Some(m))
                .any(|(k, v)| value(k).is_some_and(|now| (now - v).abs() > 1e-3)),
        })
        .collect()
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
    hands: &[String],
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
                if !by_hand(hands, param) && !by_hand(&done.hands, param) {
                    routing::set_insert_param(project, channel, id, param, value);
                }
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
        let buses = space_buses(project);
        let Some(t) = project.track_mut(track) else {
            continue;
        };
        if let Some(pan) = done.pan {
            t.pan = pan;
        }
        if done.space {
            t.sends.retain(|s| !buses.contains(&s.target));
        }
        if done.console_set && console {
            t.console = done.console;
        }
        if let Some(gain) = done.gain {
            t.gain_db = gain;
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
            gain: done.gain.filter(|_| brought_out),
            bypassed: None,
            ..done
        });
    }
}

/// `param` belongs to a module of `hands`.
fn by_hand(hands: &[String], param: &str) -> bool {
    module_of(param).is_some_and(|m| hands.iter().any(|h| h == m))
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
        &p.hands,
    );
    let before = project.track(track)?.prepared.clone();
    // A track given only a place in the mix (fader, pan, send) gets no strip for it.
    let needed = !p.strip.is_empty() || p.strip_gain != 0.0 || !p.ride.is_empty();
    let strip = if needed {
        Some(routing::ensure_strip(project, channel, p.after_first)?)
    } else {
        routing::strip(project, channel).map(|s| s.id)
    };
    // What is written on the strip, kept to tell later what the hand changed.
    let mut wrote: Vec<(String, f32)> =
        before.as_ref().map(|b| b.strip.clone()).unwrap_or_default();
    let mut write = |project: &mut Project, param: &str, value: f32| {
        let Some(strip) = strip.filter(|_| !by_hand(&p.hands, param)) else {
            return;
        };
        routing::set_insert_param(project, channel, strip, param, value);
        match wrote.iter_mut().find(|w| w.0 == param) {
            Some(w) => w.1 = value,
            None => wrote.push((param.to_owned(), value)),
        }
    };
    for (part, on) in [(true, p.clean), (false, p.bring_out)] {
        if on {
            for &(param, value) in reset(part) {
                write(project, param, value);
            }
        }
    }
    for (param, value) in &p.strip {
        write(project, param, *value);
    }
    if let Some(s) = strip_mut(project, track) {
        s.gain_db += p.strip_gain;
    }
    let inserts = if p.clean {
        place(project, track, &cleaning, &reuse, strip)?
    } else {
        before
            .as_ref()
            .map(|b| b.inserts.clone())
            .unwrap_or_default()
    };
    let out_inserts = if p.bring_out {
        let after = inserts.last().copied().or(strip);
        place(project, track, &out, &reuse_out, after)?
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
    for (bus, db) in p.space.into_iter().chain(p.echo) {
        if match routing::add_send(project, track, bus) {
            Ok(()) => true,
            Err(e) => e == routing::RoutingError::Duplicate,
        } && let Some(send) = project
            .track_mut(track)?
            .sends
            .iter_mut()
            .find(|s| s.target == bus)
        {
            send.gain_db = db;
            send.mute = false;
            space = true;
        }
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
    // The balance: the fader it had first is kept the same way (`take_back` gave it back).
    let gain = p.gain.map(|to| {
        let first = done.gain.unwrap_or(t.gain_db);
        t.gain_db = to.clamp(-60.0, 12.0);
        first
    });
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
        gain,
        hands: p.hands.clone(),
        strip: wrote,
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
    // What the hand changed since on the strip stays.
    let kept: Vec<String> = project
        .track(track)
        .map(|t| hands(&t.inserts, t.prepared.as_ref()))
        .unwrap_or_default()
        .into_iter()
        .map(str::to_owned)
        .collect();
    take_back(project, track, (true, true), (false, false), true, &kept);
    if let (Some(trim), Some(t)) = (trim, project.track_mut(track)) {
        t.trim_db = trim;
        if let Some(prepared) = &mut t.prepared {
            prepared.trim = None;
        }
    }
}

/// The most the limiter gives back of the room kept, dB: the peaks, lowered to -6 dBFS, then
/// lose 7 dB at most under its ceiling of -1 dB. Past it the mix stays quieter.
pub const HEADROOM_MAKEUP: f32 = 12.0;

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
    // The limiter gives the loudness back as long as it does not have to squash the peaks:
    // past it, the mix stays a little quieter.
    routing::set_insert_param(
        project,
        channel,
        h.makeup,
        "gain",
        (base + db.min(HEADROOM_MAKEUP)).clamp(0.0, 24.0),
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
    if done.bypassed.is_some() == on || is_space(&done.instrument) {
        return;
    }
    let strip = routing::strip(project, channel).map(|s| (s.id, s.params.clone()));
    let ride = ride_target(project, track);
    let buses = space_buses(project);
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
                if value != 0.0 && !done.hands.iter().any(|h| h == m) {
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
        for send in t
            .sends
            .iter_mut()
            .filter(|s| done.space && buses.contains(&s.target) && !s.mute)
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
        if let Some(gain) = done.gain {
            b.gain = Some(t.gain_db);
            t.gain_db = gain;
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
        if b.send {
            for send in t.sends.iter_mut().filter(|s| buses.contains(&s.target)) {
                send.mute = false;
            }
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
        if let (Some(_), Some(gain)) = (done.gain, b.gain) {
            t.gain_db = gain;
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
        let bus = ensure_space_bus(p, SPACE, "Space", ("reverb", &[("decay", 1.5)]));
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
        assert_eq!(space_bus(&p, SPACE), None, "the empty space bus is gone");
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

    /// A track whose channel strip was set by hand: its EQ on, a boost in the lows.
    fn hand_set() -> (Project, TrackId) {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Bass");
        let channel = Channel::Track(t);
        let strip = routing::ensure_strip(&mut p, channel, false).unwrap();
        routing::set_insert_param(&mut p, channel, strip, "eq", 1.0);
        routing::set_insert_param(&mut p, channel, strip, "low_gain", 5.0);
        (p, t)
    }

    #[test]
    fn a_module_set_by_hand_is_left_alone_and_survives_the_removal() {
        let (mut p, t) = hand_set();
        let track = p.track(t).unwrap();
        assert_eq!(hands(&track.inserts, track.prepared.as_ref()), ["eq"]);
        let asked = Preparation {
            instrument: "bass".into(),
            clean: true,
            bring_out: true,
            strip: vec![
                ("filters".into(), 1.0),
                ("hpf".into(), 30.0),
                ("eq".into(), 1.0),
                ("low_gain".into(), -2.0),
                ("comp".into(), 1.0),
            ],
            hands: vec!["eq".into()],
            ..Default::default()
        };
        prepare(&mut p, t, &asked).unwrap();
        let strip = routing::strip(&p, Channel::Track(t)).unwrap();
        assert_eq!(strip.params["low_gain"], 5.0, "the hand's EQ is kept");
        assert_eq!((strip.params["hpf"], strip.params["comp"]), (30.0, 1.0));
        // Bypassed, the hand's module stays on; removed, it is still there.
        set_bypassed(&mut p, t, true);
        let strip = routing::strip(&p, Channel::Track(t)).unwrap();
        assert_eq!((strip.params["eq"], strip.params["comp"]), (1.0, 0.0));
        unprepare(&mut p, t);
        let strip = routing::strip(&p, Channel::Track(t)).unwrap();
        assert_eq!((strip.params["eq"], strip.params["low_gain"]), (1.0, 5.0));
        assert_eq!(
            strip.params["comp"], 0.0,
            "what the preparation set is gone"
        );
    }

    #[test]
    fn what_the_hand_changes_after_a_preparation_is_told_apart() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Lead vox");
        prepare(&mut p, t, &cleaning()).unwrap();
        let track = p.track(t).unwrap();
        assert!(
            hands(&track.inserts, track.prepared.as_ref()).is_empty(),
            "as the preparation wrote it"
        );
        let strip = routing::strip(&p, Channel::Track(t)).unwrap().id;
        routing::set_insert_param(&mut p, Channel::Track(t), strip, "hpf", 140.0);
        let track = p.track(t).unwrap();
        assert_eq!(hands(&track.inserts, track.prepared.as_ref()), ["filters"]);
        // Removing the preparation leaves the filters the hand moved.
        unprepare(&mut p, t);
        let strip = routing::strip(&p, Channel::Track(t)).unwrap();
        assert_eq!((strip.params["filters"], strip.params["hpf"]), (1.0, 140.0));
    }

    #[test]
    fn the_balance_sets_the_fader_and_gives_it_back() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Keys");
        p.track_mut(t).unwrap().gain_db = -1.0;
        let placed = Preparation {
            instrument: "keys".into(),
            bring_out: true,
            gain: Some(-6.5),
            ..Default::default()
        };
        prepare(&mut p, t, &placed).unwrap();
        let track = p.track(t).unwrap();
        assert_eq!(track.gain_db, -6.5);
        assert!(
            track.inserts.is_empty(),
            "a place in the mix asks for no channel strip"
        );
        // Set again, the first fader is still the one to come back to.
        prepare(
            &mut p,
            t,
            &Preparation {
                gain: Some(-3.0),
                ..placed.clone()
            },
        )
        .unwrap();
        assert_eq!(p.track(t).unwrap().gain_db, -3.0);
        set_bypassed(&mut p, t, true);
        assert_eq!(p.track(t).unwrap().gain_db, -1.0);
        set_bypassed(&mut p, t, false);
        assert_eq!(p.track(t).unwrap().gain_db, -3.0);
        // Without the balance, the fader is as it was; removed too.
        prepare(
            &mut p,
            t,
            &Preparation {
                gain: None,
                ..placed
            },
        )
        .unwrap();
        assert_eq!(p.track(t).unwrap().gain_db, -1.0);
        unprepare(&mut p, t);
        assert_eq!(p.track(t).unwrap().gain_db, -1.0);
    }

    #[test]
    fn each_space_has_its_bus_and_the_unused_ones_go() {
        let mut p = Project::new("t", 48_000);
        let (vox, toms) = (p.add_track("Vox"), p.add_track("Toms"));
        let plate = ensure_space_bus(
            &mut p,
            SPACES[1],
            "Space · Plate",
            ("reverb", &[("decay", 1.4)]),
        );
        let room = ensure_space_bus(
            &mut p,
            SPACES[0],
            "Space · Room",
            ("reverb", &[("decay", 0.6)]),
        );
        assert_ne!(plate, room);
        assert_eq!(
            ensure_space_bus(&mut p, SPACES[1], "again", ("reverb", &[("decay", 9.0)])),
            plate,
            "the one already there, as it is"
        );
        let reverb = &p.track(plate).unwrap().inserts[0];
        assert_eq!(reverb.params["decay"], 1.4);
        for (track, bus) in [(vox, plate), (toms, room)] {
            let sent = Preparation {
                bring_out: true,
                space: Some((bus, -12.0)),
                ..Default::default()
            };
            prepare(&mut p, track, &sent).unwrap();
        }
        assert_eq!(space_buses(&p).len(), 2);
        assert!(is_space(SPACE) && is_space("space_room") && !is_space("vocal_male"));
        // The toms no longer send: their room goes, the plate stays.
        unprepare(&mut p, toms);
        tidy_space(&mut p);
        assert_eq!(space_buses(&p), [plate]);
        assert_eq!(space_bus(&p, SPACES[0]), None);
    }

    #[test]
    fn the_limiter_gives_back_no_more_than_it_can_without_squashing() {
        let mut p = Project::new("t", 48_000);
        master_headroom(&mut p, Some(4.0));
        let gains = |p: &Project| -> Vec<f32> {
            p.master.inserts.iter().map(|i| i.params["gain"]).collect()
        };
        assert_eq!(gains(&p), [-4.0, 4.0]);
        // A mix peaking far too high is lowered all the way, and not brought back whole.
        master_headroom(&mut p, Some(15.0));
        assert_eq!(gains(&p), [-15.0, HEADROOM_MAKEUP]);
        master_headroom(&mut p, None);
        assert!(p.master.inserts.is_empty());
    }

    #[test]
    fn console_amounts_of_older_projects_keep_their_push() {
        use crate::console::{Console, ConsoleModel};
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Vox");
        p.console = Console {
            model: ConsoleModel::Iron,
            amount: 100.0,
        };
        p.track_mut(t).unwrap().console = Some(Console {
            model: ConsoleModel::Velvet,
            amount: 50.0,
        });
        crate::legacy::rescale_console_amounts(&mut p);
        // 100 % was the console run hot, 50 % the console as measured: now 50 % and 25 %.
        assert_eq!(p.console.amount, 50.0);
        assert_eq!(p.track(t).unwrap().console.unwrap().amount, 25.0);
    }
}
