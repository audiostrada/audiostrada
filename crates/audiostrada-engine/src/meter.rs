//! Peak and RMS metering published lock-free for the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// RMS integration time constant.
const RMS_WINDOW_SECS: f32 = 0.3;

#[derive(Debug)]
struct Channel {
    peak: AtomicU32,
    rms: AtomicU32,
}

/// Levels of a multichannel signal; cheap to clone, shared with the UI.
#[derive(Clone, Debug)]
pub struct MeterReadout(Arc<[Channel]>);

impl MeterReadout {
    pub fn new(channels: usize) -> Self {
        Self(
            (0..channels)
                .map(|_| Channel {
                    peak: AtomicU32::new(0),
                    rms: AtomicU32::new(0),
                })
                .collect(),
        )
    }

    pub fn channels(&self) -> usize {
        self.0.len()
    }

    /// Highest absolute sample since the last call, then reset.
    pub fn take_peak(&self, channel: usize) -> f32 {
        f32::from_bits(self.0[channel].peak.swap(0, Ordering::Relaxed))
    }

    /// Current RMS level (linear).
    pub fn rms(&self, channel: usize) -> f32 {
        f32::from_bits(self.0[channel].rms.load(Ordering::Relaxed))
    }
}

/// Audio-thread side of a meter: keeps the RMS state and writes the readout.
#[derive(Debug)]
pub struct Meter {
    readout: MeterReadout,
    mean_square: Vec<f32>,
    coefficient: f32,
}

impl Meter {
    pub fn new(readout: MeterReadout) -> Self {
        Self {
            mean_square: vec![0.0; readout.channels()],
            readout,
            coefficient: 0.0,
        }
    }

    pub fn prepare(&mut self, sample_rate: u32) {
        self.coefficient = (-1.0 / (RMS_WINDOW_SECS * sample_rate as f32)).exp();
        self.mean_square.fill(0.0);
    }

    #[inline]
    pub fn update(&mut self, channel: usize, samples: &[f32]) {
        let Some(state) = self.mean_square.get_mut(channel) else {
            return;
        };
        let mut peak = 0.0f32;
        let mut ms = *state;
        for &s in samples {
            peak = peak.max(s.abs());
            ms = self.coefficient * ms + (1.0 - self.coefficient) * s * s;
        }
        *state = ms;
        let out = &self.readout.0[channel];
        // Non-negative floats keep their ordering when compared as integer bits.
        out.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        out.rms.store(ms.sqrt().to_bits(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_full_scale_sine_is_minus_3_db() {
        let readout = MeterReadout::new(1);
        let mut meter = Meter::new(readout.clone());
        meter.prepare(48_000);
        let sine: Vec<f32> = (0..48_000 * 2)
            .map(|i| (i as f32 * 1000.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();
        for block in sine.chunks(256) {
            meter.update(0, block);
        }
        assert!((readout.rms(0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
        assert!((readout.take_peak(0) - 1.0).abs() < 1e-3);
        assert_eq!(readout.take_peak(0), 0.0);
    }
}
