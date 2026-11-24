//! `audiostrada measure-instrument`: plays test notes through an instrument (a built-in one, or
//! the track of a project holding a plugin set up by hand) and keeps what they sound like as
//! figures: pitch, partials and their decay, loudness and brightness by velocity, attack,
//! release, the pitch's first moments. `measure-instrument-report` compares the fingerprints.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use audiostrada_core::Note;
use audiostrada_core::routing::{self, Channel};
use audiostrada_io::ChannelBuffers;
use audiostrada_media::{AudioData, write_wav};
use audiostrada_ui::document::{AppDirs, Document};
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

use crate::measure::{COLORS, Series, chart, slug};

const RATE: u32 = 48_000;
const BLOCK: usize = 1024;
/// Silence after each note's release, for its tail and the next note's clean start.
const GAP: f64 = 2.0;
/// The phrase for listening side by side: four chords of two seconds, then the tail.
const PHRASE: [[u8; 4]; 4] = [
    [48, 55, 64, 71],
    [45, 52, 60, 67],
    [50, 57, 65, 72],
    [43, 53, 59, 65],
];
const PHRASE_CHORD: f64 = 2.0;
const PHRASE_TAIL: f64 = 3.0;
/// The phrase's level once written, RMS in dBFS.
const PHRASE_DB: f32 = -20.0;

/// What `measure-instrument` plays and where it writes.
#[derive(Debug, PartialEq)]
pub struct Args {
    /// Projects (`.strada`) or built-in instruments, by id.
    pub sources: Vec<String>,
    pub out: PathBuf,
    /// Where to write what was played, to listen to it (never kept with the figures).
    pub wav: Option<PathBuf>,
    pub track: Option<String>,
    /// The fingerprint's name, in place of the source's (one source only).
    pub name: Option<String>,
    pub keys: Vec<u8>,
    pub velocities: Vec<u8>,
    /// How long each note is held, in seconds.
    pub hold: f64,
    /// Settings of a built-in instrument, by parameter id.
    pub set: Vec<(String, f32)>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            out: "instruments".into(),
            wav: None,
            track: None,
            name: None,
            keys: (28..=100).step_by(7).collect(),
            velocities: vec![20, 45, 70, 95, 120],
            hold: 4.0,
            set: Vec::new(),
        }
    }
}

/// One partial of a note: where it is, how loud against the fundamental, how long it rings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Partial {
    pub n: u32,
    pub hz: f32,
    /// Its frequency over n times the fundamental's: above 1, the partial is sharp.
    pub ratio: f32,
    /// In dB against the fundamental, at the start of the note.
    pub level_db: f32,
    /// Seconds to fall 60 dB while held; `None` when it does not fall.
    pub t60: Option<f32>,
}

/// What one note sounds like.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NotePrint {
    pub key: u8,
    pub velocity: u8,
    pub f0: f32,
    /// Against equal temperament at A4 = 440 Hz.
    pub cents: f32,
    /// RMS of the first half second, and the highest peak, in dBFS.
    pub loudness_db: f32,
    pub peak_db: f32,
    /// From the start to the loudest moment of the first 300 ms.
    pub attack_ms: f32,
    /// The spectral centroid of the first 300 ms.
    pub centroid_hz: f32,
    /// Seconds for the whole sound to fall 60 dB while held.
    pub t60: Option<f32>,
    /// From the release until 40 dB down; `None` when it is still sounding at the next note.
    pub release_ms: Option<f32>,
    pub partials: Vec<Partial>,
    /// The pitch over the first 400 ms, in cents from the note's: (ms, cents).
    pub pitch: Vec<(f32, f32)>,
}

/// The fingerprint of one instrument.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstrumentPrint {
    pub name: String,
    pub rate: u32,
    pub hold: f32,
    pub notes: Vec<NotePrint>,
}

