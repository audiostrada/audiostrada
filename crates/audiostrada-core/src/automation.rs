//! Automation: lanes of timed values for a track's controls, how they are read, and the edits
//! made by drawing or by writing them during playback.

use serde::{Deserialize, Serialize};

use crate::model::{InsertId, Track, TrackId};

/// What a lane controls. Values are in the control's own unit (dB, -1..1 pan, 0/1, plugin value).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AutomationTarget {
    Gain,
    Pan,
    Mute,
    /// Level of the send to this track.
    Send(TrackId),
    /// A parameter of an insert (built-in id, or plugin parameter number as text).
    Param {
        insert: InsertId,
        param: String,
    },
}

impl AutomationTarget {
    /// True for a parameter of the insert `id`.
    pub fn is_param_of(&self, id: InsertId) -> bool {
        matches!(self, AutomationTarget::Param { insert, .. } if *insert == id)
    }
}

/// How the curve goes from a point to the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CurveShape {
    #[default]
    Linear,
    /// Holds the value until the next point.
    Step,
    /// Eases in and out (cosine).
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    /// Project samples.
    pub time: u64,
    pub value: f32,
    #[serde(default)]
    pub shape: CurveShape,
    /// Bend of a linear curve to the next point, -1 to 1 (see [`bend`]).
    #[serde(default, skip_serializing_if = "crate::model::is_zero")]
    pub tension: f32,
}

impl AutomationPoint {
    pub fn new(time: u64, value: f32, shape: CurveShape) -> Self {
        Self {
            time,
            value,
            shape,
            tension: 0.0,
        }
    }
}

/// Steepness of the most bent curve.
const BEND: f32 = 10.0;

/// Share of the way from a point to the next at `x` (0 to 1) along a curve bent by `tension`:
/// straight at 0, rising late above 0, early below.
pub fn bend(x: f32, tension: f32) -> f32 {
    let k = tension.clamp(-1.0, 1.0) * BEND;
    if k.abs() < 1e-3 {
        return x;
    }
    (k * x).exp_m1() / k.exp_m1()
}

/// The tension of a curve passing `share` of the way at its middle.
pub fn tension_through(share: f32) -> f32 {
    let m = share.clamp(0.005, 0.995);
    (2.0 * (1.0 / m - 1.0).ln() / BEND).clamp(-1.0, 1.0)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub target: AutomationTarget,
    /// Sorted by time, at most one point per time.
    pub points: Vec<AutomationPoint>,
    /// Played back (a lane can be kept but switched off).
    #[serde(default = "crate::model::yes")]
    pub enabled: bool,
    /// Shown under its track in the editor.
    #[serde(default)]
    pub visible: bool,
    /// Height in the editor, relative to the usual one.
    #[serde(
        default = "crate::model::one",
        skip_serializing_if = "crate::model::is_one"
    )]
    pub height: f32,
}

/// How a track's automation reacts to playback and to its controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutomationMode {
    /// Automation is ignored.
    Off,
    /// Automation plays; moving a control overrides it only while held.
    #[default]
    Read,
    /// Writes while a control is held, then returns to the existing automation.
    Touch,
    /// Writes from the first touch until playback stops.
    Latch,
    /// Writes every automated control during the whole playback.
    Write,
    /// Moving a control offsets the existing automation instead of replacing it.
    Trim,
}

impl AutomationMode {
    pub const ALL: [AutomationMode; 6] = [
        AutomationMode::Off,
        AutomationMode::Read,
        AutomationMode::Touch,
        AutomationMode::Latch,
        AutomationMode::Write,
        AutomationMode::Trim,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AutomationMode::Off => "Off",
            AutomationMode::Read => "Read",
            AutomationMode::Touch => "Touch",
            AutomationMode::Latch => "Latch",
            AutomationMode::Write => "Write",
            AutomationMode::Trim => "Trim",
        }
    }

    /// True if touching a control during playback records automation.
    pub fn writes(self) -> bool {
        matches!(
            self,
            AutomationMode::Touch
                | AutomationMode::Latch
                | AutomationMode::Write
                | AutomationMode::Trim
        )
    }
}

