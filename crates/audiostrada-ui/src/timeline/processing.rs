//! Clip processing from the timeline: Heal, Consolidate, the Strip Silence window (with a
//! preview of what goes), the Process clips window (offline effects), Beat Detective and
//! Groove.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use audiostrada_core::edit::{self, StripMode};
use audiostrada_core::{ClipId, TrackId};
use eframe::egui::{self, Color32, Painter, Rect, RichText, Ui};

use super::TimelineView;
use crate::dialog;
use crate::document::{
    Band, Document, Found, GrooveMoves, GrooveScan, GrooveSettings, Grooved, HitScan, Process,
    StripSettings,
};
use crate::i18n::tr;
use crate::theme;
use crate::trf;

fn nothing_selected() -> String {
    tr("Select clips, or a range (Shift+drag across the tracks), first").into()
}

/// The Strip Silence window: its settings, the clips it works on and what it found.
#[derive(Debug)]
pub(super) struct StripWindow {
    clips: Vec<ClipId>,
    settings: StripSettings,
    mode: StripMode,
    found: Found,
    found_for: Option<StripSettings>,
    /// The analysis running for these settings.
    pending: Option<(StripSettings, Receiver<Found>)>,
}

/// What the Process clips window applies.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Choice {
    Effect(&'static str),
    Reverse,
    Normalize,
}

/// The Beat Detective window: where it looks, how hard, what it found.
#[derive(Debug)]
pub(super) struct BeatWindow {
    tracks: Vec<TrackId>,
    start: u64,
    end: u64,
    sensitivity: f32,
    strength: f64,
    /// The analysis, running on another thread until it is ready.
    pending: Option<Receiver<HitScan>>,
    scan: Option<HitScan>,
    hits: Vec<u64>,
    found_for: Option<f32>,
    /// The grid conformed to: the toolbar's when the window opens.
    grid: Option<crate::transport::Grid>,
}

/// The stretching of the grooved tracks, until their files are written.
type Stretching = Receiver<Result<Vec<Grooved>, String>>;

/// The Groove window: the tracks moved together, the reference they follow, what moves.
#[derive(Debug)]
pub(super) struct GrooveWindow {
    tracks: Vec<TrackId>,
    start: u64,
    end: u64,
    reference: Option<TrackId>,
    settings: GrooveSettings,
    /// The analysis, running on another thread until it is ready.
    pending: Option<Receiver<Arc<GrooveScan>>>,
    scan: Option<Arc<GrooveScan>>,
    moves: GrooveMoves,
    moves_for: Option<GrooveSettings>,
    /// The stretching, running until the new files are written: for a preview of some
    /// settings, or for good.
    applying: Option<(Stretching, Option<GrooveSettings>)>,
    error: Option<String>,
    /// Pre-listen: the tracks play grooved while the settings change.
    listen: bool,
    /// The preview in the project: its settings, the project revision and its files.
    preview: Option<(GrooveSettings, u64, Vec<std::path::PathBuf>)>,
    /// When the settings last changed, so a preview waits for them to settle.
    changed: (GrooveSettings, std::time::Instant),
}

impl GrooveWindow {
    /// Takes the preview out of the project.
    fn withdraw(&mut self, doc: &mut Document) {
        if let Some((_, revision, files)) = self.preview.take() {
            doc.withdraw_groove(revision, &files);
        }
    }

