//! Tempo estimation: an onset envelope from frame energies, autocorrelated over 60 to 200 BPM
//! with a mild preference for moderate tempos.

const HOP: usize = 512;

/// Most likely tempo of `samples` in beats per minute, `None` without a clear pulse.
pub fn detect_bpm(samples: &[f32], rate: u32) -> Option<f64> {
    let frame_rate = f64::from(rate) / HOP as f64;
    let energies: Vec<f64> = samples
        .chunks(HOP)
        .map(|c| {
            let e = c.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / c.len() as f64;
            (e + 1e-10).ln()
        })
        .collect();
    if energies.len() < 64 {
        return None;
    }
    let mut onsets: Vec<f64> = energies
        .windows(2)
        .map(|w| (w[1] - w[0]).max(0.0))
        .collect();
    // Remove the slow trend so only rises remain.
    let span = 16;
    let averages: Vec<f64> = (0..onsets.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(span), (i + span).min(onsets.len()));
            onsets[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect();
    for (o, a) in onsets.iter_mut().zip(&averages) {
        *o = (*o - a).max(0.0);
    }
    let lag_of = |bpm: f64| 60.0 * frame_rate / bpm;
    let (min_lag, max_lag) = (lag_of(200.0).floor() as usize, lag_of(60.0).ceil() as usize);
    if max_lag + 2 >= onsets.len() {
        return None;
    }
    let correlation = |lag: usize| -> f64 {
        onsets
            .iter()
            .zip(&onsets[lag..])
            .map(|(a, b)| a * b)
            .sum::<f64>()
            / (onsets.len() - lag) as f64
    };
    let scores: Vec<f64> = (min_lag..=max_lag).map(correlation).collect();
    let zero = correlation(0);
    if zero <= 0.0 {
        return None;
    }
    let weighted = |i: usize| {
        let bpm = 60.0 * frame_rate / (min_lag + i) as f64;
        let octave = (bpm / 120.0).log2();
        scores[i] * (-0.5 * (octave / 0.9).powi(2)).exp()
    };
    let best = (0..scores.len()).max_by(|&a, &b| weighted(a).total_cmp(&weighted(b)))?;
    if scores[best] < zero * 0.05 {
        return None;
    }
    // Parabolic refinement around the best lag.
    let lag = if best > 0 && best + 1 < scores.len() {
        let (a, b, c) = (scores[best - 1], scores[best], scores[best + 1]);
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1e-12 {
            0.5 * (a - c) / denominator
        } else {
            0.0
        };
        (min_lag + best) as f64 + shift.clamp(-0.5, 0.5)
    } else {
        (min_lag + best) as f64
    };
    Some(60.0 * frame_rate / lag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clicks(bpm: f64, seconds: usize) -> Vec<f32> {
        let rate = 48_000;
        let mut out = vec![0.0f32; rate * seconds];
        let step = 60.0 / bpm * rate as f64;
        let mut t = 0.0;
        while (t as usize) < out.len() {
            let at = t as usize;
            for i in 0..800.min(out.len() - at) {
                let decay = (-(i as f32) / 150.0).exp();
                out[at + i] += decay * (i as f32 * 0.3).sin();
            }
            t += step;
        }
        out
    }

    #[test]
    fn finds_the_pulse_of_click_tracks() {
        for bpm in [90.0, 100.0, 128.0, 140.0] {
            let found = detect_bpm(&clicks(bpm, 12), 48_000).unwrap();
            assert!((found - bpm).abs() < 1.5, "{bpm}: {found}");
        }
        assert_eq!(detect_bpm(&vec![0.0; 48_000 * 4], 48_000), None);
    }
}
