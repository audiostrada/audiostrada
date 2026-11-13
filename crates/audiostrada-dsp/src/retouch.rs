//! What the audio editor finds in a recording (its phrases and the background between them,
//! breaths, sibilants, clicks, clipped peaks, hits) and how it retouches them, offline on
//! whole channels.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::{db_to_gain, gain_to_db};

/// A part of the audio, `start..end` in frames.
pub type Range = (usize, usize);

/// What normalizing aims at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// The highest peak, dBFS.
    Peak(f32),
    /// The integrated loudness, LUFS.
    Loudness(f32),
}

fn frames_of(ms: f32, rate: u32) -> usize {
    (ms.max(0.0) / 1000.0 * rate as f32) as usize
}

/// The channels mixed to one.
fn mono(channels: &[&[f32]]) -> Vec<f32> {
    let frames = channels.iter().map(|c| c.len()).max().unwrap_or(0);
    let n = channels.len().max(1) as f32;
    (0..frames)
        .map(|i| {
            channels
                .iter()
                .map(|c| c.get(i).copied().unwrap_or(0.0))
                .sum::<f32>()
                / n
        })
        .collect()
}

/// Ranges closer than `gap` joined, then those outside `min..=max` long dropped.
fn merge(mut ranges: Vec<Range>, gap: usize, min: usize, max: usize) -> Vec<Range> {
    ranges.sort_unstable();
    let mut out: Vec<Range> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.0 <= last.1 + gap => last.1 = last.1.max(r.1),
            _ => out.push(r),
        }
    }
    out.retain(|r| (min..=max).contains(&(r.1 - r.0)));
    out
}

/// What the editor knows of a recording, every [`STEP`] seconds: computed once, then every
/// finding (silences at any threshold, breaths, sibilants) is read from it at once.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub rate: u32,
    /// Audio frames per step.
    pub hop: usize,
    /// Audio frames in all.
    pub frames: usize,
    /// Level of each step, dBFS (RMS).
    pub level: Vec<f32>,
    /// How much each step repeats itself (a pitch), 0 to 1.
    pub periodicity: Vec<f32>,
    /// How flat the spectrum is (noise near 1, a tone near 0).
    pub flatness: Vec<f32>,
    /// Where the energy sits, Hz.
    pub centroid: Vec<f32>,
    /// The share of the energy above 5 kHz.
    pub high: Vec<f32>,
    /// The level of the background between the phrases, and of the voice itself, dBFS.
    pub floor: f32,
    pub voice: f32,
}

/// The step of a [`Profile`], seconds.
pub const STEP: f32 = 0.01;

/// The level that `share` of the steps stay under, digital silence left out.
fn percentile(levels: &[f32], share: f32) -> f32 {
    let mut sorted: Vec<f32> = levels.iter().copied().filter(|v| *v > -110.0).collect();
    if sorted.is_empty() {
        return -110.0;
    }
    sorted.sort_by(f32::total_cmp);
    sorted[((sorted.len() - 1) as f32 * share) as usize]
}

/// `x` through a filter, `stages` times.
fn filtered(x: &[f32], c: &Coefficients, stages: usize) -> Vec<f32> {
    let mut out: Vec<f64> = x.iter().map(|v| f64::from(*v)).collect();
    for _ in 0..stages {
        let mut state = Biquad::default();
        for v in &mut out {
            *v = state.process(c, *v);
        }
    }
    out.into_iter().map(|v| v as f32).collect()
}

/// Runs of `true` as ranges of steps.
fn runs(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (k, &on) in flags.iter().enumerate() {
        match (on, start) {
            (true, None) => start = Some(k),
            (false, Some(s)) => {
                out.push((s, k));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, flags.len()));
    }
    out
}

/// Runs shorter than `steps` turned to the other side.
fn fill_short(flags: &mut [bool], value: bool, steps: usize) {
    let mut k = 0;
    while k < flags.len() {
        if flags[k] != value {
            k += 1;
            continue;
        }
        let start = k;
        while k < flags.len() && flags[k] == value {
            k += 1;
        }
        // A run touching an end is kept: what lies past it is unknown.
        if k - start < steps && start > 0 && k < flags.len() {
            flags[start..k].iter_mut().for_each(|f| *f = !value);
        }
    }
}

