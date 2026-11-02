//! Loudness metering after ITU-R BS.1770 / EBU R128: momentary (400 ms), short-term (3 s) and
//! gated integrated loudness, plus the sample peak.
//!
//! The audio thread K-weights the signal and sends the mean square of every 100 ms block; the
//! UI side integrates the blocks.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use audiostrada_dsp::loudness::{Biquad, k_weighting, lufs};
use audiostrada_rt::{Receiver, Sender, channel};

use crate::{ProcessContext, Processor};

const BLOCKS_PER_SECOND: u32 = 10;
const MAX_HISTORY: usize = 10 * 3600 * 4;

/// Gated integrated loudness of 100 ms block energies (400 ms windows, 75% overlap).
fn integrated(history: &[f64]) -> Option<f64> {
    let blocks: Vec<f64> = history
        .windows(4)
        .map(|w| w.iter().sum::<f64>() / 4.0)
        .collect();
    let above = |threshold: f64| blocks.iter().copied().filter(move |&e| lufs(e) > threshold);
    let gated: Vec<f64> = above(-70.0).collect();
    if gated.is_empty() {
        return None;
    }
    let relative = lufs(gated.iter().sum::<f64>() / gated.len() as f64) - 10.0;
    let kept: Vec<f64> = above(-70.0).filter(|&e| lufs(e) > relative).collect();
    (!kept.is_empty()).then(|| lufs(kept.iter().sum::<f64>() / kept.len() as f64))
}

/// Offline loudness measurement of a whole signal.
#[derive(Debug)]
pub struct LoudnessAnalyzer {
    filters: [[Biquad; 2]; 2],
    sum: f64,
    count: u32,
    block: u32,
    history: Vec<f64>,
}

impl LoudnessAnalyzer {
    pub fn new(rate: u32) -> Self {
        Self {
            filters: [k_weighting(rate); 2],
            sum: 0.0,
            count: 0,
            block: (rate / BLOCKS_PER_SECOND).max(1),
            history: Vec::new(),
        }
    }

    pub fn push(&mut self, left: &[f32], right: &[f32]) {
        for (&l, &r) in left.iter().zip(right) {
            for (ch, x) in [l, r].into_iter().enumerate() {
                let [shelf, high_pass] = &mut self.filters[ch];
                let y = high_pass.run(shelf.run(f64::from(x)));
                self.sum += y * y;
            }
            self.count += 1;
            if self.count == self.block {
                self.history.push(self.sum / f64::from(self.block));
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }

    /// Integrated loudness in LUFS, `None` for silence.
    pub fn integrated(&self) -> Option<f64> {
        integrated(&self.history)
    }
}

/// Pass-through node measuring a stereo signal while the transport plays.
pub struct LoudnessMeter {
    filters: [[Biquad; 2]; 2],
    sum: f64,
    count: u32,
    block: u32,
    blocks: Sender<f64>,
    peak: Arc<AtomicU32>,
}

impl std::fmt::Debug for LoudnessMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoudnessMeter")
            .field("block", &self.block)
            .finish_non_exhaustive()
    }
}

/// UI side: integrates the blocks sent by the meter.
#[derive(Debug)]
pub struct LoudnessReadout {
    blocks: Receiver<f64>,
    history: Vec<f64>,
    peak: Arc<AtomicU32>,
    max_peak: f32,
}

impl LoudnessMeter {
    pub fn new() -> (Self, LoudnessReadout) {
        let (tx, rx) = channel(1024);
        let peak = Arc::new(AtomicU32::new(0));
        (
            Self {
                filters: [k_weighting(48_000); 2],
                sum: 0.0,
                count: 0,
                block: 48_000 / BLOCKS_PER_SECOND,
                blocks: tx,
                peak: Arc::clone(&peak),
            },
            LoudnessReadout {
                blocks: rx,
                history: Vec::new(),
                peak,
                max_peak: 0.0,
            },
        )
    }
}

