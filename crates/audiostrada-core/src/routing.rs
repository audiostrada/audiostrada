//! Signal routing between tracks (folders, sends, VCAs), insert chains and the editor order.
//!
//! Like Reaper, any audio track can act as a bus: its input sums its clips, its children and
//! the sends it receives. Operations refuse anything that would create a feedback loop.

use std::collections::{BTreeMap, HashSet};

use crate::automation::{AutomationLane, AutomationTarget};
use crate::model::{Branch, Insert, InsertId, Project, TrackId, TrackKind, TrackSend};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RoutingError {
    #[error("unknown track")]
    UnknownTrack,
    #[error("this routing would create a feedback loop")]
    Cycle,
    #[error("VCA tracks carry no audio")]
    NotAudio,
    #[error("the send already exists")]
    Duplicate,
}

/// A mixer channel holding inserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Track(TrackId),
    Master,
}

fn audio(project: &Project, id: TrackId) -> Result<(), RoutingError> {
    match project.track(id).map(|t| t.kind) {
        Some(TrackKind::Audio) => Ok(()),
        Some(TrackKind::Vca) => Err(RoutingError::NotAudio),
        None => Err(RoutingError::UnknownTrack),
    }
}

/// Tracks fed directly by `id`: its folder (when it sends there) and its send targets.
pub fn destinations(project: &Project, id: TrackId) -> Vec<TrackId> {
    let Some(t) = project.track(id).filter(|t| t.kind == TrackKind::Audio) else {
        return Vec::new();
    };
    let mut out: Vec<TrackId> = t.sends.iter().map(|s| s.target).collect();
    if t.master_send
        && let Some(parent) = t.parent
    {
        out.push(parent);
    }
    out
}

/// True if audio flows from `from` to `to`, directly or through other tracks.
pub fn reaches(project: &Project, from: TrackId, to: TrackId) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(id) = stack.pop() {
        if id == to {
            return true;
        }
        if seen.insert(id) {
            stack.extend(destinations(project, id));
        }
    }
    false
}

pub fn add_send(project: &mut Project, from: TrackId, to: TrackId) -> Result<(), RoutingError> {
    audio(project, from)?;
    audio(project, to)?;
    if from == to || reaches(project, to, from) {
        return Err(RoutingError::Cycle);
    }
    let track = project.track_mut(from).ok_or(RoutingError::UnknownTrack)?;
    if track.sends.iter().any(|s| s.target == to) {
        return Err(RoutingError::Duplicate);
    }
    track.sends.push(TrackSend {
        target: to,
        gain_db: 0.0,
        pre_fader: false,
        mute: false,
    });
    Ok(())
}

pub fn remove_send(project: &mut Project, from: TrackId, to: TrackId) {
    if let Some(t) = project.track_mut(from) {
        t.sends.retain(|s| s.target != to);
        t.automation
            .retain(|l| l.target != AutomationTarget::Send(to));
    }
}

/// `id` and every track inside it, at any depth.
pub fn subtree(project: &Project, id: TrackId) -> Vec<TrackId> {
    subtree_of(&project.tracks, id)
}

fn subtree_of(tracks: &[std::sync::Arc<crate::Track>], id: TrackId) -> Vec<TrackId> {
    let mut out = vec![id];
    let mut i = 0;
    while i < out.len() {
        let current = out[i];
        out.extend(
            tracks
                .iter()
                .filter(|t| t.parent == Some(current))
                .map(|t| t.id),
        );
        i += 1;
    }
    out
}

/// Puts `id` inside the folder `parent` (or at the top level).
pub fn set_parent(
    project: &mut Project,
    id: TrackId,
    parent: Option<TrackId>,
) -> Result<(), RoutingError> {
    let track = project.track(id).ok_or(RoutingError::UnknownTrack)?;
    if let Some(parent) = parent {
        audio(project, parent)?;
        if subtree(project, id).contains(&parent)
            || (track.master_send && track.kind == TrackKind::Audio && reaches(project, parent, id))
        {
            return Err(RoutingError::Cycle);
        }
    }
    if let Some(t) = project.track_mut(id) {
        t.parent = parent;
    }
    normalize_order(project);
    Ok(())
}

/// Puts several tracks inside the folder `parent` (or at the top level) keeping their order;
/// those entering the folder go after its contents.
pub fn set_parents(
    project: &mut Project,
    ids: &[TrackId],
    parent: Option<TrackId>,
) -> Result<(), RoutingError> {
    let entering: Vec<TrackId> = project
        .tracks
        .iter()
        .filter(|t| ids.contains(&t.id) && t.parent != parent)
        .map(|t| t.id)
        .collect();
    let mut result = Ok(());
    for &id in &entering {
        result = result.and(set_parent(project, id, parent));
    }
    if parent.is_some() {
        for id in entering {
            if let Some(i) = project
                .tracks
                .iter()
                .position(|t| t.id == id && t.parent == parent)
            {
                let track = project.tracks.remove(i);
                project.tracks.push(track);
            }
        }
        normalize_order(project);
    }
    result
}

/// Enables or disables the feed to the folder or master, refusing loops.
pub fn set_master_send(project: &mut Project, id: TrackId, on: bool) -> Result<(), RoutingError> {
    let parent = project.track(id).ok_or(RoutingError::UnknownTrack)?.parent;
    if on
        && let Some(parent) = parent
        && reaches(project, parent, id)
    {
        return Err(RoutingError::Cycle);
    }
    if let Some(t) = project.track_mut(id) {
        t.master_send = on;
    }
    Ok(())
}

