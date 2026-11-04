//! Drawing the ruler, the grid, clips with their waveforms and fades, takes and lanes.

use super::*;

impl TimelineView {
    pub(super) fn draw_ruler(
        &self,
        painter: &egui::Painter,
        ruler: Rect,
        project: &Project,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        painter.rect_filled(ruler, 0.0, visuals.panel_fill);
        let rate = project.sample_rate;
        let tempo = &project.tempo;
        let first_beat = tempo
            .sample_to_beat(scale.sample(ruler.left()), rate)
            .floor()
            .max(0.0);
        let last_beat = tempo
            .sample_to_beat(scale.sample(ruler.right()), rate)
            .ceil();
        let beat_px = (tempo.beat_to_sample(first_beat + 1.0, rate)
            - tempo.beat_to_sample(first_beat, rate)) as f64
            / scale.zoom;
        let per_bar = tempo.signature_at_beat(first_beat).beats_per_bar();
        let label_every = if beat_px * per_bar < 40.0 {
            (40.0 / (beat_px * per_bar)).ceil() * per_bar
        } else {
            per_bar
        };
        let mut beat = first_beat;
        let mut count = 0;
        while beat <= last_beat && count < 4000 {
            count += 1;
            let x = scale.x(tempo.beat_to_sample(beat, rate));
            let bbt = tempo.beat_to_bbt(beat);
            let on_bar = bbt.beat == 1 && bbt.tick == 0;
            if on_bar || beat_px > 6.0 {
                let h = if on_bar { 12.0 } else { 6.0 };
                painter.line_segment(
                    [pos2(x, ruler.bottom() - h), pos2(x, ruler.bottom())],
                    Stroke::new(1.0, visuals.weak_text_color()),
                );
            }
            let bar_index = f64::from(bbt.bar - 1) * per_bar;
            if on_bar && (bar_index % label_every).abs() < 1e-6 {
                painter.text(
                    pos2(x + 3.0, ruler.top() + 2.0),
                    Align2::LEFT_TOP,
                    bbt.bar.to_string(),
                    FontId::monospace(11.0),
                    visuals.text_color(),
                );
            }
            beat += 1.0;
        }
        if let Some((start, end)) = project.loop_range {
            let r = Rect::from_x_y_ranges(
                scale.x(start)..=scale.x(end),
                (ruler.bottom() - 6.0)..=ruler.bottom(),
            );
            // Kept but off: an outline only.
            if project.loop_off {
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.0, visuals.weak_text_color()),
                    StrokeKind::Inside,
                );
            } else {
                painter.rect_filled(r, 2.0, visuals.selection.bg_fill.gamma_multiply(0.8));
            }
        }
        if let Some((start, end)) = project.punch_range {
            let r = Rect::from_x_y_ranges(
                scale.x(start)..=scale.x(end),
                ruler.top()..=(ruler.top() + 4.0),
            );
            painter.rect_filled(r, 2.0, PUNCH);
        }
        if let Some(range) = &self.range {
            let r =
                Rect::from_x_y_ranges(scale.x(range.start)..=scale.x(range.end), ruler.y_range());
            painter.rect_filled(r, 0.0, RANGE.gamma_multiply(0.22));
            for x in [r.left(), r.right()] {
                painter.vline(x, ruler.y_range(), Stroke::new(2.0, RANGE));
            }
        }
        for marker in &project.markers {
            let x = scale.x(marker.position);
            if x < ruler.left() || x > ruler.right() {
                continue;
            }
            let color = Color32::from_rgb(230, 170, 40);
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(x, ruler.top() + 14.0),
                    pos2(x + 8.0, ruler.top() + 14.0),
                    pos2(x, ruler.bottom() - 6.0),
                ],
                color,
                Stroke::NONE,
            ));
            painter.text(
                pos2(x + 10.0, ruler.top() + 13.0),
                Align2::LEFT_TOP,
                &marker.name,
                FontId::proportional(10.0),
                color,
            );
        }
    }

    pub(super) fn draw_grid(
        &self,
        painter: &egui::Painter,
        lanes: Rect,
        project: &Project,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        let rate = project.sample_rate;
        let tempo = &project.tempo;
        let first = tempo
            .sample_to_beat(scale.sample(lanes.left()), rate)
            .floor()
            .max(0.0);
        let last = tempo
            .sample_to_beat(scale.sample(lanes.right()), rate)
            .ceil();
        let beat_px = (tempo.beat_to_sample(first + 1.0, rate) - tempo.beat_to_sample(first, rate))
            as f64
            / scale.zoom;
        let mut beat = first;
        let mut count = 0;
        while beat <= last && count < 4000 {
            count += 1;
            let bbt = tempo.beat_to_bbt(beat);
            let on_bar = bbt.beat == 1 && bbt.tick == 0;
            if on_bar || beat_px > 12.0 {
                let x = scale.x(tempo.beat_to_sample(beat, rate));
                let alpha = if on_bar { 0.12 } else { 0.05 };
                painter.line_segment(
                    [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
                    Stroke::new(1.0, visuals.text_color().gamma_multiply(alpha)),
                );
            }
            beat += 1.0;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_clip(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        doc: &Document,
        project: &Project,
        track: &Track,
        clip: &audiostrada_core::Clip,
        selected: bool,
        visuals: &egui::Visuals,
    ) {
        let mut base = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);
        if clip.muted {
            base = visuals.weak_text_color();
        }
        let fill = if selected {
            base.gamma_multiply(0.55)
        } else {
            base.gamma_multiply(0.3)
        };
        painter.rect_filled(rect, 3.0, fill);
        // Name bar in the track color.
        let bar = Rect::from_min_max(
            rect.min,
            pos2(rect.right(), rect.top() + 14.0_f32.min(rect.height() / 3.0)),
        );
        painter.rect_filled(
            bar,
            egui::CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            base.gamma_multiply(if selected { 0.95 } else { 0.7 }),
        );
        let outline = if selected {
            Stroke::new(1.5, visuals.strong_text_color())
        } else {
            Stroke::new(1.0, base.gamma_multiply(0.8))
        };
        painter.rect_stroke(rect, 3.0, outline, StrokeKind::Inside);
        let wave_rect = rect.shrink2(vec2(1.0, 14.0_f32.min(rect.height() / 4.0)));
        if let Some(notes) = &clip.notes {
            draw_notes(painter, wave_rect, clip, notes, self.zoom, base);
        } else {
            self.draw_waveform(painter, wave_rect, doc, project, clip, base);
            self.strip_preview(painter, wave_rect, clip);
            self.beat_preview(painter, wave_rect, track.id, clip);
            self.groove_preview(painter, wave_rect, track.id, clip);
        }

        // Fades.
        let fade_color = visuals.strong_text_color().gamma_multiply(0.7);
        let px = |samples: u64| (samples as f64 / self.zoom) as f32;
        let fade = |x0: f32, width: f32, rising: bool, shape: audiostrada_core::FadeShape| {
            let steps = (width / 3.0).clamp(2.0, 48.0) as usize;
            let points: Vec<Pos2> = (0..=steps)
                .map(|i| {
                    let t = i as f32 / steps as f32;
                    let g = shape.gain(if rising { t } else { 1.0 - t });
                    pos2(x0 + t * width, rect.bottom() - g * rect.height())
                })
                .collect();
            // Shade the attenuated part, above the curve, with one quad per step.
            let mut mesh = egui::Mesh::default();
            let shade = visuals.extreme_bg_color.gamma_multiply(0.45);
            for pair in points.windows(2) {
                let i = mesh.vertices.len() as u32;
                for p in [
                    pair[0],
                    pair[1],
                    pos2(pair[1].x, rect.top()),
                    pos2(pair[0].x, rect.top()),
                ] {
                    mesh.colored_vertex(p, shade);
                }
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
            }
            painter.add(Shape::mesh(mesh));
            painter.add(Shape::line(points, Stroke::new(1.0, fade_color)));
        };
        if clip.fade_in > 0 {
            fade(rect.left(), px(clip.fade_in), true, clip.fade_shape);
        }
        if clip.fade_out > 0 {
            let width = px(clip.fade_out);
            fade(rect.right() - width, width, false, clip.out_shape());
        }
        let handle = |x: f32| Rect::from_center_size(pos2(x, rect.top() + 4.0), vec2(6.0, 6.0));
        painter.rect_filled(
            handle(rect.left() + px(clip.fade_in) + 3.0),
            1.0,
            fade_color,
        );
        painter.rect_filled(
            handle(rect.right() - px(clip.fade_out) - 3.0),
            1.0,
            fade_color,
        );

        let mut label = clip.name.clone();
        if (clip.stretch - 1.0).abs() > 1e-6 {
            label.push_str(&format!("  {:.0} %", clip.stretch * 100.0));
        }
        if clip.pitch != 0.0 {
            label.push_str(&format!("  {:+.1} st", clip.pitch));
        }
        if clip.variant().is_some()
            && audiostrada_engine::project::clip_media(project, doc.paths(), clip)
                .is_some_and(|p| doc.media().audio(&p).is_none())
        {
            label.push_str(&format!("  {}", tr("(rendering…)")));
        }
        if clip.muted {
            label.push_str(&format!("  {}", tr("(muted)")));
        }
        if clip.gain_db != 0.0 {
            label.push_str(&format!("  {:+.1} dB", clip.gain_db));
        }
        painter
            .with_clip_rect(rect.intersect(painter.clip_rect()))
            .text(
                pos2(rect.left() + 10.0, rect.top() + 1.0),
                Align2::LEFT_TOP,
                label,
                crate::theme::strong(10.5),
                Color32::from_rgb(20, 20, 24),
            );
    }

    /// Waveform from the peak caches, or from the samples when zoomed in closely.
    /// The alternate playlists of a track as dimmed lanes under it, with the range being comped.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_takes(
        &self,
        painter: &egui::Painter,
        row: Rect,
        lanes: Rect,
        doc: &Document,
        project: &Project,
        track: &Track,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        for (k, playlist) in track.alternates.iter().enumerate() {
            let top = row.bottom() + k as f32 * TAKE_HEIGHT;
            let lane = Rect::from_min_max(
                pos2(lanes.left(), top),
                pos2(lanes.right(), top + TAKE_HEIGHT),
            );
            if lane.bottom() < lanes.top() || lane.top() > lanes.bottom() {
                continue;
            }
            painter.rect_filled(lane.shrink2(vec2(0.0, 0.5)), 0.0, visuals.extreme_bg_color);
            for clip in &playlist.clips {
                let rect = Rect::from_min_max(
                    pos2(scale.x(clip.start), lane.top() + 2.0),
                    pos2(scale.x(clip.end()), lane.bottom() - 2.0),
                );
                if rect.right() < lanes.left() || rect.left() > lanes.right() {
                    continue;
                }
                self.draw_clip(painter, rect, doc, project, track, clip, false, visuals);
                painter.rect_filled(rect, 3.0, visuals.extreme_bg_color.gamma_multiply(0.45));
            }
            if let Some((t, p, start, end)) = self.comping
                && t == track.id
                && p == k
            {
                let r = Rect::from_x_y_ranges(scale.x(start)..=scale.x(end), lane.y_range());
                painter.rect_filled(r, 0.0, visuals.selection.bg_fill.gamma_multiply(0.3));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, visuals.selection.bg_fill),
                    StrokeKind::Inside,
                );
            }
        }
    }

    pub(super) fn draw_waveform(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        doc: &Document,
        project: &Project,
        clip: &audiostrada_core::Clip,
        color: Color32,
    ) {
        let Some(source) = project.sources.get(&clip.source) else {
            return;
        };
        let Some(path) = audiostrada_engine::project::clip_media(project, doc.paths(), clip) else {
            return;
        };
        let ratio = f64::from(source.sample_rate) / f64::from(project.sample_rate);
        let source_per_px = self.zoom * ratio;
        let visible = rect.intersect(painter.clip_rect());
        if visible.width() <= 0.0 {
            return;
        }
        let first_px = (visible.left() - rect.left()).floor().max(0.0) as usize;
        let pixels = visible.width().ceil() as usize + 1;
        let start = clip.offset as f64 * ratio + first_px as f64 * source_per_px;
        let peaks = doc.media().peaks(&path);
        let original = doc.media().original(&path);
        let channels = match (&peaks, &original) {
            (Some(p), _) => p.channels(),
            (None, Some(d)) => d.channels(),
            _ => return,
        };
        // Each channel in its own band (left above right) unless the track is too low.
        let bands = if rect.height() / channels as f32 >= 10.0 {
            channels
        } else {
            1
        };
        let band_height = rect.height() / bands as f32;
        let band = |ch: usize| {
            let b = ch.min(bands - 1) as f32;
            let mid = rect.top() + band_height * (b + 0.5);
            (mid, band_height / 2.0 - 1.0)
        };
        let zoom = self.wave_zoom;
        let clip_px = (clip.length as f64 / self.zoom) as f32;
        let gain_at_x = |x: f32| {
            let rel = ((x - rect.left()) / clip_px.max(1.0)) as f64 * clip.length as f64;
            envelope(clip, rel as u64)
        };
        let y = |mid: f32, half: f32, v: f32| mid - (v * zoom).clamp(-1.0, 1.0) * half;
        let zero = Stroke::new(1.0, color.gamma_multiply(0.25));
        for ch in 0..bands {
            let (mid, _) = band(ch);
            painter.line_segment(
                [pos2(visible.left(), mid), pos2(visible.right(), mid)],
                zero,
            );
            if ch > 0 {
                let edge = rect.top() + band_height * ch as f32;
                painter.line_segment(
                    [pos2(visible.left(), edge), pos2(visible.right(), edge)],
                    Stroke::new(1.0, color.gamma_multiply(0.45)),
                );
            }
        }
        // Zoomed in to the samples: a line through them, and dots when they are far apart.
        if source_per_px < 1.0
            && let Some(data) = &original
        {
            let first = (start.floor() as i64 - 1).max(0) as usize;
            let last =
                ((start + pixels as f64 * source_per_px).ceil() as usize + 1).min(data.frames());
            let x_of = |i: usize| {
                rect.left() + first_px as f32 + ((i as f64 - start) / source_per_px) as f32
            };
            for ch in 0..channels {
                let (mid, half) = band(ch);
                let samples = data.channel(ch);
                let points: Vec<Pos2> = (first..last)
                    .map(|i| {
                        let x = x_of(i);
                        pos2(x, y(mid, half, samples[i] * gain_at_x(x)))
                    })
                    .collect();
                if source_per_px < 0.15 {
                    for p in &points {
                        painter.circle_filled(*p, 1.8, color);
                    }
                }
                painter.add(egui::Shape::line(points, Stroke::new(1.2, color)));
            }
            return;
        }
        for ch in 0..channels {
            // Peaks of this channel (with one band, the channels are merged).
            let mut columns = vec![(f32::MAX, f32::MIN); pixels];
            match (&peaks, &original) {
                (Some(peaks), _) if source_per_px >= f64::from(peaks.finest()) => {
                    peaks.query(ch, start, source_per_px, &mut columns);
                }
                (_, Some(data)) => {
                    let samples = data.channel(ch);
                    for (i, column) in columns.iter_mut().enumerate() {
                        let from = (start + i as f64 * source_per_px) as usize;
                        let to = ((start + (i + 1) as f64 * source_per_px) as usize)
                            .max(from + 1)
                            .min(data.frames());
                        for &s in samples.get(from..to).unwrap_or(&[]) {
                            column.0 = column.0.min(s);
                            column.1 = column.1.max(s);
                        }
                    }
                }
                _ => return,
            }
            let (mid, half) = band(ch);
            let mut previous: Option<(f32, f32)> = None;
            for (i, &(lo, hi)) in columns.iter().enumerate() {
                if lo > hi {
                    previous = None;
                    continue;
                }
                // Each column reaches the previous one, so the waveform has no gaps.
                let (lo, hi) = match previous.replace((lo, hi)) {
                    Some((plo, phi)) => (lo.min(phi), hi.max(plo)),
                    None => (lo, hi),
                };
                let x = rect.left() + (first_px + i) as f32 + 0.5;
                if x > rect.right() {
                    break;
                }
                let gain = gain_at_x(x);
                let top = y(mid, half, hi * gain);
                let bottom = y(mid, half, lo * gain);
                painter.line_segment(
                    [pos2(x, top), pos2(x, bottom.max(top + 0.5))],
                    Stroke::new(1.0, color),
                );
            }
        }
    }
}