/// The profile of a recording; the slow part, for another thread.
pub fn profile(channels: &[&[f32]], rate: u32) -> Profile {
    use realfft::RealFftPlanner;
    let x = mono(channels);
    let hop = ((rate as f32 * STEP) as usize).max(1);
    let steps = x.len().div_ceil(hop);
    let level: Vec<f32> = (0..steps)
        .map(|k| {
            let a = (k * hop).saturating_sub(hop / 2);
            let part = &x[a.min(x.len())..(a + 2 * hop).min(x.len())];
            let power = part.iter().map(|v| v * v).sum::<f32>() / part.len().max(1) as f32;
            10.0 * power.max(1e-13).log10()
        })
        .collect();
    let floor = percentile(&level, 0.08);
    // The loudest parts are sung or spoken, however little the voice is there.
    let voice = percentile(&level, 0.98);
    // The spectrum, on about 21 ms; the pitch, on about 43 ms taken down to near 12 kHz.
    let n = (rate as f32 * 0.021).max(64.0) as usize;
    let n = n.next_power_of_two();
    let down = (rate / 12_000).max(1) as usize;
    let y: Vec<f32> = x
        .chunks(down)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    let m = ((rate as f32 / down as f32) * 0.043) as usize;
    let m = m.next_power_of_two().max(64);
    let mut planner = RealFftPlanner::<f32>::new();
    let (spec_fft, pitch_fft, pitch_inv) = (
        planner.plan_fft_forward(n),
        planner.plan_fft_forward(2 * m),
        planner.plan_fft_inverse(2 * m),
    );
    let hann = |len: usize| -> Vec<f32> {
        (0..len)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / len as f32).cos())
            .collect()
    };
    let (wn, wm) = (hann(n), hann(m));
    let (mut input, mut spectrum) = (spec_fft.make_input_vec(), spec_fft.make_output_vec());
    let (mut pin, mut pspec, mut pout) = (
        pitch_fft.make_input_vec(),
        pitch_fft.make_output_vec(),
        pitch_inv.make_output_vec(),
    );
    let bin_hz = rate as f32 / n as f32;
    let low_rate = rate as f32 / down as f32;
    let (lag_lo, lag_hi) = ((low_rate / 1000.0) as usize, (low_rate / 60.0) as usize);
    let mut periodicity = vec![0.0; steps];
    let mut flatness = vec![1.0; steps];
    let mut centroid = vec![0.0; steps];
    let mut high = vec![0.0; steps];
    for k in 0..steps {
        // Under the background, nothing to tell apart.
        if level[k] < floor + 3.0 {
            continue;
        }
        let center = k * hop + hop / 2;
        let a = center.saturating_sub(n / 2);
        for (i, v) in input.iter_mut().enumerate() {
            *v = x.get(a + i).copied().unwrap_or(0.0) * wn[i];
        }
        if spec_fft.process(&mut input, &mut spectrum).is_err() {
            continue;
        }
        let (mut sum, mut weighted, mut above, mut logs, mut count, mut flat_sum) =
            (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0usize, 0.0f64);
        for (b, c) in spectrum.iter().enumerate().skip(1) {
            let f = b as f32 * bin_hz;
            let p = f64::from(c.norm_sqr()) + 1e-20;
            sum += p;
            weighted += p * f64::from(f);
            if f > 5000.0 {
                above += p;
            }
            if (100.0..8000.0).contains(&f) {
                logs += p.ln();
                flat_sum += p;
                count += 1;
            }
        }
        if count > 0 {
            let geometric = (logs / count as f64).exp();
            flatness[k] = (geometric / (flat_sum / count as f64)) as f32;
        }
        centroid[k] = (weighted / sum) as f32;
        high[k] = (above / sum) as f32;
        let c = center / down;
        let a = c.saturating_sub(m / 2);
        for (i, v) in pin.iter_mut().enumerate() {
            *v = if i < m {
                y.get(a + i).copied().unwrap_or(0.0) * wm[i]
            } else {
                0.0
            };
        }
        if pitch_fft.process(&mut pin, &mut pspec).is_err() {
            continue;
        }
        for v in pspec.iter_mut() {
            *v = realfft::num_complex::Complex::new(v.norm_sqr(), 0.0);
        }
        if pitch_inv.process(&mut pspec, &mut pout).is_err() || pout[0] <= 0.0 {
            continue;
        }
        let zero = pout[0];
        periodicity[k] = (lag_lo.max(2)..=lag_hi.min(m - 1))
            .map(|l| pout[l] / zero * m as f32 / (m - l) as f32)
            .fold(0.0f32, f32::max)
            .clamp(0.0, 1.0);
    }
    Profile {
        rate,
        hop,
        frames: x.len(),
        level,
        periodicity,
        flatness,
        centroid,
        high,
        floor,
        voice,
    }
}

impl Profile {
    fn steps_of(&self, ms: f32) -> usize {
        (ms / 1000.0 / STEP).round() as usize
    }

    fn range(&self, (a, b): (usize, usize)) -> Range {
        (
            (a * self.hop).min(self.frames),
            (b * self.hop).min(self.frames),
        )
    }

    /// The threshold between background and sound this recording calls for, dBFS: above
    /// what fills the gaps between the phrases (noise, headphones leaking), well under the
    /// voice.
    pub fn auto_threshold(&self) -> f32 {
        let between: Vec<f32> = (0..self.level.len())
            .filter(|&k| !self.voiced(k) && self.level[k] > self.floor + 3.0)
            .map(|k| self.level[k])
            .collect();
        let background = if between.len() > 20 {
            percentile(&between, 0.5)
        } else {
            self.floor
        };
        (background + 5.0).clamp(self.voice - 30.0, self.voice - 14.0)
    }

    /// Whether a step is sung or spoken: a clear pitch, not far under the voice.
    fn voiced(&self, k: usize) -> bool {
        self.periodicity[k] > 0.65 && self.level[k] > self.voice - 26.0
    }

    /// Whether each step holds sound at `threshold_db`: entered 3 dB above it, left under it.
    /// Short sounds well under the voice (a click, a chair, a drum leaking from the headphones)
    /// are not sound; gaps shorter than 300 ms are not silence.
    fn sound(&self, threshold_db: f32) -> Vec<bool> {
        let mut on = false;
        let mut sound: Vec<bool> = self
            .level
            .iter()
            .map(|&l| {
                on = if on {
                    l > threshold_db
                } else {
                    l > threshold_db + 3.0
                };
                on
            })
            .collect();
        for (a, b) in runs(&sound) {
            let loudest = self.level[a..b].iter().fold(-120.0f32, |m, v| m.max(*v));
            if b - a < self.steps_of(150.0) && loudest < self.voice - 10.0 {
                sound[a..b].iter_mut().for_each(|s| *s = false);
            }
        }
        fill_short(&mut sound, false, self.steps_of(300.0));
        sound
    }

