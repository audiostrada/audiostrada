//! Reaper's `.rpp` projects: nested `<TAG …>` blocks of text lines. Tracks, folders (as buses),
//! audio and MIDI items, fades, play rate, volume, pan, mute, solo, receives, plugins (CLAP and
//! VST3, without their settings: Reaper keeps them in its own form), volume and pan envelopes,
//! tempo, time signature and markers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use audiostrada_core::automation::AutomationTarget;
use audiostrada_core::file::ProjectPaths;
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::tempo::{TempoMap, TempoPoint, TimeSignature};
use audiostrada_core::{Clip, FadeShape, Marker, Note, Project, TrackId, TrackKind, TrackSend};

use crate::{Build, Warning, db};

#[derive(Debug, Default)]
struct Block {
    tag: String,
    args: Vec<String>,
    lines: Vec<Vec<String>>,
    children: Vec<Block>,
}

impl Block {
    fn line(&self, key: &str) -> Option<&[String]> {
        self.lines
            .iter()
            .find(|l| l.first().is_some_and(|k| k == key))
            .map(|l| &l[1..])
    }

    fn lines_of<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a [String]> + 'a {
        self.lines
            .iter()
            .filter(move |l| l.first().is_some_and(|k| k == key))
            .map(|l| &l[1..])
    }

    fn blocks<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Block> + 'a {
        self.children.iter().filter(move |b| b.tag == tag)
    }

    fn num(&self, key: &str, i: usize) -> Option<f64> {
        self.line(key)?
            .get(i)?
            .parse()
            .ok()
            .filter(|v: &f64| v.is_finite())
    }
}

/// Splits a line into words, quoted ones (`"`, `'`, `` ` ``) kept whole.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut word = String::new();
        if matches!(c, '"' | '\'' | '`') {
            chars.next();
            for ch in chars.by_ref() {
                if ch == c {
                    break;
                }
                word.push(ch);
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                word.push(ch);
                chars.next();
            }
        }
        out.push(word);
    }
    out
}

fn parse(text: &str) -> Option<Block> {
    let mut stack: Vec<Block> = Vec::new();
    let mut root = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('<') {
            let mut w = words(rest);
            let tag = if w.is_empty() {
                String::new()
            } else {
                w.remove(0)
            };
            stack.push(Block {
                tag,
                args: w,
                ..Block::default()
            });
        } else if line == ">" {
            let done = stack.pop()?;
            match stack.last_mut() {
                Some(parent) => parent.children.push(done),
                None => root = Some(done),
            }
        } else if let Some(top) = stack.last_mut()
            && !line.is_empty()
        {
            top.lines.push(words(line));
        }
    }
    while let Some(done) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(done),
            None => root = Some(done),
        }
    }
    root.filter(|r| r.tag == "REAPER_PROJECT")
}

