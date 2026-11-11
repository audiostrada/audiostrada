//! Measures what a processor does to sound, as a fingerprint to compare consoles, preamps and
//! the plugins taken as references: gain and harmonics by frequency and level, intermodulation,
//! memory after loud passages, noise, hum and crosstalk.
//!
//! Tones last whole periods of a 0.1 s window, so that their levels are read exactly whatever
//! the processor's delay: the frequencies are multiples of 10 Hz.

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

pub const RATE: u32 = 48_000;
pub const BLOCK: usize = 256;
/// Window of a reading, and what passes before it so that the processor settles.
const WINDOW: usize = 4_800;
const SETTLE: usize = 9_600;
/// Harmonics measured: the 2nd to the 5th.
pub const HARMONICS: usize = 4;

pub const FREQUENCIES: [f32; 9] = [
    30.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0,
];
/// Levels of the tones, dBFS; references put their working level (0 dBu) at -18 dBFS.
pub const LEVELS: [f32; 7] = [-36.0, -30.0, -24.0, -18.0, -12.0, -6.0, 0.0];
/// Lowest level reported, dB: what is under it is silence.
const FLOOR: f32 = -200.0;

/// The readings of one processor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub name: String,
    pub rate: u32,
    /// Delay of the processor, in samples, found with a click.
    pub latency: usize,
    pub tones: Vec<Tone>,
    pub imd: Vec<Intermodulation>,
    pub memory: Memory,
    pub noise: Noise,
    /// Level leaking into the right side of a tone on the left only, dB, by frequency.
    pub crosstalk: Vec<(f32, f32)>,
    /// Gain of the right side minus the left one, dB, at 1 kHz and the working level.
    pub balance_db: f32,
}

/// A steady tone through the processor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tone {
    pub freq: f32,
    /// Level going in, dBFS.
    pub level: f32,
    /// Gain of the fundamental, dB.
    pub gain_db: f32,
    /// The 2nd to 5th harmonics, dB below the fundamental; none above half the sample rate.
    pub harmonics: [Option<f32>; HARMONICS],
    /// Their sum, dB below the fundamental.
    pub thd_db: f32,
}

/// Products of two tones mixed, dB below the tone they modulate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Intermodulation {
    /// Peak level of the pair, dBFS.
    pub level: f32,
    /// 60 Hz and 7 kHz four to one (SMPTE): the sidebands around 7 kHz.
    pub smpte_db: f32,
    /// 19 and 20 kHz (CCIF): the difference at 1 kHz, and the products at 18 and 21 kHz.
    pub ccif_d2_db: f32,
    pub ccif_d3_db: f32,
}

/// A quiet tone right after a loud one, against the same tone long after: what transformers
/// and drifting bias remember.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub gain_after_db: f32,
    pub gain_settled_db: f32,
    pub h2_after_db: f32,
    pub h2_settled_db: f32,
    pub h3_after_db: f32,
    pub h3_settled_db: f32,
    /// Time until the gain is within 0.1 dB of the settled one, ms.
    pub recovery_ms: f32,
}

/// What comes out of silence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Noise {
    pub rms_db: f32,
    /// Level of each octave band, by its center frequency, dBFS.
    pub bands: Vec<(f32, f32)>,
    /// Loudest of 50 and 60 Hz and their harmonics up to the 4th, dBFS.
    pub hum_db: f32,
}

fn db(ratio: f64) -> f32 {
    ((20.0 * ratio.max(1e-12).log10()) as f32).max(FLOOR)
}

fn gain(level_db: f32) -> f32 {
    10f32.powf(level_db / 20.0)
}

/// Amplitude of the component with `cycles` periods over `x` (Goertzel).
pub fn amplitude(x: &[f32], cycles: f64) -> f64 {
    let n = x.len();
    if n == 0 {
        return 0.0;
    }
    let w = std::f64::consts::TAU * cycles / n as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = f64::from(v) + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    2.0 * power.max(0.0).sqrt() / n as f64
}

/// Cycles of `freq` in a reading window.
fn cycles(freq: f32) -> f64 {
    f64::from(freq) * WINDOW as f64 / f64::from(RATE)
}

fn sine(freq: f32, amplitude: f32, len: usize) -> impl Iterator<Item = f32> {
    // In double precision: a phase in f32 drifts and shows as distortion near -110 dB.
    let step = std::f64::consts::TAU * f64::from(freq) / f64::from(RATE);
    let amplitude = f64::from(amplitude);
    (0..len).map(move |i| (amplitude * (i as f64 * step).sin()) as f32)
}