/// Curve and points of an automation lane.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_lane(
    painter: &egui::Painter,
    rect: Rect,
    lane: &audiostrada_core::automation::AutomationLane,
    min: f32,
    max: f32,
    scale: Scale,
    color: Color32,
    visuals: &egui::Visuals,
) {
    painter.rect_filled(rect.shrink2(vec2(0.0, 0.5)), 0.0, visuals.extreme_bg_color);
    if lane.points.is_empty() {
        painter.text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            tr("Click to add points, or write them during playback"),
            FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
        return;
    }
    let visible = rect.intersect(painter.clip_rect());
    let mut line = Vec::new();
    let mut x = visible.left();
    while x <= visible.right() + 2.0 {
        if let Some(v) = audiostrada_core::automation::value_at(&lane.points, scale.sample(x)) {
            line.push(pos2(x, value_y(rect, v, min, max)));
        }
        x += 2.0;
    }
    painter.add(Shape::line(line, Stroke::new(1.5, color)));
    for p in &lane.points {
        let px = scale.x(p.time);
        if px >= visible.left() - 4.0 && px <= visible.right() + 4.0 {
            let center = pos2(px, value_y(rect, p.value, min, max));
            painter.rect_filled(Rect::from_center_size(center, vec2(7.0, 7.0)), 1.0, color);
        }
    }
}