pub fn run(args: &Args) -> Result<()> {
    std::fs::create_dir_all(&args.out).with_context(|| args.out.display().to_string())?;
    if let Some(wav) = &args.wav {
        std::fs::create_dir_all(wav).with_context(|| wav.display().to_string())?;
    }
    for source in &args.sources {
        let name = args.name.clone().unwrap_or_else(|| {
            Path::new(source)
                .file_stem()
                .map_or_else(|| source.clone(), |s| s.to_string_lossy().into_owned())
        });
        let (print, audio, phrase) = measure_source(source, &name, args)?;
        let path = args.out.join(format!("{}.json", slug(&name)));
        std::fs::write(&path, serde_json::to_string_pretty(&print)?)
            .with_context(|| path.display().to_string())?;
        if let Some(dir) = &args.wav {
            let notes = dir.join(format!("{}-notes.wav", slug(&name)));
            write_wav(&notes, &audio)?;
            let listen = dir.join(format!("{}-phrase.wav", slug(&name)));
            write_wav(&listen, &phrase)?;
        }
        summary(&print, &path);
    }
    Ok(())
}

/// Plays the notes and the phrase through `source`, then measures the notes.
fn measure_source(
    source: &str,
    name: &str,
    args: &Args,
) -> Result<(InstrumentPrint, AudioData, AudioData)> {
    let mut doc = open(source, args)?;
    let rate = doc.project().sample_rate.max(1);
    let at = |s: f64| (s * f64::from(rate)) as u64;
    let slot = args.hold + GAP;
    let mut notes = Vec::new();
    let mut starts = Vec::new();
    for &key in &args.keys {
        for &velocity in &args.velocities {
            let start = starts.len() as f64 * slot;
            starts.push((key, velocity, start));
            notes.push(note(at(start), at(args.hold), key, velocity));
        }
    }
    let phrase_at = starts.len() as f64 * slot;
    for (k, chord) in PHRASE.iter().enumerate() {
        let start = phrase_at + k as f64 * PHRASE_CHORD;
        for &key in chord {
            notes.push(note(at(start), at(PHRASE_CHORD * 0.95), key, 90));
        }
    }
    let length = phrase_at + PHRASE.len() as f64 * PHRASE_CHORD + PHRASE_TAIL;
    let track = target(&doc, args.track.as_deref())?;
    doc.history_mut().edit("Measure", |p| {
        for t in &mut p.tracks {
            let t = Arc::make_mut(t);
            if t.id == track {
                t.clips.clear();
                t.mute = false;
            } else {
                t.mute = true;
            }
        }
        p.add_midi_clip(track, 0, at(length), notes);
    });
    let audio = render(&mut doc, length)?;
    let mono: Vec<f32> = audio
        .channel(0)
        .iter()
        .zip(audio.channel(audio.channels().min(2) - 1))
        .map(|(l, r)| 0.5 * (l + r))
        .collect();
    let rate = audio.sample_rate();
    let print = InstrumentPrint {
        name: name.to_owned(),
        rate,
        hold: args.hold as f32,
        notes: starts
            .iter()
            .map(|&(key, velocity, start)| {
                let from = (start * f64::from(rate)) as usize;
                let to = (from + (slot * f64::from(rate)) as usize).min(mono.len());
                measure_note(
                    &mono[from.min(to)..to],
                    rate,
                    key,
                    velocity,
                    args.hold as f32,
                )
            })
            .collect(),
    };
    let from = (phrase_at * f64::from(rate)) as usize;
    let phrase = level(&audio, from, PHRASE_DB);
    Ok((print, audio, phrase))
}

fn note(start: u64, length: u64, key: u8, velocity: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity,
        ..Default::default()
    }
}

/// A project to play: the file given, or a new one with the built-in instrument `source`.
fn open(source: &str, args: &Args) -> Result<Document> {
    let plugins = crate::audio::plugin_database(&crate::prefs::Prefs::load().ui, false);
    if crate::audio::is_project(Path::new(source)) {
        return crate::audio::load_document(&[PathBuf::from(source)], AppDirs::default(), plugins);
    }
    let info = audiostrada_dsp::info(source)
        .filter(|i| i.instrument)
        .with_context(|| format!("'{source}' is neither a project nor a built-in instrument"))?;
    let mut doc = Document::new(AppDirs::default());
    doc.set_plugins(plugins);
    let params: BTreeMap<String, f32> = args.set.iter().cloned().collect();
    for id in params.keys() {
        if !info.params.iter().any(|p| p.id == id.as_str()) {
            bail!("{} has no parameter '{id}'", info.name);
        }
    }
    doc.history_mut().edit("Measure", |p| {
        let t = p.add_track(info.name);
        routing::add_insert(p, Channel::Track(t), info.id, params);
    });
    Ok(doc)
}