/// Value of a curve at `time`, or `None` without points.
pub fn value_at(points: &[AutomationPoint], time: u64) -> Option<f32> {
    let first = points.first()?;
    if time <= first.time {
        return Some(first.value);
    }
    let i = points.partition_point(|p| p.time <= time);
    let a = points[i - 1];
    let Some(b) = points.get(i) else {
        return Some(a.value);
    };
    let x = (time - a.time) as f32 / (b.time - a.time).max(1) as f32;
    Some(match a.shape {
        CurveShape::Step => a.value,
        CurveShape::Linear => a.value + (b.value - a.value) * bend(x, a.tension),
        CurveShape::Smooth => {
            let eased = (1.0 - (x * std::f32::consts::PI).cos()) / 2.0;
            a.value + (b.value - a.value) * eased
        }
    })
}

/// Two curves added: `own` (or `fixed` without one) plus `other`, as linear points. Where
/// either bends (step or smooth), points every `step` samples follow it.
pub fn sum(
    own: Option<&[AutomationPoint]>,
    fixed: f32,
    other: &[AutomationPoint],
    step: u64,
) -> Vec<AutomationPoint> {
    let mut times: Vec<u64> = other
        .iter()
        .chain(own.unwrap_or(&[]))
        .map(|p| p.time)
        .collect();
    times.sort_unstable();
    times.dedup();
    let bends = |points: &[AutomationPoint], t: u64| {
        let i = points.partition_point(|p| p.time <= t);
        i > 0 && i < points.len() && {
            let p = points[i - 1];
            p.shape != CurveShape::Linear || p.tension != 0.0
        }
    };
    let mut all = Vec::with_capacity(times.len());
    for (k, &t) in times.iter().enumerate() {
        all.push(t);
        if let Some(&next) = times.get(k + 1)
            && (bends(other, t) || own.is_some_and(|o| bends(o, t)))
        {
            let mut at = t + step.max(1);
            while at < next {
                all.push(at);
                at += step.max(1);
            }
        }
    }
    all.into_iter()
        .map(|time| {
            let value = own.and_then(|o| value_at(o, time)).unwrap_or(fixed)
                + value_at(other, time).unwrap_or(0.0);
            AutomationPoint::new(time, value, CurveShape::Linear)
        })
        .collect()
}

impl AutomationLane {
    pub fn new(target: AutomationTarget) -> Self {
        Self {
            target,
            points: Vec::new(),
            enabled: true,
            visible: true,
            height: 1.0,
        }
    }

    pub fn value_at(&self, time: u64) -> Option<f32> {
        value_at(&self.points, time)
    }

    /// Adds a point, replacing one at the same time.
    pub fn add(&mut self, time: u64, value: f32, shape: CurveShape) {
        let point = AutomationPoint::new(time, value, shape);
        match self.points.binary_search_by_key(&time, |p| p.time) {
            Ok(i) => self.points[i] = point,
            Err(i) => self.points.insert(i, point),
        }
    }

    /// Removes the points in `start..=end`.
    pub fn remove_range(&mut self, start: u64, end: u64) {
        self.points.retain(|p| p.time < start || p.time > end);
    }

    /// Moves a point, keeping the order; returns its new index.
    pub fn move_point(&mut self, index: usize, time: u64, value: f32) -> usize {
        if index >= self.points.len() {
            return index;
        }
        let mut point = self.points.remove(index);
        point.time = time;
        point.value = value;
        self.points.retain(|p| p.time != time);
        let i = self.points.partition_point(|p| p.time < time);
        self.points.insert(i, point);
        i
    }