    /// Where the phrases start: a pitch held at least 150 ms, near the voice's level, after
    /// 250 ms without one.
    fn onsets(&self) -> Vec<usize> {
        let voiced: Vec<bool> = (0..self.level.len()).map(|k| self.voiced(k)).collect();
        let mut held = voiced.clone();
        fill_short(&mut held, false, self.steps_of(80.0));
        let mut out = Vec::new();
        let mut last_end: Option<usize> = None;
        for (a, b) in runs(&held) {
            let rested = last_end.is_none_or(|e| a - e >= self.steps_of(250.0));
            let loudest = self.level[a..b].iter().fold(-120.0f32, |m, v| m.max(*v));
            if b - a >= self.steps_of(150.0) && rested && loudest > self.voice - 16.0 {
                out.push(a);
            }
            last_end = Some(b);
        }
        out
    }

    /// The breaths: before each phrase, the last soft sound without a clear pitch nor sharp
    /// highs, from 80 ms to a second, standing out of what surrounds it.
    pub fn breaths(&self, threshold_db: f32) -> Vec<Range> {
        let len = self.level.len();
        let mut out = Vec::new();
        let look = self.steps_of(700.0);
        for onset in self.onsets() {
            let from = onset.saturating_sub(look);
            // The background between the phrases there, under what leaks in.
            let mut around: Vec<f32> =
                self.level[onset.saturating_sub(self.steps_of(1500.0))..onset].to_vec();
            around.sort_by(f32::total_cmp);
            let background = around.get(around.len() / 10).copied().unwrap_or(self.floor);
            let breathy = |k: usize| {
                self.level[k] > (background + 4.0).max(threshold_db - 12.0)
                    && self.level[k] < self.voice - 6.0
                    && self.periodicity[k] < 0.65
                    && self.high[k] < 0.45
            };
            // Back from the attack: over its consonant, then the breath itself.
            let mut end = onset;
            while end > from && !breathy(end - 1) && onset - end < self.steps_of(250.0) {
                end -= 1;
            }
            let mut start = end;
            let mut gap = 0;
            while start > from {
                if breathy(start - 1) {
                    gap = 0;
                } else {
                    gap += 1;
                    if gap > self.steps_of(60.0) {
                        break;
                    }
                }
                start -= 1;
            }
            start += gap;
            let length = end.saturating_sub(start);
            if (self.steps_of(80.0)..=self.steps_of(1000.0)).contains(&length)
                && !(start..end).any(|k| self.voiced(k))
            {
                out.push(self.range((start, end.min(len))));
            }
        }
        out
    }

    /// The silences at `threshold_db`, at least 300 ms long, without the breaths; 60 ms are
    /// left after the sound for its tail and 20 ms before the next one for its attack.
    pub fn silences(&self, threshold_db: f32) -> Vec<Range> {
        let mut quiet: Vec<bool> = self.sound(threshold_db).iter().map(|s| !s).collect();
        let hop = self.hop.max(1);
        for (a, b) in self.breaths(threshold_db) {
            let end = (b / hop).min(quiet.len());
            quiet[(a / hop).min(end)..end]
                .iter_mut()
                .for_each(|q| *q = false);
        }
        let (tail, attack) = (self.steps_of(60.0), self.steps_of(20.0));
        runs(&quiet)
            .into_iter()
            .map(|(a, b)| {
                let a = if a == 0 { 0 } else { a + tail };
                let b = if b == quiet.len() {
                    b
                } else {
                    b.saturating_sub(attack)
                };
                (a, b.max(a))
            })
            .filter(|(a, b)| b - a >= self.steps_of(200.0))
            .map(|r| self.range(r))
            .collect()
    }

    /// The phrases at `threshold_db`: what sounds between the silences, breaths left out, at
    /// least 150 ms long.
    pub fn phrases(&self, threshold_db: f32) -> Vec<Range> {
        let mut sound = self.sound(threshold_db);
        let hop = self.hop.max(1);
        for (a, b) in self.breaths(threshold_db) {
            let end = (b / hop).min(sound.len());
            sound[(a / hop).min(end)..end]
                .iter_mut()
                .for_each(|s| *s = false);
        }
        runs(&sound)
            .into_iter()
            .filter(|(a, b)| b - a >= self.steps_of(150.0))
            .map(|r| self.range(r))
            .collect()
    }

    /// How loud a part is heard, dBFS: the average of its steps within 20 dB of its loudest,
    /// its tails and gaps left out.
    pub fn loudness(&self, (start, end): Range) -> f32 {
        let hop = self.hop.max(1);
        let (a, b) = (start / hop, end.div_ceil(hop).min(self.level.len()));
        let part = &self.level[a.min(b)..b];
        let loudest = part.iter().fold(-120.0f32, |m, v| m.max(*v));
        let (mut power, mut count) = (0.0f32, 0usize);
        for &l in part.iter().filter(|l| **l > loudest - 20.0) {
            power += 10f32.powf(l / 10.0);
            count += 1;
        }
        if count == 0 {
            return self.level_at(start);
        }
        10.0 * (power / count as f32).max(1e-13).log10()
    }

