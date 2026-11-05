//! Zoom and scrolling of the timeline.

use super::*;

impl TimelineView {
    /// The whole project in small under the lanes: each track's clips in its color, the
    /// markers, the loop and the playhead, and the part shown in a frame. Drag the frame to
    /// scroll, its edges to zoom; a click elsewhere centers the view there, a double-click
    /// shows everything.
    pub(super) fn overview(&mut self, ui: &mut Ui, rect: Rect, project: &Project, playhead: u64) {
        let visible = f64::from(self.lanes_width.max(1.0)) * self.zoom;
        let extent = (project.length() as f64 * 1.05)
            .max(self.scroll + visible)
            .max(1.0);
        let width = f64::from(rect.width().max(1.0));
        let x = |s: f64| rect.left() + (s / extent * width) as f32;
        let sample = |x: f32| (f64::from(x - rect.left()) / width * extent).max(0.0);
        let frame = Rect::from_x_y_ranges(
            x(self.scroll)..=x(self.scroll + visible).max(x(self.scroll) + 6.0),
            rect.top() + 2.0..=rect.bottom() - 2.0,
        );
        let response = ui.interact(
            rect,
            ui.scope_id().with("overview"),
            Sense::click_and_drag(),
        );
        let near = |p: Pos2, edge: f32| (p.x - edge).abs() <= 5.0;
        if let Some(p) = response.hover_pos()
            && self.overview_grab.is_none()
            && (near(p, frame.left()) || near(p, frame.right()))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if response.drag_started()
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            self.overview_grab = Some(if near(p, frame.left()) {
                Grab::Left
            } else if near(p, frame.right()) {
                Grab::Right
            } else {
                Grab::Move
            });
        }
        if response.dragged()
            && let (Some(grab), Some(p)) = (self.overview_grab, response.interact_pointer_pos())
        {
            let lanes = f64::from(self.lanes_width.max(1.0));
            let right = self.scroll + visible;
            match grab {
                Grab::Move => {
                    let delta = f64::from(response.drag_delta().x) / width * extent;
                    self.scroll = (self.scroll + delta).max(0.0);
                }
                Grab::Left => {
                    let left = sample(p.x).min(right - MIN_ZOOM * lanes);
                    self.zoom = ((right - left) / lanes).clamp(MIN_ZOOM, MAX_ZOOM);
                    self.scroll = (right - self.zoom * lanes).max(0.0);
                }
                Grab::Right => {
                    let end = sample(p.x).max(self.scroll + MIN_ZOOM * lanes);
                    self.zoom = ((end - self.scroll) / lanes).clamp(MIN_ZOOM, MAX_ZOOM);
                }
            }
        }
        if response.drag_stopped() {
            self.overview_grab = None;
        }
        if response.double_clicked() {
            self.zoom_to_fit(project.length());
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && !frame.contains(p)
        {
            self.scroll = (sample(p.x) - visible / 2.0).max(0.0);
        }

        let p = crate::theme::current(ui);
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, p.base);
        painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
        if let Some((a, b)) = project.looping() {
            painter.rect_filled(
                Rect::from_x_y_ranges(x(a as f64)..=x(b as f64), rect.y_range()),
                0.0,
                p.accent.gamma_multiply(0.12),
            );
        }
        // One thin band per shown track, its clips in its color.
        let rows = self.rows.len().max(1) as f32;
        let band = ((rect.height() - 6.0) / rows).clamp(1.0, 6.0);
        let top = rect.center().y - band * rows / 2.0;
        for (i, &(index, _)) in self.rows.iter().enumerate() {
            let track = &project.tracks[index];
            let color = Color32::from_rgb(track.color[0], track.color[1], track.color[2]);
            let y = top + i as f32 * band;
            for clip in &track.clips {
                let r = Rect::from_x_y_ranges(
                    x(clip.start as f64)..=x(clip.end() as f64).max(x(clip.start as f64) + 1.0),
                    y..=y + (band - 1.0).max(1.0),
                );
                painter.rect_filled(r, 0.0, color.gamma_multiply(0.85));
            }
        }
        for marker in &project.markers {
            painter.vline(
                x(marker.position as f64),
                rect.y_range(),
                Stroke::new(1.0, Color32::from_rgb(240, 200, 80).gamma_multiply(0.7)),
            );
        }
        painter.vline(
            x(playhead as f64),
            rect.y_range(),
            Stroke::new(1.5, Color32::from_rgb(240, 80, 70)),
        );
        // The part shown.
        let active = response.hovered() || response.dragged();
        painter.rect_filled(
            frame,
            3.0,
            Color32::from_white_alpha(if active { 26 } else { 16 }),
        );
        painter.rect_stroke(
            frame,
            3.0,
            Stroke::new(
                1.0,
                Color32::from_white_alpha(if active { 200 } else { 120 }),
            ),
            StrokeKind::Inside,
        );
        response.on_hover_text(tr(
            "The whole project: drag the frame to move, its edges to zoom; click to go there, \
             double-click to see everything",
        ));
    }

    pub(super) fn zoom_around(&mut self, factor: f64, anchor_sample: f64, anchor_offset: f32) {
        self.zoom = (self.zoom / factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = (anchor_sample - f64::from(anchor_offset) * self.zoom).max(0.0);
    }

    /// Changes the track height keeping the content under `pointer_y` (from the top of the
    /// lanes) in place.
    pub(super) fn set_track_height(&mut self, height: f32, pointer_y: Option<f32>) {
        let height = height.clamp(MIN_TRACK_HEIGHT, MAX_TRACK_HEIGHT);
        let scale = height / self.track_height;
        if let Some(y) = pointer_y
            && let Some(i) = self.tops.iter().rposition(|&t| t <= self.scroll_y + y)
        {
            let within = self.scroll_y + y - self.tops[i];
            let row = self.row_height(i);
            let above: f32 = (0..i).map(|r| self.row_height(r)).sum::<f32>() * (scale - 1.0);
            // Lanes below the track keep their height.
            let moved = if within <= row {
                within * scale
            } else {
                within + row * (scale - 1.0)
            };
            self.scroll_y = (self.tops[i] + above + moved - y).max(0.0);
        }
        self.track_height = height;
    }

    /// The pointer's distance from the left and top of the lanes, when it is over them.
    pub(super) fn hover_offset(&self) -> Option<Vec2> {
        self.hover.map(|p| p - self.lanes_rect.min)
    }

    pub(super) fn zoom_to_fit(&mut self, length: u64) {
        let length = length.max(1) as f64;
        self.zoom =
            (length * 1.05 / f64::from(self.lanes_width.max(100.0))).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = 0.0;
    }

    /// Zooms on the range or the selected clips; again, back to the view before.
    pub(super) fn zoom_to_selection(&mut self, doc: &Document) {
        if let Some((zoom, scroll)) = self.zoom_back.take() {
            (self.zoom, self.scroll) = (zoom, scroll);
            return;
        }
        let project = doc.project();
        let span = self.range.as_ref().map(|r| (r.start, r.end)).or_else(|| {
            let clips: Vec<_> = self
                .selection
                .iter()
                .filter_map(|&id| edit::clip(project, id))
                .collect();
            Some((
                clips.iter().map(|c| c.start).min()?,
                clips.iter().map(|c| c.end()).max()?,
            ))
        });
        let Some((start, end)) = span.filter(|(a, b)| b > a) else {
            self.zoom_back = Some((self.zoom, self.scroll));
            self.zoom_to_fit(project.length());
            return;
        };
        self.zoom_back = Some((self.zoom, self.scroll));
        let width = f64::from(self.lanes_width.max(100.0));
        let length = (end - start) as f64;
        // A margin of a twentieth on each side.
        self.zoom = (length * 1.1 / width).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = (start as f64 - length * 0.05).max(0.0);
    }

    /// Fades on the range's edges (crossfades where clips meet inside it), or else short
    /// fades against clicks on the selected clips that have none.
    pub(super) fn fades(&mut self, doc: &mut Document) {
        if let Some(r) = self.range.clone() {
            doc.history_mut().edit(Action::Fades.label(), |p| {
                edit::fades_in_range(p, &r.tracks, r.start, r.end);
            });
            return;
        }
        let ids = self.selection.clone();
        let short = u64::from(doc.project().sample_rate) / 100;
        doc.history_mut().edit(Action::Fades.label(), |p| {
            for id in &ids {
                if let Some(c) = edit::clip(p, *id).cloned() {
                    let fade_in = if c.fade_in == 0 { short } else { c.fade_in };
                    let fade_out = if c.fade_out == 0 { short } else { c.fade_out };
                    edit::set_fades(p, *id, fade_in, fade_out);
                }
            }
        });
    }
}
