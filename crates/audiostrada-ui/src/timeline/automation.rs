//! Editing automation lanes with the mouse, the same under a track and in the master's band.
//! Points move from where they were grabbed, the curve between two points goes up and down or
//! bends, values are drawn to the set value and the neighbours', and Shift moves finely.

use std::collections::BTreeMap;

use audiostrada_core::automation::{AutomationLane, master_lane_mut};

use super::headers::{LaneInfo, Reading};
use super::magnet::Magnet;
use super::*;

/// Distance in pixels within which the curve between points can be grabbed.
const CURVE_GRAB: f32 = 6.0;
/// How near, in pixels, a value is drawn to the set value, a neighbour's or 0 dB.
const VALUE_REACH: f32 = 7.0;
/// Fine moves (Shift) go this many times slower than the pointer.
const FINE: f32 = 0.1;
/// Pixels of drag bending a curve from straight to the most bent.
const BEND_TRAVEL: f32 = 160.0;
/// Pixels between the points of a drawn stroke.
const DRAW_STEP: f32 = 4.0;

/// An automation lane, under a track or in the master's band.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LaneKey {
    Track(TrackId, AutomationTarget),
    Master(AutomationTarget),
    /// A lane of a clip's own effects, drawn over the clip.
    Clip(ClipId, AutomationTarget),
}

impl LaneKey {
    pub(super) fn lane<'a>(&self, p: &'a Project) -> Option<&'a AutomationLane> {
        let (lanes, target) = match self {
            LaneKey::Track(t, target) => (&p.track(*t)?.automation, target),
            LaneKey::Master(target) => (&p.master.automation, target),
            LaneKey::Clip(c, target) => (&edit::clip(p, *c)?.automation, target),
        };
        lanes.iter().find(|l| &l.target == target)
    }

    fn lane_mut<'a>(&self, p: &'a mut Project) -> Option<&'a mut AutomationLane> {
        match self {
            LaneKey::Track(t, target) => Some(audiostrada_core::automation::lane_mut(
                p.track_mut(*t)?,
                target,
            )),
            LaneKey::Master(target) => Some(master_lane_mut(&mut p.master, target)),
            LaneKey::Clip(c, target) => {
                let lanes = &mut edit::clip_mut(p, *c)?.automation;
                let i = match lanes.iter().position(|l| &l.target == target) {
                    Some(i) => i,
                    None => {
                        lanes.push(AutomationLane::new(target.clone()));
                        lanes.len() - 1
                    }
                };
                lanes.get_mut(i)
            }
        }
    }

    fn target(&self) -> &AutomationTarget {
        match self {
            LaneKey::Track(_, target) | LaneKey::Master(target) | LaneKey::Clip(_, target) => {
                target
            }
        }
    }
}

/// The tools of the automation editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum AutoTool {
    /// A click places a point, a drag draws; points are moved by dragging them.
    #[default]
    Pencil,
    /// Selects only: a click on a point, a rectangle dragged; what is selected moves together.
    Select,
    /// Removes the points clicked or dragged over.
    Erase,
    /// Drag across a span to fill the view with it; click zooms in, Alt+click out.
    Zoom,
}

/// What the automation tools of the tool bar do to the chosen points.
#[derive(Clone, Copy, Debug)]
pub(super) enum LaneCommand {
    Shape(CurveShape),
    Delete,
}

/// The points chosen in a lane, by time.
#[derive(Clone, Debug)]
pub(super) struct Chosen {
    pub key: LaneKey,
    pub times: Vec<u64>,
}

/// What lies under the pointer in a lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hit {
    Point(usize),
    /// The curve from a point to the next.
    Curve(usize, usize),
    /// The flat line before the first point or after the last.
    Flat(usize),
    Empty,
}

#[derive(Debug)]
enum Gesture {
    /// Points moving together, from the grabbed one.
    Move { grabbed: u64, times: Vec<u64> },
    /// The curve between points, up or down; its value under the pointer when grabbed.
    /// A steep one goes `sideways`, in time.
    Curve {
        times: Vec<u64>,
        reference: f32,
        sideways: bool,
    },
    /// The curve from the point at `at` to the next, bent.
    Bend { at: u64 },
    /// A stroke drawn freehand.
    Draw {
        drawn: BTreeMap<u64, f32>,
        last: f32,
    },
    /// A rectangle choosing points; those chosen before it stay (Shift).
    Select { kept: Vec<u64> },
    /// The eraser, from where it was last frame.
    Erase { last: Pos2 },
}

#[derive(Debug)]
pub(super) struct LaneDrag {
    key: LaneKey,
    original: AutomationLane,
    gesture: Gesture,
    origin: Pos2,
    /// The pointer as fine moves leave it, and where the real one was last frame.
    cursor: Pos2,
    last: Pos2,
}

/// A value being typed for a point.
#[derive(Clone, Debug)]
pub(super) struct ValueEdit {
    key: LaneKey,
    time: u64,
    at: Pos2,
    text: String,
    focused: bool,
}

/// A lane as the editor sees it this frame.
pub(super) struct LaneView<'a> {
    pub key: LaneKey,
    pub rect: Rect,
    /// The part of the screen the lane may use (under the ruler, beside the headers).
    pub clip: Rect,
    pub info: &'a LaneInfo,
    pub scale: Scale,
    pub magnet: &'a Magnet<'a>,
    /// Where the lane's time zero is on the timeline: a clip's lane counts from the start of
    /// its content, which can come before the project's start.
    pub origin: i64,
    /// Drawn over a clip: only its points and curve are grabbed, the rest of the clip stays
    /// the clip's.
    pub overlay: bool,
}