    /// Starts stretching the tracks, for a preview or for good.
    fn stretch(&mut self, doc: &Document, preview: bool) {
        let Some(scan) = &self.scan else {
            return;
        };
        match doc.groove_job(Arc::clone(scan), &self.moves, preview) {
            Ok(job) => {
                let (send, receive) = channel();
                std::thread::spawn(move || send.send(job()));
                self.applying = Some((receive, preview.then_some(self.settings)));
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Starts the analysis for the current reference.
    fn scan(&mut self, doc: &Document) {
        self.scan = None;
        self.moves_for = None;
        self.moves = GrooveMoves::default();
        self.error = None;
        let Some(reference) = self.reference else {
            self.error = Some(tr("Choose the track to follow").into());
            return;
        };
        let reference: Vec<TrackId> = doc
            .groove_sources(reference)
            .into_iter()
            .filter(|t| !self.tracks.contains(t))
            .collect();
        match doc.scan_groove(&self.tracks, &reference, self.start, self.end) {
            Ok(scan) => {
                let (send, receive) = channel();
                std::thread::spawn(move || send.send(Arc::new(scan())));
                self.pending = Some(receive);
            }
            Err(e) => self.error = Some(e),
        }
    }
}

/// Words of track names that tell drums, the usual reference.
const DRUM_WORDS: &[&str] = &[
    "drum",
    "drums",
    "kick",
    "kik",
    "bd",
    "batterie",
    "snare",
    "caisse",
    "overhead",
    "oh",
    "kit",
    "perc",
    "percussion",
    "percussions",
    "room",
    "toms",
];

/// Whether a track name holds one of `words`.
fn named(name: &str, words: &[&str]) -> bool {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| words.contains(&w))
}

/// The track the grooved ones most likely follow: a drum folder, else a drum track, else the
/// first other audio track.
fn guess_reference(project: &audiostrada_core::Project, tracks: &[TrackId]) -> Option<TrackId> {
    let others: Vec<&audiostrada_core::Track> = project
        .tracks
        .iter()
        .filter(|t| !tracks.contains(&t.id))
        .map(|t| t.as_ref())
        .collect();
    let folder =
        |t: &&audiostrada_core::Track| audiostrada_core::routing::has_children(project, t.id);
    let audio = |t: &&audiostrada_core::Track| t.kind == audiostrada_core::TrackKind::Audio;
    others
        .iter()
        .find(|t| folder(t) && named(&t.name, DRUM_WORDS))
        .or_else(|| {
            others
                .iter()
                .find(|t| audio(t) && named(&t.name, DRUM_WORDS))
        })
        .or_else(|| others.iter().find(|t| audio(t)))
        .map(|t| t.id)
}

#[derive(Debug)]
pub(super) struct ProcessWindow {
    clips: Vec<ClipId>,
    choice: Choice,
    params: BTreeMap<String, f32>,
    tail: bool,
    peak_db: f32,
}

impl TimelineView {
    /// The clips an operation works on: the selected ones, else those inside the range.
    pub(super) fn target_clips(&self, doc: &Document) -> Vec<ClipId> {
        if !self.selection.is_empty() {
            return self.selection.clone();
        }
        let Some(r) = &self.range else {
            return Vec::new();
        };
        doc.project()
            .tracks
            .iter()
            .filter(|t| r.tracks.contains(&t.id))
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.start < r.end && c.end() > r.start)
            .map(|c| c.id)
            .collect()
    }

    /// Track spans an operation works on: the range on its tracks, else what the selected
    /// clips cover on theirs.
    fn target_spans(&self, doc: &Document) -> Vec<(TrackId, u64, u64)> {
        if let Some(r) = &self.range {
            return r.tracks.iter().map(|&t| (t, r.start, r.end)).collect();
        }
        let project = doc.project();
        let mut spans: Vec<(TrackId, u64, u64)> = Vec::new();
        for t in &project.tracks {
            let clips: Vec<_> = t
                .clips
                .iter()
                .filter(|c| self.selection.contains(&c.id))
                .collect();
            if let (Some(start), Some(end)) = (
                clips.iter().map(|c| c.start).min(),
                clips.iter().map(|c| c.end()).max(),
            ) {
                spans.push((t.id, start, end));
            }
        }
        spans
    }

    pub(super) fn heal(&mut self, doc: &mut Document) {
        let spans = self.target_spans(doc);
        let mut healed = 0;
        for (track, start, end) in spans {
            healed += doc.heal(&[track], start, end);
        }
        if healed == 0 {
            doc.notify(
                tr("Nothing to heal: select clips cut from the same audio, still in place").into(),
            );
        }
    }

    pub(super) fn consolidate(&mut self, doc: &mut Document) {
        let spans = self.target_spans(doc);
        if let Err(e) = doc.consolidate(&spans) {
            doc.notify(trf!("Cannot consolidate: {error}", error = e));
        }
        self.selection.clear();
    }

    pub(super) fn open_strip(&mut self, doc: &mut Document) {
        let clips = self.target_clips(doc);
        if clips.is_empty() {
            doc.notify(nothing_selected());
            return;
        }
        self.strip = Some(StripWindow {
            clips,
            settings: StripSettings::default(),
            mode: StripMode::Strip,
            found: Vec::new(),
            found_for: None,
            pending: None,
        });
    }

    pub(super) fn open_process(&mut self, doc: &mut Document) {
        let clips = self.target_clips(doc);
        if clips.is_empty() {
            doc.notify(nothing_selected());
            return;
        }
        self.process = Some(ProcessWindow {
            clips,
            choice: Choice::Normalize,
            params: BTreeMap::new(),
            tail: false,
            peak_db: -1.0,
        });
    }

    pub(super) fn open_beat(&mut self, doc: &mut Document) {
        let spans = self.target_spans(doc);
        let (Some(start), Some(end)) = (
            spans.iter().map(|s| s.1).min(),
            spans.iter().map(|s| s.2).max(),
        ) else {
            doc.notify(nothing_selected());
            return;
        };
        let tracks: Vec<TrackId> = spans.iter().map(|s| s.0).collect();
        let scan = doc.scan_hits(&tracks, start, end);
        let (send, receive) = channel();
        std::thread::spawn(move || send.send(scan()));
        self.beat = Some(BeatWindow {
            tracks,
            start,
            end,
            sensitivity: 0.5,
            strength: 1.0,
            pending: Some(receive),
            scan: None,
            hits: Vec::new(),
            found_for: None,
            grid: None,
        });
    }

    pub(super) fn open_groove(&mut self, doc: &mut Document) {
        let spans = self.target_spans(doc);
        let (Some(start), Some(end)) = (
            spans.iter().map(|s| s.1).min(),
            spans.iter().map(|s| s.2).max(),
        ) else {
            doc.notify(nothing_selected());
            return;
        };
        let mut tracks: Vec<TrackId> = Vec::new();
        for s in &spans {
            if !tracks.contains(&s.0) {
                tracks.push(s.0);
            }
        }
        let project = doc.project();
        let bass = tracks
            .iter()
            .filter_map(|t| project.track(*t))
            .any(|t| named(&t.name, &["bass", "basse", "sub", "bassline"]));
        let mut window = GrooveWindow {
            reference: guess_reference(project, &tracks),
            tracks,
            start,
            end,
            settings: GrooveSettings {
                band: if bass { Band::Low } else { Band::All },
                ..GrooveSettings::default()
            },
            pending: None,
            scan: None,
            moves: GrooveMoves::default(),
            moves_for: None,
            applying: None,
            error: None,
            listen: false,
            preview: None,
            changed: (GrooveSettings::default(), std::time::Instant::now()),
        };
        window.scan(doc);
        self.groove = Some(window);
    }

    /// Shows where Groove moves the hits of `clip`: from the grey line to the colored one.
    pub(super) fn groove_preview(
        &self,
        painter: &Painter,
        rect: Rect,
        track: TrackId,
        clip: &audiostrada_core::Clip,
    ) {
        let Some(window) = &self.groove else {
            return;
        };
        if !window.tracks.contains(&track) || window.scan.is_none() {
            return;
        }
        let x = |at: u64| rect.left() + ((at as f64 - clip.start as f64) / self.zoom) as f32;
        let inside = |at: u64| at >= clip.start && at < clip.end();
        let accent = theme::current_for(painter.ctx()).accent;
        let y = rect.top() + 6.0;
        for &(from, to) in &window.moves.anchors {
            let (from, to) = (window.start + from as u64, window.start + to as u64);
            if from == to || !(inside(from) || inside(to)) {
                continue;
            }
            painter.vline(
                x(from),
                rect.y_range(),
                egui::Stroke::new(1.0, Color32::from_white_alpha(70)),
            );
            painter.vline(x(to), rect.y_range(), egui::Stroke::new(1.5, accent));
            painter.line_segment(
                [egui::pos2(x(from), y), egui::pos2(x(to), y)],
                egui::Stroke::new(2.0, accent),
            );
        }
    }

    /// Marks the hits Beat Detective found on `clip`, while its window is open.
    pub(super) fn beat_preview(
        &self,
        painter: &Painter,
        rect: Rect,
        track: TrackId,
        clip: &audiostrada_core::Clip,
    ) {
        let Some(window) = &self.beat else {
            return;
        };
        if !window.tracks.contains(&track) {
            return;
        }
        let first = window.hits.partition_point(|&at| at <= clip.start);
        for &at in &window.hits[first..] {
            if at >= clip.end() {
                break;
            }
            let x = rect.left() + ((at - clip.start) as f64 / self.zoom) as f32;
            painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, theme::RECORD));
        }
    }

    /// Shades what Strip Silence would remove from `clip`, while its window is open.
    pub(super) fn strip_preview(
        &self,
        painter: &Painter,
        rect: Rect,
        clip: &audiostrada_core::Clip,
    ) {
        let Some(window) = &self.strip else {
            return;
        };
        let Some((_, ranges)) = window.found.iter().find(|(id, _)| *id == clip.id) else {
            return;
        };
        let x = |at: u64| rect.left() + ((at - clip.start.min(at)) as f64 / self.zoom) as f32;
        let mut last = clip.start;
        let mut gaps: Vec<(u64, u64)> = Vec::new();
        for &(a, b) in ranges {
            if a > last {
                gaps.push((last, a));
            }
            last = last.max(b);
        }
        if clip.end() > last {
            gaps.push((last, clip.end()));
        }
        let (shade, keep) = match window.mode {
            StripMode::Strip => (gaps, false),
            StripMode::Extract => (ranges.clone(), false),
            StripMode::Separate => (gaps, true),
        };
        for (a, b) in shade {
            let r = Rect::from_x_y_ranges(x(a)..=x(b), rect.y_range()).intersect(rect);
            if keep {
                painter.vline(
                    r.left(),
                    rect.y_range(),
                    egui::Stroke::new(1.5, theme::RECORD),
                );
            } else {
                painter.rect_filled(r, 0.0, Color32::from_black_alpha(150));
            }
        }
    }

    pub(super) fn processing_windows(
        &mut self,
        ctx: &egui::Context,
        doc: &mut Document,
        grid: crate::transport::Grid,
    ) {
        self.strip_window(ctx, doc);
        self.process_window(ctx, doc);
        self.beat_window(ctx, doc, grid);
        self.groove_window(ctx, doc);
    }

    fn groove_window(&mut self, ctx: &egui::Context, doc: &mut Document) {
        let Some(window) = &mut self.groove else {
            return;
        };
        if let Some(pending) = &window.pending {
            match pending.try_recv() {
                Ok(scan) => {
                    window.scan = Some(scan);
                    window.pending = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint(),
                Err(TryRecvError::Disconnected) => window.pending = None,
            }
        }
        if let Some(scan) = &window.scan
            && window.moves_for != Some(window.settings)
        {
            window.moves = scan.moves(&window.settings);
            window.moves_for = Some(window.settings);
        }
        if window.changed.0 != window.settings {
            window.changed = (window.settings, std::time::Instant::now());
        }
        let mut finished = None;
        if let Some((applying, purpose)) = &window.applying {
            match applying.try_recv() {
                Ok(result) => finished = Some((result, *purpose)),
                Err(TryRecvError::Empty) => ctx.request_repaint(),
                Err(TryRecvError::Disconnected) => {
                    finished = Some((Err(tr("the stretching stopped").into()), *purpose));
                }
            }
        }
        if let Some((result, purpose)) = finished {
            window.applying = None;
            match (result, purpose) {
                (Ok(grooved), None) => {
                    doc.apply_groove(grooved, window.start, false);
                    self.groove = None;
                    self.selection.clear();
                    return;
                }
                // A preview still wanted goes in; an outdated one is thrown away.
                (Ok(grooved), Some(settings)) => {
                    if window.listen && settings == window.settings && window.preview.is_none() {
                        let (revision, files) = doc.apply_groove(grooved, window.start, true);
                        window.preview = Some((settings, revision, files));
                    } else {
                        for g in grooved {
                            let _ = std::fs::remove_file(g.path());
                        }
                    }
                }
                (Err(e), _) => window.error = Some(e),
            }
        }
        // While listening, the preview follows the settings once they settle.
        let settled = window.changed.1.elapsed() > std::time::Duration::from_millis(400);
        let outdated = window
            .preview
            .as_ref()
            .is_some_and(|p| p.0 != window.settings);
        if window.listen && outdated && settled {
            window.withdraw(doc);
        }
        if window.listen
            && window.preview.is_none()
            && window.applying.is_none()
            && window.scan.is_some()
            && window.moves.moved() > 0
            && window.error.is_none()
        {
            if settled {
                window.stretch(doc, true);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        if !window.listen && window.preview.is_some() {
            window.withdraw(doc);
        }
        let project = doc.project();
        let choices: Vec<(TrackId, String)> = project
            .tracks
            .iter()
            .filter(|t| !window.tracks.contains(&t.id))
            .filter(|t| t.kind.carries_audio())
            .map(|t| {
                let folder = audiostrada_core::routing::has_children(project, t.id);
                let name = if folder {
                    trf!("{name} (folder)", name = t.name)
                } else {
                    t.name.clone()
                };
                (t.id, name)
            })
            .collect();
        let detail = span_detail(project, &window.tracks, window.start, window.end);
        let ready = window.scan.is_some();
        let busy = window.applying.as_ref().is_some_and(|a| a.1.is_none());
        let previewing = window.applying.as_ref().is_some_and(|a| a.1.is_some());
        let mut open = true;
        let mut rescan = false;
        let mut apply = false;
        let sr = project.sample_rate.max(1) as f32;
        dialog::Dialog::new("groove", tr("Groove"))
            .detail(detail)
            .width(560.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    ui.spacing_mut().slider_width = 220.0;
                    dialog::hint(
                        ui,
                        tr("Moves the hits of these tracks onto those of another one, \
                            stretching the audio between them. The tracks move together, cut \
                            at the same places, so their phase is kept (a DI and its amp, \
                            several microphones)."),
                    );
                    dialog::section(ui, tr("Reference"), |ui| {
                        dialog::field(ui, tr("Follow"), |ui| {
                            let current = choices
                                .iter()
                                .find(|c| Some(c.0) == window.reference)
                                .map_or_else(|| tr("Choose…").to_owned(), |c| c.1.clone());
                            egui::ComboBox::from_id_salt("groove-reference")
                                .selected_text(current)
                                .width(260.0)
                                .show_ui(ui, |ui| {
                                    for (id, name) in &choices {
                                        if ui
                                            .selectable_label(window.reference == Some(*id), name)
                                            .clicked()
                                        {
                                            window.reference = Some(*id);
                                            rescan = true;
                                        }
                                    }
                                });
                        });
                        dialog::field(ui, tr("Hits to follow"), |ui| {
                            let bands: Vec<(Band, &str, bool)> =
                                Band::ALL.iter().map(|&b| (b, tr(b.name()), true)).collect();
                            dialog::segmented(ui, &mut window.settings.band, &bands);
                        });
                    });
                    dialog::section(ui, tr("Feel"), |ui| {
                        let mut chosen = feel_of(&window.settings);
                        let names: Vec<(Option<usize>, &str, bool)> = FEELS
                            .iter()
                            .enumerate()
                            .map(|(k, f)| (Some(k), tr(f.0), true))
                            .collect();
                        if dialog::segmented(ui, &mut chosen, &names)
                            && let Some(k) = chosen
                        {
                            (window.settings.tolerance_ms, window.settings.strength) =
                                (FEELS[k].2, FEELS[k].3);
                        }
                        dialog::hint(
                            ui,
                            feel_of(&window.settings)
                                .map_or(tr("Your own settings"), |k| tr(FEELS[k].1)),
                        );
                        dialog::field(ui, tr("Sensitivity"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut window.settings.sensitivity, 0.0..=1.0)
                                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                            )
                            .on_hover_text(tr("Higher finds softer hits, in both"));
                        });
                        dialog::field(ui, tr("Tolerance"), |ui| {
                            ui.add(
                                egui::Slider::new(
                                    &mut window.settings.tolerance_ms,
                                    10.0..=120.0,
                                )
                                .suffix(" ms")
                                .integer(),
                            );
                        });
                        dialog::hint(
                            ui,
                            tr("How far a hit may be from the one it follows; farther ones stay"),
                        );
                        dialog::field(ui, tr("Strength"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut window.settings.strength, 0.0..=1.0)
                                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                            );
                        });
                        dialog::hint(ui, tr("Below 100 %, some of the feel of the take stays"));
                    });
                    dialog::section(ui, tr("What moves"), |ui| {
                        dialog::well(ui, |ui| {
                            if let Some(error) = &window.error {
                                ui.colored_label(crate::settings::ERROR, error);
                            } else if ready {
                                let moves = &window.moves;
                                ui.label(trf!(
                                    "{moved} of the {hits} hits of the tracks move, by \
                                     {largest} ms at most ({reference} hits in the reference)",
                                    moved = moves.moved(),
                                    hits = moves.hits,
                                    reference = moves.reference_hits,
                                    largest =
                                        format!("{:.0}", moves.largest() as f32 / sr * 1000.0)
                                ));
                            } else {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(tr("Looking for the hits…"));
                                });
                            }
                        });
                        dialog::hint(
                            ui,
                            tr("In the clips: each hit goes from the grey line to the colored one."),
                        );
                    });
                }
                dialog::Part::Actions => {
                    let status = if busy {
                        Some(tr("Stretching…"))
                    } else if previewing {
                        Some(tr("Preparing the pre-listen…"))
                    } else {
                        None
                    };
                    if let Some(status) = status {
                        ui.spinner();
                        summary(ui, status);
                    } else if window.preview.is_some() {
                        summary(ui, tr("You hear the grooved tracks"));
                    }
                    dialog::buttons(ui, |ui| {
                        let can = ready && !busy && window.moves.moved() > 0;
                        apply = dialog::primary(ui, tr("Groove"), can)
                            .on_hover_text(tr(
                                "Stretch the tracks into a new playlist; the current one stays \
                                 as an alternate",
                            ))
                            .clicked()
                            && can;
                        toggle(ui, &mut window.listen, tr("Pre-listen")).on_hover_text(tr(
                            "Hear the tracks grooved while you adjust; nothing stays until you \
                             press Groove",
                        ));
                    });
                }
            });
        // The analysis and the result start from the tracks as they were.
        if rescan {
            window.withdraw(doc);
            window.scan(doc);
        }
        if apply {
            window.listen = false;
            window.withdraw(doc);
            window.applying = None;
            window.stretch(doc, false);
        }
        if !open && !busy {
            window.withdraw(doc);
            self.groove = None;
        }
    }

    fn beat_window(
        &mut self,
        ctx: &egui::Context,
        doc: &mut Document,
        grid: crate::transport::Grid,
    ) {
        let Some(window) = &mut self.beat else {
            return;
        };
        if let Some(pending) = &window.pending {
            match pending.try_recv() {
                Ok(scan) => {
                    window.scan = Some(scan);
                    window.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint(),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => window.pending = None,
            }
        }
        if let Some(scan) = &window.scan
            && window.found_for != Some(window.sensitivity)
        {
            window.hits = scan.hits(window.sensitivity);
            window.found_for = Some(window.sensitivity);
        }
        let grid = *window.grid.get_or_insert(grid);
        let ready = window.scan.is_some();
        let detail = span_detail(doc.project(), &window.tracks, window.start, window.end);
        let mut open = true;
        let mut action = None;
        dialog::Dialog::new("beat-detective", tr("Beat Detective"))
            .detail(detail)
            .width(520.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    ui.spacing_mut().slider_width = 200.0;
                    dialog::section(ui, tr("Hits"), |ui| {
                        dialog::field(ui, tr("Sensitivity"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut window.sensitivity, 0.0..=1.0)
                                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                            )
                            .on_hover_text(tr("Higher finds softer hits"));
                        });
                        dialog::field(ui, tr("Result"), |ui| {
                            if ready {
                                ui.label(
                                    RichText::new(trf!(
                                        "{count} hit(s), marked in red",
                                        count = window.hits.len()
                                    ))
                                    .strong(),
                                );
                            } else {
                                ui.spinner();
                                ui.label(tr("Looking for the hits…"));
                            }
                        });
                        dialog::hint(
                            ui,
                            tr("Hits are looked for on all the tracks together, so the \
                                microphones of a kit are cut at the same places."),
                        );
                    });
                    dialog::section(ui, tr("Conform to the grid"), |ui| {
                        dialog::field(ui, tr("Grid"), |ui| {
                            let choices: Vec<(crate::transport::Grid, &str, bool)> =
                                crate::transport::Grid::ALL
                                    .iter()
                                    .map(|&g| (g, g.name(), true))
                                    .collect();
                            let mut chosen = grid;
                            if dialog::segmented(ui, &mut chosen, &choices) {
                                window.grid = Some(chosen);
                            }
                        });
                        dialog::field(ui, tr("Strength"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut window.strength, 0.0..=1.0)
                                    .custom_formatter(|v, _| format!("{:.0} %", v * 100.0)),
                            );
                        });
                        dialog::hint(
                            ui,
                            tr("Each slice moves toward the nearest grid line, the slices \
                                joined with short crossfades. Below 100 %, some of the feel \
                                stays."),
                        );
                    });
                }
                dialog::Part::Actions => {
                    dialog::buttons(ui, |ui| {
                        if dialog::primary(ui, tr("Conform"), ready)
                            .on_hover_text(tr(
                                "Cut at the hits and move each slice toward the nearest grid \
                                 line, joining the slices with short crossfades",
                            ))
                            .clicked()
                            && ready
                        {
                            action = Some(Some((grid.beats(), window.strength)));
                        }
                        if dialog::secondary(ui, tr("Separate only"))
                            .on_hover_text(tr("Cut the clips where the hits start"))
                            .clicked()
                            && ready
                        {
                            action = Some(None);
                        }
                    });
                }
            });
        if let Some(conform) = action {
            let (tracks, start, end, hits) = (
                window.tracks.clone(),
                window.start,
                window.end,
                window.hits.clone(),
            );
            doc.beat_detective(&tracks, start, end, &hits, conform);
            self.selection.clear();
            open = false;
        }
        if !open {
            self.beat = None;
        }
    }

    fn strip_window(&mut self, ctx: &egui::Context, doc: &mut Document) {
        let Some(window) = &mut self.strip else {
            return;
        };
        window
            .clips
            .retain(|id| edit::clip(doc.project(), *id).is_some());
        if let Some((settings, pending)) = &window.pending {
            match pending.try_recv() {
                Ok(found) => {
                    window.found = found;
                    window.found_for = Some(*settings);
                    window.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint(),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => window.pending = None,
            }
        }
        // One analysis at a time; the latest settings are analysed once it is done.
        if window.pending.is_none() && window.found_for != Some(window.settings) {
            let scan = doc.scan_sound(&window.clips, &window.settings);
            let (send, receive) = channel();
            std::thread::spawn(move || send.send(scan()));
            window.pending = Some((window.settings, receive));
        }
        let ready = window.found_for == Some(window.settings);
        let project = doc.project();
        let detail = clips_detail(project, &window.clips);
        let rate = project.sample_rate.max(1) as f32;
        let total: u64 = window
            .clips
            .iter()
            .filter_map(|id| edit::clip(project, *id))
            .map(|c| c.length)
            .sum();
        let mut open = true;
        let (mut apply, mut cancel) = (false, false);
        dialog::Dialog::new("strip-silence", tr("Strip Silence"))
            .detail(detail)
            .width(540.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    ui.spacing_mut().slider_width = 200.0;
                    let s = &mut window.settings;
                    dialog::section(ui, tr("Ready-made settings"), |ui| {
                        let mut chosen = STRIP_PRESETS.iter().position(|p| p.2 == *s);
                        let names: Vec<(Option<usize>, &str, bool)> = STRIP_PRESETS
                            .iter()
                            .enumerate()
                            .map(|(k, p)| (Some(k), tr(p.0), true))
                            .collect();
                        if dialog::segmented(ui, &mut chosen, &names)
                            && let Some(k) = chosen
                        {
                            *s = STRIP_PRESETS[k].2;
                        }
                        let hint = STRIP_PRESETS
                            .iter()
                            .find(|p| p.2 == *s)
                            .map_or(tr("Your own settings"), |p| tr(p.1));
                        dialog::hint(ui, hint);
                    });
                    dialog::section(ui, tr("Silence"), |ui| {
                        dialog::field(ui, tr("Threshold"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut s.threshold_db, -96.0..=0.0).suffix(" dB"),
                            );
                        });
                        dialog::hint(ui, tr("Quieter than this counts as silence"));
                        dialog::field(ui, tr("Shortest silence"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut s.min_silence_ms, 0.0..=2000.0)
                                    .logarithmic(true)
                                    .suffix(" ms"),
                            );
                        });
                        dialog::hint(
                            ui,
                            tr("Shorter gaps stay, so words and notes hold together"),
                        );
                    });
                    dialog::section(ui, tr("Margins"), |ui| {
                        dialog::field(ui, tr("Keep before"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut s.pad_before_ms, 0.0..=500.0).suffix(" ms"),
                            );
                        });
                        dialog::field(ui, tr("Keep after"), |ui| {
                            ui.add(
                                egui::Slider::new(&mut s.pad_after_ms, 0.0..=2000.0).suffix(" ms"),
                            );
                        });
                        dialog::hint(
                            ui,
                            tr("Kept around each sound, for its attack and what rings after it"),
                        );
                    });
                    dialog::section(ui, tr("Result"), |ui| {
                        dialog::field(ui, tr("Keep"), |ui| {
                            dialog::segmented(
                                ui,
                                &mut window.mode,
                                &[
                                    (StripMode::Strip, tr("The sound"), true),
                                    (StripMode::Extract, tr("The silences"), true),
                                    (StripMode::Separate, tr("Both, cut"), true),
                                ],
                            );
                        });
                        dialog::hint(
                            ui,
                            match window.mode {
                                StripMode::Strip => {
                                    tr("Removes the silences. The darkened parts of the clips go.")
                                }
                                StripMode::Extract => {
                                    tr("Removes the sound, keeps the room noise. The darkened \
                                     parts of the clips go.")
                                }
                                StripMode::Separate => {
                                    tr("Only cuts the clips where the silences start and end, at \
                                     the red lines.")
                                }
                            },
                        );
                    });
                }
                dialog::Part::Actions => {
                    if ready {
                        let pieces: usize = window.found.iter().map(|(_, r)| r.len()).sum();
                        let sound: u64 = window
                            .found
                            .iter()
                            .flat_map(|(_, r)| r.iter())
                            .map(|(a, b)| b.saturating_sub(*a))
                            .sum();
                        let silence = total.saturating_sub(sound) as f32 / rate;
                        summary(
                            ui,
                            &trf!(
                                "{pieces} part(s) with sound · {silence} s of silence",
                                pieces = pieces,
                                silence = format!("{silence:.1}")
                            ),
                        );
                    } else {
                        ui.spinner();
                        summary(ui, tr("Looking for the silences…"));
                    }
                    dialog::buttons(ui, |ui| {
                        let text = match window.mode {
                            StripMode::Strip => tr("Strip"),
                            StripMode::Extract => tr("Remove the sound"),
                            StripMode::Separate => tr("Cut"),
                        };
                        apply = dialog::primary(ui, text, ready).clicked() && ready;
                        cancel = dialog::secondary(ui, tr("Cancel")).clicked();
                    });
                }
            });
        if apply {
            let left = doc.strip_silence(&window.found, window.mode);
            self.selection = left;
            open = false;
        }
        if !open || cancel {
            self.strip = None;
        }
    }

    fn process_window(&mut self, ctx: &egui::Context, doc: &mut Document) {
        let Some(window) = &mut self.process else {
            return;
        };
        let detail = clips_detail(doc.project(), &window.clips);
        let mut open = true;
        let mut render = false;
        dialog::Dialog::new("process-clips", tr("Process Clips"))
            .detail(detail)
            .width(560.0)
            .show(ctx, &mut open, |ui, part| match part {
                dialog::Part::Body => {
                    ui.spacing_mut().slider_width = 200.0;
                    let info = match window.choice {
                        Choice::Effect(id) => audiostrada_dsp::info(id),
                        _ => None,
                    };
                    dialog::section(ui, tr("Process"), |ui| {
                        dialog::field(ui, tr("Process"), |ui| {
                            process_choice(ui, window);
                        });
                        let about = match window.choice {
                            Choice::Normalize => {
                                tr("Brings the loudest peak of each clip to a level.")
                            }
                            Choice::Reverse => tr("Plays the clips backwards."),
                            Choice::Effect(_) => info.map_or("", |i| tr(i.description)),
                        };
                        dialog::hint(ui, about);
                    });
                    match window.choice {
                        Choice::Normalize => {
                            dialog::section(ui, tr("Settings"), |ui| {
                                dialog::field(ui, tr("Peak"), |ui| {
                                    ui.add(
                                        egui::Slider::new(&mut window.peak_db, -24.0..=0.0)
                                            .suffix(" dB"),
                                    );
                                    for target in [-0.1f32, -1.0, -3.0] {
                                        if ui.small_button(format!("{target}")).clicked() {
                                            window.peak_db = target;
                                        }
                                    }
                                });
                            });
                        }
                        Choice::Reverse => {}
                        Choice::Effect(_) => {
                            if let Some(info) = info
                                && !info.params.is_empty()
                            {
                                dialog::section(ui, tr("Settings"), |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(360.0)
                                        .show(ui, |ui| effect_params(ui, info, &mut window.params));
                                });
                            }
                            dialog::section(ui, tr("Result"), |ui| {
                                dialog::field(ui, tr("Tail"), |ui| {
                                    ui.checkbox(
                                        &mut window.tail,
                                        tr("Keep what rings after the clip"),
                                    );
                                });
                                dialog::hint(
                                    ui,
                                    tr("For reverbs and delays: the clip grows by their tail."),
                                );
                            });
                        }
                    }
                }
                dialog::Part::Actions => {
                    summary(
                        ui,
                        &trf!(
                            "{count} clip(s) replaced by the result; undo restores them",
                            count = window.clips.len()
                        ),
                    );
                    dialog::buttons(ui, |ui| {
                        render = dialog::primary(ui, tr("Render"), true).clicked();
                        if let Choice::Effect(id) = window.choice
                            && let Some(info) = audiostrada_dsp::info(id)
                            && !info.params.is_empty()
                            && dialog::secondary(ui, tr("Defaults"))
                                .on_hover_text(tr("Back to the effect's default settings"))
                                .clicked()
                        {
                            window.params = defaults(info);
                        }
                    });
                }
            });
        if render {
            let process = match window.choice {
                Choice::Effect(id) => Process::Effect {
                    id: id.to_owned(),
                    params: window.params.clone(),
                    tail: window.tail,
                },
                Choice::Reverse => Process::Reverse,
                Choice::Normalize => Process::Normalize(window.peak_db),
            };
            match doc.process_clips(&window.clips, &process) {
                Ok(()) => open = false,
                Err(e) => doc.notify(trf!("Cannot process: {error}", error = e)),
            }
        }
        if !open {
            self.process = None;
        }
    }
}

