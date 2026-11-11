//! Right-click menus of the timeline, in the spirit of Reaper: clips, empty lanes, the ruler
//! (markers, loop, punch) and track headers.

use std::collections::BTreeMap;
use std::path::Path;

use audiostrada_core::{
    Clip, ClipId, MarkerId, Project, ProjectPaths, Source, SourceId, TrackId, TrackKind,
};
use audiostrada_core::{edit, section};
use eframe::egui::{self, RichText, Ui};

use super::TimelineView;
use crate::actions::Action;
use crate::document::Document;
use crate::i18n::tr;
use crate::transport::EditSettings;
use crate::trf;
use audiostrada_core::routing;

/// Copied clips, ready to paste into this project or one in another tab.
#[derive(Clone, Debug, Default)]
pub(crate) struct Clipboard {
    /// Track offset from the first copied track, and the clip with its start relative to the
    /// earliest one.
    clips: Vec<(usize, Clip)>,
    /// Audio files of the clips, with absolute paths.
    sources: BTreeMap<SourceId, Source>,
    /// Sample rate of the project they come from.
    rate: u32,
    /// How long the copy lasts: the range, or from the first clip's start to the last's end.
    span: u64,
}

/// The source of `project` reading the same file as `source`, added when there is none.
fn own_source(project: &mut Project, paths: Option<&ProjectPaths>, source: &Source) -> SourceId {
    let resolve = |path: &Path| paths.map_or_else(|| path.to_path_buf(), |p| p.resolve(path));
    let found = project
        .sources
        .iter()
        .find(|(_, s)| resolve(&s.path) == source.path)
        .map(|(id, _)| *id);
    found.unwrap_or_else(|| {
        project.add_source(Source {
            path: paths.map_or_else(|| source.path.clone(), |p| p.relativize(&source.path)),
            ..source.clone()
        })
    })
}

/// Moves the times of `clip` from a project at `from` Hz to one at `to` Hz.
fn retime(clip: &mut Clip, from: u32, to: u32) {
    let at = |t: u64| (t as f64 * f64::from(to) / f64::from(from)).round() as u64;
    clip.start = at(clip.start);
    clip.offset = at(clip.offset);
    clip.length = at(clip.length).max(1);
    clip.fade_in = at(clip.fade_in);
    clip.fade_out = at(clip.fade_out);
    if let Some(notes) = &mut clip.notes {
        for n in std::sync::Arc::make_mut(notes) {
            n.start = at(n.start);
            n.length = at(n.length).max(1);
        }
    }
    if let Some(controls) = &mut clip.controls {
        for c in std::sync::Arc::make_mut(controls) {
            c.time = at(c.time);
        }
    }
}

/// What was under the pointer at the last right click.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Menu {
    Clip {
        clip: ClipId,
        at: u64,
    },
    Lane {
        track: Option<TrackId>,
        at: u64,
    },
    /// Inside the time range.
    Range,
    /// On a clip's fade (in, or out).
    Fade {
        clip: ClipId,
        fade_in: bool,
    },
    /// On the crossfade between two clips.
    Crossfade {
        left: ClipId,
        right: ClipId,
    },
}

/// Track colors offered in the header menu.
pub const TRACK_COLORS: [[u8; 3]; 16] = [
    [224, 82, 82],
    [232, 128, 64],
    [230, 176, 60],
    [196, 204, 70],
    [120, 196, 84],
    [64, 176, 112],
    [52, 182, 168],
    [62, 164, 214],
    [80, 124, 226],
    [120, 100, 220],
    [160, 92, 210],
    [204, 86, 180],
    [222, 92, 132],
    [150, 120, 96],
    [128, 140, 150],
    [96, 104, 120],
];

pub(super) fn ms(project: &Project, ms: f64) -> u64 {
    (ms * f64::from(project.sample_rate) / 1000.0) as u64
}

