//! What a compressor does in time and by level, to compare AudioStrada's dynamics with the
//! plugins taken as references (`audiostrada measure-dynamics`) and to fit its models on them.
//! A 1 kHz tone is played through the processor, stepped up and down: the static curve with its
//! harmonics, the response by frequency at rest and while compressing (what the detection
//! hears), and the gain reduction after steps up (attack) and down (release) held for
//! different times, which shows how the release depends on what came before.
//!
//! The gain is read over whole periods of the tone: one period (1 ms) for the attack, ten for
//! the release, a hundred for the steady readings.

use crate::measure::{BLOCK, RATE, amplitude};
use serde::{Deserialize, Serialize};

/// The readings of one setting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dynamics {
    pub name: String,
    /// What the plugin shows for each parameter set.
    pub settings: Vec<(String, String)>,
    pub latency: usize,
    /// Gain of a 1 kHz tone at -50 dBFS, dB: the gain without reduction.
    pub small_gain_db: f32,
    /// A 1 kHz tone from -50 to 0 dBFS, each level reached from below.
    pub static_1k: Vec<Point>,
    /// The same at 100 Hz, by 6 dB.
    pub static_100: Vec<Point>,
    /// Gain by frequency at -40 dBFS.
    pub response_quiet: Vec<(f32, f32)>,
    /// Gain by frequency at louder levels, each tone after a silence: the reduction by
    /// frequency, which tells what the detection hears.
    pub response_loud: Vec<Response>,
    pub steps: Vec<Step>,
}

/// A steady tone.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Point {
    pub level: f32,
    pub gain_db: f32,
    /// The small-signal gain minus this one.
    pub reduction_db: f32,
    pub h2_db: f32,
    pub h3_db: f32,
    pub thd_db: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub level: f32,
    pub points: Vec<(f32, f32)>,
}

/// From -40 dBFS up to `level` for `hold_ms`, then back to -40 dBFS.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub level: f32,
    pub hold_ms: f32,
    /// Reduction at the end of the hold, dB.
    pub reduction_end_db: f32,
    /// Reduction by ms through the hold's first 300 ms (or all of it).
    pub attack: Vec<(f32, f32)>,
    /// Times to 50 % and 90 % of the reduction reached at the end of the hold, ms.
    pub attack_50_ms: Option<f32>,
    pub attack_90_ms: Option<f32>,
    /// Reduction by 10 ms over the 15 s after the step down, against the small-signal gain.
    pub release: Vec<(f32, f32)>,
    /// Times until the reduction is down to half of what it was, a quarter, a tenth, and
    /// under 1 dB, ms.
    pub release_50_ms: Option<f32>,
    pub release_25_ms: Option<f32>,
    pub release_10_ms: Option<f32>,
    pub release_1db_ms: Option<f32>,
}

const FREQ: f32 = 1_000.0;
const QUIET: f32 = -40.0;
const HOLDS_MS: [f32; 3] = [50.0, 500.0, 5_000.0];
const READ: usize = 4_800;

/// How long each part of the battery lasts: the whole one, or a shorter one for fits, which
/// measure a model many times over.
#[derive(Clone, Copy, Debug)]
struct Battery {
    /// Each level of a static curve, s.
    hold: f32,
    /// What is read after a step down, s.
    release: f32,
    /// Silence between the parts, for the processor to come back to rest, s.
    rest: f32,
    freqs: &'static [f32],
}

const FULL: Battery = Battery {
    hold: 1.5,
    release: 15.0,
    rest: 20.0,
    freqs: &RESPONSE_FREQS,
};

const SHORT: Battery = Battery {
    hold: 0.8,
    release: 8.0,
    rest: 4.0,
    freqs: &[30.0, 100.0, 500.0, 1_000.0, 5_000.0, 10_000.0],
};
const RESPONSE_FREQS: [f32; 11] = [
    30.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 15_000.0, 20_000.0,
];

fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn db(ratio: f64) -> f32 {
    ((20.0 * ratio.max(1e-12).log10()) as f32).max(-200.0)
}

fn samples(seconds: f32) -> usize {
    (seconds * RATE as f32).round() as usize
}

/// Appends `len` samples of a sine of `freq` at `level` dBFS (its peak).
fn tone(out: &mut Vec<f32>, freq: f32, level: f32, len: usize) {
    // In double precision, from the stream's start: a window of whole periods reads exactly.
    let step = f64::from(freq) / f64::from(RATE);
    let amplitude = f64::from(gain(level));
    let start = out.len();
    out.extend((0..len).map(|i| {
        let t = (start + i) as f64 * step;
        (amplitude * (std::f64::consts::TAU * t.fract()).sin()) as f32
    }));
}

/// Runs `input` (both sides) through `process`, a block at a time; the left output.
fn run(process: &mut dyn FnMut(&mut [f32], &mut [f32]), input: &[f32]) -> Vec<f32> {
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
        process(l, r);
    }
    left
}

/// The processor's delay: where a click comes out loudest, else `hint`.
fn find_latency(process: &mut dyn FnMut(&mut [f32], &mut [f32]), hint: usize) -> usize {
    let at = RATE as usize / 5;
    let mut click = vec![0.0f32; at + RATE as usize];
    click[at] = 0.01;
    let out = run(process, &click);
    let (peak, index) = out
        .iter()
        .enumerate()
        .skip(at)
        .fold((0.0f32, at), |(p, i), (j, &v)| {
            if v.abs() > p { (v.abs(), j) } else { (p, i) }
        });
    // Let what the click left die away.
    let _ = run(process, &vec![0.0; RATE as usize]);
    if peak > 1e-6 { index - at } else { hint }
}

/// Gain of `freq` over `out[from..from + len]` against `level`, dB.
fn gain_at(out: &[f32], from: usize, len: usize, freq: f32, level: f32) -> f32 {
    let x = &out[from.min(out.len())..(from + len).min(out.len())];
    let cycles = f64::from(freq) * x.len() as f64 / f64::from(RATE);
    db(amplitude(x, cycles) / f64::from(gain(level)))
}

/// Harmonics 2 and 3 and their sum with 4 and 5, dB below the fundamental.
fn harmonics(x: &[f32], freq: f32) -> (f32, f32, f32) {
    let cycles = f64::from(freq) * x.len() as f64 / f64::from(RATE);
    let f = amplitude(x, cycles).max(1e-12);
    let mut sum = 0.0;
    let mut h = [0.0f32; 4];
    for (k, slot) in h.iter_mut().enumerate() {
        let order = k as f64 + 2.0;
        if f64::from(freq) * order < f64::from(RATE) * 0.475 {
            let a = amplitude(x, cycles * order);
            sum += a * a;
            *slot = db(a / f);
        } else {
            *slot = -200.0;
        }
    }
    (h[0], h[1], db(sum.sqrt() / f))
}

/// Steady tones of `freq` at `levels`, each held `hold` seconds after the one below it.
fn sweep(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    battery: &Battery,
    latency: usize,
    freq: f32,
    levels: &[f32],
    small: f32,
) -> Vec<Point> {
    let hold = samples(battery.hold);
    let mut input = Vec::new();
    for &level in levels {
        tone(&mut input, freq, level, hold);
    }
    input.resize(input.len() + latency + BLOCK, 0.0);
    let out = run(process, &input);
    levels
        .iter()
        .enumerate()
        .map(|(i, &level)| {
            let from = i * hold + hold - READ + latency;
            let gain_db = gain_at(&out, from, READ, freq, level);
            let (h2_db, h3_db, thd_db) = harmonics(&out[from..from + READ], freq);
            Point {
                level,
                gain_db,
                reduction_db: small - gain_db,
                h2_db,
                h3_db,
                thd_db,
            }
        })
        .collect()
}

