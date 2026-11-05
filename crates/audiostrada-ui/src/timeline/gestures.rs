//! Mouse gestures on the lanes and the ruler: each drag previews its edit on a copy of the
//! project, releasing commits it as one undo step.

use super::magnet::{Magnet, Pull};
use super::*;
use audiostrada_core::model::FadeShape;

impl TimelineView {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn interact(
        &mut self,
        ui: &mut Ui,
        doc: &mut Document,
        project: &Project,
        settings: &EditSettings,
        scale: Scale,
        lanes: Rect,
        ruler: Rect,
        hovered: Option<(ClipId, TrackId, Rect)>,
    ) {
        let ruler_response =
            ui.interact(ruler, ui.scope_id().with("ruler"), Sense::click_and_drag());
        let lanes_response =
            ui.interact(lanes, ui.scope_id().with("lanes"), Sense::click_and_drag());
        let modifiers = ui.input(|i| i.modifiers);
        let moving = if self.drag.is_some() {
            self.selection.clone()
        } else {
            Vec::new()
        };
        let magnet = Magnet::new(
            settings,
            project,
            scale.zoom,
            doc.player().position(),
            &moving,
        );
        // Ctrl+Alt holds the scissors.
        let tool = if modifiers.command && modifiers.alt {
            Tool::Split
        } else {
            self.tool
        };
        // The Smart Tool draws a range in the upper half of a clip, under its name.
        let upper = |pos: Pos2| {
            tool == Tool::Smart
                && hovered.is_some_and(|(_, _, r)| {
                    r.height() >= SMART_HEIGHT && pos.y > r.top() + NAME_BAR && pos.y < r.center().y
                })
        };

        // Smart Tool zone under the pointer.
        let zone = |pos: Pos2| -> Option<(DragKind, ClipId)> {
            let (clip_id, _, rect) = hovered?;
            let clip = edit::clip(project, clip_id)?;
            let px = |s: u64| (s as f64 / scale.zoom) as f32;
            let top = pos.y < rect.top() + FADE_ZONE_HEIGHT;
            let edge = pos.x < rect.left() + EDGE_ZONE || pos.x > rect.right() - EDGE_ZONE;
            let near_curve = |fade_in: bool| {
                fade_middle(rect, clip, fade_in, scale.zoom).is_some_and(|m| m.distance(pos) < 7.0)
            };
            let kind = if modifiers.command && modifiers.alt && !edge {
                DragKind::Slip
            } else if near_curve(true) || near_curve(false) {
                DragKind::FadeCurve {
                    fade_in: near_curve(true),
                    top: rect.top(),
                    height: rect.height(),
                }
            } else if top && pos.x < rect.left() + px(clip.fade_in) + 10.0 {
                DragKind::FadeIn
            } else if top && pos.x > rect.right() - px(clip.fade_out) - 10.0 {
                DragKind::FadeOut
            } else if top && rect.width() >= 48.0 && (pos.x - rect.center().x).abs() < 9.0 {
                DragKind::Gain
            } else if pos.x < rect.left() + EDGE_ZONE {
                DragKind::TrimStart
            } else if pos.x > rect.right() - EDGE_ZONE && modifiers.alt && !clip.is_midi() {
                DragKind::Stretch
            } else if pos.x > rect.right() - EDGE_ZONE {
                DragKind::TrimEnd
            } else if modifiers.command {
                DragKind::Gain
            } else {
                DragKind::Move {
                    copy: modifiers.alt,
                }
            };
            Some((kind, clip_id))
        };

        // The crossfade handle, at the top of two overlapping clips.
        let crossfade = |pos: Pos2| -> Option<DragKind> {
            let (_, track, rect) = hovered?;
            if pos.y > rect.top() + FADE_ZONE_HEIGHT || tool != Tool::Smart {
                return None;
            }
            let clips = &project.track(track)?.clips;
            clips.windows(2).find_map(|pair| {
                let (a, b) = (&pair[0], &pair[1]);
                let x = scale.x((b.start + a.end()) / 2);
                (a.end() > b.start && (pos.x - x).abs() < 8.0).then(|| DragKind::Crossfade {
                    left: a.id,
                    right: b.id,
                    overlap: a.end() - b.start,
                })
            })
        };

        // An edge of the range, on its tracks or in the ruler: the edge that stays.
        let range_view = self.range.as_ref().map(|r| {
            let spans: Vec<(f32, f32)> = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, (index, _))| r.tracks.contains(&project.tracks[*index].id))
                .map(|(i, _)| {
                    let top = lanes.top() + self.tops[i] - self.scroll_y;
                    (top, top + self.row_height(i))
                })
                .collect();
            (r.start, r.end, spans)
        });
        let range_edge = |pos: Pos2, in_ruler: bool| -> Option<u64> {
            let (start, end, spans) = range_view.as_ref()?;
            let on_track = in_ruler || spans.iter().any(|&(a, b)| pos.y >= a && pos.y < b);
            let (d0, d1) = (
                (pos.x - scale.x(*start)).abs(),
                (pos.x - scale.x(*end)).abs(),
            );
            match on_track && d0.min(d1) <= RANGE_GRAB {
                true if d0 <= d1 => Some(*end),
                true => Some(*start),
                false => None,
            }
        };
        if self.drag.is_none()
            && let Some(pos) = ruler_response.hover_pos()
            && range_edge(pos, true).is_some()
        {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if self.drag.is_none()
            && let Some(pos) = lanes_response.hover_pos()
        {
            let cursor = match zone(pos).map(|z| z.0) {
                _ if range_edge(pos, false).is_some() => CursorIcon::ResizeHorizontal,
                _ if crossfade(pos).is_some() => CursorIcon::ResizeHorizontal,
                Some(DragKind::Slip) => CursorIcon::AllScroll,
                Some(DragKind::FadeCurve { .. }) => CursorIcon::ResizeVertical,
                _ if tool == Tool::Range => CursorIcon::Text,
                Some(_) if tool == Tool::Split => CursorIcon::Crosshair,
                Some(_) if matches!(tool, Tool::Erase | Tool::Mute) => CursorIcon::PointingHand,
                _ if tool != Tool::Smart => CursorIcon::Default,
                Some(DragKind::Move { copy: false }) if upper(pos) => CursorIcon::Text,
                Some(DragKind::TrimStart | DragKind::TrimEnd) => CursorIcon::ResizeHorizontal,
                Some(DragKind::Stretch) => CursorIcon::ResizeColumn,
                Some(DragKind::FadeIn | DragKind::FadeOut) => CursorIcon::ResizeNwSe,
                Some(DragKind::Gain) => CursorIcon::ResizeVertical,
                Some(DragKind::Move { copy: true }) => CursorIcon::Copy,
                Some(DragKind::Move { .. }) => CursorIcon::Grab,
                _ => CursorIcon::Text,
            };
            ui.ctx().set_cursor_icon(cursor);
            self.tool_hover(ui, project, settings, scale, lanes, tool, pos, hovered);
        }

        // Files dragged from the browser.
        if let Some(pos) = lanes_response.dnd_hover_payload::<DraggedFile>().and(
            lanes_response
                .hover_pos()
                .or(ui.input(|i| i.pointer.hover_pos())),
        ) {
            let x = scale.x(magnet.snap(scale.sample(pos.x)));
            ui.painter().line_segment(
                [pos2(x, lanes.top()), pos2(x, lanes.bottom())],
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
        }
        if let Some(file) = lanes_response.dnd_release_payload::<DraggedFile>()
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            let track = self
                .track_at(project, pos.y - lanes.top())
                .filter(|t| t.kind == TrackKind::Audio)
                .map(|t| t.id);
            let position = magnet.snap(scale.sample(pos.x));
            doc.import_at(std::slice::from_ref(&file.0), track, position);
        }

        // Starting a drag.
        if ruler_response.drag_started()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let sample = magnet.snap(scale.sample(pos.x));
            let marker = project
                .markers
                .iter()
                .find(|m| (scale.x(m.position) - pos.x).abs() < 6.0 && pos.y > ruler.top() + 10.0);
            // The loop band (or Alt) sets the loop, from the edge not grabbed; Shift the punch.
            let in_band = pos.y > ruler.bottom() - LOOP_BAND || modifiers.alt;
            let loop_edge = project.loop_range.and_then(|(a, b)| {
                let (d0, d1) = ((pos.x - scale.x(a)).abs(), (pos.x - scale.x(b)).abs());
                (d0.min(d1) <= RANGE_GRAB).then_some(if d0 <= d1 { b } else { a })
            });
            let kind = if modifiers.shift {
                DragKind::Loop {
                    anchor: sample,
                    punch: true,
                }
            } else if in_band {
                DragKind::Loop {
                    anchor: loop_edge.unwrap_or(sample),
                    punch: false,
                }
            } else if let Some(anchor) = range_edge(pos, true) {
                DragKind::RangeEdge { anchor }
            } else if let Some(m) = marker {
                DragKind::Marker(m.id)
            } else {
                DragKind::RulerRange { anchor: sample }
            };
            self.drag = Some(Drag {
                kind,
                clip: None,
                origin: pos,
                original: doc.project().clone(),
            });
        }
        // Automation lanes: hit test of the pointer.
        let lane_hit = |pos: Pos2| -> Option<(TrackId, usize, Rect, Option<usize>)> {
            let (r, li, top) = self.lane_at(project, pos.y - lanes.top())?;
            let track = &project.tracks[self.rows[r].0];
            let lane = track.automation.get(li)?;
            let (_, min, max) = *self
                .lane_info
                .get(&(track.id, li))
                .map(|i| (&i.0, i.1, i.2))
                .as_ref()?;
            let rect = Rect::from_min_max(
                pos2(lanes.left(), lanes.top() + top),
                pos2(lanes.right(), lanes.top() + top + LANE_HEIGHT),
            );
            let point = lane
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let d = pos2(scale.x(p.time), value_y(rect, p.value, min, max)) - pos;
                    (i, d.length())
                })
                .filter(|&(_, d)| d < POINT_GRAB)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            Some((track.id, li, rect, point))
        };
        // A drag starts a few pixels away from the press: hit test where the button went down.
        let press = ui.input(|i| i.pointer.press_origin());
        let hit = if lanes_response.drag_started() {
            press.or(lanes_response.interact_pointer_pos())
        } else {
            lanes_response
                .interact_pointer_pos()
                .or(lanes_response.hover_pos())
        }
        .and_then(lane_hit);
        let start_pos = press.or(lanes_response.interact_pointer_pos());
        if self.drag.is_none() && hit.is_some() && lanes_response.hovered() {
            let point = hit.as_ref().and_then(|h| h.3);
            ui.ctx().set_cursor_icon(if point.is_some() {
                CursorIcon::Grab
            } else {
                CursorIcon::Crosshair
            });
        }
        if lanes_response.drag_started()
            && let Some((track, li, rect, Some(point))) = hit
        {
            self.drag = Some(Drag {
                kind: DragKind::Point(track, li, point, rect.top()),
                clip: None,
                origin: start_pos.unwrap_or(rect.center()),
                original: doc.project().clone(),
            });
        } else if lanes_response.drag_started()
            && let Some(pos) = start_pos
            && let Some((track, playlist)) = self.take_at(project, pos.y - lanes.top())
        {
            self.drag = Some(Drag {
                kind: DragKind::Comp(track, playlist, magnet.snap(scale.sample(pos.x))),
                clip: None,
                origin: pos,
                original: doc.project().clone(),
            });
        } else if lanes_response.drag_started()
            && hit.is_none()
            && let Some(pos) = start_pos
        {
            let edge = range_edge(pos, false);
            let (kind, clip) = match zone(pos) {
                _ if let Some(anchor) = edge => (DragKind::RangeEdge { anchor }, None),
                _ if let Some(kind) = crossfade(pos) => (kind, None),
                _ if tool == Tool::Range || (modifiers.shift && !modifiers.command) => {
                    (DragKind::Range { anchor: pos }, None)
                }
                _ if matches!(tool, Tool::Erase | Tool::Mute) => {
                    self.swept.clear();
                    let first = hovered.and_then(|(c, _, _)| edit::clip(project, c));
                    let mute = (tool == Tool::Mute).then(|| !first.is_some_and(|c| c.muted));
                    (DragKind::Sweep { mute }, None)
                }
                Some((DragKind::Slip, clip)) => {
                    if !self.selection.contains(&clip) {
                        self.selection = vec![clip];
                    }
                    (DragKind::Slip, Some(clip))
                }
                _ if tool == Tool::Split => (DragKind::Select { anchor: pos }, None),
                Some((DragKind::Move { copy: false }, _)) if upper(pos) => {
                    (DragKind::Range { anchor: pos }, None)
                }
                Some((kind, clip)) => {
                    if !self.selection.contains(&clip) {
                        if !modifiers.shift {
                            self.selection.clear();
                        }
                        self.selection.push(clip);
                        if !modifiers.command {
                            self.selection =
                                audiostrada_core::groups::edit_clips(project, &self.selection);
                        }
                    }
                    (kind, Some(clip))
                }
                None => (DragKind::Select { anchor: pos }, None),
            };
            if let Some((_, track, _)) = hovered
                && !doc.is_track_selected(track)
            {
                doc.select_track(track, SelectMode::Only);
                if !modifiers.command {
                    self.follow_groups(doc);
                }
            }
            self.drag = Some(Drag {
                kind,
                clip,
                origin: pos,
                original: doc.project().clone(),
            });
        }

        // Updating the preview.
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let (Some(drag), Some(pos)) = (&self.drag, pointer) {
            let delta = pos - drag.origin;
            let mut preview = drag.original.clone();
            let track_offset = self.track_offset(drag, pos.y - lanes.top());
            match drag.kind {
                DragKind::Move { copy } => {
                    let mut ids = self.selection.clone();
                    if copy {
                        ids = edit::duplicate(&mut preview, &ids);
                        // Duplicates land after the selection; bring them back under the pointer.
                        let shift = ids
                            .iter()
                            .zip(&self.selection)
                            .filter_map(|(c, o)| {
                                Some(
                                    edit::clip(&preview, *c)?.start as i64
                                        - edit::clip(&drag.original, *o)?.start as i64,
                                )
                            })
                            .next()
                            .unwrap_or(0);
                        edit::move_clips(&mut preview, &ids, -shift, 0);
                    }
                    let mut samples = scale.samples(delta.x);
                    if let Some(c) = drag.clip.and_then(|c| edit::clip(&drag.original, c)) {
                        let target = (c.start as i64 + samples).max(0) as u64;
                        samples = magnet.snap_span(target, c.length) as i64 - c.start as i64;
                    }
                    edit::move_clips(&mut preview, &ids, samples, track_offset);
                }
                DragKind::TrimStart
                | DragKind::TrimEnd
                | DragKind::Stretch
                | DragKind::FadeIn
                | DragKind::FadeOut
                | DragKind::FadeCurve { .. }
                | DragKind::Slip
                | DragKind::Gain => {
                    if let Some(c) = drag
                        .clip
                        .and_then(|c| edit::clip(&drag.original, c))
                        .cloned()
                    {
                        let at = |x: u64| magnet.snap(x);
                        // Trims, fades and gain act on every selected clip by the same amount.
                        let others: Vec<audiostrada_core::Clip> = if self.selection.contains(&c.id)
                        {
                            self.selection
                                .iter()
                                .filter(|&&id| id != c.id)
                                .filter_map(|&id| edit::clip(&drag.original, id).cloned())
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let shifted = |v: u64, d: i64| (v as i64 + d).max(0) as u64;
                        match drag.kind {
                            DragKind::TrimStart => {
                                let target =
                                    at((c.start as i64 + scale.samples(delta.x)).max(0) as u64);
                                edit::trim_start(&mut preview, c.id, target);
                                let d = target as i64 - c.start as i64;
                                for o in &others {
                                    edit::trim_start(&mut preview, o.id, shifted(o.start, d));
                                }
                            }
                            DragKind::TrimEnd => {
                                let target =
                                    at((c.end() as i64 + scale.samples(delta.x)).max(0) as u64);
                                edit::trim_end(&mut preview, c.id, target);
                                let d = target as i64 - c.end() as i64;
                                for o in &others {
                                    edit::trim_end(&mut preview, o.id, shifted(o.end(), d));
                                }
                                if settings.mode == EditMode::Shuffle && others.is_empty() {
                                    ripple_after(&mut preview, &drag.original, c.id);
                                }
                            }
                            DragKind::Stretch => {
                                let target =
                                    at((c.end() as i64 + scale.samples(delta.x)).max(0) as u64);
                                edit::stretch_clip(
                                    &mut preview,
                                    c.id,
                                    target.saturating_sub(c.start),
                                );
                            }
                            DragKind::FadeIn => {
                                let d = scale.samples(delta.x);
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::set_fades(
                                        &mut preview,
                                        o.id,
                                        shifted(o.fade_in, d),
                                        o.fade_out,
                                    );
                                }
                            }
                            DragKind::FadeOut => {
                                let d = -scale.samples(delta.x);
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::set_fades(
                                        &mut preview,
                                        o.id,
                                        o.fade_in,
                                        shifted(o.fade_out, d),
                                    );
                                }
                            }
                            DragKind::FadeCurve {
                                fade_in,
                                top,
                                height,
                            } => {
                                let level = ((top + height - pos.y) / height).clamp(0.0, 1.0);
                                let shape = shape_for(level);
                                for o in std::iter::once(&c).chain(&others) {
                                    if let Some(m) = edit::clip_mut(&mut preview, o.id) {
                                        if fade_in {
                                            m.fade_shape = shape;
                                        } else {
                                            m.fade_out_shape = Some(shape);
                                        }
                                    }
                                }
                            }
                            DragKind::Slip => {
                                let d = scale.samples(delta.x);
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::slip_content(&mut preview, o.id, d);
                                }
                            }
                            DragKind::Gain => {
                                for o in std::iter::once(&c).chain(&others) {
                                    edit::set_clip_gain(
                                        &mut preview,
                                        o.id,
                                        o.gain_db - delta.y * 0.1,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                DragKind::Sweep { mute } => {
                    if let Some(id) = self.clip_at(project, pos, lanes, scale)
                        && !self.swept.contains(&id)
                    {
                        self.swept.push(id);
                    }
                    match mute {
                        None => edit::delete_clips(
                            &mut preview,
                            &self.swept,
                            settings.mode == EditMode::Shuffle,
                        ),
                        Some(on) => {
                            for &id in &self.swept {
                                if let Some(c) = edit::clip_mut(&mut preview, id) {
                                    c.muted = on;
                                }
                            }
                        }
                    }
                }
                DragKind::Crossfade {
                    left,
                    right,
                    overlap,
                } => {
                    let length = (overlap as i64 + 2 * scale.samples(delta.x)).max(0) as u64;
                    edit::set_crossfade(&mut preview, left, right, length);
                }
                DragKind::Loop { anchor, punch } => {
                    let other = magnet.snap(scale.sample(pos.x));
                    let (a, b) = (anchor.min(other), anchor.max(other));
                    let range = (b > a).then_some((a, b));
                    if punch {
                        preview.punch_range = range;
                    } else {
                        preview.loop_range = range;
                        preview.loop_off = false;
                        // A linked selection moves with the loop.
                        if !project.loop_fixed
                            && let (Some(r), Some((a, b))) = (&mut self.range, range)
                        {
                            (r.start, r.end) = (a, b);
                            self.linked = Some((a, b));
                        }
                    }
                }
                DragKind::Marker(id) => {
                    let target = magnet.snap(scale.sample(pos.x));
                    if let Some(m) = preview.markers.iter_mut().find(|m| m.id == id) {
                        m.position = target;
                    }
                    preview.markers.sort_by_key(|m| (m.position, m.id));
                }
                DragKind::Point(track, li, point, top) => {
                    if let Some((_, min, max)) = self.lane_info.get(&(track, li)).cloned() {
                        let rect = Rect::from_min_max(
                            pos2(lanes.left(), top),
                            pos2(lanes.right(), top + LANE_HEIGHT),
                        );
                        let time = magnet.snap(scale.sample(pos.x));
                        let value = y_value(rect, pos.y, min, max);
                        if let Some(lane) = preview
                            .track_mut(track)
                            .and_then(|t| t.automation.get_mut(li))
                        {
                            lane.move_point(point, time, value);
                        }
                    }
                }
                DragKind::Comp(track, playlist, anchor) => {
                    let other = magnet.snap(scale.sample(pos.x));
                    let (a, b) = (anchor.min(other), anchor.max(other));
                    edit::comp_range(&mut preview, track, playlist, a, b);
                    self.comping = (b > a).then_some((track, playlist, a, b));
                }
                DragKind::Range { anchor } => {
                    let (a, b) = (
                        magnet.snap(scale.sample(anchor.x.min(pos.x))),
                        magnet.snap(scale.sample(anchor.x.max(pos.x))),
                    );
                    let first = self.row_at(anchor.y.min(pos.y) - lanes.top()).unwrap_or(0);
                    let last = self
                        .row_at(anchor.y.max(pos.y) - lanes.top())
                        .unwrap_or(self.rows.len().saturating_sub(1));
                    let tracks = self
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i >= first && *i <= last)
                        .map(|(_, &(index, _))| project.tracks[index].id)
                        .collect::<Vec<_>>();
                    let tracks = if modifiers.command {
                        tracks
                    } else {
                        audiostrada_core::groups::edit_tracks(project, &tracks)
                    };
                    self.range = (b > a).then_some(Range {
                        start: a,
                        end: b,
                        tracks,
                    });
                    self.selection.clear();
                }
                DragKind::RulerRange { anchor } => {
                    let at = magnet.snap(scale.sample(pos.x));
                    let tracks = self
                        .rows
                        .iter()
                        .map(|&(index, _)| project.tracks[index].id)
                        .collect();
                    self.range = (at != anchor).then_some(Range {
                        start: anchor.min(at),
                        end: anchor.max(at),
                        tracks,
                    });
                    self.selection.clear();
                }
                DragKind::RangeEdge { anchor } => {
                    let at = magnet.snap(scale.sample(pos.x));
                    if let Some(r) = &mut self.range
                        && at != anchor
                    {
                        (r.start, r.end) = (anchor.min(at), anchor.max(at));
                    }
                }
                DragKind::Select { anchor } => {
                    let r = Rect::from_two_pos(anchor, pos);
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        Stroke::new(1.0, ui.visuals().selection.bg_fill),
                        StrokeKind::Inside,
                    );
                    let first = self.row_at(r.top() - lanes.top()).unwrap_or(0);
                    let last = self
                        .row_at(r.bottom() - lanes.top())
                        .unwrap_or(self.rows.len());
                    let (s0, s1) = (scale.sample(r.left()), scale.sample(r.right()));
                    self.selection = self
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i >= first && *i <= last)
                        .map(|(_, &(index, _))| &project.tracks[index])
                        .flat_map(|t| {
                            t.clips
                                .iter()
                                .filter(|c| c.start < s1 && c.end() > s0)
                                .map(|c| c.id)
                        })
                        .collect();
                    if !modifiers.command {
                        self.selection =
                            audiostrada_core::groups::edit_clips(project, &self.selection);
                    }
                }
            }
            if !matches!(
                drag.kind,
                DragKind::Select { .. }
                    | DragKind::Range { .. }
                    | DragKind::RangeEdge { .. }
                    | DragKind::RulerRange { .. }
            ) {
                self.preview = Some(preview);
            }
        }

        // While dragging: what pulled the position, and the value being set.
        if let Some(drag) = &self.drag
            && let Some(pos) = pointer
        {
            if let Some((at, pull)) = magnet.hit.get()
                && !matches!(
                    drag.kind,
                    DragKind::Select { .. } | DragKind::Sweep { .. } | DragKind::Gain
                )
            {
                let x = scale.x(at);
                let guide = Color32::from_rgb(255, 196, 64);
                let painter = ui.painter().with_clip_rect(lanes.union(ruler));
                painter.vline(x, ruler.top()..=lanes.bottom(), Stroke::new(1.0, guide));
                if pull != Pull::Grid {
                    let galley = painter.layout_no_wrap(
                        pull.name().to_owned(),
                        FontId::proportional(10.5),
                        Color32::BLACK,
                    );
                    let tag = Rect::from_min_size(pos2(x + 4.0, lanes.top() + 4.0), galley.size())
                        .expand2(vec2(4.0, 1.0));
                    painter.rect_filled(tag, 3.0, guide);
                    painter.galley(tag.min + vec2(4.0, 1.0), galley, Color32::BLACK);
                }
            }
            if let Some(text) = self.preview.as_ref().and_then(|p| drag_label(drag, p)) {
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::unique("timeline-drag-label"),
                ));
                let galley =
                    painter.layout_no_wrap(text, FontId::proportional(11.5), Color32::WHITE);
                let tag = Rect::from_min_size(pos + vec2(16.0, 14.0), galley.size())
                    .expand2(vec2(6.0, 3.0));
                painter.rect_filled(tag, 4.0, Color32::from_black_alpha(215));
                painter.galley(tag.min + vec2(6.0, 3.0), galley, Color32::WHITE);
            }
        }

        // Committing.
        if (lanes_response.drag_stopped() || ruler_response.drag_stopped())
            && let Some(drag) = self.drag.take()
            && let Some(preview) = self.preview.take()
        {
            let label = match drag.kind {
                DragKind::Move { copy: true } => tr("Copy clips"),
                DragKind::Move { .. } => tr("Move clips"),
                DragKind::TrimStart | DragKind::TrimEnd => tr("Trim clip"),
                DragKind::Stretch => tr("Stretch clip"),
                DragKind::FadeIn | DragKind::FadeOut => tr("Fade"),
                DragKind::FadeCurve { .. } => tr("Fade shape"),
                DragKind::Crossfade { .. } => tr("Crossfade"),
                DragKind::Slip => tr("Slip content"),
                DragKind::Gain => tr("Clip gain"),
                DragKind::Loop { punch: true, .. } => tr("Set punch range"),
                DragKind::Loop { .. } => tr("Set loop"),
                DragKind::Marker(_) => tr("Move marker"),
                DragKind::Select { .. }
                | DragKind::Range { .. }
                | DragKind::RangeEdge { .. }
                | DragKind::RulerRange { .. } => tr("Edit"),
                DragKind::Point(..) => tr("Move automation point"),
                DragKind::Sweep { mute: None } => tr("Delete clips"),
                DragKind::Sweep { mute: Some(_) } => tr("Mute clips"),
                DragKind::Comp(..) => tr("Comp takes"),
            };
            doc.history_mut().edit(label, |p| *p = preview);
        }
        if self.drag.is_none() {
            self.comping = None;
            let swept = std::mem::take(&mut self.swept);
            self.selection
                .retain(|c| !swept.contains(c) || edit::clip(doc.project(), *c).is_some());
        }
        let tool_clicked = if hit.is_none()
            && !matches!(tool, Tool::Smart | Tool::Range)
            && lanes_response.clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
            && let Some((clip, _, _)) = hovered
        {
            self.tool_click(doc, project, settings, scale, tool, modifiers, clip, pos);
            true
        } else {
            false
        };

        // A click on a take promotes the clip under the pointer.
        if lanes_response.clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
            && let Some((track, playlist)) = self.take_at(project, pos.y - lanes.top())
        {
            let at = scale.sample(pos.x);
            let clip = project
                .track(track)
                .and_then(|t| t.alternates.get(playlist))
                .and_then(|a| a.clips.iter().find(|c| c.start <= at && at < c.end()))
                .map(|c| c.id);
            if let Some(clip) = clip {
                doc.history_mut().edit(tr("Comp take"), |p| {
                    edit::promote_take(p, track, playlist, clip);
                });
            }
        }

        // Automation clicks: add a point, double-click removes, right-click edits the shape.
        if let Some((track, li, rect, point)) = hit {
            let pos = lanes_response
                .interact_pointer_pos()
                .unwrap_or(rect.center());
            if lanes_response.double_clicked()
                && let Some(point) = point
            {
                doc.history_mut().edit(tr("Delete automation point"), |p| {
                    if let Some(lane) = p.track_mut(track).and_then(|t| t.automation.get_mut(li))
                        && point < lane.points.len()
                    {
                        lane.points.remove(point);
                    }
                });
            } else if lanes_response.clicked()
                && point.is_none()
                && let Some((_, min, max)) = self.lane_info.get(&(track, li)).cloned()
            {
                let time = magnet.snap(scale.sample(pos.x));
                let value = y_value(rect, pos.y, min, max);
                doc.history_mut().edit(tr("Add automation point"), |p| {
                    if let Some(lane) = p.track_mut(track).and_then(|t| t.automation.get_mut(li)) {
                        lane.add(time, value, CurveShape::Linear);
                    }
                });
            }
            if lanes_response.secondary_clicked() {
                self.point_menu = point.map(|p| (track, li, p));
            }
        }
        if lanes_response.secondary_clicked()
            && hit.is_none()
            && let Some(pos) = lanes_response.interact_pointer_pos()
        {
            let at = magnet.snap(scale.sample(pos.x));
            self.point_menu = None;
            let row_track = self.track_at(project, pos.y - lanes.top()).map(|t| t.id);
            let in_range = self.range.as_ref().is_some_and(|r| {
                let s = scale.sample(pos.x);
                s >= r.start && s < r.end && row_track.is_some_and(|t| r.tracks.contains(&t))
            });
            let fade = hovered.and_then(|(clip, _, rect)| {
                let c = edit::clip(project, clip)?;
                let px = |s: u64| (s as f64 / scale.zoom) as f32;
                match zone(pos).map(|z| z.0) {
                    Some(DragKind::FadeIn) => Some((clip, true)),
                    Some(DragKind::FadeOut) => Some((clip, false)),
                    _ if c.fade_in > 0 && pos.x < rect.left() + px(c.fade_in) => Some((clip, true)),
                    _ if c.fade_out > 0 && pos.x > rect.right() - px(c.fade_out) => {
                        Some((clip, false))
                    }
                    _ => None,
                }
            });
            self.menu = Some(match hovered {
                _ if in_range => menus::Menu::Range,
                _ if let Some((clip, fade_in)) = fade => menus::Menu::Fade { clip, fade_in },
                Some((clip, track, _)) => {
                    if !doc.is_track_selected(track) {
                        doc.select_track(track, SelectMode::Only);
                    }
                    menus::Menu::Clip { clip, at }
                }
                None => menus::Menu::Lane {
                    track: self.track_at(project, pos.y - lanes.top()).map(|t| t.id),
                    at,
                },
            });
        } else if lanes_response.secondary_clicked() {
            self.menu = None;
        }
        let point_menu = self.point_menu;
        let menu = self.menu;
        lanes_response.context_menu(|ui| {
            let Some((track, li, point)) = point_menu else {
                match menu {
                    Some(menus::Menu::Clip { clip, at }) => self.clip_menu(ui, doc, clip, at),
                    Some(menus::Menu::Lane { track, at }) => self.lane_menu(ui, doc, track, at),
                    Some(menus::Menu::Range) => self.range_menu(ui, doc, settings),
                    Some(menus::Menu::Fade { clip, fade_in }) => {
                        self.fade_menu(ui, doc, clip, fade_in)
                    }
                    None => ui.close(),
                }
                return;
            };
            ui.label(egui::RichText::new(tr("Curve to the next point")).weak());
            for (shape, name) in [
                (CurveShape::Linear, tr("Linear")),
                (CurveShape::Smooth, tr("Smooth")),
                (CurveShape::Step, tr("Step")),
            ] {
                if ui.button(name).clicked() {
                    doc.history_mut().edit(tr("Automation curve"), |p| {
                        if let Some(pt) = p
                            .track_mut(track)
                            .and_then(|t| t.automation.get_mut(li))
                            .and_then(|l| l.points.get_mut(point))
                        {
                            pt.shape = shape;
                        }
                    });
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tr("Delete point")).clicked() {
                doc.history_mut().edit(tr("Delete automation point"), |p| {
                    if let Some(lane) = p.track_mut(track).and_then(|t| t.automation.get_mut(li))
                        && point < lane.points.len()
                    {
                        lane.points.remove(point);
                    }
                });
                ui.close();
            }
        });

        // Double-click on an empty part of a track creates a MIDI clip.
        if hit.is_none()
            && hovered.is_none()
            && lanes_response.double_clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
            && let Some(track) = self.track_at(project, pos.y - lanes.top())
            && track.kind == TrackKind::Audio
        {
            self.pending_midi = Some((track.id, scale.sample(pos.x)));
        }

        // On the handles: Alt-click turns a fade's shape, a double-click resets the fade, the
        // volume or the crossfade.
        let handle_clicked = if hit.is_none()
            && ((lanes_response.clicked() && modifiers.alt) || lanes_response.double_clicked())
            && let Some(pos) = lanes_response.interact_pointer_pos()
        {
            let double = lanes_response.double_clicked();
            if let Some(DragKind::Crossfade { left, right, .. }) = crossfade(pos) {
                if double {
                    doc.history_mut().edit(tr("Crossfade"), |p| {
                        edit::set_crossfade(p, left, right, 0);
                    });
                }
                true
            } else if let Some((kind, clip)) = zone(pos).filter(|z| match z.0 {
                DragKind::FadeIn | DragKind::FadeOut | DragKind::FadeCurve { .. } => true,
                // The volume handle itself, not Ctrl anywhere on the clip.
                DragKind::Gain => {
                    hovered.is_some_and(|(_, _, r)| pos.y < r.top() + FADE_ZONE_HEIGHT)
                }
                _ => false,
            }) {
                let fade_in = match kind {
                    DragKind::FadeCurve { fade_in, .. } => Some(fade_in),
                    DragKind::FadeIn => Some(true),
                    DragKind::FadeOut => Some(false),
                    _ => None,
                };
                let ids = if self.selection.contains(&clip) {
                    self.selection.clone()
                } else {
                    vec![clip]
                };
                doc.history_mut().edit(tr("Fade"), |p| {
                    for id in &ids {
                        let Some(c) = edit::clip_mut(p, *id) else {
                            continue;
                        };
                        match (fade_in, double) {
                            (None, _) => c.gain_db = 0.0,
                            (Some(true), true) => c.fade_in = 0,
                            (Some(false), true) => c.fade_out = 0,
                            (Some(true), false) => c.fade_shape = next_shape(c.fade_shape),
                            (Some(false), false) => {
                                c.fade_out_shape = Some(next_shape(c.out_shape()))
                            }
                        }
                    }
                });
                true
            } else {
                false
            }
        } else {
            false
        };

        // Clicks.
        if hit.is_none()
            && !tool_clicked
            && !handle_clicked
            && lanes_response.clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
        {
            if !modifiers.shift {
                self.range = None;
            }
            let extend = modifiers.shift && self.range.is_some();
            if extend {
                let at = magnet.snap(scale.sample(pos.x));
                self.extend_range(project, at, self.row_at(pos.y - lanes.top()));
            }
            match hovered {
                _ if extend => {}
                Some((clip, track, _)) => {
                    // Ctrl toggles the clip, Shift selects from the last clicked one to it.
                    if modifiers.command {
                        if let Some(i) = self.selection.iter().position(|c| *c == clip) {
                            self.selection.remove(i);
                        } else {
                            self.selection.push(clip);
                        }
                        self.clip_anchor = Some(clip);
                    } else if modifiers.shift {
                        self.selection = self.clips_between(project, clip);
                    } else {
                        self.selection = vec![clip];
                        self.clip_anchor = Some(clip);
                    }
                    if modifiers.command || modifiers.shift {
                        for t in edit_tracks(project, &self.selection) {
                            if !doc.is_track_selected(t) {
                                doc.select_track(t, SelectMode::Toggle);
                            }
                        }
                    } else {
                        doc.select_track(track, SelectMode::Only);
                    }
                    // Ctrl takes or leaves the clip alone.
                    if !modifiers.command {
                        self.follow_groups(doc);
                    }
                }
                None => {
                    self.selection.clear();
                    match self.track_at(project, pos.y - lanes.top()) {
                        Some(t) => doc.select_track(t.id, SelectMode::Only),
                        None => doc.set_track_selection(Vec::new()),
                    }
                }
            }
            let target = magnet.snap(scale.sample(pos.x));
            doc.player_mut().locate(target);
        }
        if ruler_response.clicked()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let target = magnet.snap(scale.sample(pos.x));
            doc.player_mut().locate(target);
        }
        // Over a fade handle, or dragging one: the pointer is the fade's curve.
        let fade = match &self.drag {
            Some(d) if matches!(d.kind, DragKind::FadeIn | DragKind::FadeOut) => {
                d.clip.map(|c| (c, d.kind == DragKind::FadeIn))
            }
            Some(_) => None,
            None => lanes_response
                .hover_pos()
                .filter(|&pos| range_edge(pos, false).is_none())
                .and_then(zone)
                .and_then(|(kind, clip)| match kind {
                    DragKind::FadeIn => Some((clip, true)),
                    DragKind::FadeOut => Some((clip, false)),
                    _ => None,
                }),
        };
        if let Some((clip, fade_in)) = fade
            && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
        {
            let shape = self
                .preview
                .as_ref()
                .and_then(|p| edit::clip(p, clip))
                .or(edit::clip(project, clip))
                .map_or_else(Default::default, |c| {
                    if fade_in { c.fade_shape } else { c.out_shape() }
                });
            ui.ctx().set_cursor_icon(CursorIcon::None);
            fades::paint_cursor(ui.ctx(), pos, shape, fade_in);
        }
        if ruler_response.secondary_clicked()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let marker = project
                .markers
                .iter()
                .find(|m| (scale.x(m.position) - pos.x).abs() < 6.0)
                .map(|m| m.id);
            let at = magnet.snap(scale.sample(pos.x));
            self.ruler_target = Some((at, marker));
        }
        if let Some((at, marker)) = self.ruler_target {
            ruler_response.context_menu(|ui| self.ruler_menu(ui, doc, at, marker));
        }
        if ruler_response.double_clicked() {
            if modifiers.shift {
                doc.history_mut()
                    .edit(tr("Punch off"), |p| p.punch_range = None);
            } else if project.loop_range.is_some() {
                let off = !project.loop_off;
                let label = if off { tr("Loop off") } else { tr("Loop on") };
                doc.history_mut().edit(label, |p| p.loop_off = off);
            }
        }
    }

    /// The clip under `pos`, in the clip area of its track.
    fn clip_at(&self, project: &Project, pos: Pos2, lanes: Rect, scale: Scale) -> Option<ClipId> {
        let r = self.row_at(pos.y - lanes.top())?;
        let top = lanes.top() + self.tops[r] - self.scroll_y;
        if pos.y > top + self.row_height(r) {
            return None;
        }
        let at = scale.sample(pos.x);
        project.tracks[self.rows[r].0]
            .clips
            .iter()
            .find(|c| c.start <= at && at < c.end())
            .map(|c| c.id)
    }

    /// What a tool will do to the clip under the pointer: the cut of the scissors, the clip
    /// the eraser or the mute tool would take.
    #[allow(clippy::too_many_arguments)]
    fn tool_hover(
        &self,
        ui: &Ui,
        project: &Project,
        settings: &EditSettings,
        scale: Scale,
        lanes: Rect,
        tool: Tool,
        pos: Pos2,
        hovered: Option<(ClipId, TrackId, Rect)>,
    ) {
        let Some((_, _, rect)) = hovered else {
            return;
        };
        let painter = ui.painter().with_clip_rect(lanes);
        match tool {
            Tool::Split => {
                let at = settings.snap(project, scale.sample(pos.x));
                let x = scale.x(at);
                let all = self.tool == Tool::Split && ui.input(|i| i.modifiers.alt);
                let y = if all { lanes.y_range() } else { rect.y_range() };
                painter.vline(x, y, Stroke::new(1.5, Color32::WHITE));
                let (tempo, rate) = (&project.tempo, project.sample_rate.max(1));
                let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(at, rate));
                let label = format!("{}.{}.{:03}", bbt.bar, bbt.beat, bbt.tick);
                let galley = painter.layout_no_wrap(
                    label,
                    FontId::proportional(11.0),
                    Color32::from_rgb(20, 20, 24),
                );
                let tag = Rect::from_min_size(pos2(x + 6.0, rect.top() + 3.0), galley.size())
                    .expand2(vec2(4.0, 1.0));
                painter.rect_filled(tag, 3.0, Color32::WHITE);
                painter.galley(tag.min + vec2(4.0, 1.0), galley, Color32::BLACK);
            }
            Tool::Erase => {
                painter.rect_filled(rect, 3.0, Color32::from_rgba_unmultiplied(230, 60, 60, 70));
            }
            Tool::Mute => {
                painter.rect_filled(rect, 3.0, Color32::from_black_alpha(90));
            }
            _ => {}
        }
    }

    /// A click of the scissors, the eraser or the mute tool on `clip`.
    #[allow(clippy::too_many_arguments)]
    fn tool_click(
        &mut self,
        doc: &mut Document,
        project: &Project,
        settings: &EditSettings,
        scale: Scale,
        tool: Tool,
        modifiers: egui::Modifiers,
        clip: ClipId,
        pos: Pos2,
    ) {
        match tool {
            Tool::Split => {
                let at = settings.snap(project, scale.sample(pos.x));
                if self.tool == Tool::Split && modifiers.alt {
                    let tracks: Vec<TrackId> = project.tracks.iter().map(|t| t.id).collect();
                    doc.history_mut().edit(tr("Split clips"), |p| {
                        edit::split_at(p, &tracks, &[at]);
                    });
                } else {
                    let ids = audiostrada_core::groups::edit_clips(project, &[clip]);
                    let mut created = Vec::new();
                    doc.history_mut().edit(tr("Split clips"), |p| {
                        created = edit::split(p, at, &ids);
                    });
                    self.selection = created;
                }
            }
            Tool::Erase => {
                let shuffle = settings.mode == EditMode::Shuffle;
                doc.history_mut().edit(tr("Delete clips"), |p| {
                    edit::delete_clips(p, &[clip], shuffle);
                });
                self.selection.retain(|c| *c != clip);
            }
            Tool::Mute => {
                doc.history_mut().edit(tr("Mute clips"), |p| {
                    if let Some(c) = edit::clip_mut(p, clip) {
                        c.muted = !c.muted;
                    }
                });
            }
            Tool::Smart | Tool::Range => {}
        }
    }
}

