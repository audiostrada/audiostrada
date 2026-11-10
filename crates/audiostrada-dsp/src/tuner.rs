//! Tuner: detects the pitch of the input (YIN on the mono sum, about twenty-five times a second,
//! its search spread over the blocks in between) and reports it, smoothed while the note holds
//! so that the needle does not shake; the signal passes through, or is muted while tuning.

use crate::{Effect, EffectInfo, ParamInfo, choice, param, switch};

pub const INFO: EffectInfo = EffectInfo {
    id: "tuner",
    name: "Strada-Tuner",
    params: &PARAMS,
    meters: &["Frequency", "Clarity"],
    instrument: false,
    files: &[],
    category: "Utility",
    description: "Chromatic tuner for guitar, bass and voice.",
};

pub const VIEWS: [&str; 2] = ["Needle", "Strobe"];

const PARAMS: [ParamInfo; 3] = [
    param("reference", "Reference A4", 415.0, 466.0, 440.0, "Hz"),
    switch("mute", "Mute while tuning", 0.0),
    choice("view", "View", &VIEWS, 0.0),
];
/// How much of each new reading the shown pitch takes while the note holds.
const SMOOTHING: f32 = 0.35;

const WINDOW: usize = 4096;
const HOP: usize = 2048;
/// Lowest note tracked: a little below a five-string bass low B.
const LOWEST_HZ: f32 = 28.0;
const HIGHEST_HZ: f32 = 1400.0;

#[derive(Debug)]
pub struct Tuner {
    rate: f32,
    values: [f32; 3],
    input: Vec<f32>,
    filled: usize,
    difference: Vec<f32>,
    /// The window being searched, while `search` runs.
    analysis: Vec<f32>,
    search: Option<Search>,
    frequency: f32,
    clarity: f32,
}

/// A YIN search under way: the next lag to compare, the last one, the running sum.
#[derive(Debug, Clone, Copy)]
struct Search {
    next: usize,
    max_lag: usize,
    running: f32,
}

impl Default for Tuner {
    fn default() -> Self {
        Self {
            rate: 48_000.0,
            values: PARAMS.map(|p| p.default),
            input: vec![0.0; WINDOW],
            filled: 0,
            difference: vec![0.0; WINDOW / 2],
            analysis: vec![0.0; WINDOW],
            search: None,
            frequency: 0.0,
            clarity: 0.0,
        }
    }
}

/// YIN: fundamental frequency of `x` and how periodic it is (0 to 1).
pub fn detect(x: &[f32], rate: f32, difference: &mut [f32]) -> Option<(f32, f32)> {
    let half = difference.len().min(x.len() / 2);
    let energy = x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32;
    if energy < 1e-7 {
        return None;
    }
    let shortest = (rate / HIGHEST_HZ) as usize;
    let (lag, clarity) = yin(x, half, shortest, (rate / LOWEST_HZ) as usize, difference)?;
    Some((rate / lag, clarity))
}

/// YIN: period of `x` in samples, between `shortest` and `longest`, and how periodic it is
/// (0 to 1), comparing its first `window` samples with the ones a period later.
pub(crate) fn yin(
    x: &[f32],
    window: usize,
    shortest: usize,
    longest: usize,
    difference: &mut [f32],
) -> Option<(f32, f32)> {
    let max_lag = lag_limit(x.len(), window, longest, difference.len())?;
    difference[0] = 1.0;
    let mut running = 0.0;
    differences(x, window, 1..max_lag + 2, &mut running, difference);
    pick(difference, shortest, max_lag)
}

/// The longest lag a search can compare in `len` samples.
fn lag_limit(len: usize, window: usize, longest: usize, room: usize) -> Option<usize> {
    let max_lag = longest
        .min(len.saturating_sub(window + 1))
        .min(room.saturating_sub(2));
    (max_lag >= 3).then_some(max_lag)
}

