//! Test tone and level meters for the audio configuration panel.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[derive(Clone, Debug)]
pub struct TestTone {
    phase: f32,
    increment: f32,
    gain: f32,
}

impl TestTone {
    pub fn new(frequency: f32, gain_db: f32, sample_rate: u32) -> Self {
        Self {
            phase: 0.0,
            increment: frequency / sample_rate as f32,
            gain: db_to_gain(gain_db),
        }
    }

    pub fn set_sample_rate(&mut self, frequency: f32, sample_rate: u32) {
        self.increment = frequency / sample_rate as f32;
    }

    /// Writes the next samples of the sine wave into `out`.
    #[inline]
    pub fn fill(&mut self, out: &mut [f32]) {
        for sample in out {
            *sample = (self.phase * TAU).sin() * self.gain;
            self.phase += self.increment;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

/// Per-channel peak levels written by the audio thread and read by the UI.
#[derive(Clone, Debug)]
pub struct Meters {
    peaks: Arc<[AtomicU32]>,
}

impl Meters {
    pub fn new(channels: usize) -> Self {
        Self {
            peaks: (0..channels).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    pub fn channels(&self) -> usize {
        self.peaks.len()
    }

    /// Records the peak of `samples` for `channel`; real-time safe.
    #[inline]
    pub fn update(&self, channel: usize, samples: &[f32]) {
        let peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        // Non-negative floats keep their ordering when compared as integer bits.
        self.peaks[channel].fetch_max(peak.to_bits(), Ordering::Relaxed);
    }

    /// Returns the peak since the last call and resets it.
    pub fn take_peak(&self, channel: usize) -> f32 {
        f32::from_bits(self.peaks[channel].swap(0, Ordering::Relaxed))
    }
}

/// Round-trip latency measurement: emits an impulse on every output at a fixed period and
/// detects it on the first input (through a loopback cable, or a microphone near a speaker).
#[derive(Debug)]
pub struct LatencyProbe {
    period: u64,
    threshold: f32,
    clock: u64,
    emitted_at: Option<u64>,
    results: Arc<LatencyResults>,
}

#[derive(Debug, Default)]
pub struct LatencyResults {
    last: AtomicU64,
    min: AtomicU64,
    count: AtomicU64,
    missed: AtomicU64,
}

impl LatencyResults {
    /// Last measured round trip in frames, or `None` before the first detection.
    pub fn last(&self) -> Option<u64> {
        (self.count() > 0).then(|| self.last.load(Ordering::Relaxed))
    }

    pub fn min(&self) -> Option<u64> {
        (self.count() > 0).then(|| self.min.load(Ordering::Relaxed))
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Impulses that were not detected before the next one was sent.
    pub fn missed(&self) -> u64 {
        self.missed.load(Ordering::Relaxed)
    }
}

impl LatencyProbe {
    const AMPLITUDE: f32 = 0.8;

    /// One impulse every `period` frames; detection when the input exceeds `threshold`.
    pub fn new(period: u64, threshold: f32) -> (Self, Arc<LatencyResults>) {
        let results = Arc::new(LatencyResults {
            min: AtomicU64::new(u64::MAX),
            ..LatencyResults::default()
        });
        (
            Self {
                period: period.max(1),
                threshold,
                clock: 0,
                emitted_at: None,
                results: Arc::clone(&results),
            },
            results,
        )
    }

    /// Processes one block: `input` is the first input channel, `outputs` every output channel.
    #[inline]
    pub fn process(
        &mut self,
        input: Option<&[f32]>,
        frames: usize,
        mut outputs: impl FnMut(usize, f32),
    ) {
        for i in 0..frames {
            let now = self.clock + i as u64;
            if let (Some(sent), Some(input)) = (self.emitted_at, input)
                && input[i].abs() > self.threshold
            {
                let latency = now - sent;
                self.results.last.store(latency, Ordering::Relaxed);
                self.results.min.fetch_min(latency, Ordering::Relaxed);
                self.results.count.fetch_add(1, Ordering::Relaxed);
                self.emitted_at = None;
            }
            let impulse = now.is_multiple_of(self.period);
            if impulse {
                if self.emitted_at.is_some() {
                    self.results.missed.fetch_add(1, Ordering::Relaxed);
                }
                self.emitted_at = Some(now);
            }
            outputs(i, if impulse { Self::AMPLITUDE } else { 0.0 });
        }
        self.clock += frames as u64;
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * gain.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_has_requested_level_and_period() {
        let mut tone = TestTone::new(1000.0, -6.0, 48_000);
        let mut out = vec![0.0; 480];
        tone.fill(&mut out);
        let peak = out.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!((gain_to_db(peak) + 6.0).abs() < 0.01);
        assert!(out[0].abs() < 1e-6 && out[48].abs() < 1e-3);
    }

    #[test]
    fn meters_keep_max_until_taken() {
        let meters = Meters::new(2);
        meters.update(0, &[0.1, -0.5, 0.2]);
        meters.update(0, &[0.3]);
        assert_eq!(meters.take_peak(0), 0.5);
        assert_eq!(meters.take_peak(0), 0.0);
        assert_eq!(meters.take_peak(1), 0.0);
    }

    #[test]
    fn latency_probe_measures_a_simulated_loopback() {
        const DELAY: usize = 300;
        let (mut probe, results) = LatencyProbe::new(4800, 0.1);
        let mut line = vec![0.0f32; DELAY];
        let mut pos = 0;
        let mut input = vec![0.0f32; 128];
        let mut output = vec![0.0f32; 128];
        for _ in 0..200 {
            probe.process(Some(&input), 128, |i, v| output[i] = v);
            // Loopback with a fixed delay: this block's output arrives DELAY frames later.
            for i in 0..128 {
                input[i] = line[pos];
                line[pos] = output[i];
                pos = (pos + 1) % DELAY;
            }
        }
        // One extra block of buffering is part of the measured round trip here.
        assert_eq!(results.min(), Some((DELAY + 128) as u64));
        assert!(results.count() >= 4);
        assert_eq!(results.missed(), 0);
    }

    #[test]
    fn db_conversions() {
        assert!((db_to_gain(-20.0) - 0.1).abs() < 1e-6);
        assert_eq!(gain_to_db(0.0), f32::NEG_INFINITY);
    }
}