pub(super) fn value_y(rect: Rect, value: f32, min: f32, max: f32) -> f32 {
    let f = ((value - min) / (max - min).max(1e-9)).clamp(0.0, 1.0);
    rect.bottom() - LANE_PAD - f * (rect.height() - 2.0 * LANE_PAD)
}

pub(super) fn y_value(rect: Rect, y: f32, min: f32, max: f32) -> f32 {
    let f = ((rect.bottom() - LANE_PAD - y) / (rect.height() - 2.0 * LANE_PAD)).clamp(0.0, 1.0);
    min + f * (max - min)
}

/// Shuffle mode: clips after `trimmed` on its track follow its new end.
pub(super) fn ripple_after(preview: &mut Project, original: &Project, trimmed: ClipId) {
    let (Some(before), Some(after)) = (
        edit::clip(original, trimmed).cloned(),
        edit::clip(preview, trimmed).cloned(),
    ) else {
        return;
    };
    let delta = after.end() as i64 - before.end() as i64;
    let Some((t, _)) = edit::locate(preview, trimmed) else {
        return;
    };
    let track = preview.tracks[t].id;
    let later: Vec<ClipId> = original
        .track(track)
        .map(|tr| {
            tr.clips
                .iter()
                .filter(|c| c.start >= before.end() && c.id != trimmed)
                .map(|c| c.id)
                .collect()
        })
        .unwrap_or_default();
    if let Some(tr) = preview.track_mut(track) {
        for c in tr.clips.iter_mut().filter(|c| later.contains(&c.id)) {
            c.start = (c.start as i64 + delta).max(0) as u64;
        }
    }
}

