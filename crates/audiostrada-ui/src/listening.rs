//! How agents hear the project: a track or the mix is rendered offline and described in figures
//! (loudness, peaks, dynamics, stereo, frequency balance, resonances, rhythm), alone or against
//! another one.

use audiostrada_core::routing::Channel;
use audiostrada_dsp::report::{BANDS, Report, report};
use serde_json::{Value, json};

use crate::document::Document;

/// Integrated loudness above which streaming services turn a master down.
const STREAMING_LUFS: f32 = -14.0;

fn round(v: f32, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (f64::from(v) * k).round() / k
}

/// A rendered stretch of `target` (a track, or the mix for the master).
pub fn render(
    doc: &mut Document,
    target: Channel,
    start: Option<u64>,
    end: Option<u64>,
) -> Result<(Report, String), String> {
    let project = doc.project();
    let (track, name) = match target {
        Channel::Master => (None, "master".to_owned()),
        Channel::Track(t) => (
            Some(t),
            project
                .track(t)
                .map(|x| x.name.clone())
                .ok_or("no such track")?,
        ),
    };
    let start = start.unwrap_or(0);
    let end = end.unwrap_or_else(|| project.length()).max(start + 1);
    let (left, right, rate) = doc.render_to_measure(track, start, end)?;
    if left.is_empty() {
        return Err("nothing was rendered".into());
    }
    Ok((report(&left, &right, rate), name))
}

/// Observations an engineer would make first.
fn hints(r: &Report, master: bool) -> Vec<String> {
    let mut out = Vec::new();
    if r.integrated.is_none() {
        out.push("silent: nothing plays in this range".into());
        return out;
    }
    if r.clipped > 0 {
        out.push(format!(
            "{} samples at full scale: the signal clips",
            r.clipped
        ));
    }
    if r.true_peak > -1.0 {
        out.push(format!(
            "true peak {:.1} dBTP, above -1 dBTP: may distort once encoded (MP3, AAC)",
            r.true_peak
        ));
    }
    if let (true, Some(lufs)) = (master, r.integrated)
        && lufs > STREAMING_LUFS + 2.0
    {
        out.push(format!(
            "{lufs:.1} LUFS integrated: streaming services normalising around {STREAMING_LUFS} LUFS will turn it down by about {:.0} dB",
            lufs - STREAMING_LUFS
        ));
    }
    if r.crest < 8.0 {
        out.push(format!(
            "crest factor {:.1} dB: heavily compressed or limited",
            r.crest
        ));
    }
    if let Some(c) = r.correlation
        && c < 0.0
    {
        out.push(format!(
            "correlation {c:.2}: out-of-phase content, it will thin out in mono"
        ));
    }
    if let Some(s) = r.side_to_mid {
        if s > -3.0 {
            out.push(format!(
                "side {s:.1} dB against mid: very wide, check mono compatibility"
            ));
        } else if s < -35.0 {
            out.push("practically mono".into());
        }
    }
    if r.balance.abs() > 1.5 {
        out.push(format!(
            "{:.1} dB louder on the {}",
            r.balance.abs(),
            if r.balance > 0.0 { "left" } else { "right" }
        ));
    }
    if r.bands[0] > -8.0 {
        out.push(format!(
            "{:.0}% of the energy below 60 Hz",
            100.0 * 10f32.powf(r.bands[0] / 10.0)
        ));
    }
    for (hz, above) in r.resonances.iter().take(2) {
        out.push(format!(
            "resonance near {hz:.0} Hz, {above:.0} dB above its octave"
        ));
    }
    out
}

/// Each band by name ("sub 20-60 Hz"), in dB.
fn bands(values: &[f32; BANDS.len()]) -> Value {
    let map: serde_json::Map<String, Value> = BANDS
        .iter()
        .zip(values)
        .map(|((name, lo, hi), v)| (format!("{name} {lo}-{hi} Hz"), json!(round(*v, 1))))
        .collect();
    Value::Object(map)
}

pub fn describe(r: &Report, name: &str, master: bool, start: f64) -> Value {
    let opt = |v: Option<f32>| v.map(|v| round(v, 1));
    json!({
        "target": name,
        "from": round(start as f32, 2),
        "seconds": round(r.seconds as f32, 2),
        "loudness": {
            "integrated_lufs": opt(r.integrated),
            "short_term_max_lufs": opt(r.short_term_max),
            "range_lu": opt(r.loudness_range),
            "true_peak_dbtp": round(r.true_peak, 1),
            "sample_peak_dbfs": round(r.sample_peak, 1),
            "rms_dbfs": round(r.rms, 1),
            "crest_db": round(r.crest, 1),
            "clipped_samples": r.clipped,
        },
        "stereo": {
            "correlation": r.correlation.map(|v| round(v, 2)),
            "side_to_mid_db": opt(r.side_to_mid),
            "left_minus_right_db": round(r.balance, 1),
        },
        "spectrum": {
            "bands_db_of_total": bands(&r.bands),
            "tilt_db_per_octave": opt(r.tilt),
            "resonances": r.resonances.iter().map(|(hz, above)| json!({"hz": hz.round(), "db_above_octave": round(*above, 1)})).collect::<Vec<_>>(),
        },
        "dynamics": {
            "short_term_lufs": r.timeline.iter().map(|(t, v)| format!("{:.1}s {v:.1}", start + t)).collect::<Vec<_>>().join(", "),
        },
        "rhythm": {
            "onsets_per_second": round(r.onsets_per_second, 2),
            "bpm": r.bpm.map(|b| round(b as f32, 1)),
        },
        "notes": hints(r, master),
    })
}

/// Where two signals fight for the same frequencies: third octaves where both are within
/// 6 dB of each other and near their own loudest.
pub fn compare(a: &Report, b: &Report, names: [&str; 2]) -> Value {
    let level = |r: &Report, k: usize| r.rms + r.third_octaves.get(k).map_or(-120.0, |t| t.1);
    let loudest = |r: &Report| {
        (0..r.third_octaves.len())
            .map(|k| level(r, k))
            .fold(-200.0f32, f32::max)
    };
    let (top_a, top_b) = (loudest(a), loudest(b));
    let mut ranges: Vec<[f32; 2]> = Vec::new();
    for k in 0..a.third_octaves.len().min(b.third_octaves.len()) {
        let (la, lb) = (level(a, k), level(b, k));
        if (la - lb).abs() <= 6.0 && la > top_a - 12.0 && lb > top_b - 12.0 {
            let fc = a.third_octaves[k].0;
            match ranges.last_mut() {
                Some(last) if (fc / last[1]).log2() < 0.4 => last[1] = fc,
                _ => ranges.push([fc, fc]),
            }
        }
    }
    let diff = |x: Option<f32>, y: Option<f32>| x.zip(y).map(|(x, y)| round(y - x, 1));
    json!({
        "a": names[0],
        "b": names[1],
        "b_minus_a": {
            "integrated_lu": diff(a.integrated, b.integrated),
            "true_peak_db": round(b.true_peak - a.true_peak, 1),
            "crest_db": round(b.crest - a.crest, 1),
            "range_lu": diff(a.loudness_range, b.loudness_range),
            "bands_db_of_total": bands(&std::array::from_fn(|k| b.bands[k] - a.bands[k])),
        },
        "competing_hz": ranges.iter().map(|[lo, hi]| json!([lo.round(), hi.round()])).collect::<Vec<_>>(),
    })
}
