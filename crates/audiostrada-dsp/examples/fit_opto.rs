//! Fits the optical cell (`audiostrada_dsp::cell`) on measurements of a reference taken with
//! `audiostrada measure-dynamics`, and prints the constants to write in the code.
//!
//!   cargo run --release -p audiostrada-dsp --example fit_opto -- \
//!       --comp 50=PR50.json --comp 70=PR70.json --limit 50=PR50L.json ... \
//!       --scale comp 30=PR30.json ... --scale limit 30=PR30L.json ... \
//!       --tube G50.json --tube G70.json ...
//!
//! `--comp` and `--limit` are the settings the cell's times and curves are fitted on (a Peak
//! reduction mark each); `--scale` the marks whose threshold alone is then found on their static
//! curve; `--tube` the settings whose harmonics and loss of level the amplifier is fitted on.
//! The measured plugin's working gain (its small-signal gain) is read from each file.

use audiostrada_dsp::cell::{Amp, Cell, Curve, Grain, OPTO, OPTO_AMP, Times, Tuning, shape};
use audiostrada_dsp::measure::{RATE, amplitude};
use audiostrada_dsp::measure_dynamics::{Dynamics, measure};
use audiostrada_dsp::{db_to_gain, gain_to_db};

/// Output level above which the reference's own output stage takes part (dBFS): those
/// readings are left to the amplifier's fit.
const CLEAN_OUTPUT: f32 = -1.0;

struct Target {
    mark: f32,
    limit: bool,
    print: Dynamics,
}

fn load(path: &str) -> Dynamics {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The model of a setting: the cell linked on both sides, its photoresistor, the reference's
/// working gain and the amplifier.
fn model(
    tuning: &Tuning,
    curve: Curve,
    makeup_db: f32,
    tube: Amp,
) -> impl FnMut(&mut [f32], &mut [f32]) {
    let times = Times::new(tuning, RATE as f32);
    let mut cell = Cell::default();
    let mut grain = [Grain::default(); 2];
    let makeup = db_to_gain(makeup_db);
    move |l: &mut [f32], r: &mut [f32]| {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let reduction = cell.push(&times, a.abs().max(b.abs()), &curve);
            let g = db_to_gain(-reduction);
            *a = shape(
                grain[0].pass(&times, *a * g, reduction) * makeup,
                &tube,
                1.0,
            );
            *b = shape(
                grain[1].pass(&times, *b * g, reduction) * makeup,
                &tube,
                1.0,
            );
        }
    }
}

/// The parameters fitted, as one vector: times in log2 of ms, the rest as they are.
#[derive(Clone, Debug)]
struct Fit {
    tuning: Tuning,
    /// Slope and bend, Comp then Limit.
    shapes: [(f32, f32); 2],
    /// Threshold of each target, in their order.
    thresholds: Vec<f32>,
    tube: Amp,
}

const TIMES: usize = 8;

impl Fit {
    fn get(&self) -> Vec<f32> {
        let t = &self.tuning;
        let mut v: Vec<f32> = [
            t.light_rise_ms,
            t.light_fall_ms,
            t.attack_ms,
            t.release_ms,
            t.release_charged_ms,
            t.charge_ms,
            t.memory_release_ms,
            t.grain_ms,
        ]
        .iter()
        .map(|ms| ms.log2())
        .collect();
        v.extend([t.memory, t.knee_db]);
        v.extend([t.grain_even * 100.0, t.grain_odd * 100.0, t.grain_onset_db]);
        for (slope, bend) in self.shapes {
            v.extend([slope, bend * 100.0]);
        }
        v.extend(&self.thresholds);
        v
    }

    fn set(&mut self, v: &[f32]) {
        let ms = |k: usize| 2f32.powf(v[k]);
        self.tuning = Tuning {
            light_rise_ms: ms(0),
            light_fall_ms: ms(1),
            attack_ms: ms(2),
            release_ms: ms(3),
            release_charged_ms: ms(4),
            charge_ms: ms(5),
            memory_release_ms: ms(6),
            grain_ms: ms(7),
            peak_memory: false,
            memory: v[TIMES].clamp(0.0, 0.9),
            knee_db: v[TIMES + 1].clamp(0.0, 20.0),
            grain_even: v[TIMES + 2].max(0.0) / 100.0,
            grain_odd: v[TIMES + 3].max(0.0) / 100.0,
            grain_onset_db: v[TIMES + 4].clamp(0.2, 20.0),
        };
        let at = TIMES + 5;
        for (k, s) in self.shapes.iter_mut().enumerate() {
            *s = (v[at + 2 * k].clamp(0.3, 1.2), v[at + 1 + 2 * k] / 100.0);
        }
        self.thresholds = v[at + 4..].to_vec();
    }

