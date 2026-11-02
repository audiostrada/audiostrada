//! Input monitor: passes hardware inputs to a track strip while its track is armed.

use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

#[derive(Debug)]
pub struct Monitor {
    gain: SharedParam,
    smoothed: Smoothed,
}

impl Monitor {
    /// Returns the node and its gain (0 silent, 1 monitoring).
    pub fn new() -> (Self, SharedParam) {
        let gain = SharedParam::new(0.0);
        (
            Self {
                gain: gain.clone(),
                smoothed: Smoothed::new(),
            },
            gain,
        )
    }
}

impl Processor for Monitor {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let (start, step) = self.smoothed.ramp(self.gain.get(), b.frames());
        let channels = b.input_channels().min(b.output_channels());
        for ch in 0..channels {
            let (src, dst) = b.io(ch, ch);
            let mut gain = start;
            for (d, s) in dst.iter_mut().zip(src) {
                *d = s * gain;
                gain += step;
            }
        }
    }
}
