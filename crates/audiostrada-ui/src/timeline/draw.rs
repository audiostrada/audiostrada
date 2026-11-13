//! Drawing the ruler, the grid, clips with their waveforms and fades, takes and lanes.

use super::*;
use audiostrada_media::Peak;
use serde::{Deserialize, Serialize};

/// Decibels shown by the logarithmic scale, from its line to its edge.
const LOG_RANGE: f32 = 48.0;
/// A sample this loud is drawn as clipping.
const CLIP_LEVEL: f32 = 0.999;
/// Colors of the lows, mids and highs in the spectral view.
const SPECTRAL: [[u8; 3]; 3] = [[238, 92, 72], [118, 214, 112], [84, 168, 255]];

/// How the waveforms are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveScale {
    #[default]
    Linear,
    /// In decibels: quiet details show.
    Log,
    /// The level only, from the bottom, twice as tall.
    Rectified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveView {
    pub scale: WaveScale,
    /// Colored by where the energy is: lows red, mids green, highs blue.
    pub spectral: bool,
    /// The channels of a clip as one waveform.
    pub merged: bool,
    /// Each clip as tall as its loudest peak allows (the sound does not change).
    #[serde(default)]
    pub fit: bool,
}

/// Bars between two numbered (or emphasized) bars: 1, 2, 4, 8... so that they are at least
/// `room` pixels apart.
pub(super) fn bars_apart(bar_px: f64, room: f64) -> u64 {
    let mut every = 1u64;
    while bar_px * (every as f64) < room && every < 4096 {
        every *= 2;
    }
    every
}

/// `a` towards `b` by `t`.
pub(super) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgba_unmultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), a.a())
}

