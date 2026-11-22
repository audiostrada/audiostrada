//! `audiostrada measure-dynamics`: the plugins taken as references set as a file says, measured
//! by `audiostrada_dsp::measure_dynamics`, a JSON file each.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, anyhow, bail};
use audiostrada_dsp::measure::{BLOCK, RATE};
use audiostrada_dsp::measure_dynamics::{Dynamics, Echo, echo, measure};
use serde::Deserialize;

/// A references file: a `[[reference]]` per setting measured.
#[derive(Debug, Deserialize)]
struct References {
    #[serde(rename = "reference")]
    references: Vec<Reference>,
}

/// One setting of a plugin.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    name: String,
    /// The plugin's key (`vst3:…`) or name, as `audiostrada plugins` lists it.
    #[serde(default)]
    plugin: String,
    /// Or a built-in effect, by its id, its parameters in `params` by id or name.
    #[serde(default)]
    effect: Option<String>,
    /// Parameters by name or id, in the plugin's own units (0 to 1 for VST2 and VST3).
    #[serde(default)]
    params: BTreeMap<String, f64>,
    /// Parameters by the number the plugin shows for them (a knob's mark): the value is found
    /// by bisection on the plugin's own text.
    #[serde(default)]
    shown: BTreeMap<String, f64>,
    /// Gain reduction the steps aim at, dB: their loud level is where the static curve
    /// reaches it.
    #[serde(default = "eight")]
    target_db: f32,
    /// Send the settings with every block, for plugins that drop them otherwise.
    #[serde(default)]
    resend: bool,
    /// Measure an equalizer's curve only (in `response_quiet`, at -30 dBFS).
    #[serde(default)]
    eq: bool,
    /// Measure a tape echo instead (set to play its echo alone).
    #[serde(default)]
    echo: bool,
}

fn eight() -> f32 {
    8.0
}

/// The first number of a plugin's text for a value, in the smaller of its units when the text
/// switches between two (`1.00 s` after `990 ms`, `1.2 kHz` after `990 Hz`).
fn number(text: &str) -> Option<f64> {
    let start = text.find(|c: char| c.is_ascii_digit() || c == '-' || c == '.')?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == '.'))
        .unwrap_or(rest.len());
    let value: f64 = rest[..end].parse().ok()?;
    let unit = rest[end..].trim_start();
    let scale = if unit.starts_with("kHz") || (unit.starts_with('s') && !unit.starts_with("st")) {
        1_000.0
    } else {
        1.0
    };
    Some(value * scale)
}

/// What the plugin shows for parameter `id` once set to `value`: applied for real, with a
/// block of silence, since some plugins only tell the text of the value they have (VST2, and
/// VST3 controllers that ignore the value asked about).
fn shown_at(
    instance: &mut dyn audiostrada_plugins::PluginInstance,
    processor: &mut dyn audiostrada_plugins::PluginProcessor,
    id: u32,
    value: f64,
) -> Option<String> {
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    processor.process(&mut l, &mut r, &[(id, value)], &[], &mut |_, _| {});
    instance.set_param(id, value);
    let _ = instance.poll();
    let current = instance.param_value(id).unwrap_or(value);
    instance
        .value_text(id, current)
        .or_else(|| instance.value_text(id, value))
}

