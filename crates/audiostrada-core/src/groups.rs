//! Track groups: members share level, pan and send changes (relative), switches (mute, solo,
//! record arm, inserts bypass, polarity), automation mode, and the selection and editing of
//! their clips. A group can follow a folder: its members are then the folder's tracks.

use serde::{Deserialize, Serialize};

use crate::automation::AutomationMode;
use crate::model::{ClipId, MAX_TRACK_GAIN_DB, MIN_GAIN_DB, Project, TrackId};

fn yes() -> bool {
    true
}

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
    #[serde(default)]
    pub pan: bool,
    #[serde(default)]
    pub sends: bool,
    /// Bypass of the inserts.
    #[serde(default)]
    pub fx: bool,
    #[serde(default)]
    pub automation: bool,
    #[serde(default)]
    pub polarity: bool,
    /// Selecting and editing clips, and selecting the tracks in the timeline.
    #[serde(default = "yes")]
    pub edit: bool,
    /// The folder whose tracks are the members, in place of `tracks`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<TrackId>,
}

/// What a group links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Gain,
    Pan,
    Sends,
    Mute,
    Solo,
    Arm,
    Fx,
    Automation,
    Polarity,
    Edit,
}

impl Link {
    pub const ALL: [Link; 10] = [
        Link::Gain,
        Link::Pan,
        Link::Sends,
        Link::Mute,
        Link::Solo,
        Link::Arm,
        Link::Fx,
        Link::Automation,
        Link::Polarity,
        Link::Edit,
    ];
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
            pan: false,
            sends: false,
            fx: true,
            automation: true,
            polarity: false,
            edit: true,
            folder: None,
        }
    }

    pub fn link_mut(&mut self, link: Link) -> &mut bool {
        match link {
            Link::Gain => &mut self.gain,
            Link::Pan => &mut self.pan,
            Link::Sends => &mut self.sends,
            Link::Mute => &mut self.mute,
            Link::Solo => &mut self.solo,
            Link::Arm => &mut self.arm,
            Link::Fx => &mut self.fx,
            Link::Automation => &mut self.automation,
            Link::Polarity => &mut self.polarity,
            Link::Edit => &mut self.edit,
        }
    }

    pub fn links(&self, link: Link) -> bool {
        self.active
            && match link {
                Link::Gain => self.gain,
                Link::Pan => self.pan,
                Link::Sends => self.sends,
                Link::Mute => self.mute,
                Link::Solo => self.solo,
                Link::Arm => self.arm,
                Link::Fx => self.fx,
                Link::Automation => self.automation,
                Link::Polarity => self.polarity,
                Link::Edit => self.edit,
            }
    }
}

/// The tracks of a group: its folder's, or those added to it.
pub fn members(project: &Project, group: &TrackGroup) -> Vec<TrackId> {
    match group.folder {
        Some(folder) => crate::routing::subtree(project, folder)
            .into_iter()
            .filter(|t| *t != folder)
            .collect(),
        None => group.tracks.clone(),
    }
}

/// Group of a track, if any: one it was added to first, else its folder's.
pub fn group_of(project: &Project, track: TrackId) -> Option<usize> {
    project
        .groups
        .iter()
        .position(|g| g.folder.is_none() && g.tracks.contains(&track))
        .or_else(|| {
            project
                .groups
                .iter()
                .position(|g| g.folder.is_some() && members(project, g).contains(&track))
        })
}

/// The group following `folder`, if any.
pub fn folder_group(project: &Project, folder: TrackId) -> Option<usize> {
    project.groups.iter().position(|g| g.folder == Some(folder))
}

/// The other tracks a change of `link` on `track` also applies to.
pub fn linked(project: &Project, track: TrackId, link: Link) -> Vec<TrackId> {
    if project.groups_off {
        return Vec::new();
    }
    group_of(project, track)
        .map(|g| &project.groups[g])
        .filter(|g| g.links(link))
        .map(|g| {
            members(project, g)
                .into_iter()
                .filter(|t| *t != track)
                .collect()
        })
        .unwrap_or_default()
}

/// Runs `f` with the groups suspended when `alone` (a control moved with the clutch key).
pub fn maybe_alone<R>(project: &mut Project, alone: bool, f: impl FnOnce(&mut Project) -> R) -> R {
    let off = project.groups_off;
    project.groups_off |= alone;
    let r = f(project);
    project.groups_off = off;
    r
}

/// Moves `track` into group `group` (out of any other), or out of every group with `None`.
/// Folder groups follow their folder and take no one.
pub fn set_group(project: &mut Project, track: TrackId, group: Option<usize>) {
    if group
        .and_then(|g| project.groups.get(g))
        .is_some_and(|g| g.folder.is_some())
    {
        return;
    }
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

/// Sets a track's pan and moves its linked tracks' by as much.
pub fn set_pan(project: &mut Project, track: TrackId, pan: f32) {
    let Some(old) = project.track(track).map(|t| t.pan) else {
        return;
    };
    for id in linked(project, track, Link::Pan) {
        if let Some(t) = project.track_mut(id) {
            t.pan = (t.pan + pan - old).clamp(-1.0, 1.0);
        }
    }
    if let Some(t) = project.track_mut(track) {
        t.pan = pan;
    }
}

/// Sets the level of a track's send to `target`, and moves the linked tracks' sends to the same
/// place by as many dB.
pub fn set_send_gain(project: &mut Project, track: TrackId, target: TrackId, gain_db: f32) {
    let Some(old) = project
        .track(track)
        .and_then(|t| t.sends.iter().find(|s| s.target == target))
        .map(|s| s.gain_db)
    else {
        return;
    };
    for id in linked(project, track, Link::Sends) {
        if let Some(s) = project
            .track_mut(id)
            .and_then(|t| t.sends.iter_mut().find(|s| s.target == target))
        {
            s.gain_db = (s.gain_db + gain_db - old).clamp(MIN_GAIN_DB, MAX_TRACK_GAIN_DB);
        }
    }
    if let Some(s) = project
        .track_mut(track)
        .and_then(|t| t.sends.iter_mut().find(|s| s.target == target))
    {
        s.gain_db = gain_db;
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
                Link::Fx => t.fx_off = on,
                Link::Polarity => t.invert = on,
                _ => {}
            }
        }
    }
}