/// The settings of Strip Silence that cover most needs: name, what it does, settings.
const STRIP_PRESETS: [(&str, &str, StripSettings); 4] = [
    (
        "General",
        "Takes off what is clearly silent, with short margins",
        StripSettings {
            threshold_db: -48.0,
            min_silence_ms: 100.0,
            pad_before_ms: 5.0,
            pad_after_ms: 30.0,
        },
    ),
    (
        "Vocals",
        "Takes off the noise between the phrases, keeps the breaths before them",
        StripSettings {
            threshold_db: -45.0,
            min_silence_ms: 250.0,
            pad_before_ms: 20.0,
            pad_after_ms: 150.0,
        },
    ),
    (
        "Speech",
        "Keeps the words whole, takes off the long pauses",
        StripSettings {
            threshold_db: -50.0,
            min_silence_ms: 400.0,
            pad_before_ms: 40.0,
            pad_after_ms: 200.0,
        },
    ),
    (
        "Toms",
        "Keeps each hit and its ring, takes off the other drums heard between them",
        StripSettings {
            threshold_db: -36.0,
            min_silence_ms: 60.0,
            pad_before_ms: 2.0,
            pad_after_ms: 300.0,
        },
    ),
];

/// How tightly Groove follows: name, what it does, tolerance and strength.
const FEELS: [(&str, &str, f32, f64); 3] = [
    (
        "Tight",
        "Every hit near one of the reference lands on it",
        35.0,
        1.0,
    ),
    (
        "Natural",
        "Most of the way, some of the feel of the take stays",
        35.0,
        0.75,
    ),
    (
        "Loose",
        "Only the hits close to the reference, half of the way",
        25.0,
        0.5,
    ),
];