impl LaneView<'_> {
    /// Where a time of the lane is on the timeline.
    fn timeline(&self, time: u64) -> u64 {
        (self.origin + time as i64).max(0) as u64
    }

    /// Screen x of a time of the lane.
    fn x(&self, time: u64) -> f32 {
        let s = &self.scale;
        s.left + ((self.origin as f64 + time as f64 - s.scroll) / s.zoom) as f32
    }

    /// The lane's time under screen x.
    fn time(&self, x: f32) -> u64 {
        let s = &self.scale;
        (s.scroll + f64::from(x - s.left) * s.zoom - self.origin as f64).max(0.0) as u64
    }

    /// A time of the lane pulled as the timeline pulls positions.
    fn snap(&self, time: u64) -> u64 {
        (self.magnet.snap(self.timeline(time)) as i64 - self.origin).max(0) as u64
    }

    fn y(&self, value: f32) -> f32 {
        value_y(self.rect, value, self.info)
    }

    fn value(&self, y: f32) -> f32 {
        let v = y_value(self.rect, y, self.info);
        if matches!(self.info.reading, Reading::Toggle) {
            v.round()
        } else {
            v
        }
    }

    fn at(&self, p: &audiostrada_core::automation::AutomationPoint) -> Pos2 {
        pos2(self.x(p.time), self.y(p.value))
    }

    /// The point, else the curve, under `pos`.
    pub(super) fn hit(&self, lane: &AutomationLane, pos: Pos2) -> Hit {
        let near = lane
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, self.at(p).distance(pos)))
            .filter(|&(_, d)| d < POINT_GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = near {
            return Hit::Point(i);
        }
        // A steep segment is grabbed along its length, wherever the pointer's time falls.
        for (i, pair) in lane.points.windows(2).enumerate() {
            let (a, b) = (self.at(&pair[0]), self.at(&pair[1]));
            let along = b - a;
            if self.steep(lane, i) && along.length_sq() > 0.0 {
                let t = ((pos - a).dot(along) / along.length_sq()).clamp(0.0, 1.0);
                if (a + along * t).distance(pos) <= CURVE_GRAB {
                    return Hit::Curve(i, i + 1);
                }
            }
        }
        let time = self.time(pos.x);
        let Some(value) = lane.value_at(time) else {
            return Hit::Empty;
        };
        if (self.y(value) - pos.y).abs() > CURVE_GRAB {
            return Hit::Empty;
        }
        match lane.points.partition_point(|p| p.time <= time) {
            0 => Hit::Flat(0),
            n if n == lane.points.len() => Hit::Flat(n - 1),
            n => Hit::Curve(n - 1, n),
        }
    }

    /// Whether the segment from point `i` to the next stands more than it lies: it is then
    /// dragged sideways, in time.
    pub(super) fn steep(&self, lane: &AutomationLane, i: usize) -> bool {
        match (lane.points.get(i), lane.points.get(i + 1)) {
            (Some(a), Some(b)) => {
                let (a, b) = (self.at(a), self.at(b));
                (b.x - a.x).abs() < (b.y - a.y).abs()
            }
            _ => false,
        }
    }

    /// `value` drawn to the nearest of `marks` within reach, with the mark that pulled it.
    pub(super) fn pull(&self, value: f32, marks: &[f32]) -> (f32, bool) {
        let y = self.y(value);
        marks
            .iter()
            .map(|&m| (m, (self.y(m) - y).abs()))
            .filter(|&(_, d)| d <= VALUE_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or((value, false), |(m, _)| (m, true))
    }

    /// Values a point is drawn to: the set value, 0 dB or the center, and the values of the
    /// unmoved points beside those moving.
    fn marks(&self, lane: &AutomationLane, moving: &[u64]) -> Vec<f32> {
        let mut marks = self.info.landmarks();
        let moved = |t: u64| moving.binary_search(&t).is_ok();
        for (i, p) in lane.points.iter().enumerate() {
            if moved(p.time) {
                continue;
            }
            let beside = |j: Option<usize>| {
                j.and_then(|j| lane.points.get(j))
                    .is_some_and(|q| moved(q.time))
            };
            if beside(i.checked_sub(1)) || beside(Some(i + 1)) {
                marks.push(p.value);
            }
        }
        marks
    }
}

