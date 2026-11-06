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
        CurveShape::Linear => a.value + (b.value - a.value) * x,
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
        i > 0 && i < points.len() && points[i - 1].shape != CurveShape::Linear
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
        .map(|time| AutomationPoint {
            time,
            value: own.and_then(|o| value_at(o, time)).unwrap_or(fixed)
                + value_at(other, time).unwrap_or(0.0),
            shape: CurveShape::Linear,
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
        let point = AutomationPoint { time, value, shape };
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
                && (p.value - line).abs() <= tolerance
            {
                self.points.remove(i);
            } else {
                i += 1;
            }
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
        let p = |time, value, shape| AutomationPoint { time, value, shape };
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
}
