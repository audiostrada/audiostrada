//! Stereo gain stage: track input trim and polarity, sends.

use crate::nodes::StripControls;
use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

#[derive(Debug)]
pub struct Gain {
    gain: SharedParam,
    smoothed: Smoothed,
    /// Silent whenever this strip is (pre-fader sends follow their track's mute and solo).
    follow: Option<StripControls>,
}

impl Gain {
    /// Returns the node and its linear gain.
    pub fn new(gain: f32, follow: Option<StripControls>) -> (Self, SharedParam) {
        let param = SharedParam::new(gain);
        (
            Self {
                gain: param.clone(),
                smoothed: Smoothed::new(),
                follow,
            },
            param,
        )
    }
}

impl Processor for Gain {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let silenced = self.follow.as_ref().is_some_and(StripControls::silenced);
        let target = if silenced { 0.0 } else { self.gain.get() };
        let (start, step) = self.smoothed.ramp(target, b.frames());
        for ch in 0..b.input_channels().min(b.output_channels()) {
            let (src, dst) = b.io(ch, ch);
            let mut gain = start;
            for (d, s) in dst.iter_mut().zip(src) {
                *d = s * gain;
                gain += step;
            }
        }
    }
}