/// Miniature of a MIDI clip's notes.
pub(super) fn draw_notes(
    painter: &egui::Painter,
    rect: Rect,
    clip: &audiostrada_core::Clip,
    notes: &[audiostrada_core::Note],
    zoom: f64,
    color: Color32,
) {
    let visible: Vec<&audiostrada_core::Note> = notes
        .iter()
        .filter(|n| n.start + n.length > clip.offset && n.start < clip.offset + clip.length)
        .collect();
    let (Some(low), Some(high)) = (
        visible.iter().map(|n| n.key).min(),
        visible.iter().map(|n| n.key).max(),
    ) else {
        return;
    };
    let span = f32::from(high - low).max(11.0) + 1.0;
    let row = rect.height() / span;
    let clip_rect = painter.clip_rect().intersect(rect);
    let painter = painter.with_clip_rect(clip_rect);
    for n in visible {
        let x0 = rect.left() + ((n.start as f64 - clip.offset as f64) / zoom) as f32;
        let x1 = x0 + ((n.length as f64 / zoom) as f32).max(1.5);
        let y = rect.bottom() - (f32::from(n.key - low) + 1.0) * row;
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, y), pos2(x1, y + row.max(1.5))),
            1.0,
            color,
        );
    }
}

/// Visual gain of a clip at `pos` samples into it.
pub(super) fn envelope(clip: &audiostrada_core::Clip, pos: u64) -> f32 {
    let mut gain = 10f32.powf(clip.gain_db / 20.0);
    if pos < clip.fade_in {
        gain *= clip.fade_shape.gain(pos as f32 / clip.fade_in as f32);
    }
    let to_end = clip.length.saturating_sub(pos);
    if to_end < clip.fade_out {
        gain *= clip.out_shape().gain(to_end as f32 / clip.fade_out as f32);
    }
    gain
}