impl TimelineView {
    /// Mouse editing of one lane, drawn over the lane already painted.
    pub(super) fn lane_editor(&mut self, ui: &mut Ui, doc: &mut Document, view: &LaneView) {
        let area = view.rect.intersect(view.clip);
        if area.height() <= 0.0 || area.width() <= 0.0 {
            return;
        }
        self.lane_rects.push((view.key.clone(), view.rect));
        let key = view.key.clone();
        // Over a clip, the lane takes the pointer only near its points and curve.
        if view.overlay {
            let ours = self.lane_surface == Some(ui.scope_id())
                && self.lane_drag.as_ref().is_some_and(|d| d.key == key);
            let near = ui
                .input(|i| i.pointer.hover_pos())
                .filter(|p| area.contains(*p))
                .zip(key.lane(doc.project()))
                .is_some_and(|(p, lane)| view.hit(lane, p) != Hit::Empty);
            let menu = self.lane_surface == Some(ui.scope_id())
                && self.lane_menu.as_ref().is_some_and(|m| m.0 == key);
            if !ours && !near && !menu {
                self.draw_chosen(&ui.painter().with_clip_rect(area), doc, view);
                return;
            }
        }
        let response = ui.interact(
            area,
            ui.scope_id().with(("automation-lane", format!("{key:?}"))),
            Sense::click_and_drag(),
        );
        let lane = key
            .lane(doc.project())
            .cloned()
            .unwrap_or_else(|| AutomationLane::new(key.target().clone()));
        let modifiers = ui.input(|i| i.modifiers);
        let accent = ui.visuals().selection.bg_fill;
        let painter = ui.painter().with_clip_rect(area);
        let mut guide = None;

        // A gesture under way; one whose lane lost the pointer ends.
        // The same lane can show in the timeline and in the editor: each minds its own.
        let here = ui.scope_id();
        let mine = self.lane_surface == Some(here);
        let ours = mine && self.lane_drag.as_ref().is_some_and(|d| d.key == key);
        if ours && !response.dragged() && !response.drag_stopped() {
            self.lane_drag = None;
            doc.history_mut().end_gesture();
        }
        if let Some(drag) = self.lane_drag.as_mut().filter(|d| mine && d.key == key)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let label = match drag.gesture {
                Gesture::Move { .. } => tr("Move automation point"),
                Gesture::Curve { .. } => tr("Move automation segment"),
                Gesture::Bend { .. } => tr("Bend automation curve"),
                Gesture::Draw { .. } => tr("Draw automation"),
                Gesture::Erase { .. } => tr("Erase automation"),
                Gesture::Select { .. } => "",
            };
            let factor = if modifiers.shift { FINE } else { 1.0 };
            drag.cursor += (pos - drag.last) * factor;
            drag.last = pos;
            let (edited, tag, times, pulled) =
                gesture_step(drag, view, modifiers, pos, doc.project());
            guide = pulled;
            if let Some(edited) = edited {
                doc.history_mut()
                    .edit_merged(label, "automation-gesture", |p| {
                        if let Some(l) = key.lane_mut(p) {
                            *l = edited;
                        }
                    });
            }
            if let Some(times) = times {
                self.chosen = Some(Chosen {
                    key: key.clone(),
                    times,
                });
            }
            if let Some(tag) = tag {
                value_tag(ui.ctx(), pos, tag, accent);
            }
            if let Gesture::Select { .. } = drag.gesture {
                let band = Rect::from_two_pos(drag.origin, pos);
                painter.rect_filled(band, 0.0, accent.gamma_multiply(0.15));
                painter.rect_stroke(band, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
            }
            ui.ctx().set_cursor_icon(match drag.gesture {
                Gesture::Move { .. } => CursorIcon::Grabbing,
                Gesture::Curve { sideways: true, .. } => CursorIcon::ResizeHorizontal,
                Gesture::Curve { .. } | Gesture::Bend { .. } => CursorIcon::ResizeVertical,
                Gesture::Draw { .. } | Gesture::Select { .. } => CursorIcon::Crosshair,
                Gesture::Erase { .. } => CursorIcon::NotAllowed,
            });
        }
        if mine && response.drag_stopped() && self.lane_drag.as_ref().is_some_and(|d| d.key == key)
        {
            self.lane_drag = None;
            doc.history_mut().end_gesture();
        }

        // Hovering: what a press would grab, told by the cursor, a highlight and a hint.
        let hover = response.hover_pos().filter(|_| self.lane_drag.is_none());
        let hit = hover
            .map(|pos| view.hit(&lane, pos))
            .filter(|h| !view.overlay || *h != Hit::Empty);
        if let (Some(pos), Some(hit)) = (hover, hit) {
            let (cursor, value) = match hit {
                Hit::Point(i) => (CursorIcon::Grab, lane.points[i].value),
                Hit::Curve(a, _) if view.steep(&lane, a) => (
                    CursorIcon::ResizeHorizontal,
                    lane.value_at(view.time(pos.x)).unwrap_or_default(),
                ),
                Hit::Curve(..) | Hit::Flat(_) => (
                    CursorIcon::ResizeVertical,
                    lane.value_at(view.time(pos.x)).unwrap_or_default(),
                ),
                Hit::Empty => (CursorIcon::Crosshair, view.value(pos.y)),
            };
            // In the editor, the cursor is the tool's: only what it can do there shows.
            let cursor = match (self.automation.then_some(self.auto_tool), hit) {
                (None, _) => cursor,
                (Some(AutoTool::Pencil), Hit::Point(_)) => CursorIcon::Grab,
                (Some(AutoTool::Pencil), Hit::Curve(..)) if modifiers.alt => cursor,
                (Some(AutoTool::Pencil), _) => CursorIcon::Crosshair,
                (Some(AutoTool::Select), Hit::Empty) => CursorIcon::Default,
                (Some(AutoTool::Select), _) => cursor,
                (Some(AutoTool::Erase), Hit::Point(_)) => CursorIcon::NotAllowed,
                (Some(_), _) => CursorIcon::Default,
            };
            ui.ctx().set_cursor_icon(cursor);
            value_tag(ui.ctx(), pos, view.info.compared(value), accent);
            hover_marks(&painter, view, &lane, hit, pos, modifiers.alt, accent);
            // The editor tells what its tool does in its tool bar.
            if !self.automation {
                lane_hint(&painter, view.rect.intersect(view.clip), hit, ui.visuals());
            }
        }

        // Starting a gesture where the button went down.
        if response.drag_started()
            && let Some(origin) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            self.lane_surface = Some(here);
            self.start_gesture(view, &lane, origin, modifiers);
        }

