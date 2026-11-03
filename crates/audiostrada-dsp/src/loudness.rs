//! Loudness after ITU-R BS.1770 (K-weighting, LUFS) and level meters for effect displays.

use crate::gain_to_db;

/// Short-term loudness spans 3 s of 100 ms blocks.
const BLOCKS: usize = 30;
/// Peaks are held this long, so a display reading them 30 times a second misses none.
const HOLD_MS: f32 = 40.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    pub fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The two K-weighting stages (high shelf, then high pass) for `rate`.
pub fn k_weighting(rate: u32) -> [Biquad; 2] {
    let fs = f64::from(rate);
    let (f0, gain_db, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let high_pass = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    [shelf, high_pass]
}

pub fn lufs(mean_square: f64) -> f64 {
    if mean_square <= 0.0 {
        f64::NEG_INFINITY
    } else {
        -0.691 + 10.0 * mean_square.log10()
    }
}

/// The highest value pushed over the last 40 to 80 ms.
#[derive(Clone, Copy, Debug)]
pub struct Hold {
    current: f32,
    last: f32,
    count: u32,
    length: u32,
}

impl Hold {
    pub fn new(rate: u32) -> Self {
        Self {
            current: 0.0,
            last: 0.0,
            count: 0,
            length: ((rate as f32 * HOLD_MS / 1000.0) as u32).max(1),
        }
    }

    #[inline]
    pub fn push(&mut self, value: f32) {
        self.current = self.current.max(value);
        self.count += 1;
        if self.count >= self.length {
            self.last = self.current;
            self.current = 0.0;
            self.count = 0;
        }
    }

    pub fn value(&self) -> f32 {
        self.current.max(self.last)
    }

    pub fn reset(&mut self) {
        self.current = 0.0;
        self.last = 0.0;
        self.count = 0;
    }
}

/// Integrated loudness after BS.1770: 400 ms blocks every 100 ms, gated at -70 LUFS and
/// 10 LU under the mean; kept as a histogram of 0.1 LU, so it never allocates.
#[derive(Clone, Debug)]
pub struct Integrated {
    counts: [u32; GATE_BINS],
    energies: [f64; GATE_BINS],
}

const GATE_FLOOR: f64 = -70.0;
const GATE_BINS: usize = 800;

impl Default for Integrated {
    fn default() -> Self {
        Self {
            counts: [0; GATE_BINS],
            energies: [0.0; GATE_BINS],
        }
    }
}

impl Integrated {
    /// Adds a 400 ms block of mean square `energy`.
    pub fn push(&mut self, energy: f64) {
        let loudness = lufs(energy);
        if loudness <= GATE_FLOOR {
            return;
        }
        let bin = (((loudness - GATE_FLOOR) * 10.0) as usize).min(GATE_BINS - 1);
        self.counts[bin] += 1;
        self.energies[bin] += energy;
    }

    /// In LUFS; -120 before any block above the floor.
    pub fn value(&self) -> f32 {
        let mean = |from: usize| {
            let (n, e) = (from..GATE_BINS).fold((0u64, 0.0f64), |(n, e), b| {
                (n + u64::from(self.counts[b]), e + self.energies[b])
            });
            (n > 0).then(|| e / n as f64)
        };
        let Some(ungated) = mean(0) else {
            return -120.0;
        };
        let relative = lufs(ungated) - 10.0;
        let from = (((relative - GATE_FLOOR) * 10.0).max(0.0) as usize).min(GATE_BINS - 1);
        mean(from).map_or(-120.0, |e| (lufs(e) as f32).max(-120.0))
    }

    pub fn reset(&mut self) {
        self.counts = [0; GATE_BINS];
        self.energies = [0.0; GATE_BINS];
    }
}

/// Samples between those of a stereo signal, four times over (BS.1770 true peak). The
/// estimate lags the input by [`TRUE_PEAK_LAG`] samples.
#[derive(Clone, Debug)]
pub struct TruePeak {
    history: [[f32; 2 * TP_TAPS]; 2],
    at: usize,
    kernel: [[f32; TP_TAPS]; 4],
}

const TP_TAPS: usize = 12;
pub const TRUE_PEAK_LAG: usize = TP_TAPS / 2;

impl Default for TruePeak {
    fn default() -> Self {
        let bessel = |x: f64| {
            let (mut term, mut sum) = (1.0, 1.0);
            for k in 1..24 {
                term *= (x / (2.0 * f64::from(k))).powi(2);
                sum += term;
            }
            sum
        };
        let mut kernel = [[0.0f32; TP_TAPS]; 4];
        for (phase, taps) in kernel.iter_mut().enumerate() {
            // Weight of the sample `i` steps back, for the point `phase / 4` after the
            // sample `TRUE_PEAK_LAG` steps back.
            let t = phase as f64 / 4.0;
            let mut sum = 0.0;
            let mut w = [0.0f64; TP_TAPS];
            for (i, v) in w.iter_mut().enumerate() {
                let x = i as f64 - TRUE_PEAK_LAG as f64 + t;
                let sinc = if x.abs() < 1e-9 {
                    1.0
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                let r = x / (TRUE_PEAK_LAG as f64 + 1.0);
                *v = sinc * bessel(5.0 * (1.0 - r * r).max(0.0).sqrt()) / bessel(5.0);
                sum += *v;
            }
            for (k, v) in taps.iter_mut().zip(w) {
                *k = (v / sum) as f32;
            }
        }
        Self {
            history: [[0.0; 2 * TP_TAPS]; 2],
            at: 0,
            kernel,
        }
    }
}

impl TruePeak {
    /// Pushes a frame; returns the highest absolute value of each channel around the sample
    /// [`TRUE_PEAK_LAG`] frames ago, between it and the next one.
    #[inline]
    pub fn push(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.at = (self.at + TP_TAPS - 1) % TP_TAPS;
        let mut out = [0.0f32; 2];
        for (ch, x) in frame.into_iter().enumerate() {
            let h = &mut self.history[ch];
            h[self.at] = x;
            h[self.at + TP_TAPS] = x;
            let window = &h[self.at..self.at + TP_TAPS];
            out[ch] = self
                .kernel
                .iter()
                .map(|taps| {
                    taps.iter()
                        .zip(window)
                        .map(|(k, v)| k * v)
                        .sum::<f32>()
                        .abs()
                })
                .fold(0.0f32, f32::max);
        }
        out
    }

    pub fn reset(&mut self) {
        self.history = [[0.0; 2 * TP_TAPS]; 2];
    }
}

/// Held peaks and short-term loudness of a stereo signal; never allocates.
#[derive(Clone, Debug)]
pub struct LevelMeter {
    filters: [[Biquad; 2]; 2],
    sum: f64,
    count: u32,
    block: u32,
    blocks: [f64; BLOCKS],
    at: usize,
    filled: usize,
    peaks: [Hold; 2],
    integrated: Integrated,
    true_peak: TruePeak,
    /// Highest true peak since the last reset, linear.
    highest: f32,
}

impl LevelMeter {
    pub fn new(rate: u32) -> Self {
        Self {
            filters: [k_weighting(rate); 2],
            sum: 0.0,
            count: 0,
            block: (rate / 10).max(1),
            blocks: [0.0; BLOCKS],
            at: 0,
            filled: 0,
            peaks: [Hold::new(rate); 2],
            integrated: Integrated::default(),
            true_peak: TruePeak::default(),
            highest: 0.0,
        }
    }

    #[inline]
    pub fn push(&mut self, left: f32, right: f32) {
        let [a, b] = self.true_peak.push([left, right]);
        self.highest = self.highest.max(a).max(b);
        for (ch, x) in [left, right].into_iter().enumerate() {
            self.peaks[ch].push(x.abs());
            let [shelf, high_pass] = &mut self.filters[ch];
            let y = high_pass.run(shelf.run(f64::from(x)));
            self.sum += y * y;
        }
        self.count += 1;
        if self.count == self.block {
            self.blocks[self.at] = self.sum / f64::from(self.block);
            self.at = (self.at + 1) % BLOCKS;
            self.filled = (self.filled + 1).min(BLOCKS);
            // A 400 ms block, overlapping the last by 300 ms.
            if self.filled >= 4 {
                let last4 = (1..=4)
                    .map(|k| self.blocks[(self.at + BLOCKS - k) % BLOCKS])
                    .sum::<f64>();
                self.integrated.push(last4 / 4.0);
            }
            self.sum = 0.0;
            self.count = 0;
        }
    }

    /// Left and right peaks, in dBFS.
    pub fn peaks(&self) -> [f32; 2] {
        self.peaks.map(|p| gain_to_db(p.value()))
    }

    /// Loudness of the last 3 s (less right after a reset), in LUFS; -120 for silence.
    pub fn short_term(&self) -> f32 {
        if self.filled == 0 {
            return -120.0;
        }
        let energy = self.blocks.iter().sum::<f64>() / self.filled as f64;
        (lufs(energy) as f32).max(-120.0)
    }

    /// Loudness since the last reset, gated after BS.1770, in LUFS.
    pub fn integrated(&self) -> f32 {
        self.integrated.value()
    }

    /// Highest true peak since the last reset, in dBTP.
    pub fn highest_true_peak(&self) -> f32 {
        gain_to_db(self.highest)
    }

    pub fn reset(&mut self) {
        self.integrated.reset();
        self.true_peak.reset();
        self.highest = 0.0;
        for f in self.filters.iter_mut().flatten() {
            f.z = [0.0; 2];
        }
        self.sum = 0.0;
        self.count = 0;
        self.blocks = [0.0; BLOCKS];
        self.at = 0;
        self.filled = 0;
        for p in &mut self.peaks {
            p.reset();
        }
    }
}

/// Meters of the limiters, in this order: gain reduction (dB), output peak, input left and
/// right, output left and right peaks (dBFS), input and output short-term loudness (LUFS),
/// integrated loudness of the output (LUFS) and its highest true peak (dBTP).
pub const LIMITER_METERS: &[&str] = &[
    "Gain reduction",
    "Output peak",
    "Input L",
    "Input R",
    "Output L",
    "Output R",
    "Input loudness",
    "Output loudness",
    "Integrated loudness",
    "True peak",
];

/// Writes [`LIMITER_METERS`]; `reduction` holds how far under 1 the gain went.
pub(crate) fn limiter_meters(
    out: &mut [f32],
    reduction: &Hold,
    input: &LevelMeter,
    output: &LevelMeter,
) {
    let [il, ir] = input.peaks();
    let [ol, or] = output.peaks();
    let values = [
        gain_to_db(1.0 - reduction.value()),
        ol.max(or),
        il,
        ir,
        ol,
        or,
        input.short_term(),
        output.short_term(),
        output.integrated(),
        output.highest_true_peak(),
    ];
    for (o, v) in out.iter_mut().zip(values) {
        *o = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_at_minus_20_dbfs_reads_minus_20_lufs_and_its_peak() {
        let mut meter = LevelMeter::new(48_000);
        for n in 0..48_000 * 4 {
            let s = 0.1 * (std::f32::consts::TAU * 997.0 * n as f32 / 48_000.0).sin();
            meter.push(s, s);
        }
        assert!(
            (meter.short_term() + 20.0).abs() < 0.1,
            "{}",
            meter.short_term()
        );
        for p in meter.peaks() {
            assert!((p + 20.0).abs() < 0.05, "{p}");
        }
        meter.reset();
        assert_eq!(meter.short_term(), -120.0);
    }

    #[test]
    fn a_peak_stays_visible_for_the_hold_time() {
        let mut hold = Hold::new(1000);
        hold.push(0.8);
        for _ in 0..39 {
            hold.push(0.1);
        }
        assert_eq!(hold.value(), 0.8, "still there after 40 ms");
        for _ in 0..40 {
            hold.push(0.1);
        }
        assert_eq!(hold.value(), 0.1);
    }

    #[test]
    fn integrated_loudness_gates_the_silence() {
        let mut meter = LevelMeter::new(48_000);
        // 10 s of a -20 dBFS sine then 10 s of silence: the silence is gated away.
        for n in 0..48_000 * 20 {
            let s = if n < 48_000 * 10 {
                0.1 * (std::f32::consts::TAU * 997.0 * n as f32 / 48_000.0).sin()
            } else {
                0.0
            };
            meter.push(s, s);
        }
        assert!(
            (meter.integrated() + 20.0).abs() < 0.2,
            "{}",
            meter.integrated()
        );
    }

    #[test]
    fn true_peak_sees_between_the_samples() {
        // A sine at a quarter of the rate, sampled at +-45 degrees: samples at 0.707, the
        // wave reaches 1.
        let mut meter = LevelMeter::new(48_000);
        for n in 0..4800 {
            let s = (std::f32::consts::FRAC_PI_2 * n as f32 + std::f32::consts::FRAC_PI_4).sin();
            meter.push(s, s);
        }
        assert!((meter.peaks()[0] + 3.01).abs() < 0.05);
        let tp = meter.highest_true_peak();
        assert!(tp > -0.6 && tp < 0.3, "{tp}");
    }
}