/// Reorders tracks so that every folder is directly followed by its contents.
pub fn normalize_order(project: &mut Project) {
    let ids: Vec<TrackId> = project.tracks.iter().map(|t| t.id).collect();
    let exists = |id: Option<TrackId>| id.is_some_and(|p| ids.contains(&p));
    let mut order = Vec::with_capacity(ids.len());
    fn visit(project: &Project, id: TrackId, order: &mut Vec<TrackId>) {
        if order.contains(&id) {
            return;
        }
        order.push(id);
        for child in project.tracks.iter().filter(|t| t.parent == Some(id)) {
            visit(project, child.id, order);
        }
    }
    for t in project.tracks.iter().filter(|t| !exists(t.parent)) {
        visit(project, t.id, &mut order);
    }
    let mut tracks = std::mem::take(&mut project.tracks);
    for id in order {
        if let Some(i) = tracks.iter().position(|t| t.id == id) {
            project.tracks.push(tracks.remove(i));
        }
    }
    project.tracks.extend(tracks);
}

/// Moves tracks (with their contents) just before or after `target`, into its folder when
/// that creates no loop. Dragging tracks onto one of their own contents does nothing.
pub fn move_tracks(project: &mut Project, ids: &[TrackId], target: TrackId, after: bool) {
    let mut moved: Vec<TrackId> = Vec::new();
    for &id in ids {
        if project.track(id).is_none() || moved.contains(&id) {
            continue;
        }
        moved.extend(subtree(project, id));
    }
    if moved.contains(&target) || project.track(target).is_none() {
        return;
    }
    let parent = project.track(target).and_then(|t| t.parent);
    // Only the outermost moved tracks change folder; their contents follow.
    let tops: Vec<TrackId> = ids
        .iter()
        .copied()
        .filter(|&id| {
            project
                .track(id)
                .is_some_and(|t| t.parent.is_none_or(|p| !moved.contains(&p)))
        })
        .collect();
    for &id in &tops {
        let _ = set_parent(project, id, parent);
    }
    let mut tracks = std::mem::take(&mut project.tracks);
    let mut taken = Vec::new();
    tracks.retain(|t| {
        if moved.contains(&t.id) {
            taken.push(t.clone());
            false
        } else {
            true
        }
    });
    let at = match tracks.iter().position(|t| t.id == target) {
        Some(i) if after => {
            let inside = subtree_of(&tracks, target);
            tracks
                .iter()
                .rposition(|t| inside.contains(&t.id))
                .map_or(i + 1, |j| j + 1)
        }
        Some(i) => i,
        None => tracks.len(),
    };
    tracks.splice(at..at, taken);
    project.tracks = tracks;
    normalize_order(project);
}

/// Moves a track (with its contents) before its previous or after its next sibling.
pub fn move_track(project: &mut Project, id: TrackId, up: bool) {
    let Some(parent) = project.track(id).map(|t| t.parent) else {
        return;
    };
    let siblings: Vec<usize> = project
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.parent == parent)
        .map(|(i, _)| i)
        .collect();
    let Some(k) = siblings.iter().position(|&i| project.tracks[i].id == id) else {
        return;
    };
    let other = if up { k.checked_sub(1) } else { Some(k + 1) };
    if let Some(&j) = other.and_then(|o| siblings.get(o)) {
        project.tracks.swap(siblings[k], j);
        normalize_order(project);
    }
}

/// Number of folders containing `id`.
pub fn depth(project: &Project, id: TrackId) -> usize {
    let mut depth = 0;
    let mut current = project.track(id).and_then(|t| t.parent);
    while let Some(p) = current {
        depth += 1;
        if depth > project.tracks.len() {
            break;
        }
        current = project.track(p).and_then(|t| t.parent);
    }
    depth
}

pub fn has_children(project: &Project, id: TrackId) -> bool {
    project.tracks.iter().any(|t| t.parent == Some(id))
}

/// Indices of the tracks shown in the editor (inside no collapsed folder), with their depth.
pub fn visible_tracks(project: &Project) -> Vec<(usize, usize)> {
    project
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let mut current = t.parent;
            let mut steps = 0;
            while let Some(p) = current {
                let Some(parent) = project.track(p) else {
                    break;
                };
                if parent.collapsed {
                    return false;
                }
                current = parent.parent;
                steps += 1;
                if steps > project.tracks.len() {
                    break;
                }
            }
            true
        })
        .map(|(i, t)| (i, depth(project, t.id)))
        .collect()
}

/// Adds a VCA track: it has no audio, only a fader and a mute for its members.
pub fn add_vca(project: &mut Project, name: &str) -> TrackId {
    let id = project.add_track(name);
    if let Some(t) = project.track_mut(id) {
        t.kind = TrackKind::Vca;
        t.master_send = false;
        t.color = [150, 150, 150];
    }
    id
}

pub fn set_vca(
    project: &mut Project,
    id: TrackId,
    vca: Option<TrackId>,
) -> Result<(), RoutingError> {
    audio(project, id)?;
    if let Some(v) = vca
        && project.track(v).map(|t| t.kind) != Some(TrackKind::Vca)
    {
        return Err(RoutingError::NotAudio);
    }
    if let Some(t) = project.track_mut(id) {
        t.vca = vca;
    }
    Ok(())
}