/// Gain by frequency at `level`, each tone held `hold` seconds after `gap` of silence.
fn response(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    battery: &Battery,
    latency: usize,
    level: f32,
    gap: f32,
    hold: f32,
) -> Vec<(f32, f32)> {
    let (gap, hold) = (samples(gap), samples(hold));
    let mut input = Vec::new();
    let mut starts = Vec::new();
    for &freq in battery.freqs {
        input.resize(input.len() + gap, 0.0);
        starts.push(input.len());
        tone(&mut input, freq, level, hold);
    }
    input.resize(input.len() + latency + BLOCK, 0.0);
    let out = run(process, &input);
    battery
        .freqs
        .iter()
        .zip(starts)
        .map(|(&freq, start)| {
            (
                freq,
                gain_at(&out, start + hold - READ + latency, READ, freq, level),
            )
        })
        .collect()
}

/// The first time `curve` crosses `value` going the way `rising` says, ms.
fn crossing(curve: &[(f32, f32)], value: f32, rising: bool) -> Option<f32> {
    curve
        .iter()
        .find(|&&(_, v)| if rising { v >= value } else { v <= value })
        .map(|&(t, _)| t)
}

/// The steps up to `level` and back, held each of [`HOLDS_MS`], in one stream.
fn steps(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    battery: &Battery,
    latency: usize,
    level: f32,
    small: f32,
) -> Vec<Step> {
    let period = (RATE as f32 / FREQ) as usize;
    let release = samples(battery.release);
    let mut input = Vec::new();
    tone(&mut input, FREQ, QUIET, samples(battery.rest.min(6.0)));
    let mut starts = Vec::new();
    for &hold in &HOLDS_MS {
        let len = samples(hold / 1_000.0);
        starts.push((input.len(), len));
        tone(&mut input, FREQ, level, len);
        tone(&mut input, FREQ, QUIET, release);
    }
    input.resize(input.len() + latency + BLOCK, 0.0);
    let out = run(process, &input);
    HOLDS_MS
        .iter()
        .zip(starts)
        .map(|(&hold_ms, (start, len))| {
            let reduction = |from: usize, n: usize, lvl: f32| {
                small - gain_at(&out, from + latency, n, FREQ, lvl)
            };
            let shown = len.min(samples(0.3));
            let attack: Vec<(f32, f32)> = (0..shown / period)
                .map(|k| {
                    let t = (k * period) as f32 * 1_000.0 / RATE as f32;
                    (t, reduction(start + k * period, period, level))
                })
                .collect();
            let end = reduction(start + len - period * 10, period * 10, level);
            let full: Vec<(f32, f32)> = (0..len / period)
                .map(|k| {
                    let t = (k * period) as f32 * 1_000.0 / RATE as f32;
                    (t, reduction(start + k * period, period, level))
                })
                .collect();
            let ten = period * 10;
            let after = start + len;
            let release_curve: Vec<(f32, f32)> = (0..release / ten)
                .map(|k| {
                    let t = (k * ten) as f32 * 1_000.0 / RATE as f32;
                    (t, reduction(after + k * ten, ten, QUIET))
                })
                .collect();
            let first = release_curve.first().map_or(end, |p| p.1).max(end);
            Step {
                level,
                hold_ms,
                reduction_end_db: end,
                attack,
                attack_50_ms: crossing(&full, 0.5 * end, true),
                attack_90_ms: crossing(&full, 0.9 * end, true),
                release_50_ms: crossing(&release_curve, 0.5 * first, false),
                release_25_ms: crossing(&release_curve, 0.25 * first, false),
                release_10_ms: crossing(&release_curve, 0.1 * first, false),
                release_1db_ms: crossing(&release_curve, 1.0, false),
                release: release_curve,
            }
        })
        .collect()
}