pub(crate) fn import(
    path: &Path,
    paths: &ProjectPaths,
    name: &str,
) -> Result<(Project, Vec<Warning>), String> {
    let text = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&text);
    let root = parse(&text).ok_or("not a Reaper project")?;
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let rate = root
        .num("SAMPLERATE", 0)
        .filter(|r| *r >= 8000.0)
        .map_or(48_000, |r| r as u32);
    let mut build = Build::new(paths, name, rate);

    let bpm = root.num("TEMPO", 0).unwrap_or(120.0);
    let numerator = root.num("TEMPO", 1).map_or(4, |v| v as u32);
    let denominator = root.num("TEMPO", 2).map_or(4, |v| v as u32);
    let mut tempos = Vec::new();
    let mut signatures = Vec::new();
    if let Some(env) = root.blocks("TEMPOENVEX").next() {
        // Points in seconds: their beats follow from the tempos before them.
        let mut beat = 0.0;
        let mut last = (0.0, bpm);
        for p in env.lines_of("PT") {
            let (Some(t), Some(v)) = (
                p.first().and_then(|v| v.parse::<f64>().ok()),
                p.get(1).and_then(|v| v.parse::<f64>().ok()),
            ) else {
                continue;
            };
            beat += (t - last.0) * last.1 / 60.0;
            tempos.push(TempoPoint { beat, bpm: v });
            if let Some(sig) = p
                .get(3)
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|s| *s > 0)
            {
                let (n, d) = (sig & 0xFFFF, sig >> 16);
                if n > 0 && d > 0 {
                    let per_bar = f64::from(numerator) * 4.0 / f64::from(denominator);
                    signatures.push(TimeSignature {
                        bar: (beat / per_bar).round() as u32,
                        numerator: n,
                        denominator: d,
                    });
                }
            }
            last = (t, v);
        }
    }
    tempos.push(TempoPoint { beat: 0.0, bpm });
    signatures.push(TimeSignature {
        bar: 0,
        numerator,
        denominator,
    });
    let tempo = TempoMap::new(tempos, signatures);
    build.project.tempo = tempo.clone();
    let samples = |s: f64| (s.max(0.0) * f64::from(rate)).round() as u64;

    if let Some(v) = root.num("MASTER_VOLUME", 0) {
        build.project.master.gain_db = db(v);
    }

    let mut ids = Vec::new();
    let mut folders: Vec<TrackId> = Vec::new();
    let mut receives = Vec::new();
    for t in root.blocks("TRACK") {
        let name = t
            .line("NAME")
            .and_then(|n| n.first())
            .cloned()
            .unwrap_or_default();
        let id = build
            .project
            .add_track(if name.is_empty() { "Track" } else { &name });
        ids.push(id);
        let folder = t.num("ISBUS", 0).unwrap_or(0.0) as i32;
        let depth = t.num("ISBUS", 1).unwrap_or(0.0) as i32;
        if let Some(track) = build.project.track_mut(id) {
            track.parent = folders.last().copied();
            if let Some(v) = t.num("VOLPAN", 0) {
                track.gain_db = db(v);
            }
            if let Some(p) = t.num("VOLPAN", 1) {
                track.pan = p.clamp(-1.0, 1.0) as f32;
            }
            track.mute = t.num("MUTESOLO", 0).is_some_and(|m| m > 0.0);
            track.solo = t.num("MUTESOLO", 1).is_some_and(|s| s > 0.0);
            track.stereo = t.num("NCHAN", 0).is_none_or(|n| n >= 2.0);
            if let Some(c) = t
                .num("PEAKCOL", 0)
                .map(|c| c as i64)
                .filter(|c| c & 0x0100_0000 != 0)
            {
                track.color = [
                    (c & 0xFF) as u8,
                    ((c >> 8) & 0xFF) as u8,
                    ((c >> 16) & 0xFF) as u8,
                ];
            }
            if folder == 1 {
                track.kind = TrackKind::Bus;
            }
        }
        if folder == 1 {
            folders.push(id);
        } else if folder == 2 {
            for _ in 0..depth.unsigned_abs() {
                folders.pop();
            }
        }
        for r in t.lines_of("AUXRECV") {
            let src = r.first().and_then(|v| v.parse::<usize>().ok());
            let mode = r.get(1).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            let vol = r.get(2).and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
            let mute = r.get(4).is_some_and(|m| m == "1");
            if let Some(src) = src {
                receives.push((src, id, db(vol), mode != 0, mute));
            }
        }
        if let Some(chain) = t.blocks("FXCHAIN").next() {
            fx(&mut build, chain, Channel::Track(id));
        }
        for env in t
            .children
            .iter()
            .filter(|b| b.tag == "VOLENV2" || b.tag == "VOLENV")
        {
            let points = envelope(env, &samples, db);
            build.automate(Some(id), AutomationTarget::Gain, points);
        }
        for env in t
            .children
            .iter()
            .filter(|b| b.tag == "PANENV2" || b.tag == "PANENV")
        {
            let points = envelope(env, &samples, |v| (-v).clamp(-1.0, 1.0) as f32);
            build.automate(Some(id), AutomationTarget::Pan, points);
        }
        for item in t.blocks("ITEM") {
            self::item(&mut build, &dir, item, id, &tempo, rate);
        }
    }
    for (src, to, level, pre, mute) in receives {
        let Some(&from) = ids.get(src) else {
            continue;
        };
        if let Some(t) = build.project.track_mut(to)
            && t.clips.is_empty()
        {
            t.kind = TrackKind::Bus;
        }
        if let Some(t) = build.project.track_mut(from) {
            t.sends.push(TrackSend {
                target: to,
                gain_db: level,
                pre_fader: pre,
                mute,
            });
        }
    }
    if let Some(chain) = root.blocks("MASTERFXLIST").next() {
        fx(&mut build, chain, Channel::Master);
    }

    for m in root.lines_of("MARKER") {
        let (Some(t), Some(name)) = (m.get(1).and_then(|v| v.parse::<f64>().ok()), m.get(2)) else {
            continue;
        };
        // A region is two lines, its start named, its end not.
        if name.is_empty() && m.get(3).is_some_and(|f| f == "1") {
            continue;
        }
        let id = build.project.new_marker_id();
        build.project.markers.push(Marker {
            id,
            position: samples(t),
            name: name.clone(),
        });
    }
    Ok((build.project, build.warnings))
}

