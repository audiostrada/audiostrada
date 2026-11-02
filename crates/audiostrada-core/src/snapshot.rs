//! Mix snapshots: the console settings at one moment (levels, pans, mutes, solos, sends,
//! insert parameters and bypasses), stored in the project and recalled at once.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{InsertId, Project, TrackId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InsertState {
    pub bypass: bool,
    pub params: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StripState {
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Send level and mute by destination.
    pub sends: BTreeMap<TrackId, (f32, bool)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixSnapshot {
    pub name: String,
    pub master_gain_db: f32,
    pub tracks: BTreeMap<TrackId, StripState>,
    pub inserts: BTreeMap<InsertId, InsertState>,
}

impl MixSnapshot {
    /// The console as it is now.
    pub fn capture(project: &Project, name: &str) -> Self {
        let mut inserts = BTreeMap::new();
        let all = project
            .tracks
            .iter()
            .flat_map(|t| t.inserts.iter())
            .chain(project.master.inserts.iter());
        for i in all {
            inserts.insert(
                i.id,
                InsertState {
                    bypass: i.bypass,
                    params: i.params.clone(),
                },
            );
        }
        Self {
            name: name.to_owned(),
            master_gain_db: project.master.gain_db,
            tracks: project
                .tracks
                .iter()
                .map(|t| {
                    (
                        t.id,
                        StripState {
                            gain_db: t.gain_db,
                            pan: t.pan,
                            mute: t.mute,
                            solo: t.solo,
                            sends: t
                                .sends
                                .iter()
                                .map(|s| (s.target, (s.gain_db, s.mute)))
                                .collect(),
                        },
                    )
                })
                .collect(),
            inserts,
        }
    }

    /// Puts the console back as captured; tracks, sends and inserts added since keep their
    /// settings.
    pub fn recall(&self, project: &mut Project) {
        project.master.gain_db = self.master_gain_db;
        apply_inserts(&mut project.master.inserts, &self.inserts);
        let ids: Vec<TrackId> = project.tracks.iter().map(|t| t.id).collect();
        for id in ids {
            let Some(state) = self.tracks.get(&id) else {
                continue;
            };
            let Some(t) = project.track_mut(id) else {
                continue;
            };
            t.gain_db = state.gain_db;
            t.pan = state.pan;
            t.mute = state.mute;
            t.solo = state.solo;
            for send in &mut t.sends {
                if let Some(&(gain, mute)) = state.sends.get(&send.target) {
                    send.gain_db = gain;
                    send.mute = mute;
                }
            }
            apply_inserts(&mut t.inserts, &self.inserts);
        }
    }
}

fn apply_inserts(inserts: &mut [crate::model::Insert], states: &BTreeMap<InsertId, InsertState>) {
    for insert in inserts {
        if let Some(state) = states.get(&insert.id) {
            insert.bypass = state.bypass;
            insert.params = state.params.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{self, Channel};

    #[test]
    fn recalls_what_was_captured() {
        let mut p = Project::new("t", 48_000);
        let a = p.add_track("a");
        let bus = p.add_track("bus");
        routing::add_send(&mut p, a, bus).unwrap();
        let eq = routing::add_insert(&mut p, Channel::Track(a), "eq", Default::default()).unwrap();
        routing::set_insert_param(&mut p, Channel::Track(a), eq, "b1_gain", 3.0);
        let snapshot = MixSnapshot::capture(&p, "Mix A");
        let t = p.track_mut(a).unwrap();
        t.gain_db = -12.0;
        t.mute = true;
        t.sends[0].gain_db = -30.0;
        routing::set_insert_param(&mut p, Channel::Track(a), eq, "b1_gain", -9.0);
        routing::set_bypass(&mut p, Channel::Track(a), eq, true);
        let late = p.add_track("late");
        p.track_mut(late).unwrap().gain_db = -3.0;
        p.master.gain_db = -1.0;
        snapshot.recall(&mut p);
        let t = p.track(a).unwrap();
        assert_eq!((t.gain_db, t.mute, t.sends[0].gain_db), (0.0, false, 0.0));
        assert_eq!(t.inserts[0].params["b1_gain"], 3.0);
        assert!(!t.inserts[0].bypass);
        assert_eq!(
            p.track(late).unwrap().gain_db,
            -3.0,
            "added later: untouched"
        );
        assert_eq!(p.master.gain_db, 0.0);
        p.snapshots.push(snapshot);
        let back = crate::file::from_json(&crate::file::to_json(&p).unwrap()).unwrap();
        assert_eq!(back.snapshots, p.snapshots, "saved with the project");
    }
}