/// The value of parameter `id` for which the plugin shows `target`, by bisection.
fn value_showing(
    instance: &mut dyn audiostrada_plugins::PluginInstance,
    processor: &mut dyn audiostrada_plugins::PluginProcessor,
    id: u32,
    min: f64,
    max: f64,
    target: f64,
) -> anyhow::Result<f64> {
    let mut read = |v: f64| {
        shown_at(instance, processor, id, v)
            .as_deref()
            .and_then(number)
            .ok_or_else(|| anyhow!("parameter {id} shows no number"))
    };
    // An end may show a word (off, infinity): the search starts a little inside.
    let inside = |from: f64, towards: f64| {
        [0.0, 0.001, 0.01, 0.05]
            .into_iter()
            .map(move |k| from + (towards - from) * k)
    };
    let mut ends = Vec::new();
    for (from, towards) in [(min, max), (max, min)] {
        let found = inside(from, towards).find_map(|v| read(v).ok().map(|shown| (v, shown)));
        ends.push(found.ok_or_else(|| anyhow!("parameter {id} shows no number"))?);
    }
    let ((min, at_min), (max, at_max)) = (ends[0], ends[1]);
    let rising = at_max >= at_min;
    let (mut lo, mut hi) = (min, max);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        let shown = read(mid)?;
        if (shown < target) == rising {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // The bound that shows the mark nearest the one asked, `hi` on a tie.
    let (at_lo, at_hi) = (read(lo)?, read(hi)?);
    Ok(if (at_lo - target).abs() < (at_hi - target).abs() {
        lo
    } else {
        hi
    })
}

/// Measures a built-in effect.
fn builtin(reference: &Reference, id: &str) -> anyhow::Result<Measured> {
    let info = audiostrada_dsp::info(id).ok_or_else(|| anyhow!("no effect '{id}'"))?;
    let mut effect = audiostrada_dsp::create(id).ok_or_else(|| anyhow!("no effect '{id}'"))?;
    effect.prepare(RATE, BLOCK);
    for (i, p) in info.params.iter().enumerate() {
        effect.set_param(i, p.default);
    }
    let mut settings = Vec::new();
    for (key, &value) in &reference.params {
        let i = info
            .params
            .iter()
            .position(|p| p.id == key || p.name.eq_ignore_ascii_case(key))
            .ok_or_else(|| anyhow!("{id} has no parameter '{key}'"))?;
        effect.set_param(i, value as f32);
        settings.push((info.params[i].name.to_owned(), format!("{value}")));
    }
    let latency = effect.latency() as usize;
    Ok(dynamics_or_curve(
        reference,
        settings,
        &mut |l, r| effect.process(l, r),
        latency,
    ))
}

/// What a reference gave: the dynamics battery (or an equalizer's curve), or an echo's.
enum Measured {
    Dynamics(Dynamics),
    Echo(Echo),
}

/// The whole battery, an equalizer's curve alone, or an echo's measures.
fn dynamics_or_curve(
    reference: &Reference,
    settings: Vec<(String, String)>,
    process: &mut dyn FnMut(&mut [f32], &mut [f32]),
    latency: usize,
) -> Measured {
    if reference.echo {
        return Measured::Echo(echo(&reference.name, settings, process));
    }
    if reference.eq {
        return Measured::Dynamics(Dynamics {
            name: reference.name.clone(),
            settings,
            latency,
            small_gain_db: 0.0,
            static_1k: Vec::new(),
            static_100: Vec::new(),
            response_quiet: audiostrada_dsp::measure_dynamics::curve(process, latency, -30.0),
            response_loud: Vec::new(),
            steps: Vec::new(),
        });
    }
    Measured::Dynamics(measure(
        &reference.name,
        settings,
        process,
        latency,
        reference.target_db,
        true,
        None,
    ))
}

fn one(reference: &Reference) -> anyhow::Result<Measured> {
    if let Some(id) = &reference.effect {
        return builtin(reference, id);
    }
    let mut instance = crate::measure::load_plugin(&reference.plugin)?;
    let params = instance.params();
    let find = |key: &str| {
        params
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(key) || p.id.to_string() == key)
            .cloned()
            .ok_or_else(|| anyhow!("{} has no parameter '{key}'", reference.plugin))
    };
    let mut processor = instance.activate(RATE, BLOCK).map_err(|e| anyhow!(e))?;
    processor.set_offline(true);
    let mut changes = Vec::new();
    for (key, &value) in &reference.params {
        changes.push((find(key)?.id, value));
    }
    // The switches first: a knob's text may depend on them.
    for &(id, value) in &changes {
        let _ = shown_at(instance.as_mut(), processor.as_mut(), id, value);
    }
    for (key, &target) in &reference.shown {
        let p = find(key)?;
        let value = value_showing(
            instance.as_mut(),
            processor.as_mut(),
            p.id,
            p.min,
            p.max,
            target,
        )?;
        changes.push((p.id, value));
    }
    let mut settings = Vec::new();
    for &(id, value) in &changes {
        let name = params
            .iter()
            .find(|p| p.id == id)
            .map_or(String::new(), |p| p.name.clone());
        let shown = shown_at(instance.as_mut(), processor.as_mut(), id, value)
            .unwrap_or_else(|| format!("{value:.4}"));
        settings.push((name, shown));
    }
    let latency = instance.latency() as usize;
    // The settings go with every block: some plugins only take them once running, or after
    // they settled.
    let mut blocks = 0;
    let resend = reference.resend;
    let mut process = |l: &mut [f32], r: &mut [f32]| {
        let pending: &[(u32, f64)] = if blocks < 4 || resend { &changes } else { &[] };
        processor.process(l, r, pending, &[], &mut |_, _| {});
        blocks += 1;
    };
    let print = dynamics_or_curve(reference, settings, &mut process, latency);
    drop(processor);
    let _ = instance.try_deactivate();
    Ok(print)
}