fn feel_of(settings: &GrooveSettings) -> Option<usize> {
    FEELS
        .iter()
        .position(|f| f.2 == settings.tolerance_ms && f.3 == settings.strength)
}

/// Names of tracks, the first ones only when there are many.
fn track_names(project: &audiostrada_core::Project, tracks: &[TrackId]) -> String {
    let names: Vec<&str> = tracks
        .iter()
        .filter_map(|t| project.track(*t))
        .map(|t| t.name.as_str())
        .collect();
    if names.len() <= 3 {
        names.join(", ")
    } else {
        trf!(
            "{names} and {more} more",
            names = names[..2].join(", "),
            more = names.len() - 2
        )
    }
}

/// The tracks and the time span a window acts on.
fn span_detail(
    project: &audiostrada_core::Project,
    tracks: &[TrackId],
    start: u64,
    end: u64,
) -> String {
    let rate = project.sample_rate;
    trf!(
        "{tracks}, from {start} to {end}",
        tracks = track_names(project, tracks),
        start = crate::mixer::format_time(start, rate),
        end = crate::mixer::format_time(end, rate)
    )
}

/// The clips a window acts on, and their tracks.
fn clips_detail(project: &audiostrada_core::Project, clips: &[ClipId]) -> String {
    let tracks: Vec<TrackId> = project
        .tracks
        .iter()
        .filter(|t| t.clips.iter().any(|c| clips.contains(&c.id)))
        .map(|t| t.id)
        .collect();
    trf!(
        "{count} clip(s) on {tracks}",
        count = clips.len(),
        tracks = track_names(project, &tracks)
    )
}

