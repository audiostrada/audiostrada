//! What a piece of audio sounds like, in figures an assistant can reason about: its loudness
//! (BS.1770) and peaks, its dynamics over time, its stereo image, how its energy spreads over
//! the frequency bands, its resonances and its rhythm.

use realfft::RealFftPlanner;

use crate::loudness::LevelMeter;

/// Analysis window of the spectrum.
const FFT: usize = 8192;
/// Most windows averaged: long files are sampled evenly.
const MAX_WINDOWS: usize = 1500;
/// Points of the loudness timeline.
const TIMELINE: usize = 24;

/// Named bands of a mix, in Hz.
pub const BANDS: [(&str, f32, f32); 7] = [
    ("sub", 20.0, 60.0),
    ("bass", 60.0, 250.0),
    ("low mids", 250.0, 500.0),
    ("mids", 500.0, 2_000.0),
    ("high mids", 2_000.0, 4_000.0),
    ("presence", 4_000.0, 6_000.0),
    ("air", 6_000.0, 20_000.0),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub seconds: f64,
    /// dBFS, dBTP, LUFS; `None` for silence.
    pub sample_peak: f32,
    pub true_peak: f32,
    pub integrated: Option<f32>,
    pub short_term_max: Option<f32>,
    /// Loudness range (EBU Tech 3342), LU.
    pub loudness_range: Option<f32>,
    pub rms: f32,
    /// Peak to RMS, dB.
    pub crest: f32,
    /// Samples at or above full scale.
    pub clipped: usize,
    /// -1 (out of phase) to 1 (mono).
    pub correlation: Option<f32>,
    /// Side level against mid level, dB: very low is near mono.
    pub side_to_mid: Option<f32>,
    /// Left against right level, dB.
    pub balance: f32,
    /// Share of the energy of each of [`BANDS`], dB (0 is all of it).
    pub bands: [f32; BANDS.len()],
    /// Slope of the third-octave levels from 63 Hz to 8 kHz, dB per octave.
    pub tilt: Option<f32>,
    /// Narrow peaks standing above their octave: (Hz, dB above).
    pub resonances: Vec<(f32, f32)>,
    /// Short-term loudness through the audio: (seconds, LUFS).
    pub timeline: Vec<(f64, f32)>,
    pub onsets_per_second: f32,
    pub bpm: Option<f64>,
    /// The averaged spectrum: (Hz, dB), third-octave from 25 Hz to 16 kHz.
    pub third_octaves: Vec<(f32, f32)>,
    /// The averaged spectrum smoothed over a sixth of an octave, every 24th of an octave from
    /// 20 Hz: (Hz, dB per octave against the whole).
    pub curve: Vec<(f32, f32)>,
}

fn db(power: f64) -> f32 {
    (10.0 * power.max(1e-20).log10()) as f32
}

