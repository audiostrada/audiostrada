//! Fits how the compressor's Clean or Bus character turns its settings into the gain cell's
//! (`audiostrada_dsp::cell::Character`), on measurements of a reference taken with
//! `audiostrada measure-dynamics`, and prints the constants to write in the code:
//!
//!   cargo run --release -p audiostrada-dsp --example fit_comp -- clean OUT.txt \
//!       FILE.json:ratio=4,attack=10,release=150,knee=6,auto=0 ...
//!
//! Each file comes with the settings it was measured at, as the character's own; its threshold
//! is fitted with the rest, since the reference's scale may not be AudioStrada's. The best
//! constants so far are written to OUT.txt after each round.

use audiostrada_dsp::cell::{BUS, CLEAN, Cell, Character, Grain, Times};
use audiostrada_dsp::db_to_gain;
use audiostrada_dsp::measure::RATE;
use audiostrada_dsp::measure_dynamics::{Dynamics, measure};

struct Target {
    print: Dynamics,
    ratio: f32,
    attack: f32,
    release: f32,
    knee: f32,
    auto: bool,
}

fn near(curve: &[(f32, f32)], t: f32) -> Option<f32> {
    curve
        .iter()
        .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
        .filter(|p| (p.0 - t).abs() < 6.0)
        .map(|p| p.1)
}

fn distance(model: &Dynamics, reference: &Dynamics) -> f64 {
    let mut e = 0.0f64;
    let mut add = |a: f32, b: f32, w: f32| e += f64::from(w * (a - b).powi(2));
    for (m, r) in model.static_1k.iter().zip(&reference.static_1k) {
        if r.level >= -50.0 {
            add(m.reduction_db, r.reduction_db, 1.0);
        }
    }
    for (m, r) in model.steps.iter().zip(&reference.steps) {
        add(m.reduction_end_db, r.reduction_end_db, 2.0);
        for t in [
            0.0, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0, 49.0, 100.0, 299.0,
        ] {
            if let (Some(a), Some(b)) = (near(&m.attack, t), near(&r.attack, t)) {
                add(a, b, 1.0);
            }
        }
        for s in [
            0.0, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0, 7.0,
        ] {
            if let (Some(a), Some(b)) = (near(&m.release, s * 1e3), near(&r.release, s * 1e3)) {
                add(a, b, 3.0);
            }
        }
    }
    e
}

fn run(c: &Character, t: &Target, threshold: f32) -> f64 {
    let tuning = c.tuning(t.attack, t.release, t.auto);
    let curve = c.curve(threshold, t.ratio, t.knee);
    let times = Times::new(&tuning, RATE as f32);
    let mut cell = Cell::default();
    let mut grain = [Grain::default(); 2];
    let mut process = |l: &mut [f32], r: &mut [f32]| {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let reduction = cell.push(&times, a.abs().max(b.abs()), &curve);
            let g = db_to_gain(-reduction);
            *a = grain[0].pass(&times, *a * g, reduction);
            *b = grain[1].pass(&times, *b * g, reduction);
        }
    };
    let step = t.print.steps.first().map(|s| s.level);
    let print = measure("model", Vec::new(), &mut process, 0, 8.0, false, step);
    distance(&print, &t.print)
}

/// The parameters fitted: the character's, then each target's threshold.
fn character(v: &[f32]) -> Character {
    let ms = |k: usize| 2f32.powf(v[k]);
    Character {
        attack_scale: ms(0),
        release_scale: ms(1),
        light_rise_ms: ms(2),
        light_fall_ms: ms(3),
        charge_ms: ms(4),
        memory_release_scale: ms(5),
        auto_charge_ms: ms(6),
        auto_memory_release_ms: ms(7),
        memory: v[8].clamp(0.0, 0.9),
        auto_memory: v[9].clamp(0.0, 0.95),
        knee_scale: v[10].clamp(0.0, 6.0),
        knee_add: v[11].clamp(0.0, 30.0),
        ratio_scale: v[12].clamp(0.1, 3.0),
        threshold_add: 0.0,
    }
}

fn values(c: &Character) -> Vec<f32> {
    vec![
        c.attack_scale.log2(),
        c.release_scale.log2(),
        c.light_rise_ms.log2(),
        c.light_fall_ms.log2(),
        c.charge_ms.log2(),
        c.memory_release_scale.log2(),
        c.auto_charge_ms.log2(),
        c.auto_memory_release_ms.log2(),
        c.memory,
        c.auto_memory,
        c.knee_scale,
        c.knee_add,
        c.ratio_scale,
    ]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let which = args.next().expect("clean or bus");
    let out = args.next().expect("OUT.txt");
    let base = if which == "bus" { BUS } else { CLEAN };
    let targets: Vec<Target> = args
        .map(|a| {
            let (path, settings) = a.split_once(".json:").expect("FILE.json:settings");
            let text = std::fs::read_to_string(format!("{path}.json")).expect("file");
            let print: Dynamics = serde_json::from_str(&text).expect("JSON");
            let get = |key: &str| {
                settings
                    .split(',')
                    .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
                    .map_or(0.0, |v| v.parse().expect("a number"))
            };
            Target {
                print,
                ratio: get("ratio"),
                attack: get("attack"),
                release: get("release"),
                knee: get("knee"),
                auto: get("auto") >= 0.5,
            }
        })
        .collect();
    let onset = |p: &Dynamics| {
        p.static_1k
            .iter()
            .find(|q| q.reduction_db >= 1.0)
            .map_or(0.0, |q| q.level)
    };
    let mut start = values(&base);
    start.extend(targets.iter().map(|t| onset(&t.print) + 2.0));
    let mut steps = vec![0.5; 8];
    steps.extend([0.05, 0.05, 0.2, 1.0, 0.1]);
    steps.extend(vec![1.0; targets.len()]);
    let cost = |v: &[f32]| {
        let c = character(v);
        std::thread::scope(|scope| {
            let jobs: Vec<_> = targets
                .iter()
                .zip(&v[13..])
                .map(|(t, &th)| scope.spawn(move || run(&c, t, th)))
                .collect();
            jobs.into_iter()
                .map(|j| j.join().unwrap_or(f64::MAX))
                .sum::<f64>()
        })
    };
    let (mut best, mut steps) = (start, steps);
    let mut best_cost = cost(&best);
    eprintln!("start {best_cost:.1}");
    for round in 0..25 {
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
        let c = character(&best);
        let _ = std::fs::write(&out, format!("{c:#?}\nthresholds {:?}\n", &best[13..]));
        if !moved {
            steps.iter_mut().for_each(|s| *s *= 0.5);
            if steps.iter().all(|s| s.abs() < 0.01) {
                break;
            }
        }
    }
    print!("{}", std::fs::read_to_string(&out).unwrap_or_default());
}