/// A short line at the left of the actions band.
fn summary(ui: &mut Ui, text: &str) {
    let weak = theme::current(ui).weak;
    ui.label(RichText::new(text).size(11.5).color(weak));
}

/// A button of the actions band that stays lit while on.
fn toggle(ui: &mut Ui, on: &mut bool, text: &str) -> egui::Response {
    use crate::effect_frame::{Face, radius, text_button};
    let p = theme::current(ui);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::strong(12.0), p.text)
        .size()
        .x
        + 28.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(80.0), 30.0), egui::Sense::hover());
    let face = if *on {
        Face::Lit(theme::mix(p.surface, p.accent, 0.7))
    } else {
        Face::Plain
    };
    let id = ui.scope_id().with(("toggle", text));
    let response = text_button(ui, rect, id, text, face, radius(&p) as u8)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        *on = !*on;
    }
    response
}

fn defaults(info: &audiostrada_dsp::EffectInfo) -> BTreeMap<String, f32> {
    info.params
        .iter()
        .map(|p| (p.id.to_owned(), p.default))
        .collect()
}

/// The list of processes: normalize, reverse, then the built-in effects by family.
fn process_choice(ui: &mut Ui, window: &mut ProcessWindow) {
    let name = match window.choice {
        Choice::Effect(id) => audiostrada_dsp::info(id).map_or(id, |i| tr(i.name)),
        Choice::Reverse => tr("Reverse"),
        Choice::Normalize => tr("Normalize"),
    };
    egui::ComboBox::from_id_salt("process-choice")
        .selected_text(name)
        .width(240.0)
        .height(420.0)
        .show_ui(ui, |ui| {
            for (choice, text) in [
                (Choice::Normalize, tr("Normalize")),
                (Choice::Reverse, tr("Reverse")),
            ] {
                ui.selectable_value(&mut window.choice, choice, text);
            }
            for category in audiostrada_dsp::CATEGORIES {
                let effects: Vec<_> = audiostrada_dsp::EFFECTS
                    .iter()
                    .filter(|e| e.category == *category)
                    .filter(|e| !e.instrument && e.files.is_empty())
                    .filter(|e| e.id != "analyzer" && e.id != "tuner")
                    .collect();
                if effects.is_empty() {
                    continue;
                }
                ui.separator();
                ui.label(RichText::new(tr(category)).small().weak());
                for e in effects {
                    if ui
                        .selectable_label(window.choice == Choice::Effect(e.id), tr(e.name))
                        .clicked()
                    {
                        window.choice = Choice::Effect(e.id);
                        window.params = defaults(e);
                    }
                }
            }
        });
}