pub fn report(left: &[f32], right: &[f32], rate: u32) -> Report {
    let n = left.len().min(right.len());
    let (left, right) = (&left[..n], &right[..n]);
    let seconds = n as f64 / f64::from(rate.max(1));

    // Loudness, peaks, timeline.
    let mut meter = LevelMeter::new(rate);
    let step = (rate / 10).max(1) as usize;
    let mut short = Vec::with_capacity(n / step + 1);
    let (mut peak, mut clipped) = (0.0f32, 0usize);
    let (mut sl, mut sr, mut slr, mut mid, mut side) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let (l, r) = (left[i], right[i]);
        meter.push(l, r);
        peak = peak.max(l.abs()).max(r.abs());
        clipped += usize::from(l.abs() >= 0.999) + usize::from(r.abs() >= 0.999);
        let (l, r) = (f64::from(l), f64::from(r));
        sl += l * l;
        sr += r * r;
        slr += l * r;
        mid += (l + r) * (l + r) / 4.0;
        side += (l - r) * (l - r) / 4.0;
        if (i + 1) % step == 0 && i + 1 >= 3 * rate as usize {
            short.push(((i + 1) as f64 / f64::from(rate), meter.short_term()));
        }
    }
    if short.is_empty() && n > 0 {
        short.push((seconds, meter.short_term()));
    }
    let silent = peak < 1e-6;
    let audible: Vec<f32> = short.iter().map(|s| s.1).filter(|v| *v > -70.0).collect();
    let integrated = (!silent)
        .then(|| meter.integrated())
        .filter(|v| v.is_finite() && *v > -70.0);
    let rms = db((sl + sr) / (2.0 * n.max(1) as f64));
    let correlation = (sl > 0.0 && sr > 0.0).then(|| (slr / (sl * sr).sqrt()) as f32);
    let timeline = (0..TIMELINE.min(short.len()))
        .map(|k| short[k * short.len() / TIMELINE.min(short.len())])
        .collect();

    let (bands, third_octaves, tilt, resonances, curve) = spectrum(left, right, rate);
    let mono: Vec<f32> = left.iter().zip(right).map(|(l, r)| (l + r) / 2.0).collect();
    let onsets = crate::analysis::onsets(&[left, right], rate, 0.5).len();
    Report {
        seconds,
        sample_peak: db(f64::from(peak * peak)),
        true_peak: meter.highest_true_peak(),
        integrated,
        short_term_max: audible.iter().copied().reduce(f32::max),
        loudness_range: loudness_range(&audible),
        rms,
        crest: db(f64::from(peak * peak)) - rms,
        clipped,
        correlation,
        side_to_mid: (mid > 0.0).then(|| db(side / mid)),
        balance: db(sl.max(1e-20) / sr.max(1e-20)),
        bands,
        tilt,
        resonances,
        timeline,
        onsets_per_second: if seconds > 0.0 {
            (onsets as f64 / seconds) as f32
        } else {
            0.0
        },
        bpm: crate::tempo::detect_bpm(&mono, rate),
        third_octaves,
        curve,
    }
}

/// LRA after EBU Tech 3342: short-term values gated 20 LU below their mean, 10th to 95th
/// percentile.
fn loudness_range(short_term: &[f32]) -> Option<f32> {
    if short_term.len() < 10 {
        return None;
    }
    let mean = 10.0
        * (short_term
            .iter()
            .map(|v| 10f64.powf(f64::from(*v) / 10.0))
            .sum::<f64>()
            / short_term.len() as f64)
            .log10();
    let mut kept: Vec<f32> = short_term
        .iter()
        .copied()
        .filter(|v| f64::from(*v) > mean - 20.0)
        .collect();
    if kept.len() < 10 {
        return None;
    }
    kept.sort_by(f32::total_cmp);
    let at = |q: f32| kept[((kept.len() - 1) as f32 * q).round() as usize];
    Some(at(0.95) - at(0.10))
}

/// The averaged power spectrum of the mid signal, in bins of `rate / FFT` Hz.
fn power_spectrum(left: &[f32], right: &[f32], rate: u32) -> Vec<f64> {
    let n = left.len();
    let mut sum = vec![0.0f64; FFT / 2 + 1];
    if n < FFT {
        return sum;
    }
    let windows = ((n - FFT) / (FFT / 2) + 1).min(MAX_WINDOWS);
    let hann: Vec<f32> = (0..FFT)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos())
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    for w in 0..windows {
        let at = if windows > 1 {
            w * (n - FFT) / (windows - 1)
        } else {
            0
        };
        for i in 0..FFT {
            input[i] = (left[at + i] + right[at + i]) / 2.0 * hann[i];
        }
        if fft.process(&mut input, &mut output).is_ok() {
            for (s, c) in sum.iter_mut().zip(&output) {
                *s += f64::from(c.norm_sqr());
            }
        }
    }
    let _ = rate;
    sum.iter_mut().for_each(|s| *s /= windows as f64);
    sum
}

type Spectrum = (
    [f32; BANDS.len()],
    Vec<(f32, f32)>,
    Option<f32>,
    Vec<(f32, f32)>,
    Vec<(f32, f32)>,
);