    /// The sibilants: bursts of highs near the voice's level, from 30 to 400 ms.
    pub fn sibilants(&self) -> Vec<Range> {
        let mut burst: Vec<bool> = (0..self.level.len())
            .map(|k| {
                self.high[k] > 0.5 && self.level[k] > self.voice - 24.0 && self.periodicity[k] < 0.6
            })
            .collect();
        fill_short(&mut burst, false, self.steps_of(20.0));
        let pad = self.steps_of(10.0);
        runs(&burst)
            .into_iter()
            .filter(|&(a, b)| (self.steps_of(30.0)..=self.steps_of(400.0)).contains(&(b - a)))
            .map(|(a, b)| self.range((a.saturating_sub(pad), (b + pad).min(self.level.len()))))
            .collect()
    }

    /// The level of a step, for a frame of audio.
    pub fn level_at(&self, frame: usize) -> f32 {
        self.level
            .get(frame / self.hop.max(1))
            .copied()
            .unwrap_or(-110.0)
    }
}

/// Clicks and pops: short spikes standing out of the audio around them, `sensitivity` from 0
/// (only the obvious ones) to 1.
pub fn find_clicks(channels: &[&[f32]], rate: u32, sensitivity: f32) -> Vec<Range> {
    let x = mono(channels);
    if x.len() < 3 {
        return Vec::new();
    }
    // The second difference: smooth audio leaves little, a spike stands out.
    let e: Vec<f32> = (0..x.len())
        .map(|n| {
            if n < 2 {
                0.0
            } else {
                (x[n] - 2.0 * x[n - 1] + x[n - 2]).abs()
            }
        })
        .collect();
    let half = frames_of(10.0, rate).max(32);
    let mut sums = vec![0.0f64; e.len() + 1];
    for (i, v) in e.iter().enumerate() {
        sums[i + 1] = sums[i] + f64::from(*v);
    }
    let factor = 22.0 - 12.0 * sensitivity.clamp(0.0, 1.0);
    // The corners of clipped peaks are not clicks: they are redrawn as clipping.
    let clipped = clip_level(&x);
    let near_clip = |n: usize| {
        clipped.is_some_and(|level| {
            x[n.saturating_sub(3)..(n + 4).min(x.len())]
                .iter()
                .any(|v| v.abs() >= level * 0.98)
        })
    };
    let flagged: Vec<Range> = (0..e.len())
        .filter(|&n| {
            let (a, b) = (n.saturating_sub(half), (n + half).min(e.len()));
            let mean = ((sums[b] - sums[a]) / (b - a) as f64) as f32;
            e[n] > 2e-3 && e[n] > factor * mean && !near_clip(n)
        })
        .map(|n| (n, n + 1))
        .collect();
    let pad = frames_of(0.3, rate).max(2);
    let mean = |a: usize, b: usize| {
        let (a, b) = (a.min(e.len()), b.min(e.len()));
        if b > a {
            ((sums[b] - sums[a]) / (b - a) as f64) as f32
        } else {
            0.0
        }
    };
    let (before, after) = (frames_of(12.0, rate), frames_of(3.0, rate));
    merge(
        flagged
            .into_iter()
            .map(|(a, b)| (a.saturating_sub(pad), (b + pad).min(x.len())))
            .collect(),
        frames_of(0.5, rate),
        1,
        frames_of(3.0, rate),
    )
    .into_iter()
    // A click is over at once; a hit (a hi-hat leaking in) rings on after its attack.
    .filter(|&(a, b)| {
        let base = mean(a.saturating_sub(before), a.saturating_sub(pad));
        mean(b + pad, b + after) < 3.0 * base + 2e-4
    })
    .collect()
}

/// The level at which a channel clips: its highest peak, when it is near full scale and held
/// on several samples in a row.
fn clip_level(x: &[f32]) -> Option<f32> {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    (peak > db_to_gain(-6.0)).then_some(peak * 0.999)
}

/// Clipped peaks: runs of three samples or more held at the channel's highest level.
pub fn find_clipping(channels: &[&[f32]]) -> Vec<Range> {
    let mut found = Vec::new();
    for x in channels {
        let Some(level) = clip_level(x) else {
            continue;
        };
        let mut run: Option<usize> = None;
        for (i, v) in x.iter().enumerate() {
            let held = v.abs() >= level && run.is_none_or(|s| x[s].signum() == v.signum());
            match (held, run) {
                (true, None) => run = Some(i),
                (false, Some(s)) => {
                    if i - s >= 3 {
                        found.push((s, i));
                    }
                    run = if v.abs() >= level { Some(i) } else { None };
                }
                _ => {}
            }
        }
        if let Some(s) = run
            && x.len() - s >= 3
        {
            found.push((s, x.len()));
        }
    }
    // The peaks of one passage together: each is still redrawn on its own.
    merge(found, 480, 1, usize::MAX)
}

/// The gain at `i` of a region `start..end` lowered or raised to `gain`, ramped over `ramp`
/// frames inside its edges.
fn ramped(i: usize, (start, end): Range, gain: f32, ramp: usize) -> f32 {
    let ramp = ramp.min((end - start) / 2).max(1);
    let into = (i - start).min(end - 1 - i) as f32;
    let t = (into / ramp as f32).min(1.0);
    1.0 + (gain - 1.0) * t
}

