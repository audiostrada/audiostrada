//! Look-ahead brick-wall limiter. The gain needed by each sample is held over the look-ahead
//! window and averaged over the same length, so it ramps down before every peak reaches the
//! (delayed) output and the ceiling is never exceeded; it recovers with the release time,
//! or with a blend of a fast and a slow release (auto). Peaks can be read between the samples
//! (true peak). Unity gain hears the limiting at the level it came in, audition what it
//! removes.

use crate::loudness::{Hold, LIMITER_METERS, LevelMeter, TRUE_PEAK_LAG, TruePeak, limiter_meters};
use crate::{Effect, EffectInfo, ParamInfo, db_to_gain, param, switch, time_coefficient};

pub const MAX_LOOKAHEAD_MS: f32 = 5.0;

pub const INFO: EffectInfo = EffectInfo {
    id: "limiter",
    name: "Strada-Limit",
    params: &PARAMS,
    meters: LIMITER_METERS,
    instrument: false,
    files: &[],
    category: "Dynamics",
    description: "Brickwall peak limiter with input gain and ceiling.",
};

pub(crate) mod p {
    pub const INPUT: usize = 0;
    pub const CEILING: usize = 1;
    pub const RELEASE: usize = 2;
    pub const LOOKAHEAD: usize = 3;
    pub const AUTO: usize = 4;
    pub const LINK: usize = 5;
    pub const TRUE_PEAK: usize = 6;
    pub const UNITY: usize = 7;
    pub const AUDITION: usize = 8;
}
const COUNT: usize = 9;

const PARAMS: [ParamInfo; COUNT] = [
    param("input", "Input gain", 0.0, 24.0, 0.0, "dB"),
    param("ceiling", "Ceiling", -12.0, 0.0, -0.3, "dB"),
    param("release", "Release", 1.0, 1000.0, 60.0, "ms"),
    param("lookahead", "Lookahead", 0.1, MAX_LOOKAHEAD_MS, 2.0, "ms"),
    switch("auto", "Auto release", 0.0),
    param("link", "Stereo link", 0.0, 100.0, 100.0, "%"),
    switch("true_peak", "True peak", 0.0),
    switch("unity", "Unity gain", 0.0),
    switch("audition", "Audition", 0.0),
];

/// Samples of look-ahead for `ms`.
fn window(ms: f32, rate: f32) -> usize {
    ((ms.clamp(0.1, MAX_LOOKAHEAD_MS) * 0.001 * rate) as usize).max(1)
}

/// Delay of the limiter for parameter `values` (in [`INFO`] order), in samples.
pub fn latency_of(values: &[f32], rate: f32) -> u32 {
    let v = |i: usize| values.get(i).copied().unwrap_or(PARAMS[i].default);
    let lag = if v(p::TRUE_PEAK) >= 0.5 {
        TRUE_PEAK_LAG
    } else {
        0
    };
    (window(v(p::LOOKAHEAD), rate) - 1 + lag) as u32
}

/// Running minimum then moving average of one channel's needed gains over the window.
#[derive(Debug, Default)]
struct Window {
    needed: Vec<f32>,
    /// Indices into `needed` with increasing gains.
    deque: Vec<usize>,
    head: usize,
    len: usize,
    held: Vec<f32>,
    sum: f64,
}

impl Window {
    fn new(capacity: usize) -> Self {
        Self {
            needed: vec![1.0; capacity],
            deque: vec![0; capacity],
            held: vec![1.0; capacity],
            ..Self::default()
        }
    }

    fn clear(&mut self, length: usize) {
        self.needed.fill(1.0);
        self.held.fill(1.0);
        self.head = 0;
        self.len = 0;
        self.sum = length as f64;
    }

    /// Pushes the gain needed at `index` (of `n`); returns the smoothed gain for the output.
    fn push(&mut self, index: usize, n: usize, value: f32) -> f32 {
        self.needed[index] = value;
        if self.len > 0 && self.deque[self.head] == index {
            self.head = (self.head + 1) % n;
            self.len -= 1;
        }
        while self.len > 0 {
            let back = (self.head + self.len - 1) % n;
            if self.needed[self.deque[back]] >= value {
                self.len -= 1;
            } else {
                break;
            }
        }
        self.deque[(self.head + self.len) % n] = index;
        self.len += 1;
        let minimum = self.needed[self.deque[self.head]];
        self.sum += f64::from(minimum) - f64::from(self.held[index]);
        self.held[index] = minimum;
        (self.sum / n as f64) as f32
    }
}