fn spectrum(left: &[f32], right: &[f32], rate: u32) -> Spectrum {
    let power = power_spectrum(left, right, rate);
    let bin = rate as f32 / FFT as f32;
    let band_power = |lo: f32, hi: f32| -> f64 {
        let (a, b) = (
            ((lo / bin).ceil() as usize).max(1),
            ((hi / bin).floor() as usize).min(power.len() - 1),
        );
        if a > b {
            // Narrower than a bin: its share of the nearest one.
            let nearest = (((lo + hi) / 2.0 / bin).round() as usize).clamp(1, power.len() - 1);
            power[nearest] * f64::from((hi - lo) / bin)
        } else {
            power[a..=b].iter().sum()
        }
    };
    let total = band_power(20.0, 20_000.0f32.min(rate as f32 / 2.0)).max(1e-30);
    let bands = BANDS.map(|(_, lo, hi)| db(band_power(lo, hi.min(rate as f32 / 2.0)) / total));
    // Third octaves, from 25 Hz to 16 kHz.
    let thirds: Vec<(f32, f32)> = (-16..=12)
        .map(|k| 1000.0 * 2f32.powf(k as f32 / 3.0))
        .filter(|fc| *fc * 2f32.powf(1.0 / 6.0) < rate as f32 / 2.0)
        .map(|fc| {
            (
                fc,
                db(band_power(fc / 2f32.powf(1.0 / 6.0), fc * 2f32.powf(1.0 / 6.0)) / total),
            )
        })
        .collect();
    let fit: Vec<(f32, f32)> = thirds
        .iter()
        .filter(|(f, _)| (60.0..=8_500.0).contains(f))
        .copied()
        .collect();
    let tilt = (fit.len() >= 4 && total > 1e-20).then(|| {
        let xs: Vec<f32> = fit.iter().map(|(f, _)| f.log2()).collect();
        let (mx, my) = (
            xs.iter().sum::<f32>() / xs.len() as f32,
            fit.iter().map(|p| p.1).sum::<f32>() / xs.len() as f32,
        );
        let num: f32 = xs
            .iter()
            .zip(&fit)
            .map(|(x, p)| (x - mx) * (p.1 - my))
            .sum();
        let den: f32 = xs.iter().map(|x| (x - mx) * (x - mx)).sum();
        num / den
    });
    // Resonances: twelfth-octave levels standing above their octave.
    let level = |fc: f32, width: f32| {
        db(band_power(fc / 2f32.powf(width / 2.0), fc * 2f32.powf(width / 2.0)) / f64::from(width))
    };
    let mut peaks: Vec<(f32, f32)> = Vec::new();
    let steps: Vec<f32> = (0..)
        .map(|k| 40.0 * 2f32.powf(k as f32 / 24.0))
        .take_while(|f| *f < 12_000.0f32.min(rate as f32 / 2.2))
        .collect();
    let narrow: Vec<f32> = steps.iter().map(|f| level(*f, 1.0 / 12.0)).collect();
    for i in 1..steps.len().saturating_sub(1) {
        let above = narrow[i] - level(steps[i], 1.0);
        if narrow[i] >= narrow[i - 1]
            && narrow[i] >= narrow[i + 1]
            && above > 6.0
            && narrow[i] > -60.0
        {
            peaks.push((steps[i], above));
        }
    }
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut kept: Vec<(f32, f32)> = Vec::new();
    for p in peaks {
        if kept.iter().all(|k| (p.0 / k.0).log2().abs() > 1.0 / 6.0) {
            kept.push(p);
        }
        if kept.len() == 5 {
            break;
        }
    }
    let curve = (0..)
        .map(|k| 20.0 * 2f32.powf(k as f32 / 24.0))
        .take_while(|f| *f < 20_000.0f32.min(rate as f32 / 2.1))
        .map(|f| (f, level(f, 1.0 / 6.0)))
        .collect();
    (bands, thirds, tilt, kept, curve)
}