impl TimelineView {
    pub(super) fn range_menu(&mut self, ui: &mut Ui, doc: &mut Document, settings: &EditSettings) {
        let Some(r) = self.range.clone() else {
            ui.close();
            return;
        };
        let rate = f64::from(doc.project().sample_rate.max(1));
        ui.label(
            RichText::new(trf!(
                "{seconds} s on {count} track(s)",
                seconds = format!("{:.2}", (r.end - r.start) as f64 / rate),
                count = r.tracks.len()
            ))
            .weak(),
        );
        if ui.button(Action::Delete.label()).clicked() {
            doc.history_mut().edit(tr("Delete range"), |p| {
                edit::delete_range(p, &r.tracks, r.start, r.end, false)
            });
            ui.close();
        }
        if ui.button(tr("Delete and close the gap")).clicked() {
            doc.history_mut().edit(tr("Delete range"), |p| {
                edit::delete_range(p, &r.tracks, r.start, r.end, true)
            });
            ui.close();
        }
        if r.tracks.len() < doc.project().tracks.len()
            && ui.button(Action::RangeAllTracks.label()).clicked()
        {
            self.set_range(doc, Action::RangeAllTracks);
            ui.close();
        }
        if ui.button(Action::Fades.label()).clicked() {
            self.fades(doc);
            ui.close();
        }
        if ui.button(tr("Split at the edges")).clicked() {
            doc.history_mut().edit(tr("Split range"), |p| {
                edit::split_range(p, &r.tracks, r.start, r.end);
            });
            ui.close();
        }
        ui.separator();
        if ui.button(Action::Heal.label()).clicked() {
            self.heal(doc);
            ui.close();
        }
        if ui.button(tr("Consolidate")).clicked() {
            self.consolidate(doc);
            ui.close();
        }
        if ui.button(Action::StripSilence.label()).clicked() {
            self.open_strip(doc);
            ui.close();
        }
        if ui.button(tr("Process…")).clicked() {
            self.open_process(doc);
            ui.close();
        }
        if ui.button(Action::BeatDetective.label()).clicked() {
            self.open_beat(doc);
            ui.close();
        }
        if ui.button(Action::Groove.label()).clicked() {
            self.open_groove(doc);
            ui.close();
        }
        if ui.button(Action::Copy.label()).clicked() {
            self.copy(doc);
            ui.close();
        }
        if ui.button(Action::Cut.label()).clicked() {
            self.cut(doc, settings);
            ui.close();
        }
        if ui.button(tr("Loop this range")).clicked() {
            doc.history_mut().edit(tr("Loop range"), |p| {
                p.loop_range = Some((r.start, r.end));
                p.loop_off = false;
            });
            ui.close();
        }
        if ui
            .button(tr("Insert silence here"))
            .on_hover_text(tr(
                "Push everything after the range start by the range length, on all tracks",
            ))
            .clicked()
        {
            doc.history_mut().edit(tr("Insert silence"), |p| {
                edit::insert_silence(p, r.start, r.end - r.start)
            });
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new(tr("Whole project, every track")).weak());
        for (action, hint) in [
            (
                Action::CropToSelection,
                "Keep only this passage, which then starts the project",
            ),
            (
                Action::RemoveSection,
                "Take this passage out: what follows moves back, markers, automation and \
                 tempo with it",
            ),
            (
                Action::ClearSection,
                "Empty this passage on every track: the time stays, silent",
            ),
        ] {
            if ui.button(action.label()).on_hover_text(tr(hint)).clicked() {
                self.edit_section(doc, action);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tr("Clear the range")).clicked() {
            self.range = None;
            ui.close();
        }
    }

    /// Crops the project to the range (else the loop), or removes or clears that passage on
    /// every track.
    pub(super) fn edit_section(&mut self, doc: &mut Document, action: Action) {
        let span = self
            .range
            .as_ref()
            .map(|r| (r.start, r.end))
            .or(doc.project().loop_range)
            .filter(|(a, b)| b > a);
        let Some((start, end)) = span else {
            doc.notify(
                tr("Select a passage first: Shift+drag across the tracks, or set a loop").into(),
            );
            return;
        };
        doc.history_mut().edit(action.label(), |p| match action {
            Action::CropToSelection => section::crop(p, start, end),
            Action::RemoveSection => section::remove(p, start, end),
            _ => section::clear(p, start, end),
        });
        self.selection.clear();
        match action {
            Action::CropToSelection => {
                self.range = None;
                doc.player_mut().locate(0);
            }
            Action::RemoveSection => {
                self.range = None;
                doc.player_mut().locate(start);
            }
            _ => {}
        }
    }

    /// Copies the range, or else the selected clips.
    /// Copies the range (its clips cut at its edges, with the empty time it covers), or else
    /// the selected clips. Tracks are counted in the order shown, from the first copied one.
    pub(super) fn copy(&mut self, doc: &Document) {
        let project = doc.project();
        let shown: Vec<TrackId> = routing::visible_tracks(project)
            .iter()
            .map(|&(i, _)| project.tracks[i].id)
            .collect();
        let row = |id: TrackId| shown.iter().position(|t| *t == id);
        let mut copied: Vec<(usize, Clip)> = Vec::new();
        let span;
        if let Some(r) = &self.range {
            let mut scratch = project.clone();
            let inside = edit::split_range(&mut scratch, &r.tracks, r.start, r.end);
            for t in &scratch.tracks {
                for c in t.clips.iter().filter(|c| inside.contains(&c.id)) {
                    if let Some(i) = row(t.id) {
                        copied.push((i, c.clone()));
                    }
                }
            }
            // The first track is the range's own, even when it has no clip at the start.
            let first = r.tracks.iter().filter_map(|&t| row(t)).min().unwrap_or(0);
            for (i, c) in &mut copied {
                *i -= first.min(*i);
                c.start -= r.start;
            }
            span = r.end - r.start;
        } else {
            for t in &project.tracks {
                for c in t.clips.iter().filter(|c| self.selection.contains(&c.id)) {
                    if let Some(i) = row(t.id) {
                        copied.push((i, c.clone()));
                    }
                }
            }
            let first = copied.iter().map(|c| c.0).min().unwrap_or(0);
            let start = copied.iter().map(|c| c.1.start).min().unwrap_or(0);
            let end = copied.iter().map(|c| c.1.end()).max().unwrap_or(0);
            for (i, c) in &mut copied {
                *i -= first;
                c.start -= start;
            }
            span = end.saturating_sub(start);
        }
        if !copied.is_empty() {
            let sources = copied
                .iter()
                .filter_map(|(_, c)| {
                    let s = project.sources.get(&c.source)?;
                    let path = doc.resolve(&s.path);
                    Some((c.source, Source { path, ..s.clone() }))
                })
                .collect();
            self.clipboard = Clipboard {
                clips: copied,
                sources,
                rate: project.sample_rate,
                span,
            };
        }
    }

    pub(super) fn cut(&mut self, doc: &mut Document, settings: &EditSettings) {
        self.copy(doc);
        let shuffle = settings.mode == crate::transport::EditMode::Shuffle;
        if let Some(r) = self.range.take() {
            doc.history_mut().edit(Action::Cut.label(), |p| {
                edit::delete_range(p, &r.tracks, r.start, r.end, shuffle)
            });
        } else if !self.selection.is_empty() {
            let ids = std::mem::take(&mut self.selection);
            doc.history_mut().edit(Action::Cut.label(), |p| {
                edit::delete_clips(p, &ids, shuffle)
            });
        }
    }

    /// Pastes at the playhead, the first copied track on the selected one (else the first
    /// shown), then moves the playhead to the end of what was pasted, so pasting again follows.
    pub(super) fn paste(&mut self, doc: &mut Document) {
        let at = doc.player().position();
        let project = doc.project();
        let base = doc
            .focused_track()
            .and_then(|id| {
                routing::visible_tracks(project)
                    .iter()
                    .position(|&(i, _)| project.tracks[i].id == id)
            })
            .unwrap_or(0);
        if let Some(end) = self.paste_at(doc, at, base, tr("Paste")) {
            doc.player_mut().locate(end);
        }
    }

    /// Pastes the clipboard at `at`, its first track on shown row `base`; selects what was
    /// pasted and gives where it ends.
    pub(super) fn paste_at(
        &mut self,
        doc: &mut Document,
        at: u64,
        base: usize,
        label: &str,
    ) -> Option<u64> {
        if self.clipboard.clips.is_empty() {
            return None;
        }
        let paths = doc.paths().cloned();
        let project = doc.project();
        let rows = routing::visible_tracks(project);
        let targets: Vec<Option<TrackId>> = self
            .clipboard
            .clips
            .iter()
            .map(|(off, _)| {
                let &(index, _) = rows.get(base + off)?;
                let t = &project.tracks[index];
                (t.kind == TrackKind::Audio).then_some(t.id)
            })
            .collect();
        let board = self.clipboard.clone();
        let mut pasted = Vec::new();
        doc.history_mut().edit(label, |p| {
            let mut tracks = Vec::new();
            // Clips from another project bring their files along.
            let mut sources = BTreeMap::new();
            for ((_, clip), target) in board.clips.iter().zip(&targets) {
                let Some(target) = *target else {
                    continue;
                };
                let mut c = clip.clone();
                if board.rate != p.sample_rate && board.rate > 0 {
                    retime(&mut c, board.rate, p.sample_rate);
                }
                if let Some(source) = board.sources.get(&clip.source) {
                    c.source = *sources
                        .entry(clip.source)
                        .or_insert_with(|| own_source(p, paths.as_ref(), source));
                }
                c.id = p.new_clip_id();
                c.start = (at + c.start).min(audiostrada_core::model::MAX_POSITION);
                pasted.push(c.id);
                if let Some(t) = p.track_mut(target) {
                    t.clips.push(c);
                    t.clips.sort_by_key(|c| (c.start, c.id));
                }
                if !tracks.contains(&target) {
                    tracks.push(target);
                }
            }
            for t in tracks {
                edit::resolve_overlaps(p, t, &pasted);
            }
        });
        self.selection = pasted;
        self.range = None;
        let span = if board.rate != doc.project().sample_rate && board.rate > 0 {
            (board.span as f64 * f64::from(doc.project().sample_rate) / f64::from(board.rate))
                as u64
        } else {
            board.span
        };
        Some(at + span)
    }

    /// Clips a clip menu acts on: the selection when it holds the clicked clip, else the clip
    /// with those edited along with it.
    pub(super) fn menu_targets(&mut self, project: &Project, clip: ClipId) -> Vec<ClipId> {
        if !self.selection.contains(&clip) {
            self.selection = audiostrada_core::groups::edit_clips(project, &[clip]);
        }
        self.selection.clone()
    }

    pub(super) fn clip_menu(&mut self, ui: &mut Ui, doc: &mut Document, clip: ClipId, at: u64) {
        let Some(c) = edit::clip(doc.project(), clip).cloned() else {
            ui.close();
            return;
        };
        let ids = self.menu_targets(doc.project(), clip);
        let many = ids.len() > 1;
        ui.label(
            RichText::new(if many {
                trf!("{count} clips", count = ids.len())
            } else {
                c.name.clone()
            })
            .weak(),
        );
        let mut name = c.name.clone();
        if !many
            && ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(160.0))
                .changed()
        {
            doc.history_mut().edit_merged(
                tr("Rename clip"),
                &format!("clip-name:{}", clip.0),
                |p| {
                    if let Some(m) = edit::clip_mut(p, clip) {
                        m.name = name;
                    }
                },
            );
        }
        ui.separator();
        if !c.is_midi() {
            if ui
                .button(tr("Add effects to the clip…"))
                .on_hover_text(tr(
                    "Effects of this clip alone, between it and its track, as in the rack",
                ))
                .clicked()
            {
                self.clip_effects = Some((clip, true));
                ui.close();
            }
            if !c.inserts.is_empty()
                && ui
                    .button(trf!("Clip effects ({count})…", count = c.inserts.len()))
                    .clicked()
            {
                self.clip_effects = Some((clip, false));
                ui.close();
            }
            let any = ids
                .iter()
                .any(|id| edit::clip(doc.project(), *id).is_some_and(|c| !c.inserts.is_empty()));
            if !c.inserts.is_empty() {
                ui.menu_button(tr("Automate the clip effects"), |ui| {
                    clip_automation_menu(ui, doc, &c)
                });
            }
            if any && ui.button(tr("Remove the clip effects")).clicked() {
                doc.history_mut().edit(tr("Remove clip effects"), |p| {
                    for id in &ids {
                        if let Some(c) = edit::clip_mut(p, *id) {
                            c.inserts.clear();
                        }
                    }
                });
                ui.close();
            }
            ui.separator();
        }
        if ui.button(tr("Split here")).clicked() {
            let mut created = Vec::new();
            doc.history_mut()
                .edit(tr("Split clips"), |p| created = edit::split(p, at, &ids));
            self.selection.extend(created);
            ui.close();
        }
        if ui.button(Action::Duplicate.label()).clicked() {
            let mut created = Vec::new();
            doc.history_mut().edit(Action::Duplicate.label(), |p| {
                created = edit::duplicate(p, &ids)
            });
            self.selection = created;
            ui.close();
        }
        if ui.button(Action::Delete.label()).clicked() {
            doc.history_mut()
                .edit(tr("Delete clips"), |p| edit::delete_clips(p, &ids, false));
            self.selection.clear();
            ui.close();
        }
        if ui
            .button(Action::RangeFromClips.label())
            .on_hover_text(tr("Select the time these clips cover, on their tracks"))
            .clicked()
        {
            self.set_range(doc, Action::RangeFromClips);
            ui.close();
        }
        ui.separator();
        if many && ui.button(Action::Heal.label()).clicked() {
            self.heal(doc);
            ui.close();
        }
        if ui.button(tr("Consolidate")).clicked() {
            self.consolidate(doc);
            ui.close();
        }
        if ui.button(Action::StripSilence.label()).clicked() {
            self.open_strip(doc);
            ui.close();
        }
        if ui
            .button(tr("Process…"))
            .on_hover_text(tr("Normalize, reverse, or render through an effect"))
            .clicked()
        {
            self.open_process(doc);
            ui.close();
        }
        if ui
            .button(Action::BeatDetective.label())
            .on_hover_text(tr(
                "Find the hits of the selected clips, cut or conform them to the grid",
            ))
            .clicked()
        {
            self.open_beat(doc);
            ui.close();
        }
        if ui
            .button(Action::Groove.label())
            .on_hover_text(tr(
                "Move the hits of the selected tracks onto those of the drums (or any track)",
            ))
            .clicked()
        {
            self.open_groove(doc);
            ui.close();
        }
        ui.separator();
        for fade_in in [true, false] {
            let label = if fade_in {
                tr("Fade in")
            } else {
                tr("Fade out")
            };
            ui.menu_button(label, |ui| self.fade_options(ui, doc, &c, &ids, fade_in));
        }
        if !c.is_midi() {
            ui.menu_button(tr("Gain"), |ui| {
                let mut change = |ui: &mut Ui, text: &str, f: &dyn Fn(f32) -> f32| {
                    if ui.button(text).clicked() {
                        doc.history_mut().edit(tr("Clip gain"), |p| {
                            for id in &ids {
                                if let Some(gain) = edit::clip(p, *id).map(|m| m.gain_db) {
                                    edit::set_clip_gain(p, *id, f(gain));
                                }
                            }
                        });
                        ui.close();
                    }
                };
                change(ui, "+1 dB", &|g| g + 1.0);
                change(ui, "-1 dB", &|g| g - 1.0);
                change(ui, tr("Reset to 0 dB"), &|_| 0.0);
                ui.separator();
                for target in [0.0, -1.0, -6.0] {
                    if ui
                        .button(trf!("Normalize to {target} dBFS", target = target))
                        .clicked()
                    {
                        doc.normalize_clips(&ids, target);
                        ui.close();
                    }
                }
            });
        }
        if !c.is_midi() {
            ui.menu_button(tr("Stretch"), |ui| {
                for (text, factor) in [
                    ("50 %", 0.5),
                    ("75 %", 0.75),
                    (tr("Original speed"), 0.0),
                    ("150 %", 1.5),
                    ("200 %", 2.0),
                ] {
                    if ui.button(text).clicked() {
                        doc.history_mut().edit(tr("Stretch clip"), |p| {
                            for id in &ids {
                                if let Some(m) = edit::clip(p, *id) {
                                    // Lengths relative to the unstretched clip.
                                    let base = m.length as f64 / m.stretch;
                                    let target = if factor == 0.0 { 1.0 } else { factor };
                                    edit::stretch_clip(p, *id, (base * target).round() as u64);
                                }
                            }
                        });
                        ui.close();
                    }
                }
                ui.separator();
                let bpm = doc.project().tempo.bpm_at_beat(0.0);
                match doc.clip_tempo(clip, ui.ctx()) {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.weak(tr("Detecting the tempo…"));
                        });
                    }
                    Some(Some(found)) => {
                        if ui
                            .button(trf!(
                                "Match project tempo ({found} → {bpm} BPM)",
                                found = format!("{found:.1}"),
                                bpm = format!("{bpm:.1}")
                            ))
                            .on_hover_text(tr(
                                "Stretch so the clip's detected tempo becomes the project tempo",
                            ))
                            .clicked()
                        {
                            doc.history_mut().edit(tr("Match tempo"), |p| {
                                for id in &ids {
                                    if let Some(m) = edit::clip(p, *id) {
                                        let base = m.length as f64 / m.stretch;
                                        let length = base * found / bpm;
                                        edit::stretch_clip(p, *id, length.round() as u64);
                                    }
                                }
                            });
                            ui.close();
                        }
                    }
                    Some(None) => {
                        ui.weak(tr("No clear tempo detected"));
                    }
                }
                ui.weak(tr("Or Alt-drag the right edge"));
            });
            ui.menu_button(tr("Pitch"), |ui| {
                for (text, delta) in [
                    ("+12 st", 12.0),
                    ("+1 st", 1.0),
                    (tr("Original pitch"), 0.0),
                    ("-1 st", -1.0),
                    ("-12 st", -12.0),
                ] {
                    if ui.button(text).clicked() {
                        let semitones = if delta == 0.0 { 0.0 } else { c.pitch + delta };
                        doc.history_mut().edit(tr("Transpose clip"), |p| {
                            edit::set_pitch(p, &ids, semitones)
                        });
                        ui.close();
                    }
                }
            });
        }
        let muted = c.muted;
        if ui
            .button(if muted { tr("Unmute") } else { tr("Mute") })
            .clicked()
        {
            doc.history_mut().edit(tr("Mute clips"), |p| {
                for id in &ids {
                    if let Some(m) = edit::clip_mut(p, *id) {
                        m.muted = !muted;
                    }
                }
            });
            ui.close();
        }
        if ui.button(tr("Loop these clips")).clicked() {
            let start = ids
                .iter()
                .filter_map(|id| edit::clip(doc.project(), *id))
                .map(|m| m.start)
                .min();
            let end = ids
                .iter()
                .filter_map(|id| edit::clip(doc.project(), *id))
                .map(|m| m.end())
                .max();
            if let (Some(start), Some(end)) = (start, end) {
                doc.history_mut().edit(tr("Loop selection"), |p| {
                    p.loop_range = Some((start, end));
                    p.loop_off = false;
                });
            }
            ui.close();
        }
        if c.is_midi() {
            ui.separator();
            if ui.button(tr("Open in piano roll")).clicked() {
                self.requests.push(Action::ShowPianoRoll);
                ui.close();
            }
            if ui.button(Action::Quantize.label()).clicked() {
                self.requests.push(Action::Quantize);
                ui.close();
            }
        }
    }

    pub(super) fn lane_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        track: Option<TrackId>,
        at: u64,
    ) {
        if ui.button(tr("Add marker here")).clicked() {
            doc.history_mut().edit(tr("Add marker"), |p| {
                edit::add_marker(p, at);
            });
            ui.close();
        }
        if let Some(track) = track {
            if doc.is_instrument_track(track) && ui.button(tr("New MIDI clip here")).clicked() {
                self.pending_midi = Some((track, at));
                ui.close();
            }
            if ui.button(tr("Select all clips of the track")).clicked() {
                self.selection = doc
                    .project()
                    .track(track)
                    .map(|t| t.clips.iter().map(|c| c.id).collect())
                    .unwrap_or_default();
                ui.close();
            }
        }
        ui.separator();
        for action in [
            Action::AddTrack,
            Action::AddStereoTrack,
            Action::AddTracks,
            Action::AddInstrumentTrack,
            Action::AddDrumTrack,
        ] {
            if ui.button(action.label()).clicked() {
                self.requests.push(action);
                ui.close();
            }
        }
    }

    pub(super) fn ruler_menu(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        at: u64,
        marker: Option<MarkerId>,
    ) {
        let project = doc.project().clone();
        if let Some(id) = marker
            && let Some(m) = project.markers.iter().find(|m| m.id == id)
        {
            let mut name = m.name.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(140.0))
                .changed()
            {
                doc.history_mut().edit_merged(
                    tr("Rename marker"),
                    &format!("marker:{}", id.0),
                    |p| {
                        if let Some(m) = p.markers.iter_mut().find(|m| m.id == id) {
                            m.name = name;
                        }
                    },
                );
            }
            if ui.button(tr("Delete marker")).clicked() {
                doc.history_mut()
                    .edit(tr("Delete marker"), |p| edit::remove_marker(p, id));
                ui.close();
            }
            ui.separator();
        }
        if ui.button(tr("Add marker here")).clicked() {
            doc.history_mut().edit(tr("Add marker"), |p| {
                edit::add_marker(p, at);
            });
            ui.close();
        }
        if ui.button(tr("Go here")).clicked() {
            doc.player_mut().locate(at);
            ui.close();
        }
        if (self.range.is_some() || !self.selection.is_empty())
            && ui
                .button(tr("Remove the selection"))
                .on_hover_text(tr("Clears the time range and the selected clips (Escape)"))
                .clicked()
        {
            self.range = None;
            self.selection.clear();
            ui.close();
        }
        ui.separator();
        let length = project.length().max(at + 1);
        let mut range = |ui: &mut Ui, punch: bool| {
            let current = if punch {
                project.punch_range
            } else {
                project.loop_range
            };
            let pick = |punch_text, loop_text| if punch { punch_text } else { loop_text };
            let set = |doc: &mut Document, range: Option<(u64, u64)>| {
                let label = pick(tr("Set punch range"), tr("Set loop"));
                doc.history_mut().edit(label, |p| {
                    if punch {
                        p.punch_range = range;
                    } else {
                        p.loop_range = range;
                        p.loop_off = false;
                    }
                });
            };
            if ui
                .button(pick(tr("Start punch here"), tr("Start loop here")))
                .clicked()
            {
                let end = current.map_or(length, |r| r.1).max(at + 1);
                set(doc, Some((at, end)));
                ui.close();
            }
            if ui
                .button(pick(tr("End punch here"), tr("End loop here")))
                .clicked()
            {
                let start = current.map_or(0, |r| r.0).min(at.saturating_sub(1));
                set(doc, Some((start, at.max(1))));
                ui.close();
            }
            if current.is_some()
                && ui
                    .button(pick(tr("Remove punch"), tr("Remove loop")))
                    .clicked()
            {
                set(doc, None);
                ui.close();
            }
        };
        range(ui, false);
        ui.separator();
        range(ui, true);
    }

    pub(super) fn header_menu(&mut self, ui: &mut Ui, doc: &mut Document, id: TrackId) {
        for request in crate::track_menu::show(ui, doc, id) {
            match request {
                crate::track_menu::Request::Action(a) => self.requests.push(a),
                crate::track_menu::Request::Export(t) => self.stem_request = Some(t),
            }
        }
    }
}

