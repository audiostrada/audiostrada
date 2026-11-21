//! Fits the FET gain cell (`audiostrada_dsp::cell::FET`, `FET_ALL`, `FET_AMP`) on measurements of
//! a reference taken with `audiostrada measure-dynamics`, in stages, and prints the constants to
//! write in the code. The files are given by role:
//!
//!   cargo run --release -p audiostrada-dsp --example fit_fet -- OUT.txt \
//!       base=IN24.json base=IN36.json base=IN18.json attack1=A1.json attack7=A7.json \
//!       release1=R1.json release7=R7.json ratio8=R8.json ratio12=R12.json ratio20=R20.json \
//!       all=RALL.json all=RALL12.json amp=OUT18.json amp=OUT6.json ...
//!
//! Every setting but the amplifier's is at the output mark where the output adds nothing, the
//! gain into the cell being the reference's small-signal gain; the amplifier's are at the
//! quietest input mark. The best constants so far are written to OUT.txt after each round.

use audiostrada_dsp::cell::{Amp, Cell, Curve, FET, FET_ALL, FET_AMP, Grain, Times, Tuning, shape};
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

fn error_of(
    tuning: &Tuning,
    shape: Shape,
    amp: Amp,
    targets: &[&Dynamics],
    pre_post: impl Fn(&Dynamics) -> (f32, f32) + Sync,
) -> f64 {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = targets
            .iter()
            .map(|&t| {
                let pre_post = &pre_post;
                scope.spawn(move || {
                    let (pre, post) = pre_post(t);
                    let mut p = model(tuning, curve(shape, tuning.knee_db), pre, post, amp);
                    let step = t.steps.first().map(|s| s.level);
                    let print = measure("model", Vec::new(), &mut p, 0, 8.0, false, step);
                    distance(&print, t)
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap_or(f64::MAX)).sum()
    })
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
    let start = vec![FET_AMP.even * 100.0, FET_AMP.odd * 100.0, FET_AMP.ceiling.log2(), FET_AMP.sharpness];
    to_amp(&descend("amp", start, vec![0.5, 0.5, 0.25, 0.5], 40, cost, |_| {}))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("OUT.txt");
    let mut by: std::collections::BTreeMap<String, Vec<Dynamics>> = Default::default();
    for arg in args {
        let (role, path) = arg.split_once('=').expect("ROLE=FILE");
        by.entry(role.to_owned()).or_default().push(load(path));
    }
    let get = |role: &str| -> Vec<&Dynamics> { by.get(role).map(|v| v.iter().collect()).unwrap_or_default() };
    let report = std::cell::RefCell::new(String::new());
    let save = |text: &str| {
        let _ = std::fs::write(&out, text);
    };
    // Output 24 adds nothing: the gain into the cell is the small-signal gain.
    let into_cell = |t: &Dynamics| (t.small_gain_db, 0.0);

    let amp = if by.contains_key("amp") { fit_amp(&by["amp"]) } else { FET_AMP };
    let _ = writeln!(report.borrow_mut(), "{amp:#?}");
    save(&report.borrow());

    // 1. The base: the shared tuning and the 4:1 curve.
    let base = get("base");
    let mut tuning = FET;
    let mut shapes = [
        Shape { threshold: -25.0, slope: 0.83, bend: 0.0 },
        Shape { threshold: -22.0, slope: 0.9, bend: 0.0 },
        Shape { threshold: -20.0, slope: 0.93, bend: 0.0 },
        Shape { threshold: -19.0, slope: 0.96, bend: 0.0 },
        Shape { threshold: -22.0, slope: 0.9, bend: 0.0 },
    ];
    if let Some(t) = base.first() {
        shapes[0].threshold = onset(t) + t.small_gain_db + 1.0;
    }
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
                peak_memory: false,
                memory: v[8].clamp(0.0, 0.9),
                knee_db: v[9].clamp(0.0, 20.0),
                grain_even: v[10].max(0.0) / 100.0,
                grain_odd: v[11].max(0.0) / 100.0,
                grain_onset_db: v[12].clamp(0.2, 20.0),
            },
            Shape { threshold: v[13], slope: v[14].clamp(0.3, 1.2), bend: v[15] / 100.0 },
        )
    };
    let t = &tuning;
    let start = vec![
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
        shapes[0].threshold,
        shapes[0].slope,
        0.0,
    ];
    let mut steps = vec![0.5; 8];
    steps.extend([0.05, 1.0, 0.2, 0.2, 1.0, 1.0, 0.03, 0.2]);
    let best = descend(
        "base",
        start,
        steps,
        25,
        |v| {
            let (t, s) = to_base(v);
            error_of(&t, s, amp, &base, into_cell)
        },
        |v| {
            let (t, s) = to_base(v);
            save(&format!("{}\nbase {t:#?}\n4:1 {s:?}\n", report.borrow()));
        },
    );
    (tuning, shapes[0]) = to_base(&best);
    let _ = writeln!(report.borrow_mut(), "FET {tuning:#?}\n4:1 {:?}", shapes[0]);
    save(&report.borrow());

    // 2 and 3. Attack and release at 1 and 7.
    for (role, release) in [("attack1", false), ("attack7", false), ("release1", true), ("release7", true)] {
        let targets = get(role);
        if targets.is_empty() {
            continue;
        }
        let at = |v: &[f32]| {
            let ms = 2f32.powf(v[0]);
            if release {
                let k = ms / tuning.release_ms;
                Tuning { release_ms: ms, release_charged_ms: tuning.release_charged_ms * k, ..tuning }
            } else {
                Tuning { attack_ms: ms, ..tuning }
            }
        };
        let start = vec![if release { tuning.release_ms } else { tuning.attack_ms }.log2()
            + match role {
                "attack1" => 1.5,
                "attack7" => -2.0,
                "release1" => 2.0,
                _ => -2.0,
            }];
        let best = descend(role, start, vec![0.5], 20, |v| error_of(&at(v), shapes[0], amp, &targets, into_cell), |_| {});
        let ms = 2f32.powf(best[0]);
        let _ = writeln!(report.borrow_mut(), "{role} {ms:.3} ms");
        save(&report.borrow());
    }

    // 4. The other ratios: their curves.
    for (k, role) in [(1, "ratio8"), (2, "ratio12"), (3, "ratio20")] {
        let targets = get(role);
        if targets.is_empty() {
            continue;
        }
        let t = targets[0];
        let start = vec![onset(t) + t.small_gain_db + 1.0, shapes[k].slope, 0.0];
        let to = |v: &[f32]| Shape { threshold: v[0], slope: v[1].clamp(0.3, 1.2), bend: v[2] / 100.0 };
        let best = descend(role, start, vec![1.0, 0.03, 0.2], 20, |v| error_of(&tuning, to(v), amp, &targets, into_cell), |_| {});
        shapes[k] = to(&best);
        let _ = writeln!(report.borrow_mut(), "{role} {:?}", shapes[k]);
        save(&report.borrow());
    }

    // 5. All buttons: their own times, memory and curve.
    let all = get("all");
    if !all.is_empty() {
        let to = |v: &[f32]| {
            let ms = |k: usize| 2f32.powf(v[k]);
            (
                Tuning {
                    release_ms: ms(0),
                    release_charged_ms: ms(1),
                    charge_ms: ms(2),
                    memory_release_ms: ms(3),
                    attack_ms: ms(4),
                    memory: v[5].clamp(0.0, 0.9),
                    grain_even: v[6].max(0.0) / 100.0,
                    grain_odd: v[7].max(0.0) / 100.0,
                    ..tuning
                },
                Shape { threshold: v[8], slope: v[9].clamp(0.3, 1.3), bend: v[10] / 100.0 },
            )
        };
        let t = all[0];
        let f = &FET_ALL;
        let start = vec![
            f.release_ms.log2(),
            f.release_charged_ms.log2(),
            f.charge_ms.log2(),
            f.memory_release_ms.log2(),
            tuning.attack_ms.log2(),
            f.memory,
            f.grain_even * 100.0,
            f.grain_odd * 100.0,
            onset(t) + t.small_gain_db + 1.0,
            0.9,
            0.0,
        ];
        let mut steps = vec![0.5; 5];
        steps.extend([0.05, 0.2, 0.2, 1.0, 0.03, 0.2]);
        let best = descend("all", start, steps, 20, |v| {
            let (t, s) = to(v);
            error_of(&t, s, amp, &all, into_cell)
        }, |_| {});
        let (t, s) = to(&best);
        let _ = writeln!(report.borrow_mut(), "FET_ALL {t:#?}\nall {s:?}");
        save(&report.borrow());
    }
    print!("{}", report.borrow());
}