impl Processor for LoudnessMeter {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.filters = [k_weighting(sample_rate); 2];
        self.block = (sample_rate / BLOCKS_PER_SECOND).max(1);
        self.sum = 0.0;
        self.count = 0;
    }

    fn process(&mut self, ctx: &mut ProcessContext<'_>) {
        let playing = ctx.transport.playing;
        let b = &mut ctx.buffers;
        let channels = b.input_channels().min(b.output_channels()).min(2);
        let mut peak = 0.0f32;
        for ch in 0..channels {
            let (src, dst) = b.io(ch, ch);
            dst.copy_from_slice(src);
            peak = src.iter().fold(peak, |p, s| p.max(s.abs()));
        }
        if !playing || channels == 0 {
            return;
        }
        self.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
        for i in 0..b.frames() {
            for ch in 0..channels {
                let x = f64::from(b.input(ch)[i]);
                let [shelf, high_pass] = &mut self.filters[ch];
                let y = high_pass.run(shelf.run(x));
                self.sum += y * y;
            }
            self.count += 1;
            if self.count == self.block {
                let _ = self.blocks.try_send(self.sum / f64::from(self.block));
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}

impl LoudnessReadout {
    /// Collects the blocks measured since the last call.
    pub fn update(&mut self) {
        self.history.extend(self.blocks.drain());
        if self.history.len() > MAX_HISTORY {
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(..excess);
        }
        let peak = f32::from_bits(self.peak.swap(0, Ordering::Relaxed));
        self.max_peak = self.max_peak.max(peak);
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.max_peak = 0.0;
    }

    fn window(&self, blocks: usize) -> Option<f64> {
        (self.history.len() >= blocks).then(|| {
            let tail = &self.history[self.history.len() - blocks..];
            lufs(tail.iter().sum::<f64>() / blocks as f64)
        })
    }

    /// Loudness of the last 400 ms, in LUFS.
    pub fn momentary(&self) -> Option<f64> {
        self.window(4)
    }

    /// Loudness of the last 3 s, in LUFS.
    pub fn short_term(&self) -> Option<f64> {
        self.window(30)
    }

    /// Gated loudness since the last reset, in LUFS.
    pub fn integrated(&self) -> Option<f64> {
        integrated(&self.history)
    }

    /// Highest sample since the last reset (linear).
    pub fn peak(&self) -> f32 {
        self.max_peak
    }
}

#[cfg(test)]
mod tests {
    use audiostrada_io::ChannelBuffers;

    use super::*;
    use crate::TransportInfo;

    fn measure(amplitude: f32, frequency: f32, seconds: usize) -> LoudnessReadout {
        let (mut meter, mut readout) = LoudnessMeter::new();
        meter.prepare(48_000, 480);
        let transport = TransportInfo {
            playing: true,
            position: 0,
            sample_rate: 48_000,
            beat: 0.0,
            bpm: 120.0,
            bar: Default::default(),
        };
        let mut storage = ChannelBuffers::new(2, 2, 480);
        let mut n = 0usize;
        for _ in 0..seconds * 100 {
            for i in 0..480 {
                let s = amplitude
                    * (std::f32::consts::TAU * frequency * (n + i) as f32 / 48_000.0).sin();
                storage.input_mut(0)[i] = s;
                storage.input_mut(1)[i] = s;
            }
            n += 480;
            let mut ctx = ProcessContext {
                buffers: storage.buffers(480),
                transport: &transport,
            };
            meter.process(&mut ctx);
            readout.update();
        }
        readout
    }

    #[test]
    fn sine_at_minus_20_dbfs_reads_minus_20_lufs() {
        let r = measure(0.1, 997.0, 5);
        let check = |v: Option<f64>| assert!((v.unwrap() + 20.0).abs() < 0.1, "{v:?}");
        check(r.momentary());
        check(r.short_term());
        check(r.integrated());
        assert!((r.peak() - 0.1).abs() < 1e-3);
    }

    #[test]
    fn k_weighting_lowers_bass() {
        let r = measure(0.1, 40.0, 2);
        assert!(r.momentary().unwrap() < -21.0, "{:?}", r.momentary());
    }

    #[test]
    fn history_is_capped_and_reset_clears_it() {
        let mut r = measure(0.1, 997.0, 1);
        r.history.extend(std::iter::repeat_n(0.0, MAX_HISTORY));
        r.update();
        assert_eq!(r.history.len(), MAX_HISTORY);
        assert_eq!(
            r.momentary(),
            Some(f64::NEG_INFINITY),
            "the oldest blocks went"
        );
        r.reset();
        assert_eq!((r.momentary(), r.integrated(), r.peak()), (None, None, 0.0));
    }

    #[test]
    fn silence_is_gated_out() {
        let r = measure(0.0, 997.0, 1);
        assert!(r.integrated().is_none());
        assert_eq!(r.momentary(), Some(f64::NEG_INFINITY));
    }
}