/// Each region lowered or raised by its dB, with ramps of `ramp_ms` inside its edges.
pub fn apply_gains(channels: &mut [Vec<f32>], rate: u32, regions: &[(Range, f32)], ramp_ms: f32) {
    let ramp = frames_of(ramp_ms, rate);
    for &((start, end), db) in regions {
        let gain = if db <= -80.0 { 0.0 } else { db_to_gain(db) };
        for x in channels.iter_mut() {
            let end = end.min(x.len());
            let first = start.min(end);
            for (i, v) in x[first..end].iter_mut().enumerate() {
                *v *= ramped(first + i, (start, end), gain, ramp);
            }
        }
    }
}

/// The gain (dB) bringing `range` to `target`; none when it is silent.
pub fn gain_to(channels: &[&[f32]], rate: u32, range: Range, target: Target) -> Option<f32> {
    let part: Vec<&[f32]> = channels
        .iter()
        .map(|c| &c[range.0.min(c.len())..range.1.min(c.len())])
        .collect();
    let peak = part
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f32, |m, v| m.max(v.abs()));
    if peak < 1e-6 {
        return None;
    }
    match target {
        Target::Peak(db) => Some(db - gain_to_db(peak)),
        Target::Loudness(lufs) => {
            let mut meter = crate::loudness::LevelMeter::new(rate);
            let (left, right) = (part[0], part.get(1).copied().unwrap_or(part[0]));
            for (l, r) in left.iter().zip(right) {
                meter.push(*l, *r);
            }
            let measured = meter.integrated();
            let level = if measured.is_finite() && measured > -70.0 {
                measured
            } else {
                // Too short to be gated: its average level.
                let power = left.iter().map(|v| v * v).sum::<f32>() / left.len().max(1) as f32;
                10.0 * power.max(1e-12).log10() - 0.7
            };
            Some(lufs - level)
        }
    }
}

/// Redraws `x` over `start..end` with a curve joining the audio on each side, its slopes
/// following.
fn redraw(x: &mut [f32], (start, end): Range) {
    if start < 2 || end + 1 >= x.len() || end <= start {
        return;
    }
    let (p0, p1) = (x[start - 1], x[end]);
    let n = (end - start + 1) as f32;
    let (m0, m1) = ((x[start - 1] - x[start - 2]) * n, (x[end + 1] - x[end]) * n);
    for (k, v) in x[start..end].iter_mut().enumerate() {
        let t = (k + 1) as f32 / n;
        let (t2, t3) = (t * t, t * t * t);
        *v = (2.0 * t3 - 3.0 * t2 + 1.0) * p0
            + (t3 - 2.0 * t2 + t) * m0
            + (-2.0 * t3 + 3.0 * t2) * p1
            + (t3 - t2) * m1;
    }
}

/// Clicks drawn over on every channel.
pub fn repair_clicks(channels: &mut [Vec<f32>], regions: &[Range]) {
    for x in channels.iter_mut() {
        for &r in regions {
            redraw(x, r);
        }
    }
}

/// Clipped runs redrawn, on the channels clipped there: the peaks they cut off come back.
pub fn repair_clipping(channels: &mut [Vec<f32>], regions: &[Range]) {
    for x in channels.iter_mut() {
        let Some(level) = clip_level(x) else {
            continue;
        };
        for &(start, end) in regions {
            let end = end.min(x.len());
            let mut i = start;
            while i < end {
                if x[i].abs() >= level {
                    let s = i;
                    while i < end && x[i].abs() >= level {
                        i += 1;
                    }
                    if i - s >= 2 {
                        redraw(x, (s, i));
                    }
                }
                i += 1;
            }
        }
    }
}

/// The highs (above 4.5 kHz) of each region lowered by its dB, the rest left as it is.
pub fn lower_highs(channels: &mut [Vec<f32>], rate: u32, regions: &[(Range, f32)]) {
    let c = Coefficients::design(FilterKind::LowPass, 4500.0, 0.707, 0.0, f64::from(rate));
    let margin = frames_of(10.0, rate);
    let ramp = frames_of(4.0, rate);
    for x in channels.iter_mut() {
        for &((start, end), db) in regions {
            let (a, b) = (start.saturating_sub(margin), (end + margin).min(x.len()));
            if b <= a || end <= start {
                continue;
            }
            // Filtered both ways: no shift of phase, highs and lows add back to the audio.
            let mut low = filtered(&x[a..b], &c, 1);
            low.reverse();
            low = filtered(&low, &c, 1);
            low.reverse();
            let gain = db_to_gain(db);
            let end = end.min(b);
            for i in start..end {
                let l = low[i - a];
                x[i] = l + (x[i] - l) * ramped(i, (start, end), gain, ramp);
            }
        }
    }
}

/// `cuts` (sorted, apart) taken out, each join crossfaded over up to `fade_ms`.
pub fn cut(channels: &mut [Vec<f32>], rate: u32, cuts: &[Range], fade_ms: f32) {
    for x in channels.iter_mut() {
        let mut out = Vec::with_capacity(x.len());
        let mut at = 0;
        for &(start, end) in cuts {
            let (start, end) = (start.min(x.len()), end.min(x.len()));
            if start < at || end <= start {
                continue;
            }
            let fade = frames_of(fade_ms, rate).min(end - start).min(start - at);
            out.extend_from_slice(&x[at..start - fade]);
            for j in 0..fade {
                let t = (j as f32 + 0.5) / fade as f32;
                out.push(x[start - fade + j] * (1.0 - t) + x[end - fade + j] * t);
            }
            at = end;
        }
        out.extend_from_slice(&x[at.min(x.len())..]);
        *x = out;
    }
}

