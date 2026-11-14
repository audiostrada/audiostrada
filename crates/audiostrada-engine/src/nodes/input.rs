//! Input stage of a channel: its trim and polarity, then its console color (the channel's
//! preamp, or the summing of a bus or of the master).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use audiostrada_dsp::console::ConsoleColor;

use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

/// The console color of a channel, set from the project.
#[derive(Clone, Debug)]
pub struct ConsoleControls {
    model: SharedParam,
    amount: SharedParam,
    bus: Arc<AtomicBool>,
    noise: Arc<AtomicBool>,
    /// The seed of the channel's tolerances: its track.
    seed: Arc<AtomicU64>,
}

impl ConsoleControls {
    fn new() -> Self {
        Self {
            model: SharedParam::new(0.0),
            amount: SharedParam::new(0.0),
            bus: Arc::default(),
            noise: Arc::default(),
            seed: Arc::default(),
        }
    }

    /// The console (index into `audiostrada_dsp::console::MODELS`), its amount, summing a bus,
    /// its hiss heard, and the seed of the channel's tolerances.
    pub fn set(&self, model: usize, amount: f32, bus: bool, noise: bool, seed: u64) {
        self.model.set(model as f32);
        self.amount.set(amount);
        self.bus.store(bus, Ordering::Relaxed);
        self.noise.store(noise, Ordering::Relaxed);
        self.seed.store(seed, Ordering::Relaxed);
    }
}

/// What every console of a project shares: all colors off at once, to compare.
#[derive(Debug, Default)]
pub struct ConsoleSwitch {
    off: AtomicBool,
}

impl ConsoleSwitch {
    pub fn set_off(&self, off: bool) {
        self.off.store(off, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct InputStage {
    gain: SharedParam,
    smoothed: Smoothed,
    controls: ConsoleControls,
    /// Every console color off at once, to compare.
    bypass: Arc<ConsoleSwitch>,
    color: ConsoleColor,
}

impl InputStage {
    /// Returns the node, its linear gain and its console controls.
    pub fn new(bypass: Arc<ConsoleSwitch>) -> (Self, SharedParam, ConsoleControls) {
        let gain = SharedParam::new(1.0);
        let controls = ConsoleControls::new();
        (
            Self {
                gain: gain.clone(),
                smoothed: Smoothed::new(),
                controls: controls.clone(),
                bypass,
                color: ConsoleColor::new(48_000),
            },
            gain,
            controls,
        )
    }
}

impl Processor for InputStage {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.color = ConsoleColor::new(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let (start, step) = self.smoothed.ramp(self.gain.get(), b.frames());
        for ch in 0..b.input_channels().min(b.output_channels()) {
            let (src, dst) = b.io(ch, ch);
            let mut gain = start;
            for (d, s) in dst.iter_mut().zip(src) {
                *d = s * gain;
                gain += step;
            }
        }
        let c = &self.controls;
        let off = self.bypass.off.load(Ordering::Relaxed);
        let (model, amount) = if off {
            (0, 0.0)
        } else {
            (c.model.get().round() as usize, c.amount.get())
        };
        self.color.set(
            model,
            amount,
            c.bus.load(Ordering::Relaxed),
            c.noise.load(Ordering::Relaxed),
            Some(c.seed.load(Ordering::Relaxed)),
        );
        if self.color.active() && b.output_channels() >= 2 {
            let (left, right) = b.output_pair(0, 1);
            self.color.process(left, right);
        }
    }

    fn reset(&mut self) {
        self.color.reset();
    }
}
