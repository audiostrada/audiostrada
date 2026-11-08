//! Input monitor: passes hardware inputs to a track strip while its track hears its input,
//! and measures them all the time (the level of an armed track before recording).

use std::sync::Arc;

use crate::meter::{Meter, MeterReadout};
use crate::nodes::player::{RecordWindow, inside, window_ramp};
use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

#[derive(Debug)]
pub struct Monitor {
    gain: SharedParam,
    smoothed: Smoothed,
    meter: Meter,
    /// Where the track records: with automatic monitoring, its input is heard only there.
    window: Option<Arc<RecordWindow>>,
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
                window: None,
            },
            gain,
            readout,
        )
    }
}

impl Monitor {
    /// Hears the input only where the track records, with automatic monitoring (see
    /// [`RecordWindow`]).
    pub fn with_window(mut self, window: Arc<RecordWindow>) -> Self {
        self.window = Some(window);
        self
    }
}

impl Processor for Monitor {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.meter.prepare(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let (start, step) = self.smoothed.ramp(self.gain.get(), b.frames());
        // Before a punch-in, after a punch-out: the take being redone is heard, not the input.
        let window = self
            .window
            .as_ref()
            .and_then(|w| w.input_range())
            .filter(|_| ctx.transport.playing);
        let (position, ramp) = (
            ctx.transport.position,
            window_ramp(ctx.transport.sample_rate),
        );
        let channels = b.input_channels().min(b.output_channels());
        for ch in 0..channels {
            let (src, dst) = b.io(ch, ch);
            self.meter.update(ch, src);
            let mut gain = start;
            for (i, (d, s)) in dst.iter_mut().zip(src).enumerate() {
                let heard = window.map_or(1.0, |w| inside(w, position + i as u64, ramp));
                *d = s * gain * heard;
                gain += step;
            }
        }
    }
}