/// The hits among `onsets` (frames, in order): attacks rising above what sounded just before
/// them (the ringing of a kick and the noise of a note let go are not hits), at least `least`
/// of the loud ones (0 to 1), the first of those within 60 ms.
pub fn hits(channels: &[&[f32]], rate: u32, onsets: &[usize], least: f32) -> Vec<usize> {
    let peak = |from: usize, to: usize| {
        channels
            .iter()
            .flat_map(|c| c[from.min(c.len())..to.min(c.len())].iter())
            .fold(0.0f32, |m, v| m.max(v.abs()))
    };
    let (after, before, skip) = (
        frames_of(30.0, rate).max(1),
        frames_of(50.0, rate),
        frames_of(4.0, rate),
    );
    let peaks: Vec<f32> = onsets.iter().map(|&o| peak(o, o + after)).collect();
    let mut sorted = peaks.clone();
    sorted.sort_by(f32::total_cmp);
    // Against the loud hits as a whole: one stray peak does not hide the others.
    let loud = sorted
        .get(sorted.len().saturating_sub(1) * 9 / 10)
        .copied()
        .unwrap_or(0.0);
    let gap = frames_of(60.0, rate);
    let mut out: Vec<usize> = Vec::new();
    for (&o, &p) in onsets.iter().zip(&peaks) {
        let rising = p > 1.2 * peak(o.saturating_sub(before), o.saturating_sub(skip));
        if rising && p >= loud * least && out.last().is_none_or(|&last| o > last + gap) {
            out.push(o);
        }
    }
    out
}

/// How much of a note keeps its attack as played when the note is stretched, ms.
const ATTACK_MS: f32 = 40.0;
/// The grain of a stretch and how far it looks for the wave to line up, ms.
const GRAIN_MS: f32 = 25.0;
const REACH_MS: f32 = 13.0;

/// How many frames of a stretch of `source` frames brought to `played` stay as they are,
/// from its start: the change of length is taken up by its end, where the note has faded.
pub fn kept_of(source: usize, played: usize, rate: u32) -> usize {
    if source == played {
        return source;
    }
    let change = source.abs_diff(played);
    let zone = (8 * change).clamp(frames_of(150.0, rate), frames_of(600.0, rate));
    let attack = frames_of(ATTACK_MS, rate).min(source / 2);
    // What is left to stretch keeps at least a third of its length.
    source
        .saturating_sub(zone)
        .max(attack)
        .min((3 * played).saturating_sub(source) / 2)
}

/// The audio moved in time: each `(at, to)` of `anchors` (in order, never crossing) brings
/// the frame `at` to play at `to`, the audio between two of them stretched or squeezed at its
/// end, its attack kept. The length stays as it is.
pub fn warp(channels: &mut [Vec<f32>], rate: u32, anchors: &[(usize, usize)]) {
    let frames = channels.iter().map(Vec::len).min().unwrap_or(0);
    let mut points = vec![(0usize, 0usize)];
    for &(at, to) in anchors {
        let last = points[points.len() - 1];
        if at > last.0 && to > last.1 && at < frames && to < frames {
            points.push((at, to));
        }
    }
    points.push((frames, frames));
    if points
        .windows(2)
        .all(|w| w[1].0 - w[0].0 == w[1].1 - w[0].1)
    {
        return;
    }
    let source: Vec<Vec<f32>> = channels.iter().map(|c| c[..frames].to_vec()).collect();
    let views: Vec<&[f32]> = source.iter().map(Vec::as_slice).collect();
    let mono = mono(&views);
    // The search for where the wave lines up, on the audio taken down to about 8 kHz.
    let down = (rate / 8000).max(1) as usize;
    let low: Vec<f32> = mono
        .chunks(down)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    let search = Search {
        mono: &mono,
        low: &low,
        down,
    };
    for w in points.windows(2) {
        let ((s0, d0), (s1, d1)) = (w[0], w[1]);
        let kept = kept_of(s1 - s0, d1 - d0, rate);
        for (out, src) in channels.iter_mut().zip(&source) {
            out[d0..d0 + kept].copy_from_slice(&src[s0..s0 + kept]);
        }
        if s1 - s0 != d1 - d0 {
            stretch_end(
                &source,
                &search,
                (s0 + kept, s1),
                channels,
                (d0 + kept, d1),
                rate,
            );
        }
    }
}

/// The audio a stretch searches: as it is, and taken down by `down`.
struct Search<'a> {
    mono: &'a [f32],
    low: &'a [f32],
    down: usize,
}