/// Measures one setting of `process`: the whole battery, or with `full` off a shorter one with
/// only what a fit compares (the static curves, the steps, the response at -18 dBFS). The steps go up
/// to `step_level`, or else to where the static curve reaches `target_db` of reduction.
pub fn measure(
    name: &str,
    settings: Vec<(String, String)>,
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    latency_hint: usize,
    target_db: f32,
    full: bool,
    step_level: Option<f32>,
) -> Dynamics {
    let battery = if full { FULL } else { SHORT };
    let latency = find_latency(process, latency_hint);
    // The small-signal gain, after a silence.
    let mut input = vec![0.0; samples(3.0)];
    tone(&mut input, FREQ, -50.0, samples(2.0));
    input.resize(input.len() + latency + BLOCK, 0.0);
    let out = run(process, &input);
    let small = gain_at(&out, samples(5.0) - READ + latency, READ, FREQ, -50.0);
    // The static curve first, to find the steps' level; the steps then start from rest.
    let levels: Vec<f32> = (0..=25).map(|k| -50.0 + 2.0 * k as f32).collect();
    let static_1k = sweep(process, &battery, latency, FREQ, &levels, small);
    let loud = step_level.unwrap_or_else(|| {
        static_1k
            .iter()
            .find(|p| p.reduction_db >= target_db)
            .map_or(0.0, |p| p.level)
    });
    let _ = run(process, &vec![0.0; samples(battery.rest)]);
    let steps = steps(process, &battery, latency, loud, small);
    let _ = run(process, &vec![0.0; samples(battery.rest)]);
    if !full {
        return Dynamics {
            name: name.to_owned(),
            settings,
            latency,
            small_gain_db: small,
            static_1k,
            static_100: Vec::new(),
            response_quiet: Vec::new(),
            response_loud: vec![Response {
                level: -18.0,
                points: response(process, &battery, latency, -18.0, 1.0, 1.0),
            }],
            steps,
        };
    }
    let low: Vec<f32> = (0..=8).map(|k| -48.0 + 6.0 * k as f32).collect();
    let small_100 = {
        let mut input = Vec::new();
        tone(&mut input, 100.0, -50.0, samples(2.0));
        input.resize(input.len() + latency + BLOCK, 0.0);
        let out = run(process, &input);
        gain_at(&out, samples(2.0) - READ + latency, READ, 100.0, -50.0)
    };
    let static_100 = sweep(process, &battery, latency, 100.0, &low, small_100);
    let _ = run(process, &vec![0.0; samples(battery.rest)]);
    let response_quiet = response(process, &battery, latency, QUIET, 0.2, 0.3);
    let response_loud = [-18.0, -6.0]
        .iter()
        .map(|&level| Response {
            level,
            points: response(process, &battery, latency, level, 2.0, 2.0),
        })
        .collect();
    Dynamics {
        name: name.to_owned(),
        settings,
        latency,
        small_gain_db: small,
        static_1k,
        static_100,
        response_quiet,
        response_loud,
        steps,
    }
}


/// Frequencies of [`curve`]: twelve per octave from 20 Hz to 20 kHz, on the 10 Hz grid the
/// readings need (so the lowest ones are fewer).
pub fn curve_freqs() -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let mut f = 20.0f32;
    while f <= 20_000.0 {
        let g = (f / 10.0).round() * 10.0;
        if out.last() != Some(&g) {
            out.push(g);
        }
        f *= 2f32.powf(1.0 / 12.0);
    }
    out
}

/// The gain by frequency of a steady tone at `level` dBFS: an equalizer's curve.
pub fn curve(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    latency_hint: usize,
    level: f32,
) -> Vec<(f32, f32)> {
    let latency = find_latency(process, latency_hint);
    let freqs = curve_freqs();
    let (gap, hold) = (samples(0.05), samples(0.2));
    let mut input = Vec::new();
    let mut starts = Vec::new();
    for &freq in &freqs {
        input.resize(input.len() + gap, 0.0);
        starts.push(input.len());
        tone(&mut input, freq, level, hold);
    }
    input.resize(input.len() + latency + BLOCK, 0.0);
    let out = run(process, &input);
    freqs
        .iter()
        .zip(starts)
        .map(|(&freq, start)| {
            (
                freq,
                gain_at(&out, start + hold - READ + latency, READ, freq, level),
            )
        })
        .collect()
}