/// Envelope points (`PT time value shape`), values converted by `value`.
fn envelope(
    env: &Block,
    samples: &dyn Fn(f64) -> u64,
    value: impl Fn(f64) -> f32,
) -> Vec<(u64, f32, bool)> {
    if env.num("ACT", 0) == Some(0.0) {
        return Vec::new();
    }
    env.lines_of("PT")
        .filter_map(|p| {
            let t: f64 = p.first()?.parse().ok()?;
            let v: f64 = p.get(1)?.parse().ok()?;
            let step = p.get(2).is_some_and(|s| s == "1");
            Some((samples(t), value(v), step))
        })
        .collect()
}

/// The plugins of an FX chain; Reaper's own effects and JS scripts stay out.
fn fx(build: &mut Build, chain: &Block, channel: Channel) {
    let bypassed: Vec<bool> = chain
        .lines_of("BYPASS")
        .map(|b| b.first().is_some_and(|v| v == "1"))
        .collect();
    for (index, b) in chain.children.iter().enumerate() {
        let bypass = bypassed.get(index).copied().unwrap_or(false);
        let label = b.args.first().cloned().unwrap_or_default();
        let key = match b.tag.as_str() {
            "CLAP" => b.args.get(1).map(|id| format!("clap:{id}")),
            "VST" if label.starts_with("VST3") => b.args.iter().find_map(|a| {
                let inner = a.split_once('{')?.1.trim_end_matches('}');
                (inner.len() == 32 && inner.chars().all(|c| c.is_ascii_hexdigit()))
                    .then(|| format!("vst3:{}", inner.to_ascii_uppercase()))
            }),
            _ => None,
        };
        let shown = label
            .split_once(": ")
            .map_or(label.as_str(), |(_, n)| n)
            .to_owned();
        match key {
            Some(key) if !label.contains("(Cockos)") => {
                if let Some(id) =
                    routing::add_insert(&mut build.project, channel, &key, BTreeMap::new())
                    && let Some(i) = routing::find_insert_mut(&mut build.project, channel, id)
                {
                    i.bypass = bypass;
                    i.name = Some(shown.clone());
                }
                build.warn(Warning::NoSettings(shown));
            }
            _ => build.warn(Warning::Left(if shown.is_empty() {
                b.tag.clone()
            } else {
                shown
            })),
        }
    }
}