/// A parameter that can be automated: its target, its name and the value set.
type Automatable = (audiostrada_core::automation::AutomationTarget, String, f32);

/// The parameters of a clip's effects, each drawn over the clip as a curve while ticked; a new
/// curve starts flat at the value set, across the clip.
fn clip_automation_menu(ui: &mut Ui, doc: &mut Document, clip: &Clip) {
    use audiostrada_core::automation::{AutomationLane, AutomationTarget, CurveShape};
    let channel = routing::Channel::Clip(clip.id);
    let mut effects: Vec<(String, Vec<Automatable>)> = Vec::new();
    for insert in routing::flatten(&clip.inserts)
        .into_iter()
        .filter(|i| i.effect != routing::PARALLEL)
    {
        let target = |param: String| AutomationTarget::Param {
            insert: insert.id,
            param,
        };
        let (name, params): (String, Vec<Automatable>) = match audiostrada_dsp::info(&insert.effect)
        {
            Some(info) => (
                tr(info.name).to_owned(),
                info.params
                    .iter()
                    .map(|p| {
                        let value = insert.params.get(p.id).copied().unwrap_or(p.default);
                        (target(p.id.to_owned()), tr(p.name).to_owned(), value)
                    })
                    .collect(),
            ),
            None => {
                let name = doc
                    .player()
                    .plugins()
                    .find(&insert.effect)
                    .map_or_else(|| insert.effect.clone(), |p| p.name.clone());
                let params = doc
                    .player_mut()
                    .plugin(channel, insert.id)
                    .map(|plugin| {
                        plugin
                            .params()
                            .into_iter()
                            .filter(|p| !p.hidden && !p.read_only)
                            .map(|p| {
                                let value = plugin.param_value(p.id).unwrap_or(p.default) as f32;
                                (target(p.id.to_string()), p.name, value)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (name, params)
            }
        };
        effects.push((name, params));
    }
    let mut changed = None;
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for (effect, params) in &effects {
                ui.label(RichText::new(effect).weak());
                for (target, name, value) in params {
                    let mut shown = clip
                        .automation
                        .iter()
                        .any(|l| &l.target == target && l.visible);
                    if ui.checkbox(&mut shown, name.as_str()).changed() {
                        changed = Some((target.clone(), *value, shown));
                    }
                }
            }
        });
    let Some((target, value, shown)) = changed else {
        return;
    };
    let (id, from, to) = (clip.id, clip.offset, clip.offset + clip.length);
    doc.history_mut().edit(tr("Clip automation"), |p| {
        let Some(c) = edit::clip_mut(p, id) else {
            return;
        };
        match c.automation.iter().position(|l| l.target == target) {
            Some(i) => c.automation[i].visible = shown,
            None if shown => {
                let mut lane = AutomationLane::new(target);
                lane.add(from, value, CurveShape::Linear);
                lane.add(to, value, CurveShape::Linear);
                c.automation.push(lane);
            }
            None => {}
        }
    });
}
