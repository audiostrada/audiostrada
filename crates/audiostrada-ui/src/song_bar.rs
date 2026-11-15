//! The song's playback bar, shared by Express Mix and the effect editors: an outline of the
//! audio, the markers, the loop, where the song plays. A click moves there, a drag loops the
//! passage, a marker's name plays from it.

use audiostrada_core::routing::Channel;
use audiostrada_core::{Clip, Project, TrackId};
use audiostrada_media::Peak;
use eframe::egui::{self, Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::document::Document;
use crate::effect_frame::{field_fill, radius};
use crate::i18n::tr;
use crate::settings::WARN;
use crate::theme;

/// Room above the bar for the markers' names, below it, and at its right for the time.
const LABELS: f32 = 20.0;
const BELOW: f32 = 6.0;
const TIME: f32 = 120.0;

/// The bar in `rect`, `left` in from its left edge. `fill` gives the outline's heights (0 to
/// 1), one per column of the given number of samples from the song's start.
pub(crate) fn show(
    ui: &mut Ui,
    rect: Rect,
    left: f32,
    doc: &mut Document,
    fill: impl FnOnce(&Document, &mut [f32], f64),
) {
    let p = theme::current(ui);
    let project = doc.project();
    let rate = f64::from(project.sample_rate.max(1));
    let length = project.length().max(1);
    let position = doc.player().position().min(length);
    let bar = Rect::from_min_max(
        pos2(rect.left() + left, rect.top() + LABELS),
        pos2(rect.right() - TIME, rect.bottom() - BELOW),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(bar, radius(&p), field_fill(&p));
    let x_of = |at: u64| bar.left() + bar.width() * (at as f64 / length as f64) as f32;
    let columns = bar.width().max(1.0) as usize;
    let per_column = length as f64 / columns as f64;
    let mut heights = vec![0.0f32; columns];
    fill(doc, &mut heights, per_column);
    let played = x_of(position);
    if let Some((a, b)) = project.looping() {
        let span = Rect::from_x_y_ranges(x_of(a.min(length))..=x_of(b.min(length)), bar.y_range());
        painter.rect_filled(span, 0.0, p.accent.gamma_multiply(0.14));
        for x in [span.left(), span.right()] {
            painter.vline(
                x,
                bar.y_range(),
                Stroke::new(1.0, p.accent.gamma_multiply(0.7)),
            );
        }
    }
    for (k, h) in heights.iter().enumerate() {
        let x = bar.left() + k as f32 + 0.5;
        let h = h.clamp(0.0, 1.0) * (bar.height() / 2.0 - 2.0);
        if h < 0.5 {
            continue;
        }
        let ink = if x <= played {
            p.accent.gamma_multiply(0.8)
        } else {
            p.weak.gamma_multiply(0.45)
        };
        painter.line_segment(
            [pos2(x, bar.center().y - h), pos2(x, bar.center().y + h)],
            Stroke::new(1.0, ink),
        );
    }
    let response = ui
        .interact(
            bar.expand2(vec2(0.0, 4.0)),
            ui.scope_id().with("song-bar"),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tr("Click to play from there, drag to loop a passage"));
    let at = |x: f32| {
        let share = ((x - bar.left()) / bar.width()).clamp(0.0, 1.0);
        (f64::from(share) * length as f64) as u64
    };
    let mut go = None;
    // A drag draws the loop, from where it started to where it is.
    let start_id = response.id.with("loop-start");
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        ui.data_mut(|d| d.insert_temp(start_id, at(pos.x)));
    }
    let start: Option<u64> = ui.data(|d| d.get_temp(start_id));
    let mut looped = None;
    if let (Some(start), Some(pos)) = (start, response.interact_pointer_pos())
        && (response.dragged() || response.drag_stopped())
    {
        let end = at(pos.x);
        let span = (start.min(end), start.max(end));
        let drawn = Rect::from_x_y_ranges(x_of(span.0)..=x_of(span.1), bar.y_range());
        painter.rect_filled(drawn, 0.0, p.accent.gamma_multiply(0.22));
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<u64>(start_id));
            // Too short to hear as a loop: a click that slipped.
            if (span.1 - span.0) as f64 > rate * 0.25 {
                looped = Some(span);
            } else {
                go = Some(end);
            }
        }
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        go = Some(at(pos.x));
    }
    let project = doc.project();
    for (k, marker) in project.markers.iter().enumerate() {
        let x = x_of(marker.position.min(length));
        painter.line_segment(
            [pos2(x, bar.top() - 2.0), pos2(x, bar.bottom())],
            Stroke::new(1.0, WARN.gamma_multiply(0.8)),
        );
        let galley =
            painter.layout_no_wrap(marker.name.clone(), FontId::proportional(10.5), p.text);
        let label = Rect::from_min_size(
            pos2(x + 2.0, rect.top() + 3.0),
            galley.size() + vec2(6.0, 2.0),
        );
        let hit = ui.interact(label, ui.scope_id().with(("marker", k)), Sense::click());
        painter.rect_filled(
            label,
            3.0,
            if hit.hovered() {
                p.hover
            } else {
                WARN.gamma_multiply(0.18)
            },
        );
        painter.galley(label.min + vec2(3.0, 1.0), galley, p.text);
        if hit
            .on_hover_text(tr("Play from this marker"))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            go = Some(marker.position);
        }
    }
    painter.line_segment(
        [
            pos2(played, bar.top() - 3.0),
            pos2(played, bar.bottom() + 3.0),
        ],
        Stroke::new(2.0, p.text),
    );
    let time = |at: u64| {
        let s = (at as f64 / rate) as u64;
        format!("{}:{:02}", s / 60, s % 60)
    };
    painter.text(
        pos2(rect.right() - 22.0, bar.center().y),
        Align2::RIGHT_CENTER,
        format!("{} / {}", time(position), time(length)),
        FontId::monospace(11.5),
        p.weak,
    );
    if let Some(span) = looped {
        doc.history_mut().modify_unrecorded(|p| {
            p.loop_range = Some(span);
            p.loop_off = false;
        });
        go = Some(span.0);
    }
    if let Some(at) = go {
        doc.player_mut().locate(at.min(length));
    }
}