/// Gain and mute of a track including its VCA.
pub fn effective_gain(project: &Project, id: TrackId) -> (f32, bool) {
    let Some(t) = project.track(id) else {
        return (0.0, true);
    };
    match t.vca.and_then(|v| project.track(v)) {
        Some(v) => (t.gain_db + v.gain_db, t.mute || v.mute),
        None => (t.gain_db, t.mute),
    }
}

/// Tracks that stay audible while something is soloed, or `None` when nothing is. Soloing a
/// track keeps its destinations audible; soloing a folder or a VCA solos its members.
pub fn solo_audible(project: &Project) -> Option<HashSet<TrackId>> {
    let soloed: Vec<TrackId> = project
        .tracks
        .iter()
        .filter(|t| t.solo || t.vca.and_then(|v| project.track(v)).is_some_and(|v| v.solo))
        .map(|t| t.id)
        .collect();
    if soloed.is_empty() {
        return None;
    }
    let mut audible = HashSet::new();
    for &id in &soloed {
        audible.extend(subtree(project, id));
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if audible.insert(current) || current == id {
                stack.extend(
                    destinations(project, current)
                        .into_iter()
                        .filter(|d| !audible.contains(d)),
                );
            }
        }
    }
    Some(audible)
}

fn inserts_mut(project: &mut Project, channel: Channel) -> Option<&mut Vec<Insert>> {
    match channel {
        Channel::Track(id) => project.track_mut(id).map(|t| &mut t.inserts),
        Channel::Master => Some(&mut project.master.inserts),
    }
}

fn lanes_mut(project: &mut Project, channel: Channel) -> Option<&mut Vec<AutomationLane>> {
    match channel {
        Channel::Track(id) => project.track_mut(id).map(|t| &mut t.automation),
        Channel::Master => Some(&mut project.master.automation),
    }
}

fn insert_mut(project: &mut Project, channel: Channel, id: InsertId) -> Option<&mut Insert> {
    find_in_mut(inserts_mut(project, channel)?, id)
}

fn find_in_mut(list: &mut [Insert], id: InsertId) -> Option<&mut Insert> {
    for insert in list {
        if insert.id == id {
            return Some(insert);
        }
        for branch in &mut insert.branches {
            if let Some(found) = find_in_mut(&mut branch.inserts, id) {
                return Some(found);
            }
        }
    }
    None
}

fn find_in(list: &[Insert], id: InsertId) -> Option<&Insert> {
    list.iter().find_map(|i| {
        (i.id == id)
            .then_some(i)
            .or_else(|| i.branches.iter().find_map(|b| find_in(&b.inserts, id)))
    })
}

/// An insert of a channel, in its list or in a branch of a parallel chain.
pub fn find_insert(project: &Project, channel: Channel, id: InsertId) -> Option<&Insert> {
    find_in(inserts(project, channel), id)
}

/// Every insert of a list, the ones in branches after their parallel chain.
pub fn flatten(list: &[Insert]) -> Vec<&Insert> {
    let mut out = Vec::new();
    for insert in list {
        out.push(insert);
        for branch in &insert.branches {
            out.extend(flatten(&branch.inserts));
        }
    }
    out
}

/// Effect of a parallel chain: no sound of its own, its branches mixed.
pub const PARALLEL: &str = "parallel";

/// Where an insert goes: the channel's list, or a branch (parallel chain, branch index).
pub type Place = Option<(InsertId, usize)>;

fn list_mut(project: &mut Project, channel: Channel, place: Place) -> Option<&mut Vec<Insert>> {
    let list = inserts_mut(project, channel)?;
    match place {
        None => Some(list),
        Some((parallel, branch)) => find_in_mut(list, parallel)?
            .branches
            .get_mut(branch)
            .map(|b| &mut b.inserts),
    }
}

/// Takes an insert out of a list or its branches.
fn take(list: &mut Vec<Insert>, id: InsertId) -> Option<Insert> {
    if let Some(at) = list.iter().position(|i| i.id == id) {
        return Some(list.remove(at));
    }
    list.iter_mut()
        .flat_map(|i| i.branches.iter_mut())
        .find_map(|b| take(&mut b.inserts, id))
}

/// A branch of a parallel chain, to change its name, level, mute or solo.
pub fn branch_mut(
    project: &mut Project,
    channel: Channel,
    parallel: InsertId,
    branch: usize,
) -> Option<&mut Branch> {
    insert_mut(project, channel, parallel)?
        .branches
        .get_mut(branch)
}

/// Gives an insert copy (and the inserts of its branches) new ids.
fn renew_ids(project: &mut Project, insert: &mut Insert) {
    insert.id = project.new_insert_id();
    for branch in &mut insert.branches {
        for inner in &mut branch.inserts {
            renew_ids(project, inner);
        }
    }
}

pub fn inserts(project: &Project, channel: Channel) -> &[Insert] {
    match channel {
        Channel::Track(id) => project.track(id).map_or(&[], |t| t.inserts.as_slice()),
        Channel::Master => &project.master.inserts,
    }
}

/// Effect of the channel strip, the insert ahead of the others (see [`ensure_strip`]).
pub const STRIP: &str = "channel";