impl Search<'_> {
    /// Where, within `reach` of `wanted` and inside `lo..=hi`, the `len` frames before it
    /// resemble most those from `from`: that place and how alike they are (0 to 1).
    fn best(
        &self,
        from: usize,
        len: usize,
        wanted: usize,
        reach: usize,
        (lo, hi): Range,
    ) -> (usize, f32) {
        let score = |x: &[f32], a: usize, b: usize, n: usize| -> (f32, f32) {
            let (mut ab, mut aa, mut bb) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..n {
                let (u, v) = (x[a + i], x[b + i]);
                ab += u * v;
                aa += u * u;
                bb += v * v;
            }
            (ab / (bb.sqrt() + 1e-9), ab / ((aa * bb).sqrt() + 1e-12))
        };
        let lo = lo.max(len);
        if hi < lo {
            return (lo, 0.0);
        }
        let (first, last) = (
            wanted.saturating_sub(reach).clamp(lo, hi),
            (wanted + reach).clamp(lo, hi),
        );
        let d = self.down;
        let n = (len / d).max(1);
        let mut best = (wanted.clamp(first, last), f32::MIN);
        let mut q = first;
        while q <= last {
            let (a, b) = (from / d, (q - len) / d);
            if a + n <= self.low.len() && b + n <= self.low.len() {
                // Near where it would fall anyway, when nothing lines up better.
                let near = 1.0 - 0.05 * q.abs_diff(wanted) as f32 / reach.max(1) as f32;
                let s = score(self.low, a, b, n).0 * near;
                if s > best.1 {
                    best = (q, s);
                }
            }
            q += d;
        }
        // To the frame, around the best found.
        let (mut place, mut top, mut alike) = (best.0, f32::MIN, 0.0);
        for q in best.0.saturating_sub(d).max(first)..=(best.0 + d).min(last) {
            if from + len <= self.mono.len() && q <= self.mono.len() {
                let (s, c) = score(self.mono, from, q - len, len);
                if s > top {
                    (place, top, alike) = (q, s, c);
                }
            }
        }
        (place, alike.clamp(0.0, 1.0))
    }
}

/// `source[a..b]` played over `out[oa..ob]`: grains of it overlapped where the wave lines up
/// (WSOLA), the last one joining the audio that comes at `b`.
fn stretch_end(
    source: &[Vec<f32>],
    search: &Search,
    (a, b): Range,
    out: &mut [Vec<f32>],
    (oa, ob): Range,
    rate: u32,
) {
    let (len_in, len_out) = (b - a, ob - oa);
    if len_out == 0 {
        return;
    }
    let grain = frames_of(GRAIN_MS, rate).max(8);
    let join = frames_of(4.0, rate).max(2);
    if len_in < 2 * grain || len_out < grain {
        // Too short to stretch: cut short, or left to end before the gap.
        let n = len_in.min(len_out);
        for (o, s) in out.iter_mut().zip(source) {
            o[oa..oa + n].copy_from_slice(&s[a..a + n]);
            o[oa + n..ob].fill(0.0);
            if len_out > len_in {
                let fade = join.min(n);
                for j in 0..fade {
                    o[oa + n - fade + j] *= 1.0 - (j as f32 + 0.5) / fade as f32;
                }
            }
        }
        join_end(source, out, b, ob, join.min(len_out));
        return;
    }
    let count = (len_out / grain).max(1);
    let edge = |k: usize| oa + k * len_out / count;
    let reach = frames_of(REACH_MS, rate);
    // Where the source plays at the start of each grain.
    let mut from = a;
    for k in 0..count {
        let (start, end) = (edge(k), edge(k + 1));
        let len = end - start;
        if k + 1 == count {
            // The last grain plays on to the end.
            for (o, s) in out.iter_mut().zip(source) {
                for j in 0..len {
                    o[start + j] = s.get(from + j).copied().unwrap_or(0.0);
                }
            }
            break;
        }
        let wanted = a + (k + 1) * len_in / count;
        let next_len = edge(k + 2) - end;
        let (to, alike) = search.best(from, len, wanted, reach, (a, b.saturating_sub(next_len)));
        for (o, s) in out.iter_mut().zip(source) {
            for j in 0..len {
                let w = 0.5 - 0.5 * (std::f32::consts::PI * (j as f32 + 0.5) / len as f32).cos();
                // Unlike waves add in power: the gain keeps the level through the fade.
                let power = (1.0 - w) * (1.0 - w) + w * w + 2.0 * w * (1.0 - w) * alike;
                let leaving = s.get(from + j).copied().unwrap_or(0.0);
                let coming = s[to - len + j];
                o[start + j] = (leaving * (1.0 - w) + coming * w) / power.sqrt();
            }
        }
        from = to;
    }
    join_end(source, out, b, ob, join.min(len_out));
}