/// The height of the bar's middle line, from the top of its rect.
pub(crate) fn middle(rect: Rect) -> f32 {
    rect.top() + LABELS + (rect.height() - LABELS - BELOW) / 2.0
}

/// Whether `track` is `ancestor` or sits inside it, through folders and buses.
fn inside(project: &Project, mut track: TrackId, ancestor: TrackId) -> bool {
    for _ in 0..64 {
        if track == ancestor {
            return true;
        }
        match project.track(track).and_then(|t| t.parent) {
            Some(parent) => track = parent,
            None => return false,
        }
    }
    false
}

/// The clips `channel` hears: its track's and those of the tracks inside it, every track for
/// the master, a clip's own for a clip effect.
fn clips_heard(project: &Project, channel: Channel) -> Vec<&Clip> {
    match channel {
        Channel::Clip(id) => project
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| c.id == id)
            .collect(),
        Channel::Master => project.tracks.iter().flat_map(|t| &t.clips).collect(),
        Channel::Track(id) => project
            .tracks
            .iter()
            .filter(|t| inside(project, t.id, id))
            .flat_map(|t| &t.clips)
            .collect(),
    }
}

/// The outline of what `channel` hears: the peaks of its audio clips, against the loudest of
/// them, and its MIDI clips as a steady band, in columns of `per_column` samples.
pub(crate) fn outline(doc: &Document, channel: Channel, heights: &mut [f32], per_column: f64) {
    let project = doc.project();
    let mut amplitudes = vec![0.0f32; heights.len()];
    let mut buffer: Vec<Peak> = Vec::new();
    for clip in clips_heard(project, channel) {
        let first = (clip.start as f64 / per_column).ceil() as usize;
        let last = ((clip.end() as f64 / per_column).floor() as usize).min(heights.len());
        if first >= last {
            continue;
        }
        if clip.is_midi() {
            for h in &mut heights[first..last] {
                *h = h.max(0.35);
            }
            continue;
        }
        let Some(source) = project.sources.get(&clip.source) else {
            continue;
        };
        let Some(path) = audiostrada_engine::project::clip_media(project, doc.paths(), clip) else {
            continue;
        };
        let Some(peaks) = doc.media().peaks(&path) else {
            continue;
        };
        let ratio = f64::from(source.sample_rate) / f64::from(project.sample_rate.max(1));
        let gain = 10f32.powf(clip.gain_db / 20.0);
        let start = (clip.offset as f64 + (first as f64 * per_column - clip.start as f64)) * ratio;
        buffer.clear();
        buffer.resize(last - first, Peak::EMPTY);
        for channel in 0..peaks.channels() {
            peaks.query(channel, start, per_column * ratio, &mut buffer);
            for (a, peak) in amplitudes[first..last].iter_mut().zip(&buffer) {
                *a = a.max(peak.hi.max(-peak.lo) * gain);
            }
        }
    }
    let top = amplitudes.iter().copied().fold(1e-6, f32::max);
    for (h, a) in heights.iter_mut().zip(&amplitudes) {
        if *a > 0.0 {
            *h = h.max(((20.0 * (a / top).log10() + 36.0) / 36.0).clamp(0.0, 1.0));
        }
    }
}
