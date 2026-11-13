//! `audiostrada measure-presets`: every factory preset of the built-in effects played on a
//! sound of its kind (a folder of recordings: `vocal.wav`, `kick.wav`, `mix.wav`...) and on a
//! steady tone, and what it did to them in figures: loudness, peaks, gain reduction, the
//! change by frequency band, the tail, the modulation. A plugin taken as a reference goes
//! through the same measures.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use audiostrada_dsp::report::{Report, report};
use audiostrada_dsp::{Effect, EffectInfo, MidiEvent};
use audiostrada_media::AudioData;

const RATE: u32 = 48_000;
const BLOCK: usize = 256;
/// Silence played after the sound, to hear the tail.
const TAIL_SECONDS: usize = 8;
const BPM: f64 = 120.0;

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    /// Folder of the sounds.
    pub audio: PathBuf,
    /// Effects to measure; all of them when empty.
    pub effects: Vec<String>,
    /// Only the presets whose name contains this.
    pub only: Option<String>,
    /// Folder receiving what each preset gave, to listen to.
    pub write: Option<PathBuf>,
    /// A plugin to measure instead, on `source`.
    pub plugin: Option<String>,
    pub source: Option<String>,
    /// Parameters of the plugin (or of the one effect given), by name or id.
    pub set: Vec<(String, f64)>,
    /// Presets to measure in place of the factory ones: a JSON file, by effect, of
    /// `[name, {parameter: value}]` pairs.
    pub presets: Option<PathBuf>,
    /// Print the factory presets as such a file instead of measuring.
    pub dump: bool,
}

type Stereo = (Vec<f32>, Vec<f32>);
/// Presets by name: the parameters each one sets.
type Presets = Vec<(String, BTreeMap<String, f32>)>;
/// Processes a block that starts at a frame.
type Process<'a> = &'a mut dyn FnMut(&mut [f32], &mut [f32], usize);

/// The sound a preset is made for: what the role its name opens with says, else what the
/// rest of the name says ("Bus: drum glue").
fn source_of(effect: &str, preset: &str) -> &'static str {
    let role = preset.split(':').next().unwrap_or("").to_lowercase();
    let whole = preset.to_lowercase();
    let by_words = |text: &str| -> Option<&'static str> {
        let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
        Some(if has(&["backing"]) {
            "backing"
        } else if has(&["vocal", "voice", "voix", "podcast"]) {
            "vocal"
        } else if has(&["kick"]) {
            "kick"
        } else if has(&["snare"]) {
            "snare"
        } else if has(&["tom"]) {
            "toms"
        } else if has(&["overhead", "cymbal", "hi-hat"]) {
            "overheads"
        } else if has(&["drum", "room", "parallel"]) {
            "drums"
        } else if has(&["bass"]) {
            "bass"
        } else if has(&["acoustic"]) {
            "acoustic"
        } else if has(&["guitar"]) {
            if matches!(effect, "amp_rig" | "drive") {
                "guitar_di"
            } else {
                "guitar"
            }
        } else if has(&["pad", "string", "ambient"]) {
            "pad"
        } else if has(&["key", "piano", "synth", "organ"]) {
            "keys"
        } else {
            return None;
        })
    };
    by_words(&role)
        .or_else(|| by_words(&whole))
        .unwrap_or(match effect {
            "amp_rig" => "guitar_di",
            "pitch_correct" | "deesser" | "doubler" => "vocal",
            _ => "mix",
        })
}

/// Sounds by name, read from the folder once; `keys` and `pad` are played by Strada-Synth
/// when the folder has no recording of them.
struct Sources {
    folder: PathBuf,
    loaded: BTreeMap<String, Stereo>,
}

