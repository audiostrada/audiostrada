//! Zoom and scrolling of the timeline.

use super::*;

impl TimelineView {
    /// The horizontal scrollbar: its thumb shows the visible part of the session, dragged to
    /// scroll; a click beside it centers the view there.
    pub(super) fn scrollbar(&mut self, ui: &mut Ui, rect: Rect, length: u64) {
        let visible = f64::from(rect.width()) * self.zoom;
        let extent = (length as f64 * 1.05).max(self.scroll + visible).max(1.0);
        let width = f64::from(rect.width());
        let thumb_width = (visible / extent * width).max(24.0).min(width) as f32;
        let travel = (rect.width() - thumb_width).max(1.0);
        let samples_per_px = (extent - visible).max(0.0) / f64::from(travel);
        let thumb_left = rect.left() + (self.scroll / samples_per_px.max(1e-9)) as f32;
        let thumb = Rect::from_min_size(
            pos2(thumb_left.min(rect.right() - thumb_width), rect.top() + 2.0),
            vec2(thumb_width, rect.height() - 4.0),
        );
        let response = ui.interact(
            rect,
            ui.scope_id().with("h-scrollbar"),
            Sense::click_and_drag(),
        );
        if response.dragged() {
            self.scroll = (self.scroll + f64::from(response.drag_delta().x) * samples_per_px)
                .clamp(0.0, (extent - visible).max(0.0));
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && !thumb.contains(pos)
        {
            let center = f64::from(pos.x - rect.left()) / width * extent;
            self.scroll = (center - visible / 2.0).clamp(0.0, (extent - visible).max(0.0));
        }
        let visuals = ui.visuals();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
        let fill = if response.dragged() || response.hovered() {
            visuals.weak_text_color().gamma_multiply(0.8)
        } else {
            visuals.weak_text_color().gamma_multiply(0.45)
        };
        painter.rect_filled(thumb, 4.0, fill);
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