/// The track that plays: the one named, else the first holding an instrument.
fn target(doc: &Document, named: Option<&str>) -> Result<audiostrada_core::TrackId> {
    let tracks = &doc.project().tracks;
    if let Some(name) = named {
        return tracks
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.id)
            .with_context(|| format!("no track named '{name}'"));
    }
    let instrument = |effect: &str| {
        effect.contains(':') || audiostrada_dsp::info(effect).is_some_and(|i| i.instrument)
    };
    tracks
        .iter()
        .find(|t| t.inserts.iter().any(|i| instrument(&i.effect)))
        .map(|t| t.id)
        .context("no track with an instrument: name it with --track")
}

fn render(doc: &mut Document, seconds: f64) -> Result<AudioData> {
    let mut processor = doc.processor(RATE, BLOCK);
    if !doc.wait_loaded(std::time::Duration::from_secs(120)) {
        bail!("timed out preparing the project");
    }
    for message in doc.take_messages() {
        println!("{message}");
    }
    doc.player_mut().engine_mut().play();
    let length = (seconds * f64::from(RATE)) as usize;
    let mut storage = ChannelBuffers::new(0, 2, BLOCK);
    let mut channels: Vec<Vec<f32>> = (0..2).map(|_| Vec::with_capacity(length)).collect();
    while channels[0].len() < length {
        let frames = (length - channels[0].len()).min(BLOCK);
        processor.render(&mut storage.buffers(frames));
        for (ch, channel) in channels.iter_mut().enumerate() {
            channel.extend_from_slice(&storage.output(ch)[..frames]);
        }
    }
    Ok(AudioData::new(channels, RATE))
}

/// The audio from `from` on, brought to `db` RMS.
fn level(audio: &AudioData, from: usize, db: f32) -> AudioData {
    let channels: Vec<Vec<f32>> = (0..audio.channels())
        .map(|c| audio.channel(c)[from.min(audio.frames())..].to_vec())
        .collect();
    let rms = rms_db(&channels[0]);
    let gain = if rms > -150.0 {
        10f32.powf((db - rms) / 20.0)
    } else {
        1.0
    };
    let channels = channels
        .into_iter()
        .map(|c| c.into_iter().map(|s| s * gain).collect())
        .collect();
    AudioData::new(channels, audio.sample_rate())
}

fn summary(print: &InstrumentPrint, path: &Path) {
    let loud = print
        .notes
        .iter()
        .map(|n| n.loudness_db)
        .fold(f32::MIN, f32::max);
    let off = print
        .notes
        .iter()
        .map(|n| n.cents.abs())
        .fold(0.0f32, f32::max);
    println!(
        "{:<32} {} notes, loudest {loud:.1} dBFS, tuning within {off:.1} cents → {}",
        print.name,
        print.notes.len(),
        path.display()
    );
}

// The measures of one note.

fn rms_db(x: &[f32]) -> f32 {
    let mean = x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32;
    10.0 * mean.max(1e-30).log10()
}

/// Equal temperament at A4 = 440 Hz.
fn equal(key: u8) -> f32 {
    440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0)
}

/// A magnitude spectrum in dB (amplitude of a sine), finely spaced by zero padding.
struct Spectrum {
    db: Vec<f32>,
    hz_per_bin: f32,
}

impl Spectrum {
    fn of(x: &[f32], rate: u32) -> Self {
        let n = x.len().max(2);
        let size = (n * 4)
            .next_power_of_two()
            .min(1 << 18)
            .max(n.next_power_of_two());
        let mut input = vec![0.0f32; size];
        let mut sum = 0.0;
        for (i, (o, s)) in input.iter_mut().zip(x).enumerate() {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
            *o = s * w;
            sum += w;
        }
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        let mut out = fft.make_output_vec();
        let _ = fft.process(&mut input, &mut out);
        let db = out
            .iter()
            .map(|c| 20.0 * (2.0 * c.norm() / sum.max(1e-9)).max(1e-12).log10())
            .collect();
        Self {
            db,
            hz_per_bin: rate as f32 / size as f32,
        }
    }