fn item(build: &mut Build, dir: &Path, item: &Block, track: TrackId, tempo: &TempoMap, rate: u32) {
    let samples = |s: f64| (s.max(0.0) * f64::from(rate)).round() as u64;
    let position = item.num("POSITION", 0).unwrap_or(0.0);
    let length = item.num("LENGTH", 0).unwrap_or(0.0);
    if length <= 0.0 {
        return;
    }
    let start = samples(position);
    let len = samples(position + length).saturating_sub(start).max(1);
    let muted = item.num("MUTE", 0).is_some_and(|m| m > 0.0);
    let name = item.line("NAME").and_then(|n| n.first()).cloned();
    let fade = |key: &str| samples(item.num(key, 1).unwrap_or(0.0));
    let Some(source) = item.children.iter().find(|b| b.tag == "SOURCE") else {
        return;
    };
    // Sections and reversed sources wrap the real one.
    let source = match source.args.first().map(String::as_str) {
        Some("SECTION") => source
            .children
            .iter()
            .find(|b| b.tag == "SOURCE")
            .unwrap_or(source),
        _ => source,
    };
    let kind = source.args.first().map(String::as_str).unwrap_or("");
    if kind == "MIDI" {
        let ppq = source
            .num("HASDATA", 1)
            .filter(|p| *p > 0.0)
            .unwrap_or(960.0);
        let start_beat = tempo.seconds_to_beat(position);
        let mut ticks = 0.0;
        let mut open: BTreeMap<(u8, u8), (u64, u8)> = BTreeMap::new();
        let mut notes = Vec::new();
        for l in source
            .lines
            .iter()
            .filter(|l| l.first().is_some_and(|k| k == "E" || k == "e"))
        {
            let Some(delta) = l.get(1).and_then(|d| d.parse::<f64>().ok()) else {
                continue;
            };
            ticks += delta;
            let byte = |i: usize| l.get(i).and_then(|b| u8::from_str_radix(b, 16).ok());
            let (Some(status), Some(key), Some(vel)) = (byte(2), byte(3), byte(4)) else {
                continue;
            };
            let at = tempo
                .beat_to_sample(start_beat + ticks / ppq, rate)
                .saturating_sub(start);
            let channel = status & 0x0F;
            match status & 0xF0 {
                0x90 if vel > 0 => {
                    open.insert((channel, key), (at, vel));
                }
                0x80 | 0x90 => {
                    if let Some((from, velocity)) = open.remove(&(channel, key))
                        && from < len
                    {
                        notes.push(Note {
                            start: from,
                            length: at.min(len).saturating_sub(from).max(1),
                            key: key.min(127),
                            velocity: velocity.min(127),
                            channel,
                            ..Note::default()
                        });
                    }
                }
                _ => {}
            }
        }
        notes.sort_by_key(|n| (n.start, n.key));
        if build
            .project
            .track(track)
            .is_some_and(|t| t.inserts.is_empty())
        {
            routing::add_insert(
                &mut build.project,
                Channel::Track(track),
                "synth",
                BTreeMap::new(),
            );
        }
        if let Some(clip) = build.project.add_midi_clip(track, start, len, notes)
            && let Some(t) = build.project.track_mut(track)
            && let Some(c) = t.clips.iter_mut().find(|c| c.id == clip)
        {
            c.muted = muted;
            if let Some(n) = name {
                c.name = n;
            }
        }
        return;
    }
    let Some(file) = source.line("FILE").and_then(|f| f.first()) else {
        return;
    };
    let path = PathBuf::from(file.replace('\\', "/"));
    let path = if path.is_absolute() {
        path
    } else {
        dir.join(path)
    };
    let Some(src) = build.source(&path) else {
        return;
    };
    let speed = item.num("PLAYRATE", 0).filter(|r| *r > 0.0).unwrap_or(1.0);
    let stretch = if (speed - 1.0).abs() < 1e-6 {
        1.0
    } else {
        1.0 / speed
    };
    let offset = item.num("SOFFS", 0).unwrap_or(0.0).max(0.0);
    let gain = item.num("VOLPAN", 2).map_or(0.0, db);
    let id = build.project.new_clip_id();
    let clip = Clip {
        id,
        source: src,
        name: name.unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned())
        }),
        start,
        offset: (offset * stretch * f64::from(rate)).round() as u64,
        length: len,
        gain_db: gain,
        fade_in: fade("FADEIN").min(len),
        fade_out: fade("FADEOUT").min(len),
        fade_shape: FadeShape::default(),
        fade_out_shape: None,
        muted,
        stretch,
        pitch: 0.0,
        controls: None,
        notes: None,
        variations: Vec::new(),
        variation_names: BTreeMap::new(),
        take: None,
        inserts: Vec::new(),
        automation: Vec::new(),
        edits: None,
    };
    if let Some(t) = build.project.track_mut(track) {
        t.clips.push(clip);
        t.clips.sort_by_key(|c| (c.start, c.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpp_blocks_and_quoted_words_are_read() {
        let root = parse(
            "<REAPER_PROJECT 0.1 \"7.0\" 1\n  TEMPO 100 3 4\n  <TRACK {A}\n    NAME \"Lead vocal\"\n    VOLPAN 0.5 -0.25 -1\n  >\n>\n",
        )
        .unwrap();
        assert_eq!(root.num("TEMPO", 0), Some(100.0));
        let track = root.blocks("TRACK").next().unwrap();
        assert_eq!(track.line("NAME").unwrap()[0], "Lead vocal");
        assert_eq!(track.num("VOLPAN", 1), Some(-0.25));
        assert!(parse("<OTHER\n>").is_none());
    }
}