/// The channel strip of a channel, if it has one.
pub fn strip(project: &Project, channel: Channel) -> Option<&Insert> {
    inserts(project, channel).iter().find(|i| i.effect == STRIP)
}

/// The channel strip of a channel, added ahead of its inserts if missing (after the first one
/// when it leads the channel: an instrument). Returns its id.
pub fn ensure_strip(
    project: &mut Project,
    channel: Channel,
    after_first: bool,
) -> Option<InsertId> {
    if let Some(strip) = strip(project, channel) {
        return Some(strip.id);
    }
    let id = add_insert(project, channel, STRIP, BTreeMap::new())?;
    let has_first = inserts(project, channel).len() > 1;
    let index = usize::from(after_first && has_first);
    place_insert(project, (channel, id), (channel, false, index), false)
}

pub fn add_insert(
    project: &mut Project,
    channel: Channel,
    effect: &str,
    params: BTreeMap<String, f32>,
) -> Option<InsertId> {
    if let Channel::Track(id) = channel {
        audio(project, id).ok()?;
    }
    let id = project.new_insert_id();
    inserts_mut(project, channel)?.push(Insert {
        id,
        effect: effect.to_owned(),
        bypass: false,
        params,
        state: None,
        files: BTreeMap::new(),
        sidechain: None,
        monitor: false,
        gain_db: 0.0,
        branches: Vec::new(),
        macros: Vec::new(),
    });
    Some(id)
}

/// Adds an effect at `index` of a channel's list or of a branch. Returns its id.
pub fn add_insert_at(
    project: &mut Project,
    channel: Channel,
    (place, index): (Place, usize),
    effect: &str,
) -> Option<InsertId> {
    let id = add_insert(project, channel, effect, BTreeMap::new())?;
    move_within(project, channel, id, (place, index))
}

/// A parallel chain of two empty branches at `index` of a channel's list: the first passes
/// the signal as it comes, effects go on the second.
pub fn add_parallel(project: &mut Project, channel: Channel, index: usize) -> Option<InsertId> {
    let id = add_insert(project, channel, PARALLEL, BTreeMap::new())?;
    if let Some(insert) = insert_mut(project, channel, id) {
        insert.branches = vec![Branch::new("A"), Branch::new("B")];
    }
    move_within(project, channel, id, (None, index))
}

/// Adds an empty branch to a parallel chain; returns its index.
pub fn add_branch(project: &mut Project, channel: Channel, parallel: InsertId) -> Option<usize> {
    let insert = insert_mut(project, channel, parallel)?;
    let name = ((b'A' + (insert.branches.len() % 26) as u8) as char).to_string();
    insert.branches.push(Branch::new(&name));
    Some(insert.branches.len() - 1)
}

/// Removes a branch and its inserts (with their automation and MIDI mappings).
pub fn remove_branch(project: &mut Project, channel: Channel, parallel: InsertId, branch: usize) {
    let Some(insert) = insert_mut(project, channel, parallel) else {
        return;
    };
    if branch >= insert.branches.len() {
        return;
    }
    let gone = insert.branches.remove(branch);
    let ids: Vec<InsertId> = flatten(&gone.inserts).iter().map(|i| i.id).collect();
    forget(project, channel, &ids);
}

/// Drops the automation and MIDI mappings of removed inserts.
fn forget(project: &mut Project, channel: Channel, ids: &[InsertId]) {
    if let Some(lanes) = lanes_mut(project, channel) {
        lanes.retain(|l| !ids.iter().any(|&id| l.target.is_param_of(id)));
    }
    project
        .midi_map
        .retain(|m| !ids.iter().any(|&id| m.target.is_param_of(id)));
}

/// Moves an insert within its channel, to `index` of its list or of a branch (a parallel
/// chain stays out of branches). Returns its id when placed.
pub fn move_within(
    project: &mut Project,
    channel: Channel,
    id: InsertId,
    (place, index): (Place, usize),
) -> Option<InsertId> {
    let parallel = find_insert(project, channel, id)?.effect == PARALLEL;
    if place.is_some() && parallel {
        return None;
    }
    // Into its own place: the old position no longer counts.
    let list = inserts_mut(project, channel)?;
    let mut index = index;
    let same: Option<usize> = match place {
        None => list.iter().filter(|i| !i.monitor).position(|i| i.id == id),
        Some((p, b)) => find_in_mut(list, p)?
            .branches
            .get(b)?
            .inserts
            .iter()
            .position(|i| i.id == id),
    };
    if same.is_some_and(|w| w < index) {
        index -= 1;
    }
    let insert = take(inserts_mut(project, channel)?, id)?;
    let list = list_mut(project, channel, place)?;
    let at = match place {
        // Among the mix inserts, before the monitoring ones of the master.
        None => list
            .iter()
            .enumerate()
            .filter(|(_, i)| !i.monitor)
            .nth(index)
            .map_or_else(
                || list.iter().rposition(|i| !i.monitor).map_or(0, |p| p + 1),
                |(p, _)| p,
            ),
        Some(_) => index.min(list.len()),
    };
    list.insert(at, insert);
    Some(id)
}