        // Clicks.
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.lane_click(doc, view, &lane, pos, modifiers);
        }
        if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(lane) = key.lane(doc.project())
            && let Hit::Point(i) = view.hit(lane, pos)
        {
            self.open_value_edit(&key, lane.points[i].time, pos, view.info);
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let point = match view.hit(&lane, pos) {
                Hit::Point(i) => Some(lane.points[i].time),
                _ => None,
            };
            if let Some(t) = point
                && !self.chosen_in(&key).contains(&t)
            {
                self.chosen = Some(Chosen {
                    key: key.clone(),
                    times: vec![t],
                });
            }
            self.lane_surface = Some(here);
            self.lane_menu = Some((key.clone(), point, pos));
        }
        let open = response
            .context_menu(|ui| self.lane_context_menu(ui, doc, view))
            .is_some();
        if !open
            && self.lane_surface == Some(here)
            && !response.secondary_clicked()
            && self.lane_menu.as_ref().is_some_and(|m| m.0 == key)
        {
            self.lane_menu = None;
        }

        // The chosen points, and the value a moved point was drawn to.
        self.draw_chosen(&painter, doc, view);
        if let Some(value) = guide {
            let y = view.y(value);
            painter.extend(Shape::dashed_line(
                &[pos2(area.left(), y), pos2(area.right(), y)],
                Stroke::new(1.0, accent),
                6.0,
                3.0,
            ));
        }
        self.value_editor(ui, doc, view);
    }

    /// The chosen points of a lane, outlined.
    fn draw_chosen(&self, painter: &egui::Painter, doc: &Document, view: &LaneView) {
        let chosen = self.chosen_in(&view.key);
        let Some(lane) = view.key.lane(doc.project()) else {
            return;
        };
        let accent = painter.ctx().global_style().visuals.selection.bg_fill;
        for p in lane
            .points
            .iter()
            .filter(|p| chosen.binary_search(&p.time).is_ok())
        {
            painter.circle_filled(view.at(p), 5.5, accent);
            painter.circle_filled(view.at(p), 3.6, Color32::WHITE);
        }
    }

    /// The points chosen in `key`'s lane.
    pub(super) fn chosen_in(&self, key: &LaneKey) -> Vec<u64> {
        self.chosen
            .as_ref()
            .filter(|c| &c.key == key)
            .map(|c| c.times.clone())
            .unwrap_or_default()
    }

    fn start_gesture(
        &mut self,
        view: &LaneView,
        lane: &AutomationLane,
        origin: Pos2,
        modifiers: egui::Modifiers,
    ) {
        let key = view.key.clone();
        if view.overlay && view.hit(lane, origin) == Hit::Empty {
            return;
        }
        let mut chosen = self.chosen_in(&key);
        chosen.retain(|&t| lane.index_at(t).is_some());
        let mut original = lane.clone();
        // In the automation editor, its tool decides.
        let tool = self.automation.then_some(self.auto_tool);
        let gesture = match view.hit(lane, origin) {
            _ if tool == Some(AutoTool::Erase) => Gesture::Erase { last: origin },
            // The pencil draws wherever it starts, but on a point, which it moves, and with
            // Alt on a segment, which it bends.
            hit if tool == Some(AutoTool::Pencil)
                && !matches!(hit, Hit::Point(_))
                && !(modifiers.alt && matches!(hit, Hit::Curve(..))) =>
            {
                Gesture::Draw {
                    drawn: BTreeMap::new(),
                    last: f32::NEG_INFINITY,
                }
            }
            Hit::Empty if tool == Some(AutoTool::Select) => Gesture::Select {
                kept: if modifiers.shift { chosen } else { Vec::new() },
            },
            Hit::Point(i) => {
                let t = lane.points[i].time;
                let times = if chosen.contains(&t) { chosen } else { vec![t] };
                Gesture::Move { grabbed: t, times }
            }
            // With the pencil, Alt moves the segment; elsewhere it bends it.
            Hit::Curve(a, _) if modifiers.alt && tool != Some(AutoTool::Pencil) => Gesture::Bend {
                at: lane.points[a].time,
            },
            hit @ (Hit::Curve(..) | Hit::Flat(_)) => {
                let ends: Vec<u64> = match hit {
                    Hit::Curve(a, b) => vec![lane.points[a].time, lane.points[b].time],
                    Hit::Flat(a) => vec![lane.points[a].time],
                    _ => Vec::new(),
                };
                let times = if ends.iter().all(|t| chosen.contains(t)) {
                    chosen
                } else {
                    ends
                };
                let reference = lane.value_at(view.time(origin.x)).unwrap_or_default();
                let sideways = matches!(hit, Hit::Curve(a, _) if view.steep(lane, a));
                Gesture::Curve {
                    times,
                    reference,
                    sideways,
                }
            }
            Hit::Empty if modifiers.command => Gesture::Draw {
                drawn: BTreeMap::new(),
                last: f32::NEG_INFINITY,
            },
            Hit::Empty if modifiers.shift || self.tool == Tool::Range => Gesture::Select {
                kept: if modifiers.shift { chosen } else { Vec::new() },
            },
            // Pressed on nothing: a new point, dragged at once.
            Hit::Empty => {
                let time = view.snap(view.time(origin.x));
                let marks = view.marks(lane, &[]);
                let (value, _) = view.pull(view.value(origin.y), &marks);
                original.add(time, value, CurveShape::Linear);
                let at = pos2(view.x(time), view.y(value));
                self.lane_drag = Some(LaneDrag {
                    key: key.clone(),
                    original,
                    gesture: Gesture::Move {
                        grabbed: time,
                        times: vec![time],
                    },
                    origin: at,
                    cursor: at,
                    last: origin,
                });
                self.chosen = Some(Chosen {
                    key,
                    times: vec![time],
                });
                return;
            }
        };
        match &gesture {
            Gesture::Move { times, .. } | Gesture::Curve { times, .. } => {
                self.chosen = Some(Chosen {
                    key: key.clone(),
                    times: times.clone(),
                });
            }
            Gesture::Draw { .. } => self.chosen = None,
            Gesture::Bend { .. } | Gesture::Select { .. } | Gesture::Erase { .. } => {}
        }
        self.lane_drag = Some(LaneDrag {
            key,
            original,
            gesture,
            origin,
            cursor: origin,
            last: origin,
        });
    }

    fn lane_click(
        &mut self,
        doc: &mut Document,
        view: &LaneView,
        lane: &AutomationLane,
        pos: Pos2,
        modifiers: egui::Modifiers,
    ) {
        let key = view.key.clone();
        if view.overlay && view.hit(lane, pos) == Hit::Empty {
            return;
        }
        let edit = |doc: &mut Document, label: &str, f: &mut dyn FnMut(&mut AutomationLane)| {
            doc.history_mut().edit(label, |p| {
                if let Some(l) = key.lane_mut(p) {
                    f(l);
                }
            });
        };
        let tool = self.automation.then_some(self.auto_tool);
        match view.hit(lane, pos) {
            // The selection tool only selects: a click away from the points clears it.
            Hit::Curve(..) | Hit::Flat(_) | Hit::Empty if tool == Some(AutoTool::Select) => {
                self.chosen = None;
            }
            Hit::Curve(..) | Hit::Flat(_) | Hit::Empty if tool == Some(AutoTool::Erase) => {}
            Hit::Point(i) => {
                let t = lane.points[i].time;
                if modifiers.alt || tool == Some(AutoTool::Erase) {
                    edit(doc, tr("Delete automation point"), &mut |l| {
                        l.remove_times(&[t])
                    });
                    self.chosen = None;
                    return;
                }
                let mut times = self.chosen_in(&key);
                if modifiers.shift {
                    match times.binary_search(&t) {
                        Ok(k) => {
                            times.remove(k);
                        }
                        Err(k) => times.insert(k, t),
                    }
                } else {
                    times = vec![t];
                }
                self.chosen = Some(Chosen { key, times });
            }
            Hit::Curve(..) | Hit::Flat(_) if !modifiers.alt => {
                let time = view.snap(view.time(pos.x));
                edit(doc, tr("Add automation point"), &mut |l| {
                    l.insert_on_curve(time);
                });
                self.chosen = Some(Chosen {
                    key,
                    times: vec![time],
                });
            }
            Hit::Empty if self.tool == Tool::Range || modifiers.shift => {
                self.chosen = None;
            }
            Hit::Empty => {
                let time = view.snap(view.time(pos.x));
                let (value, _) = view.pull(view.value(pos.y), &view.marks(lane, &[]));
                edit(doc, tr("Add automation point"), &mut |l| {
                    l.add(time, value, CurveShape::Linear)
                });
                self.chosen = Some(Chosen {
                    key,
                    times: vec![time],
                });
            }
            Hit::Curve(..) | Hit::Flat(_) => {}
        }
    }

    fn open_value_edit(&mut self, key: &LaneKey, time: u64, at: Pos2, info: &LaneInfo) {
        let text = self
            .doc
            .try_borrow()
            .ok()
            .and_then(|d| {
                key.lane(d.project())?
                    .points
                    .iter()
                    .find(|p| p.time == time)
                    .copied()
            })
            .map(|p| info.text(p.value))
            .unwrap_or_default();
        self.value_edit = Some(ValueEdit {
            key: key.clone(),
            time,
            at,
            text,
            focused: false,
        });
    }

    /// A field over a point to type its value: Enter sets it, Escape or a click away closes.
    fn value_editor(&mut self, ui: &mut Ui, doc: &mut Document, view: &LaneView) {
        let Some(edit) = self.value_edit.as_mut().filter(|e| e.key == view.key) else {
            return;
        };
        let ready = edit.focused;
        let mut done = false;
        let mut apply = None;
        let area = egui::Area::new(egui::Id::unique("automation-value"))
            .order(egui::Order::Foreground)
            .fixed_pos(edit.at + vec2(10.0, 10.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(tr("Value of the point")).weak());
                    let field =
                        ui.add(egui::TextEdit::singleline(&mut edit.text).desired_width(110.0));
                    if !edit.focused {
                        field.request_focus();
                        edit.focused = true;
                    }
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if field.lost_focus() && enter {
                        apply = view.info.parse(&edit.text);
                        done = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = true;
                    }
                    if !edit.text.is_empty() && view.info.parse(&edit.text).is_none() {
                        ui.label(
                            egui::RichText::new(tr("Not a value of this lane"))
                                .small()
                                .color(crate::settings::WARN),
                        );
                    }
                });
            });
        if ready && area.response.clicked_elsewhere() {
            done = true;
        }
        if let Some(value) = apply {
            let (key, time) = (edit.key.clone(), edit.time);
            doc.history_mut().edit(tr("Automation point value"), |p| {
                if let Some(l) = key.lane_mut(p)
                    && let Some(i) = l.index_at(time)
                {
                    l.points[i].value = value;
                }
            });
        }
        if done {
            self.value_edit = None;
        }
    }

    fn lane_context_menu(&mut self, ui: &mut Ui, doc: &mut Document, view: &LaneView) {
        let Some((key, point, at)) = self.lane_menu.clone().filter(|m| m.0 == view.key) else {
            ui.close();
            return;
        };
        let chosen = self.chosen_in(&key);
        let edit = |doc: &mut Document, label: &str, f: &mut dyn FnMut(&mut AutomationLane)| {
            doc.history_mut().edit(label, |p| {
                if let Some(l) = key.lane_mut(p) {
                    f(l);
                }
            });
        };
        if point.is_some() {
            ui.label(egui::RichText::new(tr("Curve to the next point")).weak());
            for (shape, name) in [
                (CurveShape::Linear, tr("Linear")),
                (CurveShape::Smooth, tr("Smooth")),
                (CurveShape::Step, tr("Step")),
            ] {
                if ui.button(name).clicked() {
                    edit(doc, tr("Automation curve"), &mut |l| {
                        for p in l.points.iter_mut().filter(|p| chosen.contains(&p.time)) {
                            p.shape = shape;
                            p.tension = 0.0;
                        }
                    });
                    ui.close();
                }
            }
            ui.separator();
            if let Some(t) = point
                && ui.button(tr("Type a value…")).clicked()
            {
                self.open_value_edit(&key, t, at, view.info);
                ui.close();
            }
            let label = if chosen.len() > 1 {
                tr("Delete the selected points")
            } else {
                tr("Delete point")
            };
            if ui.button(label).clicked() {
                edit(doc, tr("Delete automation point"), &mut |l| {
                    l.remove_times(&chosen)
                });
                self.chosen = None;
                ui.close();
            }
            return;
        }
        let all: Vec<u64> = key
            .lane(doc.project())
            .map(|l| l.points.iter().map(|p| p.time).collect())
            .unwrap_or_default();
        if ui
            .add_enabled(!all.is_empty(), egui::Button::new(tr("Select every point")))
            .clicked()
        {
            self.chosen = Some(Chosen {
                key: key.clone(),
                times: all.clone(),
            });
            ui.close();
        }
        if ui
            .add_enabled(
                !chosen.is_empty(),
                egui::Button::new(tr("Delete the selected points")),
            )
            .clicked()
        {
            edit(doc, tr("Delete automation point"), &mut |l| {
                l.remove_times(&chosen)
            });
            self.chosen = None;
            ui.close();
        }
        if ui
            .add_enabled(!all.is_empty(), egui::Button::new(tr("Delete every point")))
            .clicked()
        {
            edit(doc, tr("Clear automation lane"), &mut |l| l.points.clear());
            self.chosen = None;
            ui.close();
        }
    }

    /// Does what the automation tools asked for.
    pub(super) fn run_lane_command(&mut self, doc: &mut Document) {
        match self.lane_command.take() {
            Some(LaneCommand::Delete) => {
                self.delete_chosen_points(doc);
            }
            Some(LaneCommand::Shape(shape)) => {
                if let Some(chosen) = self.chosen.clone() {
                    doc.history_mut().edit(tr("Automation curve"), |p| {
                        if let Some(l) = chosen.key.lane_mut(p) {
                            for point in l
                                .points
                                .iter_mut()
                                .filter(|point| chosen.times.binary_search(&point.time).is_ok())
                            {
                                point.shape = shape;
                                point.tension = 0.0;
                            }
                        }
                    });
                }
            }
            None => {}
        }
    }

    /// The curve a track shows in the automation mode.
    pub(super) fn lane_choice(&self, track: &Track) -> AutomationTarget {
        self.lane_choice
            .get(&track.id)
            .cloned()
            .unwrap_or(AutomationTarget::Gain)
    }

    /// Deletes the chosen points; false when none are chosen.
    pub(super) fn delete_chosen_points(&mut self, doc: &mut Document) -> bool {
        let Some(chosen) = self.chosen.take().filter(|c| !c.times.is_empty()) else {
            return false;
        };
        doc.history_mut().edit(tr("Delete automation point"), |p| {
            if let Some(l) = chosen.key.lane_mut(p) {
                l.remove_times(&chosen.times);
            }
        });
        true
    }

    /// Chooses every point of the lane holding chosen points; false when none are chosen.
    pub(super) fn choose_every_point(&mut self, doc: &Document) -> bool {
        let Some(chosen) = &mut self.chosen else {
            return false;
        };
        chosen.times = chosen
            .key
            .lane(doc.project())
            .map(|l| l.points.iter().map(|p| p.time).collect())
            .unwrap_or_default();
        true
    }
}

