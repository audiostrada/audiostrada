//! The control room's listening controls, on the main output only: Dim and Mono.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::param::Smoothed;
use crate::{ProcessContext, Processor};

/// How much Dim lowers the main output.
pub const DIM_DB: f32 = -20.0;

#[derive(Clone, Debug, Default)]
pub struct ControlRoomControls {
    pub dim: Arc<AtomicBool>,
    pub mono: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct ControlRoom {
    controls: ControlRoomControls,
    smoothed: Smoothed,
}

impl ControlRoom {
    pub fn new() -> (Self, ControlRoomControls) {
        let controls = ControlRoomControls::default();
        (
            Self {
                controls: controls.clone(),
                smoothed: Smoothed::new(),
            },
            controls,
        )
    }
}

impl Processor for ControlRoom {
    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let dim = self.controls.dim.load(Ordering::Relaxed);
        let mono = self.controls.mono.load(Ordering::Relaxed);
        let target = if dim { 10f32.powf(DIM_DB / 20.0) } else { 1.0 };
        let (start, step) = self.smoothed.ramp(target, b.frames());
        if b.input_channels() < 2 || b.output_channels() < 2 {
            return;
        }
        for i in 0..b.frames() {
            let gain = start + step * i as f32;
            let (l, r) = (b.input(0)[i], b.input(1)[i]);
            let (l, r) = if mono {
                let m = 0.5 * (l + r);
                (m, m)
            } else {
                (l, r)
            };
            let (left, right) = b.output_pair(0, 1);
            left[i] = l * gain;
            right[i] = r * gain;
        }
    }
}