/// A spectrogram: `columns` moments by `rows` frequencies spaced evenly in octaves from `low`
/// to `high` Hz (lowest row first), in dB against the loudest cell.
pub fn spectrogram(
    left: &[f32],
    right: &[f32],
    rate: u32,
    columns: usize,
    rows: usize,
    low: f32,
    high: f32,
) -> Vec<f32> {
    const SIZE: usize = 4096;
    let n = left.len().min(right.len());
    let mut out = vec![-120.0f32; columns * rows];
    if n < SIZE || columns == 0 || rows == 0 {
        return out;
    }
    let hann: Vec<f32> = (0..SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let (mut input, mut output) = (fft.make_input_vec(), fft.make_output_vec());
    let bin = rate as f32 / SIZE as f32;
    let span = (high / low).log2();
    let edges: Vec<usize> = (0..=rows)
        .map(|r| ((low * 2f32.powf(span * r as f32 / rows as f32)) / bin).round() as usize)
        .collect();
    for c in 0..columns {
        let at = (c * (n - SIZE) / columns.saturating_sub(1).max(1)).min(n - SIZE);
        for i in 0..SIZE {
            input[i] = (left[at + i] + right[at + i]) / 2.0 * hann[i];
        }
        if fft.process(&mut input, &mut output).is_err() {
            continue;
        }
        for r in 0..rows {
            let a = edges[r].max(1).min(output.len() - 1);
            let b = edges[r + 1].max(a + 1).min(output.len());
            let power = output[a..b]
                .iter()
                .map(|c| c.norm_sqr())
                .fold(0.0f32, f32::max);
            out[r * columns + c] = 10.0 * power.max(1e-20).log10();
        }
    }
    let top = out.iter().copied().fold(-120.0f32, f32::max);
    out.iter_mut().for_each(|v| *v -= top);
    out
}

/// The most likely key of some music: a chromagram of its pitch classes (55 Hz to 5 kHz)
/// matched against the Krumhansl-Kessler major and minor profiles. (name, how much better it
/// fits than the next key, 0 to 1).
pub fn key(left: &[f32], right: &[f32], rate: u32) -> Option<(String, f32)> {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    ];
    const MAJOR: [f32; 12] = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const MINOR: [f32; 12] = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let power = power_spectrum(left, right, rate);
    let bin = rate as f32 / FFT as f32;
    let mut chroma = [0.0f32; 12];
    for (k, p) in power.iter().enumerate().skip(1) {
        let f = k as f32 * bin;
        if !(55.0..=5_000.0).contains(&f) {
            continue;
        }
        let pitch = 69.0 + 12.0 * (f / 440.0).log2();
        let class = (pitch.round() as i32).rem_euclid(12) as usize;
        // Bins close to a semitone count more than those between two.
        let near = 1.0 - (pitch - pitch.round()).abs() * 2.0;
        chroma[class] += (*p as f32).sqrt() * near.max(0.0);
    }
    if chroma.iter().all(|c| *c <= 0.0) {
        return None;
    }
    let correlate = |profile: &[f32; 12], tonic: usize| {
        let (mc, mp) = (
            chroma.iter().sum::<f32>() / 12.0,
            profile.iter().sum::<f32>() / 12.0,
        );
        let (mut num, mut dc, mut dp) = (0.0, 0.0, 0.0);
        for i in 0..12 {
            let (c, p) = (chroma[(i + tonic) % 12] - mc, profile[i] - mp);
            num += c * p;
            dc += c * c;
            dp += p * p;
        }
        num / (dc * dp).sqrt().max(1e-12)
    };
    let mut scores: Vec<(f32, String)> = (0..12)
        .flat_map(|t| {
            [
                (correlate(&MAJOR, t), format!("{} major", NAMES[t])),
                (correlate(&MINOR, t), format!("{} minor", NAMES[t])),
            ]
        })
        .collect();
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    Some((
        scores[0].1.clone(),
        (scores[0].0 - scores[1].0).clamp(0.0, 1.0),
    ))
}

/// Lowest and highest sample of each of `columns` slices of the mid signal.
pub fn envelope(left: &[f32], right: &[f32], columns: usize) -> Vec<(f32, f32)> {
    let n = left.len().min(right.len());
    (0..columns)
        .map(|c| {
            let a = c * n / columns.max(1);
            let b = ((c + 1) * n / columns.max(1)).max(a + 1).min(n);
            (a..b).fold((0.0f32, 0.0f32), |(lo, hi), i| {
                let m = (left[i] + right[i]) / 2.0;
                (lo.min(m), hi.max(m))
            })
        })
        .collect()
}

