//! `audiostrada measure`: fingerprints of the built-in consoles and effects and of plugins taken
//! as references, and a page comparing them (see `audiostrada_dsp::measure`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use audiostrada_dsp::console::{ConsoleColor, MODELS};
use audiostrada_dsp::measure::{self, BLOCK, Fingerprint, RATE};
use audiostrada_plugins::PluginInstance;
use serde::Deserialize;

/// A references file: what to measure, and how each one is set.
#[derive(Debug, Deserialize)]
struct References {
    #[serde(rename = "reference")]
    references: Vec<Reference>,
}

/// One processor to measure: a built-in console, a built-in effect or a plugin.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub name: String,
    /// A console of [`MODELS`], by name.
    #[serde(default)]
    pub console: Option<String>,
    /// A built-in effect, by its id.
    #[serde(default)]
    pub effect: Option<String>,
    /// A plugin, by its key (`vst3:…`) or its name.
    #[serde(default)]
    pub plugin: Option<String>,
    /// A console's amount, 0 to 100.
    #[serde(default = "half")]
    pub amount: f32,
    /// A console summing a bus rather than a channel's input.
    #[serde(default)]
    pub bus: bool,
    /// A console's hiss and crosstalk.
    #[serde(default)]
    pub noise: bool,
    /// Gain before the processor, taken back after it: moves the test levels to where the
    /// reference has its working level.
    #[serde(default)]
    pub input_db: f32,
    /// Parameters by name or id, in the processor's own units.
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

fn half() -> f32 {
    50.0
}