/// Sliders, switches and lists for an effect's parameters, one labelled field each.
fn effect_params(
    ui: &mut Ui,
    info: &audiostrada_dsp::EffectInfo,
    params: &mut BTreeMap<String, f32>,
) {
    for p in info.params {
        let value = params.entry(p.id.to_owned()).or_insert(p.default);
        dialog::field(ui, tr(p.name), |ui| {
            if p.toggle {
                let mut on = *value >= 0.5;
                if ui.checkbox(&mut on, "").changed() {
                    *value = f32::from(u8::from(on));
                }
            } else if !p.choices.is_empty() {
                let current = (*value - p.min).round().max(0.0) as usize;
                egui::ComboBox::from_id_salt(("process-param", p.id))
                    .selected_text(p.choices.get(current).copied().map_or("?", tr))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (i, name) in p.choices.iter().enumerate() {
                            if ui.selectable_label(i == current, tr(name)).clicked() {
                                *value = p.min + i as f32;
                            }
                        }
                    });
            } else {
                ui.add(egui::Slider::new(value, p.min..=p.max).suffix(format!(" {}", p.unit)));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use audiostrada_core::{Project, Source};

    use super::*;
    use crate::document::AppDirs;
    use crate::timeline::Range;

    /// A timeline over two tracks of real audio (drum-like hits), loaded.
    fn loaded_view(name: &str) -> (TimelineView, Rc<RefCell<Document>>, std::path::PathBuf) {
        use audiostrada_media::{AudioData, write_wav};
        let dir = audiostrada_core::scratch::dir(&format!("processing-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut files = Vec::new();
        for (i, name) in ["kick.wav", "bass.wav"].iter().enumerate() {
            let samples: Vec<f32> = (0..96_000)
                .map(|n| {
                    let hit = (n + i * 700) % 12_000;
                    if hit < 2_000 {
                        (1.0 - hit as f32 / 2_000.0) * ((n as f32) * 0.05).sin()
                    } else {
                        0.0
                    }
                })
                .collect();
            let file = dir.join(name);
            write_wav(&file, &AudioData::new(vec![samples], 48_000)).unwrap();
            files.push(file);
        }
        let mut doc = Document::new(AppDirs::default());
        doc.import(&files);
        crate::control::loaded(&mut doc, false).unwrap();
        let doc = Rc::new(RefCell::new(doc));
        let mut view = TimelineView::new(Rc::clone(&doc));
        view.selection = doc
            .borrow()
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        (view, doc, dir)
    }

    fn frames(view: &mut TimelineView, n: usize) {
        let ctx = egui::Context::default();
        let mut settings = crate::transport::EditSettings::default();
        for i in 0..n {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                time: Some(i as f64 * 0.1),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| view.show(ui, &mut settings))
                .drop_without_applying_deltas();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn the_timeline_answers_a_user_trying_everything() {
        let (mut view, doc, dir) = loaded_view("sweep");
        doc.borrow_mut().history_mut().edit("MIDI", |p| {
            let t = p.add_track("Keys");
            p.add_midi_clip(t, 0, 48_000, Vec::new());
            // Fades on the first clip, and a copy overlapping it into a crossfade.
            let first = p.tracks[0].clips[0].clone();
            edit::set_fades(p, first.id, 6_000, 9_000);
            if let Some(copy) = p.add_clip(p.tracks[0].id, first.source, first.length / 2) {
                edit::resolve_overlaps(p, p.tracks[0].id, &[copy]);
            }
        });
        view.track_height = 110.0;
        let ctx = egui::Context::default();
        let mut settings = crate::transport::EditSettings::default();
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 600.0));
        for (i, events) in crate::test_sweep::script(area, 28.0, true)
            .into_iter()
            .enumerate()
        {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                time: Some(i as f64 * 0.05),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| view.show(ui, &mut settings))
                .drop_without_applying_deltas();
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_processing_window_draws_and_finds_something() {
        let (mut view, doc, dir) = loaded_view("windows");
        view.open_strip(&mut doc.borrow_mut());
        for mode in [StripMode::Strip, StripMode::Extract, StripMode::Separate] {
            view.strip.as_mut().unwrap().mode = mode;
            frames(&mut view, 3);
        }
        assert!(
            !view.strip.as_ref().unwrap().found.is_empty(),
            "silences found"
        );
        let choices = [Choice::Normalize, Choice::Reverse].into_iter().chain(
            audiostrada_dsp::EFFECTS
                .iter()
                .filter(|e| !e.instrument)
                .map(|e| Choice::Effect(e.id)),
        );
        view.open_process(&mut doc.borrow_mut());
        for choice in choices {
            view.process.as_mut().unwrap().choice = choice;
            frames(&mut view, 1);
        }
        view.open_beat(&mut doc.borrow_mut());
        frames(&mut view, 20);
        assert!(
            view.beat.as_ref().is_some_and(|b| !b.hits.is_empty()),
            "hits found"
        );
        view.open_groove(&mut doc.borrow_mut());
        frames(&mut view, 20);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn operations_target_the_selection_or_the_range() {
        let mut p = Project::new("t", 48_000);
        let s = p.add_source(Source {
            path: "a.wav".into(),
            channels: 1,
            sample_rate: 48_000,
            frames: 1000,
        });
        let (a, b) = (p.add_track("A"), p.add_track("B"));
        let a1 = p.add_clip(a, s, 0).unwrap();
        let a2 = p.add_clip(a, s, 5000).unwrap();
        let b1 = p.add_clip(b, s, 2000).unwrap();
        let mut doc = Document::new(AppDirs::default());
        doc.history_mut().replace(p, false);
        let mut view = TimelineView::new(Rc::new(RefCell::new(Document::new(AppDirs::default()))));

        assert!(view.target_clips(&doc).is_empty());
        assert!(view.target_spans(&doc).is_empty());
        view.open_strip(&mut doc);
        view.open_process(&mut doc);
        view.open_beat(&mut doc);
        assert!(view.strip.is_none() && view.process.is_none() && view.beat.is_none());
        assert_eq!(doc.take_messages(), vec![nothing_selected(); 3]);

        view.selection = vec![a2, b1, a1];
        assert_eq!(view.target_clips(&doc), [a2, b1, a1]);
        assert_eq!(view.target_spans(&doc), [(a, 0, 6000), (b, 2000, 3000)]);
        view.open_process(&mut doc);
        assert_eq!(view.process.as_ref().unwrap().clips, [a2, b1, a1]);

        view.selection.clear();
        view.range = Some(Range {
            start: 500,
            end: 2500,
            tracks: vec![a, b],
        });
        assert_eq!(view.target_clips(&doc), [a1, b1]);
        assert_eq!(view.target_spans(&doc), [(a, 500, 2500), (b, 500, 2500)]);
        view.heal(&mut doc);
        assert_eq!(doc.take_messages().len(), 1, "nothing to heal");
    }
}