/// Runs `left` and `right` through `process`, a block at a time.
fn run(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    mut left: Vec<f32>,
    mut right: Vec<f32>,
) -> (Vec<f32>, Vec<f32>) {
    for (l, r) in left.chunks_mut(BLOCK).zip(right.chunks_mut(BLOCK)) {
        process(l, r);
    }
    (left, right)
}

/// Where the output of a segment starting at `start` is, past the processor's delay.
fn window(out: &[f32], start: usize, latency: usize) -> &[f32] {
    let from = (start + SETTLE + latency).min(out.len());
    &out[from..(from + WINDOW).min(out.len())]
}

/// The delay of the processor: where a click comes out loudest, else `hint`.
fn find_latency(process: &mut dyn FnMut(&mut [f32], &mut [f32]), hint: usize) -> usize {
    let at = RATE as usize / 5;
    let len = at + RATE as usize;
    let mut click = vec![0.0f32; len];
    click[at] = 0.25;
    let (out, _) = run(process, click.clone(), click);
    let (peak, index) = out
        .iter()
        .enumerate()
        .skip(at)
        .fold((0.0f32, at), |(p, i), (j, &v)| {
            if v.abs() > p { (v.abs(), j) } else { (p, i) }
        });
    if peak > 1e-4 { index - at } else { hint }
}

/// Measures `process`, which takes blocks of at most [`BLOCK`] frames at [`RATE`]. `latency`
/// is what the processor says of its delay, used when a click finds none.
pub fn measure(
    name: &str,
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    latency: usize,
) -> Fingerprint {
    let latency = find_latency(process, latency);
    Fingerprint {
        name: name.to_owned(),
        rate: RATE,
        latency,
        tones: tones(process, latency),
        imd: intermodulation(process, latency),
        memory: memory(process, latency),
        noise: noise(process),
        crosstalk: crosstalk(process, latency),
        balance_db: balance(process, latency),
    }
}

/// Every frequency at every level, in one stream.
fn tones(process: &mut dyn FnMut(&mut [f32], &mut [f32]), latency: usize) -> Vec<Tone> {
    let plan: Vec<(f32, f32)> = LEVELS
        .iter()
        .flat_map(|&level| FREQUENCIES.iter().map(move |&freq| (freq, level)))
        .collect();
    tones_at(process, &plan, latency)
}

/// The tones of `plan` (frequency, level in dBFS) only, in one stream: what a fit reads, many
/// times over.
pub fn tones_at(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    plan: &[(f32, f32)],
    latency: usize,
) -> Vec<Tone> {
    let segment = SETTLE + WINDOW;
    let mut input = Vec::with_capacity(plan.len() * segment + latency + WINDOW);
    for &(freq, level) in plan {
        input.extend(sine(freq, gain(level), segment));
    }
    input.resize(input.len() + latency + WINDOW, 0.0);
    let (out, _) = run(process, input.clone(), input);
    plan.iter()
        .enumerate()
        .map(|(i, &(freq, level))| {
            let x = window(&out, i * segment, latency);
            let fundamental = amplitude(x, cycles(freq));
            let mut harmonics = [None; HARMONICS];
            let mut sum = 0.0;
            for (k, h) in harmonics.iter_mut().enumerate() {
                let order = k as f32 + 2.0;
                if freq * order < RATE as f32 * 0.475 {
                    let a = amplitude(x, cycles(freq * order));
                    sum += a * a;
                    *h = Some(db(a / fundamental.max(1e-12)));
                }
            }
            Tone {
                freq,
                level,
                gain_db: db(fundamental / f64::from(gain(level))),
                harmonics,
                thd_db: db(sum.sqrt() / fundamental.max(1e-12)),
            }
        })
        .collect()
}

