//! Where things are in the piano roll: time across, keys down (all of them, or folded to the
//! scale and the keys in use).

use eframe::egui::{Rect, pos2};

pub const MIN_KEY_HEIGHT: f32 = 6.0;
pub const MAX_KEY_HEIGHT: f32 = 40.0;
pub const MIN_ZOOM: f64 = 0.5;
pub const MAX_ZOOM: f64 = 50_000.0;

#[derive(Clone, Debug)]
pub struct View {
    /// Project samples per pixel.
    pub zoom: f64,
    /// Project time at the left edge.
    pub scroll: f64,
    pub key_height: f32,
    /// Pixels scrolled from the top row.
    pub scroll_y: f32,
    /// Keys shown, from the top.
    pub rows: Vec<u8>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 100.0,
            scroll: 0.0,
            key_height: 14.0,
            scroll_y: 50.0 * 14.0,
            rows: (0..=127u8).rev().collect(),
        }
    }
}

/// The view's mapping for one frame, over the note area.
#[derive(Clone)]
pub struct Frame {
    pub view: View,
    pub area: Rect,
}

impl Frame {
    pub fn x(&self, t: u64) -> f32 {
        self.area.left() + ((t as f64 - self.view.scroll) / self.view.zoom) as f32
    }

    pub fn t(&self, x: f32) -> u64 {
        (self.view.scroll + f64::from(x - self.area.left()) * self.view.zoom).max(0.0) as u64
    }

    /// Top of a key's row, if it is shown.
    pub fn y(&self, key: u8) -> Option<f32> {
        let row = self.view.rows.iter().position(|&k| k == key)?;
        Some(self.row_top(row))
    }

    pub fn row_top(&self, row: usize) -> f32 {
        self.area.top() + row as f32 * self.view.key_height - self.view.scroll_y
    }

    /// The key whose row is at `y` (the nearest at the ends).
    pub fn key(&self, y: f32) -> u8 {
        let rows = &self.view.rows;
        let row = ((y - self.area.top() + self.view.scroll_y) / self.view.key_height).floor();
        rows[(row.max(0.0) as usize).min(rows.len().saturating_sub(1))]
    }

    pub fn note_rect(&self, start: u64, end: u64, key: u8) -> Option<Rect> {
        let top = self.y(key)?;
        let x0 = self.x(start);
        let x1 = self.x(end).max(x0 + 3.0);
        Some(Rect::from_min_max(
            pos2(x0, top + 0.5),
            pos2(x1, top + self.view.key_height - 0.5),
        ))
    }
}

impl View {
    pub fn content_height(&self) -> f32 {
        self.rows.len() as f32 * self.key_height
    }

    pub fn clamp(&mut self, height: f32) {
        self.scroll_y = self
            .scroll_y
            .clamp(0.0, (self.content_height() - height).max(0.0));
        self.scroll = self.scroll.max(-1.0e9);
    }

    /// Zooms time by `factor` keeping the time under `x` (pixels from the left) in place.
    pub fn zoom_time(&mut self, factor: f64, x: f32) {
        let anchor = self.scroll + f64::from(x) * self.zoom;
        self.zoom = (self.zoom / factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = anchor - f64::from(x) * self.zoom;
    }

    /// Changes the row height keeping the row under `y` (pixels from the top) in place.
    pub fn zoom_keys(&mut self, factor: f32, y: f32) {
        let row = (self.scroll_y + y) / self.key_height;
        self.key_height = (self.key_height * factor).clamp(MIN_KEY_HEIGHT, MAX_KEY_HEIGHT);
        self.scroll_y = row * self.key_height - y;
    }

    /// Shows the keys `rows` (from the top), keeping `key` at the same height on screen.
    pub fn set_rows(&mut self, rows: Vec<u8>, key: u8) {
        let before = self.rows.iter().position(|&k| k == key);
        let after = (0..rows.len()).min_by_key(|&i| rows[i].abs_diff(key));
        if let (Some(b), Some(a)) = (before, after) {
            self.scroll_y += (a as f32 - b as f32) * self.key_height;
        }
        self.rows = rows;
    }

    /// Frames `start..end` (project samples) across `width` and keys `low..=high` down `height`.
    pub fn fit(&mut self, start: u64, end: u64, low: u8, high: u8, width: f32, height: f32) {
        let span = end.saturating_sub(start).max(1) as f64;
        self.zoom = (span * 1.08 / f64::from(width.max(100.0))).clamp(MIN_ZOOM, MAX_ZOOM);
        self.scroll = start as f64 - span * 0.04;
        let shown = self
            .rows
            .iter()
            .filter(|&&k| k >= low.saturating_sub(2) && k <= high.saturating_add(2))
            .count()
            .max(12);
        self.key_height = (height / shown as f32).clamp(11.0, MAX_KEY_HEIGHT.min(22.0));
        let top = self
            .rows
            .iter()
            .position(|&k| k <= high.saturating_add(2))
            .unwrap_or(0);
        let rows_shown = height / self.key_height;
        let wanted = shown as f32;
        self.scroll_y = (top as f32 - ((rows_shown - wanted) / 2.0).max(0.0)) * self.key_height;
        self.clamp(height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn rows_map_both_ways_and_zoom_keeps_the_anchor() {
        let mut view = View {
            scroll_y: 0.0,
            ..Default::default()
        };
        let area = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 400.0));
        let frame = Frame {
            view: view.clone(),
            area,
        };
        assert_eq!(frame.y(127), Some(50.0));
        assert_eq!(frame.key(50.0 + 14.0 * 3.0 + 1.0), 124);
        assert_eq!(frame.t(frame.x(48_000)), 48_000);
        let t = frame.t(500.0);
        view.zoom_time(2.0, 400.0);
        let frame = Frame {
            view: view.clone(),
            area,
        };
        assert!(frame.t(500.0).abs_diff(t) <= 100);
        view.rows = vec![72, 67, 64, 60];
        let frame = Frame {
            view: view.clone(),
            area,
        };
        assert_eq!(frame.y(61), None);
        assert_eq!(frame.key(50.0 + 14.0 * 2.5), 64);
    }
}