impl Sources {
    fn get(&mut self, name: &str) -> anyhow::Result<&Stereo> {
        if !self.loaded.contains_key(name) {
            let path = self.folder.join(format!("{name}.wav"));
            let sound = if path.exists() {
                let data = audiostrada_media::load_at(&path, RATE)
                    .with_context(|| path.display().to_string())?;
                let left = data.channel(0).to_vec();
                let right = if data.channels() > 1 {
                    data.channel(1).to_vec()
                } else {
                    left.clone()
                };
                (left, right)
            } else {
                match name {
                    "keys" => synth("Keys: Electric", false)?,
                    "pad" => synth("Pad: Warm", true)?,
                    _ => bail!("no {}", path.display()),
                }
            };
            self.loaded.insert(name.to_owned(), sound);
        }
        Ok(&self.loaded[name])
    }
}

/// Twelve seconds of chords from a factory sound of Strada-Synth.
fn synth(sound: &str, held: bool) -> anyhow::Result<Stereo> {
    let info = audiostrada_dsp::info("synth").ok_or_else(|| anyhow!("no synth"))?;
    let preset = audiostrada_ui::factory_presets("synth")
        .into_iter()
        .find(|p| p.0 == sound)
        .ok_or_else(|| anyhow!("no synth sound '{sound}'"))?;
    let mut fx = audiostrada_dsp::create("synth").ok_or_else(|| anyhow!("no synth"))?;
    fx.prepare(RATE, BLOCK);
    set_params(fx.as_mut(), info, &preset.1);
    fx.set_tempo(BPM);
    let chords: [[u8; 4]; 4] = [
        [48, 60, 64, 67],
        [45, 60, 64, 69],
        [41, 57, 60, 65],
        [43, 59, 62, 67],
    ];
    let frames = RATE as usize * 12;
    let beat = RATE as usize / 2;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut at = 0;
    while at < frames {
        let n = BLOCK.min(frames - at);
        if at % beat < BLOCK {
            let step = at / beat;
            let chord = chords[(step / 4) % 4];
            let before = chords[((step + 15) / 4) % 4];
            if held {
                if step.is_multiple_of(4) {
                    for key in before {
                        fx.midi(&MidiEvent::note_off(0, 0, key));
                    }
                    for key in chord {
                        fx.midi(&MidiEvent::note_on(0, 0, key, 90));
                    }
                }
            } else {
                for key in before.iter().chain(&chord) {
                    fx.midi(&MidiEvent::note_off(0, 0, *key));
                }
                let keys: &[u8] = if step.is_multiple_of(2) {
                    &chord
                } else {
                    &chord[1..]
                };
                for key in keys {
                    fx.midi(&MidiEvent::note_on(0, 0, *key, 80 + (step % 3) as u8 * 15));
                }
            }
        }
        fx.process(&mut left[at..at + n], &mut right[at..at + n]);
        at += n;
    }
    let peak = left
        .iter()
        .chain(&right)
        .fold(0.0f32, |m, v| m.max(v.abs()))
        .max(1e-6);
    let gain = 0.5 / peak;
    for v in left.iter_mut().chain(right.iter_mut()) {
        *v *= gain;
    }
    Ok((left, right))
}

fn set_params(fx: &mut dyn Effect, info: &EffectInfo, values: &BTreeMap<String, f32>) {
    for (i, p) in info.params.iter().enumerate() {
        fx.set_param(i, values.get(p.id).copied().unwrap_or(p.default));
    }
}

/// What ran: the sound it gave, with its tail, and what its meters showed.
struct Run {
    out: Stereo,
    /// Lowest, highest and mean value of each named meter.
    meters: Vec<(String, f32, f32, f32)>,
}

/// Plays `input` then silence through `process`, taking the latency back.
fn play(input: &Stereo, latency: usize, tail: usize, process: Process) -> Stereo {
    let total = input.0.len() + tail + latency;
    let mut left = input.0.clone();
    let mut right = input.1.clone();
    left.resize(total, 0.0);
    right.resize(total, 0.0);
    let mut at = 0;
    while at < total {
        let n = BLOCK.min(total - at);
        process(&mut left[at..at + n], &mut right[at..at + n], at);
        at += n;
    }
    left.drain(..latency);
    right.drain(..latency);
    (left, right)
}

