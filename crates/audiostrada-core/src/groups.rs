//! Track groups: members share level changes (relative), mute, solo and record arm.

use serde::{Deserialize, Serialize};

use crate::model::{MAX_TRACK_GAIN_DB, MIN_GAIN_DB, Project, TrackId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackGroup {
    pub name: String,
    pub color: [u8; 3],
    pub tracks: Vec<TrackId>,
    /// Off: members behave on their own.
    pub active: bool,
    pub gain: bool,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
}

/// What a group links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Gain,
    Mute,
    Solo,
    Arm,
}

impl TrackGroup {
    pub fn new(name: &str, color: [u8; 3]) -> Self {
        Self {
            name: name.to_owned(),
            color,
            tracks: Vec::new(),
            active: true,
            gain: true,
            mute: true,
            solo: true,
            arm: true,
        }
    }

    fn links(&self, link: Link) -> bool {
        self.active
            && match link {
                Link::Gain => self.gain,
                Link::Mute => self.mute,
                Link::Solo => self.solo,
                Link::Arm => self.arm,
            }
    }
}

/// Group of a track, if any.
pub fn group_of(project: &Project, track: TrackId) -> Option<usize> {
    project
        .groups
        .iter()
        .position(|g| g.tracks.contains(&track))
}

/// The other tracks a change of `link` on `track` also applies to.
pub fn linked(project: &Project, track: TrackId, link: Link) -> Vec<TrackId> {
    group_of(project, track)
        .map(|g| &project.groups[g])
        .filter(|g| g.links(link))
        .map(|g| g.tracks.iter().copied().filter(|t| *t != track).collect())
        .unwrap_or_default()
}

/// Moves `track` into group `group` (out of any other), or out of every group with `None`.
pub fn set_group(project: &mut Project, track: TrackId, group: Option<usize>) {
    for g in &mut project.groups {
        g.tracks.retain(|t| *t != track);
    }
    if let Some(g) = group.and_then(|g| project.groups.get_mut(g)) {
        g.tracks.push(track);
    }
}

/// Sets a track's level and moves its linked tracks and `also` (the other selected tracks) by
/// as many dB.
pub fn set_gain_with(project: &mut Project, track: TrackId, gain_db: f32, also: &[TrackId]) {
    let Some(old) = project.track(track).map(|t| t.gain_db) else {
        return;
    };
    let delta = gain_db - old;
    let mut others = linked(project, track, Link::Gain);
    for &t in also {
        if t != track && !others.contains(&t) {
            others.push(t);
        }
    }
    for id in others {
        if let Some(t) = project.track_mut(id) {
            t.gain_db = (t.gain_db + delta).clamp(MIN_GAIN_DB, MAX_TRACK_GAIN_DB);
        }
    }
    if let Some(t) = project.track_mut(track) {
        t.gain_db = gain_db;
    }
}

/// Sets a switch on a track and its linked tracks.
pub fn set_switch(project: &mut Project, track: TrackId, link: Link, on: bool) {
    let mut all = linked(project, track, link);
    all.push(track);
    for id in all {
        if let Some(t) = project.track_mut(id) {
            match link {
                Link::Mute => t.mute = on,
                Link::Solo => t.solo = on,
                Link::Arm => t.armed = on,
                Link::Gain => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_follows_the_fader_moved() {
        let mut p = Project::new("t", 48_000);
        let (a, b, c) = (p.add_track("a"), p.add_track("b"), p.add_track("c"));
        p.track_mut(b).unwrap().gain_db = -6.0;
        set_gain_with(&mut p, a, -3.0, &[a, b]);
        let gains: Vec<f32> = [a, b, c]
            .iter()
            .map(|t| p.track(*t).unwrap().gain_db)
            .collect();
        assert_eq!(gains, [-3.0, -9.0, 0.0]);
    }

    #[test]
    fn members_follow_each_other() {
        let mut p = Project::new("t", 48_000);
        let (a, b, c) = (p.add_track("a"), p.add_track("b"), p.add_track("c"));
        p.track_mut(b).unwrap().gain_db = -6.0;
        p.groups.push(TrackGroup::new("Drums", [200, 80, 80]));
        set_group(&mut p, a, Some(0));
        set_group(&mut p, b, Some(0));
        set_gain_with(&mut p, a, -3.0, &[]);
        assert_eq!(p.track(b).unwrap().gain_db, -9.0, "relative");
        assert_eq!(p.track(c).unwrap().gain_db, 0.0);
        set_switch(&mut p, b, Link::Mute, true);
        assert!(p.track(a).unwrap().mute);
        p.groups[0].solo = false;
        set_switch(&mut p, a, Link::Solo, true);
        assert!(!p.track(b).unwrap().solo, "solo not linked");
        p.groups[0].active = false;
        set_gain_with(&mut p, a, 0.0, &[]);
        assert_eq!(p.track(b).unwrap().gain_db, -9.0, "inactive group");
        set_group(&mut p, a, None);
        assert_eq!(p.groups[0].tracks, vec![b]);
    }
}