/// One column of a filled waveform, joined to the previous one when `join`.
fn strip(
    mesh: &mut egui::Mesh,
    join: bool,
    x: f32,
    (top, bottom): (f32, f32),
    (color, foot): (Color32, Color32),
) {
    if !join {
        // A lone column still shows: a pixel-wide quad.
        let i = mesh.vertices.len() as u32;
        for (p, c) in [
            (pos2(x - 0.5, top), color),
            (pos2(x - 0.5, bottom), foot),
            (pos2(x + 0.5, top), color),
            (pos2(x + 0.5, bottom), foot),
        ] {
            mesh.colored_vertex(p, c);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
        return;
    }
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(pos2(x, top), color);
    mesh.colored_vertex(pos2(x, bottom), foot);
    // The previous column's two last vertices.
    mesh.add_triangle(i - 2, i - 1, i);
    mesh.add_triangle(i - 1, i + 1, i);
}

impl TimelineView {
    pub(super) fn draw_ruler(
        &self,
        painter: &egui::Painter,
        ruler: Rect,
        project: &Project,
        scale: Scale,
        visuals: &egui::Visuals,
    ) {
        let lift = if visuals.dark_mode {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        crate::theme::gradient_rect(
            painter,
            ruler,
            visuals.panel_fill,
            mix(visuals.panel_fill, lift, 0.05),
        );
        painter.hline(
            ruler.x_range(),
            ruler.bottom() - 0.5,
            Stroke::new(
                1.0,
                Color32::from_black_alpha(if visuals.dark_mode { 140 } else { 50 }),
            ),
        );
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
        // Bar numbers 1, 2, 4, 8... bars apart, as many as fit.
        let every = bars_apart(beat_px * per_bar, 44.0);
        let weak = visuals.weak_text_color();
        let mut beat = first_beat;
        let mut count = 0;
        while beat <= last_beat && count < 4000 {
            count += 1;
            let x = scale.x(tempo.beat_to_sample(beat, rate));
            let bbt = tempo.beat_to_bbt(beat);
            let on_bar = bbt.beat == 1 && bbt.tick == 0;
            let bar = u64::from(bbt.bar.max(1) - 1);
            let numbered = on_bar && bar % every == 0;
            let (h, ink) = if numbered {
                (ruler.height() - 4.0, weak.gamma_multiply(0.9))
            } else if on_bar {
                (9.0, weak.gamma_multiply(0.6))
            } else {
                (5.0, weak.gamma_multiply(0.45))
            };
            if on_bar || beat_px > 6.0 {
                painter.vline(
                    x,
                    (ruler.bottom() - h)..=ruler.bottom(),
                    Stroke::new(1.0, ink),
                );
            }
            if numbered {
                painter.text(
                    pos2(x + 4.0, ruler.top() + 1.0),
                    Align2::LEFT_TOP,
                    bbt.bar.to_string(),
                    crate::theme::strong(11.0),
                    visuals.text_color(),
                );
            } else if !on_bar && beat_px > 34.0 {
                // The beats, when they have room: "5.2".
                painter.text(
                    pos2(x + 3.0, ruler.top() + 3.0),
                    Align2::LEFT_TOP,
                    format!("{}.{}", bbt.bar, bbt.beat),
                    FontId::proportional(9.0),
                    weak.gamma_multiply(0.75),
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
            let color = Color32::from_rgb(240, 178, 48);
            // A stem down to the ruler's foot, and the name on a tab of the marker's color.
            painter.vline(
                x,
                (ruler.top() + 15.0)..=ruler.bottom(),
                Stroke::new(1.5, color),
            );
            let galley = painter.layout_no_wrap(
                marker.name.clone(),
                FontId::proportional(10.0),
                Color32::from_rgb(40, 26, 0),
            );
            let tab = Rect::from_min_size(
                pos2(x, ruler.top() + 15.0),
                vec2(galley.size().x + 12.0, 13.0),
            );
            painter.rect_filled(
                tab,
                egui::CornerRadius {
                    nw: 0,
                    ne: 4,
                    sw: 0,
                    se: 4,
                },
                color,
            );
            painter.galley(
                pos2(tab.left() + 6.0, tab.center().y - galley.size().y / 2.0),
                galley,
                Color32::from_rgb(40, 26, 0),
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
        let per_bar = tempo.signature_at_beat(first).beats_per_bar();
        let every = bars_apart(beat_px * per_bar, 60.0);
        let ink = visuals.text_color();
        let x_of = |beat: f64| scale.x(tempo.beat_to_sample(beat, rate));
        let mut beat = first;
        let mut count = 0;
        while beat <= last && count < 4000 {
            count += 1;
            let bbt = tempo.beat_to_bbt(beat);
            let on_bar = bbt.beat == 1 && bbt.tick == 0;
            let x = x_of(beat);
            if on_bar {
                let bar = u64::from(bbt.bar.max(1) - 1);
                let length = tempo.signature_at_beat(beat).beats_per_bar();
                // Every other group of bars a shade apart, to keep one's place.
                if (bar / every) % 2 == 1 && bar % every == 0 {
                    let end = x_of(beat + length * every as f64);
                    painter.rect_filled(
                        Rect::from_x_y_ranges(x..=end, lanes.y_range()),
                        0.0,
                        ink.gamma_multiply(0.03),
                    );
                }
                if bar % every == 0 {
                    painter.vline(
                        x,
                        lanes.y_range(),
                        Stroke::new(1.0, ink.gamma_multiply(0.13)),
                    );
                } else if beat_px * length > 8.0 {
                    painter.vline(
                        x,
                        lanes.y_range(),
                        Stroke::new(1.0, ink.gamma_multiply(0.06)),
                    );
                }
            } else if beat_px > 14.0 {
                painter.vline(
                    x,
                    lanes.y_range(),
                    Stroke::new(1.0, ink.gamma_multiply(0.045)),
                );
            }
            // Sixteenths, faintly, when there is room for them.
            if beat_px > 64.0 {
                for k in 1..4 {
                    let xs = x_of(beat + f64::from(k) * 0.25);
                    painter.vline(
                        xs,
                        lanes.y_range(),
                        Stroke::new(1.0, ink.gamma_multiply(0.02)),
                    );
                }
            }
            beat += 1.0;
        }
    }

    /// A drum pattern's bars: the variation each plays, as in the drum machine's chain.
    fn draw_variations(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        project: &Project,
        clip: &audiostrada_core::Clip,
    ) {
        let Some((grid, chain)) = crate::drum_sequencer::clip_chain(project, clip) else {
            return;
        };
        let bar = (grid.step * 16.0 / self.zoom) as f32;
        if bar < 6.0 {
            return;
        }
        for (b, &v) in chain.iter().enumerate() {
            let left = rect.left() + b as f32 * bar;
            if left > rect.right() {
                break;
            }
            let band = Rect::from_min_max(
                pos2(left + 1.0, rect.top()),
                pos2((left + bar - 1.0).min(rect.right()), rect.top() + 4.0),
            );
            let color = crate::drum_sequencer::variation_color(v);
            painter.rect_filled(band, 2.0, color);
            if bar >= 26.0 && rect.height() >= 30.0 {
                let galley = painter.layout_no_wrap(
                    crate::drum_sequencer::variation_name(v),
                    egui::FontId::proportional(9.5),
                    Color32::WHITE,
                );
                let tag = Rect::from_min_size(
                    pos2(left + 3.0, band.bottom() + 2.0),
                    galley.size() + vec2(8.0, 2.0),
                );
                painter.rect_filled(tag, 3.0, color.gamma_multiply(0.85));
                painter.galley(tag.min + vec2(4.0, 1.0), galley, Color32::WHITE);
            }
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
        let mut base = crate::theme::track_color(track.color);
        if clip.muted {
            base = visuals.weak_text_color();
        }
        let dark = visuals.dark_mode;
        let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
        // Lifted off the lane by a soft shadow.
        painter.rect_filled(
            rect.translate(vec2(0.0, 1.5)),
            5.0,
            Color32::from_black_alpha(if dark { 110 } else { 40 }),
        );
        // A body in the track's color for the waveform to stand out, lit from above, lighter
        // when selected.
        let strength = match (dark, selected) {
            (true, false) => 0.30,
            (true, true) => 0.42,
            (false, false) => 0.3,
            (false, true) => 0.45,
        };
        let body = mix(visuals.extreme_bg_color, base, strength);
        painter.rect_filled(rect, 5.0, body);
        crate::gear::sheen(
            &clipped,
            rect.shrink2(vec2(1.0, 0.0)),
            Color32::from_white_alpha(if dark { 14 } else { 30 }),
            Color32::from_black_alpha(if dark { 70 } else { 6 }),
        );
        // The name band in the track color, its top edge catching the light.
        let name_height = NAME_BAR.min(rect.height() / 3.0);
        let bar = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + name_height));
        let band = if selected {
            mix(base, Color32::WHITE, 0.12)
        } else {
            base.gamma_multiply(0.82)
        };
        painter.rect_filled(
            bar,
            egui::CornerRadius {
                nw: 5,
                ne: 5,
                sw: 0,
                se: 0,
            },
            band,
        );
        crate::gear::sheen(
            &clipped,
            bar.shrink2(vec2(1.0, 0.0)),
            Color32::from_white_alpha(46),
            Color32::TRANSPARENT,
        );
        painter.hline(
            bar.x_range(),
            bar.bottom(),
            Stroke::new(1.0, Color32::from_black_alpha(if dark { 70 } else { 30 })),
        );
        let outline = if selected {
            Stroke::new(1.5, mix(base, Color32::WHITE, 0.75))
        } else {
            Stroke::new(
                1.0,
                mix(base, Color32::BLACK, if dark { 0.35 } else { 0.2 }),
            )
        };
        painter.rect_stroke(rect, 5.0, outline, StrokeKind::Inside);
        if selected {
            painter.rect_stroke(
                rect.expand(1.5),
                6.5,
                Stroke::new(2.0, base.gamma_multiply(0.45)),
                StrokeKind::Inside,
            );
        }
        if clip.muted {
            // Hatched across.
            let hatch = Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.35));
            let mut x = rect.left() - rect.height();
            while x < rect.right() {
                clipped.line_segment(
                    [pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())],
                    hatch,
                );
                x += 10.0;
            }
        }
        let wave_rect = Rect::from_min_max(
            pos2(rect.left() + 1.0, bar.bottom() + 1.0),
            pos2(rect.right() - 1.0, rect.bottom() - 2.0),
        );
        if let Some(notes) = &clip.notes {
            draw_notes(painter, wave_rect, clip, notes, self.zoom, base);
            self.draw_variations(&clipped, wave_rect, project, clip);
        } else {
            // Light over the dark body; on a light theme the track color is too pale: darker.
            let ink = if visuals.dark_mode {
                mix(base, Color32::WHITE, 0.35)
            } else {
                mix(base, Color32::BLACK, 0.5)
            };
            self.draw_waveform(painter, wave_rect, doc, project, clip, ink);
            self.strip_preview(painter, wave_rect, clip);
            self.beat_preview(painter, wave_rect, track.id, clip);
            self.groove_preview(painter, wave_rect, track.id, clip);
        }

        let px = |samples: u64| (samples as f64 / self.zoom) as f32;
        // Where the audio dropped out while it was recorded.
        if let Some(take) = &clip.take {
            for &at in &take.dropouts {
                if at >= clip.offset && at < clip.offset + clip.length {
                    let x = rect.left() + px(at - clip.offset);
                    painter.vline(
                        x,
                        wave_rect.y_range(),
                        Stroke::new(2.0, Color32::from_rgb(255, 64, 64)),
                    );
                }
            }
        }

        // Fades.
        let fade_color = if visuals.dark_mode {
            Color32::from_white_alpha(200)
        } else {
            Color32::from_black_alpha(170)
        };
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
            painter.add(Shape::line(points, Stroke::new(1.4, fade_color)));
        };
        if clip.fade_in > 0 {
            fade(rect.left(), px(clip.fade_in), true, clip.fade_shape);
        }
        if clip.fade_out > 0 {
            let width = px(clip.fade_out);
            fade(rect.right() - width, width, false, clip.out_shape());
        }
        // The fade handles, small dots at the top corners.
        for x in [
            rect.left() + px(clip.fade_in) + 4.0,
            rect.right() - px(clip.fade_out) - 4.0,
        ] {
            let at = pos2(x, rect.top() + 4.5);
            clipped.circle_filled(at, 3.0, Color32::from_black_alpha(90));
            clipped.circle_filled(at, 2.2, fade_color);
        }

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
        if let Some(take) = &clip.take
            && take.rating > 0
        {
            label.push_str("  ");
            label.push_str(&"★".repeat(usize::from(take.rating)));
        }
        let at = pos2(
            fx_button(rect, clip).map_or(rect.left() + 10.0, |b| b.right() + 5.0),
            rect.top() + 1.0,
        );
        let ink = crate::theme::text_on(band);
        // Pressed into the band: a faint shadow under light letters.
        if ink == Color32::WHITE {
            clipped.text(
                at + vec2(0.0, 1.0),
                Align2::LEFT_TOP,
                &label,
                crate::theme::strong(10.5),
                Color32::from_black_alpha(110),
            );
        }
        clipped.text(at, Align2::LEFT_TOP, label, crate::theme::strong(10.5), ink);
        // Its effects button: lit when it has effects of its own.
        if let Some(button) = fx_button(rect, clip) {
            let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            let ink = crate::theme::text_on(band);
            let text = if clip.inserts.is_empty() {
                painter.rect_stroke(button, 2.0, Stroke::new(1.0, ink), StrokeKind::Inside);
                ink
            } else {
                painter.rect_filled(button, 2.0, ink);
                band
            };
            painter.text(
                button.center(),
                Align2::CENTER_CENTER,
                "FX",
                crate::theme::strong(8.5),
                text,
            );
        }
    }

    /// The take lanes of a track under it: each take whole, lit in its color where the comp
    /// plays it and dimmed elsewhere, with the range being comped.
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
        let used = edit::comp_sources(track);
        for (k, playlist) in track.takes.iter().enumerate() {
            let top = row.bottom() + k as f32 * TAKE_HEIGHT;
            let lane = Rect::from_min_max(
                pos2(lanes.left(), top),
                pos2(lanes.right(), top + TAKE_HEIGHT),
            );
            if lane.bottom() < lanes.top() || lane.top() > lanes.bottom() {
                continue;
            }
            painter.rect_filled(lane.shrink2(vec2(0.0, 0.5)), 0.0, visuals.extreme_bg_color);
            let color = take_color(track, k);
            let ranges = used.get(k).map_or(&[][..], Vec::as_slice);
            for clip in &playlist.clips {
                let rect = Rect::from_min_max(
                    pos2(scale.x(clip.start), lane.top() + 2.0),
                    pos2(scale.x(clip.end()), lane.bottom() - 2.0),
                );
                if rect.right() < lanes.left() || rect.left() > lanes.right() {
                    continue;
                }
                self.draw_clip(painter, rect, doc, project, track, clip, false, visuals);
                let span =
                    |a: u64, b: u64| Rect::from_x_y_ranges(scale.x(a)..=scale.x(b), rect.y_range());
                let dim = visuals.extreme_bg_color.gamma_multiply(0.6);
                // Lit where the comp plays this take, dimmed elsewhere.
                let mut x = clip.start;
                for &(a, b) in ranges
                    .iter()
                    .filter(|(a, b)| *a < clip.end() && *b > clip.start)
                {
                    let (a, b) = (a.max(clip.start), b.min(clip.end()));
                    if a > x {
                        painter.rect_filled(span(x, a), 0.0, dim);
                    }
                    let lit = span(a, b);
                    painter.rect_stroke(lit, 2.0, Stroke::new(1.5, color), StrokeKind::Inside);
                    painter.rect_filled(
                        Rect::from_min_max(lit.left_top(), pos2(lit.right(), lit.top() + 3.0)),
                        0.0,
                        color,
                    );
                    x = x.max(b);
                }
                if x < clip.end() {
                    painter.rect_filled(span(x, clip.end()), 0.0, dim);
                }
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

    /// Waveform from the peak caches, or from the samples when zoomed in closely.
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
        let view = self.wave_view;
        // Each channel in its own band (left above right) unless merged or the track is low.
        let bands = if !view.merged && rect.height() / channels as f32 >= 24.0 {
            channels
        } else {
            1
        };
        let band_height = rect.height() / bands as f32;
        let rectified = view.scale == WaveScale::Rectified;
        // The line the band's waveform grows from, and how far it may go.
        let band = |ch: usize| {
            let b = ch.min(bands - 1) as f32;
            let top = rect.top() + band_height * b;
            if rectified {
                (top + band_height - 1.0, band_height - 2.0)
            } else {
                (top + band_height / 2.0, band_height / 2.0 - 1.0)
            }
        };
        let mut zoom = self.wave_zoom;
        if view.fit
            && let Some(peaks) = &peaks
        {
            let frames = (clip.length as f64 * ratio).max(1.0);
            let mut whole = [Peak::EMPTY];
            let loudest = (0..channels)
                .map(|c| {
                    peaks.query(c, clip.offset as f64 * ratio, frames, &mut whole);
                    whole[0].hi.max(-whole[0].lo)
                })
                .fold(0.0f32, f32::max);
            if loudest > 1e-4 {
                zoom = (zoom * 0.95 / loudest).min(MAX_WAVE_ZOOM);
            }
        }
        let clip_px = (clip.length as f64 / self.zoom) as f32;
        let gain_at_x = |x: f32| {
            let rel = ((x - rect.left()) / clip_px.max(1.0)) as f64 * clip.length as f64;
            envelope(clip, rel as u64)
        };
        // A level (signed) to a height from the line, -1..=1, linear or in decibels.
        let shape = |v: f32| -> f32 {
            let v = v * zoom;
            match view.scale {
                WaveScale::Log => {
                    let a = v.abs();
                    if a < 1e-6 {
                        0.0
                    } else {
                        (1.0 + 20.0 * a.log10() / LOG_RANGE)
                            .clamp(0.0, 1.0)
                            .copysign(v)
                    }
                }
                _ => v.clamp(-1.0, 1.0),
            }
        };
        let line = Stroke::new(1.0, color.gamma_multiply(0.12));
        for ch in 0..bands {
            let (base, _) = band(ch);
            painter.hline(visible.x_range(), base, line);
            if ch > 0 {
                let edge = rect.top() + band_height * ch as f32;
                painter.hline(
                    visible.x_range(),
                    edge,
                    Stroke::new(1.0, color.gamma_multiply(0.18)),
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
            let bright = mix(color, Color32::WHITE, 0.35);
            for ch in 0..channels {
                let b = ch.min(bands - 1) as f32;
                let mid = rect.top() + band_height * (b + 0.5);
                let half = band_height / 2.0 - 1.0;
                let samples = data.channel(ch);
                let points: Vec<Pos2> = (first..last)
                    .map(|i| {
                        let x = x_of(i);
                        let v = (samples[i] * gain_at_x(x) * zoom).clamp(-1.0, 1.0);
                        pos2(x, mid - v * half)
                    })
                    .collect();
                if source_per_px < 0.15 {
                    for p in &points {
                        painter.circle_filled(*p, 1.8, bright);
                    }
                }
                painter.add(egui::Shape::line(points, Stroke::new(1.2, bright)));
            }
            return;
        }
        // On a light theme the wave is dark ink: opaque peaks, a darker body.
        let light = !painter.ctx().global_style().visuals.dark_mode;
        let peak_color = |p: &Peak| -> Color32 {
            let s = p.spectrum();
            if view.spectral && s.iter().sum::<f32>() > 0.0 {
                let mut c = [0.0f32; 3];
                for (share, band) in s.iter().zip(SPECTRAL) {
                    for (k, v) in c.iter_mut().enumerate() {
                        *v += share * f32::from(band[k]);
                    }
                }
                Color32::from_rgb(c[0] as u8, c[1] as u8, c[2] as u8)
            } else if light {
                color
            } else {
                mix(color, Color32::WHITE, 0.2)
            }
        };
        // Lit from its line outwards: two halves, bright at the line, fading to the peaks.
        let mut outer = egui::Mesh::default();
        let mut lower = egui::Mesh::default();
        let mut inner = egui::Mesh::default();
        let mut clipped: Vec<(f32, f32)> = Vec::new();
        for ch in 0..bands {
            // The band's peaks; one band for several channels merges them.
            let mut columns = vec![Peak::EMPTY; pixels];
            let merged: Vec<usize> = if bands == 1 {
                (0..channels).collect()
            } else {
                vec![ch]
            };
            for (n, &c) in merged.iter().enumerate() {
                let mut these = vec![Peak::EMPTY; pixels];
                match (&peaks, &original) {
                    (Some(peaks), _) if source_per_px >= f64::from(peaks.finest()) => {
                        peaks.query(c, start, source_per_px, &mut these);
                    }
                    (_, Some(data)) => {
                        let samples = data.channel(c);
                        for (i, column) in these.iter_mut().enumerate() {
                            let from = (start + i as f64 * source_per_px) as usize;
                            let to = ((start + (i + 1) as f64 * source_per_px) as usize)
                                .max(from + 1)
                                .min(data.frames());
                            let slice = samples.get(from..to).unwrap_or(&[]);
                            for &s in slice {
                                column.lo = column.lo.min(s);
                                column.hi = column.hi.max(s);
                                column.mean_square += s * s;
                            }
                            column.mean_square /= slice.len().max(1) as f32;
                        }
                    }
                    _ => return,
                }
                if n == 0 {
                    columns = these;
                } else {
                    for (a, b) in columns.iter_mut().zip(&these) {
                        a.lo = a.lo.min(b.lo);
                        a.hi = a.hi.max(b.hi);
                        a.mean_square = a.mean_square.max(b.mean_square);
                    }
                }
            }
            let (base, reach) = band(ch);
            let mut previous: Option<usize> = None;
            for (i, p) in columns.iter().enumerate() {
                let x = rect.left() + (first_px + i) as f32 + 0.5;
                if p.is_empty() || x > rect.right() {
                    previous = None;
                    continue;
                }
                let gain = gain_at_x(x);
                let (top, bottom, rms_top, rms_bottom) = if rectified {
                    let peak = shape(p.lo.abs().max(p.hi.abs()) * gain);
                    let rms = shape(p.rms() * gain).min(peak);
                    (base - peak * reach, base, base - rms * reach, base)
                } else {
                    let (hi, lo) = (shape(p.hi * gain), shape(p.lo * gain));
                    let rms = shape(p.rms() * gain).min(hi.max(0.0)).min(-lo.min(0.0));
                    (
                        base - hi * reach,
                        base - lo * reach,
                        base - rms * reach,
                        base + rms * reach,
                    )
                };
                // Silence stays a faint line, not a bright one.
                let quiet = bottom - top < 1.0;
                let bottom = bottom.max(top + 1.0);
                let tint = peak_color(p);
                let (soft, strong) = if light {
                    (tint.gamma_multiply(0.85), mix(tint, Color32::BLACK, 0.4))
                } else {
                    (tint.gamma_multiply(0.55), mix(tint, Color32::WHITE, 0.25))
                };
                let (soft, strong) = if quiet {
                    (soft.gamma_multiply(0.35), Color32::TRANSPARENT)
                } else {
                    (soft, strong)
                };
                let core = if quiet {
                    soft
                } else if light {
                    mix(tint, Color32::BLACK, 0.25)
                } else {
                    mix(tint, Color32::WHITE, 0.3)
                };
                let middle = base.clamp(top, bottom);
                let join = previous.is_some();
                strip(&mut outer, join, x, (top, middle), (soft, core));
                strip(&mut lower, join, x, (middle, bottom), (core, soft));
                strip(
                    &mut inner,
                    join,
                    x,
                    (rms_top, rms_bottom.max(rms_top + 0.5)),
                    (strong.gamma_multiply(0.45), strong.gamma_multiply(0.45)),
                );
                previous = Some(i);
                if p.hi >= CLIP_LEVEL || p.lo <= -CLIP_LEVEL {
                    clipped.push((x, if rectified { top } else { base - reach }));
                }
            }
        }
        painter.add(Shape::mesh(outer));
        painter.add(Shape::mesh(lower));
        painter.add(Shape::mesh(inner));
        for (x, y) in clipped {
            painter.rect_filled(
                Rect::from_min_size(pos2(x - 0.5, y), vec2(1.5, 3.0)),
                0.0,
                Color32::from_rgb(255, 64, 64),
            );
        }
    }
}

/// An automation lane under its track: its background, its scale, its curve.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_lane(
    painter: &egui::Painter,
    rect: Rect,
    lane: &audiostrada_core::automation::AutomationLane,
    info: &super::headers::LaneInfo,
    scale: Scale,
    color: Color32,
    visuals: &egui::Visuals,
) {
    painter.rect_filled(rect.shrink2(vec2(0.0, 0.5)), 0.0, visuals.extreme_bg_color);
    draw_scale(painter, rect, info, visuals);
    if lane.points.is_empty() {
        painter.text(
            pos2(rect.left() + 70.0, rect.center().y),
            Align2::LEFT_CENTER,
            tr("Click to draw this automation, or write it during playback"),
            FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
        return;
    }
    draw_curve(painter, rect, lane, info, scale, color);
}

/// The values a lane's scale names, a few across its range.
fn scale_marks(info: &super::headers::LaneInfo) -> Vec<f32> {
    use super::headers::Reading;
    match info.reading {
        Reading::Db => vec![12.0, 6.0, 0.0, -6.0, -12.0, -24.0, -60.0],
        Reading::Pan => vec![-1.0, 0.0, 1.0],
        Reading::Toggle => vec![0.0, 1.0],
        _ => [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|f| info.at_share(*f))
            .collect(),
    }
}

/// A lane's scale: faint lines named at the left, and the value set on the control, dashed.
pub(super) fn draw_scale(
    painter: &egui::Painter,
    rect: Rect,
    info: &super::headers::LaneInfo,
    visuals: &egui::Visuals,
) {
    let visible = rect.intersect(painter.clip_rect());
    let weak = visuals.weak_text_color();
    let small = FontId::proportional(9.0);
    let mut last = f32::NEG_INFINITY;
    for value in scale_marks(info) {
        let y = value_y(rect, value, info);
        painter.hline(
            visible.x_range(),
            y,
            Stroke::new(1.0, weak.gamma_multiply(0.13)),
        );
        // Named where the names do not run into each other.
        if (y - last).abs() >= 12.0 {
            painter.text(
                pos2(visible.left() + 4.0, y),
                Align2::LEFT_CENTER,
                info.text(value),
                small.clone(),
                weak.gamma_multiply(0.75),
            );
            last = y;
        }
    }
    // The value set on the control: the line the automation moves away from.
    if let Some(base) = info.base {
        let y = value_y(rect, base, info);
        painter.extend(Shape::dashed_line(
            &[pos2(visible.left() + 52.0, y), pos2(visible.right(), y)],
            Stroke::new(1.0, weak.gamma_multiply(0.8)),
            5.0,
            4.0,
        ));
        if let Some(set) = info.base_label() {
            let text = crate::trf!("Set: {value}", value = set);
            let galley = painter.layout_no_wrap(text, small, weak);
            let at = pos2(
                visible.right() - galley.size().x - 10.0,
                (y - galley.size().y - 2.0).max(rect.top() + 1.0),
            );
            painter.rect_filled(
                Rect::from_min_size(at - vec2(4.0, 1.0), galley.size() + vec2(8.0, 2.0)),
                3.0,
                visuals.extreme_bg_color,
            );
            painter.galley(at, galley, weak);
        }
    }
}

/// A lane's curve: lit underneath, fading down, with its points as beads.
pub(super) fn draw_curve(
    painter: &egui::Painter,
    rect: Rect,
    lane: &audiostrada_core::automation::AutomationLane,
    info: &super::headers::LaneInfo,
    scale: Scale,
    color: Color32,
) {
    let visible = rect.intersect(painter.clip_rect());
    let mut line = Vec::new();
    let mut x = visible.left();
    while x <= visible.right() + 2.0 {
        if let Some(v) = audiostrada_core::automation::value_at(&lane.points, scale.sample(x)) {
            line.push(pos2(x, value_y(rect, v, info)));
        }
        x += 2.0;
    }
    // The area under the curve, in its color fading to nothing at the foot of the lane.
    let foot = rect.bottom();
    let mut mesh = egui::Mesh::default();
    for p in &line {
        let depth = ((foot - p.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
        mesh.colored_vertex(*p, color.gamma_multiply(0.10 + 0.22 * depth));
        mesh.colored_vertex(pos2(p.x, foot), color.gamma_multiply(0.02));
    }
    for k in 0..line.len().saturating_sub(1) as u32 {
        let i = k * 2;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(
        line.clone(),
        Stroke::new(4.5, color.gamma_multiply(0.18)),
    ));
    painter.add(Shape::line(line, Stroke::new(1.8, color)));
    for p in &lane.points {
        let px = scale.x(p.time);
        if px >= visible.left() - 5.0 && px <= visible.right() + 5.0 {
            let center = pos2(px, value_y(rect, p.value, info));
            painter.circle_filled(center, 4.5, Color32::from_black_alpha(140));
            painter.circle_filled(center, 3.6, color);
            painter.circle_filled(center, 1.5, Color32::WHITE);
        }
    }
}

/// The effects button of an audio clip, before its name, when the clip has room for it.
pub(super) fn fx_button(rect: Rect, clip: &audiostrada_core::Clip) -> Option<Rect> {
    (!clip.is_midi() && rect.width() >= 60.0 && rect.height() >= 18.0)
        .then(|| Rect::from_min_size(pos2(rect.left() + 12.0, rect.top() + 2.0), vec2(20.0, 11.0)))
}

/// A value shown by the pointer, above everything.
pub(super) fn value_tag(ctx: &egui::Context, at: Pos2, text: String, accent: Color32) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("lane-value"),
    ));
    let style = ctx.global_style();
    let galley =
        painter.layout_no_wrap(text, FontId::proportional(11.5), style.visuals.text_color());
    let tag = Rect::from_min_size(at + vec2(14.0, -26.0), galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(tag, 4.0, style.visuals.window_fill);
    painter.rect_stroke(tag, 4.0, Stroke::new(1.0, accent), egui::StrokeKind::Inside);
    painter.galley(tag.min + vec2(6.0, 3.0), galley, style.visuals.text_color());
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

/// The handles on a clip's top edge: its volume in the middle, the middle of its fades.
pub(super) fn draw_handles(
    painter: &egui::Painter,
    rect: Rect,
    clip: &audiostrada_core::Clip,
    zoom: f64,
    hovered: bool,
) {
    let color = if hovered {
        Color32::WHITE
    } else {
        Color32::from_white_alpha(170)
    };
    if rect.width() >= 48.0 && !clip.is_midi() {
        let handle =
            Rect::from_center_size(pos2(rect.center().x, rect.top() + 4.0), vec2(16.0, 5.0));
        painter.rect_filled(handle, 2.5, color);
        painter.rect_stroke(
            handle,
            2.5,
            Stroke::new(1.0, Color32::from_black_alpha(140)),
            StrokeKind::Outside,
        );
    }
    for fade_in in [true, false] {
        if let Some(middle) = super::gestures::fade_middle(rect, clip, fade_in, zoom) {
            painter.circle_filled(middle, 3.5, color);
            painter.circle_stroke(
                middle,
                3.5,
                Stroke::new(1.0, Color32::from_black_alpha(140)),
            );
        }
    }
}

/// Where two clips of a track overlap: the overlap shaded, the fade-out of the one and the
/// fade-in of the other crossing as they are shaped, and the length handle on top.
pub(super) fn draw_crossfades(painter: &egui::Painter, row: Rect, track: &Track, scale: Scale) {
    for pair in track.clips.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.end() <= b.start {
            continue;
        }
        let r = Rect::from_x_y_ranges(
            scale.x(b.start)..=scale.x(a.end()),
            row.top() + 2.0..=row.bottom() - 2.0,
        );
        painter.rect_filled(r, 0.0, Color32::from_black_alpha(50));
        let curve = |shape: audiostrada_core::model::FadeShape, rising: bool| -> Vec<Pos2> {
            (0..=32)
                .map(|i| {
                    let t = i as f32 / 32.0;
                    let gain = shape.gain(if rising { t } else { 1.0 - t });
                    pos2(r.left() + r.width() * t, r.bottom() - r.height() * gain)
                })
                .collect()
        };
        painter.add(Shape::line(
            curve(a.out_shape(), false),
            Stroke::new(1.5, Color32::from_rgb(240, 170, 90)),
        ));
        painter.add(Shape::line(
            curve(b.fade_shape, true),
            Stroke::new(1.5, Color32::from_rgb(120, 200, 240)),
        ));
        painter.vline(
            r.left(),
            r.y_range(),
            Stroke::new(1.0, Color32::from_white_alpha(60)),
        );
        painter.vline(
            r.right(),
            r.y_range(),
            Stroke::new(1.0, Color32::from_white_alpha(60)),
        );
        let c = pos2(r.center().x, r.top() + 5.0);
        painter.add(Shape::convex_polygon(
            vec![
                c + vec2(0.0, -4.5),
                c + vec2(4.5, 0.0),
                c + vec2(0.0, 4.5),
                c + vec2(-4.5, 0.0),
            ],
            Color32::WHITE,
            Stroke::new(1.0, Color32::from_black_alpha(140)),
        ));
    }
}

/// The clips of a folder's tracks in its row, a band per track in its color.
pub(super) fn draw_folder(
    painter: &egui::Painter,
    row: Rect,
    project: &Project,
    members: &[TrackId],
    scale: Scale,
) {
    let inner = row.shrink2(vec2(0.0, 9.0));
    let n = members.len() as f32;
    let gap = if inner.height() / n > 5.0 { 2.0 } else { 1.0 };
    let band = ((inner.height() - gap * (n - 1.0)) / n).clamp(2.0, 9.0);
    let top = inner.center().y - (band * n + gap * (n - 1.0)) / 2.0;
    for (k, id) in members.iter().enumerate() {
        let Some(track) = project.track(*id) else {
            continue;
        };
        let color = crate::theme::track_color(track.color);
        let y = top + k as f32 * (band + gap);
        // Its own thin lane, faintly in its color, its clips as blocks along it.
        painter.rect_filled(
            Rect::from_x_y_ranges(row.x_range(), y..=y + band),
            0.0,
            color.gamma_multiply(0.06),
        );
        for clip in &track.clips {
            let (a, b) = (scale.x(clip.start), scale.x(clip.end()));
            if b < row.left() || a > row.right() {
                continue;
            }
            let r = Rect::from_x_y_ranges(a..=b.max(a + 2.0), y..=y + band);
            painter.rect_filled(
                r,
                (band / 2.0).min(2.5),
                color.gamma_multiply(if clip.muted { 0.3 } else { 0.85 }),
            );
        }
    }
}

/// Which take a piece of the comp comes from: the take's color along the clip's foot, and its
/// name when there is room.
pub(super) fn draw_take_badge(painter: &egui::Painter, rect: Rect, track: &Track, k: usize) {
    let color = take_color(track, k);
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - 3.0), rect.right_bottom()),
        0.0,
        color,
    );
    if rect.width() > 70.0
        && let Some(lane) = track.takes.get(k)
    {
        painter.text(
            pos2(rect.left() + 5.0, rect.bottom() - 5.0),
            Align2::LEFT_BOTTOM,
            lane_label(lane),
            FontId::proportional(10.0),
            color,
        );
    }
}