fn run_effect(
    id: &str,
    info: &EffectInfo,
    values: &BTreeMap<String, f32>,
    input: &Stereo,
    key: Option<&Stereo>,
    tail: usize,
) -> anyhow::Result<Run> {
    let mut fx = audiostrada_dsp::create(id).ok_or_else(|| anyhow!("no effect '{id}'"))?;
    fx.prepare(RATE, BLOCK);
    set_params(fx.as_mut(), info, values);
    fx.set_tempo(BPM);
    fx.set_watched(true);
    let latency = fx.latency() as usize;
    let mut shown = vec![0.0f32; info.meters.len()];
    let mut ranges = vec![(f32::MAX, f32::MIN, 0.0f64, 0usize); info.meters.len()];
    let (mut kl, mut kr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let out = play(input, latency, tail, &mut |l, r, at| {
        fx.set_tempo(BPM);
        match key {
            Some(key) => {
                for i in 0..l.len() {
                    kl[i] = key.0.get(at + i).copied().unwrap_or(0.0);
                    kr[i] = key.1.get(at + i).copied().unwrap_or(0.0);
                }
                fx.process_keyed(l, r, (&kl[..l.len()], &kr[..l.len()]));
            }
            None => fx.process(l, r),
        }
        if at < input.0.len() {
            fx.meters(&mut shown);
            for (range, v) in ranges.iter_mut().zip(&shown) {
                *range = (
                    range.0.min(*v),
                    range.1.max(*v),
                    range.2 + f64::from(*v),
                    range.3 + 1,
                );
            }
        }
    });
    // The multiband compressor's meters have no names: its bands' gains are what to read.
    let band_gain = |i: usize| {
        let first = audiostrada_dsp::multiband::GAINS;
        (id == "mb_comp" && (first..first + 4).contains(&i)).then(|| format!("B{}", i - first + 1))
    };
    let meters = info
        .meters
        .iter()
        .zip(ranges)
        .enumerate()
        .filter_map(|(i, (name, range))| {
            let name = if name.is_empty() {
                band_gain(i)?
            } else {
                (*name).to_owned()
            };
            Some((name, range))
        })
        .map(|(name, (lo, hi, sum, n))| {
            let mean = (sum / n.max(1) as f64) as f32;
            (name, lo, hi, mean)
        })
        .collect();
    Ok(Run { out, meters })
}

/// Level in dB of the part of `x` from `from` to `to`.
fn level(x: &Stereo, from: usize, to: usize) -> f32 {
    let to = to.min(x.0.len());
    if from >= to {
        return -200.0;
    }
    let e: f64 = x.0[from..to]
        .iter()
        .zip(&x.1[from..to])
        .map(|(l, r)| f64::from(*l) * f64::from(*l) + f64::from(*r) * f64::from(*r))
        .sum();
    (10.0 * (e / (2 * (to - from)) as f64).max(1e-20).log10()) as f32
}

/// Seconds the tail takes to fall 60 dB under the level of the last second of the sound
/// (twice the time to fall 30 dB, which noise floors leave readable).
fn tail_seconds(out: &Stereo, end: usize) -> f32 {
    let rate = RATE as usize;
    let reference = level(out, end.saturating_sub(rate), end);
    let window = rate / 20;
    let mut at = end;
    while at + window <= out.0.len() {
        if level(out, at, at + window) < reference - 30.0 {
            return 2.0 * (at - end) as f32 / rate as f32;
        }
        at += window;
    }
    2.0 * (out.0.len() - end) as f32 / rate as f32
}

/// What a preset does to a steady tone.
struct Probe {
    /// Level of the tone at the output against the input, dB.
    gain: f32,
    /// Swing of the level, dB, and its rate, Hz.
    depth: f32,
    rate: f32,
    /// Swing of left against right, dB.
    pan: f32,
    /// Swing of the pitch, cents, and its mean shift.
    wobble: f32,
    shift: f32,
    /// Harmonics 2 to 8 against the fundamental, dB.
    thd: f32,
}

const TONE: f32 = 440.0;

fn tone(seconds: usize) -> Stereo {
    let x: Vec<f32> = (0..RATE as usize * seconds)
        .map(|i| 0.25 * (std::f32::consts::TAU * TONE * i as f32 / RATE as f32).sin())
        .collect();
    (x.clone(), x)
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn swing(values: &[f32]) -> f32 {
    let mut v = values.to_vec();
    v.sort_by(f32::total_cmp);
    percentile(&v, 0.98) - percentile(&v, 0.02)
}

/// Amplitude of `x` at `hz` (Goertzel over the whole of it).
fn amplitude(x: &[f32], hz: f32) -> f32 {
    let w = std::f64::consts::TAU * f64::from(hz) / f64::from(RATE);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, v) in x.iter().enumerate() {
        // A Hann window keeps a modulated tone's sidebands out of its harmonics.
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
        let v = f64::from(*v) * window;
        re += v * (w * i as f64).cos();
        im += v * (w * i as f64).sin();
    }
    (4.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn probe(input: &Stereo, out: &Stereo) -> Probe {
    let rate = RATE as usize;
    let (from, to) = (rate, input.0.len().min(out.0.len()));
    let gain = level(out, from, to) - level(input, from, to);
    let (window, hop) = (rate / 100, rate / 400);
    let (mut levels, mut pans) = (Vec::new(), Vec::new());
    let mut at = from;
    while at + window <= to {
        let e = |x: &[f32]| {
            let s: f32 = x[at..at + window].iter().map(|v| v * v).sum();
            10.0 * (s / window as f32).max(1e-12).log10()
        };
        let (l, r) = (e(&out.0), e(&out.1));
        levels.push(10.0 * ((10f32.powf(l / 10.0) + 10f32.powf(r / 10.0)) / 2.0).log10());
        pans.push(l - r);
        at += hop;
    }
    let depth = swing(&levels);
    // The rate of the swing: the first peak of the envelope's autocorrelation.
    let mean = levels.iter().sum::<f32>() / levels.len().max(1) as f32;
    let centered: Vec<f32> = levels.iter().map(|v| v - mean).collect();
    let energy: f32 = centered.iter().map(|v| v * v).sum();
    let mut mod_rate = 0.0;
    if depth > 0.5 && energy > 0.0 {
        let corr = |lag: usize| -> f32 {
            centered[lag..]
                .iter()
                .zip(&centered)
                .map(|(a, b)| a * b)
                .sum::<f32>()
                / energy
        };
        let max_lag = centered.len() / 2;
        let mut lag = 2;
        // Past the first dip, the highest point.
        while lag < max_lag && corr(lag) > 0.0 {
            lag += 1;
        }
        let mut best = (0.0f32, 0usize);
        for k in lag..max_lag {
            let c = corr(k);
            if c > best.0 {
                best = (c, k);
            }
        }
        if best.0 > 0.3 {
            mod_rate = rate as f32 / (hop * best.1) as f32;
        }
    }
    // Pitch from the upward zero crossings of the left channel, 25 ms at a time.
    let x = &out.0;
    let mut crossings = Vec::new();
    for i in from + 1..to {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            crossings.push((i - 1) as f32 + x[i - 1] / (x[i - 1] - x[i]));
        }
    }
    let mut cents = Vec::new();
    let span = 11;
    for pair in crossings.windows(span + 1).step_by(span) {
        let hz = span as f32 * rate as f32 / (pair[span] - pair[0]);
        if hz > 20.0 {
            cents.push(1200.0 * (hz / TONE).log2());
        }
    }
    let mut sorted = cents.clone();
    sorted.sort_by(f32::total_cmp);
    let shift = percentile(&sorted, 0.5);
    let wobble = swing(&cents);
    let fundamental = amplitude(&x[from..to], TONE).max(1e-9);
    let harmonics: f32 = (2..=8)
        .map(|k| amplitude(&x[from..to], TONE * k as f32).powi(2))
        .sum();
    let thd = 20.0 * (harmonics.sqrt() / fundamental).max(1e-6).log10();
    Probe {
        gain,
        depth,
        rate: mod_rate,
        pan: swing(&pans),
        wobble,
        shift,
        thd,
    }
}

/// Third-octave bands shown, Hz.
const SHOWN: [f32; 10] = [
    50.0, 100.0, 200.0, 400.0, 800.0, 1_600.0, 3_150.0, 6_300.0, 10_000.0, 16_000.0,
];

fn band(report: &Report, hz: f32) -> f32 {
    report
        .third_octaves
        .iter()
        .min_by(|a, b| (a.0 / hz).ln().abs().total_cmp(&(b.0 / hz).ln().abs()))
        .map_or(-200.0, |b| b.1)
}

fn header() {
    println!(
        "{:<38} {:<9} {:>6} {:>6} {:>6} {:>5} {:>5} {:>5} | {} | {:>5} | tone: {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} | meters",
        "preset",
        "source",
        "LUFS",
        "dLU",
        "dBTP",
        "dCrst",
        "corr",
        "S/M",
        SHOWN
            .iter()
            .map(|f| if *f >= 1000.0 {
                format!("{:>5}", format!("{}k", f / 1000.0))
            } else {
                format!("{f:>5}")
            })
            .collect::<Vec<_>>()
            .join(" "),
        "tail",
        "gain",
        "depth",
        "Hz",
        "pan",
        "wobbl",
        "shift",
        "THD"
    );
}

fn row(name: &str, source: &str, input: &Stereo, run: &Run, tone_in: &Stereo, tone_out: &Stereo) {
    let n = input.0.len();
    let dry = report(&input.0, &input.1, RATE);
    let wet = report(&run.out.0[..n], &run.out.1[..n], RATE);
    let lufs = |r: &Report| r.integrated.unwrap_or(-99.0);
    let bands: Vec<String> = SHOWN
        .iter()
        .map(|&hz| {
            // The bands are shares of the whole: the change of level brings them back to
            // what the preset did at each frequency.
            let (a, b) = (band(&dry, hz), band(&wet, hz) + wet.rms - dry.rms);
            // Nothing to compare where the sound has nothing.
            if a < -85.0 && b < -85.0 {
                "    .".to_owned()
            } else {
                format!("{:>+5.1}", (b - a).clamp(-99.0, 99.0))
            }
        })
        .collect();
    let p = probe(tone_in, tone_out);
    let meters: Vec<String> = run
        .meters
        .iter()
        .map(|(name, lo, hi, mean)| format!("{name} {lo:.1}..{hi:.1}~{mean:.1}"))
        .collect();
    println!(
        "{:<38} {:<9} {:>6.1} {:>+6.1} {:>6.1} {:>+5.1} {:>5.2} {:>5.0} | {} | {:>5.2} | tone: {:>+5.1} {:>5.1} {:>5.2} {:>5.1} {:>5.0} {:>+5.0} {:>5.0} | {}",
        name.chars().take(38).collect::<String>(),
        source,
        lufs(&wet),
        lufs(&wet) - lufs(&dry),
        wet.true_peak,
        wet.crest - dry.crest,
        wet.correlation.unwrap_or(0.0),
        wet.side_to_mid.unwrap_or(-99.0).max(-99.0),
        bands.join(" "),
        tail_seconds(&run.out, n),
        p.gain,
        p.depth,
        p.rate,
        p.pan,
        p.wobble,
        p.shift,
        p.thd,
        meters.join(", ")
    );
}

fn write(folder: &Path, name: &str, sound: &Stereo) -> anyhow::Result<()> {
    std::fs::create_dir_all(folder).with_context(|| folder.display().to_string())?;
    let file: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let data = AudioData::new(vec![sound.0.clone(), sound.1.clone()], RATE);
    let path = folder.join(format!("{file}.wav"));
    audiostrada_media::write_wav(&path, &data).with_context(|| path.display().to_string())
}

pub fn run(args: &Args) -> anyhow::Result<()> {
    if args.dump {
        let all: BTreeMap<&str, Presets> = audiostrada_dsp::EFFECTS
            .iter()
            .filter(|info| args.effects.is_empty() || args.effects.iter().any(|e| e == info.id))
            .map(|info| (info.id, audiostrada_ui::factory_presets(info.id)))
            .collect();
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    let mut sources = Sources {
        folder: args.audio.clone(),
        loaded: BTreeMap::new(),
    };
    let tail = RATE as usize * TAIL_SECONDS;
    let tone_in = tone(4);
    if let Some(query) = &args.plugin {
        let source = args.source.as_deref().unwrap_or("mix");
        let input = sources.get(source)?.clone();
        header();
        let out = run_plugin(query, &args.set, &input, tail)?;
        let tone_out = run_plugin(query, &args.set, &tone_in, 0)?;
        let run = Run {
            out,
            meters: Vec::new(),
        };
        row(query, source, &input, &run, &tone_in, &tone_out);
        if let Some(folder) = &args.write {
            write(folder, &format!("{query} {source}"), &run.out)?;
        }
        return Ok(());
    }
    let kick = sources.get("kick").ok().cloned();
    let voice = sources.get("vocal").ok().cloned();
    let given: Option<BTreeMap<String, Presets>> = match &args.presets {
        Some(path) => {
            let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
            Some(serde_json::from_str(&text).with_context(|| path.display().to_string())?)
        }
        None => None,
    };
    for info in audiostrada_dsp::EFFECTS {
        if info.instrument
            || !(args.effects.is_empty() || args.effects.iter().any(|e| e == info.id))
        {
            continue;
        }
        let mut presets = match &given {
            Some(given) => given.get(info.id).cloned().unwrap_or_default(),
            None => audiostrada_ui::factory_presets(info.id),
        };
        if !args.set.is_empty() {
            let values = args
                .set
                .iter()
                .map(|(k, v)| (k.clone(), *v as f32))
                .collect();
            presets = vec![("(given settings)".to_owned(), values)];
        }
        if presets.is_empty() {
            continue;
        }
        println!("\n== {} ({})", info.name, info.id);
        header();
        for (name, values) in presets {
            if args
                .only
                .as_ref()
                .is_some_and(|only| !name.to_lowercase().contains(&only.to_lowercase()))
            {
                continue;
            }
            for key in values.keys() {
                if !info.params.iter().any(|p| p.id == key) {
                    bail!("{}: {name} sets '{key}', which is no parameter", info.id);
                }
            }
            let source = args
                .source
                .clone()
                .unwrap_or_else(|| source_of(info.id, &name).to_owned());
            let input = match sources.get(&source) {
                Ok(input) => input.clone(),
                Err(_) => sources.get("mix")?.clone(),
            };
            // A preset listening to another track hears the kick, or the voice it names.
            let lower = name.to_lowercase();
            let ducker = info.id == "gate" && values.get("mode").is_some_and(|m| *m >= 1.5);
            let keyed = ducker || lower.contains("sidechain");
            let key = if !keyed {
                None
            } else if lower.contains("voice") || lower.contains("vocal") {
                voice.as_ref()
            } else {
                kick.as_ref()
            };
            let run = run_effect(info.id, info, &values, &input, key, tail)?;
            let tone_out = run_effect(info.id, info, &values, &tone_in, None, 0)?.out;
            row(&name, &source, &input, &run, &tone_in, &tone_out);
            if let Some(folder) = &args.write {
                write(&folder.join(info.id), &name, &run.out)?;
            }
        }
    }
    Ok(())
}

/// A plugin set by `set`, played on `input`.
fn run_plugin(
    query: &str,
    set: &[(String, f64)],
    input: &Stereo,
    tail: usize,
) -> anyhow::Result<Stereo> {
    let mut instance = crate::measure::load_plugin(query)?;
    let params = instance.params();
    let mut pending = Vec::new();
    for (key, value) in set {
        let p = params
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(key) || p.id.to_string() == *key)
            .ok_or_else(|| anyhow!("{query} has no parameter '{key}'"))?;
        instance.set_param(p.id, *value);
        pending.push((p.id, *value));
    }
    let mut processor = instance.activate(RATE, BLOCK).map_err(|e| anyhow!(e))?;
    processor.set_offline(true);
    let latency = instance.latency() as usize;
    let out = play(input, latency, tail, &mut |l, r, _| {
        processor.process(l, r, &pending, &[], &mut |_, _| {});
        pending.clear();
    });
    drop(processor);
    let _ = instance.try_deactivate();
    Ok(out)
}
