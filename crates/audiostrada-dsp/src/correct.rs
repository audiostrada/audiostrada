//! The engine of Strada-Correct: the pitch of a voice followed every 5 ms (a normalized
//! difference on a low-passed, decimated copy, the candidate nearest to the course of the
//! pitch kept, refined at full rate), each reading dated at the middle of what it compared.
//! The audio waits behind the readings, so that a grain is moved by the pitch the voice had
//! where the grain is, not by the one it had a moment before: a vibrato is flattened whole,
//! a note is on its target from its first cycle. The voice is moved with TD-PSOLA, grains of
//! two periods taken at marks one period of the voice apart, which keeps the formants. The
//! notes sung also tell the key of the song.

use crate::biquad::{Biquad, Coefficients, FilterKind};
use crate::pitch_correct::in_scale;
use crate::time_coefficient;

const LOWEST_HZ: f32 = 70.0;
const HIGHEST_HZ: f32 = 1200.0;
/// Rate the pitch is first looked for at.
const DETECTION_HZ: f32 = 12_000.0;
/// The voice's fundamental and first harmonics: what is above blurs the period.
const DETECTION_BAND: (f32, f32) = (60.0, 1500.0);
const HOP_SECONDS: f32 = 0.005;
/// Stretch of signal compared with the one a period before.
const WINDOW_SECONDS: f32 = 0.0125;
/// Below this mean square (-60 dBFS) there is nothing to follow.
const SILENCE: f32 = 1e-6;
/// Depth of the difference's dip under which a note starts, and under which it goes on.
const ONSET: f32 = 0.2;
const CONTINUE: f32 = 0.5;
/// How far the period may move from one reading to the next and still be the same course.
const COURSE: f32 = 0.12;
/// Unvoiced readings after which the next note starts afresh (60 ms).
const GAP: u32 = 12;
/// How much further the current note may be than the nearest one before it changes.
const HYSTERESIS: f32 = 0.15;
/// The same for the staircase, whose steps must not flutter.
const STEP_HYSTERESIS: f32 = 0.25;
/// Pseudo-period of the grains when the sound has no pitch.
const UNVOICED_SECONDS: f32 = 0.004;
const MARKS: usize = 128;
/// The least sum of the grains' windows made up for.
const THINNEST: f32 = 0.4;
const FRAMES: usize = 64;
/// Full-rate lags tried on each side of the decimated estimate.
const REACH: usize = 16;
/// Readings between two guesses of the key, and those needed before the first.
const KEY_EVERY: u32 = 100;
const KEY_NEEDS: f32 = 400.0;

/// How much each degree weighs in a major and in a minor key (Krumhansl and Kessler).
const MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

fn to_note(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

fn to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// What the correction is set to.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Settings {
    pub key: i32,
    pub scale: usize,
    pub notes: u32,
    pub speed: f32,
    pub humanize: f32,
    pub amount: f32,
    pub transpose: f32,
    pub tolerance: f32,
    /// Every note flat on its target at once, whatever the other settings.
    pub staircase: bool,
}

/// A reading of the pitch: when, the note sung (0 without one), and the move decided there.
#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    at: f64,
    note: f32,
    correction: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Mark {
    at: f64,
    period: f32,
    voiced: bool,
}

#[derive(Debug)]
pub(crate) struct Corrector {
    rate: f32,
    /// Detection: one sample in `factor`, band-passed first.
    factor: usize,
    band: [Coefficients; 3],
    filters: [Biquad; 3],
    shortest: usize,
    longest: usize,
    window: usize,
    hop: usize,
    decimated: Vec<f32>,
    filled: usize,
    difference: Vec<f32>,
    /// Period of the last reading, in decimated samples, and unvoiced readings since.
    course: Option<f32>,
    gap: u32,
    frames: [Frame; FRAMES],
    frame_count: usize,
    /// Full-rate rings: input per channel, its mono sum, overlap-added grains and windows.
    mask: usize,
    input: [Vec<f32>; 2],
    mono: Vec<f32>,
    output: [Vec<f32>; 2],
    weight: Vec<f32>,
    written: u64,
    latency: usize,
    marks: [Mark; MARKS],
    count: usize,
    next_mark: f64,
    next_grain: f64,
    /// How far from adding up in amplitude the overlapping grains are: 0 for a correction,
    /// most of the way for a voice moved three semitones up or more.
    crowding: f32,
    /// The note followed: its slow center, the target, the move gliding to it.
    center: f32,
    note: Option<i32>,
    correction: f32,
    clarity: f32,
    /// How long each pitch class was sung, and the key it tells.
    chroma: [f32; 12],
    since_key: u32,
    key: Option<(usize, bool, f32)>,
}

