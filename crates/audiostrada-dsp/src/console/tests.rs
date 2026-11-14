use super::*;
use crate::gain_to_db;

fn sine(freq: f32, db: f32) -> Vec<f32> {
    let a = crate::db_to_gain(db);
    (0..48_000)
        .map(|i| {
            a * (f64::from(i) * f64::from(freq) * std::f64::consts::TAU / 48_000.0).sin() as f32
        })
        .collect()
}

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
}

fn color(model: usize, amount: f32, bus: bool) -> ConsoleColor {
    let mut c = ConsoleColor::new(48_000);
    c.set(model, amount, bus, false, None);
    c
}

/// Both sides of a tone through a console.
fn through(color: &mut ConsoleColor, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
        color.process(a, b);
    }
    (l, r)
}

const CONSOLES: [(usize, bool); 8] = [
    (1, false),
    (1, true),
    (2, false),
    (2, true),
    (3, false),
    (3, true),
    (4, false),
    (4, true),
];

fn name(model: usize, bus: bool) -> String {
    format!("{} {}", MODELS[model], if bus { "bus" } else { "channel" })
}

#[test]
fn clean_or_none_changes_nothing() {
    for (model, amount) in [(0, 100.0), (1, 0.0)] {
        let input = sine(440.0, -6.0);
        let (l, _) = through(&mut color(model, amount, false), &input, &input);
        assert_eq!(input, l);
    }
}

#[test]
fn the_working_level_keeps_its_loudness() {
    let input = sine(1_000.0, -18.0);
    for (model, bus) in CONSOLES {
        for amount in [1.0, 50.0, 100.0] {
            let (l, _) = through(&mut color(model, amount, bus), &input, &input);
            let change = gain_to_db(rms(&l[24_000..]) / rms(&input[24_000..]));
            assert!(
                change.abs() < 0.5,
                "{} {amount}: {change}",
                name(model, bus)
            );
        }
    }
}

