//! Writes a track's preparation (see `audiostrada_dsp::prepare`) into the project: settings of
//! its channel strip, the effects added after it (a Strada-EQ for its resonances, a de-esser),
//! a ride of the strip's output; and takes it back out.

use crate::automation::{self, AutomationPoint, AutomationTarget};
use crate::model::{Prepared, Project, TrackId};
use crate::routing::{self, Channel};

/// An effect added after the channel strip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Added {
    pub effect: String,
    pub params: Vec<(String, f32)>,
    /// Its output gain, dB: what keeps the level it had going in.
    pub gain_db: f32,
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
    /// The effects of the cleaning, in their order after the strip.
    pub inserts: Vec<Added>,
    /// Gain of the strip's output (project samples, dB); empty, no ride.
    pub ride: Vec<(u64, f32)>,
    /// The track leads with an instrument: the strip goes after it.
    pub after_first: bool,
}

/// What a part of the preparation turns off before setting its own.
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
/// added (unless `keep`: they are about to be set again), the ride, and the gain kept on the
/// strip.
fn take_back(project: &mut Project, track: TrackId, clean: bool, bring_out: bool, keep: bool) {
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
        if part {
            for &id in done.inserts.iter().filter(|_| !keep) {
                routing::remove_insert(project, channel, id);
            }
        } else if let Some(target) = ride_target(project, track)
            && let Some(t) = project.track_mut(track)
        {
            t.automation.retain(|l| l.target != target);
        }
    }
    if let Some(s) = strip_mut(project, track) {
        s.gain_db -= done.strip_gain;
    }
    let cleaned = done.cleaned && !clean;
    let brought_out = done.brought_out && !bring_out;
    if let Some(t) = project.track_mut(track) {
        t.prepared = (cleaned || brought_out).then(|| Prepared {
            cleaned,
            brought_out,
            inserts: if cleaned { done.inserts } else { Vec::new() },
            strip_gain: 0.0,
            ..done
        });
    }
}

/// Prepares `track`: what it did before for the same parts is replaced, never added to.
pub fn prepare(project: &mut Project, track: TrackId, p: &Preparation) -> Option<()> {
    let channel = Channel::Track(track);
    // The same effects as last time stay in place, only their settings change: nothing is
    // rebuilt under a preparation heard while it is adjusted.
    let reuse: Vec<crate::model::InsertId> = project
        .track(track)
        .and_then(|t| {
            let done = t.prepared.as_ref().filter(|d| d.cleaned && p.clean)?;
            let same = done.inserts.len() == p.inserts.len()
                && done.inserts.iter().zip(&p.inserts).all(|(id, a)| {
                    t.inserts
                        .iter()
                        .any(|i| i.id == *id && i.effect == a.effect)
                });
            same.then(|| done.inserts.clone())
        })
        .unwrap_or_default();
    take_back(project, track, p.clean, p.bring_out, !reuse.is_empty());
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
    let mut added = Vec::new();
    let first = routing::inserts(project, channel)
        .iter()
        .position(|i| i.id == strip)
        .map_or(0, |i| i + 1);
    for (index, a) in (first..).zip(&p.inserts) {
        let id = match reuse.get(index - first) {
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
            None => routing::add_insert_at(project, channel, (None, index), &a.effect)?,
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
    }
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
    let before = project.track(track)?.prepared.clone();
    let t = project.track_mut(track)?;
    t.prepared = Some(Prepared {
        instrument: p.instrument.clone(),
        cleaned: p.clean || before.as_ref().is_some_and(|b| b.cleaned),
        brought_out: p.bring_out || before.as_ref().is_some_and(|b| b.brought_out),
        inserts: if p.clean {
            added
        } else {
            before.map(|b| b.inserts).unwrap_or_default()
        },
        strip_gain: p.strip_gain,
    });
    Some(())
}

/// Takes the whole preparation of `track` back out.
pub fn unprepare(project: &mut Project, track: TrackId) {
    take_back(project, track, true, true, false);
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
                },
                Added {
                    effect: "deesser".into(),
                    params: vec![("frequency".into(), 6000.0)],
                    gain_db: 0.0,
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
}