/// Sets the automation mode of a track and its linked tracks.
pub fn set_automation_mode(project: &mut Project, track: TrackId, mode: AutomationMode) {
    let mut all = linked(project, track, Link::Automation);
    all.push(track);
    for id in all {
        if let Some(t) = project.track_mut(id) {
            t.automation_mode = mode;
        }
    }
}

/// `tracks` and the tracks edited along with them.
pub fn edit_tracks(project: &Project, tracks: &[TrackId]) -> Vec<TrackId> {
    let mut out = tracks.to_vec();
    for &t in tracks {
        for m in linked(project, t, Link::Edit) {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

/// `clips` and the clips edited along with them: those of the linked tracks overlapping them.
pub fn edit_clips(project: &Project, clips: &[ClipId]) -> Vec<ClipId> {
    let mut out = clips.to_vec();
    for &id in clips {
        let Some((t, c)) = crate::edit::locate(project, id) else {
            continue;
        };
        let track = &project.tracks[t];
        let (start, end) = (track.clips[c].start, track.clips[c].end());
        for m in linked(project, track.id, Link::Edit) {
            let Some(member) = project.track(m) else {
                continue;
            };
            for clip in &member.clips {
                if clip.start < end && clip.end() > start && !out.contains(&clip.id) {
                    out.push(clip.id);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::Source;

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

    #[test]
    fn pan_sends_switches_and_mode_follow_when_linked() {
        let mut p = Project::new("t", 48_000);
        let (a, b, bus) = (p.add_track("a"), p.add_track("b"), p.add_track("bus"));
        for t in [a, b] {
            crate::routing::add_send(&mut p, t, bus).unwrap();
        }
        p.track_mut(b).unwrap().pan = -0.5;
        p.groups.push(TrackGroup::new("G", [0; 3]));
        set_group(&mut p, a, Some(0));
        set_group(&mut p, b, Some(0));
        set_pan(&mut p, a, 0.3);
        assert_eq!(p.track(b).unwrap().pan, -0.5, "pan not linked by default");
        p.groups[0].pan = true;
        p.groups[0].sends = true;
        set_pan(&mut p, a, 0.5);
        assert!((p.track(b).unwrap().pan + 0.3).abs() < 1e-6);
        let before = p.track(b).unwrap().sends[0].gain_db;
        let a_send = p.track(a).unwrap().sends[0].gain_db;
        set_send_gain(&mut p, a, bus, a_send - 4.0);
        assert_eq!(p.track(b).unwrap().sends[0].gain_db, before - 4.0);
        set_switch(&mut p, a, Link::Fx, true);
        assert!(p.track(b).unwrap().fx_off);
        set_automation_mode(&mut p, b, AutomationMode::Touch);
        assert_eq!(p.track(a).unwrap().automation_mode, AutomationMode::Touch);
        // The clutch, then every group switched off.
        maybe_alone(&mut p, true, |p| set_switch(p, a, Link::Mute, true));
        assert!(!p.track(b).unwrap().mute);
        assert!(!p.groups_off);
        p.groups_off = true;
        set_switch(&mut p, a, Link::Solo, true);
        assert!(!p.track(b).unwrap().solo);
    }

    #[test]
    fn a_folder_group_follows_the_folder_and_edits_overlapping_clips() {
        let mut p = Project::new("t", 48_000);
        let (drums, kick, snare, bass) = (
            p.add_track("drums"),
            p.add_track("kick"),
            p.add_track("snare"),
            p.add_track("bass"),
        );
        for t in [kick, snare] {
            crate::routing::set_parent(&mut p, t, Some(drums)).unwrap();
        }
        let mut group = TrackGroup::new("Drums", [0; 3]);
        group.folder = Some(drums);
        p.groups.push(group);
        assert_eq!(group_of(&p, snare), Some(0));
        assert_eq!(group_of(&p, drums), None);
        assert_eq!(linked(&p, kick, Link::Mute), vec![snare]);
        set_group(&mut p, bass, Some(0));
        assert_eq!(group_of(&p, bass), None, "membership follows the folder");

        let s = p.add_source(Source {
            path: PathBuf::from("a.wav"),
            channels: 1,
            sample_rate: 48_000,
            frames: 100,
        });
        let k = p.add_clip(kick, s, 0).unwrap();
        let s1 = p.add_clip(snare, s, 50).unwrap();
        p.add_clip(snare, s, 200).unwrap();
        let b = p.add_clip(bass, s, 0).unwrap();
        assert_eq!(edit_clips(&p, &[k]), vec![k, s1]);
        assert_eq!(edit_clips(&p, &[b]), vec![b]);
        assert_eq!(edit_tracks(&p, &[snare]), vec![snare, kick]);
    }
}