/// Where a range starts and ends, in bars, beats and ticks, and how long it lasts.
pub(super) fn range_label(project: &Project, start: u64, end: u64) -> String {
    let (tempo, rate) = (&project.tempo, project.sample_rate.max(1));
    let at = |sample: u64| {
        let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(sample, rate));
        format!("{}.{}.{:03}", bbt.bar, bbt.beat, bbt.tick)
    };
    let beats = tempo.sample_to_beat(end, rate) - tempo.sample_to_beat(start, rate);
    trf!(
        "{start} → {end}   {seconds} s, {beats} beats",
        start = at(start),
        end = at(end),
        seconds = format!("{:.2}", (end - start) as f64 / f64::from(rate)),
        beats = format!("{beats:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
    )
}

/// Tracks holding the given clips, each once.
pub(super) fn edit_tracks(project: &Project, clips: &[ClipId]) -> Vec<TrackId> {
    let mut tracks = Vec::new();
    for &c in clips {
        if let Some((t, _)) = edit::locate(project, c)
            && !tracks.contains(&project.tracks[t].id)
        {
            tracks.push(project.tracks[t].id);
        }
    }
    tracks
}

/// Where the middle of a fade's curve is drawn, when the fade is wide enough to grab it.
pub(super) fn fade_middle(
    rect: Rect,
    clip: &audiostrada_core::Clip,
    fade_in: bool,
    zoom: f64,
) -> Option<Pos2> {
    let samples = if fade_in { clip.fade_in } else { clip.fade_out };
    let width = (samples as f64 / zoom) as f32;
    (width >= 16.0 && rect.height() >= 30.0).then(|| {
        let shape = if fade_in {
            clip.fade_shape
        } else {
            clip.out_shape()
        };
        let x = if fade_in {
            rect.left() + width / 2.0
        } else {
            rect.right() - width / 2.0
        };
        pos2(x, rect.bottom() - shape.gain(0.5) * rect.height())
    })
}