/// Measures every reference of `file`, a JSON file each in `out`.
pub fn references(file: &Path, out: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
    let references: References = toml::from_str(&text).context("invalid references file")?;
    std::fs::create_dir_all(out).with_context(|| out.display().to_string())?;
    let mut failed = 0;
    for reference in &references.references {
        let started = std::time::Instant::now();
        match one(reference) {
            Ok(measured) => {
                let path = out.join(format!("{}.json", crate::measure::slug(&reference.name)));
                let json = match &measured {
                    Measured::Dynamics(print) => serde_json::to_string_pretty(print)?,
                    Measured::Echo(echo) => serde_json::to_string_pretty(echo)?,
                };
                std::fs::write(&path, json).with_context(|| path.display().to_string())?;
                match &measured {
                    Measured::Dynamics(print) => {
                        summary(print, started.elapsed().as_secs_f32(), &path);
                    }
                    Measured::Echo(echo) => println!(
                        "{}: repeats {:?}, wobble {:.1} cents at {:.2} Hz → {}",
                        echo.name,
                        &echo.repeats[..echo.repeats.len().min(4)],
                        echo.wobble_cents,
                        echo.wobble_hz,
                        path.display()
                    ),
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("{}: {e:#}", reference.name);
            }
        }
    }
    if failed > 0 {
        bail!("{failed} reference(s) not measured");
    }
    Ok(())
}

fn summary(d: &Dynamics, seconds: f32, path: &Path) {
    let settings: Vec<String> = d.settings.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("{} ({})", d.name, settings.join(", "));
    let onset = d
        .static_1k
        .iter()
        .find(|p| p.reduction_db >= 1.0)
        .map_or("-".into(), |p| format!("{:.0}", p.level));
    let at = |level: f32| {
        d.static_1k
            .iter()
            .find(|p| (p.level - level).abs() < 0.5)
            .map_or(0.0, |p| p.reduction_db)
    };
    println!(
        "  small gain {:.1} dB, 1 dB of reduction from {onset} dBFS, reduction at -18/-6/0 dBFS {:.1}/{:.1}/{:.1} dB, latency {}",
        d.small_gain_db,
        at(-18.0),
        at(-6.0),
        at(0.0),
        d.latency
    );
    let ms = |v: Option<f32>| v.map_or("-".into(), |v| format!("{v:.0}"));
    for s in &d.steps {
        println!(
            "  step to {:.0} dBFS held {:>5.0} ms: {:.1} dB, attack 50/90 % {}/{} ms, release 50/25/10 %/1 dB {}/{}/{}/{} ms",
            s.level,
            s.hold_ms,
            s.reduction_end_db,
            ms(s.attack_50_ms),
            ms(s.attack_90_ms),
            ms(s.release_50_ms),
            ms(s.release_25_ms),
            ms(s.release_10_ms),
            ms(s.release_1db_ms)
        );
    }
    println!("  measured in {seconds:.0} s → {}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_read_from_texts() {
        assert_eq!(number("46.00"), Some(46.0));
        assert_eq!(number("Gain 30 dB"), Some(30.0));
        assert_eq!(number("-3.5 dB"), Some(-3.5));
        assert_eq!(number("Comp"), None);
        assert_eq!(number("1.50 s"), Some(1_500.0));
        assert_eq!(number("990 ms"), Some(990.0));
        assert_eq!(number("2.2 kHz"), Some(2_200.0));
    }
}