    fn curve(&self, threshold: f32, limit: bool) -> Curve {
        let (slope, bend) = self.shapes[usize::from(limit)];
        Curve {
            threshold,
            slope,
            bend,
            knee: self.tuning.knee_db,
        }
    }
}

fn near(curve: &[(f32, f32)], t: f32) -> Option<f32> {
    curve
        .iter()
        .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
        .filter(|p| (p.0 - t).abs() < 6.0)
        .map(|p| p.1)
}

/// How far a model's readings are from the reference's: squared dB, weighted.
fn distance(model: &Dynamics, reference: &Dynamics, static_only: bool) -> f64 {
    let mut e = 0.0f64;
    let mut add = |a: f32, b: f32, w: f32| e += f64::from(w * (a - b).powi(2));
    let floor = |h: f32| h.max(-90.0);
    for (statics, h) in [
        (&model.static_1k, &reference.static_1k),
        (&model.static_100, &reference.static_100),
    ] {
        for (m, r) in statics.iter().zip(h.iter()) {
            let out = r.level + reference.small_gain_db - r.reduction_db;
            if r.level >= -44.0 && out <= CLEAN_OUTPUT {
                add(m.reduction_db, r.reduction_db, 1.0);
                if !static_only && r.level >= -36.0 {
                    add(floor(m.h2_db), floor(r.h2_db), 0.02);
                    add(floor(m.h3_db), floor(r.h3_db), 0.02);
                }
            }
        }
    }
    if static_only {
        return e;
    }
    for (m, r) in model.steps.iter().zip(&reference.steps) {
        add(m.reduction_end_db, r.reduction_end_db, 2.0);
        for t in [
            1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0, 49.0, 100.0, 200.0, 299.0,
        ] {
            if let (Some(a), Some(b)) = (near(&m.attack, t), near(&r.attack, t)) {
                add(a, b, 1.0);
            }
        }
        for s in [
            0.0, 0.02, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0,
        ] {
            if let (Some(a), Some(b)) = (near(&m.release, s * 1e3), near(&r.release, s * 1e3)) {
                add(a, b, 3.0);
            }
        }
    }
    // What the detection hears: the reduction by frequency against 1 kHz, at -18 dBFS.
    if let (Some(m), Some(r)) = (model.response_loud.first(), reference.response_loud.first()) {
        let at = |p: &[(f32, f32)], f: f32| p.iter().find(|q| q.0 == f).map(|q| q.1);
        let (m1, r1) = (at(&m.points, 1_000.0), at(&r.points, 1_000.0));
        for f in [30.0, 50.0, 100.0, 200.0, 500.0, 2_000.0, 5_000.0, 10_000.0] {
            if let (Some(a), Some(b), Some(a1), Some(b1)) =
                (at(&m.points, f), at(&r.points, f), m1, r1)
            {
                add(a - a1, b - b1, 4.0);
            }
        }
    }
    e
}

fn run_target(fit: &Fit, target: &Target, threshold: f32, static_only: bool) -> f64 {
    let curve = fit.curve(threshold, target.limit);
    let mut process = model(&fit.tuning, curve, target.print.small_gain_db, fit.tube);
    let step = target.print.steps.first().map(|s| s.level);
    let print = measure(
        "model",
        Vec::new(),
        &mut process,
        0,
        8.0,
        !static_only,
        step,
    );
    distance(&print, &target.print, static_only)
}

fn error(fit: &Fit, targets: &[Target]) -> f64 {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = targets
            .iter()
            .zip(&fit.thresholds)
            .map(|(target, &threshold)| {
                scope.spawn(move || run_target(fit, target, threshold, false))
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap_or(f64::MAX)).sum()
    })
}

/// Coordinate descent: each parameter moved by its step both ways, kept when better; the steps
/// halve when a whole round brings nothing.
fn descend(
    start: Vec<f32>,
    steps: Vec<f32>,
    min_step: f32,
    rounds: usize,
    mut cost: impl FnMut(&[f32]) -> f64,
) -> Vec<f32> {
    let (mut best, mut steps) = (start, steps);
    let mut best_cost = cost(&best);
    eprintln!("start {best_cost:.1}");
    for round in 0..rounds {
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
        eprintln!("round {round}: {best_cost:.1}");
        if !moved {
            steps.iter_mut().for_each(|s| *s *= 0.5);
            if steps.iter().all(|s| *s < min_step) {
                break;
            }
        }
    }
    best
}

/// The level of a target's 1 dB of reduction, where its threshold is first guessed.
fn onset(print: &Dynamics) -> f32 {
    print
        .static_1k
        .iter()
        .find(|p| p.reduction_db >= 1.0)
        .map_or(0.0, |p| p.level)
}

