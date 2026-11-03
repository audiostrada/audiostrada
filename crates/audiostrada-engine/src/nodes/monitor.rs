//! Input monitor: passes hardware inputs to a track strip while its track hears its input,
//! and measures them all the time (the level of an armed track before recording).

use crate::meter::{Meter, MeterReadout};
use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

#[derive(Debug)]
pub struct Monitor {
    gain: SharedParam,
    smoothed: Smoothed,
    meter: Meter,
}

impl Monitor {
    /// Returns the node, its gain (0 silent, 1 monitoring) and its input level.
    pub fn new() -> (Self, SharedParam, MeterReadout) {
        let gain = SharedParam::new(0.0);
        let readout = MeterReadout::new(2);
        (
            Self {
                gain: gain.clone(),
                smoothed: Smoothed::new(),
                meter: Meter::new(readout.clone()),
            },
            gain,
            readout,
        )
    }
}

impl Processor for Monitor {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.meter.prepare(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let (start, step) = self.smoothed.ramp(self.gain.get(), b.frames());
        let channels = b.input_channels().min(b.output_channels());
        for ch in 0..channels {
            let (src, dst) = b.io(ch, ch);
            self.meter.update(ch, src);
            let mut gain = start;
            for (d, s) in dst.iter_mut().zip(src) {
                *d = s * gain;
                gain += step;
            }
        }
    }
}