/// What the limiter and the maximizer share: the look-ahead windows, the delay of the audio
/// and the release. Allocates only in [`Core::new`].
#[derive(Debug)]
pub(crate) struct Core {
    length: usize,
    lag: usize,
    windows: [Window; 2],
    at: usize,
    delay: [Vec<f32>; 2],
    delay_len: usize,
    delay_at: usize,
    fast: [f32; 2],
    slow: [f32; 2],
}

impl Core {
    /// Room for `max_length` samples of look-ahead and `lag` more of detection delay.
    pub(crate) fn new(max_length: usize, lag: usize) -> Self {
        let mut core = Self {
            length: 1,
            lag: 0,
            windows: [Window::new(max_length), Window::new(max_length)],
            at: 0,
            delay: [vec![0.0; max_length + lag], vec![0.0; max_length + lag]],
            delay_len: 0,
            delay_at: 0,
            fast: [1.0; 2],
            slow: [1.0; 2],
        };
        core.configure(1, 0);
        core
    }

    /// Sets the look-ahead and the detection delay, clearing everything.
    pub(crate) fn configure(&mut self, length: usize, lag: usize) {
        self.length = length.clamp(1, self.windows[0].needed.len());
        self.lag = lag;
        self.delay_len = (self.length - 1 + lag).min(self.delay[0].len());
        self.clear();
    }

    pub(crate) fn matches(&self, length: usize, lag: usize) -> bool {
        self.length == length && self.lag == lag
    }

    pub(crate) fn clear(&mut self) {
        for w in &mut self.windows {
            w.clear(self.length);
        }
        for line in &mut self.delay {
            line.fill(0.0);
        }
        self.at = 0;
        self.delay_at = 0;
        self.fast = [1.0; 2];
        self.slow = [1.0; 2];
    }

    pub(crate) fn latency(&self) -> u32 {
        self.delay_len as u32
    }

    /// Pushes a frame and the gains its peaks need; returns the delayed frame and the gain
    /// for each channel. `release` is the fast and the slow coefficient and how much of the
    /// slow one to use.
    pub(crate) fn run(
        &mut self,
        frame: [f32; 2],
        needed: [f32; 2],
        release: (f32, f32, f32),
    ) -> ([f32; 2], [f32; 2]) {
        let (fast, slow, weight) = release;
        let n = self.length;
        let mut gains = [1.0f32; 2];
        let mut dry = frame;
        for ch in 0..2 {
            let target = self.windows[ch].push(self.at, n, needed[ch]);
            // Down at once; up with the fast and the slow release blended. Both stay under
            // the target, so their blend does too.
            self.fast[ch] = if target < self.fast[ch] {
                target
            } else {
                target + fast * (self.fast[ch] - target)
            };
            self.slow[ch] = if target < self.slow[ch] {
                target
            } else {
                target + slow * (self.slow[ch] - target)
            };
            gains[ch] = self.fast[ch] + (self.slow[ch] - self.fast[ch]) * weight;
            if self.delay_len > 0 {
                let line = &mut self.delay[ch];
                dry[ch] = line[self.delay_at];
                line[self.delay_at] = frame[ch];
            }
        }
        self.at = (self.at + 1) % n;
        if self.delay_len > 0 {
            self.delay_at = (self.delay_at + 1) % self.delay_len;
        }
        (dry, gains)
    }
}

/// The gains each channel needs to stay under `ceiling`, the louder pulling the other by
/// `link` (0 to 1).
pub(crate) fn needed(peaks: [f32; 2], ceiling: f32, link: f32) -> [f32; 2] {
    let each = peaks.map(|p| if p > ceiling { ceiling / p } else { 1.0 });
    let both = each[0].min(each[1]);
    each.map(|g| g + (both - g) * link)
}

#[derive(Debug)]
pub struct Limiter {
    rate: f32,
    values: [f32; COUNT],
    core: Core,
    true_peak: TruePeak,
    reduction: Hold,
    input_meter: LevelMeter,
    output_meter: LevelMeter,
}