/// Short-term loudness (LUFS) at the end of each of `columns` slices.
pub fn loudness_curve(left: &[f32], right: &[f32], rate: u32, columns: usize) -> Vec<f32> {
    let n = left.len().min(right.len());
    let mut meter = LevelMeter::new(rate);
    let mut out = Vec::with_capacity(columns);
    let mut next = 1;
    for i in 0..n {
        meter.push(left[i], right[i]);
        while next <= columns && i + 1 >= next * n / columns {
            out.push(meter.short_term());
            next += 1;
        }
    }
    out.resize(columns, -120.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, gain: f32, seconds: f32, rate: u32) -> Vec<f32> {
        (0..(seconds * rate as f32) as usize)
            .map(|i| gain * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn a_sine_reads_as_a_sine() {
        let rate = 48_000;
        let x = sine(1_000.0, 0.5, 10.0, rate);
        let r = report(&x, &x, rate);
        assert!((r.sample_peak + 6.02).abs() < 0.1, "{}", r.sample_peak);
        assert!((r.crest - 3.01).abs() < 0.2, "{}", r.crest);
        assert!((r.correlation.unwrap() - 1.0).abs() < 1e-3);
        assert!(r.side_to_mid.unwrap() < -60.0);
        assert!(r.balance.abs() < 0.01);
        assert_eq!(r.clipped, 0);
        // BS.1770: -0.691 + 10 log10(2 x 0.125), plus about 0.7 dB of K weighting at 1 kHz.
        let lufs = r.integrated.unwrap();
        assert!((lufs + 6.0).abs() < 0.5, "{lufs}");
        let mids = BANDS.iter().position(|b| b.0 == "mids").unwrap();
        assert!(r.bands[mids] > -0.5, "{:?}", r.bands);
        assert!(r.bands[0] < -40.0);
        assert!(r.loudness_range.unwrap() < 1.0);
    }

    #[test]
    fn a_c_major_chord_is_in_c_major() {
        let rate = 48_000;
        let n = 6 * rate as usize;
        let tone = |f: f32| {
            (0..n).map(move |i| (std::f32::consts::TAU * f * i as f32 / rate as f32).sin())
        };
        // C E G, then F A C and G B D: a I IV V progression.
        let chords = [
            [261.63, 329.63, 392.0],
            [349.23, 440.0, 523.25],
            [392.0, 493.88, 587.33],
        ];
        let mut x = vec![0.0f32; n];
        for (k, chord) in chords.iter().enumerate() {
            for f in chord {
                for (i, v) in tone(*f).enumerate().skip(k * n / 3).take(n / 3) {
                    x[i] += 0.1 * v;
                }
            }
        }
        // The tonic chord held longer, as music does.
        for f in chords[0] {
            for (i, v) in tone(f).enumerate() {
                x[i] += 0.05 * v;
            }
        }
        assert_eq!(key(&x, &x, rate).unwrap().0, "C major");
    }

    #[test]
    fn a_hum_over_noise_is_a_resonance_and_full_scale_is_clipping() {
        let rate = 48_000;
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
        };
        let hum = sine(300.0, 0.3, 8.0, rate);
        let l: Vec<f32> = hum.iter().map(|h| h + 0.05 * noise()).collect();
        let r: Vec<f32> = hum.iter().map(|h| h + 0.05 * noise()).collect();
        let rep = report(&l, &r, rate);
        let (f, above) = rep.resonances[0];
        assert!((f / 300.0).log2().abs() < 0.1, "{:?}", rep.resonances);
        assert!(above > 10.0);
        let clip = vec![1.0f32; 4800];
        assert_eq!(report(&clip, &clip, rate).clipped, 9600);
        let silence = vec![0.0f32; 48_000];
        let s = report(&silence, &silence, rate);
        assert_eq!(s.integrated, None);
        assert_eq!(s.correlation, None);
    }
}