fn fit_tube(files: &[Dynamics]) -> Amp {
    // Each reading: the level the amplifier is driven at, and what it gave.
    let mut points = Vec::new();
    for print in files {
        for p in &print.static_1k {
            // The cell's own reduction is known where the output is clean; past it, the
            // readings of a setting that does not compress tell the amplifier alone.
            let driven = p.level + print.small_gain_db;
            let compresses = print.static_1k.iter().any(|q| {
                q.level + print.small_gain_db - q.reduction_db <= CLEAN_OUTPUT
                    && q.reduction_db > 0.5
            });
            if !compresses && p.level >= -40.0 {
                points.push((driven, p.reduction_db, p.h2_db, p.h3_db));
            } else if compresses && driven - p.reduction_db <= CLEAN_OUTPUT && p.level >= -40.0 {
                points.push((driven - p.reduction_db, 0.0, p.h2_db, p.h3_db));
            }
        }
    }
    let read = |tube: &Amp, level: f32| {
        let a = db_to_gain(level);
        let n = 4_800;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                shape(
                    a * (i as f32 * std::f32::consts::TAU / 48.0).sin(),
                    tube,
                    1.0,
                )
            })
            .collect();
        let f = amplitude(&x, 100.0).max(1e-12);
        (
            gain_to_db(f as f32 / a),
            gain_to_db((amplitude(&x, 200.0) / f) as f32),
            gain_to_db((amplitude(&x, 300.0) / f) as f32),
        )
    };
    let to_tube = |v: &[f32]| Amp {
        even: v[0].max(0.0),
        odd: v[1].max(0.0),
        ceiling: 2f32.powf(v[2]),
        sharpness: v[3].clamp(1.0, 8.0),
    };
    let cost = |v: &[f32]| {
        let tube = to_tube(v);
        points
            .iter()
            .map(|&(level, loss, h2, h3)| {
                let (gain, m2, m3) = read(&tube, level);
                let floor = |h: f32| h.max(-90.0);
                f64::from(
                    4.0 * (-gain - loss).powi(2)
                        + 0.05 * (floor(m2) - floor(h2)).powi(2)
                        + 0.05 * (floor(m3) - floor(h3)).powi(2),
                )
            })
            .sum()
    };
    let start = vec![
        OPTO_AMP.even,
        OPTO_AMP.odd,
        OPTO_AMP.ceiling.log2(),
        OPTO_AMP.sharpness,
    ];
    let best = descend(start, vec![0.01, 0.01, 0.25, 0.5], 1e-4, 60, cost);
    to_tube(&best)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut targets, mut scale, mut tubes) = (Vec::new(), Vec::new(), Vec::new());
    let pair = |text: String| -> (f32, String) {
        let (mark, path) = text.split_once('=').expect("MARK=FILE");
        (mark.parse().expect("a mark"), path.to_owned())
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--comp" | "--limit" => {
                let (mark, path) = pair(args.next().expect("MARK=FILE"));
                targets.push(Target {
                    mark,
                    limit: arg == "--limit",
                    print: load(&path),
                });
            }
            "--scale" => {
                let limit = args.next().as_deref() == Some("limit");
                let (mark, path) = pair(args.next().expect("MARK=FILE"));
                scale.push(Target {
                    mark,
                    limit,
                    print: load(&path),
                });
            }
            "--tube" => tubes.push(load(&args.next().expect("FILE"))),
            other => panic!("unknown argument {other}"),
        }
    }

    let tube = if tubes.is_empty() {
        OPTO_AMP
    } else {
        fit_tube(&tubes)
    };
    println!("{tube:#?}");

    let mut fit = Fit {
        tuning: OPTO,
        shapes: [(0.775, 0.0), (0.85, 0.003)],
        thresholds: targets.iter().map(|t| onset(&t.print) + 1.0).collect(),
        tube,
    };
    if !targets.is_empty() {
        let start = fit.get();
        let mut steps = vec![0.3; TIMES];
        steps.extend([0.03, 1.0, 0.3, 0.3, 1.0, 0.03, 0.2, 0.03, 0.2]);
        steps.extend(vec![1.0; targets.len()]);
        let best = descend(start, steps, 0.02, 30, |v| {
            let mut f = fit.clone();
            f.set(v);
            error(&f, &targets)
        });
        fit.set(&best);
    }
    println!("{:#?}", fit.tuning);
    println!(
        "comp (slope, bend) {:?}, limit {:?}",
        fit.shapes[0], fit.shapes[1]
    );
    for (t, th) in targets.iter().zip(&fit.thresholds) {
        let kind = if t.limit { "limit" } else { "comp" };
        println!("{kind} {} -> threshold {th:.2}", t.mark);
    }

    // The other marks: their threshold alone, on their static curve.
    for target in scale.iter_mut() {
        let start = vec![onset(&target.print) - 2.0];
        let best = descend(start, vec![2.0], 0.05, 30, |v| {
            run_target(&fit, target, v[0], true)
        });
        let kind = if target.limit { "limit" } else { "comp" };
        println!("{kind} {} -> threshold {:.2}", target.mark, best[0]);
    }
}