/// Every console within a few dB of what its references measure.
#[test]
fn each_console_meets_its_targets() {
    let mut misses = Vec::new();
    for (model, bus) in CONSOLES {
        let targets = targets(model, bus);
        let read = read(&mut color(model, MEASURED, bus), targets);
        for (t, got) in targets.iter().zip(read) {
            let tolerance = if t.reading == Reading::Thd { 3.0 } else { 5.0 };
            if (got - t.db).abs() > tolerance {
                misses.push(format!(
                    "{} {:?} {} Hz {} dBFS: {got:.1} for {}",
                    name(model, bus),
                    t.reading,
                    t.freq,
                    t.level,
                    t.db
                ));
            }
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}

#[test]
fn the_response_stays_flat_at_the_working_level() {
    let plan: Vec<(f32, f32)> = [30.0, 100.0, 1_000.0, 5_000.0, 10_000.0]
        .into_iter()
        .map(|f| (f, -18.0))
        .collect();
    for (model, bus) in CONSOLES {
        let mut c = color(model, MEASURED, bus);
        let tones = crate::measure::tones_at(&mut |l, r| c.process(l, r), &plan, 0);
        for t in tones {
            // A transformer's core holds back a little of the lowest lows, as the references
            // (one of them loses 1.1 dB at 30 Hz).
            let tolerance = if t.freq < 50.0 { 1.0 } else { 0.6 };
            assert!(t.gain_db.abs() < tolerance, "{} {t:?}", name(model, bus));
        }
    }
}

#[test]
fn the_amount_pushes_the_circuit() {
    let target = [Target {
        freq: 1_000.0,
        level: -6.0,
        reading: Reading::Thd,
        db: 0.0,
    }];
    for (model, bus) in CONSOLES {
        let at = |amount| read(&mut color(model, amount, bus), &target)[0];
        let (soft, half, hard) = (at(1.0), at(MEASURED), at(100.0));
        assert!(
            hard > half + 6.0 && half > soft + 6.0,
            "{}: {soft} {half} {hard}",
            name(model, bus)
        );
    }
}

#[test]
fn a_new_amount_glides() {
    let mut c = color(1, 0.0, false);
    let input = sine(100.0, -6.0);
    let mut out = Vec::new();
    for (i, block) in input.chunks(256).enumerate() {
        c.set(1, if i < 40 { 5.0 } else { 50.0 }, false, false, None);
        let (mut l, mut r) = (block.to_vec(), block.to_vec());
        c.process(&mut l, &mut r);
        out.extend(l);
    }
    // A 100 Hz tone at -6 dBFS moves at most 0.007 a sample: a jump would show far above.
    let jump = out
        .windows(2)
        .map(|o| (o[1] - o[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(jump < 0.015, "{jump}");
}

#[test]
fn hiss_and_crosstalk_only_when_asked() {
    let silence = vec![0.0f32; 48_000];
    let mut quiet = color(1, MEASURED, true);
    let (l, r) = through(&mut quiet, &silence, &silence);
    assert!(l.iter().chain(&r).all(|&s| s == 0.0));
    for (model, bus) in CONSOLES {
        let mut c = ConsoleColor::new(48_000);
        c.set(model, MEASURED, bus, true, Some(7));
        let (l, _) = through(&mut c, &silence, &silence);
        let level = gain_to_db(rms(&l[4_800..]));
        assert!(
            (-112.0..-95.0).contains(&level),
            "{}: {level}",
            name(model, bus)
        );
    }
    // A bus leaks a little of one side into the other, more in the highs.
    let leak = |freq| {
        let mut c = ConsoleColor::new(48_000);
        c.set(2, MEASURED, true, true, None);
        let (_, r) = through(&mut c, &sine(freq, -6.0), &silence);
        gain_to_db(crate::measure::amplitude(&r[24_000..], f64::from(freq) / 2.0) as f32) + 6.0
    };
    let (mid, high) = (leak(1_000.0), leak(10_000.0));
    assert!(
        (-84.0..-76.0).contains(&mid) && high > mid + 12.0,
        "{mid} {high}"
    );
}

#[test]
fn each_channel_has_its_own_tolerances() {
    let input = sine(1_000.0, -6.0);
    let out = |seed| {
        let mut c = ConsoleColor::new(48_000);
        c.set(1, MEASURED, false, false, seed);
        through(&mut c, &input, &input)
    };
    let (a, b) = (out(Some(3)), out(Some(4)));
    assert_eq!(a, out(Some(3)));
    assert_ne!(a, b);
    // Its sides differ a little, never by much.
    let balance = gain_to_db(rms(&a.1[24_000..]) / rms(&a.0[24_000..]));
    assert!(balance != 0.0 && balance.abs() < 0.5, "{balance}");
    let perfect = out(None);
    assert_eq!(perfect.0, perfect.1);
}

#[test]
fn the_fingerprints_tell_the_consoles_apart() {
    let warm = harmonics(1, MEASURED, -12.0);
    let bright = harmonics(2, MEASURED, -12.0);
    let punch = harmonics(3, MEASURED, -12.0);
    let nashville = harmonics(4, MEASURED, -12.0);
    // An API's even harmonics, an SSL channel's odd ones.
    assert!(punch[0] > punch[1] + 6.0, "{punch:?}");
    assert!(bright[1] > bright[0] + 6.0, "{bright:?}");
    // The Neve the richest, the Harrison the cleanest.
    let total = |h: [f32; 5]| h.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>();
    assert!(total(warm) > total(punch) && total(warm) > total(bright));
    assert!(total(nashville) < total(bright) && total(nashville) < total(punch));
}

/// A setting of a recipe the fit moves: its name, where it is, and its step for a step of 1;
/// a step of 0 for a ratio (an asymmetry, a share), moved by factors.
type Knob = (&'static str, fn(&mut Recipe) -> &mut f32, f32);

/// `v` moved by `step` (signed) of `unit`.
fn nudge(v: &mut f32, step: f32, unit: f32) {
    if unit > 0.0 {
        *v += step * unit;
    } else {
        // A step of 4 doubles or halves.
        *v = (v.max(0.002) * 2f32.powf(step / 4.0)).min(0.95);
    }
}

fn knobs(model: usize, bus: bool) -> Vec<Knob> {
    fn input(r: &mut Recipe) -> &mut f32 {
        &mut r.input.as_mut().unwrap().saturation_dbu
    }
    fn output(r: &mut Recipe) -> &mut f32 {
        &mut r.output.as_mut().unwrap().saturation_dbu
    }
    fn input_hysteresis(r: &mut Recipe) -> &mut f32 {
        &mut r.input.as_mut().unwrap().hysteresis
    }
    fn input_coupling(r: &mut Recipe) -> &mut f32 {
        &mut r.input.as_mut().unwrap().coupling
    }
    fn output_hysteresis(r: &mut Recipe) -> &mut f32 {
        &mut r.output.as_mut().unwrap().hysteresis
    }
    fn headroom(r: &mut Recipe) -> &mut f32 {
        &mut r.class_a.as_mut().unwrap().headroom_dbu
    }
    fn asymmetry(r: &mut Recipe) -> &mut f32 {
        &mut r.class_a.as_mut().unwrap().asymmetry
    }
    fn h2(r: &mut Recipe) -> &mut f32 {
        &mut r.vca.as_mut().unwrap().0
    }
    fn h3(r: &mut Recipe) -> &mut f32 {
        &mut r.vca.as_mut().unwrap().1
    }
    let r = recipe(model, bus).unwrap();
    let mut out: Vec<Knob> = Vec::new();
    if r.input.is_some() {
        out.push(("input saturation dBu", input, 1.0));
        out.push(("input hysteresis", input_hysteresis, 0.0));
        out.push(("input coupling", input_coupling, 0.0));
    }
    if r.class_a.is_some() {
        out.push(("class A headroom dBu", headroom, 1.0));
        out.push(("class A asymmetry", asymmetry, 0.0));
    }
    if r.vca.is_some() {
        out.push(("VCA H2 dB", h2, 1.0));
        out.push(("VCA H3 dB", h3, 1.0));
    }
    if r.output.is_some() {
        out.push(("output saturation dBu", output, 1.0));
        out.push(("output hysteresis", output_hysteresis, 0.0));
    }
    out
}

/// How far a recipe measures from its targets.
fn error(recipe: Recipe, bus: bool, targets: &[Target]) -> f32 {
    let mut c = ConsoleColor::of_recipe(48_000, recipe, MEASURED, bus);
    read(&mut c, targets)
        .iter()
        .zip(targets)
        .map(|(got, t)| (got.max(-140.0) - t.db).powi(2))
        .sum()
}

/// Moves each recipe's stages toward its targets and prints the settings found, to write into
/// `recipes.rs`: `cargo test -p audiostrada-dsp --release -- --ignored fit_recipes --nocapture`.
#[test]
#[ignore]
fn fit_recipes() {
    for (model, bus) in CONSOLES {
        let targets = targets(model, bus);
        let knobs = knobs(model, bus);
        let mut best = recipe(model, bus).unwrap();
        let mut best_error = error(best, bus, targets);
        let start = best_error;
        for step in [8.0, 4.0, 2.0, 1.0, 0.5, 0.25] {
            loop {
                // One setting at a time, then two together when none alone helps.
                let mut moves: Vec<Vec<(usize, f32)>> = Vec::new();
                for i in 0..knobs.len() {
                    moves.extend([vec![(i, step)], vec![(i, -step)]]);
                }
                let singles = moves.len();
                for i in 0..knobs.len() {
                    for j in i + 1..knobs.len() {
                        for (a, b) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                            moves.push(vec![(i, a * step), (j, b * step)]);
                        }
                    }
                }
                let mut moved = false;
                for (n, m) in moves.iter().enumerate() {
                    if n == singles && moved {
                        break;
                    }
                    let mut trial = best;
                    for &(k, st) in m {
                        let (_, knob, unit) = knobs[k];
                        nudge(knob(&mut trial), st, unit);
                    }
                    let e = error(trial, bus, targets);
                    if e < best_error - 1e-3 {
                        (best, best_error, moved) = (trial, e, true);
                    }
                }
                if !moved {
                    break;
                }
            }
        }
        println!(
            "{}: error {start:.1} -> {best_error:.1} (rms {:.2} dB)",
            name(model, bus),
            (best_error / targets.len() as f32).sqrt()
        );
        for (label, knob, _) in &knobs {
            println!("    {label:<24} {:.4}", knob(&mut best));
        }
        let read = read(
            &mut ConsoleColor::of_recipe(48_000, best, MEASURED, bus),
            targets,
        );
        for (t, got) in targets.iter().zip(read) {
            println!(
                "    {:>6} Hz {:>4} dBFS {:<4} {got:7.1} for {:6.1}",
                t.freq,
                t.level,
                format!("{:?}", t.reading),
                t.db
            );
        }
    }
}

/// Nanoseconds per sample of each console, a side, without and with its noise: within about
/// 100 ns, 1 % of a core for a stereo channel at 48 kHz.
/// `cargo test -p audiostrada-dsp --release -- --ignored console_budget --nocapture`.
#[test]
#[ignore]
fn console_budget() {
    let input = sine(440.0, -12.0);
    for (model, bus) in CONSOLES {
        let mut cost = [0.0; 2];
        for (k, noise) in [false, true].into_iter().enumerate() {
            let mut c = ConsoleColor::new(48_000);
            c.set(model, MEASURED, bus, noise, Some(1));
            let (mut l, mut r) = (input.clone(), input.clone());
            let start = std::time::Instant::now();
            for _ in 0..20 {
                for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
                    c.process(a, b);
                }
            }
            cost[k] = start.elapsed().as_nanos() as f64 / (input.len() * 20 * 2) as f64;
            std::hint::black_box(&l);
        }
        println!(
            "{:<24} {:6.1} ns/sample a side, {:6.1} with its noise",
            name(model, bus),
            cost[0],
            cost[1]
        );
    }
}

/// Noise with the slope of music: less and less toward the highs.
fn program(seconds: usize) -> Vec<f32> {
    let mut state = 0x2545_f491u32;
    let (mut low, mut lower) = (0.0f32, 0.0f32);
    (0..48_000 * seconds)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (state >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0;
            low += 0.02 * (white - low);
            lower += 0.002 * (white - lower);
            0.1 * white + 0.6 * low + 1.2 * lower
        })
        .collect()
}

/// Level (dB) of `x` between two frequencies.
fn band(x: &[f32], lo: f32, hi: f32) -> f32 {
    crate::report::third_octave_levels(x, 48_000)
        .iter()
        .filter(|b| (lo..=hi).contains(&b.0))
        .map(|b| 10f32.powf(b.1 / 10.0))
        .sum::<f32>()
        .log10()
        * 10.0
}

#[test]
fn the_voicing_grows_with_the_amount_at_an_equal_loudness() {
    let input = program(4);
    let tail = 96_000..;
    let tilt = |x: &[f32]| band(x, 5_000.0, 16_000.0) - band(x, 40.0, 125.0);
    for (model, brighter) in [(1, false), (2, true), (4, false)] {
        let at = |amount: f32| {
            let (l, _) = through(&mut color(model, amount, false), &input, &input);
            (
                gain_to_db(rms(&l[tail.clone()]) / rms(&input[tail.clone()])),
                tilt(&l[tail.clone()]) - tilt(&input[tail.clone()]),
            )
        };
        let (half, full) = (at(50.0), at(100.0));
        // As loud as what went in, at any amount.
        assert!(
            half.0.abs() < 0.4 && full.0.abs() < 0.4,
            "{half:?} {full:?}"
        );
        // British Bright opens the top, British Warm and Nashville soften it; more at 100 %.
        let sign = if brighter { 1.0 } else { -1.0 };
        assert!(
            half.1 * sign > 0.1 && full.1 * sign > half.1 * sign + 0.2,
            "{}: {half:?} {full:?}",
            MODELS[model]
        );
    }
}

#[test]
fn a_bus_is_pushed_half_as_hard_and_british_bright_glues_it() {
    use crate::analog::chain::{bus_drive_db, drive_db};
    assert_eq!(drive_db(MEASURED), 0.0);
    assert_eq!(drive_db(50.0), 12.0);
    assert_eq!(bus_drive_db(50.0), 6.0);
    assert!(drive_db(100.0) > drive_db(50.0) && bus_drive_db(0.0) == drive_db(0.0));
    // A loud hit after a quiet stretch: the glue holds it back, on that console's bus only.
    let mut input = sine(200.0, -30.0);
    input.extend(sine(200.0, -3.0));
    let hit = |model: usize, bus: bool| {
        let (l, _) = through(&mut color(model, 100.0, bus), &input, &input);
        gain_to_db(rms(&l[52_800..57_600]) / rms(&l[24_000..47_000])) - 27.0
    };
    assert!(
        hit(2, true) < hit(2, false) - 1.5,
        "{} {}",
        hit(2, true),
        hit(2, false)
    );
    assert!((hit(3, true) - hit(3, false)).abs() < 1.5);
}