    /// Writes `value` over `(from, to]`: points in between are replaced by one at `to`.
    pub fn write(&mut self, from: u64, to: u64, value: f32) {
        if to > from {
            self.points.retain(|p| p.time <= from || p.time > to);
        }
        self.add(to, value, CurveShape::Linear);
    }

    /// Adds `delta` to every point in `start..=end`, within `min..=max`.
    pub fn trim(&mut self, start: u64, end: u64, delta: f32, min: f32, max: f32) {
        for p in self
            .points
            .iter_mut()
            .filter(|p| p.time >= start && p.time <= end)
        {
            p.value = (p.value + delta).clamp(min, max);
        }
    }

    /// Removes points in `start..=end` that lie within `tolerance` of the line through their
    /// neighbours, keeping the shape of written passes light.
    pub fn thin(&mut self, start: u64, end: u64, tolerance: f32) {
        let mut i = 1;
        while i + 1 < self.points.len() {
            let (a, p, b) = (self.points[i - 1], self.points[i], self.points[i + 1]);
            let inside = p.time >= start && p.time <= end;
            let x = (p.time - a.time) as f32 / (b.time - a.time).max(1) as f32;
            let line = a.value + (b.value - a.value) * x;
            if inside
                && a.shape == CurveShape::Linear
                && p.shape == CurveShape::Linear
                && a.tension == 0.0
                && p.tension == 0.0
                && (p.value - line).abs() <= tolerance
            {
                self.points.remove(i);
            } else {
                i += 1;
            }
        }
    }
}

impl AutomationLane {
    /// Index of the point at `time`.
    pub fn index_at(&self, time: u64) -> Option<usize> {
        self.points.binary_search_by_key(&time, |p| p.time).ok()
    }

    /// Moves the points at `times` together by `dt` samples, as far as their unmoved neighbours
    /// let them, and maps their values through `value`. Returns their new times.
    pub fn shift(&mut self, times: &[u64], dt: i64, value: impl Fn(f32) -> f32) -> Vec<u64> {
        let moved = |p: &AutomationPoint| times.binary_search(&p.time).is_ok();
        let (mut low, mut high) = (i64::MIN, i64::MAX);
        let mut fixed_before: Option<u64> = None;
        for (i, p) in self.points.iter().enumerate() {
            if !moved(p) {
                fixed_before = Some(p.time);
                continue;
            }
            let t = p.time as i64;
            low = low.max(fixed_before.map_or(-t, |f| f as i64 + 1 - t));
            if let Some(next) = self.points[i + 1..].iter().find(|q| !moved(q)) {
                high = high.min(next.time as i64 - 1 - t);
            }
        }
        let dt = if low > high { 0 } else { dt.clamp(low, high) };
        let mut out = Vec::new();
        for p in self.points.iter_mut().filter(|p| moved(p)) {
            p.time = (p.time as i64 + dt) as u64;
            p.value = value(p.value);
            out.push(p.time);
        }
        out
    }

    /// Removes the points at `times` (sorted).
    pub fn remove_times(&mut self, times: &[u64]) {
        self.points
            .retain(|p| times.binary_search(&p.time).is_err());
    }

    /// Adds a point on the curve at `time`, leaving its course as it was; returns its index.
    pub fn insert_on_curve(&mut self, time: u64) -> Option<usize> {
        let value = self.value_at(time)?;
        let i = self.points.partition_point(|p| p.time < time);
        if self.points.get(i).is_some_and(|p| p.time == time) {
            return Some(i);
        }
        let mut point = AutomationPoint::new(time, value, CurveShape::Linear);
        if i > 0 && i < self.points.len() {
            // A bent curve cut in two: each piece is the same curve, steepened by its share.
            let (a, b) = (self.points[i - 1], self.points[i]);
            let s = (time - a.time) as f32 / (b.time - a.time).max(1) as f32;
            point.shape = a.shape;
            point.tension = a.tension * (1.0 - s);
            self.points[i - 1].tension = a.tension * s;
        }
        self.points.insert(i, point);
        Some(i)
    }