impl Corrector {
    pub(crate) fn new(rate: f32) -> Self {
        let mut c = Self {
            rate: 0.0,
            factor: 1,
            band: [Coefficients::IDENTITY; 3],
            filters: [Biquad::default(); 3],
            shortest: 0,
            longest: 0,
            window: 0,
            hop: 1,
            decimated: Vec::new(),
            filled: 0,
            difference: Vec::new(),
            course: None,
            gap: GAP,
            frames: [Frame::default(); FRAMES],
            frame_count: 0,
            mask: 0,
            input: [Vec::new(), Vec::new()],
            mono: Vec::new(),
            output: [Vec::new(), Vec::new()],
            weight: Vec::new(),
            written: 0,
            latency: 0,
            marks: [Mark::default(); MARKS],
            count: 0,
            next_mark: 0.0,
            next_grain: 0.0,
            crowding: 0.0,
            center: 0.0,
            note: None,
            correction: 0.0,
            clarity: 0.0,
            chroma: [0.0; 12],
            since_key: 0,
            key: None,
        };
        c.allocate(rate);
        c
    }

    pub(crate) fn rate(&self) -> f32 {
        self.rate
    }

    pub(crate) fn allocate(&mut self, rate: f32) {
        self.rate = rate;
        self.factor = ((rate / DETECTION_HZ).round() as usize).max(1);
        let detection = rate / self.factor as f32;
        let design = |kind, hz: f32, q: f64| {
            Coefficients::design(kind, f64::from(hz), q, 0.0, f64::from(rate))
        };
        // Filtered at full rate, so that decimating folds nothing back.
        self.band = [
            design(FilterKind::HighPass, DETECTION_BAND.0, 0.707),
            design(FilterKind::LowPass, DETECTION_BAND.1, 0.541_196_1),
            design(FilterKind::LowPass, DETECTION_BAND.1, 1.306_563),
        ];
        self.shortest = ((detection / HIGHEST_HZ) as usize).max(2);
        self.longest = (detection / LOWEST_HZ).ceil() as usize + 1;
        self.window = (detection * WINDOW_SECONDS).round() as usize;
        self.hop = ((detection * HOP_SECONDS).round() as usize).max(1);
        self.decimated = vec![0.0; self.window + self.longest + 2];
        self.difference = vec![0.0; self.longest + 2];
        let longest_period = rate / LOWEST_HZ;
        // A grain reaches a period after its center, and waits a period and a half of
        // input after it for its mark; the readings are never older than that.
        self.latency = (2.5 * longest_period).ceil() as usize + 4;
        let size = (self.latency + 4 * longest_period as usize + 64).next_power_of_two();
        self.mask = size - 1;
        self.input = [vec![0.0; size], vec![0.0; size]];
        self.mono = vec![0.0; size];
        self.output = [vec![0.0; size], vec![0.0; size]];
        self.weight = vec![0.0; size];
        self.chroma = [0.0; 12];
        self.since_key = 0;
        self.key = None;
        self.reset();
    }

    pub(crate) fn reset(&mut self) {
        self.filters = [Biquad::default(); 3];
        self.filled = 0;
        for ring in self
            .input
            .iter_mut()
            .chain(self.output.iter_mut())
            .chain([&mut self.mono, &mut self.weight])
        {
            ring.fill(0.0);
        }
        self.written = 0;
        self.count = 0;
        self.frame_count = 0;
        self.next_mark = 0.0;
        self.next_grain = 0.0;
        self.crowding = 0.0;
        self.course = None;
        self.gap = GAP;
        self.note = None;
        self.correction = 0.0;
        self.clarity = 0.0;
    }

    pub(crate) fn latency(&self) -> usize {
        self.latency
    }

    fn unvoiced_period(&self) -> f32 {
        (self.rate * UNVOICED_SECONDS).round()
    }

