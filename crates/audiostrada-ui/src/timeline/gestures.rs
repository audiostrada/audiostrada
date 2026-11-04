//! Mouse gestures on the lanes and the ruler: each drag previews its edit on a copy of the
//! project, releasing commits it as one undo step.

use super::*;

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

        // Smart Tool zone under the pointer.
        let zone = |pos: Pos2| -> Option<(DragKind, ClipId)> {
            let (clip_id, _, rect) = hovered?;
            let clip = edit::clip(project, clip_id)?;
            let px = |s: u64| (s as f64 / scale.zoom) as f32;
            let kind = if pos.y < rect.top() + FADE_ZONE_HEIGHT
                && pos.x < rect.left() + px(clip.fade_in) + 10.0
            {
                DragKind::FadeIn
            } else if pos.y < rect.top() + FADE_ZONE_HEIGHT
                && pos.x > rect.right() - px(clip.fade_out) - 10.0
            {
                DragKind::FadeOut
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

        // An edge of the range, on its tracks or in the ruler: the edge that stays.
        let range_view = self.range.as_ref().map(|r| {
            let spans: Vec<(f32, f32)> = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, (index, _))| r.tracks.contains(&project.tracks[*index].id))
                .map(|(i, _)| {
                    let top = lanes.top() + self.tops[i] - self.scroll_y;
                    (top, top + self.track_height)
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
                Some(DragKind::TrimStart | DragKind::TrimEnd) => CursorIcon::ResizeHorizontal,
                Some(DragKind::Stretch) => CursorIcon::ResizeColumn,
                Some(DragKind::FadeIn | DragKind::FadeOut) => CursorIcon::ResizeNwSe,
                Some(DragKind::Gain) => CursorIcon::ResizeVertical,
                Some(DragKind::Move { copy: true }) => CursorIcon::Copy,
                Some(DragKind::Move { .. }) => CursorIcon::Grab,
                _ => CursorIcon::Text,
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        // Files dragged from the browser.
        if let Some(pos) = lanes_response.dnd_hover_payload::<DraggedFile>().and(
            lanes_response
                .hover_pos()
                .or(ui.input(|i| i.pointer.hover_pos())),
        ) {
            let x = scale.x(settings.snap(project, scale.sample(pos.x)));
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
            let position = settings.snap(project, scale.sample(pos.x));
            doc.import_at(std::slice::from_ref(&file.0), track, position);
        }

        // Starting a drag.
        if ruler_response.drag_started()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let pos = ui.input(|i| i.pointer.press_origin()).unwrap_or(pos);
            let sample = settings.snap(project, scale.sample(pos.x));
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
                kind: DragKind::Comp(track, playlist, settings.snap(project, scale.sample(pos.x))),
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
                _ if modifiers.shift && !modifiers.command => {
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
                        samples = settings.snap(project, target) as i64 - c.start as i64;
                    }
                    edit::move_clips(&mut preview, &ids, samples, track_offset);
                }
                DragKind::TrimStart
                | DragKind::TrimEnd
                | DragKind::Stretch
                | DragKind::FadeIn
                | DragKind::FadeOut
                | DragKind::Gain => {
                    if let Some(c) = drag
                        .clip
                        .and_then(|c| edit::clip(&drag.original, c))
                        .cloned()
                    {
                        let at = |x: u64| settings.snap(project, x);
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
                DragKind::Loop { anchor, punch } => {
                    let other = settings.snap(project, scale.sample(pos.x));
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
                    let target = settings.snap(project, scale.sample(pos.x));
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
                        let time = settings.snap(project, scale.sample(pos.x));
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
                    let other = settings.snap(project, scale.sample(pos.x));
                    let (a, b) = (anchor.min(other), anchor.max(other));
                    edit::comp_range(&mut preview, track, playlist, a, b);
                    self.comping = (b > a).then_some((track, playlist, a, b));
                }
                DragKind::Range { anchor } => {
                    let (a, b) = (
                        settings.snap(project, scale.sample(anchor.x.min(pos.x))),
                        settings.snap(project, scale.sample(anchor.x.max(pos.x))),
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
                    let at = settings.snap(project, scale.sample(pos.x));
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
                    let at = settings.snap(project, scale.sample(pos.x));
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
                DragKind::Gain => tr("Clip gain"),
                DragKind::Loop { punch: true, .. } => tr("Set punch range"),
                DragKind::Loop { .. } => tr("Set loop"),
                DragKind::Marker(_) => tr("Move marker"),
                DragKind::Select { .. }
                | DragKind::Range { .. }
                | DragKind::RangeEdge { .. }
                | DragKind::RulerRange { .. } => tr("Edit"),
                DragKind::Point(..) => tr("Move automation point"),
                DragKind::Comp(..) => tr("Comp takes"),
            };
            doc.history_mut().edit(label, |p| *p = preview);
        }
        if self.drag.is_none() {
            self.comping = None;
        }

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
                let time = settings.snap(project, scale.sample(pos.x));
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
            let at = settings.snap(project, scale.sample(pos.x));
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

        // Clicks.
        if hit.is_none()
            && lanes_response.clicked()
            && let Some(pos) = lanes_response.interact_pointer_pos()
        {
            if !modifiers.shift {
                self.range = None;
            }
            let extend = modifiers.shift && self.range.is_some();
            if extend {
                let at = settings.snap(project, scale.sample(pos.x));
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
            let target = settings.snap(project, scale.sample(pos.x));
            doc.player_mut().locate(target);
        }
        if ruler_response.clicked()
            && let Some(pos) = ruler_response.interact_pointer_pos()
        {
            let target = settings.snap(project, scale.sample(pos.x));
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
            let at = settings.snap(project, scale.sample(pos.x));
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
