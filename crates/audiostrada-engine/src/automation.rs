//! Automation playback on the audio thread: before each segment, every active lane writes its
//! value at the transport position into the parameter it drives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audiostrada_core::automation::{AutomationPoint, value_at};

use crate::param::SharedParam;

/// How a lane value becomes the parameter value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Conversion {
    Identity,
    /// Decibels plus an offset (a VCA).
    Db {
        offset: f32,
    },
    /// Decibels to a linear gain.
    DbToGain,
}

impl Conversion {
    pub fn apply(self, value: f32) -> f32 {
        match self {
            Conversion::Identity => value,
            Conversion::Db { offset } => value + offset,
            Conversion::DbToGain => 10f32.powf(value / 20.0),
        }
    }
}

#[derive(Debug)]
pub struct Lane {
    pub points: Arc<[AutomationPoint]>,
    pub target: SharedParam,
    pub conversion: Conversion,
    /// Set while the user holds the control: the lane stops driving it.
    pub overridden: Arc<AtomicBool>,
    /// Engine samples per project sample.
    pub ratio: f64,
}

/// Every lane of the project, swapped as a whole when automation changes.
#[derive(Debug, Default)]
pub struct AutomationSet {
    pub lanes: Vec<Lane>,
}

impl AutomationSet {
    /// Writes each lane's value at `position` (engine samples).
    pub fn apply(&self, position: u64) {
        for lane in &self.lanes {
            if lane.overridden.load(Ordering::Relaxed) {
                continue;
            }
            let time = (position as f64 / lane.ratio) as u64;
            if let Some(value) = value_at(&lane.points, time) {
                lane.target.set(lane.conversion.apply(value));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiostrada_core::automation::CurveShape;

    #[test]
    fn lanes_drive_parameters_unless_overridden() {
        let point = |time, value| AutomationPoint {
            time,
            value,
            shape: CurveShape::Linear,
        };
        let target = SharedParam::new(0.0);
        let overridden = Arc::new(AtomicBool::new(false));
        let set = AutomationSet {
            lanes: vec![Lane {
                points: vec![point(0, -6.0), point(1000, 0.0)].into(),
                target: target.clone(),
                conversion: Conversion::Db { offset: -3.0 },
                overridden: Arc::clone(&overridden),
                ratio: 2.0,
            }],
        };
        set.apply(1000);
        assert_eq!(
            target.get(),
            -6.0,
            "engine 1000 = project 500, -3 dB, then the offset"
        );
        overridden.store(true, Ordering::Relaxed);
        set.apply(2000);
        assert_eq!(target.get(), -6.0);
        assert!((Conversion::DbToGain.apply(-6.0206) - 0.5).abs() < 1e-4);
    }
}
