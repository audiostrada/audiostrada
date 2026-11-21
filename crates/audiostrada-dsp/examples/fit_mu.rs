//! Fits the variable-mu limiter's gain cell (Strada-Mu's constants) on measurements of a
//! reference taken with `audiostrada measure-dynamics`, in stages, and prints the constants to
//! write in the code. The files are given as `ROLE:MARK:PRE=FILE`:
//!
//!   cargo run --release -p audiostrada-dsp --example fit_mu -- OUT.txt \
//!       base:5:0=TC2.json base:1.9:0=TH2.json base:5:5=IN4.json ... tc1:5:0=TC1.json ...
//!
//! `MARK` is the threshold knob's mark, `PRE` the gain (dB) the input knob adds against its
//! reference position. `base` settings fit the shared constants, the curve and the thresholds
//! with time constant 2; `tcN` the times of each time constant. The best constants so far are
//! written to OUT.txt after each round.

use audiostrada_dsp::cell::{Amp, Cell, Curve, Grain, OPTO_AMP, Times, Tuning, shape};
use audiostrada_dsp::measure::{RATE, amplitude};
use audiostrada_dsp::measure_dynamics::{Dynamics, measure};
use audiostrada_dsp::{db_to_gain, gain_to_db};
use std::fmt::Write as _;

const CLEAN_OUTPUT: f32 = -1.0;