/// What a tape echo does: its repeats, their color, how they saturate and waver.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Echo {
    pub name: String,
    pub settings: Vec<(String, String)>,
    /// The repeats of a click: when (ms) and how loud (dB against the click).
    pub repeats: Vec<(f32, f32)>,
    /// The first repeat's gain by frequency (dB), from short bursts.
    pub color: Vec<(f32, f32)>,
    /// The first repeat of a 1 kHz burst by level in: its gain (dB) and its third harmonic.
    pub drive: Vec<(f32, f32, f32)>,
    /// How far a steady tone's echo wavers in pitch (cents, the spread of its swing) and how
    /// fast (Hz).
    pub wobble_cents: f32,
    pub wobble_hz: f32,
}

/// Measures a tape echo set to play its echo alone (no dry signal).
pub fn echo(
    name: &str,
    settings: Vec<(String, String)>,
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
) -> Echo {
    let ms = |t: usize| t as f32 * 1_000.0 / RATE as f32;
    // A click: four milliseconds of 1 kHz in a Hann window, at -12 dBFS (tape takes the highs
    // away: a brighter click would read the echo's tone, not its level).
    let click_len = RATE as usize / 250;
    let click_peak = gain(-12.0);
    let mut input = vec![0.0f32; samples(0.1)];
    let at = input.len();
    input.extend((0..click_len).map(|i| {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / click_len as f32).cos();
        click_peak * w * (std::f32::consts::TAU * 1_000.0 * i as f32 / RATE as f32).sin()
    }));
    input.resize(at + samples(4.0), 0.0);
    let out = run(process, &input);
    // The envelope by millisecond, and its peaks well above the floor.
    let step = RATE as usize / 1_000;
    let env: Vec<f32> = out[at..]
        .chunks(step)
        .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
        .collect();
    let mut repeats = Vec::new();
    for k in 2..env.len().saturating_sub(2) {
        let v = env[k];
        if v > click_peak * 1e-3 && (k - 2..=k + 2).all(|j| env[j] <= v) && repeats.len() < 12 {
            if repeats.last().is_none_or(|&(t, _): &(f32, f32)| ms(k * step) - t > 30.0) {
                repeats.push((ms(k * step), db(f64::from(v / click_peak))));
            }
        }
    }
    let first = repeats.first().map_or(samples(0.3), |r| (r.0 * RATE as f32 / 1_000.0) as usize);
    // Bursts of 60 ms, their echo read where the first repeat comes, 40 ms in.
    let burst = samples(0.06);
    let read_burst = |process: &mut dyn FnMut(&mut [f32], &mut [f32]), freq: f32, level: f32| {
        let mut input = vec![0.0f32; samples(0.05)];
        let at = input.len();
        tone(&mut input, freq, level, burst);
        input.resize(at + first + burst + samples(2.0), 0.0);
        let out = run(process, &input);
        let from = at + first + samples(0.01);
        let len = samples(0.04);
        let x = &out[from.min(out.len())..(from + len).min(out.len())];
        let cycles = f64::from(freq) * x.len() as f64 / f64::from(RATE);
        let f = amplitude(x, cycles);
        let h3 = db(amplitude(x, 3.0 * cycles) / f.max(1e-12));
        (db(f / f64::from(gain(level))), h3)
    };
    let color = [100.0, 200.0, 500.0, 1_000.0, 2_000.0, 3_000.0, 5_000.0, 8_000.0]
        .iter()
        .map(|&f| (f, read_burst(process, f, -24.0).0))
        .collect();
    let drive = [-36.0, -24.0, -18.0, -12.0, -6.0, 0.0]
        .iter()
        .map(|&l| {
            let (g, h3) = read_burst(process, 1_000.0, l);
            (l, g, h3)
        })
        .collect();
    // A steady tone: the echo's phase against a steady 1 kHz, over 4 s after it settled.
    let mut input = Vec::new();
    tone(&mut input, 1_000.0, -24.0, samples(6.0));
    let out = run(process, &input);
    let from = samples(2.0);
    let hop = samples(0.005);
    let mut phases = Vec::new();
    let mut k = from;
    while k + 480 <= out.len() {
        let x = &out[k..k + 480];
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let t = std::f64::consts::TAU * 1_000.0 * (k + i) as f64 / f64::from(RATE);
            re += f64::from(v) * t.cos();
            im -= f64::from(v) * t.sin();
        }
        phases.push(im.atan2(re));
        k += hop;
    }
    // Unwrapped phase to frequency offset, in cents, by hop.
    let mut cents = Vec::new();
    for w in phases.windows(2) {
        let mut d = w[1] - w[0];
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        let hz = 1_000.0 + d / std::f64::consts::TAU / (hop as f64 / f64::from(RATE));
        cents.push((1_200.0 * (hz / 1_000.0).log2()) as f32);
    }
    let mean = cents.iter().sum::<f32>() / cents.len().max(1) as f32;
    let mut sorted = cents.clone();
    sorted.sort_by(f32::total_cmp);
    let q = |p: f32| sorted.get(((sorted.len().max(1) - 1) as f32 * p) as usize).copied().unwrap_or(0.0);
    let crossings = cents
        .windows(2)
        .filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0)
        .count();
    let seconds = cents.len() as f32 * hop as f32 / RATE as f32;
    Echo {
        name: name.to_owned(),
        settings,
        repeats,
        color,
        drive,
        wobble_cents: q(0.95) - q(0.05),
        wobble_hz: crossings as f32 / 2.0 / seconds.max(1e-3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A feed-forward compressor of known figures: 2 to 1 above -30 dBFS, peak detection
    /// with a 1 ms attack and a 100 ms release.
    fn compressor() -> impl FnMut(&mut [f32], &mut [f32]) {
        let rate = RATE as f32;
        let (attack, release) = ((-1.0 / (0.001 * rate)).exp(), (-1.0 / (0.1 * rate)).exp());
        let mut env = 0.0f32;
        move |l: &mut [f32], r: &mut [f32]| {
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                let x = a.abs().max(b.abs());
                let k = if x > env { attack } else { release };
                env = x + (env - x) * k;
                let level = 20.0 * env.max(1e-9).log10();
                let over = (level + 30.0).max(0.0);
                let g = gain(-over * 0.5);
                *a *= g;
                *b *= g;
            }
        }
    }

    #[test]
    fn a_known_compressor_reads_as_built() {
        let mut process = compressor();
        let d = measure("test", Vec::new(), &mut process, 0, 8.0, true, None);
        assert!(d.small_gain_db.abs() < 0.05, "{}", d.small_gain_db);
        let at = |level: f32| {
            d.static_1k
                .iter()
                .find(|p| (p.level - level).abs() < 0.5)
                .unwrap()
                .reduction_db
        };
        // The tones' levels are their peaks, which the detection follows: 12 dB over the
        // threshold at 2 to 1 is 6 dB of reduction.
        assert!((at(-18.0) - 6.0).abs() < 0.5, "{}", at(-18.0));
        assert!(at(-36.0).abs() < 0.1);
        let long = d.steps.last().unwrap();
        assert!(long.attack_90_ms.is_some_and(|t| t < 10.0), "{long:?}");
        // From 8 dB to 4 dB in about 0.1 s × ln 2 × 2 (the reduction in dB follows the
        // envelope in dB, which falls 8.7 dB per time constant).
        let half = long.release_50_ms.unwrap();
        assert!(half > 20.0 && half < 200.0, "{half}");
        assert!(d.response_quiet.iter().all(|&(_, g)| g.abs() < 0.1));
    }

}