    fn detect(&mut self, mono: f32, settings: &Settings) {
        let mut x = f64::from(mono);
        for (f, c) in self.filters.iter_mut().zip(&self.band) {
            x = f.process(c, x);
        }
        if !(self.written as usize).is_multiple_of(self.factor) {
            return;
        }
        self.decimated[self.filled] = x as f32;
        self.filled += 1;
        if self.filled == self.decimated.len() {
            self.analyse(settings);
            self.decimated.copy_within(self.hop.., 0);
            self.filled -= self.hop;
        }
    }

    /// The normalized difference between the latest window and the one each lag before;
    /// returns the longest lag compared.
    fn differences(&mut self) -> usize {
        let x = &self.decimated;
        let start = x.len() - self.window;
        let max_lag = self.longest.min(start).min(self.difference.len() - 2);
        let mut running = 0.0;
        self.difference[0] = 1.0;
        for lag in 1..=max_lag + 1 {
            let mut sum = 0.0;
            for i in 0..self.window {
                let d = x[start + i] - x[start + i - lag];
                sum += d * d;
            }
            running += sum;
            self.difference[lag] = if running > 0.0 {
                sum * lag as f32 / running
            } else {
                1.0
            };
        }
        max_lag
    }

    /// The dip of the difference at `lag`, refined between its neighbors: (lag, depth).
    fn dip(&self, lag: usize) -> (f32, f32) {
        let d = &self.difference;
        let (a, b, c) = (d[lag - 1], d[lag], d[lag + 1]);
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1e-9 {
            (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        (lag as f32 + shift, b)
    }

    /// The period of the latest window in decimated samples, if it has one: the dip that
    /// goes on the course of the last readings, else the first deep one.
    fn period(&mut self) -> Option<(f32, f32)> {
        let max_lag = self.differences();
        let d = &self.difference;
        let is_dip = |l: usize| d[l] <= d[l - 1] && d[l] < d[l + 1];
        let first = self.shortest.max(2);
        if let Some(before) = self.course {
            let reach = (before * COURSE).max(1.5);
            let lo = ((before - reach).floor() as usize).max(first);
            let hi = ((before + reach).ceil() as usize).min(max_lag - 1);
            let near = (lo..=hi)
                .filter(|&l| is_dip(l) && d[l] < CONTINUE)
                .min_by(|&a, &b| d[a].total_cmp(&d[b]));
            if let Some(lag) = near {
                return Some(self.dip(lag));
            }
        }
        // A new note: the first dip deep enough, as YIN picks it.
        let mut lag = (first..max_lag).find(|&l| d[l] < ONSET)?;
        while lag + 1 < max_lag && d[lag + 1] < d[lag] {
            lag += 1;
        }
        Some(self.dip(lag))
    }

    /// Distance between the latest `window` samples and the ones `lag` before, at full rate.
    fn full_rate_difference(&self, lag: usize, window: usize) -> f32 {
        let end = self.written as usize - 1;
        (0..window)
            .map(|i| {
                let at = end.wrapping_sub(i);
                let d = self.mono[at & self.mask] - self.mono[at.wrapping_sub(lag) & self.mask];
                d * d
            })
            .sum()
    }

    /// Refines a period found on the decimated copy, near `guess` full-rate samples.
    fn refine(&self, guess: f32) -> f32 {
        let window = self.window * self.factor;
        let center = guess.round() as usize;
        let reach = self.factor.min(REACH);
        if self.factor == 1 || center <= reach + 2 {
            return guess;
        }
        let mut sums = [0.0f32; 2 * REACH + 1];
        let sums = &mut sums[..2 * reach + 1];
        for (k, sum) in sums.iter_mut().enumerate() {
            *sum = self.full_rate_difference(center - reach + k, window);
        }
        let best = (1..sums.len() - 1)
            .min_by(|&a, &b| sums[a].total_cmp(&sums[b]))
            .unwrap_or(reach);
        if sums[best] > sums[0] || sums[best] > sums[sums.len() - 1] {
            return guess;
        }
        let (a, b, c) = (sums[best - 1], sums[best], sums[best + 1]);
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1e-12 {
            (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        (center - reach + best) as f32 + shift
    }

    fn analyse(&mut self, settings: &Settings) {
        let start = self.decimated.len() - self.window;
        let energy =
            self.decimated[start..].iter().map(|s| s * s).sum::<f32>() / self.window as f32;
        let found = if energy > SILENCE {
            self.period()
        } else {
            None
        };
        let now = (self.written - 1) as f64;
        let hop_rate = self.rate / (self.hop * self.factor) as f32;
        let (at, note) = match found {
            Some((lag, depth)) => {
                self.course = Some(lag);
                self.clarity = (1.0 - depth).clamp(0.0, 1.0);
                let period = self.refine(lag * self.factor as f32);
                // What was compared is centered half a window and half a period back.
                let age = 0.5 * (self.window * self.factor) as f32 + 0.5 * period;
                (now - f64::from(age), to_note(self.rate / period))
            }
            None => {
                self.course = None;
                self.clarity = 0.0;
                let age = 0.5 * (self.window * self.factor) as f32;
                (now - f64::from(age), 0.0)
            }
        };
        if note > 0.0 {
            self.judge(note, settings, hop_rate);
            self.gap = 0;
            self.listen(note);
        } else {
            self.gap = self.gap.saturating_add(1);
            if self.gap > GAP {
                self.note = None;
            }
        }
        // Readings stay in order: a longer period dates its reading earlier.
        let last = self
            .frame_count
            .checked_sub(1)
            .map(|i| self.frames[i % FRAMES].at);
        let at = last.map_or(at, |last| at.max(last + 1.0));
        self.frames[self.frame_count % FRAMES] = Frame {
            at,
            note,
            correction: self.correction,
        };
        self.frame_count += 1;
    }

    /// Chooses the target of the note sung and moves the correction toward it.
    fn judge(&mut self, pitch: f32, s: &Settings, hop_rate: f32) {
        let fresh = self.note.is_none();
        if fresh {
            self.center = pitch;
            self.correction = 0.0;
        } else {
            let c = time_coefficient(150.0, hop_rate);
            self.center = pitch + c * (self.center - pitch);
        }
        let humanize = if s.staircase { 0.0 } else { s.humanize };
        let heard = pitch + humanize / 100.0 * (self.center - pitch);
        let base = heard.round() as i32;
        let nearest = (base - 6..=base + 6)
            .filter(|&n| in_scale(n, s.key, s.scale, s.notes))
            .min_by(|&a, &b| {
                (heard - a as f32)
                    .abs()
                    .total_cmp(&(heard - b as f32).abs())
            })
            .unwrap_or(base);
        let hysteresis = if s.staircase {
            STEP_HYSTERESIS
        } else {
            HYSTERESIS
        };
        let note = match self.note {
            Some(current)
                if in_scale(current, s.key, s.scale, s.notes)
                    && (heard - current as f32).abs()
                        < (heard - nearest as f32).abs() + hysteresis =>
            {
                current
            }
            _ => nearest,
        };
        self.note = Some(note);
        let off = note as f32 - heard;
        if s.staircase {
            self.correction = off + s.transpose.round();
            return;
        }
        // Close enough already: left as sung (flex tuning).
        let off = if off.abs() * 100.0 < s.tolerance {
            0.0
        } else {
            off
        };
        let goal = off * s.amount / 100.0 + s.transpose.round();
        let c = time_coefficient(s.speed, hop_rate);
        self.correction = goal + c * (self.correction - goal);
    }

    /// Counts the note sung toward the key of the song.
    fn listen(&mut self, pitch: f32) {
        if self.clarity > 0.7 {
            // Only what is sung on a note: a glide between two says nothing.
            let off = pitch - pitch.round();
            if off.abs() < 0.35 {
                self.chroma[(pitch.round() as i32).rem_euclid(12) as usize] += 1.0;
            }
        }
        for c in &mut self.chroma {
            // Halved in a minute or so: the key follows a song that changes.
            *c *= 0.999_95;
        }
        self.since_key += 1;
        if self.since_key >= KEY_EVERY {
            self.since_key = 0;
            self.key = key_of(&self.chroma);
        }
    }

    /// The reading nearest before `at` and the one after, or the newest twice past it.
    fn around(&self, at: f64) -> Option<(Frame, Frame)> {
        let newest = self.frames[self.frame_count.checked_sub(1)? % FRAMES];
        let mut after = newest;
        for back in 1..self.frame_count.min(FRAMES) {
            let frame = self.frames[(self.frame_count - 1 - back) % FRAMES];
            if frame.at <= at {
                return Some((frame, after));
            }
            after = frame;
        }
        Some((after, after))
    }

    /// The period of the voice at `at` in samples, if it has a pitch there.
    fn period_at(&self, at: f64) -> Option<f32> {
        let (a, b) = self.around(at)?;
        let note = if a.note > 0.0 && b.note > 0.0 && b.at > a.at {
            let t = ((at - a.at) / (b.at - a.at)).clamp(0.0, 1.0) as f32;
            a.note + t * (b.note - a.note)
        } else if at - a.at <= b.at - at {
            a.note
        } else {
            b.note
        };
        (note > 0.0).then(|| self.rate / to_hz(note))
    }

    /// The move of the pitch at `at`, in semitones.
    fn correction_at(&self, at: f64) -> f32 {
        let Some((a, b)) = self.around(at) else {
            return 0.0;
        };
        match (a.note > 0.0, b.note > 0.0) {
            (true, true) if b.at > a.at => {
                let t = ((at - a.at) / (b.at - a.at)).clamp(0.0, 1.0) as f32;
                a.correction + t * (b.correction - a.correction)
            }
            (true, _) => a.correction,
            (_, true) => b.correction,
            _ => 0.0,
        }
    }

    /// Time of the newest reading: the pitch is known up to there.
    fn horizon(&self) -> Option<f64> {
        Some(self.frames[self.frame_count.checked_sub(1)? % FRAMES].at)
    }

    fn place_marks(&mut self) {
        let Some(horizon) = self.horizon() else {
            return;
        };
        loop {
            let found = self.period_at(self.next_mark.min(horizon));
            let period = found.unwrap_or_else(|| self.unvoiced_period());
            // The pitch holds for half a period past its last reading.
            if self.next_mark > horizon + 0.5 * f64::from(period) {
                return;
            }
            self.marks[self.count % MARKS] = Mark {
                at: self.next_mark,
                period,
                voiced: found.is_some(),
            };
            self.count += 1;
            self.next_mark += f64::from(period);
        }
    }

    /// The mark nearest to `at`, once no later one can be nearer.
    fn nearest_mark(&self, at: f64) -> Option<Mark> {
        let newest = self.marks[(self.count.checked_sub(1)?) % MARKS];
        if newest.at < at - 0.5 * f64::from(newest.period) {
            return None;
        }
        let mut best = newest;
        for back in 1..self.count.min(MARKS) {
            let mark = self.marks[(self.count - 1 - back) % MARKS];
            if (mark.at - at).abs() < (best.at - at).abs() {
                best = mark;
            } else if mark.at < at {
                break;
            }
        }
        Some(best)
    }

    fn place_grains(&mut self) {
        let Some(horizon) = self.horizon() else {
            return;
        };
        let now = (self.written - 1) as f64;
        while self.next_grain <= horizon {
            let Some(mark) = self.nearest_mark(self.next_grain) else {
                return;
            };
            let half = f64::from(mark.period);
            if mark.at + half + 3.0 > now {
                return;
            }
            let at = self.next_grain;
            let first = (at - half).ceil().max(0.0) as u64;
            let last = (at + half).floor() as u64;
            for j in first..=last {
                let x = j as f64 - at;
                let w = (0.5 + 0.5 * (std::f64::consts::PI * x / half).cos()) as f32;
                let source = mark.at + x;
                let slot = j as usize & self.mask;
                for ch in 0..2 {
                    self.output[ch][slot] += w * cubic(&self.input[ch], self.mask, source);
                }
                self.weight[slot] += w;
            }
            let semitones = if mark.voiced {
                self.correction_at(at).clamp(-12.0, 12.0)
            } else {
                0.0
            };
            self.crowding = 0.85 * (semitones / 3.0).clamp(0.0, 1.0);
            let ratio = 2f32.powf(semitones / 12.0);
            self.next_grain += f64::from(mark.period / ratio);
        }
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32], settings: &Settings) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let slot = self.written as usize & self.mask;
            self.input[0][slot] = *l;
            self.input[1][slot] = *r;
            let mono = 0.5 * (*l + *r);
            self.mono[slot] = mono;
            self.written += 1;
            self.detect(mono, settings);
            self.place_marks();
            self.place_grains();
            if self.written > self.latency as u64 {
                let slot = (self.written - 1 - self.latency as u64) as usize & self.mask;
                // Where the windows do not add up to one (a change of period, a voice moved
                // down), the sum is brought back to it.
                let w = self.weight[slot].max(THINNEST);
                // Grains crowded by a voice moved far up are different cycles, which add up
                // in power, not in amplitude.
                let gain = if w > 1.0 && self.crowding > 0.0 {
                    w.powf(self.crowding - 1.0)
                } else {
                    1.0 / w
                };
                *l = self.output[0][slot] * gain;
                *r = self.output[1][slot] * gain;
                self.output[0][slot] = 0.0;
                self.output[1][slot] = 0.0;
                self.weight[slot] = 0.0;
            } else {
                *l = 0.0;
                *r = 0.0;
            }
        }
    }

    /// Input pitch, output pitch, target note, clarity (zeros without a note), then the key
    /// heard (-1 without one), its scale (1 major, 2 minor) and how sure it is.
    pub(crate) fn meters(&self, out: &mut [f32]) {
        let newest = self
            .frame_count
            .checked_sub(1)
            .map(|i| self.frames[i % FRAMES])
            .filter(|f| f.note > 0.0);
        let (key, scale, sure) = match self.key {
            Some((key, minor, sure)) => (key as f32, if minor { 2.0 } else { 1.0 }, sure),
            None => (-1.0, 0.0, 0.0),
        };
        let values = match newest {
            Some(frame) => [
                frame.note,
                frame.note + frame.correction,
                self.note.unwrap_or(0) as f32,
                self.clarity,
                key,
                scale,
                sure,
            ],
            None => [0.0, 0.0, 0.0, self.clarity, key, scale, sure],
        };
        for (o, v) in out.iter_mut().zip(values) {
            *o = v;
        }
    }
}

/// The key the time spent on each pitch class tells: its root, whether it is minor, and how
/// well it fits (0 to 1). None before enough was sung.
fn key_of(chroma: &[f32; 12]) -> Option<(usize, bool, f32)> {
    let total: f32 = chroma.iter().sum();
    if total < KEY_NEEDS {
        return None;
    }
    let mean = total / 12.0;
    let spread: f32 = chroma.iter().map(|c| (c - mean) * (c - mean)).sum();
    if spread <= 0.0 {
        return None;
    }
    let mut best = (f32::MIN, 0, false);
    for (profile, minor) in [(&MAJOR, false), (&MINOR, true)] {
        let profile_mean = profile.iter().sum::<f32>() / 12.0;
        let profile_spread: f32 = profile
            .iter()
            .map(|p| (p - profile_mean) * (p - profile_mean))
            .sum();
        for root in 0..12 {
            let dot: f32 = (0..12)
                .map(|pc| (chroma[(root + pc) % 12] - mean) * (profile[pc] - profile_mean))
                .sum();
            let fit = dot / (spread * profile_spread).sqrt();
            if fit > best.0 {
                best = (fit, root, minor);
            }
        }
    }
    Some((best.1, best.2, best.0.clamp(0.0, 1.0)))
}

/// Cubic Hermite interpolation of the ring `x` at the fractional position `at`.
fn cubic(x: &[f32], mask: usize, at: f64) -> f32 {
    let i = at.floor();
    let t = (at - i) as f32;
    let i = i as i64;
    let s = |k: i64| x[(i + k) as usize & mask];
    let (y0, y1, y2, y3) = (s(-1), s(0), s(1), s(2));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_told_from_the_notes_sung() {
        // A major scale on D, the tonic and the fifth held longer.
        let mut chroma = [0.0; 12];
        for (step, weight) in [
            (0, 6.0),
            (2, 3.0),
            (4, 4.0),
            (5, 3.0),
            (7, 5.0),
            (9, 3.0),
            (11, 2.0),
        ] {
            chroma[(2 + step) % 12] = weight * 40.0;
        }
        let (root, minor, sure) = key_of(&chroma).unwrap();
        assert_eq!((root, minor), (2, false));
        assert!(sure > 0.8, "{sure}");
        // A natural minor on A.
        let mut chroma = [0.0; 12];
        for (step, weight) in [
            (0, 6.0),
            (2, 2.5),
            (3, 4.5),
            (5, 3.0),
            (7, 4.5),
            (8, 3.0),
            (10, 3.0),
        ] {
            chroma[(9 + step) % 12] = weight * 40.0;
        }
        let (root, minor, _) = key_of(&chroma).unwrap();
        assert_eq!((root, minor), (9, true));
        assert!(key_of(&[1.0; 12]).is_none(), "too little, or nothing told");
    }
}
