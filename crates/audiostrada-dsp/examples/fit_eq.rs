//! Fits a console style of the channel strip's EQ (`audiostrada_dsp::channel::EqStyle`) on the
//! curves of a reference console's EQ, measured with `audiostrada measure-dynamics` (`eq`):
//!
//!   cargo run --release -p audiostrada-dsp --example fit_eq -- PREFIX Q
//!
//! `PREFIX` is the path the files start with, each ending with its band and setting
//! (`low-shelf-plus12.json`, `low-mid-plus4.json`, `high-bell-plus12.json`...); `Q` is the mid
//! bands' Q the reference was measured at, as the strip's Q knob (its default when it has none).

use audiostrada_dsp::biquad::{Coefficients, FilterKind};
use audiostrada_dsp::channel::EqStyle;
use audiostrada_dsp::measure_dynamics::Dynamics;

const RATE: f64 = 48_000.0;

struct Curve {
    band: &'static str,
    gain: f32,
    freq: f32,
    points: Vec<(f32, f32)>,
}

fn response(style: &EqStyle, c: &Curve, q: f32) -> Vec<f32> {
    let coefficients = match c.band {
        "low-shelf" => style.shelf(FilterKind::LowShelf, c.freq, c.gain, RATE),
        "high-shelf" => style.shelf(FilterKind::HighShelf, c.freq, c.gain, RATE),
        "low-bell" | "high-bell" => style.outer_bell(c.freq, c.gain, RATE),
        _ => style.bell(c.freq, c.gain, q, RATE),
    };
    c.points
        .iter()
        .map(|&(f, _)| coefficients.magnitude_db(f64::from(f), RATE) as f32)
        .collect()
}

fn to_style(v: &[f32]) -> EqStyle {
    EqStyle {
        shelf_q: v[0].clamp(0.3, 3.0),
        shelf_spread: 2f32.powf(v[1]),
        shelf_gain: v[2].clamp(0.5, 1.6),
        broad: 2f32.powf(v[3]),
        proportional: v[4].clamp(0.0, 4.0),
        bell_gain: v[5].clamp(0.5, 1.6),
        bell_freq: 2f32.powf(v[6]),
        outer_broad: 2f32.powf(v[7]),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let prefix = args.next().expect("PREFIX");
    let q: f32 = args.next().expect("Q").parse().expect("a number");
    let roles: [(&str, &str, f32, f32); 11] = [
        ("low-shelf-plus12", "low-shelf", 12.0, 100.0),
        ("low-shelf-minus12", "low-shelf", -12.0, 100.0),
        ("low-bell-plus12", "low-bell", 12.0, 100.0),
        ("low-mid-plus12", "mid", 12.0, 500.0),
        ("low-mid-plus4", "mid", 4.0, 500.0),
        ("low-mid-minus12", "mid", -12.0, 500.0),
        ("high-mid-plus12", "mid", 12.0, 3_000.0),
        ("high-mid-plus4", "mid", 4.0, 3_000.0),
        ("high-shelf-plus12", "high-shelf", 12.0, 8_000.0),
        ("high-shelf-minus12", "high-shelf", -12.0, 8_000.0),
        ("high-bell-plus12", "high-bell", 12.0, 8_000.0),
    ];
    // The frequencies the reference showed, when they were not quite the ones aimed at.
    let freqs: Vec<f32> = std::env::args()
        .skip(3)
        .map(|f| f.parse().expect("a frequency"))
        .collect();
    let curves: Vec<Curve> = roles
        .iter()
        .filter_map(|&(file, band, gain, freq)| {
            let text = std::fs::read_to_string(format!("{prefix}{file}.json")).ok()?;
            let print: Dynamics = serde_json::from_str(&text).ok()?;
            let freq = match (band, freqs.as_slice()) {
                ("low-shelf" | "low-bell", [low, ..]) => *low,
                ("mid", [_, lm, hm, ..]) => {
                    if freq < 1_000.0 {
                        *lm
                    } else {
                        *hm
                    }
                }
                ("high-shelf" | "high-bell", [_, _, _, high]) => *high,
                _ => freq,
            };
            Some(Curve {
                band,
                gain,
                freq,
                points: print
                    .response_quiet
                    .into_iter()
                    .filter(|p| p.0 >= 20.0 && p.0 <= 18_000.0)
                    .collect(),
            })
        })
        .collect();
    let cost = |v: &[f32]| -> f64 {
        let style = to_style(v);
        curves
            .iter()
            .map(|c| {
                response(&style, c, q)
                    .iter()
                    .zip(&c.points)
                    .map(|(m, &(_, r))| f64::from((m - r).powi(2)))
                    .sum::<f64>()
            })
            .sum()
    };
    let mut best = vec![0.7, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut steps = [0.1, 0.5, 0.1, 0.5, 0.5, 0.1, 0.1, 0.5];
    let mut best_cost = cost(&best);
    for _ in 0..400 {
        let mut moved = false;
        for k in 0..best.len() {
            for dir in [1.0, -1.0] {
                let mut v = best.clone();
                v[k] += dir * steps[k];
                let c = cost(&v);
                if c < best_cost {
                    (best, best_cost, moved) = (v, c, true);
                    break;
                }
            }
        }
        if !moved {
            steps.iter_mut().for_each(|s| *s *= 0.5);
            if steps.iter().all(|s| *s < 1e-4) {
                break;
            }
        }
    }
    let n: usize = curves.iter().map(|c| c.points.len()).sum();
    println!(
        "{:#?}\nrms error {:.2} dB over {} curves",
        to_style(&best),
        (best_cost / n.max(1) as f64).sqrt(),
        curves.len()
    );
    let _ = Coefficients::IDENTITY;
}