fn intermodulation(
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    latency: usize,
) -> Vec<Intermodulation> {
    let levels = [-24.0, -18.0, -12.0, -6.0, 0.0];
    let segment = SETTLE + WINDOW;
    let mut input = Vec::new();
    for &level in &levels {
        // Four to one: the pair peaks at `level`.
        let (low, high) = (gain(level) * 0.8, gain(level) * 0.2);
        input.extend(
            sine(60.0, low, segment)
                .zip(sine(7_000.0, high, segment))
                .map(|(a, b)| a + b),
        );
        let half = gain(level) * 0.5;
        input.extend(
            sine(19_000.0, half, segment)
                .zip(sine(20_000.0, half, segment))
                .map(|(a, b)| a + b),
        );
    }
    input.resize(input.len() + latency + WINDOW, 0.0);
    let (out, _) = run(process, input.clone(), input);
    levels
        .iter()
        .enumerate()
        .map(|(i, &level)| {
            let smpte = window(&out, 2 * i * segment, latency);
            let carrier = amplitude(smpte, cycles(7_000.0)).max(1e-12);
            let sidebands: f64 = [6_880.0, 6_940.0, 7_060.0, 7_120.0]
                .iter()
                .map(|&f| amplitude(smpte, cycles(f)).powi(2))
                .sum();
            let ccif = window(&out, (2 * i + 1) * segment, latency);
            let tone = amplitude(ccif, cycles(19_000.0)).max(1e-12);
            let d2 = amplitude(ccif, cycles(1_000.0));
            let d3 = (amplitude(ccif, cycles(18_000.0)).powi(2)
                + amplitude(ccif, cycles(21_000.0)).powi(2))
            .sqrt();
            Intermodulation {
                level,
                smpte_db: db(sidebands.sqrt() / carrier),
                ccif_d2_db: db(d2 / tone),
                ccif_d3_db: db(d3 / tone),
            }
        })
        .collect()
}

/// A second of 100 Hz near full scale, then a quiet second of it.
fn memory(process: &mut dyn FnMut(&mut [f32], &mut [f32]), latency: usize) -> Memory {
    let second = RATE as usize;
    let quiet = gain(-24.0);
    let mut input: Vec<f32> = sine(100.0, gain(-3.0), second).collect();
    input.extend(sine(100.0, quiet, second));
    input.resize(input.len() + latency + WINDOW, 0.0);
    let (out, _) = run(process, input.clone(), input);
    let quiet_start = second + latency;
    let read = |from: usize, len: usize| {
        let x = &out[from..from + len];
        let periods = len as f64 * 100.0 / f64::from(RATE);
        let f = amplitude(x, periods).max(1e-12);
        (
            db(f / f64::from(quiet)),
            db(amplitude(x, 2.0 * periods) / f),
            db(amplitude(x, 3.0 * periods) / f),
        )
    };
    // The first 10 ms (one period) after the change, and the last 100 ms.
    let period = RATE as usize / 100;
    let after = read(quiet_start, period);
    let settled = read(quiet_start + second - WINDOW, WINDOW);
    let mut recovery = 0.0;
    for k in 0..(second - WINDOW) / period {
        let (g, _, _) = read(quiet_start + k * period, period);
        if (g - settled.0).abs() > 0.1 {
            recovery = (k + 1) as f32 * 10.0;
        }
    }
    Memory {
        gain_after_db: after.0,
        gain_settled_db: settled.0,
        h2_after_db: after.1,
        h2_settled_db: settled.1,
        h3_after_db: after.2,
        h3_settled_db: settled.2,
        recovery_ms: recovery,
    }
}

/// Half a second to settle, then a second of silence read through an FFT.
fn noise(process: &mut dyn FnMut(&mut [f32], &mut [f32])) -> Noise {
    let n = RATE as usize;
    let len = n / 2 + n;
    let (out, _) = run(process, vec![0.0; len], vec![0.0; len]);
    let x = &out[len - n..];
    let rms = (x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / n as f64).sqrt();
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(n);
    let mut input: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    let mut spectrum = fft.make_output_vec();
    let _ = fft.process(&mut input, &mut spectrum);
    // Bins of 1 Hz; the power of a bin, as a share of the mean square.
    let power = |k: usize| spectrum.get(k).map_or(0.0, |c| 2.0 * c.norm_sqr()) / (n * n) as f64;
    let bands = [
        31.5f32, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ]
    .iter()
    .map(|&center| {
        let (lo, hi) = (
            (center / std::f32::consts::SQRT_2) as usize,
            ((center * std::f32::consts::SQRT_2) as usize).min(n / 2),
        );
        let sum: f64 = (lo..hi).map(power).sum();
        (center, db(sum.sqrt()))
    })
    .collect();
    let hum = [50usize, 60]
        .iter()
        .flat_map(|&f| (1..=4).map(move |k| f * k))
        .map(|f| db(power(f).sqrt()))
        .fold(FLOOR, f32::max);
    Noise {
        rms_db: db(rms),
        bands,
        hum_db: hum,
    }
}