    /// The highest peak between `lo` and `hi` Hz, placed between bins: (Hz, dB).
    fn peak(&self, lo: f32, hi: f32) -> Option<(f32, f32)> {
        let a = ((lo / self.hz_per_bin).floor() as usize).max(1);
        let b = ((hi / self.hz_per_bin).ceil() as usize).min(self.db.len() - 2);
        let k = (a..=b).max_by(|&i, &j| self.db[i].total_cmp(&self.db[j]))?;
        let (l, c, r) = (self.db[k - 1], self.db[k], self.db[k + 1]);
        let d = l - 2.0 * c + r;
        let shift = if d.abs() > 1e-9 {
            (0.5 * (l - r) / d).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        Some((
            (k as f32 + shift) * self.hz_per_bin,
            c - 0.25 * (l - r) * shift,
        ))
    }

    fn centroid(&self) -> f32 {
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (k, db) in self.db.iter().enumerate() {
            let power = 10f64.powf(f64::from(*db) / 10.0);
            num += power * k as f64;
            den += power;
        }
        (num / den.max(1e-30)) as f32 * self.hz_per_bin
    }
}

fn window(x: &[f32], rate: u32, from: f32, to: f32) -> &[f32] {
    let a = ((from * rate as f32) as usize).min(x.len());
    let b = ((to * rate as f32) as usize).clamp(a, x.len());
    &x[a..b]
}

/// Seconds to fall 60 dB from levels in dB at times in seconds (a straight line through
/// them), counting only levels above `floor`; `None` when they do not fall.
fn t60(points: &[(f32, f32)], floor: f32) -> Option<f32> {
    let pts: Vec<(f32, f32)> = points.iter().copied().filter(|p| p.1 > floor).collect();
    if pts.len() < 2 {
        return None;
    }
    let n = pts.len() as f32;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (mut num, mut den) = (0.0, 0.0);
    for (x, y) in &pts {
        num += (x - mx) * (y - my);
        den += (x - mx) * (x - mx);
    }
    let slope = num / den.max(1e-9);
    (slope < -0.5).then(|| -60.0 / slope)
}

/// Times at which a held note's decay is read, in seconds.
const DECAY_AT: [f32; 6] = [0.2, 0.5, 1.0, 1.6, 2.4, 3.4];
const DECAY_WINDOW: f32 = 0.2;

pub fn measure_note(x: &[f32], rate: u32, key: u8, velocity: u8, hold: f32) -> NotePrint {
    let want = equal(key);
    let decay_at: Vec<f32> = DECAY_AT
        .iter()
        .copied()
        .filter(|t| t + DECAY_WINDOW <= hold)
        .collect();
    // The fundamental over half a second once the strike has passed.
    let steady = Spectrum::of(window(x, rate, 0.15, (0.15 + 0.5f32).min(hold)), rate);
    let f0 = steady
        .peak(want * 2f32.powf(-0.6 / 12.0), want * 2f32.powf(0.6 / 12.0))
        .map_or(want, |p| p.0);
    let start = Spectrum::of(window(x, rate, 0.03, 0.23), rate);
    let decays: Vec<Spectrum> = decay_at
        .iter()
        .map(|t| Spectrum::of(window(x, rate, *t, t + DECAY_WINDOW), rate))
        .collect();
    let mut partials = Vec::new();
    let fundamental = start.peak(f0 * 0.98, f0 * 1.02).map_or(-200.0, |p| p.1);
    // Each partial looked for where the stiffness so far puts it.
    let mut b = 0.0f32;
    for n in 1..=16u32 {
        let nf = n as f32;
        let guess = nf * f0 * (1.0 + b * nf * nf).sqrt();
        if guess > (0.45 * rate as f32).min(12_000.0) {
            break;
        }
        // Where it is, once the strike's pitch has settled; how loud, at the strike.
        let tolerance = 0.025 * guess;
        let Some((hz, _)) = steady.peak(guess - tolerance, guess + tolerance) else {
            break;
        };
        let db = start.peak(hz * 0.985, hz * 1.015).map_or(-200.0, |p| p.1);
        let ratio = hz / (nf * f0);
        if n > 1 && ratio > 1.0 && db - fundamental > -60.0 {
            b = (ratio * ratio - 1.0) / (nf * nf);
        }
        let decay: Vec<(f32, f32)> = decay_at
            .iter()
            .zip(&decays)
            .filter_map(|(t, s)| {
                s.peak(hz * 0.99, hz * 1.01)
                    .map(|p| (t + 0.5 * DECAY_WINDOW, p.1))
            })
            .collect();
        partials.push(Partial {
            n,
            hz,
            ratio,
            level_db: db - fundamental,
            t60: t60(&decay, -110.0),
        });
    }
    // The level every 10 ms.
    let step = (rate / 100) as usize;
    let envelope: Vec<f32> = x.chunks(step).map(rms_db).collect();
    let first = &envelope[..envelope.len().min(30)];
    let attack_ms = first
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0.0, |(i, _)| (i as f32 + 0.5) * 10.0);
    let broadband: Vec<(f32, f32)> = decay_at
        .iter()
        .map(|t| {
            (
                t + 0.5 * DECAY_WINDOW,
                rms_db(window(x, rate, *t, t + DECAY_WINDOW)),
            )
        })
        .collect();
    let release_ms = {
        let off = (hold * 100.0) as usize;
        let at_off = envelope
            .get(off.saturating_sub(1))
            .copied()
            .unwrap_or(-200.0);
        envelope
            .iter()
            .skip(off)
            .position(|l| *l < at_off - 40.0)
            .map(|i| i as f32 * 10.0)
    };
    NotePrint {
        key,
        velocity,
        f0,
        cents: 1200.0 * (f0 / want).log2(),
        loudness_db: rms_db(window(x, rate, 0.0, 0.5)),
        peak_db: 20.0
            * x.iter()
                .fold(0.0f32, |m, s| m.max(s.abs()))
                .max(1e-10)
                .log10(),
        attack_ms,
        centroid_hz: Spectrum::of(window(x, rate, 0.0, 0.3), rate).centroid(),
        t60: t60(&broadband, -100.0),
        release_ms,
        partials,
        pitch: pitch_curve(x, rate, f0),
    }
}