/// Adds a copy of `insert` (a saved one: preset chain, other project) at the end of a channel's
/// list, with a new id. Returns its id.
pub fn add_insert_copy(
    project: &mut Project,
    channel: Channel,
    insert: &Insert,
) -> Option<InsertId> {
    if let Channel::Track(id) = channel {
        audio(project, id).ok()?;
    }
    let mut copy = insert.clone();
    renew_ids(project, &mut copy);
    copy.monitor = copy.monitor && channel == Channel::Master;
    copy.sidechain = None;
    let id = copy.id;
    inserts_mut(project, channel)?.push(copy);
    Some(id)
}

pub fn remove_insert(project: &mut Project, channel: Channel, id: InsertId) {
    let removed = inserts_mut(project, channel).and_then(|list| take(list, id));
    let mut ids = vec![id];
    if let Some(removed) = &removed {
        ids.extend(
            removed
                .branches
                .iter()
                .flat_map(|b| flatten(&b.inserts))
                .map(|i| i.id),
        );
    }
    forget(project, channel, &ids);
}

/// Puts insert `id` of `from` before the `index`-th insert of `to` that is in the same list
/// (`monitor` or not; past the last one for a larger index), moving it or copying it with a
/// new id. A move to another channel takes its automation and MIDI mappings along. Returns
/// the id of the insert in place.
pub fn place_insert(
    project: &mut Project,
    (from, id): (Channel, InsertId),
    (to, monitor, index): (Channel, bool, usize),
    copy: bool,
) -> Option<InsertId> {
    if let Channel::Track(t) = to {
        audio(project, t).ok()?;
    }
    let mut insert = inserts(project, from).iter().find(|i| i.id == id)?.clone();
    // Moving down its own list: the insert's old place no longer counts.
    let mut index = index;
    if !copy && from == to && insert.monitor == monitor {
        let was = inserts(project, from)
            .iter()
            .filter(|i| i.monitor == monitor)
            .position(|i| i.id == id);
        if was.is_some_and(|w| w < index) {
            index -= 1;
        }
    }
    let ours = |l: &AutomationLane| l.target.is_param_of(id);
    let mut lanes: Vec<AutomationLane> = lanes_mut(project, from)
        .map(|l| l.iter().filter(|l| ours(l)).cloned().collect())
        .unwrap_or_default();
    if copy {
        renew_ids(project, &mut insert);
        for lane in &mut lanes {
            if let AutomationTarget::Param { insert: i, .. } = &mut lane.target {
                *i = insert.id;
            }
        }
    } else {
        inserts_mut(project, from)?.retain(|i| i.id != id);
        if from != to {
            if let Some(l) = lanes_mut(project, from) {
                l.retain(|l| !ours(l));
            }
            let track = match to {
                Channel::Track(t) => Some(t),
                Channel::Master => None,
            };
            for m in project.midi_map.iter_mut() {
                if m.target.is_param_of(id) {
                    m.track = track;
                }
            }
        } else {
            lanes.clear();
        }
    }
    insert.monitor = monitor && to == Channel::Master;
    let placed = insert.id;
    let list = inserts_mut(project, to)?;
    let at = list
        .iter()
        .enumerate()
        .filter(|(_, i)| i.monitor == insert.monitor)
        .nth(index)
        .map_or_else(
            || {
                list.iter()
                    .rposition(|i| i.monitor == insert.monitor)
                    .map_or(list.len(), |p| p + 1)
            },
            |(p, _)| p,
        );
    list.insert(at, insert);
    if let Some(l) = lanes_mut(project, to) {
        l.extend(lanes);
    }
    Some(placed)
}

pub fn move_insert(project: &mut Project, channel: Channel, id: InsertId, up: bool) {
    if let Some(list) = inserts_mut(project, channel)
        && let Some(i) = list.iter().position(|x| x.id == id)
    {
        let j = if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|&j| j < list.len())
        };
        if let Some(j) = j {
            list.swap(i, j);
        }
    }
}

/// Sets or removes (`None`) the file of an insert's slot.
pub fn set_insert_file(
    project: &mut Project,
    channel: Channel,
    id: InsertId,
    slot: &str,
    file: Option<std::path::PathBuf>,
) {
    if let Some(insert) = insert_mut(project, channel, id) {
        match file {
            Some(file) => {
                insert.files.insert(slot.to_owned(), file);
            }
            None => {
                insert.files.remove(slot);
            }
        }
    }
}

pub fn set_insert_param(
    project: &mut Project,
    channel: Channel,
    id: InsertId,
    name: &str,
    value: f32,
) {
    if let Some(insert) = insert_mut(project, channel, id) {
        insert.params.insert(name.to_owned(), value);
    }
}

/// Sets the gain after an insert's effect, in dB.
pub fn set_insert_gain(project: &mut Project, channel: Channel, id: InsertId, gain_db: f32) {
    if let Some(insert) = insert_mut(project, channel, id) {
        insert.gain_db = gain_db.clamp(-24.0, 24.0);
    }
}