fn load(path: &str) -> Dynamics {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// One setting of the model: the cell's tuning and curve, the gain into the cell and after it.
fn model(
    tuning: &Tuning,
    curve: Curve,
    pre_db: f32,
    post_db: f32,
    amp: Amp,
) -> impl FnMut(&mut [f32], &mut [f32]) {
    let times = Times::new(tuning, RATE as f32);
    let mut cell = Cell::default();
    let mut grain = [Grain::default(); 2];
    let (pre, post) = (db_to_gain(pre_db), db_to_gain(post_db));
    move |l: &mut [f32], r: &mut [f32]| {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (x, y) = (*a * pre, *b * pre);
            let reduction = cell.push(&times, x.abs().max(y.abs()), &curve);
            let g = db_to_gain(-reduction);
            *a = shape(grain[0].pass(&times, x * g, reduction) * post, &amp, 1.0);
            *b = shape(grain[1].pass(&times, y * g, reduction) * post, &amp, 1.0);
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
fn distance(model: &Dynamics, reference: &Dynamics) -> f64 {
    let mut e = 0.0f64;
    let mut add = |a: f32, b: f32, w: f32| e += f64::from(w * (a - b).powi(2));
    let floor = |h: f32| h.max(-90.0);
    for (m, r) in model.static_1k.iter().zip(&reference.static_1k) {
        let out = r.level + reference.small_gain_db - r.reduction_db;
        if r.level >= -50.0 && out <= CLEAN_OUTPUT {
            add(m.reduction_db, r.reduction_db, 1.0);
            if r.level >= -40.0 {
                add(floor(m.h2_db), floor(r.h2_db), 0.02);
                add(floor(m.h3_db), floor(r.h3_db), 0.02);
            }
        }
    }
    for (m, r) in model.steps.iter().zip(&reference.steps) {
        add(m.reduction_end_db, r.reduction_end_db, 2.0);
        for t in [0.0, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 49.0, 100.0, 299.0] {
            if let (Some(a), Some(b)) = (near(&m.attack, t), near(&r.attack, t)) {
                add(a, b, 1.0);
            }
        }
        for s in [0.0, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0] {
            if let (Some(a), Some(b)) = (near(&m.release, s * 1e3), near(&r.release, s * 1e3)) {
                add(a, b, 3.0);
            }
        }
    }
    e
}

#[derive(Clone, Copy, Debug)]
struct Shape {
    threshold: f32,
    slope: f32,
    bend: f32,
}

fn curve(s: Shape, knee: f32) -> Curve {
    Curve {
        threshold: s.threshold,
        slope: s.slope,
        bend: s.bend,
        knee,
    }
}

fn descend(
    label: &str,
    start: Vec<f32>,
    steps: Vec<f32>,
    rounds: usize,
    mut cost: impl FnMut(&[f32]) -> f64,
    mut keep: impl FnMut(&[f32]),
) -> Vec<f32> {
    let (mut best, mut steps) = (start, steps);
    let mut best_cost = cost(&best);
    eprintln!("{label}: start {best_cost:.1}");
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
        eprintln!("{label} round {round}: {best_cost:.1}");
        keep(&best);
        if !moved {
            steps.iter_mut().for_each(|s| *s *= 0.5);
            if steps.iter().all(|s| s.abs() < 0.01) {
                break;
            }
        }
    }
    best
}

fn onset(print: &Dynamics) -> f32 {
    print
        .static_1k
        .iter()
        .find(|p| p.reduction_db >= 1.0)
        .map_or(0.0, |p| p.level)
}

fn fit_amp(files: &[Dynamics]) -> Amp {
    // Readings of a setting that does not compress them: the amplifier alone, driven at the
    // level the output knob gives.
    let mut points = Vec::new();
    for print in files {
        for p in &print.static_1k {
            if p.reduction_db.abs() < 0.3 && p.level >= -40.0 {
                points.push((p.level + print.small_gain_db, p.h2_db, p.h3_db));
            }
        }
    }
    let read = |amp: &Amp, level: f32| {
        let a = db_to_gain(level);
        let x: Vec<f32> = (0..4_800)
            .map(|i| shape(a * (i as f32 * std::f32::consts::TAU / 48.0).sin(), amp, 1.0))
            .collect();
        let f = amplitude(&x, 100.0).max(1e-12);
        (
            gain_to_db((amplitude(&x, 200.0) / f) as f32),
            gain_to_db((amplitude(&x, 300.0) / f) as f32),
        )
    };
    let to_amp = |v: &[f32]| Amp {
        even: v[0].max(0.0) / 100.0,
        odd: v[1].max(0.0) / 100.0,
        ceiling: 2f32.powf(v[2]),
        sharpness: v[3].clamp(1.0, 8.0),
    };
    let cost = |v: &[f32]| {
        let amp = to_amp(v);
        points
            .iter()
            .map(|&(level, h2, h3)| {
                let (m2, m3) = read(&amp, level);
                f64::from((m2.max(-90.0) - h2.max(-90.0)).powi(2) + (m3.max(-90.0) - h3.max(-90.0)).powi(2))
            })
            .sum()
    };
    let start = vec![OPTO_AMP.even * 100.0, OPTO_AMP.odd * 100.0, OPTO_AMP.ceiling.log2(), OPTO_AMP.sharpness];
    to_amp(&descend("amp", start, vec![0.5, 0.5, 0.25, 0.5], 40, cost, |_| {}))
}

struct Target {
    mark: f32,
    pre: f32,
    print: Dynamics,
}

fn thresholds_for(marks: &[f32], v: &[f32], mark: f32) -> f32 {
    marks
        .iter()
        .position(|&m| (m - mark).abs() < 0.05)
        .map_or(-20.0, |k| v[k])
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("OUT.txt");
    let mut by: std::collections::BTreeMap<String, Vec<Target>> = Default::default();
    for arg in args {
        let (head, path) = arg.split_once('=').expect("ROLE:MARK:PRE=FILE");
        let mut parts = head.split(':');
        let role = parts.next().expect("role").to_owned();
        let mark: f32 = parts.next().expect("mark").parse().expect("a mark");
        let pre: f32 = parts.next().expect("pre").parse().expect("a gain");
        by.entry(role).or_default().push(Target {
            mark,
            pre,
            print: load(path),
        });
    }
    let report = std::cell::RefCell::new(String::new());
    let save = |text: &str| {
        let _ = std::fs::write(&out, text);
    };
    // The amplifier, from what does not compress.
    let quiet: Vec<Dynamics> = by.values().flatten().map(|t| t.print.clone()).collect();
    let amp = fit_amp(&quiet);
    let _ = writeln!(report.borrow_mut(), "{amp:#?}");
    save(&report.borrow());
    let into_cell = |t: &Target| (t.pre, t.print.small_gain_db - t.pre);
    let run = |tuning: &Tuning, shape: Shape, t: &Target| -> f64 {
        let (pre, post) = into_cell(t);
        let mut p = model(tuning, curve(shape, tuning.knee_db), pre, post, amp);
        let step = t.print.steps.first().map(|s| s.level);
        let print = measure("model", Vec::new(), &mut p, 0, 8.0, false, step);
        distance(&print, &t.print)
    };
    let run_all = |tuning: &Tuning, shapes: &[(Shape, &Target)]| -> f64 {
        std::thread::scope(|scope| {
            let jobs: Vec<_> = shapes
                .iter()
                .map(|&(s, t)| scope.spawn(move || run(tuning, s, t)))
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap_or(f64::MAX)).sum()
        })
    };

    // 1. The base: shared constants, the curve, the thresholds, time constant 2.
    let base: Vec<&Target> = by.get("base").map(|v| v.iter().collect()).unwrap_or_default();
    let mut marks: Vec<f32> = base.iter().map(|t| t.mark).collect();
    marks.sort_by(f32::total_cmp);
    marks.dedup();
    let start_tuning = Tuning {
        light_rise_ms: 0.05,
        light_fall_ms: 2.0,
        knee_db: 10.0,
        attack_ms: 1.0,
        release_ms: 300.0,
        release_charged_ms: 400.0,
        memory: 0.2,
        charge_ms: 500.0,
        memory_release_ms: 1_500.0,
        grain_even: 0.004,
        grain_odd: 0.002,
        grain_onset_db: 3.0,
        grain_ms: 2.0,
        peak_memory: true,
    };
    let to_base = |v: &[f32]| {
        let ms = |k: usize| 2f32.powf(v[k]);
        (
            Tuning {
                light_rise_ms: ms(0),
                light_fall_ms: ms(1),
                attack_ms: ms(2),
                release_ms: ms(3),
                release_charged_ms: ms(4),
                charge_ms: ms(5),
                memory_release_ms: ms(6),
                grain_ms: ms(7),
                peak_memory: true,
                memory: v[8].clamp(0.0, 0.9),
                knee_db: v[9].clamp(0.0, 30.0),
                grain_even: v[10].max(0.0) / 100.0,
                grain_odd: v[11].max(0.0) / 100.0,
                grain_onset_db: v[12].clamp(0.2, 20.0),
            },
            (v[13].clamp(0.3, 1.2), v[14] / 100.0),
        )
    };
    let t = &start_tuning;
    let mut start = vec![
        t.light_rise_ms.log2(),
        t.light_fall_ms.log2(),
        t.attack_ms.log2(),
        t.release_ms.log2(),
        t.release_charged_ms.log2(),
        t.charge_ms.log2(),
        t.memory_release_ms.log2(),
        t.grain_ms.log2(),
        t.memory,
        t.knee_db,
        t.grain_even * 100.0,
        t.grain_odd * 100.0,
        t.grain_onset_db,
        0.85,
        0.0,
    ];
    for &m in &marks {
        let t = base
            .iter()
            .find(|t| (t.mark - m).abs() < 0.05 && t.pre == 0.0)
            .unwrap_or(&base[0]);
        start.push(onset(&t.print) + 2.0);
    }
    let mut steps = vec![0.5; 8];
    steps.extend([0.05, 2.0, 0.2, 0.2, 1.0, 0.05, 0.2]);
    steps.extend(vec![1.0; marks.len()]);
    let cost_base = |v: &[f32]| {
        let (tuning, (slope, bend)) = to_base(v);
        let ths = &v[15..];
        let shapes: Vec<(Shape, &Target)> = base
            .iter()
            .map(|&t| {
                (
                    Shape {
                        threshold: thresholds_for(&marks, ths, t.mark),
                        slope,
                        bend,
                    },
                    t,
                )
            })
            .collect();
        run_all(&tuning, &shapes)
    };
    let given: Option<Vec<f32>> = std::env::var("MU_BASE")
        .ok()
        .map(|v| v.split(',').map(|x| x.trim().parse().expect("a number")).collect());
    let best = match given {
        Some(v) => v,
        None => {
            let best = descend("base", start, steps, 25, cost_base, |v| {
                let (t, s) = to_base(v);
                save(&format!(
                    "{}\nbase {t:#?}\ncurve {s:?}\nthresholds {:?} at {:?}\n",
                    report.borrow(),
                    &v[15..],
                    marks
                ));
            });
            best
        }
    };
    let (tuning, (slope, bend)) = to_base(&best);
    let ths = best[15..].to_vec();
    let _ = writeln!(
        report.borrow_mut(),
        "MU {tuning:#?}\ncurve ({slope}, {bend})\nthresholds {ths:?} at {marks:?}"
    );
    save(&report.borrow());

    // 2. Each time constant's times.
    for role in ["tc1", "tc2", "tc3", "tc4", "tc5", "tc6"] {
        let targets: Vec<&Target> = by.get(role).map(|v| v.iter().collect()).unwrap_or_default();
        if targets.is_empty() {
            continue;
        }
        let to = |v: &[f32]| {
            let ms = |k: usize| 2f32.powf(v[k]);
            Tuning {
                attack_ms: ms(0),
                release_ms: ms(1),
                release_charged_ms: ms(2),
                charge_ms: ms(3),
                memory_release_ms: ms(4),
                memory: v[5].clamp(0.0, 0.95),
                ..tuning
            }
        };
        let shapes: Vec<(Shape, &Target)> = targets
            .iter()
            .map(|&t| {
                (
                    Shape {
                        threshold: thresholds_for(&marks, &ths, t.mark),
                        slope,
                        bend,
                    },
                    t,
                )
            })
            .collect();
        let start = vec![
            tuning.attack_ms.log2(),
            tuning.release_ms.log2(),
            tuning.release_charged_ms.log2(),
            tuning.charge_ms.log2(),
            tuning.memory_release_ms.log2(),
            tuning.memory,
        ];
        let best = descend(
            role,
            start,
            vec![0.7, 0.7, 0.7, 0.7, 0.7, 0.1],
            20,
            |v| run_all(&to(v), &shapes),
            |_| {},
        );
        let t = to(&best);
        let _ = writeln!(
            report.borrow_mut(),
            "{role}: attack {:.3} release {:.1} charged {:.1} charge {:.1} memory release {:.1} memory {:.3}",
            t.attack_ms, t.release_ms, t.release_charged_ms, t.charge_ms, t.memory_release_ms, t.memory
        );
        save(&report.borrow());
    }
    print!("{}", report.borrow());
}