/// The pitch over the first 400 ms, from the fundamental's cycles: the sound low-passed just
/// above the note, every rising zero crossing timed.
fn pitch_curve(x: &[f32], rate: u32, f0: f32) -> Vec<(f32, f32)> {
    let span = window(x, rate, 0.0, 0.4);
    // Four poles just above the note: the harmonics, even louder than it, fall well below.
    let a = 1.0 - (-std::f32::consts::TAU * 1.2 * f0 / rate as f32).exp();
    let mut stages = [0.0f32; 4];
    let low: Vec<f32> = span
        .iter()
        .map(|v| {
            let mut x = *v;
            for s in &mut stages {
                *s += a * (x - *s);
                x = *s;
            }
            x
        })
        .collect();
    let crossings: Vec<f32> = low
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
        .map(|(i, w)| i as f32 + w[0] / (w[0] - w[1]))
        .collect();
    let mut curve = Vec::new();
    let mut next = 0.0;
    for w in crossings.windows(2) {
        let t = w[1] / rate as f32 * 1000.0;
        if t < next {
            continue;
        }
        let hz = rate as f32 / (w[1] - w[0]);
        curve.push((t, 1200.0 * (hz / f0).log2()));
        next = t + 10.0;
    }
    curve
}

// The comparison page.

pub fn report(files: &[PathBuf], output: &Path) -> Result<()> {
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
        .map(|p| {
            let text = std::fs::read_to_string(p).with_context(|| p.display().to_string())?;
            serde_json::from_str::<InstrumentPrint>(&text).with_context(|| p.display().to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    std::fs::write(output, page(&prints)).with_context(|| output.display().to_string())?;
    println!(
        "{} instrument(s) compared in {}",
        prints.len(),
        output.display()
    );
    Ok(())
}

/// The velocity shown when one is chosen: the one closest to a firm touch.
fn firm(print: &InstrumentPrint) -> u8 {
    let mut velocities: Vec<u8> = print.notes.iter().map(|n| n.velocity).collect();
    velocities.sort_unstable();
    velocities.dedup();
    velocities
        .into_iter()
        .min_by_key(|v| (i32::from(*v) - 95).abs())
        .unwrap_or(95)
}

fn page(prints: &[InstrumentPrint]) -> String {
    let by_key = |f: &dyn Fn(&NotePrint) -> Option<f32>| -> Vec<Series> {
        prints
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let v = firm(p);
                Series {
                    name: &p.name,
                    color: COLORS[i % COLORS.len()],
                    points: p
                        .notes
                        .iter()
                        .filter(|n| n.velocity == v)
                        .map(|n| (f32::from(n.key), f(n).unwrap_or(-200.0)))
                        .collect(),
                }
            })
            .collect()
    };
    let by_velocity = |key: u8, f: &dyn Fn(&NotePrint) -> f32| -> Vec<Series> {
        prints
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let near = p
                    .notes
                    .iter()
                    .min_by_key(|n| (i32::from(n.key) - i32::from(key)).abs())
                    .map_or(key, |n| n.key);
                Series {
                    name: &p.name,
                    color: COLORS[i % COLORS.len()],
                    points: p
                        .notes
                        .iter()
                        .filter(|n| n.key == near)
                        .map(|n| (f32::from(n.velocity), f(n)))
                        .collect(),
                }
            })
            .collect()
    };
    let partial = |n: u32, f: fn(&Partial) -> Option<f32>| {
        move |note: &NotePrint| note.partials.iter().find(|p| p.n == n).and_then(f)
    };
    let mut charts = vec![
        chart(
            "Loudness by key (firm touch)",
            "key",
            "dBFS",
            false,
            &by_key(&|n| Some(n.loudness_db)),
        ),
        chart(
            "Loudness by velocity, middle C",
            "velocity",
            "dBFS",
            false,
            &by_velocity(60, &|n| n.loudness_db),
        ),
        chart(
            "Brightness by velocity, middle C (spectral centroid, octaves above 100 Hz)",
            "velocity",
            "octaves × 6",
            false,
            &by_velocity(60, &|n| 6.0 * (n.centroid_hz / 100.0).max(1e-3).log2()),
        ),
        chart(
            "Decay of the whole sound by key: seconds to fall 60 dB",
            "key",
            "s",
            false,
            &by_key(&|n| n.t60),
        ),
        chart(
            "Decay of the fundamental by key: seconds to fall 60 dB",
            "key",
            "s",
            false,
            &by_key(&partial(1, |p| p.t60)),
        ),
        chart(
            "Release by key: ms to fall 40 dB once the key is let go",
            "key",
            "ms / 10",
            false,
            &by_key(&|n| n.release_ms.map(|ms| ms / 10.0)),
        ),
        chart(
            "Attack by key: ms to the loudest moment",
            "key",
            "ms",
            false,
            &by_key(&|n| Some(n.attack_ms)),
        ),
        chart(
            "Tuning by key",
            "key",
            "cents",
            false,
            &by_key(&|n| Some(n.cents)),
        ),
    ];
    for n in [2u32, 3, 4, 7] {
        charts.push(chart(
            &format!("Partial {n} by key: level against the fundamental"),
            "key",
            "dB",
            false,
            &by_key(&partial(n, |p| Some(p.level_db))),
        ));
    }
    for n in [4u32, 7, 12] {
        charts.push(chart(
            &format!("Partial {n} by key: cents sharp of n × the fundamental"),
            "key",
            "cents",
            false,
            &by_key(&partial(n, |p| Some(1200.0 * p.ratio.max(1e-6).log2()))),
        ));
    }
    let pitch: Vec<Series> = prints
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let loud = p
                .notes
                .iter()
                .filter(|n| n.key <= 40)
                .max_by_key(|n| n.velocity);
            Series {
                name: &p.name,
                color: COLORS[i % COLORS.len()],
                points: loud.map(|n| n.pitch.clone()).unwrap_or_default(),
            }
        })
        .collect();
    charts.push(chart(
        "Pitch over the first 400 ms of a low note, hardest touch",
        "ms",
        "cents",
        false,
        &pitch,
    ));
    let mut legend = String::new();
    for (i, p) in prints.iter().enumerate() {
        let _ = write!(
            legend,
            r#"<li><span style="background:{}"></span>{} ({} notes, velocity {} shown by key)</li>"#,
            COLORS[i % COLORS.len()],
            p.name,
            p.notes.len(),
            firm(p)
        );
    }
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Instruments compared</title><style>
body{{font:14px system-ui,sans-serif;margin:24px;color:#1d2125}}
ul{{list-style:none;padding:0}}li{{margin:4px 0}}li span{{display:inline-block;width:14px;height:14px;border-radius:3px;margin-right:8px;vertical-align:-2px}}
.grid{{display:grid;grid-template-columns:repeat(auto-fill,minmax(560px,1fr));gap:16px}}
figure{{margin:0;border:1px solid #e3e6ea;border-radius:8px;padding:8px}}figcaption{{font-weight:600;margin-bottom:4px}}
</style></head><body><h1>Instruments compared</h1><ul>{legend}</ul><div class="grid">{}</div></body></html>"#,
        charts.join("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A struck note: partials `ratios` at `levels` (dB), each falling 60 dB in `t60s`.
    fn struck(f0: f32, partials: &[(f32, f32, f32)], seconds: f32, hold: f32) -> Vec<f32> {
        let rate = RATE as f32;
        (0..(seconds * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let damp = if t > hold {
                    10f32.powf(-3.0 * (t - hold) / 0.1)
                } else {
                    1.0
                };
                partials
                    .iter()
                    .enumerate()
                    .map(|(k, &(ratio, db, t60))| {
                        let n = (k + 1) as f32;
                        let a = 10f32.powf(db / 20.0) * 10f32.powf(-3.0 * t / t60);
                        0.3 * a * (std::f32::consts::TAU * n * ratio * f0 * t).sin()
                    })
                    .sum::<f32>()
                    * damp
            })
            .collect()
    }

    #[test]
    fn a_note_is_measured_as_it_was_made() {
        let f0 = equal(45);
        let x = struck(
            f0,
            &[(1.0, 0.0, 6.0), (1.01, -6.0, 3.0), (1.02, -12.0, 2.0)],
            6.0,
            4.0,
        );
        let n = measure_note(&x, RATE, 45, 100, 4.0);
        assert!(n.cents.abs() < 0.5, "{}", n.cents);
        let p2 = &n.partials[1];
        assert!((p2.ratio - 1.01).abs() < 0.002, "{p2:?}");
        // Read 0.13 s in, where it has fallen 20 dB/s against the fundamental's 10.
        assert!((p2.level_db + 7.3).abs() < 0.5, "{p2:?}");
        assert!((p2.t60.unwrap() - 3.0).abs() < 0.3, "{p2:?}");
        assert!(
            (n.partials[0].t60.unwrap() - 6.0).abs() < 0.6,
            "{:?}",
            n.partials[0]
        );
        let release = n.release_ms.unwrap();
        assert!(release > 40.0 && release < 120.0, "{release}");
    }

    #[test]
    fn a_falling_pitch_shows_in_the_curve() {
        let rate = RATE as f32;
        let f0 = 55.0;
        let mut phase = 0.0f32;
        let x: Vec<f32> = (0..(0.5 * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let hz = f0 * 2f32.powf(12.0 * (-t / 0.05).exp() / 12.0);
                phase += hz / rate;
                (std::f32::consts::TAU * phase).sin() * 0.5
            })
            .collect();
        let curve = pitch_curve(&x, RATE, f0);
        let early = curve.iter().find(|p| p.0 > 15.0).unwrap().1;
        let late = curve.last().unwrap().1;
        assert!(early > 300.0, "{early}");
        assert!(late.abs() < 20.0, "{late}");
    }
}