/// Tones on the left only: what leaks into the right.
fn crosstalk(process: &mut dyn FnMut(&mut [f32], &mut [f32]), latency: usize) -> Vec<(f32, f32)> {
    let freqs = [100.0, 1_000.0, 10_000.0];
    let segment = SETTLE + WINDOW;
    let level = gain(-18.0);
    let mut left = Vec::new();
    for &f in &freqs {
        left.extend(sine(f, level, segment));
    }
    left.resize(left.len() + latency + WINDOW, 0.0);
    let right = vec![0.0; left.len()];
    let (_, out) = run(process, left, right);
    freqs
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let x = window(&out, i * segment, latency);
            (f, db(amplitude(x, cycles(f)) / f64::from(level)))
        })
        .collect()
}

fn balance(process: &mut dyn FnMut(&mut [f32], &mut [f32]), latency: usize) -> f32 {
    let mut input: Vec<f32> = sine(1_000.0, gain(-18.0), SETTLE + WINDOW).collect();
    input.resize(input.len() + latency + WINDOW, 0.0);
    let (l, r) = run(process, input.clone(), input);
    let (l, r) = (window(&l, 0, latency), window(&r, 0, latency));
    let c = cycles(1_000.0);
    db(amplitude(r, c) / amplitude(l, c).max(1e-12))
}

impl Fingerprint {
    /// The tone at `freq` and `level`, when measured.
    pub fn tone(&self, freq: f32, level: f32) -> Option<&Tone> {
        self.tones
            .iter()
            .find(|t| (t.freq - freq).abs() < 0.5 && (t.level - level).abs() < 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wire_measures_flat_and_clean() {
        let print = measure("wire", &mut |_, _| {}, 0);
        assert_eq!(print.latency, 0);
        for t in &print.tones {
            assert!(t.gain_db.abs() < 0.01, "{t:?}");
            assert!(t.thd_db < -120.0, "{t:?}");
        }
        assert!(print.imd.iter().all(|i| i.smpte_db < -120.0));
        assert!(print.noise.rms_db <= FLOOR + 1.0);
        assert!(print.crosstalk.iter().all(|&(_, d)| d <= FLOOR + 1.0));
        assert!(print.balance_db.abs() < 0.01);
    }

    #[test]
    fn a_delay_is_found_and_read_past() {
        // Ten samples late, half the level.
        let mut tail = [vec![0.0f32; 10], vec![0.0f32; 10]];
        let print = measure(
            "late",
            &mut |l: &mut [f32], r: &mut [f32]| {
                for (side, buffer) in [l, r].into_iter().enumerate() {
                    let mut delayed = tail[side].clone();
                    delayed.extend_from_slice(buffer);
                    let n = buffer.len();
                    buffer.copy_from_slice(&delayed[..n]);
                    for v in buffer.iter_mut() {
                        *v *= 0.5;
                    }
                    tail[side] = delayed[n..].to_vec();
                }
            },
            0,
        );
        assert_eq!(print.latency, 10);
        let tone = print.tone(1_000.0, -18.0).unwrap();
        assert!((tone.gain_db + 6.02).abs() < 0.05, "{tone:?}");
    }

    #[test]
    fn a_square_law_shows_its_second_harmonic_and_intermodulation() {
        // y = x + 0.1 x²: the 2nd harmonic is 0.05 A relative to A, -26 dB at full scale,
        // falling 1 dB per dB of level.
        let print = measure(
            "square",
            &mut |l: &mut [f32], r: &mut [f32]| {
                for v in l.iter_mut().chain(r.iter_mut()) {
                    *v += 0.1 * *v * *v;
                }
            },
            0,
        );
        let loud = print.tone(1_000.0, 0.0).unwrap().harmonics[0].unwrap();
        let quiet = print.tone(1_000.0, -12.0).unwrap().harmonics[0].unwrap();
        assert!((loud + 26.0).abs() < 0.5, "{loud}");
        assert!((loud - quiet - 12.0).abs() < 0.5, "{loud} {quiet}");
        let h3 = print.tone(1_000.0, 0.0).unwrap().harmonics[1].unwrap();
        assert!(h3 < -100.0, "{h3}");
        assert!(print.tone(10_000.0, 0.0).unwrap().harmonics[2].is_none());
        let imd = print.imd.last().unwrap();
        assert!(imd.smpte_db > -40.0 && imd.ccif_d2_db > -40.0, "{imd:?}");
    }
}