/// The last `fade` frames before `ob` of `out` faded into the source coming to `b`.
fn join_end(source: &[Vec<f32>], out: &mut [Vec<f32>], b: usize, ob: usize, fade: usize) {
    let fade = fade.min(b).min(ob);
    for (o, s) in out.iter_mut().zip(source) {
        for j in 0..fade {
            let w = (j as f32 + 0.5) / fade as f32;
            let i = ob - fade + j;
            o[i] = o[i] * (1.0 - w) + s[b - fade + j] * w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tone(seconds: f32, freq: f32, level: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| level * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn noise(seconds: f32, level: f32, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..(seconds * RATE as f32) as usize)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                level * ((s >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn the_silence_between_two_phrases_is_found() {
        let mut x = tone(0.5, 220.0, 0.5);
        x.extend(vec![0.0; RATE as usize]);
        x.extend(tone(0.5, 220.0, 0.5));
        let profile = profile(&[&x], RATE);
        let found = profile.silences(profile.auto_threshold());
        assert_eq!(found.len(), 1, "{found:?}");
        let (a, b) = found[0];
        // A tail kept after the sound, an attack before the next one.
        assert!((a as i64 - 26_880).abs() < 1_000, "{found:?}");
        assert!((b as i64 - 71_040).abs() < 1_000, "{found:?}");
    }

    #[test]
    fn a_hit_ringing_on_is_not_a_click() {
        let mut x = tone(1.0, 220.0, 0.3);
        let hit = noise(0.03, 0.5, 5);
        for (i, v) in hit.iter().enumerate() {
            x[24_000 + i] += v * (-(i as f32) / 300.0).exp();
        }
        let found = find_clicks(&[&x], RATE, 0.5);
        assert!(
            found.iter().all(|r| r.1 < 23_900 || r.0 > 25_500),
            "{found:?}"
        );
    }

    #[test]
    fn a_click_in_a_tone_is_found_and_drawn_over() {
        let mut x = tone(1.0, 220.0, 0.3);
        x[24_000] += 0.6;
        x[24_001] -= 0.4;
        let found = find_clicks(&[&x], RATE, 0.5);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0 <= 24_000 && found[0].1 > 24_001);
        let clean = tone(1.0, 220.0, 0.3);
        let mut channels = vec![x];
        repair_clicks(&mut channels, &found);
        let error = (23_990..24_010)
            .map(|i| (channels[0][i] - clean[i]).abs())
            .fold(0.0f32, f32::max);
        assert!(error < 0.02, "{error}");
    }

    #[test]
    fn a_tone_alone_has_no_click() {
        let x = tone(1.0, 3000.0, 0.8);
        assert!(find_clicks(&[&x], RATE, 1.0).is_empty());
    }

    #[test]
    fn clipped_peaks_come_back_rounded() {
        let x: Vec<f32> = tone(0.1, 100.0, 1.4)
            .into_iter()
            .map(|v| v.clamp(-1.0, 1.0))
            .collect();
        let found = find_clipping(&[&x]);
        assert!(!found.is_empty());
        let mut channels = vec![x];
        repair_clipping(&mut channels, &found);
        let peak = channels[0].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 1.05, "the peak redrawn above the cut: {peak}");
    }

    #[test]
    fn a_burst_of_highs_in_a_voice_is_a_sibilant() {
        let mut x = tone(0.4, 200.0, 0.4);
        let mut hiss = noise(0.12, 0.4, 3);
        let c = Coefficients::design(FilterKind::HighPass, 6000.0, 0.707, 0.0, f64::from(RATE));
        hiss = filtered(&hiss, &c, 2);
        x.extend(hiss);
        x.extend(tone(0.4, 200.0, 0.4));
        let found = profile(&[&x], RATE).sibilants();
        assert_eq!(found.len(), 1, "{found:?}");
        let mut channels = vec![x.clone()];
        lower_highs(&mut channels, RATE, &[(found[0], -12.0)]);
        let energy = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>();
        let mid = 19_200 + 1_000..19_200 + 4_700;
        assert!(energy(&channels[0][mid.clone()]) < energy(&x[mid]) * 0.2);
        assert_eq!(channels[0][..10_000], x[..10_000], "untouched away from it");
    }

    #[test]
    fn a_soft_noise_before_a_phrase_is_a_breath() {
        let mut x = tone(0.6, 180.0, 0.5);
        x.extend(vec![0.0; 4_800]);
        let c = Coefficients::design(FilterKind::LowPass, 2500.0, 0.707, 0.0, f64::from(RATE));
        x.extend(filtered(&noise(0.35, 0.05, 9), &c, 1));
        x.extend(vec![0.0; 4_800]);
        x.extend(tone(0.6, 180.0, 0.5));
        let profile = profile(&[&x], RATE);
        let found = profile.breaths(profile.auto_threshold());
        assert_eq!(found.len(), 1, "{found:?}");
        let (a, b) = found[0];
        assert!(a >= 32_000 && b <= 52_000, "{found:?}");
        let silences = profile.silences(profile.auto_threshold());
        assert!(
            silences.iter().all(|s| s.1 <= a || s.0 >= b),
            "the breath is not a silence: {silences:?}"
        );
    }

    #[test]
    fn normalizing_a_part_finds_its_gain() {
        let x = tone(1.0, 440.0, 0.25);
        let db = gain_to(&[&x], RATE, (0, x.len()), Target::Peak(-1.0)).unwrap();
        assert!((db - (-1.0 - gain_to_db(0.25))).abs() < 0.05);
        let lufs = gain_to(&[&x], RATE, (0, x.len()), Target::Loudness(-20.0)).unwrap();
        assert!(lufs.is_finite());
    }

    #[test]
    fn gains_ramp_inside_their_region_only() {
        let mut channels = vec![vec![1.0f32; 1_000]];
        apply_gains(&mut channels, 1_000, &[((100, 300), -80.0)], 10.0);
        assert_eq!(channels[0][99], 1.0);
        assert_eq!(channels[0][200], 0.0);
        assert!(channels[0][102] > 0.0 && channels[0][102] < 1.0);
        assert_eq!(channels[0][300], 1.0);
    }

    #[test]
    fn a_cut_shortens_by_its_length() {
        let mut channels = vec![(0..1_000).map(|i| i as f32).collect::<Vec<_>>()];
        cut(&mut channels, 1_000, &[(200, 500)], 10.0);
        assert_eq!(channels[0].len(), 700);
        assert_eq!(channels[0][100], 100.0);
        assert_eq!(channels[0][200], 500.0, "the audio after the cut moved up");
        assert_eq!(channels[0][699], 999.0);
    }
}