/// Cumulative mean normalized differences of `x` for `lags`, after those before them.
fn differences(
    x: &[f32],
    window: usize,
    lags: std::ops::Range<usize>,
    running: &mut f32,
    difference: &mut [f32],
) {
    for lag in lags {
        let mut sum = 0.0;
        for i in 0..window {
            let d = x[i] - x[i + lag];
            sum += d * d;
        }
        *running += sum;
        difference[lag] = if *running > 0.0 {
            sum * lag as f32 / *running
        } else {
            1.0
        };
    }
}

/// The first dip under the threshold, refined between its neighbors, and its clarity.
fn pick(difference: &[f32], shortest: usize, max_lag: usize) -> Option<(f32, f32)> {
    let threshold = 0.15;
    let mut lag = (shortest.max(2)..max_lag).find(|&l| difference[l] < threshold)?;
    while lag + 1 < max_lag && difference[lag + 1] < difference[lag] {
        lag += 1;
    }
    let (a, b, c) = (difference[lag - 1], difference[lag], difference[lag + 1]);
    let denominator = a - 2.0 * b + c;
    let shift = if denominator.abs() > 1e-9 {
        (0.5 * (a - c) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Some((lag as f32 + shift, (1.0 - b).clamp(0.0, 1.0)))
}

/// Nearest note to `frequency` (MIDI number) and the offset in cents.
pub fn note(frequency: f32, reference: f32) -> (i32, f32) {
    let semitones = 12.0 * (frequency / reference).log2() + 69.0;
    let nearest = semitones.round();
    (nearest as i32, (semitones - nearest) * 100.0)
}

impl Tuner {
    /// Takes the full window for a new search, unless it is silent.
    fn start(&mut self) {
        self.analysis.copy_from_slice(&self.input);
        let energy = self.analysis.iter().map(|s| s * s).sum::<f32>() / WINDOW as f32;
        let longest = (self.rate / LOWEST_HZ) as usize;
        match lag_limit(WINDOW, WINDOW / 2, longest, self.difference.len()) {
            Some(max_lag) if energy >= 1e-7 => {
                self.difference[0] = 1.0;
                self.search = Some(Search {
                    next: 1,
                    max_lag,
                    running: 0.0,
                });
            }
            _ => self.clarity = 0.0,
        }
    }

    /// Compares up to `lags` more lags, and reads the pitch once all are done.
    fn search(&mut self, lags: usize) {
        let Some(mut search) = self.search else {
            return;
        };
        let end = search.next.saturating_add(lags).min(search.max_lag + 2);
        differences(
            &self.analysis,
            WINDOW / 2,
            search.next..end,
            &mut search.running,
            &mut self.difference,
        );
        search.next = end;
        if end <= search.max_lag + 1 {
            self.search = Some(search);
            return;
        }
        self.search = None;
        let shortest = (self.rate / HIGHEST_HZ) as usize;
        match pick(&self.difference, shortest, search.max_lag) {
            Some((lag, clarity)) => {
                let f = self.rate / lag;
                // Same note: glide in cents; a new note: at once.
                let cents = 1200.0 * (f / self.frequency.max(1.0)).log2();
                self.frequency = if self.frequency > 0.0 && cents.abs() < 50.0 {
                    self.frequency * 2f32.powf(cents * SMOOTHING / 1200.0)
                } else {
                    f
                };
                self.clarity = clarity;
            }
            None => self.clarity = 0.0,
        }
    }
}

impl Effect for Tuner {
    fn prepare(&mut self, sample_rate: u32, _max_frames: usize) {
        self.rate = sample_rate as f32;
        self.filled = 0;
        self.search = None;
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(v) = self.values.get_mut(index) {
            *v = value;
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter().zip(right.iter()) {
            self.input[self.filled] = 0.5 * (l + r);
            self.filled += 1;
            if self.filled == WINDOW {
                self.search(usize::MAX);
                self.start();
                self.input.copy_within(HOP.., 0);
                self.filled = WINDOW - HOP;
            }
        }
        // Enough lags each block for the search to end before the next window.
        if let Some(search) = self.search {
            self.search((search.max_lag + 1) * left.len() / HOP + 1);
        }
        if self.values[1] >= 0.5 {
            left.fill(0.0);
            right.fill(0.0);
        }
    }

    fn meters(&self, out: &mut [f32]) {
        for (o, v) in out.iter_mut().zip([self.frequency, self.clarity]) {
            *o = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_guitar_and_bass_notes() {
        let mut difference = vec![0.0; WINDOW / 2];
        for f in [41.2, 82.41, 110.0, 196.0, 329.63, 440.0] {
            // A rich waveform, as from a string: fundamental and harmonics.
            let x: Vec<f32> = (0..WINDOW)
                .map(|i| {
                    let t = i as f32 / 48_000.0 * f * std::f32::consts::TAU;
                    0.5 * t.sin() + 0.3 * (2.0 * t).sin() + 0.2 * (3.0 * t).sin()
                })
                .collect();
            let (found, clarity) = detect(&x, 48_000.0, &mut difference).unwrap();
            let (_, cents) = note(found, f);
            assert!(cents.abs() < 3.0, "{f} Hz read as {found} ({cents} cents)");
            assert!(clarity > 0.8);
        }
        assert!(detect(&[0.0; WINDOW], 48_000.0, &mut difference).is_none());
        assert_eq!(note(440.0, 440.0), (69, 0.0));
        let (n, cents) = note(452.0, 440.0);
        assert_eq!(n, 69);
        assert!((cents - 46.6).abs() < 0.5, "{cents}");
    }

    #[test]
    fn reads_the_pitch_of_the_stream_and_mutes_when_asked() {
        let mut tuner = Tuner::default();
        tuner.prepare(48_000, 512);
        tuner.set_param(1, 1.0);
        let mut meters = [0.0; 2];
        for block in 0..20 {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..512)
                .map(|i| {
                    let t = (block * 512 + i) as f32 / 48_000.0;
                    let s = 0.5 * (t * 110.0 * std::f32::consts::TAU).sin();
                    (s, s)
                })
                .unzip();
            tuner.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|s| *s == 0.0), "muted");
        }
        tuner.meters(&mut meters);
        assert!(
            (meters[0] - 110.0).abs() < 0.5 && meters[1] > 0.8,
            "{meters:?}"
        );
        for _ in 0..20 {
            let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
            tuner.process(&mut l, &mut r);
        }
        tuner.meters(&mut meters);
        assert_eq!(meters[1], 0.0, "silence has no pitch");
    }

    #[test]
    fn a_new_note_shows_at_once_and_a_held_one_settles() {
        let mut tuner = Tuner::default();
        tuner.prepare(48_000, 1024);
        let tone = |f: f32, n: usize| -> Vec<f32> {
            (0..n)
                .map(|i| 0.4 * (i as f32 * f * std::f32::consts::TAU / 48_000.0).sin())
                .collect()
        };
        let mut run = |f: f32| {
            let mut l = tone(f, 24_000);
            let mut r = l.clone();
            tuner.process(&mut l, &mut r);
            tuner.frequency
        };
        let a = run(110.0);
        assert!((note(a, 110.0).1).abs() < 2.0, "{a}");
        // An octave up is another note: no glide from the A2.
        let b = run(220.0);
        assert!((note(b, 220.0).1).abs() < 2.0, "{b}");
    }

    #[test]
    fn small_blocks_share_the_search_and_read_the_same_pitch() {
        let mut tuner = Tuner::default();
        tuner.prepare(48_000, 64);
        let mut spread = 0;
        for block in 0..400 {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (0..64)
                .map(|i| {
                    let t = (block * 64 + i) as f32 / 48_000.0;
                    let s = 0.5 * (t * 82.41 * std::f32::consts::TAU).sin();
                    (s, s)
                })
                .unzip();
            tuner.process(&mut l, &mut r);
            spread += usize::from(tuner.search.is_some());
        }
        // A window every 32 blocks, searched over most of them.
        assert!(spread > 300, "{spread}");
        assert!(
            (note(tuner.frequency, 82.41).1).abs() < 2.0,
            "{}",
            tuner.frequency
        );
    }
}
