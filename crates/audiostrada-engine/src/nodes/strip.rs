//! Mixer channel strip: gain, constant-power pan, mute, solo and metering. Outputs 0-1 go on
//! to the track's destination, 2-3 (when the node has them) feed its post-fader sends: a track
//! heard only through its sends, under a bus's solo, silences the first pair only.

use std::f64::consts::{FRAC_PI_4, SQRT_2};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::meter::{Meter, MeterReadout};
use crate::param::{SharedParam, Smoothed};
use crate::{ProcessContext, Processor};

/// Number of soloed strips in a mix; any solo silences the strips that are not soloed.
#[derive(Clone, Debug, Default)]
pub struct SoloBus(Arc<AtomicU32>);

impl SoloBus {
    fn any(&self) -> bool {
        self.0.load(Ordering::Relaxed) > 0
    }
}

/// UI side of a strip.
#[derive(Clone, Debug)]
pub struct StripControls {
    pub gain_db: SharedParam,
    /// -1 (left) to 1 (right).
    pub pan: SharedParam,
    mute: Arc<AtomicBool>,
    /// Mute driven by automation (0 or 1).
    pub mute_automation: SharedParam,
    solo: Arc<AtomicBool>,
    sends_only: Arc<AtomicBool>,
    solo_bus: SoloBus,
    /// Masters ignore solo (they are never silenced by it).
    solo_safe: bool,
    pub meter: MeterReadout,
}

impl StripControls {
    pub fn muted(&self) -> bool {
        self.mute.load(Ordering::Relaxed) || self.mute_automation.get() >= 0.5
    }

    pub fn set_mute(&self, mute: bool) {
        self.mute.store(mute, Ordering::Relaxed);
    }

    /// True when mute or another channel's solo silences this strip.
    pub fn silenced(&self) -> bool {
        self.muted() || (!self.solo_safe && self.solo_bus.any() && !self.soloed())
    }

    pub fn soloed(&self) -> bool {
        self.solo.load(Ordering::Relaxed)
    }

    /// Heard only through its sends: its own output is silent.
    pub fn sends_only(&self) -> bool {
        self.sends_only.load(Ordering::Relaxed)
    }

    pub fn set_sends_only(&self, sends_only: bool) {
        self.sends_only.store(sends_only, Ordering::Relaxed);
    }

    pub fn set_solo(&self, solo: bool) {
        if self.solo.swap(solo, Ordering::Relaxed) != solo {
            if solo {
                self.solo_bus.0.fetch_add(1, Ordering::Relaxed);
            } else {
                self.solo_bus.0.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

/// Stereo strip: mono or stereo in, stereo out.
#[derive(Debug)]
pub struct Strip {
    controls: StripControls,
    left: Smoothed,
    right: Smoothed,
    /// Gain of the outputs to the destination: 0 while heard through the sends only.
    main: Smoothed,
    meter: Meter,
}

impl Strip {
    pub fn new(solo_bus: &SoloBus, solo_safe: bool) -> (Self, StripControls) {
        let controls = StripControls {
            gain_db: SharedParam::new(0.0),
            pan: SharedParam::new(0.0),
            mute: Arc::new(AtomicBool::new(false)),
            mute_automation: SharedParam::new(0.0),
            solo: Arc::new(AtomicBool::new(false)),
            sends_only: Arc::new(AtomicBool::new(false)),
            solo_bus: solo_bus.clone(),
            solo_safe,
            meter: MeterReadout::new(2),
        };
        let strip = Self {
            meter: Meter::new(controls.meter.clone()),
            controls: controls.clone(),
            left: Smoothed::new(),
            right: Smoothed::new(),
            main: Smoothed::new(),
        };
        (strip, controls)
    }

    /// Left and right gains for the current controls.
    fn targets(&self) -> (f32, f32) {
        let c = &self.controls;
        if c.silenced() {
            return (0.0, 0.0);
        }
        // Computed in f64 so that the center position is exactly unity in f32 (null tests).
        let gain = 10f64.powf(f64::from(c.gain_db.get()) / 20.0);
        let angle = (f64::from(c.pan.get().clamp(-1.0, 1.0)) + 1.0) * FRAC_PI_4;
        (
            (gain * angle.cos() * SQRT_2) as f32,
            (gain * angle.sin() * SQRT_2) as f32,
        )
    }
}

impl Processor for Strip {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.meter.prepare(sample_rate);
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let b = &mut ctx.buffers;
        let frames = b.frames();
        let (left, right) = self.targets();
        let right_input = usize::from(b.input_channels() > 1);
        for (input, output, ramp) in [
            (0, 0, self.left.ramp(left, frames)),
            (right_input, 1, self.right.ramp(right, frames)),
        ] {
            let (src, dst) = b.io(input, output);
            let (mut gain, step) = ramp;
            for (d, s) in dst.iter_mut().zip(src) {
                *d = s * gain;
                gain += step;
            }
            self.meter.update(output, dst);
        }
        if b.output_channels() >= 4 {
            b.copy_output(0, 2);
            b.copy_output(1, 3);
            let target = if self.controls.sends_only() { 0.0 } else { 1.0 };
            let (start, step) = self.main.ramp(target, frames);
            if start != 1.0 || step != 0.0 {
                for ch in 0..2 {
                    let mut gain = start;
                    for d in b.output(ch).iter_mut() {
                        *d *= gain;
                        gain += step;
                    }
                }
            }
        }
    }
}