pub fn set_bypass(project: &mut Project, channel: Channel, id: InsertId, bypass: bool) {
    if let Some(insert) = insert_mut(project, channel, id) {
        insert.bypass = bypass;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_goes_ahead_of_the_inserts_once() {
        let mut p = Project::new("t", 48_000);
        let t = p.add_track("Keys");
        let synth = add_insert(&mut p, Channel::Track(t), "synth", BTreeMap::new()).unwrap();
        add_insert(&mut p, Channel::Track(t), "reverb", BTreeMap::new()).unwrap();
        let strip = ensure_strip(&mut p, Channel::Track(t), true).unwrap();
        let order: Vec<InsertId> = inserts(&p, Channel::Track(t))
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(order[..2], [synth, strip], "after the instrument");
        assert_eq!(
            ensure_strip(&mut p, Channel::Track(t), true),
            Some(strip),
            "only one"
        );
        let master = ensure_strip(&mut p, Channel::Master, false).unwrap();
        assert_eq!(p.master.inserts.first().map(|i| i.id), Some(master));
    }

    #[test]
    fn parallel_chains_hold_their_branches() {
        let (mut p, a, b, _) = project();
        let c = Channel::Track(a);
        let eq = add_insert(&mut p, c, "eq", BTreeMap::new()).unwrap();
        let par = add_parallel(&mut p, c, 1).unwrap();
        let comp = add_insert_at(&mut p, c, (Some((par, 1)), 0), "compressor").unwrap();
        let order = |p: &Project| inserts(p, c).iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(order(&p), [eq, par]);
        assert_eq!(find_insert(&p, c, comp).unwrap().effect, "compressor");
        assert_eq!(flatten(inserts(&p, c)).len(), 3);
        // Settings reach an insert in a branch.
        set_bypass(&mut p, c, comp, true);
        assert!(find_insert(&p, c, comp).unwrap().bypass);
        // The EQ moves into the first branch, then back after the chain.
        move_within(&mut p, c, eq, (Some((par, 0)), 0)).unwrap();
        assert_eq!(order(&p), [par]);
        assert_eq!(inserts(&p, c)[0].branches[0].inserts[0].id, eq);
        move_within(&mut p, c, eq, (None, 1)).unwrap();
        assert_eq!(order(&p), [par, eq]);
        // A chain never goes into a branch.
        assert!(move_within(&mut p, c, par, (Some((par, 0)), 0)).is_none());
        // A copy elsewhere gets new ids down its branches.
        let copy = place_insert(&mut p, (c, par), (Channel::Track(b), false, 0), true).unwrap();
        let inner = inserts(&p, Channel::Track(b))[0].branches[1].inserts[0].id;
        assert!(copy != par && inner != comp);
        // Removing the chain takes its branches' automation along.
        p.track_mut(a)
            .unwrap()
            .automation
            .push(AutomationLane::new(AutomationTarget::Param {
                insert: comp,
                param: "ratio".into(),
            }));
        remove_insert(&mut p, c, par);
        assert_eq!(order(&p), [eq]);
        assert!(p.track(a).unwrap().automation.is_empty());
        let added = add_parallel(&mut p, c, 0).unwrap();
        assert_eq!(add_branch(&mut p, c, added), Some(2));
        remove_branch(&mut p, c, added, 0);
        assert_eq!(inserts(&p, c)[0].branches.len(), 2);
    }

    #[test]
    fn inserts_move_and_copy_with_their_automation() {
        let (mut p, a, b, _) = project();
        let (ca, cb) = (Channel::Track(a), Channel::Track(b));
        let ids: Vec<InsertId> = ["eq", "compressor", "delay"]
            .iter()
            .map(|e| add_insert(&mut p, ca, e, BTreeMap::new()).unwrap())
            .collect();
        let order = |p: &Project, c| inserts(p, c).iter().map(|i| i.id).collect::<Vec<_>>();
        // Dropped after the last one: moves to the end.
        place_insert(&mut p, (ca, ids[0]), (ca, false, 3), false);
        assert_eq!(order(&p, ca), [ids[1], ids[2], ids[0]]);
        place_insert(&mut p, (ca, ids[0]), (ca, false, 0), false);
        assert_eq!(order(&p, ca), ids);
        // A copy to another track gets a new id; a move takes the automation along.
        let lane = |insert| AutomationTarget::Param {
            insert,
            param: "mix".into(),
        };
        p.track_mut(a)
            .unwrap()
            .automation
            .push(AutomationLane::new(lane(ids[2])));
        let copy = place_insert(&mut p, (ca, ids[1]), (cb, false, 0), true).unwrap();
        assert_ne!(copy, ids[1]);
        assert_eq!(order(&p, ca).len(), 3);
        place_insert(&mut p, (ca, ids[2]), (cb, false, 0), false);
        assert_eq!(order(&p, cb), [ids[2], copy]);
        assert!(p.track(a).unwrap().automation.is_empty());
        assert_eq!(p.track(b).unwrap().automation[0].target, lane(ids[2]));
        // Into the master's monitoring list.
        place_insert(&mut p, (cb, copy), (Channel::Master, true, 0), false);
        assert!(p.master.inserts[0].monitor);
    }

    #[test]
    fn removing_an_insert_drops_its_automation_and_mappings() {
        let (mut p, a, _, _) = project();
        let ca = Channel::Track(a);
        let eq = add_insert(&mut p, ca, "eq", BTreeMap::new()).unwrap();
        let target = AutomationTarget::Param {
            insert: eq,
            param: "gain".into(),
        };
        assert!(target.is_param_of(eq) && !AutomationTarget::Gain.is_param_of(eq));
        let track = p.track_mut(a).unwrap();
        track.automation.push(AutomationLane::new(target.clone()));
        track
            .automation
            .push(AutomationLane::new(AutomationTarget::Pan));
        p.midi_map.push(crate::model::MidiMapping {
            channel: 0,
            controller: 7,
            track: Some(a),
            target,
            min: 0.0,
            max: 1.0,
        });
        set_bypass(&mut p, ca, eq, true);
        set_insert_param(&mut p, ca, eq, "gain", 3.0);
        set_insert_gain(&mut p, ca, eq, 40.0);
        set_insert_file(&mut p, ca, eq, "ir", Some("room.wav".into()));
        let insert = &inserts(&p, ca)[0];
        assert!(insert.bypass);
        assert_eq!(insert.params["gain"], 3.0);
        assert_eq!(insert.gain_db, 24.0, "clamped");
        assert_eq!(insert.files.len(), 1);
        set_insert_file(&mut p, ca, eq, "ir", None);
        assert!(inserts(&p, ca)[0].files.is_empty());
        remove_insert(&mut p, ca, eq);
        assert!(inserts(&p, ca).is_empty());
        let lanes = &p.track(a).unwrap().automation;
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].target, AutomationTarget::Pan);
        assert!(p.midi_map.is_empty());
    }

    #[test]
    fn inserts_are_copied_and_reordered() {
        let (mut p, a, _, _) = project();
        let ca = Channel::Track(a);
        let mut saved = Insert {
            id: InsertId(999),
            effect: "reverb".into(),
            bypass: false,
            params: BTreeMap::new(),
            state: None,
            files: BTreeMap::new(),
            sidechain: Some(a),
            monitor: true,
            gain_db: 0.0,
            branches: Vec::new(),
            macros: Vec::new(),
        };
        let copy = add_insert_copy(&mut p, ca, &saved).unwrap();
        let first = &inserts(&p, ca)[0];
        assert_ne!(copy, saved.id);
        assert!(!first.monitor && first.sidechain.is_none());
        saved.effect = "delay".into();
        let second = add_insert_copy(&mut p, Channel::Master, &saved).unwrap();
        assert!(inserts(&p, Channel::Master)[0].monitor);
        let vca = add_vca(&mut p, "vca");
        assert_eq!(add_insert_copy(&mut p, Channel::Track(vca), &saved), None);
        add_insert_copy(&mut p, ca, &saved).unwrap();
        let effects = |p: &Project| -> Vec<String> {
            inserts(p, ca).iter().map(|i| i.effect.clone()).collect()
        };
        move_insert(&mut p, ca, copy, true);
        assert_eq!(effects(&p), ["reverb", "delay"], "already first");
        move_insert(&mut p, ca, copy, false);
        assert_eq!(effects(&p), ["delay", "reverb"]);
        move_insert(&mut p, ca, copy, false);
        assert_eq!(effects(&p), ["delay", "reverb"], "already last");
        assert_ne!(second, copy);
    }

    #[test]
    fn the_folder_feed_refuses_loops_and_siblings_swap() {
        let (mut p, a, b, bus) = project();
        set_parent(&mut p, a, Some(bus)).unwrap();
        set_master_send(&mut p, a, false).unwrap();
        add_send(&mut p, bus, a).unwrap();
        assert_eq!(set_master_send(&mut p, a, true), Err(RoutingError::Cycle));
        assert_eq!(
            set_master_send(&mut p, TrackId(999), true),
            Err(RoutingError::UnknownTrack)
        );
        let order = |p: &Project| p.tracks.iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(order(&p), [b, bus, a]);
        move_track(&mut p, bus, true);
        assert_eq!(order(&p), [bus, a, b], "the folder moves with its contents");
        move_track(&mut p, bus, true);
        assert_eq!(order(&p), [bus, a, b], "already first");
        move_track(&mut p, b, false);
        assert_eq!(order(&p), [bus, a, b], "already last");
    }

    fn project() -> (Project, TrackId, TrackId, TrackId) {
        let mut p = Project::default();
        let a = p.add_track("a");
        let b = p.add_track("b");
        let bus = p.add_track("bus");
        (p, a, b, bus)
    }

    #[test]
    fn sends_refuse_loops() {
        let (mut p, a, b, bus) = project();
        add_send(&mut p, a, bus).unwrap();
        add_send(&mut p, b, bus).unwrap();
        assert_eq!(add_send(&mut p, a, bus), Err(RoutingError::Duplicate));
        assert_eq!(add_send(&mut p, bus, a), Err(RoutingError::Cycle));
        assert_eq!(add_send(&mut p, a, a), Err(RoutingError::Cycle));
        assert!(reaches(&p, a, bus));
        assert!(!reaches(&p, bus, a));
        remove_send(&mut p, a, bus);
        add_send(&mut p, bus, a).unwrap();
    }

    #[test]
    fn folders_keep_their_contents_after_them() {
        let (mut p, a, b, bus) = project();
        set_parent(&mut p, a, Some(bus)).unwrap();
        let order: Vec<TrackId> = p.tracks.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![b, bus, a]);
        assert_eq!(depth(&p, a), 1);
        assert!(has_children(&p, bus));
        assert_eq!(destinations(&p, a), vec![bus]);
        assert_eq!(set_parent(&mut p, bus, Some(a)), Err(RoutingError::Cycle));
        add_send(&mut p, b, a).unwrap();
        assert_eq!(set_parent(&mut p, b, Some(a)), Ok(()));
        p.track_mut(bus).unwrap().collapsed = true;
        assert_eq!(visible_tracks(&p), vec![(0, 0)]);
        move_track(&mut p, bus, true);
        set_parent(&mut p, a, None).unwrap();
        assert_eq!(depth(&p, b), 1);
    }

    #[test]
    fn removing_a_track_cleans_references() {
        let (mut p, a, b, bus) = project();
        set_parent(&mut p, a, Some(bus)).unwrap();
        add_send(&mut p, b, bus).unwrap();
        let vca = add_vca(&mut p, "vca");
        set_vca(&mut p, b, Some(vca)).unwrap();
        assert_eq!(set_vca(&mut p, b, Some(a)), Err(RoutingError::NotAudio));
        p.remove_track(bus);
        p.remove_track(vca);
        assert_eq!(p.track(a).unwrap().parent, None);
        assert!(p.track(b).unwrap().sends.is_empty());
        assert_eq!(p.track(b).unwrap().vca, None);
    }

    #[test]
    fn vca_and_solo() {
        let (mut p, a, b, bus) = project();
        let vca = add_vca(&mut p, "vca");
        set_vca(&mut p, a, Some(vca)).unwrap();
        p.track_mut(vca).unwrap().gain_db = -6.0;
        p.track_mut(a).unwrap().gain_db = -3.0;
        assert_eq!(effective_gain(&p, a), (-9.0, false));
        assert!(solo_audible(&p).is_none());
        add_send(&mut p, b, bus).unwrap();
        p.track_mut(b).unwrap().solo = true;
        let audible = solo_audible(&p).unwrap();
        assert!(audible.contains(&b) && audible.contains(&bus) && !audible.contains(&a));
        p.track_mut(b).unwrap().solo = false;
        p.track_mut(vca).unwrap().solo = true;
        assert!(solo_audible(&p).unwrap().contains(&a));
        set_parent(&mut p, b, Some(bus)).unwrap();
        p.track_mut(vca).unwrap().solo = false;
        p.track_mut(bus).unwrap().solo = true;
        assert!(
            solo_audible(&p).unwrap().contains(&b),
            "folder solo includes children"
        );
    }

    #[test]
    fn insert_chain_edits() {
        let (mut p, a, ..) = project();
        let x = add_insert(&mut p, Channel::Track(a), "utility", BTreeMap::new()).unwrap();
        let y = add_insert(&mut p, Channel::Master, "utility", BTreeMap::new()).unwrap();
        let z = add_insert(&mut p, Channel::Track(a), "utility", BTreeMap::new()).unwrap();
        move_insert(&mut p, Channel::Track(a), z, true);
        assert_eq!(inserts(&p, Channel::Track(a))[0].id, z);
        set_insert_param(&mut p, Channel::Track(a), x, "gain", -3.0);
        set_bypass(&mut p, Channel::Track(a), x, true);
        let insert = &inserts(&p, Channel::Track(a))[1];
        assert!(insert.bypass && insert.params["gain"] == -3.0);
        remove_insert(&mut p, Channel::Master, y);
        assert!(inserts(&p, Channel::Master).is_empty());
        let vca = add_vca(&mut p, "vca");
        assert!(add_insert(&mut p, Channel::Track(vca), "utility", BTreeMap::new()).is_none());
    }

    #[test]
    fn tracks_move_before_or_after_another_with_their_contents() {
        let mut p = Project::new("t", 48_000);
        let a = p.add_track("a");
        let folder = p.add_track("folder");
        let child = p.add_track("child");
        let b = p.add_track("b");
        set_parent(&mut p, child, Some(folder)).unwrap();
        let names = |p: &Project| p.tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        move_tracks(&mut p, &[b], a, false);
        assert_eq!(names(&p), ["b", "a", "folder", "child"]);
        move_tracks(&mut p, &[folder], b, false);
        assert_eq!(names(&p), ["folder", "child", "b", "a"]);
        move_tracks(&mut p, &[a], child, true);
        assert_eq!(names(&p), ["folder", "child", "a", "b"]);
        assert_eq!(p.track(a).unwrap().parent, Some(folder), "joins the folder");
        move_tracks(&mut p, &[a, b], folder, false);
        assert_eq!(names(&p), ["a", "b", "folder", "child"]);
        assert_eq!(p.track(a).unwrap().parent, None);
        move_tracks(&mut p, &[folder], child, false);
        assert_eq!(names(&p), ["a", "b", "folder", "child"], "not into itself");
    }

    #[test]
    fn tracks_enter_a_folder_in_their_order() {
        let mut p = Project::new("t", 48_000);
        let a = p.add_track("a");
        let b = p.add_track("b");
        let c = p.add_track("c");
        let bus = p.add_track("bus");
        let names = |p: &Project| p.tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
        // In the order they were selected, last to first.
        set_parents(&mut p, &[c, a, b], Some(bus)).unwrap();
        assert_eq!(names(&p), ["bus", "a", "b", "c"]);
        let d = p.add_track("d");
        set_parents(&mut p, &[d, b], Some(bus)).unwrap();
        assert_eq!(names(&p), ["bus", "a", "b", "c", "d"], "after the contents");
        set_parents(&mut p, &[a, c], None).unwrap();
        assert_eq!(p.track(a).unwrap().parent, None);
        assert_eq!(p.track(b).unwrap().parent, Some(bus));
    }
}