/// The fade shape whose middle is nearest `level` (0 at the bottom, 1 at the top).
pub(super) fn shape_for(level: f32) -> FadeShape {
    [
        FadeShape::Slow,
        FadeShape::Linear,
        FadeShape::EqualPower,
        FadeShape::Fast,
    ]
    .into_iter()
    .min_by(|a, b| {
        (a.gain(0.5) - level)
            .abs()
            .total_cmp(&(b.gain(0.5) - level).abs())
    })
    .unwrap_or_default()
}

fn seconds(project: &Project, samples: i64) -> String {
    format!(
        "{:+.3} s",
        samples as f64 / f64::from(project.sample_rate.max(1))
    )
}

fn millis(project: &Project, samples: u64) -> String {
    format!(
        "{:.0} ms",
        samples as f64 * 1000.0 / f64::from(project.sample_rate.max(1))
    )
}

fn bars(project: &Project, at: u64) -> String {
    let (tempo, rate) = (&project.tempo, project.sample_rate.max(1));
    let bbt = tempo.beat_to_bbt(tempo.sample_to_beat(at, rate));
    format!("{}.{}.{:03}", bbt.bar, bbt.beat, bbt.tick)
}

/// What a drag is setting, told beside the pointer.
fn drag_label(drag: &Drag, preview: &Project) -> Option<String> {
    let before = drag.clip.and_then(|c| edit::clip(&drag.original, c));
    let after = drag.clip.and_then(|c| edit::clip(preview, c));
    Some(match drag.kind {
        DragKind::Move { .. } => {
            let (a, b) = (before?, after?);
            let mut text = format!(
                "{}   {}",
                bars(preview, b.start),
                seconds(preview, b.start as i64 - a.start as i64)
            );
            let from = edit::track_of_clip(&drag.original, a.id);
            let to = edit::track_of_clip(preview, b.id);
            if from != to
                && let Some(track) = to.and_then(|t| preview.track(t))
            {
                text.push_str(&format!("   → {}", track.name));
            }
            text
        }
        DragKind::TrimStart | DragKind::TrimEnd => {
            let b = after?;
            let edge = if drag.kind == DragKind::TrimStart {
                b.start
            } else {
                b.end()
            };
            trf!(
                "{edge}   length {length}",
                edge = bars(preview, edge),
                length = format!(
                    "{:.3} s",
                    b.length as f64 / f64::from(preview.sample_rate.max(1))
                )
            )
        }
        DragKind::Stretch => format!("{:.1} %", after?.stretch * 100.0),
        DragKind::FadeIn => trf!(
            "Fade in {length}, {shape}",
            length = millis(preview, after?.fade_in),
            shape = tr(after?.fade_shape.name())
        ),
        DragKind::FadeOut => trf!(
            "Fade out {length}, {shape}",
            length = millis(preview, after?.fade_out),
            shape = tr(after?.out_shape().name())
        ),
        DragKind::FadeCurve { fade_in, .. } => {
            let c = after?;
            tr(if fade_in { c.fade_shape } else { c.out_shape() }.name()).to_owned()
        }
        DragKind::Gain => {
            let gain = after?.gain_db;
            if gain <= audiostrada_core::model::MIN_GAIN_DB {
                "-inf dB".to_owned()
            } else {
                format!("{gain:+.1} dB")
            }
        }
        DragKind::Slip => {
            let (a, b) = (before?, after?);
            trf!(
                "Content {shift}",
                shift = seconds(preview, a.offset as i64 - b.offset as i64)
            )
        }
        DragKind::Crossfade { left, right, .. } => {
            let (a, b) = (edit::clip(preview, left)?, edit::clip(preview, right)?);
            trf!(
                "Crossfade {length}",
                length = millis(preview, a.end().saturating_sub(b.start))
            )
        }
        _ => return None,
    })
}

fn next_shape(shape: FadeShape) -> FadeShape {
    let i = FadeShape::ALL.iter().position(|s| *s == shape).unwrap_or(0);
    FadeShape::ALL[(i + 1) % FadeShape::ALL.len()]
}