/// Measures every reference of `file`, a fingerprint per reference in `out`.
pub fn references(file: &Path, out: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
    let references: References = toml::from_str(&text).context("invalid references file")?;
    std::fs::create_dir_all(out).with_context(|| out.display().to_string())?;
    let mut failed = 0;
    for reference in &references.references {
        match one(reference) {
            Ok(print) => {
                let path = out.join(format!("{}.json", slug(&reference.name)));
                save(&print, &path)?;
                summary(&print, &path);
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

/// Measures one reference.
pub fn one(reference: &Reference) -> anyhow::Result<Fingerprint> {
    let trim = 10f32.powf(reference.input_db / 20.0);
    let measured = |inner: &mut dyn FnMut(&mut [f32], &mut [f32]), latency: usize| {
        let mut process = |l: &mut [f32], r: &mut [f32]| {
            for v in l.iter_mut().chain(r.iter_mut()) {
                *v *= trim;
            }
            inner(l, r);
            for v in l.iter_mut().chain(r.iter_mut()) {
                *v /= trim;
            }
        };
        measure::measure(&reference.name, &mut process, latency)
    };
    match (&reference.console, &reference.effect, &reference.plugin) {
        (Some(console), None, None) => {
            let model = MODELS
                .iter()
                .position(|m| m.eq_ignore_ascii_case(console))
                .ok_or_else(|| anyhow!("no console '{console}' (one of {})", MODELS.join(", ")))?;
            let mut color = ConsoleColor::new(RATE);
            color.set(
                model,
                reference.amount,
                reference.bus,
                reference.noise,
                None,
            );
            Ok(measured(&mut |l, r| color.process(l, r), 0))
        }
        (None, Some(id), None) => {
            let info = audiostrada_dsp::info(id).ok_or_else(|| anyhow!("no effect '{id}'"))?;
            let mut effect =
                audiostrada_dsp::create(id).ok_or_else(|| anyhow!("no effect '{id}'"))?;
            effect.prepare(RATE, BLOCK);
            for (i, p) in info.params.iter().enumerate() {
                effect.set_param(i, p.default);
            }
            for (key, &value) in &reference.params {
                let i = info
                    .params
                    .iter()
                    .position(|p| p.id == key || p.name.eq_ignore_ascii_case(key))
                    .ok_or_else(|| anyhow!("{id} has no parameter '{key}'"))?;
                effect.set_param(i, value as f32);
            }
            let latency = effect.latency() as usize;
            Ok(measured(&mut |l, r| effect.process(l, r), latency))
        }
        (None, None, Some(query)) => {
            let mut instance = load_plugin(query)?;
            let params = instance.params();
            let mut changes = Vec::new();
            for (key, &value) in &reference.params {
                let p = params
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(key) || p.id.to_string() == *key)
                    .ok_or_else(|| anyhow!("{query} has no parameter '{key}'"))?;
                instance.set_param(p.id, value);
                changes.push((p.id, value));
            }
            let mut processor = instance.activate(RATE, BLOCK).map_err(|e| anyhow!(e))?;
            processor.set_offline(true);
            let latency = instance.latency() as usize;
            // The settings go with the first block; the plugin keeps them.
            let mut pending = changes.clone();
            let print = measured(
                &mut |l, r| {
                    processor.process(l, r, &pending, &[], &mut |_, _| {});
                    pending.clear();
                },
                latency,
            );
            drop(processor);
            // What the plugin shows for each setting, to check it took the one meant: a VST2
            // plugin only tells the text of the value it has.
            for (id, value) in changes {
                let name = params.iter().find(|p| p.id == id).map_or("", |p| &p.name);
                let shown = instance.value_text(id, value).unwrap_or_else(|| "?".into());
                println!("  {}: {name} = {shown}", reference.name);
            }
            let _ = instance.try_deactivate();
            Ok(print)
        }
        _ => bail!("give one of console, effect or plugin"),
    }
}

/// The installed plugin of key or name `query`, loaded in this process.
pub(crate) fn load_plugin(query: &str) -> anyhow::Result<Box<dyn PluginInstance>> {
    let database = crate::audio::plugin_database(&crate::prefs::Prefs::load().ui, false);
    let lower = query.to_lowercase();
    let descriptor = database
        .find(query)
        .or_else(|| database.plugins().find(|p| p.name.to_lowercase() == lower))
        .or_else(|| {
            database
                .plugins()
                .find(|p| p.name.to_lowercase().contains(&lower))
        })
        .cloned()
        .ok_or_else(|| anyhow!("no plugin '{query}'; `audiostrada plugins` lists them"))?;
    audiostrada_plugins::instantiate_in_process(&descriptor).map_err(|e| anyhow!(e))
}

/// Lists a plugin's parameters, to write the references file.
pub fn plugin_params(query: &str) -> anyhow::Result<()> {
    let mut instance = load_plugin(query)?;
    let name = instance.descriptor().name.clone();
    let key = instance.descriptor().key();
    println!("{name} ({key})\n");
    println!(
        "{:>10}  {:<40} {:>10} {:>10} {:>10}  shown",
        "id", "name", "min", "max", "default"
    );
    for p in instance.params() {
        if p.hidden {
            continue;
        }
        let shown = instance.value_text(p.id, p.default).unwrap_or_default();
        println!(
            "{:>10}  {:<40} {:>10.3} {:>10.3} {:>10.3}  {shown}",
            p.id, p.name, p.min, p.max, p.default
        );
    }
    Ok(())
}

pub fn save(print: &Fingerprint, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(print)?;
    std::fs::write(path, json).with_context(|| path.display().to_string())
}

fn load(path: &Path) -> anyhow::Result<Fingerprint> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    serde_json::from_str(&text).with_context(|| format!("{}: not a fingerprint", path.display()))
}

/// A file name for a reference's name.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// A line per fingerprint written.
fn summary(print: &Fingerprint, path: &Path) {
    let at = |freq, level| {
        print
            .tone(freq, level)
            .map_or(String::from("-"), |t| format!("{:.1}", t.thd_db))
    };
    println!(
        "{:<36} THD 100 Hz {} / 1 kHz {} dB at -6 dBFS, noise {:.1} dBFS, latency {} → {}",
        print.name,
        at(100.0, -6.0),
        at(1_000.0, -6.0),
        print.noise.rms_db,
        print.latency,
        path.display()
    );
}

// The comparison page.

const COLORS: [&str; 8] = [
    "#2f6fdb", "#d9480f", "#2b8a3e", "#ae3ec9", "#e67700", "#0c8599", "#c2255c", "#5c636e",
];

/// One curve of a chart: a fingerprint's points.
struct Series<'a> {
    name: &'a str,
    color: &'a str,
    points: Vec<(f32, f32)>,
}

/// A chart as inline SVG; `log` puts the x axis in octaves.
fn chart(title: &str, x_label: &str, y_label: &str, log: bool, series: &[Series]) -> String {
    let (w, h, left, right, top, bottom) = (560.0, 300.0, 56.0, 16.0, 30.0, 40.0);
    let all = series.iter().flat_map(|s| s.points.iter());
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for &(x, y) in all {
        if y <= -199.0 {
            continue;
        }
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if x0 > x1 {
        return format!("<figure><figcaption>{title}</figcaption><p>No data</p></figure>");
    }
    let (y0, y1) = (
        (y0 / 6.0).floor() * 6.0,
        ((y1 / 6.0).ceil() * 6.0).max(y0 + 6.0),
    );
    let fx = |x: f32| {
        let (a, b, v) = if log {
            (x0.ln(), x1.ln(), x.ln())
        } else {
            (x0, x1, x)
        };
        left + (v - a) / (b - a).max(1e-6) * (w - left - right)
    };
    let fy = |y: f32| top + (y1 - y) / (y1 - y0) * (h - top - bottom);
    let mut svg = format!(
        r#"<figure><figcaption>{title}</figcaption><svg viewBox="0 0 {w} {h}" role="img" aria-label="{title}">"#
    );
    let steps = ((y1 - y0) / 6.0) as usize;
    let every = steps.div_ceil(8).max(1);
    for k in (0..=steps).step_by(every) {
        let y = y0 + k as f32 * 6.0;
        let _ = write!(
            svg,
            r##"<line x1="{left}" x2="{}" y1="{py}" y2="{py}" stroke="#e3e6ea"/><text x="{}" y="{}" text-anchor="end" font-size="11" fill="#5c636e">{y}</text>"##,
            w - right,
            left - 6.0,
            fy(y) + 4.0,
            py = fy(y)
        );
    }
    let ticks: Vec<f32> = series
        .first()
        .map(|s| s.points.iter().map(|p| p.0).collect())
        .unwrap_or_default();
    for x in ticks {
        let label = if x >= 1_000.0 {
            format!("{}k", x / 1_000.0)
        } else {
            format!("{x}")
        };
        let _ = write!(
            svg,
            r##"<text x="{}" y="{}" text-anchor="middle" font-size="11" fill="#5c636e">{label}</text>"##,
            fx(x),
            h - bottom + 16.0
        );
    }
    let _ = write!(
        svg,
        r##"<text x="{}" y="{}" text-anchor="middle" font-size="11" fill="#5c636e">{x_label}</text><text x="12" y="{}" font-size="11" fill="#5c636e" transform="rotate(-90 12 {})" text-anchor="middle">{y_label}</text>"##,
        (left + w - right) / 2.0,
        h - 6.0,
        (top + h - bottom) / 2.0,
        (top + h - bottom) / 2.0
    );
    for s in series {
        let points: Vec<String> = s
            .points
            .iter()
            .filter(|p| p.1 > -199.0)
            .map(|&(x, y)| format!("{:.1},{:.1}", fx(x), fy(y.clamp(y0, y1))))
            .collect();
        let _ = write!(
            svg,
            r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="2"><title>{}</title></polyline>"#,
            points.join(" "),
            s.color,
            s.name
        );
        for p in &points {
            let (x, y) = p.split_once(',').unwrap_or(("0", "0"));
            let _ = write!(
                svg,
                r#"<circle cx="{x}" cy="{y}" r="2.5" fill="{}"/>"#,
                s.color
            );
        }
    }
    svg.push_str("</svg></figure>");
    svg
}

/// The page comparing `files`, written to `output`.
pub fn report(files: &[PathBuf], output: &Path) -> anyhow::Result<()> {
    // A folder stands for its fingerprints: PowerShell does not expand `*.json`.
    let mut paths = Vec::new();
    for f in files {
        if f.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(f)
                .with_context(|| f.display().to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect();
            inside.sort();
            paths.extend(inside);
        } else {
            paths.push(f.clone());
        }
    }
    let prints = paths
        .iter()
        .map(|f| load(f))
        .collect::<anyhow::Result<Vec<_>>>()?;
    std::fs::write(output, page(&prints)).with_context(|| output.display().to_string())?;
    println!(
        "{} fingerprint(s) compared in {}",
        prints.len(),
        output.display()
    );
    Ok(())
}

fn page(prints: &[Fingerprint]) -> String {
    let series = |f: &dyn Fn(&Fingerprint) -> Vec<(f32, f32)>| -> Vec<Series> {
        prints
            .iter()
            .enumerate()
            .map(|(i, p)| Series {
                name: &p.name,
                color: COLORS[i % COLORS.len()],
                points: f(p),
            })
            .collect()
    };
    let gain: Reading = |t| Some(t.gain_db);
    let thd: Reading = |t| Some(t.thd_db);
    let h2: Reading = |t| t.harmonics[0];
    let h3: Reading = |t| t.harmonics[1];
    let charts = [
        chart(
            "Response at the working level (-18 dBFS)",
            "Hz",
            "dB",
            true,
            &series(&|p| by_freq(p, -18.0, gain)),
        ),
        chart(
            "Distortion by frequency at -6 dBFS",
            "Hz",
            "THD, dB",
            true,
            &series(&|p| by_freq(p, -6.0, thd)),
        ),
        chart(
            "2nd harmonic at 1 kHz, by level",
            "dBFS in",
            "dB below the tone",
            false,
            &series(&|p| by_level(p, 1_000.0, h2)),
        ),
        chart(
            "3rd harmonic at 1 kHz, by level",
            "dBFS in",
            "dB below the tone",
            false,
            &series(&|p| by_level(p, 1_000.0, h3)),
        ),
        chart(
            "Distortion at 100 Hz, by level",
            "dBFS in",
            "THD, dB",
            false,
            &series(&|p| by_level(p, 100.0, thd)),
        ),
        chart(
            "Intermodulation (SMPTE), by level",
            "dBFS in",
            "dB",
            false,
            &series(&|p: &Fingerprint| p.imd.iter().map(|i| (i.level, i.smpte_db)).collect()),
        ),
        chart(
            "Gain by level at 1 kHz",
            "dBFS in",
            "dB",
            false,
            &series(&|p| by_level(p, 1_000.0, gain)),
        ),
        chart(
            "Noise by octave",
            "Hz",
            "dBFS",
            true,
            &series(&|p: &Fingerprint| p.noise.bands.clone()),
        ),
    ];
    let mut rows = String::new();
    for (i, p) in prints.iter().enumerate() {
        let crosstalk = p
            .crosstalk
            .iter()
            .find(|c| (c.0 - 1_000.0).abs() < 1.0)
            .map_or(-200.0, |c| c.1);
        let m = &p.memory;
        let _ = write!(
            rows,
            r#"<tr><td><span class="dot" style="background:{}"></span>{}</td><td>{}</td><td>{:.1}</td><td>{:.1}</td><td>{:.1}</td><td>{:.2}</td><td>{:.1} → {:.1}</td><td>{:.0}</td></tr>"#,
            COLORS[i % COLORS.len()],
            escape(&p.name),
            p.latency,
            p.noise.rms_db,
            p.noise.hum_db,
            crosstalk,
            p.balance_db,
            m.h2_after_db,
            m.h2_settled_db,
            m.recovery_ms
        );
    }
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Console fingerprints</title><style>
body{{margin:0;padding:24px;background:#f7f8fa;color:#1d2125;font:14px/1.5 system-ui,sans-serif}}
h1{{font-size:20px;margin:0 0 4px}}p.lead{{color:#5c636e;margin:0 0 20px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(420px,1fr));gap:16px}}
figure{{margin:0;background:#fff;border:1px solid #e3e6ea;border-radius:8px;padding:12px}}
figcaption{{font-weight:600;margin-bottom:6px}}svg{{width:100%;height:auto}}
table{{border-collapse:collapse;background:#fff;margin:20px 0;width:100%}}
td,th{{border:1px solid #e3e6ea;padding:6px 10px;text-align:right}}td:first-child,th:first-child{{text-align:left}}
.dot{{display:inline-block;width:10px;height:10px;border-radius:50%;margin-right:8px}}
</style></head><body><h1>Console fingerprints</h1><p class="lead">Tones, harmonics and noise of {} processor(s), measured at {} Hz. Levels in dBFS; the working level is -18 dBFS.</p>
<table><tr><th>Processor</th><th>Latency</th><th>Noise dBFS</th><th>Hum dBFS</th><th>Crosstalk 1 kHz</th><th>R-L dB</th><th>H2 after a loud passage → settled</th><th>Recovery ms</th></tr>{rows}</table>
<div class="grid">{}</div></body></html>"#,
        prints.len(),
        RATE,
        charts.join("")
    )
}

/// What a chart shows of a tone.
type Reading = fn(&measure::Tone) -> Option<f32>;

/// A reading of the tones of `level` by frequency.
fn by_freq(p: &Fingerprint, level: f32, value: Reading) -> Vec<(f32, f32)> {
    measure::FREQUENCIES
        .iter()
        .filter_map(|&f| p.tone(f, level).and_then(value).map(|v| (f, v)))
        .collect()
}

/// A reading of the tones of `freq` by level.
fn by_level(p: &Fingerprint, freq: f32, value: Reading) -> Vec<(f32, f32)> {
    measure::LEVELS
        .iter()
        .filter_map(|&l| p.tone(freq, l).and_then(value).map(|v| (l, v)))
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_consoles_are_measured_and_compared() {
        let reference = Reference {
            name: "British Warm".into(),
            console: Some("british warm".into()),
            amount: 50.0,
            ..Default::default()
        };
        let warm = one(&reference).unwrap();
        let clean = one(&Reference {
            name: "Clean".into(),
            console: Some("Clean".into()),
            ..Default::default()
        })
        .unwrap();
        let thd = |p: &Fingerprint| p.tone(100.0, -6.0).unwrap().thd_db;
        assert!(
            thd(&warm) > thd(&clean) + 40.0,
            "{} {}",
            thd(&warm),
            thd(&clean)
        );
        let page = page(&[warm, clean]);
        assert!(page.contains("<svg") && page.contains("British Warm"));
        assert!(one(&Reference::default()).is_err());
        assert_eq!(slug("Console Channel 2 (E)"), "console-channel-2-e");
    }

    #[test]
    fn an_effect_is_set_by_its_parameters() {
        let reference = Reference {
            name: "Drive".into(),
            effect: Some("drive".into()),
            input_db: -6.0,
            ..Default::default()
        };
        assert!(one(&reference).is_ok());
        let bad = Reference {
            params: [("nope".to_owned(), 1.0)].into(),
            ..reference
        };
        assert!(one(&bad).is_err());
    }
}