    /// Replaces the points between the first and last drawn ones by the drawn ones.
    pub fn draw(&mut self, drawn: &[(u64, f32)]) {
        let (Some(first), Some(last)) = (
            drawn.iter().map(|d| d.0).min(),
            drawn.iter().map(|d| d.0).max(),
        ) else {
            return;
        };
        self.remove_range(first, last);
        for &(t, v) in drawn {
            self.add(t, v, CurveShape::Linear);
        }
    }
}

/// The lane of `target`, created if missing.
pub fn lane_mut<'a>(track: &'a mut Track, target: &AutomationTarget) -> &'a mut AutomationLane {
    if let Some(i) = track.automation.iter().position(|l| &l.target == target) {
        return &mut track.automation[i];
    }
    track.automation.push(AutomationLane::new(target.clone()));
    track.automation.last_mut().expect("just pushed")
}

/// The master's lane of `target`, created if missing.
pub fn master_lane_mut<'a>(
    master: &'a mut crate::model::Master,
    target: &AutomationTarget,
) -> &'a mut AutomationLane {
    if let Some(i) = master.automation.iter().position(|l| &l.target == target) {
        return &mut master.automation[i];
    }
    master.automation.push(AutomationLane::new(target.clone()));
    master.automation.last_mut().expect("just pushed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_curves_add_up() {
        let p = AutomationPoint::new;
        let vca = [
            p(0, 0.0, CurveShape::Linear),
            p(100, -10.0, CurveShape::Linear),
        ];
        let own = [
            p(50, -6.0, CurveShape::Step),
            p(150, 0.0, CurveShape::Linear),
        ];
        let sum = sum(Some(&own), 0.0, &vca, 10);
        let at = |t| value_at(&sum, t).unwrap();
        assert_eq!(at(0), -6.0);
        assert_eq!(at(100), -16.0);
        assert!((at(75) - (-13.5)).abs() < 1e-4, "{}", at(75));
        // A fixed level plus the VCA's curve.
        let fixed = super::sum(None, -3.0, &vca, 10);
        assert_eq!(value_at(&fixed, 100), Some(-13.0));
    }

    fn lane(points: &[(u64, f32)]) -> AutomationLane {
        let mut l = AutomationLane::new(AutomationTarget::Gain);
        for &(t, v) in points {
            l.add(t, v, CurveShape::Linear);
        }
        l
    }

    #[test]
    fn curves_interpolate() {
        let mut l = lane(&[(100, 0.0), (200, 10.0)]);
        assert_eq!(l.value_at(0), Some(0.0));
        assert_eq!(l.value_at(150), Some(5.0));
        assert_eq!(l.value_at(500), Some(10.0));
        l.points[0].shape = CurveShape::Step;
        assert_eq!(l.value_at(199), Some(0.0));
        l.points[0].shape = CurveShape::Smooth;
        assert!((l.value_at(150).unwrap() - 5.0).abs() < 1e-4);
        assert!(l.value_at(110).unwrap() < 1.0);
        assert_eq!(value_at(&[], 0), None);
    }

    #[test]
    fn editing_points() {
        let mut l = lane(&[(0, 0.0), (100, 1.0), (200, 2.0), (300, 3.0)]);
        l.add(100, 5.0, CurveShape::Linear);
        assert_eq!(l.points.len(), 4);
        l.write(50, 250, -1.0);
        let times: Vec<u64> = l.points.iter().map(|p| p.time).collect();
        assert_eq!(times, vec![0, 250, 300]);
        assert_eq!(l.move_point(1, 400, 7.0), 2);
        assert_eq!(l.points.last().unwrap().value, 7.0);
        l.trim(0, 1000, 10.0, -100.0, 12.0);
        assert_eq!(l.points[0].value, 10.0);
        assert_eq!(l.points[2].value, 12.0);
        l.remove_range(0, 350);
        assert_eq!(l.points.len(), 1);
    }

    #[test]
    fn thinning_keeps_corners() {
        let mut l = lane(&[
            (0, 0.0),
            (10, 1.0),
            (20, 2.0),
            (30, 3.0),
            (40, 0.0),
            (50, 0.0),
        ]);
        l.thin(0, 50, 0.01);
        let times: Vec<u64> = l.points.iter().map(|p| p.time).collect();
        assert_eq!(times, vec![0, 30, 40, 50]);
        assert!(AutomationMode::Latch.writes() && !AutomationMode::Read.writes());
    }

    #[test]
    fn bent_curves_keep_their_ends_and_pass_where_asked() {
        assert_eq!(bend(0.3, 0.0), 0.3);
        for t in [-1.0, -0.4, 0.5, 1.0] {
            assert!(bend(0.0, t).abs() < 1e-6 && (bend(1.0, t) - 1.0).abs() < 1e-5);
            assert!((bend(0.5, tension_through(bend(0.5, t))) - bend(0.5, t)).abs() < 1e-4);
        }
        assert!(bend(0.5, 0.6) < 0.5 && bend(0.5, -0.6) > 0.5);
        let mut l = lane(&[(0, 0.0), (100, 10.0)]);
        l.points[0].tension = tension_through(0.2);
        assert!((l.value_at(50).unwrap() - 2.0).abs() < 1e-3);
        // Thinning leaves bent curves alone.
        l.add(200, 20.0, CurveShape::Linear);
        l.thin(0, 200, 100.0);
        assert_eq!(l.points.len(), 3);
    }

    #[test]
    fn a_point_added_on_the_curve_leaves_it_as_it_was() {
        let mut l = lane(&[(0, 0.0), (1000, 10.0), (2000, 0.0)]);
        l.points[0].tension = 0.7;
        l.points[1].shape = CurveShape::Step;
        let before: Vec<f32> = (0..2000)
            .step_by(37)
            .map(|t| l.value_at(t).unwrap())
            .collect();
        assert_eq!(l.insert_on_curve(300), Some(1));
        assert_eq!(l.insert_on_curve(1500), Some(3));
        assert_eq!(l.insert_on_curve(1000), Some(2));
        let after: Vec<f32> = (0..2000)
            .step_by(37)
            .map(|t| l.value_at(t).unwrap())
            .collect();
        for (a, b) in before.iter().zip(&after) {
            assert!((a - b).abs() < 1e-3, "{a} {b}");
        }
        assert_eq!(l.points.len(), 5);
    }

    #[test]
    fn points_moved_together_stop_at_their_neighbours() {
        let mut l = lane(&[(0, 0.0), (100, 1.0), (200, 2.0), (300, 3.0), (400, 4.0)]);
        let moved = l.shift(&[100, 200], 500, |v| v + 1.0);
        assert_eq!(moved, vec![199, 299]);
        let times: Vec<u64> = l.points.iter().map(|p| p.time).collect();
        assert_eq!(times, vec![0, 199, 299, 300, 400]);
        assert_eq!(l.points[1].value, 2.0);
        // Alone before the first unmoved point, as far as time zero.
        let mut l = lane(&[(50, 0.0), (100, 1.0)]);
        assert_eq!(l.shift(&[50], -80, |v| v), vec![0]);
        l.remove_times(&[0]);
        assert_eq!(l.points.len(), 1);
        // Drawing replaces what was under the stroke.
        let mut l = lane(&[(0, 0.0), (100, 1.0), (200, 2.0), (300, 3.0)]);
        l.draw(&[(150, 9.0), (120, 8.0), (250, 7.0)]);
        let times: Vec<u64> = l.points.iter().map(|p| p.time).collect();
        assert_eq!(times, vec![0, 100, 120, 150, 250, 300]);
    }
}