/// One frame of a gesture: the lane as edited (when it changed), the tag beside the pointer,
/// the chosen points' new times and the value a moved point was drawn to.
fn gesture_step(
    drag: &mut LaneDrag,
    view: &LaneView,
    modifiers: egui::Modifiers,
    pos: Pos2,
    project: &Project,
) -> (
    Option<AutomationLane>,
    Option<String>,
    Option<Vec<u64>>,
    Option<f32>,
) {
    // The eraser takes the points it passes over, for good.
    if let Gesture::Erase { last } = &mut drag.gesture {
        let (a, b) = (*last, pos);
        *last = pos;
        let before = drag.original.points.len();
        drag.original.points.retain(|p| {
            let at = view.at(p);
            let along = b - a;
            let t = if along.length_sq() > 0.0 {
                ((at - a).dot(along) / along.length_sq()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (a + along * t).distance(at) > POINT_GRAB
        });
        let changed = drag.original.points.len() != before;
        return (changed.then(|| drag.original.clone()), None, None, None);
    }
    let info = view.info;
    let fine = modifiers.shift;
    let dy = drag.cursor.y - drag.origin.y;
    let original = &drag.original;
    // Values follow the pointer through the lane's scale, all by the same share.
    let shifted = |from: f32, to: f32| {
        let delta = info.share(to) - info.share(from);
        let toggle = matches!(info.reading, Reading::Toggle);
        move |v: f32| {
            let v = info.at_share((info.share(v) + delta).clamp(0.0, 1.0));
            if toggle { v.round() } else { v }
        }
    };
    match &mut drag.gesture {
        Gesture::Move { grabbed, times } => {
            let Some(p) = original.index_at(*grabbed).map(|i| original.points[i]) else {
                return (None, None, None, None);
            };
            let mut target = view.value(view.y(p.value) + dy);
            let mut pulled = None;
            if !fine {
                let (v, hit) = view.pull(target, &view.marks(original, times));
                if hit {
                    pulled = Some(v);
                }
                target = v;
            }
            let x = view.x(p.time) + (drag.cursor.x - drag.origin.x);
            let mut time = view.time(x);
            if !fine {
                time = view.snap(time);
            }
            if modifiers.command {
                time = p.time;
            }
            let mut lane = original.clone();
            let moved = lane.shift(times, time as i64 - p.time as i64, shifted(p.value, target));
            let at = times
                .iter()
                .position(|t| t == grabbed)
                .and_then(|k| moved.get(k).copied())
                .unwrap_or(time);
            let value = lane.index_at(at).map_or(target, |i| lane.points[i].value);
            let tag = format!(
                "{} · {}",
                info.compared(value),
                bars(project, view.timeline(at))
            );
            (Some(lane), Some(tag), Some(moved), pulled)
        }
        Gesture::Curve {
            times,
            sideways: true,
            ..
        } => {
            // In time, by the drag, its first point pulled as positions are.
            let Some(&first) = times.first() else {
                return (None, None, None, None);
            };
            let mut time = view.time(view.x(first) + (drag.cursor.x - drag.origin.x));
            if !fine {
                time = view.snap(time);
            }
            let mut lane = original.clone();
            let moved = lane.shift(times, time as i64 - first as i64, |v| v);
            let at = moved.first().copied().unwrap_or(time);
            (
                Some(lane),
                Some(bars(project, view.timeline(at))),
                Some(moved),
                None,
            )
        }
        Gesture::Curve {
            times, reference, ..
        } => {
            let mut target = view.value(view.y(*reference) + dy);
            let mut pulled = None;
            if !fine {
                let (v, hit) = view.pull(target, &view.marks(original, times));
                if hit {
                    pulled = Some(v);
                }
                target = v;
            }
            let mut lane = original.clone();
            let moved = lane.shift(times, 0, shifted(*reference, target));
            (Some(lane), Some(info.compared(target)), Some(moved), pulled)
        }
        Gesture::Bend { at } => {
            let Some(i) = original.index_at(*at) else {
                return (None, None, None, None);
            };
            let (a, b) = (original.points[i], original.points[i + 1]);
            let rising = b.value > a.value;
            let start = if a.shape == CurveShape::Linear {
                a.tension
            } else {
                0.0
            };
            let delta = dy / BEND_TRAVEL;
            let mut tension = (start + if rising { delta } else { -delta }).clamp(-1.0, 1.0);
            // Straight pulls when near.
            if !fine && tension.abs() < 0.04 {
                tension = 0.0;
            }
            let mut lane = original.clone();
            lane.points[i].shape = CurveShape::Linear;
            lane.points[i].tension = tension;
            let middle = lane.value_at((a.time + b.time) / 2).unwrap_or(a.value);
            let tag = if tension == 0.0 {
                tr("Straight").to_owned()
            } else {
                info.compared(middle)
            };
            (Some(lane), Some(tag), None, None)
        }
        Gesture::Draw { drawn, last } => {
            if (pos.x - *last).abs() < DRAW_STEP {
                return (None, None, None, None);
            }
            *last = pos.x;
            let value = view.value(pos.y);
            drawn.insert(view.time(pos.x), value);
            let points: Vec<(u64, f32)> = drawn.iter().map(|(&t, &v)| (t, v)).collect();
            let mut lane = original.clone();
            lane.draw(&points);
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                lane.thin(first.0, last.0, (info.max - info.min).abs() * 0.002);
            }
            (Some(lane), Some(info.compared(value)), None, None)
        }
        Gesture::Erase { .. } => (None, None, None, None),
        Gesture::Select { kept } => {
            let band = Rect::from_two_pos(drag.origin, pos);
            let mut times = kept.clone();
            for p in &original.points {
                if band.contains(view.at(p)) && times.binary_search(&p.time).is_err() {
                    let k = times.partition_point(|&t| t < p.time);
                    times.insert(k, p.time);
                }
            }
            (None, None, Some(times), None)
        }
    }
}

/// The point or curve a press would grab, outlined.
fn hover_marks(
    painter: &egui::Painter,
    view: &LaneView,
    lane: &AutomationLane,
    hit: Hit,
    pos: Pos2,
    alt: bool,
    accent: Color32,
) {
    let x = |i: usize| view.x(lane.points[i].time);
    let (x0, x1) = match hit {
        Hit::Point(i) => {
            painter.circle_stroke(view.at(&lane.points[i]), 7.0, Stroke::new(1.5, accent));
            return;
        }
        Hit::Empty => return,
        Hit::Curve(a, b) => (x(a), x(b)),
        Hit::Flat(i) if pos.x < x(i) => (view.clip.left(), x(i)),
        Hit::Flat(i) => (x(i), view.clip.right()),
    };
    let mut line = Vec::new();
    let mut at = x0.max(view.clip.left());
    while at <= x1.min(view.clip.right()) + 1.0 {
        if let Some(v) = lane.value_at(view.time(at)) {
            line.push(pos2(at, view.y(v)));
        }
        at += 2.0;
    }
    painter.add(Shape::line(
        line,
        Stroke::new(3.0, accent.gamma_multiply(0.8)),
    ));
    // Alt: the handle bending the curve, at its middle.
    if alt && let Hit::Curve(a, b) = hit {
        let middle = (lane.points[a].time + lane.points[b].time) / 2;
        if let Some(v) = lane.value_at(middle) {
            painter.circle_filled(pos2(view.x(middle), view.y(v)), 5.0, accent);
        }
    }
}

/// What the mouse does over a lane, told at its top right while the pointer is over it.
fn lane_hint(painter: &egui::Painter, area: Rect, hit: Hit, visuals: &egui::Visuals) {
    let text = match hit {
        Hit::Point(_) => tr(
            "Drag to move (Shift: finely, Ctrl: value only) · Alt+click deletes · Double-click to type a value",
        ),
        Hit::Curve(..) => {
            tr("Click adds a point on the curve · Drag moves this segment · Alt+drag bends it")
        }
        Hit::Flat(_) => tr("Click adds a point on the line · Drag moves the line"),
        Hit::Empty => tr("Click adds a point · Ctrl+drag draws · Shift+drag selects points"),
    };
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::proportional(10.0),
        visuals.weak_text_color(),
    );
    if galley.size().x + 160.0 > area.width() || area.height() < 30.0 {
        return;
    }
    let at = pos2(area.right() - galley.size().x - 8.0, area.top() + 3.0);
    painter.rect_filled(
        Rect::from_min_size(at - vec2(4.0, 1.0), galley.size() + vec2(8.0, 2.0)),
        3.0,
        visuals.extreme_bg_color.gamma_multiply(0.85),
    );
    painter.galley(at, galley, visuals.weak_text_color());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_tests::Harness;
    use eframe::egui::{Event, Modifiers, PointerButton};

    fn press(h: &mut Harness, at: Pos2, pressed: bool) {
        let modifiers = h.modifiers;
        h.frame(vec![Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        }]);
    }

    fn click(h: &mut Harness, at: Pos2) {
        h.frame(vec![Event::PointerMoved(at)]);
        press(h, at, true);
        press(h, at, false);
        h.frame(Vec::new());
    }

    fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
        h.frame(vec![Event::PointerMoved(from)]);
        press(h, from, true);
        for k in 1..=8 {
            h.frame(vec![Event::PointerMoved(
                from + (to - from) * (k as f32 / 8.0),
            )]);
        }
        press(h, to, false);
        h.frame(Vec::new());
    }

    fn points(h: &Harness, key: &LaneKey) -> Vec<audiostrada_core::automation::AutomationPoint> {
        key.lane(h.app.doc.borrow().project())
            .map(|l| l.points.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_lane_is_drawn_point_by_point_and_its_segments_move_and_bend() {
        let mut h = Harness::with_song("automation-lanes");
        let track = h.app.doc.borrow().project().tracks[0].id;
        h.app.doc.borrow_mut().history_mut().modify_unrecorded(|p| {
            let t = p.track_mut(track).unwrap();
            t.show_automation = true;
            t.gain_db = 0.0;
            audiostrada_core::automation::lane_mut(t, &AutomationTarget::Gain);
        });
        h.app.views.borrow_mut().edit = EditSettings::default();
        // The master's lane is edited in place; the tracks' open the editor.
        h.app.views.borrow_mut().timeline.show_master = true;
        for _ in 0..4 {
            h.frame(Vec::new());
        }
        let key = LaneKey::Master(AutomationTarget::Gain);
        let rect = h
            .app
            .views
            .borrow()
            .timeline
            .lane_rects
            .iter()
            .find(|r| r.0 == key)
            .map(|r| r.1)
            .expect("the volume lane is on screen");
        assert!(rect.height() >= LANE_HEIGHT, "lanes are roomy: {rect:?}");
        let info = h
            .app
            .views
            .borrow()
            .timeline
            .lane_info
            .values()
            .next()
            .cloned()
            .unwrap();
        let y = |db: f32| value_y(rect, db, &info);

        // Four points a little off 0 dB land on it, the set value pulling them.
        for k in 0..4 {
            click(
                &mut h,
                pos2(rect.left() + 80.0 + 100.0 * k as f32, y(0.0) + 3.0),
            );
        }
        let before = points(&h, &key);
        assert_eq!(before.len(), 4);
        assert!(before.iter().all(|p| p.value == 0.0), "{before:?}");

        // The segment between the middle points goes down with them, in time where they were.
        let middle = pos2(rect.left() + 230.0, y(0.0));
        drag(&mut h, middle, middle + vec2(0.0, 25.0));
        let after = points(&h, &key);
        assert_eq!(after.len(), 4);
        assert_eq!(after[0].value, 0.0);
        assert_eq!(after[3].value, 0.0);
        assert!(
            after[1].value < -3.0 && after[1].value == after[2].value,
            "{after:?}"
        );
        assert_eq!(
            after.iter().map(|p| p.time).collect::<Vec<_>>(),
            before.iter().map(|p| p.time).collect::<Vec<_>>()
        );

        // Alt bends the curve between the first two points.
        h.modifiers = Modifiers::ALT;
        let from = (before[0].time + before[1].time) / 2;
        let scale_x = |t: u64| {
            rect.left()
                + 80.0
                + (t - before[0].time) as f32 / (before[1].time - before[0].time) as f32 * 100.0
        };
        let on_curve = audiostrada_core::automation::value_at(&after, from).unwrap();
        let at = pos2(scale_x(from), y(on_curve));
        drag(&mut h, at, at + vec2(0.0, -40.0));
        h.modifiers = Modifiers::NONE;
        let bent = points(&h, &key);
        assert!(bent[0].tension != 0.0, "{bent:?}");
        assert!(
            audiostrada_core::automation::value_at(&bent, from).unwrap() > on_curve,
            "a falling curve bowed up stays high, then falls late"
        );

        // A point grabbed off its center does not jump; Shift moves it finely.
        let p = bent[3];
        let grab = pos2(scale_x(p.time) + 4.0, y(p.value) + 4.0);
        h.modifiers = Modifiers::SHIFT;
        drag(&mut h, grab, grab + vec2(0.0, 60.0));
        h.modifiers = Modifiers::NONE;
        let fine = points(&h, &key)[3];
        assert!(fine.value < -1.0 && fine.value > -6.0, "{fine:?}");

        // Ctrl draws freehand, replacing what lay under the stroke.
        h.modifiers = Modifiers::COMMAND;
        drag(
            &mut h,
            pos2(rect.left() + 500.0, y(-20.0)),
            pos2(rect.left() + 700.0, y(6.0)),
        );
        h.modifiers = Modifiers::NONE;
        let drawn = points(&h, &key);
        assert!(drawn.len() >= 6, "{drawn:?}");

        // Shift chooses points with a rectangle; Delete removes them.
        h.modifiers = Modifiers::SHIFT;
        drag(
            &mut h,
            pos2(rect.left() + 490.0, rect.top() + 2.0),
            pos2(rect.left() + 710.0, rect.bottom() - 2.0),
        );
        h.modifiers = Modifiers::NONE;
        h.act(crate::actions::Action::Delete);
        assert_eq!(points(&h, &key).len(), 4);

        // Alt+click deletes a point.
        let last = points(&h, &key)[3];
        h.modifiers = Modifiers::ALT;
        click(&mut h, pos2(scale_x(last.time), y(last.value)));
        h.modifiers = Modifiers::NONE;
        assert_eq!(points(&h, &key).len(), 3);
    }
}
