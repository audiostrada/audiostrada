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

/// What every console of a project shares: all colors off at once, to compare, or one console
/// heard in place of each of them (a blind test).
#[derive(Debug, Default)]
pub struct ConsoleSwitch {
    off: AtomicBool,
    /// 0: each channel's own console; else a model (high half) and its amount in tenths of a
    /// percent plus one (low half).
    test: std::sync::atomic::AtomicU32,
}

impl ConsoleSwitch {
    pub fn set_off(&self, off: bool) {
        self.off.store(off, Ordering::Relaxed);
    }

    /// The console (index into `audiostrada_dsp::console::MODELS`) and amount every channel
    /// plays, or `None` for its own.
    pub fn set_test(&self, test: Option<(usize, f32)>) {
        let word = test.map_or(0, |(model, amount)| {
            ((model as u32) << 16) | ((amount.clamp(0.0, 100.0) * 10.0).round() as u32 + 1)
        });
        self.test.store(word, Ordering::Relaxed);
    }

    fn test(&self) -> Option<(usize, f32)> {
        let word = self.test.load(Ordering::Relaxed);
        (word != 0).then(|| ((word >> 16) as usize, ((word & 0xffff) - 1) as f32 / 10.0))
    }
}

#[derive(Debug)]
pub struct InputStage {
    gain: SharedParam,
    smoothed: Smoothed,
    controls: ConsoleControls,
    /// Every console color off at once, to compare, or one console in place of them all.
    bypass: Arc<ConsoleSwitch>,
    color: ConsoleColor,
    /// What the console plays while a blind test has it heard dry: it keeps running, so that
    /// switching back gives no tell (no warm-up, no level settling).
    unheard: [Vec<f32>; 2],
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
                unheard: [Vec::new(), Vec::new()],
            },
            gain,
            controls,
        )
    }
}

impl Processor for InputStage {
    fn prepare(&mut self, sample_rate: u32, max_frames: usize) {
        self.color = ConsoleColor::new(sample_rate);
        self.unheard = [vec![0.0; max_frames], vec![0.0; max_frames]];
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
        let test = self.bypass.test();
        let (model, amount) = match (off, test) {
            // A blind test's dry take: the console tested still runs, unheard.
            (true, Some(t)) => t,
            (true, None) => (0, 0.0),
            (false, t) => t.unwrap_or((c.model.get().round() as usize, c.amount.get())),
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
            let n = left.len();
            if off && n <= self.unheard[0].len() {
                let [l, r] = &mut self.unheard;
                l[..n].copy_from_slice(left);
                r[..n].copy_from_slice(right);
                self.color.process(&mut l[..n], &mut r[..n]);
            } else if !off {
                self.color.process(left, right);
            }
        }
    }

    fn reset(&mut self) {
        self.color.reset();
    }
}