impl Default for Limiter {
    fn default() -> Self {
        let mut limiter = Limiter {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            core: Core::new(window(MAX_LOOKAHEAD_MS, 48_000.0), TRUE_PEAK_LAG),
            true_peak: TruePeak::default(),
            reduction: Hold::new(48_000),
            input_meter: LevelMeter::new(48_000),
            output_meter: LevelMeter::new(48_000),
        };
        limiter.configure();
        limiter
    }
}

impl Limiter {
    fn shape(&self) -> (usize, usize) {
        let lag = if self.values[p::TRUE_PEAK] >= 0.5 {
            TRUE_PEAK_LAG
        } else {
            0
        };
        (window(self.values[p::LOOKAHEAD], self.rate), lag)
    }

    fn configure(&mut self) {
        let (length, lag) = self.shape();
        if !self.core.matches(length, lag) {
            self.core.configure(length, lag);
            self.true_peak.reset();
        }
    }
}

impl Effect for Limiter {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.core = Core::new(window(MAX_LOOKAHEAD_MS, self.rate), TRUE_PEAK_LAG);
        self.reduction = Hold::new(sample_rate);
        self.input_meter = LevelMeter::new(sample_rate);
        self.output_meter = LevelMeter::new(sample_rate);
        self.configure();
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.configure();
        let v = self.values;
        let input = db_to_gain(v[p::INPUT]);
        let ceiling = db_to_gain(v[p::CEILING]);
        let release = v[p::RELEASE];
        let auto = v[p::AUTO] >= 0.5;
        let times = (
            time_coefficient(if auto { release * 0.5 } else { release }, self.rate),
            time_coefficient(release * 8.0, self.rate),
            if auto { 0.5 } else { 0.0 },
        );
        let link = v[p::LINK] / 100.0;
        let true_peak = v[p::TRUE_PEAK] >= 0.5;
        let unity = if v[p::UNITY] >= 0.5 { 1.0 / input } else { 1.0 };
        let audition = v[p::AUDITION] >= 0.5;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.input_meter.push(*l, *r);
            let frame = [*l * input, *r * input];
            let peaks = if true_peak {
                self.true_peak.push(frame)
            } else {
                frame.map(f32::abs)
            };
            let (dry, gains) = self.core.run(frame, needed(peaks, ceiling, link), times);
            self.reduction.push(1.0 - gains[0].min(gains[1]));
            let out = [0, 1].map(|ch| {
                let limited = (dry[ch] * gains[ch]).clamp(-ceiling, ceiling);
                if audition {
                    (dry[ch] - limited) * unity
                } else {
                    limited * unity
                }
            });
            (*l, *r) = (out[0], out[1]);
            self.output_meter.push(*l, *r);
        }
    }

    // Runs on the audio thread after jumps: clears in place, never reallocates.
    fn reset(&mut self) {
        self.core.clear();
        self.true_peak.reset();
        self.reduction.reset();
        self.input_meter.reset();
        self.output_meter.reset();
    }

    fn latency(&self) -> u32 {
        self.core.latency()
    }

    fn meters(&self, out: &mut [f32]) {
        limiter_meters(out, &self.reduction, &self.input_meter, &self.output_meter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(id: &str) -> usize {
        PARAMS.iter().position(|p| p.id == id).unwrap()
    }

    /// A tone with short bursts, twice as loud as the ceiling once boosted.
    fn program(n: usize, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                let t = (n + i) as f32 / 48_000.0;
                (t * 440.0 * std::f32::consts::TAU).sin()
                    * if (n + i) % 9000 < 40 { 1.0 } else { 0.3 }
            })
            .collect()
    }

    fn run(limiter: &mut Limiter, blocks: usize) -> (f32, Vec<f32>) {
        let mut max: f32 = 0.0;
        let mut last = Vec::new();
        for b in 0..blocks {
            let mut l = program(b * 256, 256);
            let mut r = l.clone();
            limiter.process(&mut l, &mut r);
            max = l.iter().chain(&r).fold(max, |m, s| m.max(s.abs()));
            last = l;
        }
        (max, last)
    }

    #[test]
    fn never_exceeds_the_ceiling() {
        for auto in [0.0, 1.0] {
            for lookahead in [0.1, 2.0, 5.0] {
                let mut limiter = Limiter::default();
                limiter.set_param(index("lookahead"), lookahead);
                limiter.set_param(index("auto"), auto);
                limiter.prepare(48_000, 256);
                limiter.set_param(index("input"), 12.0);
                let ceiling = db_to_gain(-0.3);
                let (max, _) = run(&mut limiter, 400);
                assert!(max <= ceiling + 1e-4, "{max} > {ceiling}");
                assert!(max > ceiling * 0.9, "the limiter does not just mute");
            }
        }
        let limiter = Limiter::default();
        assert_eq!(limiter.latency(), 95);
        assert_eq!(latency_of(&limiter.values, 48_000.0), 95);
    }

    #[test]
    fn true_peak_keeps_the_peaks_between_samples_under_the_ceiling() {
        let mut limiter = Limiter::default();
        limiter.set_param(index("true_peak"), 1.0);
        limiter.set_param(index("input"), 12.0);
        limiter.prepare(48_000, 256);
        assert_eq!(limiter.latency(), 95 + TRUE_PEAK_LAG as u32);
        // A tone at a quarter of the rate peaks between its samples.
        let mut meter = LevelMeter::new(48_000);
        for b in 0..200 {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..256)
                .map(|i| {
                    let n = (b * 256 + i) as f32;
                    let s = 0.5 * (std::f32::consts::FRAC_PI_2 * n + 0.785).sin();
                    (s, s)
                })
                .unzip();
            limiter.process(&mut l, &mut r);
            for (a, b) in l.iter().zip(&r) {
                meter.push(*a, *b);
            }
        }
        assert!(
            meter.highest_true_peak() < -0.3 + 0.2,
            "{}",
            meter.highest_true_peak()
        );
    }

    #[test]
    fn unity_and_audition() {
        let mut limiter = Limiter::default();
        limiter.prepare(48_000, 256);
        limiter.set_param(index("input"), 12.0);
        limiter.set_param(index("unity"), 1.0);
        let (max, _) = run(&mut limiter, 100);
        assert!(
            max < db_to_gain(-12.0) * 1.05,
            "back at the input level: {max}"
        );
        let mut plain = Limiter::default();
        plain.prepare(48_000, 256);
        plain.set_param(index("input"), 12.0);
        let mut heard = Limiter::default();
        heard.prepare(48_000, 256);
        heard.set_param(index("input"), 12.0);
        heard.set_param(index("audition"), 1.0);
        let (_, limited) = run(&mut plain, 50);
        let (_, removed) = run(&mut heard, 50);
        // Limited plus removed is the boosted input, delayed.
        let d = plain.latency() as usize;
        let boosted = program(49 * 256 - d, 256);
        for i in 0..256 {
            let sum = limited[i] + removed[i];
            assert!((sum - boosted[i] * db_to_gain(12.0)).abs() < 1e-3, "{i}");
        }
    }

    #[test]
    fn quiet_signals_pass_unchanged_after_the_delay() {
        let mut limiter = Limiter::default();
        limiter.prepare(48_000, 256);
        let mut l: Vec<f32> = (0..256).map(|i| i as f32 / 1000.0 * 0.1).collect();
        let mut r = l.clone();
        let original = l.clone();
        limiter.process(&mut l, &mut r);
        let d = limiter.latency() as usize;
        assert!((l[100] - original[100 - d]).abs() < 1e-6);
    }

    #[test]
    fn reset_clears_the_look_ahead_and_meters_read() {
        let mut limiter = Limiter::default();
        limiter.prepare(48_000, 256);
        limiter.set_param(0, 24.0);
        let (mut l, mut r) = (vec![0.9f32; 256], vec![0.9f32; 256]);
        limiter.process(&mut l, &mut r);
        let mut meters = [0.0; LIMITER_METERS.len()];
        limiter.meters(&mut meters);
        assert!(meters[0] < -20.0, "reduction shown: {meters:?}");
        limiter.reset();
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        limiter.process(&mut l, &mut r);
        assert!(
            l.iter().chain(&r).all(|s| *s == 0.0),
            "nothing left in the delay"
        );
    }
}
